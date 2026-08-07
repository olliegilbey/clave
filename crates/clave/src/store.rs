//! The clave state store (spec §5): one JSON file, read-modified-written under
//! an advisory lock held on a SEPARATE, never-renamed lockfile, with the data
//! write done as temp-file + atomic rename.
//!
//! WHY the separate lockfile: locking the data file itself would be a bug —
//! the atomic rename swaps the data file's inode out from under a second
//! writer's lock, and concurrent hooks (Claude's global hook fan-in) would
//! silently lose updates. The lockfile never gets renamed, so its inode is a
//! stable lock anchor. The flip side: readers that only need a consistent
//! (possibly slightly stale) view can skip the lock entirely — an
//! atomic-rename reader always sees a whole file. That lock-free path is what
//! keeps `clave hook` from ever delaying an UNTRACKED Claude session (§6.5).

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use clave_types::{Agent, AgentSnapshot, Status};
use fs4::fs_std::FileExt;
use serde::{Deserialize, Serialize};

/// Where an agent's label came from (§6.4). While `FirstPrompt`, `clave hook`
/// keeps tail-scanning the jsonl for a session summary; once `Summary`, it
/// stops re-scanning forever (the label only meaningfully changes once).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LabelSource {
    FirstPrompt,
    Summary,
}

/// One store row (spec §5's agent record, minus the deleted `archived`).
/// Mirrors `clave_types::Agent` plus the store-only `label_source`, which the
/// plugin never needs to see. `worktree` was store-only until #69 put it on
/// the wire for S6's provenance glyph (#61).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRecord {
    /// Minted session UUID — the join key (invariant #3).
    pub uuid: String,
    /// PHYSICAL (canonicalized) working directory — see munge.rs / S0b.
    pub cwd: String,
    /// git toplevel of cwd; keys the §6.3 add/resume picker.
    pub repo_root: String,
    pub branch: String,
    /// `dir · branch · summary-or-first-prompt` (§6.4).
    pub label: String,
    pub status: Status,
    /// unix s; bumped on UserPromptSubmit. DISPLAY and cross-session policy
    /// only (`clave ls`, the §6.3 picker, eager-launch selection) — NOT the
    /// bar's sort key any more (S1/#39). Wire twin of
    /// `clave_types::Agent::last_interacted`, which carries the rationale.
    pub last_interacted: u64,
    /// The commitment ORDINAL that orders this row (S1/#39). Wire twin of
    /// `clave_types::Agent::commit_ord`. Minted by [`Store::mint_ord`] under the
    /// flock, so it is a total order with no clock and no ties; 0 = never
    /// committed. `default` keeps pre-field store files loading, and
    /// [`clear_session_order`] backfills those rows from `last_interacted` once.
    #[serde(default)]
    pub commit_ord: u64,
    /// unix s; bumped on focus (`clave focus`) → clears done-unread.
    pub last_visited: u64,
    /// Worktree path if `clave add --worktree` created one (§6.3), else None.
    pub worktree: Option<String>,
    pub label_source: LabelSource,
    /// Zellij tab id hosting this agent (§6.6 Design B), bound by the agent
    /// tab's own bar via `clave bind`. Keys the hook's prompt→timeline stamp
    /// and the bar's glyph join. Session-scoped: None until bound, reset on
    /// session recreate (see clear_session_order).
    #[serde(default)]
    pub tab_id: Option<usize>,
    /// §5 (2026-07-17): `clave open` found the row's cwd missing → the bar
    /// renders ✗ instead of ◌. A row flag, NOT a status (statuses are hook
    /// lifecycle); cleared by a later successful open. `default` keeps
    /// pre-field payloads parseable.
    #[serde(default)]
    pub stale: bool,
    /// Claude's session rename, from the transcript's `custom-title` line.
    /// Store-side home for the wire field of the same name (#69). Written by
    /// `hook::refresh_row_fields`, held last-non-empty — `None` means the
    /// session was never renamed, which is most rows, and the design renders
    /// that as a blank chip. `default` keeps pre-field store files loading —
    /// a missing key is a whole-store parse failure, not a blank field.
    #[serde(default)]
    pub title: Option<String>,
    /// The words segment, held structurally rather than only inside `label`
    /// (design-lock §7.1). Seeded once from existing labels by
    /// `backfill_summaries`; thereafter written by `hook::refresh_row_fields`
    /// from `ai-title`, the `type:"summary"` tier being extinct (#79).
    /// `default` keeps pre-field store files loading.
    #[serde(default)]
    pub summary: String,
    /// The REPOSITORY's default branch (the branch a plain checkout sits on),
    /// resolved once at `add` time by `add::resolve_default_branch`. Wire twin
    /// of `clave_types::Agent::default_branch`, which carries the full rationale
    /// (#86): the bar must not decide "default checkout" from a hardcoded
    /// `main`/`master` list. `None` = not discoverable (no remote), which the
    /// bar handles by falling back to that heuristic. `default` keeps pre-field
    /// store files loading — a missing key is a whole-store parse failure.
    #[serde(default)]
    pub default_branch: Option<String>,
    /// The session id Claude is CURRENTLY using for this row, when it is no
    /// longer the minted `uuid` — see UBIQUITOUS_LANGUAGE, "minted uuid vs live
    /// session id".
    ///
    /// `None` means the two are NOT KNOWN to disagree, which is not quite the
    /// same as agreeing: a row written before this field existed, and a seeded
    /// sandbox row, are both `None` regardless of what Claude is doing. Every
    /// consumer treats that as "fall back to the minted uuid", which is exactly
    /// the pre-#99 behaviour.
    ///
    /// Deliberately reintroduced (#99). #98 deleted a field of this shape
    /// because its only job was keeping a DERIVED jsonl path alive, which S4
    /// forbids — `payload.transcript_path` replaced it for the READ half. This
    /// one exists for the RESURRECTION half, which no payload can serve: `clave
    /// spawn` runs before any Claude exists to send one, so the id must have
    /// been written down beforehand or it is gone. (`spawn::resume_target` does
    /// still DERIVE a path from it, as `spawn_mode` always has — but only to
    /// probe existence. S4's withdrawal is of the derived READ.) Justified by
    /// measured loss, not symmetry: resurrection on the minted uuid reopens the
    /// pre-rotation conversation and orphans everything since the `/clear`
    /// (confirmed live 2026-07-31 — the resumed agent knew only the pre-clear
    /// content).
    ///
    /// Written by `hook::apply_hook_event` under [`PidGate`] — BOTH directions,
    /// which is stricter than the event admission around it: `resolve_row` lets
    /// a payload whose id names a row through ungated, and an outside `claude
    /// --resume <minted>` on the orphaned transcript would otherwise read as
    /// agreement and wipe a pointer that is still true. Never trusted blind on
    /// the way out either: `resume_target` requires the named jsonl to EXIST,
    /// and refuses a value shaped like a flag, before it reaches argv.
    ///
    /// [`PidGate`]: crate::hook::PidGate
    #[serde(default)]
    pub live_session: Option<String>,
}

/// The whole store file. `seq` is the monotonic snapshot counter of the §5
/// pipe contract — persisted HERE so pushes stay monotonic across processes
/// (every hook invocation is a fresh process).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub seq: u64,
    /// Keyed by uuid. BTreeMap for stable iteration order (deterministic
    /// snapshots and `ls` output).
    #[serde(default)]
    pub agents: BTreeMap<String, AgentRecord>,
    /// tab_id → the commitment ORDINAL of the last user commitment to that tab
    /// (§6.6 row order / S1). Kept HERE, not per bar instance: instance-local
    /// copies fed by fire-and-forget pipe deltas diverged live (C5 round 5) —
    /// the store RMW is the one writer, and the map rides every snapshot push.
    /// tab_ids are session-scoped: cleared on session (re)create.
    ///
    /// RENAMED from `tab_timeline` (S1 §3.6), which held unix seconds. Renaming
    /// rather than repurposing is what makes the upgrade safe: an old store
    /// file's `tab_timeline` key is ignored, so second-scale values cannot leak
    /// into the ordinal space and outrank every ordinal minted afterwards.
    #[serde(default)]
    pub tab_order: BTreeMap<usize, u64>,
    /// Bar collapse mode (issue #5): plugin-side per-instance memory synced
    /// only by the toggle broadcast desynced live (C8 parity-desync — a
    /// reload or missed pipe flips one instance forever). Same doctrine as
    /// tab_order above: the store RMW is the one writer and the flag
    /// rides every snapshot push, so instances hydrate at birth and heal on
    /// every push. `default` (expanded) keeps pre-field store files loading.
    #[serde(default)]
    pub collapsed: bool,
}

