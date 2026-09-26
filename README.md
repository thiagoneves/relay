<div align="center">

# relay

**Your agent forgets. Your repo doesn't have to.**

A context layer for coding agents. What one session learned, the next one
gets, whichever agent you open: Claude Code, Codex, Gemini CLI or Cursor.
It lives in your repo. One binary, no server, no account, no LLM.

[![ci](https://github.com/thiagoneves/relay/actions/workflows/ci.yml/badge.svg)](https://github.com/thiagoneves/relay/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

Claude Code · Codex · Gemini CLI · Cursor

</div>

---

Every session starts over. You explain the architecture again, the agent reads
the same files again, and the decision you settled yesterday is gone. Switch
from Claude Code to Codex and even the little it remembered stays behind. Then
a test run dumps 600 lines into the context window, and the one line that
mattered scrolls past.

relay sits under the agent, in the hooks it already calls, and fixes all of it.

## What you get

**Pick up where you left off, in any agent.** When a session ends, relay writes
a handoff from what actually happened: where the work stopped, what you asked,
the decisions you made, the files touched, what was failing. The next session
opens with a short brief built from it, whether it runs in Claude Code, Codex,
Gemini CLI or Cursor, and it lists what else happened here in the last days:
a session in another agent, a teammate's shared handoff. Your memory belongs
to the repo, not to a vendor. Nothing to run, nothing to paste.

**Memory that lives with the code.** `relay remember` saves a rule, a gotcha or
a decision as one small file under `.relay/`. Commit it and every session, and
every teammate, gets it; it shows up in a pull request like any other change.
`.relay/` is an [Open Knowledge Format](https://okf.md) bundle, so anything
that reads OKF reads relay's memory, and relay asks nothing extra of you.
`relay compile` lists what recent sessions decided and lets you keep it with
one flag. The bar for an item: it would change what the next session does, and
it cannot be read from the code. A trap, a settled choice, a rule the code does
not show. Not principles, not what a linter already enforces. When the code an item describes moves on, relay says so:
_may be stale: src/pay.rs changed since_. An item can carry an expiry, for the
gotcha that only holds until the next upgrade.

**Long output, short context.** A 400-line script output reached the model as
918 tokens instead of 3,248, with errors, failing tests and `file:line`
references intact. The original is one `relay get <id>` away. Replayed over
one developer's 21,000 real shell calls, relay kept 100% of the signal lines.

**Sessions that share a checkout see each other.** Every edit a harness
reports claims its file for that session, in a small file under
`.git/relay/claims/` that expires two hours after the last edit. The brief
names the other sessions working here, what each is editing
(`apps/backstage/**`) and which uncommitted files are whose; before an edit to
a file another live session just changed, Claude Code gets a one-line note
beside the tool call. It never blocks the edit. Three sessions in one checkout
once spent about a million tokens on rebases and ports; this is the cheap
half of that fix. The other half is `git worktree add`.

**Big files are read by section.** A 345 KB plan doc is about 100k tokens, and
agents read it whole to find one task. A whole-file read of a text file over
60 KB is turned down with the file's outline: Markdown headings, or a code
file's top-level blocks, each with its line range, and the offset and limit to
read one. Ranged reads always go through. Outlines come from an index under
`.git/relay/index/` that is rebuilt only for files whose content changed.
Reading a file again costs little too: in Claude Code, a re-read of text the
agent already has becomes a one-line note, and one after a change becomes the
diff since, per session and per subagent, forgotten at each compaction.

`relay brief T-253` goes straight to a task: the section whose heading starts
with the id, the sections that name it, the remembered items that mention it,
the files it will likely touch (changed by commits naming it, edited in turns
that asked about it) and what is in the way: other sessions on those files,
branches not merged yet, and an order to integrate in. About 1,500 tokens.

**Subagents get the same treatment.** Claude Code runs relay's hooks inside
subagents, so their shell output is compressed and their big reads guarded;
a subagent also starts with a short brief (memory and the other sessions
here), and its cost is its own. `relay usage` lists who spent the context,
each session and each subagent on its own line: context sent and output,
exact, from the harness transcript, and the biggest reads. Twenty subagents
once spent 9M tokens in a day, 300k to 650k each; this is where that shows.
While it happens, an agent gets a note each time its tool output passes
another 150k tokens, and `relay usage --history` shows what each task id cost,
day by day.

**Test runs that pass take one line.** A green cargo test, jest, vitest or
Playwright run shows its counts; a failing one keeps every failure with its
`file:line` and the totals. Playwright and `tsc` have filters of their own.

**Budgets for what agents read over and over.** `relay lint` flags a Markdown
doc over 100 KB, an instruction file or memory index over 8 KB, a finished
task (checked box or check mark) that kept more than one line, an ADR
implementation section over 10 lines and a commit subject over 72 characters.
It exits 1, so it works as a git hook: `relay lint --staged` in `pre-commit`,
`relay lint --commit-msg "$1"` in `commit-msg`. `relay compile --hygiene` names
memory items to merge, check, shorten or drop.

**Small enough to hold in your head.** One binary, the hooks your harness
already calls, and two directories: `.relay/` that you commit, `.git/relay/`
that you don't. No server, no account, no database, no background process, no
config file. `relay uninstall` takes the hooks back out of your config;
`relay purge` deletes what relay stored.

**Nothing happens where you can't see it.** Every compressed view names its
original. `relay status` shows what was saved, how often the compressed view
was not enough and the agent fetched the original, how long the hooks took,
and anything that failed. At the end of each session relay checks that the
model really received what it swapped in, and reports it if not. The memory is
plain files under version control. Config files relay edits are backed up
first. And when relay itself breaks, hooks fail open: the agent never blocks.

**See what is eating your context.** `relay audit` reads your harness's own
transcripts and shows what every call carries: MCP tool schemas, skill
listings, instruction files, hook output, with the share each one costs and a
concrete step to trim it. Costs, not verdicts; you decide what stays.

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

Plain `claude` keeps working too, once the hooks are in: `relay claude` only
adds a closing summary and a handoff even when the harness exits without
firing its hooks.

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
| `relay remember rule\|gotcha\|decision "<one line>"` | Save what the next session should know (`--path`, `--until 30d`) |
| `relay compile` | List what recent sessions decided; `--save 1,3` or `--save all` keeps it |
| `relay get <id>` | The original output behind a compressed view |
| `relay status` | What relay saved, what it stores, anything that failed |
| `relay audit` | What fills your context, and how to trim it |
| `relay brief` · `relay handoff --show` | What the next session receives |
| `relay brief <task id or words>` | One page for one task: its plan section, memory and files |
| `relay usage` | Who spent the context, per session and subagent (`--since 30d`); `--history` per task |
| `relay lint` | Docs, instruction files and commit subjects over budget (`--staged`, `--commit-msg`, `--all`) |
| `relay compile --hygiene` | Memory items to merge, check, shorten or drop |
| `relay handoff --share` | Put a session's handoff in `.relay/` for the team, credentials masked |
| `relay init --local` | Keep memory in `.git/relay/` for a repo you cannot commit to |
| `relay log` | What failed in the hooks, newest last |
| `relay purge` | Preview, then with `--yes` delete this worktree's local data |
| `relay uninstall claude\|codex\|gemini\|cursor` | Remove the hooks |

## Where your data lives

| Place | What | Shared |
|---|---|---|
| `.relay/` | An OKF bundle: project rules, remembered items, handoffs you chose to share, and an `index.md` | Yes, you commit it |
| `.git/relay/` | Session events, handoffs, originals, path claims, the section index, read hashes, per-agent totals, logs | No, per worktree |
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

**How are those numbers measured?** Every number relay shows says how. Token
counts marked _estimated_ come from a calibrated estimator, about 7% off a
real tokenizer. `relay bench --history` replays the filters on your own past
shell output and counts the signal lines kept. Numbers marked _exact_ come
from the usage your harness recorded in its transcript, and at the end of each
session relay checks in that transcript that the model received the view it
swapped in.

**Does it work with agents that have no hooks?** Partly. `relay init` points
`AGENTS.md` (and `CLAUDE.md`, when present) at `.relay/`, so any agent that
reads those files gets the rules, gotchas, decisions and shared handoffs, and
is told how to add one. Compression, the brief and automatic handoffs need
the hooks.

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

What each harness gets beyond compression and the brief:

| | Claude Code | Codex | Gemini CLI | Cursor |
|---|---|---|---|---|
| Read guard (outline for a big whole-file read) | yes, `Read` | no read tool; `cat` is compressed | yes, `read_file` | no: its read hook cannot tell the agent why |
| Path claims | from `Write`, `Edit`, `MultiEdit`, `NotebookEdit` | not yet: patches do not reach its hooks | from `write_file`, `replace` | from the edit tools it reports |
| Note before editing a claimed file | yes | no | no | no |
| Re-read as a note or a diff | yes | no | no: its output cannot be replaced | no |
| Note when an agent's tool output adds up | yes, per subagent | no | main thread | no |
| Subagents | same hooks, own brief, own line in `relay usage` | no subagent hooks | not verified | not verified |

Claims are per worktree: sessions in separate worktrees do not collide on
disk, so they do not see each other's claims.

relay only approves what it rewrites: reads and routine dev tasks. `rm -rf`,
`git push` and everything else reach your normal permission prompt untouched.

On Windows, `relay x` needs Git Bash; Codex, Gemini CLI and Cursor run commands
in PowerShell there, so relay compresses nothing for them on Windows.

## Roadmap

What comes next, and what is not planned, is in [ROADMAP.md](ROADMAP.md).

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
