---
type: Gotcha
title: "Cursor runs Claude Code's hooks by default; the Claude adapter skips events that carry cursor_version so nothing fires twice"
timestamp: 2026-09-22T19:35:37Z
generated: {by: "process:relay", at: 2026-09-22T19:35:37Z}
branch: develop
sha: a4cccd5
session: af0f9809-406a-4a5e-92fd-7b8f7b11ddd6
paths: src/harness/cursor/mod.rs
---

Cursor runs Claude Code's hooks by default; the Claude adapter skips events that carry cursor_version so nothing fires twice
