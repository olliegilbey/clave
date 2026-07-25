# S2 — the terminal-interaction signal (RC-D): spike plan + implementation spec

_2026-07-22 · workstream **S2**, root cause **RC-D** of
[`2026-07-22-ux-defect-dossier.md`](2026-07-22-ux-defect-dossier.md) · spike-first_

**The requirement, verbatim from the maintainer:**

> "This also extends to terminal tabs, interaction from the user->terminal
> should also bump the terminal tab to the top of the list. Terminal responses
> shouldn't affect if there's a process that's running and completes (that would
> ideally use the same glyphs down the line, but that's something for a future
> feature, can issue separately as low importance)."

Two clauses, and the second is the hard one:

| | Must bump | Must NOT bump |
| --- | --- | --- |
| plain terminal tab | the user starts a command / gives an instruction | a process that was already running completes |
| agent tab | already handled — `UserPromptSubmit` hook (`hook.rs:246,250-253`) | Claude's own tool subprocesses |

Status glyphs for terminal processes are **explicitly out of scope** — proposed
as a separate low-priority issue in §8.

Read the dossier's **RC-D** section first (`2026-07-22-ux-defect-dossier.md:224-297`).
The API survey there is not re-derived here.

> **Naming note.** `docs/superpowers/spikes/S2.md` is an *unrelated, historical*
> spike (the uuid→pane→focus join, PASS 2026-07-03). Do not overwrite it. Record
> this spike's outcome as a new **C-section entry in
> `docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md`**, per AGENTS.md's "record
> what you find in the ledger in the same commit as the change".

---

## 0. Why this is being reopened

The 2026-06-30 design parked terminal-input ordering
(`2026-06-30-clave-orchestrator-design.md:539-540`,
`SUBSYSTEM-VALIDATION.md:260-262`) for two reasons, both still true of the
options it knew about:

1. The only signal it had was `EventType::InputReceived`, which carries **no
   payload** (`zellij-utils-0.44.3/src/data.rs:959-960`) and therefore cannot
   tell a nav keybind from a keystroke in a pane. Round 4 (2026-07-10) turned
   that into one `clave touch` + `zellij pipe` spawn per keystroke and
   **exhausted the zellij server's file descriptors** — `EMFILE`, `ipc.rs:388`
   panic (`SUBSYSTEM-VALIDATION.md:232-243`).
2. The fallback was a shell `preexec` hook, and the maintainer **declined shell
   config** (`SUBSYSTEM-VALIDATION.md:260-262`; policy restated at
   `crates/clave/src/discover.rs:7`).

zellij 0.44.3 ships events that the 2026-06-30 design did not consider:
`CommandChanged`, `CwdChanged`, `UserAction` (`data.rs:1011,1015,1016`). The
dossier flagged their **emit conditions as unverifiable** because `zellij-server`
is not vendored — which is why this workstream was scoped spike-first.

**That constraint has been lifted.** `zellij-server` is fetchable from
crates.io (TESTING.md:376-378 says as much; several C6/C8 findings were confirmed
that way). §1 below is the emit mechanism read directly from
`zellij-server-0.44.3`. It converts most of the spike's open questions into
**falsifiable predictions**, and shrinks the spike from "discover the semantics"
to "confirm the semantics and measure the volume on this machine".

Fetch it yourself before trusting §1 (no repo change, scratch dir only):

```bash
curl -sSL https://static.crates.io/crates/zellij-server/zellij-server-0.44.3.crate \
  -o "$TMPDIR/zs.crate" && tar xzf "$TMPDIR/zs.crate" -C "$TMPDIR"
# → $TMPDIR/zellij-server-0.44.3/src/{pty.rs,route.rs,screen.rs,background_jobs.rs}
```

---

## 1. The emit mechanism, read from source

All line numbers are `zellij-server-0.44.3/src/…` unless prefixed.

### 1.1 `CommandChanged` is a 1 Hz **sampler**, not an event stream

The whole chain, in four hops:

| # | What | Where |
| --- | --- | --- |
| 1 | A background tokio task ticks every **1000 ms** and sends `PtyInstruction::UpdateAndReportCwds` | `background_jobs.rs:114` (`UPDATE_AND_REPORT_CWDS_INTERVAL_MS = 1000`), spawned `:185-195` |
| 2 | The pty thread runs `Pty::update_and_report_cwds()` | `pty.rs:895-897` → `pty.rs:2066` |
| 3 | It selects **only panes that produced output since the last tick**, consuming the flag: `.swap(false, Ordering::Relaxed)`. Empty ⇒ early return, no `ps`, no events | `pty.rs:2069-2083`; flag set on every pty read in `terminal_bytes.rs:62` |
| 4 | One `ps -ao ppid,args` per tick builds `ppid → argv` for *all* processes; per active terminal it looks up `id_to_child_pid[terminal] → argv` | `pty.rs:2131-2136`, `os_input_output.rs:549-592` |

Then, **change-gated per pane** (`pty.rs:2137-2175`):

```rust
if self.terminal_foreground_cmds.get(terminal_id) != Some(&foreground_cmd) {
    let (command, is_foreground) = if foreground_cmd.is_empty() {
        (self.terminal_cmds.get(terminal_id).cloned().unwrap_or_default(), false)
    } else {
        (foreground_cmd.clone(), true)
    };
    …send Event::CommandChanged(pane_id.into(), command, is_foreground, focused_client_ids)
    self.terminal_foreground_cmds.insert(*terminal_id, foreground_cmd);
}
```

Five consequences, each load-bearing:

- **`is_foreground` does not mean what its name suggests.** It is
  `!foreground_cmd.is_empty()` — i.e. *"the pane's direct child process has at
  least one child of its own right now"*. `true` = a command is running under
  the shell; `false` = the transition back to a bare shell. A `sleep 60 &` is
  still a child of the shell, so it too reports `true`. **The discriminator the
  maintainer needs is the `true`/`false` transition, not "foreground vs
  background":** a command *starting* emits `true`; a command *completing* (the
  last child going away) emits `false`. Gating on `is_foreground == true`
  delivers the requirement exactly.
- **Output alone emits nothing.** Output sets the activity flag, which only makes
  the pane *eligible* for sampling; the emit is gated on the argv changing.
  Prompt redraws, `cat` of a big file, a spinner — all silent.
- **`cd` emits nothing** (shell builtin, no child) — it emits `CwdChanged`
  instead (`pty.rs:2098-2121`), which is state-based (compares
  `terminal_cwds`), so it is *reliable* where `CommandChanged` is *sampled*.
- **Sub-second commands are invisible.** `ls`, `git status`, `echo` typically
  start and exit inside one 1000 ms window; the sampler sees `empty → empty` and
  emits nothing at all. **This is the single biggest threat to Branch A** and the
  primary quantitative question for the spike.
- **Rate is bounded by construction**: ≤ 1 event per pane per second, and only on
  transitions. That is a different universe from round 4's per-keystroke ×
  N-instances storm.

### 1.2 `focused_client_ids` is a free "the user is looking at this pane" flag

The 4th field is built from `pty.active_panes` (`pty.rs:2143-2150`), a
`HashMap<ClientId, PaneId>` maintained by `set_active_pane` (`pty.rs:1842-1846`)
from `PtyInstruction::UpdateActivePane` (`pty.rs:475`, sent by
`tab/mod.rs:3276`). Non-empty ⇔ this pane is some client's currently-focused
pane. Free, no extra permission, and it kills the "unattended pane churns"
class (a `make -j` in a tab you left spawns a new child every few hundred ms;
each distinct argv is a change ⇒ an `is_foreground=true` emit ⇒ a false bump —
unless the pane must be focused).

### 1.3 Permission: **already held; nothing to reseed**

`check_event_permission` (`plugins/wasm_bridge.rs:2075-2109`) maps
`Event::CwdChanged | Event::CommandChanged | Event::InputReceived` →
`PermissionType::ReadApplicationState`, which `clave-bar` already requests
(`crates/clave-bar/src/main.rs:360`) and `clave setup` already pre-seeds. Only
`Event::UserAction` needs `PermissionType::InterceptInput` (`:2107`).

This matters because of the hazard recorded at `crates/clave-bar/src/main.rs:352-356`
— *"Changing this list without changing the seed hangs every pipe (this re-bit
S2 — see the ledger)"*. **Subscribing to `CommandChanged`/`CwdChanged` does not
change the permission set, so that hazard does not apply.** Say so in the PR;
a reviewer will ask.

