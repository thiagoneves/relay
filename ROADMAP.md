# Roadmap

What relay will do next, in rough order. Each item stays small and keeps
the five truths: under a minute to the first saved token, the next session
opens with the right brief on its own, no hook over 50 ms, no model call in
the default path, no process left running.

## Next

- **First release.** Tag `v0.1.0`; the release workflow builds the archives
  and checksums, and `npx -y @thiagoneves/relay` starts working.
- **Homebrew.** A tap with a formula that installs the release binary, so
  `brew install thiagoneves/tap/relay` is an option next to `npx`.
- **Partial output on Windows.** `relay x` stops a command before the
  harness's timeout on Unix only; Windows needs a process-group equivalent.

## Later

- **More harnesses.** OpenCode and Copilot CLI have lifecycle hooks; each is
  one adapter under `src/harness/` now that dialect translation exists. They
  wait until the current four have earned their keep in daily use.
- **A data-handling page.** What relay stores, where, for how long, what is
  masked, what never leaves the machine. All of it is true today; the page
  makes it easy to check.

- **Claims for Codex and Cursor.** Codex's patches and Cursor's reads do not
  reach their hooks in a shape relay can act on; each needs its harness to
  report them first.

## Not planned

- Locks. Claims warn and never block an edit; a session that must own a
  path works in its own worktree.
- A server, an account, a database or a web UI.
- Search over memory (embeddings, full-text). Memory stays small and curated.
- A model in the default path, for summaries or anything else.
