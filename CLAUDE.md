# CLAUDE.md — clave

@AGENTS.md
@OPUS.md

Your Claude models have been upgraded massively since this project started, your additional skill and capability means you can reassess past work for refactors, simplification, and improvements to keep everything elegant and sensible for the forward momentum and to have a great codebase.
Do this without allowing conversations and implementations to creep massively in scope.

Work directly and proportionately.

- Read only the files necessary to complete the requested change.
- Implement the requested scope only; do not refactor nearby code unless required.
- Make ordinary engineering choices without asking.
- Ask a question only if different interpretations would materially change the implementation.
- Run the relevant validation suite after changes, assessing where the validation suite can be quickly expanded or improved.
- Keep progress concise; report changed files, tests run, and any unresolved risk at the end.
- Stop when the acceptance criteria are met.
- Keep the majority of message content for after you've made tool calls to avoid prose going unseen between calls. Short sentences between, consolidation in the final message of a tool-call stream. That final message is a report, not a summary of everything you did — see OPUS.md § Response style for its shape and length.
