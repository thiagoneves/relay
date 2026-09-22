<div align="center">

# relay

**Your agent forgets. Your repo doesn't have to.**

A context layer for coding agents: it compresses what goes in, remembers what
matters, and lives in your repo. One binary, no server, no account, no LLM.

[![ci](https://github.com/thiagoneves/relay/actions/workflows/ci.yml/badge.svg)](https://github.com/thiagoneves/relay/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Claude Code · Codex · Gemini CLI · Cursor

</div>

---

Every session starts over. You explain the architecture again, the agent reads
the same files again, and the decision you settled yesterday is gone. Then a
test run dumps 600 lines into the context window, and the one line that
mattered scrolls past.

relay sits under the agent, in the hooks it already calls, and fixes both.

## What you get

**The next session knows where you stopped.** When a session ends, relay writes
a handoff from what actually happened: where the work stopped, what you asked,
the decisions you made, the files touched, what was failing. The next session
opens with a short brief built from it. Nothing to run, nothing to paste.

**Memory that lives with the code.** `relay remember` saves a rule, a gotcha or
a decision as one small file under `.relay/`. Commit it and every session, and
every teammate, gets it. When the code it describes moves on, relay says so:
_may be stale: src/pay.rs changed since_.

**Long output, short context.** A 400-line script output reached the model as
918 tokens instead of 3,248, with errors, failing tests and `file:line`
references intact. The original is one `relay get <id>` away. Replayed over
one developer's 21,000 real shell calls, relay kept 100% of the signal lines.

**Small enough to hold in your head.** One binary, the hooks your harness
already calls, and two directories: `.relay/` that you commit, `.git/relay/`
that you don't. No server, no account, no database, no background process.
`relay uninstall` takes the hooks back out of your config; `relay purge`
deletes what relay stored.

**Costs you nothing to run.** No model calls in the default path, so the layer
spends zero tokens of your quota. No daemon, no network. Hooks take about
12 ms at the 95th percentile, and `relay status` shows that number, so you never
take it on faith.

## Start

Once the first release is published, one command does everything:

```sh
npx -y @thiagoneves/relay claude      # or codex, gemini
```

relay installs itself to `~/.local/bin`, registers hooks in every harness it
finds, and starts the session.

Until then, from source (Rust 1.88+):

```sh
git clone https://github.com/thiagoneves/relay && cd relay
cargo build --release
./target/release/relay setup
```

Day to day you type one command:

```sh
relay claude      # or relay codex, relay gemini; in Cursor, just open the project
```

## A session, end to end

```console
$ relay claude
# relay brief
# payments-api
## Remembered
- gotcha: Amounts are in cents _(may be stale: src/pay.rs changed since)_
## Last session (2026-09-22, claude-code)
### Where it stopped
Retry added to the payment client; the timeout test still fails.
### Decisions
- Backoff: exponential with jitter
```

The agent runs the tests, which print 600 lines. The model sees the failures,
the panic and the paths, then a footer:

```
[relay 8.2k→1.1k tokens · original: relay get o_1a0c9bd1f7d_3c4]
```

You close the session. relay writes the handoff for the next one.

## Commands

| Command | What it does |
|---|---|
| `relay claude` · `relay codex` · `relay gemini` | Start a session under relay |
| `relay remember rule\|gotcha\|decision "<one line>"` | Save what the next session should know |
| `relay get <id>` | The original output behind a compressed view |
| `relay status` | What relay saved, what it stores, anything that failed |
| `relay audit` | What fills your context, and how to trim it |
| `relay brief` · `relay handoff --show` | What the next session receives |
| `relay log` | What failed in the hooks, newest last |
| `relay purge` | Preview, then with `--yes` delete this worktree's local data |
| `relay uninstall claude\|codex\|gemini\|cursor` | Remove the hooks |

## Where your data lives

| Place | What | Shared |
|---|---|---|
| `.relay/` | Project rules and remembered items | Yes, you commit it |
| `.git/relay/` | Session events, handoffs, originals, logs | No, per worktree |
| `$TMPDIR/relay-<uid>/` | Originals from sandboxed runs, until the next hook moves them | No, owner-only |

Nothing leaves your machine. Originals are deleted after 30 days, or at once
with `relay purge --yes`. Credentials in prompts, commands and replies are
masked before relay stores them; command output is kept as printed, so
`relay get` stays faithful.

## Questions worth asking

**Is this just `/compact`?** No. Compaction summarizes a context that is
already full, with the model, on your quota. relay keeps the context small from
the start, and writes the handoff from what happened rather than from a summary.

**What if a filter eats the error I needed?** Then relay failed. That is why
every original is kept and `relay get` prints it, why `relay bench` measures
kept signal against a corpus of real output, and why the build fails when one
required line goes missing.

**Does it slow the agent down?** `relay status` reports the p95 per hook
against a 50 ms budget. When relay itself breaks, hooks fail open: the harness
never blocks, and the failure shows up in `relay status` and `relay log`.

**How much does it really save?** On one developer's 21,000 shell calls, 3% of
tokens, with no signal lost. Compression is the smaller half of relay. The
larger half is not re-explaining your project every morning.

## How each harness is handled

relay speaks each harness's hook dialect and translates it.

- **Claude Code** runs every command as the agent wrote it, under your own
  permission rules and mode; relay swaps in the compressed view afterwards.
  Tests, builds, linters and reads go through `relay x` instead, which relay
  approves, so their output is compressed even when they fail. `relay x` also
  stops a command just before the harness's timeout, so what was printed still
  reaches the model.
- **Codex, Gemini CLI and Cursor** accept a rewritten command but cannot have
  its output replaced. There relay compresses the same reads and routine
  development tasks, and leaves everything else exactly as the agent wrote it.
  Cursor also runs Claude Code's hooks; relay leaves those events to its Cursor
  hooks, so nothing is recorded twice.

relay only approves what it rewrites: reads and routine dev tasks. `rm -rf`,
`git push` and everything else reach your normal permission prompt untouched.

On Windows, `relay x` needs Git Bash; Codex, Gemini CLI and Cursor run commands
in PowerShell there, so relay compresses nothing for them on Windows.

## Contributing

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

`tests/snapshots/` pins what every command prints; `RELAY_UPDATE_SNAPSHOTS=1`
rewrites them after an intended change. `relay bench --corpus tests/corpus`
measures compression against real output. After a Claude Code update,
`cargo test --test e2e_claude -- --ignored` checks against the real harness
that the model still receives relay's view; in daily use relay checks the same
thing at the end of every session.

## License

MIT. See [LICENSE](LICENSE).
