---
kind: gotcha
created: 2026-09-22T19:35:37Z
branch: develop
sha: a4cccd5
session: af0f9809-406a-4a5e-92fd-7b8f7b11ddd6
paths: src/harness/mod.rs
---

Gemini CLI denies the tool call when a hook exits with an unexpected code or prints to stderr; hooks catch panics and always exit 0