impl Store {
    /// Mint the next commitment ORDINAL (§6.6 / S1). The store's `seq` IS the
    /// ordinal: it is persisted, monotonic, and bumped exactly once per locked
    /// write, so two commitments can never collide and no wall clock is
    /// involved. Callers MUST be inside [`with_store_mut`] — the flock is what
    /// makes this a total order — and must NOT bump `seq` again afterwards.
    ///
    /// The shared counter is deliberate (S1 §3.4): a second counter would have
    /// to be bumped in lockstep with `seq` forever, and the first write that
    /// bumped one and forgot the other would corrupt the order invisibly, with
    /// no test able to see it. The rule that keeps the two roles from being
    /// conflated: NOTHING ever compares an ordinal to a snapshot `seq`.
    pub(crate) fn mint_ord(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }
}

pub struct StorePaths {
    pub dir: PathBuf,
    pub data: PathBuf,
    pub lock: PathBuf,
}

/// Spec §5 names the literal path `~/.local/state/clave/`. Built from $HOME
/// rather than `dirs::state_dir()` because the latter is `None` on macOS and
/// we want one path on every platform. `$CLAVE_STATE_DIR` overrides the whole
/// dir (spec §6.9: the dev harness sandboxes the store).
pub fn store_paths() -> Result<StorePaths> {
    let home = dirs::home_dir().context("cannot determine home directory")?;
    let default = home.join(".local").join("state").join("clave");
    let dir = crate::env::dir_from(std::env::var("CLAVE_STATE_DIR").ok(), default);
    Ok(StorePaths {
        data: dir.join("agents.json"),
        lock: dir.join("agents.lock"),
        dir,
    })
}

/// Lock-free read. Safe without the lock because writers replace the file by
/// atomic rename — a reader opens either the old whole file or the new whole
/// file, never a torn write. Missing file = empty store (first run).
pub fn read_store(paths: &StorePaths) -> Result<Store> {
    match fs::read(&paths.data) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
        Err(e) => Err(e).context("reading store"),
    }
}

/// Locked read-modify-write. The exclusive flock on the lockfile is held
/// across the WHOLE read→mutate→write, so concurrent hook events serialize
/// instead of clobbering each other. The closure's return value passes
/// through so callers can extract data computed under the lock.
pub fn with_store_mut<T>(paths: &StorePaths, f: impl FnOnce(&mut Store) -> T) -> Result<T> {
    fs::create_dir_all(&paths.dir).context("creating store dir")?;
    let lock = fs::OpenOptions::new()
        .create(true)
        // Contents are never read or written — this file exists only to hold
        // an flock (spec §5 store locking, fugu 2026-07-01: the lock is a
        // SEPARATE file, never the renamed-over data file, else concurrent
        // hooks lose updates). `truncate(false)` spells out the existing
        // default explicitly (no behavior change) — suspicious_open_options.
        .truncate(false)
        .write(true)
        .open(&paths.lock)
        .context("opening lockfile")?;
    lock.lock_exclusive().context("locking store")?;
    // From here to the end of the function we hold the lock; any early return
    // (via ?) drops `lock`, which releases the flock — no poisoned state.
    let mut store = read_store(paths)?;
    let out = f(&mut store);
    let tmp = paths.dir.join("agents.json.tmp");
    {
        let mut t = fs::File::create(&tmp).context("creating temp store")?;
        t.write_all(&serde_json::to_vec_pretty(&store)?)?;
        // fsync BEFORE the rename so a crash can't leave a renamed-but-empty
        // file (rename is atomic, buffered content is not).
        t.sync_all()?;
    }
    fs::rename(&tmp, &paths.data).context("atomic store swap")?;
    FileExt::unlock(&lock).context("unlocking store")?;
    Ok(out)
}

/// Store → pipe snapshot (§5): drop `label_source`, keep the order. The single
/// producer — design-lock §7.1 rules the bar renders from the store, so every
/// field it lays a column from arrives here as a value, never as a position
/// inside `label` (#69).
pub fn snapshot_from(store: &Store) -> AgentSnapshot {
    AgentSnapshot {
        seq: store.seq,
        tab_order: store.tab_order.clone(),
        collapsed: store.collapsed,
        agents: store
            .agents
            .values()
            .map(|r| Agent {
                uuid: r.uuid.clone(),
                cwd: r.cwd.clone(),
                repo_root: r.repo_root.clone(),
                branch: r.branch.clone(),
                label: r.label.clone(),
                status: r.status,
                last_interacted: r.last_interacted,
                commit_ord: r.commit_ord,
                last_visited: r.last_visited,
                tab_id: r.tab_id,
                stale: r.stale,
                title: r.title.clone(),
                summary: r.summary.clone(),
                // Projected now — `AgentRecord` has carried this since §6.3
                // and the wire simply never did (S6 #61 §2.4).
                worktree: r.worktree.clone(),
                // The other half of provenance (#86): without it the bar can
                // only guess the default branch by name.
                default_branch: r.default_branch.clone(),
            })
            .collect(),
    }
}

/// `clave focus <uuid>` (§6.5): persist the "user looked at it" transition
/// and hand back a seq-bumped snapshot for the caller to push. Zellij only
/// delivers TabUpdate to the ACTIVE tab's bar instance (C3 live finding), so
/// exactly one instance repainted locally — the pipe push is how every other
/// instance learns the flip. Unknown uuid returns None (no bump, no push):
/// the plugin can race an agent whose tab just closed.
/// #139: repoint a relocated row to its transcript's true home. Same
/// contract as its apply_* neighbours (#143 review): seq advances and a
/// snapshot returns only when the row EXISTS — an unknown uuid is a no-op
/// that broadcasts nothing.
pub fn apply_relocation(
    paths: &StorePaths,
    uuid: &str,
    cwd: &str,
    branch: Option<&str>,
) -> Result<Option<AgentSnapshot>> {
    with_store_mut(paths, |s| {
        let r = s.agents.get_mut(uuid)?;
        r.cwd = cwd.to_string();
        if let Some(b) = branch {
            r.branch = b.to_string();
        }
        s.seq += 1; // monotonic pipe contract (§5)
        Some(snapshot_from(s))
    })
}

pub fn apply_focus(paths: &StorePaths, uuid: &str, now: u64) -> Result<Option<AgentSnapshot>> {
    with_store_mut(paths, |s| {
        let r = s.agents.get_mut(uuid)?;
        r.last_visited = now;
        if r.status == Status::Done {
            r.status = Status::Idle; // green "done & unread" → dim
        }
        s.seq += 1; // monotonic pipe contract (§5)
        Some(snapshot_from(s))
    })
}

/// `clave touch <tab_id>` (§6.6): stamp a user commitment on the STORE's tab
/// order and hand back a seq-bumped snapshot for the pipe push.
///
/// The ordinal is minted INSIDE the lock, so it is strictly greater than every
/// ordinal already in the map — a plain `insert` is therefore already a max.
/// The old max-merge existed only because `now` was read BEFORE the lock (in
/// `main.rs`) and two touches could serialize in the opposite order to their
/// clock reads. That race is now impossible, not merely absorbed (S1 §3.1).
pub fn apply_touch(paths: &StorePaths, tab_id: usize) -> Result<AgentSnapshot> {
    with_store_mut(paths, |s| {
        touch_in(s, tab_id);
        snapshot_from(s)
    })
}

/// The pure half of [`apply_touch`] — mint and stamp, no I/O. Returns the
/// ordinal it minted. `mint_ord` bumps `seq` itself, so this IS the pipe
/// contract's one bump for the write (§5); callers must not bump again.
pub(crate) fn touch_in(s: &mut Store, tab_id: usize) -> u64 {
    let ord = s.mint_ord();
    s.tab_order.insert(tab_id, ord);
    ord
}