### 1.4 The protobuf round-trip exists in both directions

`zellij-utils-0.44.3/src/plugin_api/event.rs`: encode at `:1031-1063`, decode at
`:463-499`, `EventType` mapping at `:2074-2075` / `:2128-2129`. There is no
"declared but unimplemented" trap here (which *does* bite some newer zellij
events). The event will materialise plugin-side.

### 1.5 Delivery fan-out

`send_to_plugin(PluginInstruction::Update(vec![(None, None, event)]))` —
`None` plugin id, `None` client id (`pty.rs:2156-2166`) ⇒ **broadcast to every
subscribed plugin instance**, unlike `TabUpdate`, which C3 found reaches only
the active tab's instance. So **all N bars hear every `CommandChanged`**. One
emitter must be elected; see §4.1.

### 1.6 `InputReceived`, for completeness (Branch B's substrate)

Two emit sites:

- `route.rs:212-222` — fired for **every non-mouse `Action`**, before the action
  executes, broadcast (`None` plugin id). A keystroke into a terminal pane is
  `Action::Write`/`WriteChars`; a clave keybind is `Action::MessagePlugin`/
  `NewTab`/`CloseTab`/`ToggleTab`. **Indistinguishable at the plugin.** This is
  the round-4 mechanism, confirmed.
- `screen.rs:4876-4890` — mouse effects that are not bare motion, targeted via
  `targeted_plugin_ids(client_id, EventType::InputReceived)`. **So clicks and
  scrolls also fire it** — including a click on a sidebar row.

### 1.7 Agent panes will fire `CommandChanged` on Claude's tool calls

`clave spawn` **`exec`s** into `claude` (`crates/clave/src/main.rs:231,255,258`
— `CommandExt::exec`, "exec only returns on failure"). So the pane's
`id_to_child_pid` **is the `claude` pid**, and `ppid → argv` therefore resolves
to whatever `claude` spawns directly — every Bash-tool `/bin/bash -c …`. An
agent tab would emit `is_foreground=true` on each distinct tool subprocess.

That is agent activity, not user interaction; it races the `UserPromptSubmit`
hook path (`hook.rs:246,250-253`) which already owns agent-tab ordering, and it
would bump an agent tab on the agent's own work. **Agent panes must be excluded
by the pane→uuid join the bar already holds** (`model.rs:176` `uuid_to_pane`,
`model.rs:510-513` `register`). This is a prediction the spike must confirm.

### 1.8 Predictions the spike must falsify

| # | Prediction | Source |
| --- | --- | --- |
| P1 | A foreground command lasting **> ~1.5 s** emits exactly two `CommandChanged` for its pane: `fg=true` at start, `fg=false` at exit | §1.1 |
| P2 | A command lasting **< ~0.5 s** usually emits **nothing** | §1.1 |
| P3 | `sleep 30 &` emits `fg=true` at start and `fg=false` at completion — i.e. a completing background job **does** emit, and is excluded only by the `fg` gate | §1.1 |
| P4 | Pressing Enter at a bare prompt, plain output, and tab switches emit **nothing** | §1.1 |
| P5 | `cd ..` emits `CwdChanged` and **no** `CommandChanged` | §1.1 |
| P6 | Every bar instance receives every `CommandChanged`, regardless of tab | §1.5 |
| P7 | Agent panes emit `fg=true` on Claude tool calls | §1.7 |
| P8 | `focused_client_ids` is non-empty iff the pane is focused | §1.2 |
| P9 | Total volume over 10 min of ordinary use is **tens**, not thousands | §1.1 |

---

## 2. The spike

### 2.1 Objective and stop condition

Confirm or falsify P1–P9 **on the maintainer's machine, in the sandbox**, at zero
risk to the live fleet. Stop as soon as the decision table in §2.7 resolves.

### 2.2 Safety envelope — non-negotiable

- **Sandbox only.** Every step runs against `clave-test` /
  `~/.local/state/clave-dev/` — the `CLAVE_SESSION` / `CLAVE_STATE_DIR` /
  `CLAVE_DATA_DIR` triple (`crates/clave/src/dev.rs:112-118,122-130`). The
  maintainer launches with `clave dev launch` (`dev.rs:135-140`) from a
  **non-zellij terminal**. Nothing in this spike touches the `clave` session.
- **Zero subprocesses per event.** The spike build calls `run_command` **never**.
  It only `eprintln!`s. This is the direct answer to the round-4 fd storm: the
  storm was spawns, not events.
- **Hard event cap.** A counter stops logging after 2000 events, so an
  unanticipated emit path cannot flood the shared zellij log.
