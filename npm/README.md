# @thiagoneves/relay

The npm launcher for [relay](https://github.com/thiagoneves/relay), a context
layer for coding agents.

```sh
npx -y @thiagoneves/relay claude     # or codex, gemini
```

On first run it downloads the relay binary for your platform from the matching
GitHub release, checks it against the release's `SHA256SUMS`, and runs it.
`relay setup` then installs it to `~/.local/bin`, where the harness hooks point.
Set `RELAY_BINARY` to use a relay you built yourself.