/// `clave bind <uuid> <tab_id>` (§6.6 Design B): persist the uuid→tab join
/// reported by the agent tab's own bar instance (the only one whose data is
/// reliably fresh — it is active at spawn time). Snapshot back only on
/// CHANGE, so a bar re-reporting an existing bind costs no push. Unknown
/// uuid returns None (bar may race a pruned agent).
pub fn apply_bind(paths: &StorePaths, uuid: &str, tab_id: usize) -> Result<Option<AgentSnapshot>> {
    with_store_mut(paths, |s| {
        if !s.agents.contains_key(uuid) {
            return None; // bar may race a pruned agent
        }
        // Evict any OTHER agent still bound to this tab_id. zellij reuses
        // tab_ids (get_new_tab_id = max-key+1, screen.rs:1617), so a reused id
        // could otherwise leave a dead agent AND the new one both claiming the
        // tab — the bar's glyph join (agent_in_tab) would decorate it with
        // whichever uuid sorts first. One tab hosts one agent: the freshest
        // bind wins. (Prune_tabs is the primary cleaner; this closes the
        // close-then-immediately-reuse window where the id never went absent.)
        let mut evicted: Vec<String> = Vec::new();
        for r in s.agents.values_mut() {
            if r.uuid != uuid && r.tab_id == Some(tab_id) {
                r.tab_id = None;
                evicted.push(r.uuid.clone());
            }
        }
        if !evicted.is_empty() {
            // #55 observability: this whole bug class was invisible to the
            // evlog — bind/touch/prune-tabs/focus log nothing. A legitimate
            // eviction (tab-id reuse, the branch above) and an RC-A mis-bind
            // look identical in the store, so the discriminator is whether the
            // evicted uuid still has a live pane in that tab — a question only
            // answerable by joining this line against `zellij action
            // list-panes`. Costs nothing when nothing is wrong: the log stays
            // empty. Cheap and store-side-effect-free on purpose —
            // `with_store_mut` holds an exclusive flock across this closure.
            crate::evlog::log_event_in(
                &paths.dir,
                "bind-evict",
                &format!("tab={tab_id} winner={uuid} evicted={evicted:?}"),
            );
        }
        let evicted = !evicted.is_empty();
        let already = s.agents.get(uuid).and_then(|r| r.tab_id) == Some(tab_id);
        if already && !evicted {
            return None; // re-report of an existing bind, no collision → free
        }
        if let Some(r) = s.agents.get_mut(uuid) {
            r.tab_id = Some(tab_id);
        }
        s.seq += 1; // monotonic pipe contract (§5)
        Some(snapshot_from(s))
    })
}

/// `clave prune-tabs <stale tab ids…>` (#6/F3): carry each dying tab's
/// commitment ordinal onto the agent that was bound to it (S1), then drop the
/// tab_order entries and clear agent tab_id binds for EXACTLY the ids listed —
/// the ones the bar observed die on a close. Session recreate wiped these
/// wholesale (clear_session_order); mid-session tab CLOSE left them to grow
/// unbounded —
/// and, since zellij REUSES tab_ids (get_new_tab_id = max-key+1, screen.rs:1617),
/// a survivor entry would let a reused-id tab inherit a dead agent's glyph/order.
///
/// REMOVE-LISTED, not retain-carried: the payload is the DEAD ids, so two
/// fire-and-forget prunes (no arrival-order guarantee — the collapse
/// pending-write class) COMMUTE and are idempotent; a late prune can only
/// re-remove ids already judged dead, never strip a bind for a tab it never
/// observed close. (A retain-only-live payload would clobber the bind of ANY
/// tab created after the prune was computed → live agent rendered dormant → the
/// #6 double-attach via a race.) Change-gated (no push when nothing matched).
/// Self-heal lives in the bar's detection (staleness re-derived each set
/// change), so a lost push is re-emitted while the entry persists. Residual: if
/// a listed id is REUSED within the subprocess-latency window a late removal
/// could unbind the new tenant — `apply_bind` eviction is the backstop and the
/// window is milliseconds. Empty payload = no-op (nothing observed dead).
/// None = no change.
pub fn apply_prune_tabs(paths: &StorePaths, stale_ids: &[usize]) -> Result<Option<AgentSnapshot>> {
    with_store_mut(paths, |s| {
        prune_in(s, stale_ids).then(|| {
            s.seq += 1; // monotonic pipe contract (§5)
            snapshot_from(s)
        })
    })
}

/// The pure half of [`apply_prune_tabs`] — the state transition with no I/O and
/// no `seq` bump, so the ordinal properties can quantify over it without a
/// filesystem. Returns whether anything changed. Factored out exactly as
/// `apply_hook_event` already was, for the same reason.
pub(crate) fn prune_in(s: &mut Store, stale_ids: &[usize]) -> bool {
    if stale_ids.is_empty() {
        return false; // nothing observed dead
    }
    let mut changed = false;
    // S1: the row INHERITS its tab's ordinal before the entry dies, so a
    // close moves nothing RELATIVE to anything else (R2). Without this the
    // row falls back to a different key in a different tiebreak class and
    // every neighbour re-sorts — the "an unrelated tab jumped to the top"
    // report. `max` keeps this idempotent and commuting with a second prune
    // (the #6/F3 order-safety property): a re-run finds tab_id already None
    // and carries nothing. Runs BEFORE the retain below, or there is no
    // ordinal left to inherit.
    for r in s.agents.values_mut() {
        if let Some(id) = r.tab_id.filter(|id| stale_ids.contains(id)) {
            let carried = s.tab_order.get(&id).copied().unwrap_or(0);
            r.commit_ord = r.commit_ord.max(carried);
            r.tab_id = None;
            changed = true;
        }
    }
    let before = s.tab_order.len();
    s.tab_order.retain(|id, _| !stale_ids.contains(id));
    changed |= s.tab_order.len() != before;
    changed
}

/// `clave collapse <true|false>` (issue #5): persist the bar collapse mode
/// as an ABSOLUTE value — never a flip, so broadcast races and duplicate
/// executor writes stay idempotent — and hand back a seq-bumped snapshot
/// for the pipe push that heals any instance the `clave-toggle` broadcast
/// missed. Snapshot back only on CHANGE (like apply_bind): a re-assert of
/// the current mode must not generate pipe traffic (round 11: storms).
pub fn apply_collapse(paths: &StorePaths, collapsed: bool) -> Result<Option<AgentSnapshot>> {
    with_store_mut(paths, |s| {
        if s.collapsed == collapsed {
            return None;
        }
        s.collapsed = collapsed;
        s.seq += 1; // monotonic pipe contract (§5)
        Some(snapshot_from(s))
    })
}

/// `clave open` outcome (§6.3, 2026-07-17): record whether the row's cwd was
/// missing. Snapshot back only on CHANGE (a repeated stale open must not
/// generate pipe traffic); None for unknown uuids.
pub fn apply_open_result(
    paths: &StorePaths,
    uuid: &str,
    stale: bool,
) -> Result<Option<AgentSnapshot>> {
    with_store_mut(paths, |s| {
        let r = s.agents.get_mut(uuid)?;
        if r.stale == stale {
            return None;
        }
        r.stale = stale;
        s.seq += 1; // monotonic pipe contract (§5)
        Some(snapshot_from(s))
    })
}

/// Seed `summary` for rows written before the field existed, by lifting the
/// words segment out of the composed label (`dir · branch · words`).
///
/// `splitn(3)` so a summary that itself contains the separator survives
/// whole. Matches only EMPTY summaries, so it is idempotent and self-limiting
/// — after one pass nothing matches again. Same shape as S1 §3.6's
/// `commit_ord` backfill.
///
/// WHY it is needed at all, now that `refresh_row_fields` keeps summaries live
/// (`hook.rs`): DORMANT rows receive no hook events by definition — no claude
/// process, no transcript being written — so without this they render a blank
/// summary field indefinitely. (The other half of the original rationale, the
/// §6.4 freeze, no longer applies: the summary write is decoupled from it.)
///
/// Returns whether anything changed, so the caller can gate its `seq` bump:
/// §5 forbids no-op pushes.
pub fn backfill_summaries(s: &mut Store) -> bool {
    let mut changed = false;
    for r in s.agents.values_mut() {
        if !r.summary.is_empty() {
            continue;
        }
        if let Some(words) = r.label.splitn(3, clave_types::LABEL_SEP).nth(2)
            && !words.is_empty()
        {
            r.summary = words.to_string();
            changed = true;
        }
    }
    changed
}

