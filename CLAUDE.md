# CLAUDE.md — clave

Claude-specific information, all agents refer to:
@AGENTS.md

Your Claude models have been upgraded massively since this project started, your additional skill and capability means you can reassess past work for refactors, simplification, and improvements to keep everything elegant and sensible for the forward momentum and to have a great codebase.
Do this without allowing conversations and implementations to creep massively in scope.

Work directly and proportionately.

- Read only the files necessary to complete the requested change.
- Implement the requested scope only; do not refactor nearby code unless required.
- Make ordinary engineering choices without asking.
- Ask a question only if different interpretations would materially change the implementation.
- Do not perform separate review, re-verification, or subagent validation passes unless requested or a test fails.
- Run the smallest relevant validation suite once after changes.
- Delegate only for large, independent workstreams.
- Keep progress updates to one sentence; report changed files, tests run, and any unresolved risk at the end.
- Stop when the acceptance criteria are met.
