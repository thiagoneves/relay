# relay

A context layer for coding agents. relay makes each Claude Code, Codex,
Gemini CLI or Cursor session start where the last one stopped, keeps long command output from
filling the context, and stores everything in your repo.

- **Continuity.** When a session ends, relay writes a handoff: where the work
  stopped, what you asked, the decisions you made, the files touched, what was
  failing. The next session opens with a short brief built from it.
- **Memory that lives in the repo.** `relay remember` saves a rule, a gotcha or
  a decision as one small file under `.relay/`. Commit it and every session,
  and every teammate, gets it.
- **Compression.** Long shell output reaches the model cut to what matters
  (errors, failing tests, `file:line` references, head and tail). The original
  is one `relay get <id>` away.
- **An audit of context waste.** `relay audit` reads your harness transcripts
  and shows what fills every call: MCP tool schemas, skills, instruction files.

No model calls, no network, no daemon. relay is one binary run by the
harness's hooks.

## Install

Once a release is published, one command sets everything up and starts a
session:

```sh
npx -y @thiagoneves/relay claude      # or codex, gemini
```

The npm package is only a launcher: it downloads the binary for your platform
from the GitHub release and checks it against the release's `SHA256SUMS`.

Until then, build from source (Rust 1.88 or newer):

```sh
git clone <this repo> relay && cd relay
cargo build --release
./target/release/relay setup
```

`relay setup` copies the binary to `~/.local/bin`, puts that directory on your
PATH if needed, and registers hooks in every harness it finds: Claude Code in
`~/.claude/settings.json`, Codex in `~/.codex/hooks.json`, Gemini CLI in
`~/.gemini/settings.json` and Cursor in `~/.cursor/hooks.json`. It keeps a
backup of any file it changes.

## Use

In any git repo:

```sh
relay claude          # or: relay codex, relay gemini
```

That is all day to day. The session gets the brief, output is compressed, and
the handoff is written when it ends. `relay claude --last` resumes the last
session instead of starting a new one. Running `claude` directly works too, once
the hooks are installed; the wrapper adds a closing summary and a handoff even
when the harness exits without firing its hooks.

Other commands:

| Command | What it does |
|---|---|
| `relay remember rule\|gotcha\|decision "<one line>"` | Save an item under `.relay/` |
| `relay get <id>` | Print the original output behind a compressed view |
| `relay status` | What relay saved, what it stores, anything that failed |
| `relay log` | What failed in the hooks, newest last |
| `relay audit` | What fills your context and how to trim it |
| `relay brief` / `relay handoff --show` | What the next session receives |
| `relay purge` | Preview, then with `--yes` delete this worktree's local data |
| `relay uninstall claude\|codex\|gemini\|cursor` | Remove the hooks |

## Where data lives

| Place | What | Shared |
|---|---|---|
| `.relay/` | `project.md` and remembered items | Yes, commit it |
| `.git/relay/` | Session events, handoffs, compressed outputs' originals, log | No, per worktree |
| `$TMPDIR/relay-<uid>/` | Originals from sandboxed runs that cannot write `.git`, until the next hook moves them | No, owner-only |

Nothing leaves your machine. Stored originals are deleted after 30 days;
`relay purge --yes` deletes them at once. They hold command output as it was
printed, so anything a command printed, a token included, is in them too.

## How it works with each harness

relay speaks the hook protocol Claude Code introduced and Codex adopted.

- **Claude Code** runs every command as the agent wrote it, under your own
  permission rules and mode. After a command succeeds, relay swaps in the
  compressed view of its output. Tests, builds, linters and reads are also
  routed through `relay x` and approved, so their output is compressed even when
  they fail; relay never approves anything else. When such a command would hit
  the Bash tool's timeout, `relay x` stops it just before and shows what it
  printed so far (on Unix).
- **Codex, Gemini CLI and Cursor** accept a rewritten command but cannot have
  its output replaced, so there relay compresses reads and the same routine
  development tasks, which it rewrites and approves. Each speaks its own hook
  dialect; relay translates it. Cursor also runs Claude Code's hooks, and relay
  leaves those events to its Cursor hooks so nothing is recorded twice. Cursor
  works from the editor, so there is no `relay cursor`; its hooks do the work.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

`tests/snapshots/` pins what each command prints; after an intended change,
`RELAY_UPDATE_SNAPSHOTS=1 cargo test --test cli_output` rewrites them.
`relay bench --corpus tests/corpus` checks that compression keeps every line a
fixture marks as required. After a Claude Code update,
`cargo test --test e2e_claude -- --ignored` checks against the real harness
that the model still gets relay's compressed view (it makes one small model
call). In daily use relay checks the same thing at the end of each session and
reports a miss in `relay status`.

## License

MIT, see [LICENSE](LICENSE).