/// Session (re)create hygiene: tab_ids are SESSION-scoped, so a fresh
/// session must inherit neither dead tabs' commitments (reused ids) nor
/// stale uuid→tab binds. No push — no bar instance exists yet at launch
/// time; hydration reads the store.
///
/// Agent ordinals (`commit_ord`) are agent-scoped and deliberately SURVIVE
/// (S1 §3.1): clearing them would collapse every dormant row to 0 and
/// cold-start the list in uuid order instead of recency order.
///
/// Also the S1 backfill point (§3.6). A store written by a pre-ordinal binary
/// has `commit_ord == 0` everywhere, so without this the first launch after the
/// upgrade would render the dormant list in uuid order — a visible regression
/// on a real fleet. Seeding from the old wall-clock ranking converts it into
/// the ordinal space exactly once; self-limiting, since after one launch
/// nothing matches.
pub fn clear_session_order(paths: &StorePaths) -> Result<()> {
    with_store_mut(paths, |s| {
        let bound = s.agents.values().any(|r| r.tab_id.is_some());
        let mut changed = false;
        if !s.tab_order.is_empty() || bound {
            s.tab_order.clear();
            s.agents.values_mut().for_each(|r| r.tab_id = None);
            changed = true;
        }
        // Session create is the one locked pass that runs at every launch,
        // so it is where the one-shot backfill rides (#69). Accepted cost: a
        // MID-session upgrade leaves dormant rows blank until the next
        // launch. The alternative is a migration hook on every store open —
        // more machinery than a cosmetic gap on unused rows justifies.
        changed |= backfill_summaries(s);
        // S1 ordinal backfill, oldest first so the seeded ordinals preserve the
        // old wall-clock ranking exactly.
        let mut pre_ordinal: Vec<(u64, String)> = s
            .agents
            .values()
            .filter(|r| r.commit_ord == 0 && r.last_interacted > 0)
            .map(|r| (r.last_interacted, r.uuid.clone()))
            .collect();
        pre_ordinal.sort();
        // `mint_ord` bumps `seq` itself, so a mint already satisfies the §5
        // invariant ("content changed ⇒ seq advanced"). The trailing bump must
        // therefore fire only when something changed WITHOUT minting, or a
        // launch that both cleared and backfilled would advance `seq` twice.
        let minted = !pre_ordinal.is_empty();
        for (_, uuid) in pre_ordinal {
            let ord = s.mint_ord();
            if let Some(r) = s.agents.get_mut(&uuid) {
                r.commit_ord = ord;
            }
        }
        if changed && !minted {
            s.seq += 1; // content changed ⇒ seq changed (§5)
        }
    })
}

/// Retire dormant rows idle longer than the cutoff (#149). Pure — the caller
/// owns the locked write, and the caller supplies `protected`: the uuids that
/// are live in the RUNNING zellij session. Protection is deliberately not
/// read off `tab_id` here — binds are session-scoped and only cleared at the
/// NEXT launch, so between a kill and a relaunch every row still carries its
/// dead session's tab_id, and a bind-based guard would silently make the
/// whole fleet unprunable (#150 review). The transcript is untouched either
/// way — a pruned conversation stays resumable through the Alt+a resume
/// flow; only the sidebar row goes. Returns the removed rows, oldest-idle
/// first, for the caller to print; bumps seq only when something went.
pub fn prune_idle(
    store: &mut Store,
    now: u64,
    idle_days: u64,
    protected: &std::collections::BTreeSet<String>,
) -> Vec<AgentRecord> {
    let cutoff = now.saturating_sub(idle_days.saturating_mul(86_400));
    let doomed: Vec<String> = store
        .agents
        .values()
        .filter(|r| !protected.contains(&r.uuid) && r.last_interacted < cutoff)
        .map(|r| r.uuid.clone())
        .collect();
    let mut removed: Vec<AgentRecord> = doomed
        .iter()
        .filter_map(|u| store.agents.remove(u))
        .collect();
    removed.sort_by_key(|r| r.last_interacted);
    if !removed.is_empty() {
        store.seq += 1; // monotonic pipe contract (§5)
    }
    removed
}