- **No `just dev-install`.** That recipe also runs `cargo install --path`
  (`justfile:54`), which writes `~/.cargo/bin/clave` — forbidden while the
  maintainer may be daily-driving (AGENTS.md; #43/#44). Use the explicit
  build + `cp` in §2.4.
- The **only** live mutation the agent performs is the sanctioned sandbox
  hot-reload (TESTING.md:212-217), and only after a liveness gate.

### 2.3 Instrumentation — exact diff sketch

Two files. All of it is temporary and stripped before commit (TESTING.md:333).

**(a) `crates/clave-bar/src/model.rs`** — two pure accessors. These are *not*
throwaway: Branch A needs both, and both are unit-testable on the host.

```rust
// after tab_position_of_pane (model.rs:396-402)

/// Which tab_id hosts this terminal pane, per THIS instance's frames?
/// NOTE: joins the PANE frame (apply_panes) to the TAB frame (apply_tabs)
/// by position — the exact cross-frame join RC-A indicts. Callers must be
/// the active instance. See §4.1.
pub fn tab_of_pane(&self, pane_id: u32) -> Option<usize> {
    let pos = self.tab_position_of_pane(pane_id)?;
    self.tabs.iter().find(|t| t.position == pos).map(|t| t.tab_id)
}

/// Is this pane a registered AGENT pane (clave-register, model.rs:510)?
pub fn is_agent_pane(&self, pane_id: u32) -> bool {
    self.uuid_to_pane.values().any(|p| *p == pane_id)
}
```

**(b) `crates/clave-bar/src/main.rs`** — subscribe + three log-only arms, plus a
counter field.

```rust
 struct State {
     …
     pending_dwells: std::collections::VecDeque<u64>,
+    /// SPIKE ONLY — hard cap so an unknown emit path can't flood the log.
+    dbg_events: u32,
 }
```

```rust
         subscribe(&[
             EventType::TabUpdate,
             EventType::PaneUpdate,
             EventType::Mouse,
             EventType::RunCommandResult,
             EventType::PermissionRequestResult,
             EventType::Timer,
+            // SPIKE ONLY (S2/RC-D). Both map to ReadApplicationState
+            // (wasm_bridge.rs:2098-2103), already granted — no permissions.kdl
+            // reseed, so main.rs:352-356's hazard does not apply.
+            EventType::CommandChanged,
+            EventType::CwdChanged,
+            EventType::InputReceived, // volume comparison ONLY; never spawns
         ]);
```

```rust
+            // ================= SPIKE ONLY — remove before commit =================
+            Event::CommandChanged(pid, cmd, is_foreground, clients) => {
+                self.dbg_events += 1;
+                if self.dbg_events > 2000 { return false; }
+                // Log argv[0] BASENAME and the arg COUNT only — never the full
+                // command line. The zellij log is shared across every session on
+                // the machine and the repo has a pre-commit PII blocklist.
+                let argv0 = cmd
+                    .first()
+                    .map(|s| s.rsplit('/').next().unwrap_or(s.as_str()).to_string())
+                    .unwrap_or_else(|| "-".into());
+                let (kind, raw) = match pid {
+                    PaneId::Terminal(id) => ("term", id),
+                    PaneId::Plugin(id) => ("plug", id),
+                };
+                eprintln!(
+                    "CLAVE_DBG_cc pane={kind}:{raw} fg={is_foreground} argv0={argv0} \
+                     nargs={} focused={} agent={} tab={:?} own_active={} own_tab={:?} n={}",
+                    cmd.len(),
+                    clients.len(),
+                    matches!(pid, PaneId::Terminal(id) if self.model.is_agent_pane(id)),
+                    match pid { PaneId::Terminal(id) => self.model.tab_of_pane(id), _ => None },
+                    self.is_active_instance(),
+                    self.own_tab_id(),
+                    self.dbg_events,
+                );
+                false
+            }
+            Event::CwdChanged(pid, _cwd, clients) => {
+                self.dbg_events += 1;
+                if self.dbg_events > 2000 { return false; }
+                // NEVER log the path itself (PII blocklist + shared log).
+                let (kind, raw) = match pid {
+                    PaneId::Terminal(id) => ("term", id),
+                    PaneId::Plugin(id) => ("plug", id),
+                };
+                eprintln!(
+                    "CLAVE_DBG_cwd pane={kind}:{raw} focused={} tab={:?} own_active={} n={}",
+                    clients.len(),
+                    match pid { PaneId::Terminal(id) => self.model.tab_of_pane(id), _ => None },
+                    self.is_active_instance(),
+                    self.dbg_events,
+                );
+                false
+            }
+            Event::InputReceived => {
+                // Volume comparison for Branch B. NO run_command — the round-4
+                // storm was spawns, not events. Sample 1-in-25 to keep the log sane.
+                self.dbg_events += 1;
+                if self.dbg_events > 2000 { return false; }
+                if self.dbg_events % 25 == 0 {
+                    eprintln!(
+                        "CLAVE_DBG_input n={} own_active={} peeks={}",
+                        self.dbg_events, self.is_active_instance(), self.pending_peeks
+                    );
+                }
+                false
+            }
+            // ==================== end SPIKE ONLY ====================
```

All three arms `return false` — **no repaint, no effect, no spawn**.

**Per-signal counters, not a shared modulo (CodeRabbit, 2026-07-22).** The sketch
above shares one `dbg_events` across all three arms, which biases §2.6's
`CLAVE_DBG_input × 25` extrapolation the moment command/cwd events interleave, and
lets one noisy signal exhaust the joint 2000-event cap before later probes run —
invalidating P1–P9. Replace it with **independent per-signal accounting**: three
counters `dbg_cmd` / `dbg_cwd` / `dbg_input` for exact per-signal accounting, so
`CLAVE_DBG_input=n` is an exact count of `InputReceived` and §2.6 reads exact
counts rather than an extrapolation.

**The hard cap stays global, at 2000 (CodeRabbit, 2026-07-22 — reconciling with
§2.2).** To keep the per-signal counters from turning §2.2's single 2000-event
safety envelope into three independent caps (6000 worst case), the **hard stop
is a global `dbg_total`** — the first arm to push it past 2000 flips every arm
to a plain `return false` with no logging. The per-signal counters are
**sub-budgets for accounting only**, not independent safety limits. So the
envelope §2.2 promises is unchanged (one 2000-event ceiling across all signals),
and the per-signal counts are still exact up to that ceiling. If the analysis
needs a full 2000 samples of a *specific* signal, run the spike with the other
two arms compiled out — stated as the isolation procedure, not a larger cap. The
arms otherwise stay identical — `return false`, no spawn.

### 2.4 Build and load into the sandbox

Agent-runnable. Liveness gate first — a `zellij action` at a dead session
**blocks forever without erroring** (TESTING.md:231-236).

```bash
# 0. liveness gate (read-only, safe anywhere)
zellij list-sessions | grep -q clave-test || { echo "clave-test not live — ask the maintainer to run: clave dev launch"; exit 1; }

# 1. tag the build so the log says WHICH wasm produced the trace
TAG="s2spike-$(date +%m%d-%H%M%S)"
CLAVE_BUILD_TAG="$TAG" cargo build -p clave-bar --release --target wasm32-wasip1

# 2. SANDBOX data dir only — never ~/.local/share/clave/ (release surface)
cp target/wasm32-wasip1/release/clave-bar.wasm "$HOME/.local/state/clave-dev/data/clave-bar.wasm"

# 3. the ONE sanctioned live mutation (TESTING.md:212-217)
ZELLIJ_SESSION_NAME=clave-test zellij action start-or-reload-plugin \
  "file:$HOME/.local/state/clave-dev/data/clave-bar.wasm"

echo "build tag: $TAG"
```

Hot-reload reincarnates every bar model from scratch (TESTING.md:335-338) — for
this spike that is *desirable*: it gives a clean `clave-bar: loaded v… build=$TAG`
line to anchor the analysis window (`main.rs:347-351`).

### 2.5 The exercise script (the maintainer drives; the agent prints)

In the `clave-test` session. Suggested seed: `clave dev scenario c8-cold-start`
before launch, so there is one agent tab plus plain tabs.

| # | Do | Probes |
| --- | --- | --- |
| E1 | Do nothing for 60 s | P9 idle baseline; expect **zero** lines |
| E2 | In a plain tab, press Enter at a bare prompt ×5 | P4 |
| E3 | Type `ls` + Enter, ×3 | P2 — the make-or-break case |
| E4 | `sleep 3` + Enter | P1 |
| E5 | `sleep 30 &` + Enter, wait for the completion notice | P3 |
| E6 | `cd ..` then `cd -` | P5 |
| E7 | `cat` a large file (a few seconds of scroll) | P4 — output alone |
| E8 | `Alt+j` / `Alt+k` ×5, `Alt+o` ×2, `Alt+1`..`Alt+3` | P4, nav noise; `CLAVE_DBG_input` volume |
| E9 | Click three sidebar rows | mouse `InputReceived` (§1.6) |
| E10 | `sleep 20` in tab A, switch to tab B while it runs, wait for it to finish | P8 + the "unattended completion" case |
| E11 | In the agent tab, send a prompt that makes Claude run 2–3 Bash tool calls | **P7 — decisive for the agent-pane exclusion** |
| E12 | Open `vim`, edit, `:q` | long foreground child, start/exit pair |
| E13 | `ssh localhost` inside a pane, type a few commands, `exit` | the SSH-in-pane case (§5) |

### 2.6 Analysis commands

```bash
LOG="$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log"   # macOS; shared by EVERY session
TAG="s2spike-…"                                        # from §2.4
OUT="$TMPDIR/s2-spike-window.txt"

# Window the log to THIS run: everything after the last matching load line.
awk -v tag="$TAG" '
  index($0, "clave-bar: loaded") && index($0, tag) { n = NR }
  { l[NR] = $0 }
  END { for (i = n; i <= NR; i++) print l[i] }
' "$LOG" > "$OUT"

# Totals
grep -c CLAVE_DBG_cc    "$OUT"
grep -c CLAVE_DBG_cwd   "$OUT"
grep -c CLAVE_DBG_input "$OUT"   # ×25 ≈ true InputReceived volume

# P1/P3: start/exit pairing per pane
grep -o 'CLAVE_DBG_cc pane=[^ ]* fg=[a-z]*' "$OUT" | sort | uniq -c | sort -rn

# P2: did `ls` ever appear?
grep 'CLAVE_DBG_cc' "$OUT" | grep -E 'argv0=(ls|git|echo)'

# P6: how many distinct plugin instances logged the same event
grep 'CLAVE_DBG_cc' "$OUT" | grep -oE '\[id: [0-9]+' | sort | uniq -c

# P7: agent panes
grep 'CLAVE_DBG_cc' "$OUT" | grep 'agent=true'

# P8: focus correlation
grep -o 'CLAVE_DBG_cc .*focused=[0-9]*' "$OUT" | grep -oE 'fg=[a-z]+ .*focused=[0-9]+' | sort | uniq -c

# Join resolution health (empty `tab=None` ⇒ the pane→tab join failed)
grep -c 'tab=None' "$OUT"
```

The log line format is `|<plugin name>| <ts> [id: N] <message>`
(`zellij-server-0.44.3/src/logging_pipe.rs:27-35`), so `[id: N]` separates bar
instances and the timestamp is `%Y-%m-%d %H:%M:%S.%3f`. The file is shared across
every session on the machine and old entries linger (TESTING.md:294-300) — the
`awk` window above is mandatory, not optional.

### 2.7 Decision table

| Observation | Verdict | Go to |
| --- | --- | --- |
| E3 (`ls`) produces `fg=true` lines ≥ 2 of 3 times, **and** E5's completion is `fg=false`, **and** E1/E2/E4-output/E8 are silent | `CommandChanged` is sufficient on its own | **Branch A** (§4) |
| E3 silent but E4/E12 (`sleep 3`, `vim`) reliably paired, E5 completion `fg=false`, volume in the tens | Works, but **only for commands ≳ 1 s**. Real but partial coverage | **Branch A′** (§4.6) — ship A, and escalate the `ls` gap to the maintainer as a UX decision |
| E5's completion emits `fg=true` (i.e. `is_foreground` does not discriminate) | The second clause of the requirement cannot be met by this event | **Branch B** (§6) |
| E1 or E7 produce lines (output alone emits), or volume is in the hundreds | Sampler assumption wrong — treat as a storm risk | **Branch B** (§6), and record the refutation in the ledger |
| `tab=None` on most lines | The pane→tab join is broken in this frame | **block on S0** (§9), re-run the spike after S0 lands |
| E11 shows `agent=true` lines | Expected (P7). Not a blocker — it is the exclusion conjunct in §4.2 | continue |
| `CLAVE_DBG_input` × 25 is in the thousands for E8 alone | Confirms Branch B needs the latch + cooldown of §6.2, not a bare subscription | informs B |
| `CommandChanged` never appears at all, in any step | Either subscription or delivery is broken. Check the load line carries `$TAG`; check `clave --version` vs the bar version (#44) | re-run; if still silent, **Branch B** |

**`UserAction` is not on this table.** It requires `PermissionType::InterceptInput`
(`wasm_bridge.rs:2107`), which clave does not hold, and adding it changes the
permission set — the exact change that hangs every pipe unless `permissions.kdl`
is reseeded in lockstep (`clave-bar/src/main.rs:352-356`). It is a last resort
that needs a maintainer decision of its own; do not reach for it in this spike.

---

## 3. Shared design: what a "touch" costs, and the change-gating question

Both branches end at the same place: `clave touch <tab_id>`
(`crates/clave/src/main.rs:93-101,301-307` → `store::apply_touch`,
`store.rs:213-220`).

```rust
pub fn apply_touch(paths: &StorePaths, tab_id: usize, now: u64) -> Result<AgentSnapshot> {
    with_store_mut(paths, |s| {
        let e = s.tab_timeline.entry(tab_id).or_insert(0);
        *e = (*e).max(now);
        s.seq += 1;                 // ← unconditional
        snapshot_from(s)            // ← unconditional push
    })
}
```

**Decision: change-gate it.** Return `Result<Option<AgentSnapshot>>` and push
only when the stored value actually moved:

```rust
pub fn apply_touch(paths: &StorePaths, tab_id: usize, now: u64) -> Result<Option<AgentSnapshot>> {
    with_store_mut(paths, |s| {
        let e = s.tab_timeline.entry(tab_id).or_insert(0);
        if *e >= now {
            return None;            // §5: no no-op pushes (cf. apply_bind :247-249,
        }                           // apply_prune_tabs :291-294)
        *e = now;
        s.seq += 1;
        Some(snapshot_from(s))
    })
}
```

Why:

1. **Today it is a once-ever event.** `needs_birth_touch` (`model.rs:383-385`) is
   "once ever per (instance, tab), never re-armed", so an unconditional push
   costs one broadcast per tab per session. Under a per-command signal that
   changes character: same-second repeats become **routine**, and every one of
   them is a `seq` bump plus a full-snapshot pipe broadcast to every bar plus a
   repaint of every bar, for a timeline that did not move.
2. **§5's "no no-op pushes" rule already governs the other writers.** `apply_bind`
   and `apply_prune_tabs` both return `None` on no-change and both are cited in
   the store's own comments as doing so deliberately. `apply_touch` is the
   outlier, and it is the outlier only because its caller fired once.
3. **Nothing depends on the unconditional push.** The birth touch's purpose is to
   *distribute a new order*; if the value did not move, there is no new order to
   distribute. A suppressed push is a push of identical content.
4. **Interaction with S1.** If S1 (RC-C) moves `tab_timeline` to sub-second
   resolution, every touch moves the value and this gate becomes a permanent
   no-op — still correct, just inert. The bar-side **already-at-top suppressor**
   (§4.3) then becomes the real limiter. Layer both; neither depends on the
   other.
5. Call-site change is two lines at `crates/clave/src/main.rs:301-307`, matching
   the `apply_bind` arm three lines below it. `clave touch`'s **CLI surface does
   not change**, so no new `try_parse_from` pin is required — but note that no
   pin for `touch` exists today (only `collapse` and `prune-tabs`,
   `crates/clave/src/main.rs:367,382`), and the taxonomy wants one for every
   plugin-invoked subcommand. Add `Cli::try_parse_from(["clave","touch","7"])`
   while you are here.

---

## 4. Branch A — `CommandChanged` works

### 4.1 Who emits, and why it is safer than the bind path

`CommandChanged` is broadcast to every instance (§1.5). Elect exactly one, using
the established pattern (`run_effects`'s `active` gate, `main.rs:88`), **plus**
a "the pane is in *my* tab" conjunct:

```text
touch iff  frames_coherent()                         // S0's witness — REQUIRED first
      AND  is_active_instance()
      AND  model.tab_of_pane(pane_id) == own_tab_id()
```

Three conjuncts, and the first is load-bearing. Rationale:

- **`frames_coherent()` gates first (CodeRabbit, 2026-07-22).** `tab_of_pane`
  and `own_tab_id` are both **positional joins** — the RC-A defect. Under an
  incoherent frame (a create/close renumbering in flight, or S0 Residual 2's
  coherent-but-permuted set) both conjuncts below can pass while the pane
  actually belongs to a *different* tab, so a touch could stamp the wrong tab's
  ordinal. Requiring S0's `frames_coherent()` witness first removes the
  renumbering-in-progress window; the residual coherent-but-permuted case is
  S0 Residual 2's, inherited here — and it is far weaker on this path than on
  the bind path (below).
- The active-instance gate alone is the RC-A-indicted gate — a stale
  `plugin_panes` frame lets a non-active instance believe it is active
  (`dossier RC-A`, `SUBSYSTEM-VALIDATION.md:656-659`).
- Adding "the pane resolves to *my own* tab" makes a surviving mis-election
  **low-harm, not harmless**: worst case is a touch on the wrong tab, which only
  *reorders* a row and is corrected by the next legitimate touch — because
  `apply_touch` is an idempotent max-merge that **never evicts**. Compare
  `Effect::Bind`, where a mis-election actively evicts the rightful tenant
  (`store.rs:239-245`) and is sticky. This path degrades to a transient,
  self-correcting reorder, not corruption. (The earlier draft called it
  "harmless"; with the positional join that overclaimed — it is bounded and
  transient, which is the honest statement.)
- The restriction costs nothing real: a user-started command happens in the
  focused pane, which is in the active tab, by definition, and a coherent frame
  is the steady state the user is typing in.

**Dependency on S0, stated:** this election reuses S0's `frames_coherent()`, so
S2's terminal-touch path **must land after S0** (or carry a local copy of the
witness if it lands first, flagged for de-duplication when S0 merges).

### 4.2 The gate, as a pure model function

New in `crates/clave-bar/src/model.rs`, unit-testable, zero zellij types:

```rust
/// zellij 0.44.3 `CommandChanged` (pty.rs:2137-2175) sampled at 1 Hz.
/// `is_foreground` means "the pane's child process now has a child of its
/// own" — so `true` is a command STARTING and `false` is the last child
/// GOING AWAY. The maintainer's rule ("a running process completing must
/// not bump") is therefore exactly `is_foreground == true`.
pub fn command_started(
    &mut self,
    pane_id: u32,
    is_foreground: bool,
    pane_focused: bool,
    own_tab: usize,
) -> Vec<Effect> {
    if !is_foreground {
        return vec![];              // (1) a command COMPLETING — the excluded case
    }
    if !pane_focused {
        return vec![];              // (2) unattended pane churn (a `make -j` in a
    }                               //     tab you left spawns a child per file)
    if self.is_agent_pane(pane_id) {
        return vec![];              // (3) Claude's tool subprocesses — the hook
    }                               //     (hook.rs:246-253) owns agent ordering
    if self.tab_of_pane(pane_id) != Some(own_tab) {
        return vec![];              // (4) see §4.1 — mis-election degrades to silence
    }
    if self.is_top_of_timeline(own_tab) {
        return vec![];              // (5) already row 0 — see §4.3
    }
    vec![Effect::Touch { tab_id: own_tab }]
}
```

Adapter arm in `crates/clave-bar/src/main.rs` (`update`), thin by house rule
(`main.rs:1-4`):

```rust
Event::CommandChanged(PaneId::Terminal(pane_id), _cmd, is_foreground, clients) => {
    if self.is_active_instance()
        && let Some(own) = self.own_tab_id()
    {
        let fx = self.model.command_started(
            pane_id, is_foreground, !clients.is_empty(), own,
        );
        self.run_effects(fx);
    }
    false   // no repaint: the store push repaints every instance
}
```

`_cmd` is deliberately unused: clave must never read, log, or store the user's
command line.

### 4.3 The already-at-top suppressor (this is the debounce)

```rust
fn is_top_of_timeline(&self, tab_id: usize) -> bool {
    let mine = self.timeline.get(&tab_id).copied().unwrap_or(0);
    mine > 0 && self.timeline.values().copied().max().is_none_or(|m| mine >= m)
}
```

**This is a spawn-reduction optimisation, and it must never suppress the
authoritative write on a stale mirror (CodeRabbit, 2026-07-22).** `self.timeline`
is a local copy that a lost snapshot push can leave stale — it could say `own_tab`
is already top while the store has a newer tab, and then this predicate would
drop the touch and it would *never* reach the store's idempotent max-merge. That
is fail-**closed**, contradicting §7.3's "fails open." The rule: this suppressor
may skip a touch **only** when the local mirror is known-fresh — i.e. gate it
behind the same seq fence S0/S1 use (`self.seq` equal to the last snapshot's
`seq`, no push observed lost). When freshness is not established, **do not
suppress — emit the touch** and let `apply_touch`'s max-merge absorb the
redundant write (it is idempotent and change-gated store-side under S2's decision
to make `apply_touch` return `Option`). The optimisation's whole value is cutting
the `make`/`cargo build` burst, and that burst is on a tab the user is actively
in, where the mirror *is* fresh — so the fence costs nothing in the case it
matters and preserves correctness in the case it doesn't.

Touching a tab that is already row 0 changes nothing observable. This one
predicate collapses the worst realistic burst — a foreground `make`/`cargo build`
you are watching, which spawns a distinct child every tick — from one touch per
second to exactly one touch, because the snapshot echo arrives within ~100 ms
(`SUBSYSTEM-VALIDATION.md`, round 6) and arms the suppressor. **No timer, no
clock, no new `classify_timer` branch.** The wasm plugin has no trustworthy clock
(`crates/clave/src/main.rs:93-97`), which is precisely why a time-based debounce
is the wrong shape here.

Residual: between the touch and its echo (~100 ms) a second start double-touches.
`apply_touch`'s max-merge absorbs it, and §3's change-gate suppresses the second
push. Self-healing if a push is lost: the suppressor simply does not arm and the
next start re-touches.

### 4.4 Rate bound (the safety argument the fd-storm precedent demands)

Derived, not measured:

```text
events/sec/pane   ≤ 1                         (1 Hz sampler, pty.rs / background_jobs.rs:114)
panes that pass conjunct (2) ≤ 1 per client   (focused_client_ids, pty.rs:2143-2150)
instances that pass (4)      = 1              (own-tab election, §4.1)
touches/sec       ≤ 1, and → 0 once the tab reaches row 0   (§4.3)
```

**Upper bound: one `clave touch` per second, transiently, from one instance.**
Round 4 was *unbounded × per keystroke × N instances*, plus an echo-gated
birth-touch loop that re-fired on every `TabUpdate`
(`SUBSYSTEM-VALIDATION.md:232-243`). This is three orders of magnitude away from
that, and the guard is structural rather than disciplinary.

### 4.5 `CwdChanged` as a companion (optional, cheap)

`cd` is a builtin and emits no `CommandChanged` (P5), but `CwdChanged`
(`pty.rs:2098-2121`) is **state-compare, not transient-sample**, so it is
reliable where `CommandChanged` is lossy. Same permission, same broadcast, same
gates:

```rust
Event::CwdChanged(PaneId::Terminal(pane_id), _cwd, clients) => { /* identical gate,
    minus the is_foreground conjunct */ }
```

Include it iff the spike confirms P5 and the volume stays in the tens. Never log
or store the path. Note the emitting process is the pane's **direct child** (the
shell), so this fires for the user's `cd` and not for a build tool chdir-ing.

### 4.6 Branch A′ — `CommandChanged` fires only for commands ≳ 1 s

The likely outcome per P2. Ship Branch A exactly as specified — it is strictly
better than today (today plain tabs order by **birth only**,
`SUBSYSTEM-VALIDATION.md:260-262`) — and be explicit in the PR that:

- `sleep 3`, `vim`, `cargo build`, `ssh`, `less`, `psql` bump the tab;
- `ls`, `git status`, `echo`, and any sub-second command do **not**;
- `cd` bumps iff §4.5 is included.

Then escalate one question to the maintainer, with the numbers from the spike:

> Plain-tab bumping covers long-running commands but not `ls`-class commands. The
> only signals that cover those are (i) `InputReceived` — every keystroke,
> undiscriminating, latched and cooled down (§6), the same event that caused the
> round-4 fd storm; or (ii) a shell `preexec` hook, which you declined in July.
> A is safe and partial. Do you want partial, or do you want to revisit (i)/(ii)?

Do **not** pre-empt that decision. A and B(i) are not exclusive — A can ship
first and B(i) layer on top, because both funnel into the same
`Effect::Touch`/`clave touch` sink.

### 4.7 Effect plumbing

```rust
// model.rs, alongside Bind (model.rs:~200)
/// run_command(["clave","touch",tab_id]) — stamp a user commitment on the
/// STORE's tab timeline. Executor-gated in run_effects, like Bind/MarkRead.
Touch { tab_id: usize },
```

```rust
// main.rs run_effects, alongside Effect::Bind (main.rs:145-153)
Effect::Touch { tab_id } if active => {
    run_command(&["clave", "touch", &tab_id.to_string()], BTreeMap::new());
}
```

**Leave the birth touch at `main.rs:434-445` alone.** It is a direct
`run_command`, not an `Effect`. Migrating it to `Effect::Touch` is a tidy-up that
collides head-on with S0, which edits `main.rs:434-467`. Do it in a follow-up if
at all; note the duplication in the PR so a reviewer does not flag it as an
oversight.

---

## 5. SSH implications (a hard project constraint)

clave must eventually work when the CLI and terminal are on a remote host.
Two distinct scenarios; keep them apart.

| Scenario | Branch A / A′ | Branch B(i) `InputReceived` | Branch B(ii) `preexec` |
| --- | --- | --- | --- |
| **User SSHes to a host and attaches to a zellij session there** | ✅ by construction — the pty poll, `ps`, the plugin, `run_command` children and the store **all** live where the zellij *server* lives (dossier RC-D:288-290) | ✅ same reason | ⚠️ works, but clave must write the *remote* user's rc file — a new install-surface obligation |
| **User runs `ssh other-host` *inside* a pane** | ⚠️ **degrades**: `ssh` is a direct child of the local shell ⇒ one `fg=true` at connect, one `fg=false` at disconnect. The tab bumps when you connect and then never again, however long you work on the remote | ✅ **holds** — keystrokes still route through the local server's input path (`route.rs:212-222`) regardless of where the bytes end up | ❌ **breaks entirely** — `ZELLIJ_*` are ordinary env vars ssh does not forward; the remote host has no zellij socket, no store, no `clave` (dossier RC-D:291-293) |
| **Latency** | none added — the poll is server-local | none | ⚠️ `zellij action list-panes -t` at ~40 ms per command, on the interactive path, per the dossier's measurements |

**Ranking for the SSH constraint: B(i) > A > B(ii).** Branch A's in-pane-ssh
degradation is a *known, bounded* limitation to state in the PR, not a blocker —
and note that today the behaviour is "never bumps at all", so even the degraded
case is an improvement.

---

## 6. Branch B — `CommandChanged` is unusable

Reached when the spike shows `is_foreground` does not discriminate completion,
or the volume/emit profile refutes §1.

### 6.1 B(i) — `InputReceived` with a nav-suppression latch and a cooldown

The event carries nothing (`data.rs:959-960`); its only usable content is
*"input happened somewhere, now"*. The design is therefore entirely about
**suppression** and **rate**.

**State** (model-side, so it is testable):

```rust
/// Suppress the next InputReceived after a nav/click/toggle signal — the
/// keystroke that IS a clave keybind must not touch the tab it navigates to.
/// Ordering between a MessagePlugin pipe and InputReceived is NOT guaranteed
/// (different server paths: route.rs:212 vs the plugin pipe route), so this is
/// a countdown, not a boolean edge.
nav_suppress: u8,
/// Cooldown in flight — at most one `clave touch` per TOUCH_COOLDOWN_SECS.
touch_cooling: bool,
```

**Gate:**

```text
touch own_tab iff  is_active_instance()                 (S0 — hard dependency)
              AND  own_tab_id().is_some()
              AND  nav_suppress == 0                    (else decrement and drop)
              AND  pending_peeks == 0                   (a nav/click peek is in flight)
              AND  !touch_cooling
then: emit Effect::Touch{own_tab} + Effect::ArmTouchCooldown
```

**Latch arming points** — every clave keybind that produces an `Action` and
therefore an `InputReceived` (`crates/clave/src/setup.rs:81,84,94,95,101,105,114,118`):

| Bind | Reaches the bar as | Latch |
| --- | --- | --- |
| `Alt j/k`, `Alt ↑/↓`, `Alt 1..9` | `clave-nav` pipe | `nav_suppress = 2` in the `clave-nav` arm (`main.rs:315-328`) |
| `Alt o` | `ToggleTab` + `clave-organic` pipe | `nav_suppress = 2` in `set_organic_pending` |
| `Alt c` | `clave-toggle` pipe | `nav_suppress = 2` in `toggle_collapsed` |
| `Alt t` / `Alt w` | `NewTab` / `CloseTab`, **no pipe** | ✗ unlatchable — see residuals |
| `Alt a` | `Run clave add` (floating pane) | ✗ unlatchable |
| mouse click on a row | `Mouse::LeftClick` → `AnnounceVisit` → `clave-visited` pipe | `pending_peeks > 0` covers it (`main.rs:287-304`) |

`pending_peeks` is doing double duty here and is the neatest part of the design:
every nav and every click already arms a ~1 s peek (`main.rs:295-297`,
`model.rs` `visited()`), so "a peek is in flight" is already a precise
"the user is navigating, not typing" signal. Reuse it; do not invent a parallel
notion.

**Cooldown.** `set_timeout` is the only clock the plugin has. A third timer class
means extending `classify_timer` (`model.rs:122-131`), which today splits 0.4 s
dwells from 0.9 s peeks at a single cutoff and carries a hard-won
reclassification rule for late dwells. Extending it to three classes is the
riskiest part of B(i):

- pick `TOUCH_COOLDOWN_SECS = 2.0` — far from both existing durations;
- cutoffs at 0.65 and 1.5;
- carry the same "a late X is still an X when nothing else is pending" rule for
  each class, or the FIFO latches off-by-one for the life of the instance
  (the failure the existing comment at `main.rs:470-476` documents);
- extend the proptests: a generated sequence of dwell/peek/cooldown arms and
  expiries must never mis-pop a dwell generation.

**Rate bound:** ≤ 1 `clave touch` per 2 s, from one instance. Round 4 was one
`clave touch` **and** one `zellij pipe` per keystroke, from N instances, with a
birth-touch loop re-firing on every `TabUpdate`. State this comparison
explicitly in the PR dossier — it is the argument that this is not a re-run of
round 4.

**Residual defects, stated honestly (all belong in the PR, not discovered by a
reviewer):**

| # | Residual | Severity |
| --- | --- | --- |
| R1 | `Alt+t` / `Alt+a` produce an unlatchable `InputReceived` ⇒ a spurious bump of the tab you were on. `Alt+w` also does, but the tab is closing and the entry is pruned | low; `Alt+t`'s new tab birth-touches to the top anyway |
| R2 | Scroll wheel in a pane fires `InputReceived` (`screen.rs:4876-4890`) ⇒ scrolling back through output bumps the tab. Arguably wrong — scrolling is reading, not instructing | medium |
| R3 | A click on a sidebar row is latched by `pending_peeks`, but the pipe/event ordering is not guaranteed ⇒ a rare focus-driven reorder, violating the ratified "focus never reorders" rule (`model.rs:327-341`, `setup.rs:96-99`) | medium — this is the invariant the maintainer chose deliberately |
| R4 | Multi-client: another client's typing is indistinguishable from the maintainer's | low today, structural |
| R5 | Depends on `is_active_instance()` being correct — i.e. **hard-blocked on S0** | high until S0 lands |

Branch A has none of R1–R4, because it carries a pane identity.

### 6.2 B(ii) — a shell `preexec` hook: escalate, do not assume

This is a **maintainer decision, not an engineering choice**, and the standing
ruling is *declined* (`SUBSYSTEM-VALIDATION.md:260-262`; policy at
`crates/clave/src/discover.rs:7` — *"the user's shell config is their business;
clave just works"*). Present it with the trade-offs and stop.

Escalation text to hand him:

> `preexec`/`precmd` (zsh) or `DEBUG` trap (bash) would give an exact,
> zero-ambiguity "the user just ran a command" signal — better than anything the
> plugin API offers. It costs:
>
> - **Shell config.** clave would write to your rc file, or ask you to source a
>   snippet. You declined this on 2026-07-14 and the "clave just works" policy is
>   written into `discover.rs:7`.
> - **A pane→tab resolution per command.** `ZELLIJ_TAB_ID` does not exist — a
>   pane knows only `ZELLIJ`, `ZELLIJ_PANE_ID`, `ZELLIJ_SESSION_NAME` (dossier
>   RC-D:264-267). `zellij action list-panes -t` resolves it at **~40 ms**, on the
>   interactive path, before every command you run. (A cheaper route exists —
>   pipe `$ZELLIJ_PANE_ID` to the bar and let it do the join it already does for
>   `clave-register`, `spawn.rs:55-80` — but that is a second fire-and-forget
>   delta channel, and per-instance deltas are exactly what diverged in C5
>   round 5.)
> - **It breaks for `ssh other-host` inside a pane** — `ZELLIJ_*` are not
>   forwarded and the remote host has no socket, no store, no `clave`. Given
>   "clave must work over SSH", this is the option that ages worst.
> - **It is the only option that catches `ls`.**
>
> Recommendation: no, unless Branch A′'s coverage gap turns out to matter in
> daily use. It is cheap to revisit later; it is expensive to un-ship a change to
> someone's rc file.

---

## 7. Test plan

**Change class (TESTING.md risk taxonomy): Cross-process / IPC** — a new producer
of `clave touch` subprocesses and a new store-write trigger. Required: *"written
argument for ordering/idempotency in the PR dossier; adversarial reviewer must
attack it; tier-2 coverage once #47 lands."* Tier 2 does not exist, so **the
written argument plus the adversarial reviewer are the verification**. Also add
the `needs-live-validation` label — the signal itself is only observable live.

### 7.1 Tier 1 (hermetic) — the gate

`cargo test --workspace` (load-bearing `--workspace`), `cargo build -p clave-bar
--target wasm32-wasip1`, `cargo clippy --workspace --all-targets -- -D warnings`.

New tests, red-first:

**`crates/clave-bar/src/model.rs`** (host-tested, no zellij types):

| Test | Asserts |
| --- | --- |
| `command_started_emits_touch_for_focused_plain_pane_in_own_tab` | the happy path emits exactly `[Effect::Touch{own}]` |
| `command_started_ignores_completion` | `is_foreground=false` ⇒ `[]` — **the maintainer's second clause, pinned** |
| `command_started_ignores_unfocused_pane` | `pane_focused=false` ⇒ `[]` |
| `command_started_ignores_agent_panes` | after `register(uuid, pane)`, that pane ⇒ `[]` |
| `command_started_ignores_pane_in_another_tab` | pane resolving to a different tab ⇒ `[]` (the §4.1 mis-election guard) |
| `command_started_suppressed_when_tab_already_top` | timeline max is `own_tab` ⇒ `[]` |
| `command_started_reemits_after_another_tab_overtakes` | suppressor re-arms, i.e. it is not a once-ever latch |
| `tab_of_pane_resolves_through_position_join` | + a **stale-frame** case: panes frame from before a close, tabs frame after ⇒ documents the RC-A exposure rather than pretending it away |
| `is_agent_pane_true_only_for_registered_panes` | |
| **proptest** `touch_never_emitted_for_completion_or_agent_panes` | over generated `(pane, fg, focused, tab)` sequences, no `Effect::Touch` ever follows `fg=false` or an agent pane. A new reachable branch demands a new property (TESTING.md:122-127) |

**`crates/clave/src/store.rs`:**

| Test | Asserts |
| --- | --- |
| `apply_touch_returns_none_when_stamp_does_not_move` | the §3 change-gate |
| `apply_touch_still_max_merges_and_bumps_seq_on_advance` | regression on the existing contract |
| `apply_touch_then_prune_leaves_no_entry_for_a_dead_tab` | pins the §7.3 ordering hazard's *terminal* state |

**`crates/clave/src/main.rs`:** `Cli::try_parse_from(["clave","touch","7"])`
parse pin (§3.5) — the class of gap that produced the `ArgAction` escape.

If **Branch B(i)**: add `classify_timer` three-way tests mirroring
`classify_timer_splits_by_elapsed_and_reclassifies_late_dwells`
(`model.rs:2394-2406`), plus latch tests (`nav_suppress` decrements and drops;
`pending_peeks > 0` suppresses; cooldown blocks a second touch).

### 7.2 Tier 2 — does not exist (#47, blocked on #44)

Name the scenario now so it is written down for whoever builds the harness:
*spawn a `sleep 5` in a plain pane of an isolated `clave-it-<pid>` session,
assert `tab_timeline[<tab>]` advances within 2 s; spawn a backgrounded `sleep 1`,
assert it does **not** advance on completion.* That is a clean, `claude`-free,
auth-free scenario — exactly the shape TESTING.md:56-60 wants.

### 7.3 The written ordering / idempotency argument (for the PR dossier)

*This is required output, not optional prose. Copy it into the PR.*

**The only new cross-process artifact is `clave touch <tab_id>`** — an existing
subcommand, unchanged in surface, executed fire-and-forget via `run_command`. It
performs a locked read-modify-write under an exclusive `flock` held across
read→mutate→write (`store.rs:135-164`), so no interleaving can tear the store.

1. **Idempotent.** `apply_touch` is a max-merge (`store.rs:216-217`). Re-delivery
   of the same touch is a no-op, and with §3's change-gate it also stops
   producing a push.
2. **Commutative.** Two touches of the same tab commute (max). Touches of
   different tabs commute (disjoint `BTreeMap` keys). Touch vs `bind` commute
   (disjoint fields: `tab_timeline` vs `agents[*].tab_id`).
3. **Touch vs `prune-tabs` does *not* commute** — and this is the one genuinely
   new exposure. Today the only touch producer is the once-ever birth touch, so
   the window is a tab's first `TabUpdate`. Under S2 a touch can be in flight
   when its tab closes (`Alt+w` while a command runs), and a late touch
   re-creates a `tab_timeline` entry for a dead tab id.
   **Bounded and self-healing:** stale detection is *detection-driven, not
   set-change-gated* (`model.rs:694-712`; the Codex finding behind PR #26), so
   it is re-derived on **every** `TabUpdate` until the store's echo clears the
   mirror — the resurrected entry is re-pruned on the next tab event.
   **Residual, unchanged in kind:** if the id is recycled (zellij's
   `get_new_tab_id` = max-key+1 ⇒ closing the highest id recycles it,
   `store.rs:232-238,262-263`) inside that window, the new tab inherits a
   top-of-list key for ≤ 1 `TabUpdate`. That is the RC-E recycled-id class,
   **owned by S3**, whose window this change widens from "birth" to "birth or a
   command start". Say so; do not claim it is eliminated.
4. **Duplicate delivery across instances is structurally impossible to act on.**
   `CommandChanged` is broadcast to all N bars (`pty.rs:2156-2166`), but the
   conjunction of `is_active_instance()` **and** `tab_of_pane(pane) == own_tab_id()`
   (§4.1) admits at most one. Crucially, a *wrong* election fails the second
   conjunct and emits nothing — unlike `Effect::Bind`, where a wrong election
   evicts a live tenant (`store.rs:239-245`).
5. **Lost pushes** are pre-existing: `push_snapshot` is fire-and-forget, and a
   lost push leaves instances on a stale timeline until the next one. The
   change-gate cannot worsen this — the pushes it suppresses carry byte-identical
   content. The bar's already-at-top suppressor fails *open* (no arm ⇒ re-touch),
   so a lost push costs one extra subprocess, never a missed bump.
6. **No new file descriptors and no new IPC channel.** No pipe is added; the
   snapshot broadcast is the existing one. The spawn rate is bounded at ≤ 1/s
   (Branch A, §4.4) or ≤ 0.5/s (Branch B(i)) from a single instance, derived
   from the 1 Hz sampler in `background_jobs.rs:114` and the gates above — versus
   round 4's unbounded per-keystroke × N-instance storm that reached `EMFILE`.

**Adversarial reviewer brief** (hand this to the attacking lane verbatim):
*attack the ordering of `clave touch` against `clave prune-tabs` around
`Alt+w` on the highest-numbered tab while a command is running; attack the
election under a stale `plugin_panes` frame immediately after a close; attack
the already-at-top suppressor when the snapshot push is dropped; attack the
agent-pane exclusion when `clave-register` was lost (dossier RC-B) so
`uuid_to_pane` is empty and an agent pane looks plain.*

That last one is real and must be answered in the PR: **if the register pipe was
lost, an agent pane is indistinguishable from a plain pane**, so Claude's tool
calls would bump its tab. Consequence is a bump of a tab the user is actively
watching — cosmetic, self-limiting via the already-at-top suppressor, and it
disappears when S0/RC-B fixes the register/bind gap. State it; do not hide it.

### 7.4 Tier 3

§8's live-validation script.

---

## 8. Live validation (the maintainer runs it; a different agent prints)

**Contract** (TESTING.md:188-204): the human drives every keypress and every
session launch/kill; the agent prints commands and reads observability. The agent
driving this holds **only this document**.

All paths genericised (`$HOME`, `$TMPDIR`) — the pre-commit PII blocklist rejects
private local paths in staged lines and has fired twice (AGENTS.md:122-124).

### Pre-flight (mandatory — issue #44 is unfixed)

| | |
| --- | --- |
| **P0 (a)** | Agent prints; maintainer runs in a **non-zellij** terminal: `command -v clave && clave --version` |
| **(b)** | Then: `grep -h 'clave-bar: loaded' "$TMPDIR/zellij-$(id -u)/zellij-log/zellij.log" \| tail -3` |
| **(c)** | Report both outputs |
| **(d)** | **Versions match** → proceed. **Mismatch** → STOP. Issue #43/#44: the bar shells out to bare `clave` through `PATH`; a stale binary is answering. Every reading below is untrustworthy (dossier:534). Report and stop |

### Steps

| # | (a) Command / keystroke | (b) Look at | (c) Report | (d) Branch |
| --- | --- | --- | --- | --- |
| **1** | Non-zellij terminal: `clave dev reset` then `clave dev scenario c8-cold-start` then `clave dev launch` | the `clave-test` session opens with a sidebar | "sandbox up" / any error | No sidebar → stop; the wasm is missing from the sandbox data dir (`just dev-install` is the maintainer's to run, not the agent's) |
| **2** | Agent runs (after a `zellij list-sessions` liveness check) the §2.4 build + `cp` + hot-reload; prints the build tag | sidebar reappears after reload | the tag string | Reload errors → stop |
| **3** | `Alt+t` for a fresh plain terminal tab | the new tab appears at the **top** of the sidebar | "top" / "not top" | Not top → pre-existing RC-B/RC-C; **stop and report** — S2 cannot be evaluated on a broken baseline |
| **4** | `Alt+j` to a *different* tab, then `Alt+k` back — 4 presses total | sidebar order | "order unchanged" / "order changed" | Changed → the "focus never reorders" invariant is already broken; stop |
| **5** | Go to a plain tab that is **not** row 0. Type `sleep 4` + Enter. Watch the sidebar for 6 s | does the tab move to row 0, and **when** — at start or at finish? | "moved at start" / "moved at finish" / "never moved" | **at start** → the core requirement works. **at finish** → the `fg` polarity is inverted; report immediately, it is a one-line fix. **never** → go to step 9 |
| **6** | Same tab, now at row 0. `Alt+j` to another tab. From *there*, watch while the first tab's `sleep 4` (re-run it first) completes | does the *other* tab jump on completion? | "jumped" / "did not jump" | **did not jump** → the second clause holds — the headline result. **jumped** → the completion exclusion has failed; report the exact sequence |
| **7** | On a non-top plain tab: `sleep 30 &` + Enter, then wait ~35 s without touching anything | at start, and at the completion notice | two observations | Bump at start only = correct (the `&` still starts a command *you* ran). Bump at completion = **defect**, report |
| **8** | On a non-top plain tab: `ls` + Enter, three times, waiting 2 s between | did the tab move? | "moved n of 3" | 3/3 → Branch A full. 0/3 → **Branch A′** (§4.6): expected, prints the escalation question. 1–2/3 → report the count, it is the sampler racing the command |
| **9** | On a non-top plain tab: `cd ..` then `cd -` | did the tab move? | yes/no | Answers whether §4.5's `CwdChanged` companion shipped/works |
| **10** | Go to the agent tab. Send a prompt that makes Claude run 2–3 Bash tool calls (e.g. "run `git status` then `ls`") | does the agent tab move on the **prompt** (expected — the hook) or also on each **tool call**? | "prompt only" / "also on tool calls" | **also on tool calls** → the agent-pane exclusion (§4.2 conjunct 3) is not firing; likely a lost `clave-register` (dossier RC-B). Report, and check `clave dev status \| jq '.store.agents'` for `tab_id: null` |
| **11** | On a plain tab: scroll back through output with the mouse wheel; click two sidebar rows | did the order change? | yes/no | Any change → Branch B(i)'s R2/R3 residuals are live; under Branch A this must be a firm **no** |
| **12** | On a plain tab: `ssh localhost`, run `ls` and `sleep 3` remotely, then `exit` | bumps at connect? during remote work? at exit? | three observations | Confirms §5's in-pane-ssh degradation. Expect: bump at connect, nothing during, nothing at exit |
| **13** | Agent runs: `clave dev status \| jq '.store.tab_timeline'` and `jq . "$HOME/.local/state/clave-dev/state/agents.json"` | timeline entries vs the sidebar order you can see | paste both | Divergence between store order and visible order = a bar/store desync; report with the sidebar screenshot |
| **14** | Agent runs the §2.6 volume greps against the windowed log | `CLAVE_DBG_*` counts (spike build) or `clave touch` frequency in `$HOME/.local/state/clave-dev/state/clave.log` | the counts | Hundreds-to-thousands of touches over this session ⇒ the rate bound in §4.4 is wrong; **revert and re-spike**. Note `touch` calls **no `log_event`** today (dossier:536-540), so under the implementation build the evlog will be silent — count from the spike build, or add a temporary `log_event` |
| **15** | Teardown (maintainer, non-zellij terminal): `zellij kill-session clave-test && zellij delete-session --force clave-test` | — | done | — |

**The agent must not run steps 1, 3–12, or 15.** It prints them. Steps 2, 13 and
14 are agent-side and read-only (2 is the one sanctioned mutation).

---

## 9. Proposed follow-up issue (low priority)

Open separately; **do not implement in S2**.

> **Title:** Status glyphs for plain terminal tabs (running / done / failed)
>
> **Labels:** `enhancement`, `host-untestable`, `priority: low`
>
> Agent rows carry a status glyph (`Status::glyph()`,
> `crates/clave-types/src/lib.rs:24-32`); plain terminal tabs render a 2-space
> gutter (`crates/clave-bar/src/main.rs:540-543`). The maintainer's ask
> (2026-07-22): *"Terminal responses shouldn't affect [ordering] if there's a
> process that's running and completes (that would ideally use the same glyphs
> down the line, but that's something for a future feature, can issue separately
> as low importance)."*
>
> **Shape.** S2 (RC-D) establishes that zellij 0.44.3's `CommandChanged`
> (`zellij-utils-0.44.3/src/data.rs:1016`; emitted from
> `zellij-server-0.44.3/src/pty.rs:2137-2175`) delivers a per-pane
> `is_foreground` flag whose `true`→`false` transition is a command finishing.
> That is enough for a two-state running/idle glyph on a plain tab **with no new
> permission** (`ReadApplicationState`, already granted) and no shell config.
>
> **Known limits, inherited from S2:**
> - It is a **1 Hz sampler** (`background_jobs.rs:114`), so sub-second commands
>   are never observed — glyphs would only appear for commands ≳ 1 s.
> - The event carries **no exit status**, so "done" and "failed" cannot be
>   distinguished. `PaneInfo.exit_status` is documented as only meaningful for
>   *command* panes (`data.rs:2312-2316`), which a plain shell tab is not.
>   A red/green split needs a different source and is likely out of reach without
>   shell integration.
> - `TabInfo.has_bell_notification` (`data.rs:2271-2274`) is output-side and is
>   the wrong signal — S2 excluded it deliberately.
>
> **Blast radius:** `Row.glyph` and `rows()` in `crates/clave-bar/src/model.rs`;
> the render at `main.rs:539-559`. Overlaps **S5** (per-repo colour), which
> restructures `Row`. Sequence after S5.
>
> **Verification:** Tier 1 for the state machine; `host-untestable` for the
> glyphs themselves.

---

## 10. Risks, sequencing, out of scope

### Risks

| # | Risk | Mitigation |
| --- | --- | --- |
| K1 | The 1 Hz sampler misses `ls`-class commands ⇒ partial UX | §4.6 Branch A′; escalate with the spike's numbers rather than guessing |
| K2 | Re-run of the round-4 fd storm | Spike spawns nothing at all; implementation's rate is bounded by §4.4, derived from source, and the bound goes in the PR |
| K3 | The pane→tab join inherits RC-A's stale-frame defect | The §4.1 own-tab conjunct makes a mis-election **silent** rather than corrupting. Prefer landing after S0 |
| K4 | A late touch resurrects a `tab_timeline` entry for a closed tab | §7.3 ¶3 — self-healing via detection-driven prune; residual is RC-E's, owned by S3 |
| K5 | A lost `clave-register` makes an agent pane look plain ⇒ tool calls bump the agent tab | Declared in §7.3; disappears with S0/RC-B; cosmetic and self-limiting meanwhile |
| K6 | `sysinfo`'s cwd probe (`os_input_output.rs:512-540`) may be unreliable on macOS ⇒ `CwdChanged` silent | §4.5 is optional and gated on live confirmation (step 9) |
| K7 | Reading `zellij-server` from crates.io ≠ the binary the maintainer runs | Pin the version: confirm `zellij --version` is `0.44.3`; the repo's version tripwire (`crates/clave/tests/zellij_pin_tripwire.rs`) already asserts a single zellij-family version in `Cargo.lock` |
| K8 | Branch B(i)'s three-way `classify_timer` mis-pops a dwell generation ⇒ dwell-to-open dies silently | Only if B(i) is taken: proptest the three-class FIFO before shipping |

### Sequencing with the other workstreams

- **S2 is parallelisable with S0, S4 and S5** (dossier:564-566) — different
  files, different seams.
- **But S2 and S0 both edit `crates/clave-bar/src/main.rs`.** S0 owns
  `main.rs:43-71` (`is_active_instance` / `own_tab_id`) and `:434-467`
  (`TabUpdate`/`PaneUpdate`, including the birth touch). S2 adds a `subscribe`
  entry at `:363-375`, a new `update` arm, and a `run_effects` arm at `:145-166`.
  **Sequencing rule:**
  1. The subscribe list is a one-line addition in a stable region — trivially
     rebasable either way.
  2. S2 must **not** touch `main.rs:434-445` (the birth touch). Leave it as a
     direct `run_command` even though `Effect::Touch` now exists (§4.7). This is
     the single decision that keeps the two branches conflict-free.
  3. **Land S0 first if the schedule allows.** S2's election reuses
     `is_active_instance()`; S0 makes it correct. If S2 lands first, note in the
     PR that its gate is only as good as S0's and that the own-tab conjunct is
     what makes that acceptable.
  4. If the spike shows `tab=None` on most events, S2 is **hard-blocked** on S0
     (§2.7).
- **S1 and S2 both touch `store.rs` timeline semantics.** S1 owns the sort key
  and the resolution; S2 owns `apply_touch`'s return type. Small, orthogonal
  hunks — whichever lands second rebases. If S1 goes sub-second, §3's gate becomes
  inert but stays correct (§3.4).
- **S3 owns the close path.** S2 widens the recycled-id window (§7.3 ¶3); cite it
  in S3's issue rather than fixing it here.

### Out of scope

- Status glyphs for terminal processes — §9, separate issue, low priority.
- `PermissionType::InterceptInput` / `Event::UserAction` — needs a permission-set
  change and a `permissions.kdl` reseed in lockstep (`main.rs:352-356`); a
  maintainer decision of its own.
- Shell integration of any kind — §6.2, declined, escalate only.
- Adopting manually-run `claude` processes, terminal-pane exit codes, and
  per-pane (as opposed to per-tab) ordering.
- Fixing `clave touch`'s absence from the evlog (dossier:536-540). Worth an
  issue: it makes this exact bug class invisible to `clave.log`.
