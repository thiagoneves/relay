# Changelog

All notable changes to relay. Dates are UTC.

## Unreleased

### Changed

- Signal lines, which the compression gate must keep, no longer include a
  zero count (`0 failed`) or a line that starts with a runner's pass mark.
- Claude Code runs every command as the agent wrote it. relay compresses the
  output after the command succeeds, so permission rules, auto mode and the
  worktree guard see the real command. Tests, builds, linters and reads still
  go through `relay x`, approved, so their failures are compressed too.
- relay approves only the commands it rewrites (reads and routine development
  tasks) and leaves every other command untouched for the harness to judge.
  Before, every rewritten command skipped the permission prompt.
- `git` subcommands that change the repository are never rewritten.
- A leading `cd` or `export` stays in the harness's shell; only the rest runs
  under `relay x`.
- Every command's output goes through one presentation layer: a mark for each
  outcome and a next step. Errors say what to do.
- The brief and `--last` skip headless sessions (`claude -p`, SDK apps) when an
  interactive one exists.

### Added

- Path claims: each edit claims its file for the session under
  `.git/relay/claims/`, live for two hours. The brief names the other
  sessions in the checkout, what they are editing and which uncommitted
  files are whose; Claude Code gets a note before editing a file another live
  session changed. On 26/09 three sessions in one checkout spent about 1M
  tokens on rebases and ports.
- Read guard: a whole-file read of a text file over 60 KB (Claude Code
  `Read`, Gemini CLI `read_file`) gets the file's outline with line ranges
  instead; ranged reads go through. A 345 KB plan doc (~100k tokens) was read
  whole by many agents that needed one section; Bash reads were already
  compressed (8,090 → 2,798 tokens in the same session), `Read` was not.
- Subagents: Claude Code's `SubagentStart` gives a subagent a short brief,
  tool events carry its id and `SubagentStop` records its transcript.
- `relay brief <query>`: the plan section for a task id or words, the memory
  that mentions it and the files sessions edited for it, about 1,500 tokens.
- `relay usage`: context per session and per subagent, exact from the
  transcript or estimated from tool output, the biggest reads and what relay
  kept out. On 26/09 about 20 subagents spent ~9M tokens, 300k–650k each.
- Re-reads in Claude Code: the same text again becomes a one-line note, a
  changed one the diff since, per session and subagent, keyed by content
  hash under `.git/relay/reads/`; a compaction forgets them.
- A section index under `.git/relay/index/`: outlines and task ids per file,
  rebuilt only when the content changes. The read guard and
  `relay brief <query>` answer from it (1.6 s cold, 0.12 s warm on 232 docs).
- `relay brief <query>` lists the files the task will likely touch, from
  commits naming it and the turns that asked about it, and what is in the
  way: live sessions on those files, unmerged branches, an order to
  integrate in.
- A note to the agent each time its tool output passes another 150k tokens,
  and `relay usage --history`: each task's cost per day.
- Green cargo test, jest, vitest and Playwright runs in one line; Playwright
  and `tsc` filters.
- `relay lint`: size budgets for plan docs, finished-task notes, ADR
  implementation sections, instruction files, the memory index and commit
  subjects; exits 1, for git hooks.
- `relay compile --hygiene`: memory items to merge, check, shorten or drop,
  and how many the brief leaves out.

- Structured filters for RSpec, ExUnit (`mix test`), PHPUnit, Minitest and
  `dart test`/`flutter test`: passing lines go, every failure line stays.
- `.relay/` is an Open Knowledge Format (v0.2) bundle: every item, the
  project file and shared handoffs carry `type`, `title`, `timestamp` and
  `generated`; an `index.md` lists them. `relay init` migrates items written
  before.
- `relay init` points `AGENTS.md` (and an existing `CLAUDE.md`) at `.relay/`,
  so agents without hooks read the memory too; `--no-agents-md` skips it.
- `relay init --terse` adds an Answers section to `project.md` asking the
  agent for shorter replies.
- `relay status` labels each number as estimated or exact.
- `relay compile`: the decisions of recent sessions that memory does not
  hold yet, kept with `--save`.
- `relay handoff --share` copies a session's handoff into `.relay/`,
  credentials masked; the brief reads shared handoffs too.
- `relay init --local` keeps memory in `.git/relay/` for a repo you cannot
  commit to.
- `relay remember --until 30d` gives an item an expiry.
- The brief lists the other sessions of the last week, one line each.
- `relay status` opens with one headline: tokens kept out of the context,
  sessions handed off, items remembered.
- Gemini CLI and Cursor adapters, each translating its own hook dialect, and
  `relay gemini`.
- Remembered items whose paths changed since they were saved are marked "may
  be stale" in the brief and counted in `relay status`.
- Hooks time themselves; `relay status` shows the p95 against the budget.
- At session end relay checks that the model saw the compressed output it
  swapped in, and reports a miss in `relay status`.
- `relay log`, and a failure count in `relay status`: hooks fail open, so this
  is where a broken relay shows up.
- `relay x` stops a command just before the harness's timeout and shows what it
  printed so far (Unix, Claude Code).
- Release archives with checksums, built on a `v*` tag, and an npm launcher
  for `npx -y @thiagoneves/relay` that downloads and verifies them.
- README and the MIT license text.

### Fixed

- An image read counted its base64 text (~350k tokens a screenshot) instead
  of the image the model sees.
- Compression kept every line a fixture marks as required: chained commands,
  `git diff --name-only`, `git diff` with SQL comments, deleted and binary
  files, `git log -p`, hyperlinks in colored output, grep indentation, reads
  through `awk`, `jq` and `git show`, and jest, vitest and mocha failures.
- Two sessions in one worktree no longer mix outputs, and the wrapper closes
  its own session, not the newest.
- Installing hooks keeps the order of `settings.json` and `hooks.json`, every
  entry relay does not own, symlinks and file permissions. An unreadable or
  unusual Codex `config.toml` is never blanked or made invalid.
- Setup no longer aborts on a read-only shell profile and respects an existing
  `.profile` for bash on macOS.
- The handoff reads only the end of large transcripts, keeps prompts typed in
  the same second, leaves subagent reports out of what was asked, and reads
  `.relay/` files with CRLF line endings.
- `relay audit` finds sessions started below the repo root and judges project
  config against the project's own newest session.
- Stored outputs are written atomically, deduplicated in linear time and
  deleted after 30 days.

### Security

- Credentials in prompts, commands and replies (auth headers, key=value
  secrets, URL passwords, common token shapes) are masked before they reach
  the spool and the handoff.
- Originals spilled from sandboxed runs live in a per-user, owner-only
  directory, and relay refuses one another user could have planted.
