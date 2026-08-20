# Frecency E2E sandbox drive — plan (PR #218)

State: branch worktree-frecency-ordering @ 7cf4f5c, pushed, PR #218 open.
Drive not yet staged. Follow TESTING.md § the sandbox drive loop exactly.

## Stage (agent)
1. `just sandbox` from THIS worktree (per-worktree sandbox; `clave dev instance` prints session/root/key — never touch Ollie's live session).
2. **Seed mocked buckets AFTER staging** (run_setup rewrites the data dir): with `CLAVE_STATE_DIR=$(clave-dev dev instance --field state 2>/dev/null || clave dev instance --field state)` — verify the exact seeding surface first; the store is `<state>/agents.json`. Compute `TODAY=$(( $(date +%s) / 86400 ))`. Edit the seeded store with jq to mock buckets + decays:
   - agent A "invested": buckets `{TODAY-6:8, TODAY-3:8, TODAY-1:8, TODAY:2}` (~high score)
   - agent B "one-off today": `{TODAY:1}` but the NEWEST commit_ord/tab_order ordinal
   - agent C "dormant giant": `{TODAY-6:30}` (decayed), no tab
   - agent D "empty": no buckets (zero-fallback ordinal path)
   Also set `order` field absent (tests default=Frecency{24}).
3. Print launch command for Ollie (`clave dev launch` / the printed launch line).

## Drive (agent, after Ollie launches)
Per SOP: (3) verify build tag on TAIL of zellij.log loaded lines; (4) baseline store+panes BEFORE provoking; (5) provoke + re-join after EVERY step; (6) 60s quiescence — seq and evlog must hold (anti-storm); (7) force tab-id reuse (close highest, create one); (8) report what didn't reproduce; (9) hand teardown to Ollie.

Permutation matrix (each = provoke → read `rows()` order via pane/store join):
1. Cold default: bare `clave order` prints Frecency{24}; initial order A > B > C(dormant block) with D bottom-of-live by ordinal fallback.
2. Recency flip: `clave order recency` → B jumps above A on ALL instances incl. background tabs (snapshot broadcast, not TabUpdate).
3. Back to frecency: `clave order frecency` → A above B again (buckets persisted, instant re-sort).
4. Dial ends: `clave order frecency 1` ≈ recency order; `clave order frecency 999` ≈ raw 7-day counts (C's 30 decayed points outrank A? at 999h: yes 30 > 26 — check C tops the dormant block / A-vs-C only if C woken).
5. Prompt commitment: prompt an agent once (sandbox claude or `clave hook` UserPromptSubmit simulation via ct.sh if scenario agents are fake) → +1 today bucket, order shifts accordingly, seq bumps once.
6. Newborn inheritance: from A's tab, `clave add` (or new-tab) → new row lands DIRECTLY BELOW A (exact copy + position tiebreak). Terminal tab from A's tab → same.
7. Dormancy R2: close A's tab → A holds rank in dormant block; reopen → rank unchanged.
8. Upgrade path: copy a bucket-less pre-branch store in → order identical to ordinal order until a commitment lands.

## Teardown (Ollie)
Print kill pair from TESTING.md § tear the sandbox down.

## Report
Numbered live steps + evidence into PR #218 body/comment per TESTING.md §113 (PR must state its live steps, sandbox-first).