/// Seconds since the epoch — the store's one timestamp format.
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_paths(dir: &std::path::Path) -> StorePaths {
        StorePaths {
            dir: dir.to_path_buf(),
            data: dir.join("agents.json"),
            lock: dir.join("agents.lock"),
        }
    }

    fn rec(uuid: &str) -> AgentRecord {
        AgentRecord {
            uuid: uuid.into(),
            cwd: "/x".into(),
            repo_root: "/x".into(),
            branch: "main".into(),
            label: "x · main".into(),
            status: Status::Idle,
            last_interacted: 0,
            commit_ord: 0,
            last_visited: 0,
            worktree: None,
            label_source: LabelSource::FirstPrompt,
            tab_id: None,
            stale: false,
            title: None,
            summary: String::new(),
            default_branch: None,
            live_session: None,
        }
    }

    /// #149: only unprotected rows past the cutoff go; a protected row NEVER
    /// prunes however old; a DEAD session's bind protects nothing (#150
    /// review: binds outlive their session until the next launch clears
    /// them); exactly-at-cutoff stays; seq advances once per real removal
    /// and not on a no-op; removed rows come back oldest-idle first.
    #[test]
    fn prune_idle_takes_only_unprotected_rows_past_the_cutoff() {
        let mut s = Store::default();
        let now = 100 * 86_400;
        let mut older = rec("u-older");
        older.last_interacted = now - 20 * 86_400;
        let mut old = rec("u-old");
        old.last_interacted = now - 11 * 86_400;
        let mut fresh = rec("u-fresh");
        fresh.last_interacted = now - 9 * 86_400;
        let mut live = rec("u-live");
        live.last_interacted = now - 30 * 86_400;
        live.tab_id = Some(4);
        // A bind from a session that is no longer running: NOT protected.
        let mut deadbind = rec("u-deadbind");
        deadbind.last_interacted = now - 30 * 86_400;
        deadbind.tab_id = Some(9);
        let mut edge = rec("u-edge");
        edge.last_interacted = now - 10 * 86_400;
        for r in [older, old, fresh, live, deadbind, edge] {
            s.agents.insert(r.uuid.clone(), r);
        }
        let protected: std::collections::BTreeSet<String> = ["u-live".to_string()].into();
        let seq0 = s.seq;
        let removed = prune_idle(&mut s, now, 10, &protected);
        let ids: Vec<&str> = removed.iter().map(|r| r.uuid.as_str()).collect();
        assert_eq!(
            ids,
            ["u-deadbind", "u-older", "u-old"],
            "oldest-idle first, and a dead-session bind does not protect"
        );
        assert!(s.agents.contains_key("u-fresh"));
        assert!(
            s.agents.contains_key("u-live"),
            "a protected row never prunes"
        );
        assert!(s.agents.contains_key("u-edge"), "exactly-at-cutoff stays");
        assert_eq!(s.seq, seq0 + 1);
        // A no-op prune must not advance seq.
        assert!(prune_idle(&mut s, now, 10, &protected).is_empty());
        assert_eq!(s.seq, seq0 + 1);
    }

    #[test]
    fn missing_file_reads_as_default() {
        let d = tempfile::tempdir().unwrap();
        let s = read_store(&tmp_paths(d.path())).unwrap();
        assert_eq!(s.seq, 0);
        assert!(s.agents.is_empty());
    }

    #[test]
    fn rmw_roundtrips_and_bumps_seq() {
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            s.agents.insert("u1".into(), rec("u1"));
            s.seq += 1;
        })
        .unwrap();
        // A second, separate RMW sees the first one's write (file-backed).
        with_store_mut(&p, |s| {
            assert_eq!(s.seq, 1);
            s.agents.get_mut("u1").unwrap().status = Status::Working;
            s.seq += 1;
        })
        .unwrap();
        let s = read_store(&p).unwrap();
        assert_eq!(s.seq, 2);
        assert_eq!(s.agents["u1"].status, Status::Working);
        // The write went through temp+rename: no stray temp file remains.
        assert!(!p.dir.join("agents.json.tmp").exists());
        // The lockfile exists and was NOT renamed away (it is the lock anchor).
        assert!(p.lock.exists());
    }

    #[test]
    fn focus_clears_done_to_idle_and_stamps_visit() {
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            let mut r = rec("u1");
            r.status = Status::Done;
            s.agents.insert("u1".into(), r);
        })
        .unwrap();
        // Returns a seq-bumped snapshot for the pipe push: only the focused
        // tab's bar instance repaints locally (zellij starves hidden
        // instances of TabUpdates — C3 finding), so the flip must broadcast.
        let snap = apply_focus(&p, "u1", 999).unwrap().expect("row changed");
        let s = read_store(&p).unwrap();
        assert_eq!(s.agents["u1"].status, Status::Idle); // unread cleared
        assert_eq!(s.agents["u1"].last_visited, 999);
        assert!(s.seq > 0); // §5 pipe contract: the push must be strictly newer
        assert_eq!(snap.seq, s.seq);
        assert_eq!(snap.agents[0].status, Status::Idle);
        // Unknown uuid: silently fine (plugin may race a just-closed agent) —
        // no snapshot, seq untouched.
        assert!(apply_focus(&p, "ghost", 1000).unwrap().is_none());
        assert_eq!(read_store(&p).unwrap().seq, s.seq);
    }

    #[test]
    fn snapshot_mirrors_store_rows() {
        let mut s = Store {
            seq: 7,
            ..Store::default()
        };
        s.agents.insert("u1".into(), rec("u1"));
        s.tab_order.insert(4, 1700);
        s.agents.get_mut("u1").unwrap().tab_id = Some(4);
        let snap = snapshot_from(&s);
        assert_eq!(snap.seq, 7);
        assert_eq!(snap.agents.len(), 1);
        assert_eq!(snap.agents[0].uuid, "u1");
        assert_eq!(snap.agents[0].label, "x · main");
        // §6.6 store-timeline: order rides every snapshot.
        assert_eq!(snap.tab_order.get(&4), Some(&1700));
        // §6.6 Design B: the uuid→tab bind rides it too (glyph join key).
        assert_eq!(snap.agents[0].tab_id, Some(4));
    }

    #[test]
    fn collapse_persists_absolute_value_and_dedupes() {
        // `clave collapse` (issue #5): absolute value, seq-bumped RMW,
        // change-gated push — the store copy of the C8 parity-desync fix.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        let snap = apply_collapse(&p, true).unwrap().expect("changed");
        assert!(snap.collapsed, "snapshot must carry the new mode");
        // The FILE is the durable truth (CodeRabbit CLI, PR #13): assert the
        // persisted store too, not just the in-memory snapshot projection.
        assert!(read_store(&p).unwrap().collapsed);
        assert_eq!(snap.seq, 1);
        // Re-asserting the same mode: no change, no seq bump, no push —
        // duplicate executor writes after a broadcast are free (round 11).
        assert!(apply_collapse(&p, true).unwrap().is_none());
        assert_eq!(read_store(&p).unwrap().seq, 1);
        // Toggling back is a change again.
        let snap = apply_collapse(&p, false).unwrap().expect("changed back");
        assert!(!snap.collapsed);
        assert!(!read_store(&p).unwrap().collapsed);
        assert_eq!(snap.seq, 2);
    }

    #[test]
    fn bind_records_tab_id_once_and_ignores_unknown_or_unchanged() {
        // `clave bind <uuid> <tab_id>` (§6.6 Design B): the agent tab's own
        // bar reports its join to the store so every OTHER instance can key
        // glyphs/order off the snapshot instead of local joins (round 6:
        // register pipes don't replay; hidden manifests go stale).
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            s.agents.insert("u1".into(), rec("u1"));
        })
        .unwrap();
        let snap = apply_bind(&p, "u1", 4).unwrap().expect("bound");
        assert_eq!(snap.agents[0].tab_id, Some(4));
        assert_eq!(snap.seq, 1);
        // Same bind again: no change, no seq bump, no push.
        assert!(apply_bind(&p, "u1", 4).unwrap().is_none());
        assert_eq!(read_store(&p).unwrap().seq, 1);
        // A MOVED agent (pane broken out to a new tab) re-binds.
        let snap = apply_bind(&p, "u1", 9).unwrap().expect("rebound");
        assert_eq!(snap.agents[0].tab_id, Some(9));
        // Unknown uuid: silently none (bar may race a pruned agent).
        assert!(apply_bind(&p, "ghost", 1).unwrap().is_none());
    }

    #[test]
    fn bind_logs_only_when_it_evicts_and_logs_beside_its_own_store() {
        // #55 observability: the eviction is the ONE store-side trace of a
        // wrong bind, and the live-validation SOP joins these lines against
        // `zellij action list-panes` to tell a legitimate tab-id-reuse
        // eviction (victim has no pane) from an RC-A mis-bind (victim still
        // has a live pane in that tab). It must stay silent when nothing goes
        // wrong, or the signal drowns — and it must land beside the store
        // being written, not in the ambient one, so the sandbox's evictions
        // are readable in the sandbox's log.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            s.agents.insert("u1".into(), rec("u1"));
            s.agents.insert("u2".into(), rec("u2"));
        })
        .unwrap();
        let log = p.dir.join("clave.log");
        // An uncontested bind evicts nobody: no line.
        apply_bind(&p, "u1", 4).unwrap().expect("bound");
        assert!(!log.exists(), "an uncontested bind must log nothing");
        // u2 takes tab 4 from u1: one line, naming both sides.
        apply_bind(&p, "u2", 4).unwrap().expect("bound");
        let body = std::fs::read_to_string(&log).unwrap();
        assert_eq!(body.lines().count(), 1);
        let v: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(v["cmd"], "bind-evict");
        let detail = v["detail"].as_str().unwrap();
        assert!(detail.contains("tab=4"), "{detail}");
        assert!(detail.contains("winner=u2"), "{detail}");
        assert!(detail.contains("u1"), "{detail}");
        assert_eq!(read_store(&p).unwrap().agents["u1"].tab_id, None);
    }

    #[test]
    fn prune_tabs_removes_listed_stale_ids_order_safe_and_change_gated() {
        // #6/F3: mid-session tab CLOSE left binds + tab_timeline entries to grow
        // unbounded, and — with tab_id reuse (screen.rs:1617) — a survivor
        // decorates a reused-id tab. The bar reports the DEAD ids it observed;
        // the store REMOVES exactly those (not "retain the live set") so
        // out-of-order prunes commute and can't unbind a tab they never saw die.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            let mut live = rec("u-live");
            live.tab_id = Some(10);
            s.agents.insert("u-live".into(), live);
            let mut dead = rec("u-dead");
            dead.tab_id = Some(11); // its tab just closed
            s.agents.insert("u-dead".into(), dead);
        })
        .unwrap();
        apply_touch(&p, 10).unwrap();
        apply_touch(&p, 11).unwrap(); // stale timeline entry
        // Stale set is {11}: remove EXACTLY 11's bind + timeline entry; 10 (a
        // tab the prune never observed die) is untouched — the order-safety.
        let snap = apply_prune_tabs(&p, &[11]).unwrap().expect("pruned");
        let s = read_store(&p).unwrap();
        assert_eq!(s.agents["u-live"].tab_id, Some(10), "live bind untouched");
        assert_eq!(s.agents["u-dead"].tab_id, None, "dead bind cleared");
        assert!(s.tab_order.contains_key(&10));
        assert!(!s.tab_order.contains_key(&11), "stale timeline dropped");
        assert!(snap.agents.iter().all(|a| a.tab_id != Some(11)));
        // Idempotent late arrival: re-removing an already-dead id → no change,
        // no push, no seq bump (this is what makes two out-of-order prunes safe).
        let seq = read_store(&p).unwrap().seq;
        assert!(apply_prune_tabs(&p, &[11]).unwrap().is_none());
        assert_eq!(read_store(&p).unwrap().seq, seq);
        // Empty payload (nothing observed dead) → no-op.
        assert!(apply_prune_tabs(&p, &[]).unwrap().is_none());
        assert_eq!(read_store(&p).unwrap().agents["u-live"].tab_id, Some(10));
    }

    #[test]
    fn ordinals_are_minted_strictly_increasing_under_the_lock() {
        // S1 §3.1: the store's own `seq` IS the commitment ordinal — persisted,
        // monotonic, bumped exactly once per locked write. Two commitments are
        // two locked writes and therefore get two distinct ordered values BY
        // CONSTRUCTION, with no clock read anywhere. There is no `now`
        // parameter to pass, which is the point.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        let mut seen = Vec::new();
        for tab in [4usize, 4, 9, 4, 9] {
            seen.push(apply_touch(&p, tab).unwrap().tab_order[&tab]);
        }
        assert_eq!(
            seen,
            vec![1, 2, 3, 4, 5],
            "same tab and different tabs both advance"
        );
        for w in seen.windows(2) {
            assert!(w[1] > w[0], "ordinals must strictly increase");
        }
        // The map never regresses: the last write for each tab is what stands.
        let s = read_store(&p).unwrap();
        assert_eq!(s.tab_order[&4], 4);
        assert_eq!(s.tab_order[&9], 5);
    }

    #[test]
    fn prune_carries_the_tabs_ordinal_onto_the_agent() {
        // S1 §1.2, the headline defect: closing a tab used to throw its ordering
        // key away, so the row fell back to a DIFFERENT key in a DIFFERENT
        // tiebreak class and every neighbour re-sorted — reported as "an
        // unrelated tab jumped to the top". The row now inherits the ordinal
        // before the entry dies.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            let mut a = rec("u-A");
            a.tab_id = Some(10);
            s.agents.insert("u-A".into(), a);
        })
        .unwrap();
        apply_touch(&p, 11).unwrap(); // a neighbour, ordinal 1
        let carried = apply_touch(&p, 10).unwrap().tab_order[&10]; // ordinal 2
        apply_prune_tabs(&p, &[10]).unwrap().expect("pruned");
        let s = read_store(&p).unwrap();
        assert_eq!(
            s.agents["u-A"].commit_ord, carried,
            "row inherited its tab's rank"
        );
        assert_eq!(s.agents["u-A"].tab_id, None, "and was unbound");
        assert!(!s.tab_order.contains_key(&10), "tab entry gone");
        assert!(
            s.agents["u-A"].commit_ord > s.tab_order[&11],
            "the closed row still outranks the neighbour it outranked before"
        );
    }

    #[test]
    fn prune_pushes_when_only_a_bind_was_cleared() {
        // A tab born and BOUND but never touched has no entry in the tab order
        // at all. Pruning it changes exactly one thing — the bind — and that
        // still has to reach the bar: the two change sources are independent,
        // so the push gate must be an OR over them, not an AND.
        //
        // Caught by cargo-mutants (`|=` → `&=` survived): under the AND the
        // store would clear the bind and stay silent, leaving every bar
        // rendering the row as live until some unrelated write pushed.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            let mut a = rec("u-A");
            a.tab_id = Some(10); // bound…
            s.agents.insert("u-A".into(), a);
        })
        .unwrap();
        assert!(
            read_store(&p).unwrap().tab_order.is_empty(),
            "…but never touched, so it holds no ordinal"
        );
        let snap = apply_prune_tabs(&p, &[10])
            .unwrap()
            .expect("a cleared bind alone must still push");
        assert!(snap.agents.iter().all(|a| a.tab_id.is_none()));
        assert_eq!(read_store(&p).unwrap().agents["u-A"].tab_id, None);
    }

    #[test]
    fn prune_carry_is_idempotent_and_commutes() {
        // The #6/F3 order-safety property, extended to the new write: prunes are
        // fire-and-forget with no arrival-order guarantee, so a second prune of
        // the same id must change nothing at all.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            let mut a = rec("u-A");
            a.tab_id = Some(10);
            s.agents.insert("u-A".into(), a);
        })
        .unwrap();
        apply_touch(&p, 10).unwrap();
        apply_prune_tabs(&p, &[10]).unwrap().expect("pruned");
        let after_first = read_store(&p).unwrap();
        // Re-run: tab_id is already None, so there is nothing to carry.
        assert!(apply_prune_tabs(&p, &[10]).unwrap().is_none(), "no push");
        let after_second = read_store(&p).unwrap();
        assert_eq!(after_first, after_second, "second prune is a total no-op");
    }

    #[test]
    fn prune_carry_never_lowers_an_agents_ordinal() {
        // The carry is a `max`, not an assignment. An agent prompted AFTER its
        // tab's last touch already outranks that tab; inheriting blindly would
        // demote it on close — the very thing S1 exists to prevent.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        apply_touch(&p, 10).unwrap(); // tab ordinal 1
        with_store_mut(&p, |s| {
            let mut a = rec("u-A");
            a.tab_id = Some(10);
            a.commit_ord = 99; // prompted later than its tab was touched
            s.agents.insert("u-A".into(), a);
        })
        .unwrap();
        apply_prune_tabs(&p, &[10]).unwrap().expect("pruned");
        assert_eq!(read_store(&p).unwrap().agents["u-A"].commit_ord, 99);
    }

    #[test]
    fn clear_session_order_backfills_pre_ordinal_rows() {
        // S1 §3.6, the upgrade path. A store written by a pre-ordinal binary has
        // `commit_ord == 0` everywhere; without a backfill the first launch
        // after the upgrade would render the dormant list in UUID order, which
        // is a visible regression on a real fleet. Seed from the old wall clock,
        // oldest first, so the previous ranking survives into the ordinal space.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            for (uuid, li) in [("u-mid", 200u64), ("u-old", 100), ("u-new", 300)] {
                let mut a = rec(uuid);
                a.last_interacted = li;
                s.agents.insert(uuid.into(), a);
            }
            // Never interacted with: no clock to convert, so it stays at 0 and
            // sorts to the bottom, which is exactly where it belongs.
            s.agents.insert("u-never".into(), rec("u-never"));
            // Already carries an ordinal: must NOT be touched.
            let mut done = rec("u-done");
            done.last_interacted = 50;
            done.commit_ord = 7;
            s.agents.insert("u-done".into(), done);
        })
        .unwrap();
        clear_session_order(&p).unwrap();
        let s = read_store(&p).unwrap();
        assert_eq!(
            s.agents["u-done"].commit_ord, 7,
            "existing ordinal untouched"
        );
        assert_eq!(s.agents["u-never"].commit_ord, 0, "no clock ⇒ no seed");
        let (old, mid, new) = (
            s.agents["u-old"].commit_ord,
            s.agents["u-mid"].commit_ord,
            s.agents["u-new"].commit_ord,
        );
        assert!(
            old > 0 && old < mid && mid < new,
            "wall-clock ranking preserved: {old} < {mid} < {new}"
        );
        // Self-limiting: a second launch finds nothing to seed and changes none
        // of the values it wrote.
        let before = read_store(&p).unwrap();
        clear_session_order(&p).unwrap();
        let after = read_store(&p).unwrap();
        assert_eq!(
            before.agents, after.agents,
            "backfill must run exactly once"
        );
    }

    #[test]
    fn clear_session_order_preserves_agent_ordinals() {
        // Tab ids are SESSION-scoped, so the tab order and the binds go. Agent
        // ordinals are AGENT-scoped and must survive: clearing them would
        // collapse every dormant row to 0 and cold-start the list in uuid order
        // (S1 §3.1).
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            let mut a = rec("u1");
            a.tab_id = Some(4);
            a.commit_ord = 42;
            a.last_interacted = 900;
            s.agents.insert("u1".into(), a);
            s.tab_order.insert(4, 42);
        })
        .unwrap();
        clear_session_order(&p).unwrap();
        let s = read_store(&p).unwrap();
        assert!(s.tab_order.is_empty(), "session-scoped tab order cleared");
        assert_eq!(s.agents["u1"].tab_id, None, "session-scoped bind cleared");
        assert_eq!(
            s.agents["u1"].commit_ord, 42,
            "agent-scoped ordinal SURVIVES"
        );
    }

    #[test]
    fn bind_evicts_a_reused_tab_id_from_the_previous_agent() {
        // zellij reuses tab_ids (get_new_tab_id = max-key+1, screen.rs:1617):
        // if a dead agent's bind survived a close, a NEW agent binding the
        // REUSED id must EVICT it — one tab hosts one agent, else the bar's
        // glyph join (agent_in_tab) decorates the tab with whichever uuid sorts
        // first (a dead agent's colour). Belt-and-suspenders with prune_tabs.
        // Fixture (CodeRabbit MINOR): u-new ALREADY holds Some(11) too, so the
        // call hits the `already && evicted` path — it must NOT short-circuit
        // the no-op return (that would leave the dead bind in place); it must
        // evict u-dead AND emit a snapshot.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            let mut dead = rec("u-dead");
            dead.tab_id = Some(11); // survived a close, still bound to 11
            s.agents.insert("u-dead".into(), dead);
            let mut newer = rec("u-new");
            newer.tab_id = Some(11); // already bound to the reused id → `already`
            s.agents.insert("u-new".into(), newer);
        })
        .unwrap();
        // `already && evicted`: u-new is unchanged but u-dead IS evicted, so the
        // write is real — a snapshot must come back (not the no-op None).
        let snap = apply_bind(&p, "u-new", 11)
            .unwrap()
            .expect("collision eviction must push even when the binder is unchanged");
        let s = read_store(&p).unwrap();
        assert_eq!(s.agents["u-new"].tab_id, Some(11), "new agent stays bound");
        assert_eq!(
            s.agents["u-dead"].tab_id, None,
            "reused id evicted the dead bind"
        );
        assert!(snap.agents.iter().filter(|a| a.tab_id == Some(11)).count() == 1);
    }

    #[test]
    fn clear_session_state_wipes_timeline_and_binds() {
        // Both maps are keyed by session-scoped tab_ids — a recreated
        // session must inherit neither.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            s.agents.insert("u1".into(), rec("u1"));
        })
        .unwrap();
        apply_touch(&p, 4).unwrap();
        apply_bind(&p, "u1", 4).unwrap();
        clear_session_order(&p).unwrap();
        let s = read_store(&p).unwrap();
        assert!(s.tab_order.is_empty());
        assert_eq!(s.agents["u1"].tab_id, None);
    }

    #[test]
    fn touch_mints_a_monotone_ordinal_and_bumps_seq() {
        // `clave touch <tab_id>` (§6.6): the ONE writer of tab order. Locked
        // RMW here — per-instance pipe-delta merges diverged live (C5 rd 5).
        //
        // REWRITTEN by S1. This test used to pin a max-merge against a `now`
        // argument that no longer exists: the stamp was a wall clock read
        // BEFORE the lock, so two touches could serialize in the opposite order
        // to their clock reads and a late one had to be prevented from
        // regressing the map. The ordinal is minted INSIDE the lock, so it is
        // strictly greater than every ordinal already there — monotone by
        // construction rather than by defence.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        let snap = apply_touch(&p, 4).unwrap();
        assert_eq!(snap.tab_order.get(&4), Some(&1));
        assert_eq!(snap.seq, 1); // §5: every push strictly newer
        // The write's own seq IS the ordinal, so they cannot drift apart.
        let snap = apply_touch(&p, 4).unwrap();
        assert_eq!(snap.tab_order.get(&4), Some(&2));
        assert_eq!(snap.seq, 2);
        // A different tab draws from the same counter — one ordinal space, so
        // tabs and rows interleave correctly with no cross-space comparison.
        let snap = apply_touch(&p, 9).unwrap();
        assert_eq!(snap.tab_order.get(&9), Some(&3));
        assert_eq!(
            snap.tab_order.get(&4),
            Some(&2),
            "another tab's touch must not move this one"
        );
        assert_eq!(snap.seq, 3);
        // Persisted: a fresh read sees the same map.
        assert_eq!(read_store(&p).unwrap().tab_order.get(&4), Some(&2));
    }

    #[test]
    fn open_result_sets_and_clears_stale_on_change_only() {
        // §6.3 `clave open`: cwd missing → stale=true (bar ✗); a later
        // successful open clears it. Snapshot back only on CHANGE (§5: no
        // no-op pushes), None for unknown uuids (row may have been pruned).
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            s.agents.insert("u1".into(), rec("u1"));
        })
        .unwrap();
        let snap = apply_open_result(&p, "u1", true).unwrap().expect("changed");
        assert!(snap.agents[0].stale);
        assert_eq!(snap.seq, 1);
        // Same value again: no change, no seq bump, no push.
        assert!(apply_open_result(&p, "u1", true).unwrap().is_none());
        assert_eq!(read_store(&p).unwrap().seq, 1);
        // Successful open clears it.
        let snap = apply_open_result(&p, "u1", false)
            .unwrap()
            .expect("cleared");
        assert!(!snap.agents[0].stale);
        // Unknown uuid: silently none.
        assert!(apply_open_result(&p, "ghost", true).unwrap().is_none());
    }

    #[test]
    fn a_failed_open_touches_no_ordering_field_so_stale_rows_sink() {
        // #124's whole retention rule, and it is an ABSENCE: stale rows are
        // never deleted and never hidden, they just sink. That works only
        // because a failed open mints nothing an ordering key reads, so the
        // row's recency freezes at its last real prompt while every row that
        // IS used passes it. Nothing in `apply_open_result` implements this
        // -- it is a property of what the function does not do, which is
        // exactly the kind of guarantee a refactor deletes silently.
        //
        // Asserted as a WHOLE-RECORD comparison rather than a field list on
        // purpose: S1 (#56) moves the dormant sort key from `last_interacted`
        // to `commit_ord`, and a field-list assertion would keep passing while
        // the new field got minted on a failed open. This form fails the day
        // any field but `stale` moves here, including one that does not exist
        // yet.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        let mut row = rec("u1");
        row.last_interacted = 5_000; // non-zero: catches a clobber-to-0 too
        row.last_visited = 4_000;
        // Same reasoning, applied to the field S1 added: `rec()` leaves this at
        // 0, so a change that CLOBBERED it to 0 would be a no-op against the
        // fixture and pass. Seeding it non-zero closes that half, while the
        // whole-record assertion below already covers the half that matters
        // more — a failed open MINTING an ordinal (#124's author, PR #135).
        row.commit_ord = 7_000;
        with_store_mut(&p, |s| {
            s.agents.insert("u1".into(), row.clone());
        })
        .unwrap();

        apply_open_result(&p, "u1", true).unwrap().expect("changed");
        let mut only_stale_moved = row.clone();
        only_stale_moved.stale = true;
        assert_eq!(
            read_store(&p).unwrap().agents["u1"],
            only_stale_moved,
            "a failed open must flip `stale` and nothing else -- any other \
             field moving here lets a dead row hold its place in the ring"
        );

        // Healing is symmetric: remount the volume, open again, and the row
        // is byte-for-byte what it was. The grace period for a transiently
        // missing cwd is this retry, not a timer (#124).
        apply_open_result(&p, "u1", false)
            .unwrap()
            .expect("cleared");
        assert_eq!(read_store(&p).unwrap().agents["u1"], row);
    }

    #[test]
    fn clear_tab_timeline_wipes_session_scoped_ids() {
        // tab_ids are SESSION-scoped: a recreated session reuses ids, so a
        // stale timeline would order new tabs by dead tabs' commitments.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        apply_touch(&p, 4).unwrap();
        clear_session_order(&p).unwrap();
        let s = read_store(&p).unwrap();
        assert!(s.tab_order.is_empty());
        assert_eq!(s.seq, 2); // content changed ⇒ seq changed (§5 invariant)
        // Idempotent: clearing an empty timeline changes nothing.
        clear_session_order(&p).unwrap();
        assert_eq!(read_store(&p).unwrap().seq, 2);
    }

    #[test]
    fn snapshot_projects_title_summary_and_worktree_from_the_record() {
        // One producer, one consumer (§5): snapshot_from is the only place
        // a record becomes a wire Agent, so this is the whole contract.
        let mut s = Store::default();
        let mut r = rec("u1");
        r.title = Some("CLA-MAIN".into());
        r.summary = "fix the flaky auth".into();
        r.worktree = Some("/x/.claude/worktrees/wt".into());
        r.default_branch = Some("trunk".into());
        s.agents.insert("u1".into(), r);

        let snap = snapshot_from(&s);
        let a = &snap.agents[0];
        assert_eq!(a.title.as_deref(), Some("CLA-MAIN"));
        assert_eq!(a.summary, "fix the flaky auth");
        assert_eq!(a.worktree.as_deref(), Some("/x/.claude/worktrees/wt"));
        // #86: the bar cannot decide provenance from the branch NAME, so the
        // repo's real default has to cross this seam too — a record that knows
        // it and a wire Agent that does not is the whole bug.
        assert_eq!(a.default_branch.as_deref(), Some("trunk"));
    }

    #[test]
    fn agent_record_title_and_summary_default_on_pre_field_store_files() {
        // The first run of a new binary reads the EXISTING agents.json, which
        // has neither key. Without #[serde(default)] that is a whole-store
        // parse failure and every agent vanishes — not a blank field.
        let json = serde_json::to_value(rec("u1")).unwrap();
        let mut o = json.as_object().unwrap().clone();
        o.remove("title");
        o.remove("summary");
        // #86's field joins the same set, and it is the one Ollie's REAL store
        // is about to be missing: every row written before this branch.
        o.remove("default_branch");
        // #99's field joins it too. Note what `None` means for a row read this
        // way: not "the ids agree" but "nothing has told us otherwise yet" —
        // resurrection falls back to the minted uuid until a hook reports.
        o.remove("live_session");
        let back: AgentRecord = serde_json::from_value(serde_json::Value::Object(o)).unwrap();
        assert_eq!(back.title, None);
        assert!(back.summary.is_empty());
        assert_eq!(back.default_branch, None);
        assert_eq!(back.live_session, None);
    }

    #[test]
    fn backfill_lifts_the_words_segment_out_of_an_existing_label() {
        // Rows written before `summary` existed carry it only inside `label`.
        // refresh_label returns early forever once label_source == Summary
        // (hook.rs:155), and dormant rows get no hook events at all — so
        // without this they would render a blank 17-column field for good.
        let mut s = Store::default();
        let mut r = rec("u1");
        r.label = "clave \u{00b7} main \u{00b7} fix the flaky auth".into();
        r.summary = String::new();
        s.agents.insert("u1".into(), r);

        assert!(backfill_summaries(&mut s));
        assert_eq!(s.agents["u1"].summary, "fix the flaky auth");
    }

    #[test]
    fn backfill_keeps_a_separator_inside_the_summary_text() {
        // splitn(3) — a summary that itself contains the separator survives
        // whole. A plain split() would truncate it at the first occurrence.
        let mut s = Store::default();
        let mut r = rec("u1");
        r.label = "clave \u{00b7} main \u{00b7} a \u{00b7} b".into();
        r.summary = String::new();
        s.agents.insert("u1".into(), r);

        backfill_summaries(&mut s);
        assert_eq!(s.agents["u1"].summary, "a \u{00b7} b");
    }

    #[test]
    fn backfill_is_idempotent_and_skips_labels_without_a_words_segment() {
        // Self-limiting: it matches only EMPTY summaries, so a second pass
        // changes nothing. Same shape as S1 §3.6's commit_ord backfill.
        let mut s = Store::default();
        let mut earned = rec("u1");
        earned.label = "clave \u{00b7} main \u{00b7} fix the flaky auth".into();
        earned.summary = "already set by S4".into();
        s.agents.insert("u1".into(), earned);
        let mut bare = rec("u2");
        bare.label = "clave \u{00b7} main".into(); // never earned any words
        bare.summary = String::new();
        s.agents.insert("u2".into(), bare);

        assert!(!backfill_summaries(&mut s), "nothing to do");
        assert_eq!(s.agents["u1"].summary, "already set by S4");
        assert!(s.agents["u2"].summary.is_empty());

        // And a real pass must not re-fire on a second run.
        let mut t = Store::default();
        let mut r = rec("u3");
        r.label = "clave \u{00b7} main \u{00b7} words".into();
        r.summary = String::new();
        t.agents.insert("u3".into(), r);
        assert!(backfill_summaries(&mut t));
        assert!(!backfill_summaries(&mut t), "second pass is a no-op");
    }

    #[test]
    fn clear_tab_timeline_backfills_summaries_and_bumps_seq_on_its_own() {
        // `clear_session_order` is the backfill's ONLY production caller
        // (#69): the wiring, not the helper, is what a live upgrade runs.
        // Nothing here is CLEARABLE — empty timeline, no bind — so the seq
        // bump can only come from the backfill, which is exactly §5's
        // invariant: content changed ⇒ seq changed, whichever cause fired.
        let d = tempfile::tempdir().unwrap();
        let p = tmp_paths(d.path());
        with_store_mut(&p, |s| {
            let mut r = rec("u1");
            r.label = "x \u{00b7} main \u{00b7} fix the flaky auth".into();
            s.agents.insert("u1".into(), r);
        })
        .unwrap();
        let before = read_store(&p).unwrap();
        // Assert the precondition rather than only documenting it: if `rec()`
        // ever gained a `tab_id`, the clearing branch would fire and the seq
        // assertion below would still pass while testing nothing.
        assert!(before.tab_order.is_empty() && before.agents["u1"].tab_id.is_none());
        let before = before.seq;

        clear_session_order(&p).unwrap();
        let s = read_store(&p).unwrap();
        assert_eq!(
            s.agents["u1"].summary, "fix the flaky auth",
            "the backfill must persist through the locked RMW, not just mutate in memory"
        );
        assert_eq!(s.seq, before + 1, "backfill alone still bumps seq (§5)");

        // Self-limiting at the call site too: a second launch finds nothing.
        clear_session_order(&p).unwrap();
        assert_eq!(
            read_store(&p).unwrap().seq,
            before + 1,
            "§5 forbids no-op pushes"
        );
    }

    /// The ordinal invariants, quantified rather than exemplified (S1 §5.3).
    /// Both claims below are universal — "every ordinal ever minted" and "no
    /// event but a prompt" — and a table of examples cannot establish either.
    /// These run against the PURE halves (`touch_in`, `prune_in`,
    /// `apply_hook_event`), so no filesystem or lock is involved.
    mod proptests {
        use super::*;
        use crate::hook::{HookPayload, apply_hook_event};
        use proptest::prelude::*;

        /// One arbitrary store mutation. Deliberately includes the events that
        /// must NOT reorder, so a regression that stamped on `Stop` is
        /// reachable by the generator rather than needing to be guessed.
        #[derive(Debug, Clone)]
        enum Op {
            Touch(usize),
            Prune(Vec<usize>),
            Event(&'static str, usize),
        }

        const EVENTS: [&str; 7] = [
            "UserPromptSubmit",
            "Stop",
            "StopFailure",
            "SessionEnd",
            "Notification",
            "PermissionRequest",
            "PreToolUse",
        ];

        fn op_strategy() -> impl Strategy<Value = Op> {
            prop_oneof![
                (0usize..6).prop_map(Op::Touch),
                prop::collection::vec(0usize..6, 0..3).prop_map(Op::Prune),
                (0usize..EVENTS.len(), 0usize..3).prop_map(|(e, a)| Op::Event(EVENTS[e], a)),
            ]
        }

        /// A store with three agents, each bound to a tab, so every op has
        /// something to act on.
        fn seeded() -> Store {
            let mut s = Store::default();
            for i in 0..3usize {
                let uuid = format!("u{i}");
                let mut r = rec(&uuid);
                r.tab_id = Some(i);
                s.agents.insert(uuid, r);
            }
            s
        }

        fn run(s: &mut Store, op: &Op, minted: &mut Vec<u64>) {
            match op {
                Op::Touch(id) => minted.push(touch_in(s, *id)),
                Op::Prune(ids) => {
                    prune_in(s, ids);
                }
                Op::Event(event, agent) => {
                    let uuid = format!("u{agent}");
                    let p = HookPayload {
                        session_id: Some(uuid.clone()),
                        prompt: None,
                        message: Some("needs your permission".into()),
                        transcript_path: None,
                    };
                    if apply_hook_event(s, &uuid, event, &p, None, 1000, true) {
                        // Any accepted write mints exactly one ordinal, which is
                        // the write's own seq.
                        minted.push(s.seq);
                    }
                }
            }
        }

        proptest! {
            /// Property — ordinals are a TOTAL ORDER. Every ordinal ever minted
            /// is distinct and strictly increasing, and nothing in the store
            /// ever holds an ordinal above `seq`. This is what makes ties
            /// unreachable for committed rows, which is the whole §1.1 fix.
            #[test]
            fn prop_ordinals_are_a_total_order(
                ops in prop::collection::vec(op_strategy(), 1..25)
            ) {
                let mut s = seeded();
                let mut minted = Vec::new();
                for op in &ops {
                    run(&mut s, op, &mut minted);
                }
                for w in minted.windows(2) {
                    prop_assert!(w[1] > w[0], "ordinals must strictly increase: {:?}", minted);
                }
                // No ordinal may exceed the counter that minted it. (The
                // converse — comparing an ordinal TO a seq — is exactly what
                // `mint_ord`'s doc forbids anywhere in production code.)
                for v in s.tab_order.values() {
                    prop_assert!(*v <= s.seq);
                }
                for r in s.agents.values() {
                    prop_assert!(r.commit_ord <= s.seq);
                }
            }

            /// Property — ONLY prompts change the order. The §2 table as an
            /// invariant rather than a list of examples: for any sequence of
            /// events containing no `UserPromptSubmit`, the pair (tab order,
            /// every row's ordinal) comes out unchanged — even though statuses
            /// and labels do change along the way.
            #[test]
            fn prop_only_prompts_change_the_order(
                picks in prop::collection::vec((1usize..EVENTS.len(), 0usize..3), 1..20)
            ) {
                let mut s = seeded();
                // Seed one real commitment so there is a non-trivial order to
                // preserve, then never prompt again.
                touch_in(&mut s, 0);
                let mut minted = Vec::new();
                run(&mut s, &Op::Event("UserPromptSubmit", 1), &mut minted);

                let order_before = s.tab_order.clone();
                let ords_before: Vec<(String, u64)> = s
                    .agents
                    .iter()
                    .map(|(u, r)| (u.clone(), r.commit_ord))
                    .collect();

                for (e, a) in picks {
                    // `1..` skips UserPromptSubmit — every other event is fair
                    // game and none of them may re-rank anything.
                    run(&mut s, &Op::Event(EVENTS[e], a), &mut minted);
                }

                prop_assert_eq!(&s.tab_order, &order_before, "a non-prompt event moved a tab");
                let ords_after: Vec<(String, u64)> = s
                    .agents
                    .iter()
                    .map(|(u, r)| (u.clone(), r.commit_ord))
                    .collect();
                prop_assert_eq!(ords_after, ords_before, "a non-prompt event re-ranked a row");
            }
        }
    }
}
