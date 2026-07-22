---
name: Bug report
about: Something in clave behaves wrongly — include how we would have caught it
title: ''
labels: bug
assignees: ''
---

## What happened

<!-- Symptom first, as seen. "A second sidebar appeared in new tabs and Alt-↑/↓
     navigation half-died" is the useful shape. -->

## What you expected

## Reproduction

<!-- Exact steps. Say whether this was the STABLE session (`clave`) or the
     SANDBOX (`clave dev launch`, session `clave-test`) — they use different
     binaries, state dirs and artifacts, and the difference has been the bug
     more than once. -->

## Environment

```
clave --version         :
which clave             :
ls ~/.local/share/clave/bin/ :
```

Loaded plugin versions (**every line must report the same version** — mixed
versions are the #44 failure mode in progress):

```sh
grep 'clave-bar: loaded' "$TMPDIR"/zellij-*/zellij-log/zellij.log | tail
```

```
<paste>
```

<!-- Other windows worth pasting: the evlog (`~/.local/state/clave/clave.log`,
     or `~/.local/state/clave-dev/state/clave.log` for the sandbox) and
     `clave dev status`. Observability map: docs/dev/TESTING.md. -->

## How would we test this?

<!-- The point of the field: turn the incident into a guard. What is the
     smallest assertion that would have failed? A pure function over generated
     artifacts? A parse pin? A property the model does not yet have? If you
     think nothing could have caught it, say so — that is a finding too. -->

## Which tier would catch it?

- [ ] **Tier 1 — hermetic** (unit, proptest, KDL parser guardrail, version pin,
      CLI parse pin, sandboxed subcommand run). Runs anywhere, no TTY.
- [ ] **Tier 2 — real zellij** in an isolated `clave-it-<pid>` session.
      *Does not exist yet — #47, blocked on #44.* Tick this to record that the
      bug is currently unguarded and add it to #47's scenario list.
- [ ] **Tier 3 — human at a terminal.** Glyphs, colour, fonts, geometry, feel.
      Nothing automated will ever adjudicate it.

**Risk class** (docs/dev/TESTING.md taxonomy): <!-- pure logic / generated
artifacts / CLI surface / cross-process / install-environment / visual-UX -->

## Suspected subsystem

<!-- If you know it, name the C-section in
     docs/superpowers/spikes/SUBSYSTEM-VALIDATION.md — the ledger may already
     record this approach failing. -->
