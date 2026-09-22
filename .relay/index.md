---
okf_version: "0.2"
---

# relay · relay memory

## Project

- [project.md](project.md): what a session reads first.

## Rules

- [A fixed list may only describe a tool or a protocol; a person's habits become a rule of shape (word count, structure) or stay out](rules/a-fixed-list-may-only-describe-a-tool-or-a-protocol-a-person.md)
- [Clippy pedantic with a 40-line function limit; comments only for why, always in English](rules/clippy-pedantic-with-a-40-line-function-limit-comments-only.md)
- [Every fix ships with a test that fails before it; a test that touches file paths must pass on Windows too](rules/every-fix-ships-with-a-test-that-fails-before-it-a-test-that.md)
- [One concern per commit, in English, no attribution lines; fold a lint fix into the commit it fixes](rules/one-concern-per-commit-in-english-no-attribution-lines-fold.md)
- [Wait for CI to be green on Linux, macOS and Windows before fast-forwarding main](rules/wait-for-ci-to-be-green-on-linux-macos-and-windows-before-fa.md)

## Gotchas

- [Cursor runs Claude Code's hooks by default; the Claude adapter skips events that carry cursor_version so nothing fires twice](gotchas/cursor-runs-claude-code-s-hooks-by-default-the-claude-adapte.md)
- [Gemini CLI denies the tool call when a hook exits with an unexpected code or prints to stderr; hooks catch panics and always exit 0](gotchas/gemini-cli-denies-the-tool-call-when-a-hook-exits-with-an-un.md)
- [On Windows Path::is_absolute needs a drive letter; a path starting with / is still outside the repo](gotchas/on-windows-path-is-absolute-needs-a-drive-letter-a-path-star.md)

## Decisions

- [Claude Code: compress after the command through PostToolUse updatedToolOutput; rewrite and approve only reads and routine dev tasks (tests, builds, linters)](decisions/claude-code-compress-after-the-command-through-posttooluse-u.md)
- [Codex, Gemini CLI and Cursor cannot replace output, so there relay compresses only what it rewrites and approves](decisions/codex-gemini-cli-and-cursor-cannot-replace-output-so-there-r.md)
- [Memory is one file per item under .relay/; handoffs stay local unless shared with relay handoff --share](decisions/memory-is-one-file-per-item-under-relay-handoffs-stay-local.md)
- [No model call in the default path: the layer costs zero tokens, and handoffs come from events, not summaries](decisions/no-model-call-in-the-default-path-the-layer-costs-zero-token.md)

