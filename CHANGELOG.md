# Changelog

All notable changes to relay. Dates are UTC.

## Unreleased

### Changed

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

- `relay log`, and a failure count in `relay status`: hooks fail open, so this
  is where a broken relay shows up.
- `relay x` stops a command just before the harness's timeout and shows what it
  printed so far (Unix, Claude Code).
- Release archives with checksums, built on a `v*` tag.
- README and the MIT license text.

### Fixed

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
