//! Compression entry point. Structured filter chosen by command shape,
//! then the generic pipeline. Any panic inside a filter is caught and
//! the raw output is returned: a filter bug must never eat a real error.

pub mod fidelity;
pub mod generic;
pub mod git;
pub mod tests_runner;

pub struct Compressed {
    pub text: String,
    /// Name of the structured filter used, `generic` when none matched.
    pub filter: &'static str,
}

/// First meaningful token of a command, skipping env assignments and
/// wrappers like `sudo`, `rtk`, `npx`, `pnpm exec`.
pub fn head_tokens(cmd: &str) -> Vec<String> {
    let mut toks: Vec<String> = Vec::new();
    for raw in cmd.split_whitespace() {
        if toks.is_empty() {
            if raw.contains('=') && !raw.starts_with('-') {
                continue; // FOO=bar prefix
            }
            if matches!(raw, "sudo" | "rtk" | "time" | "env" | "command" | "exec") {
                continue;
            }
        }
        let t = raw.trim_matches(|c| c == '"' || c == '\'');
        toks.push(t.to_string());
        if toks.len() >= 4 {
            break;
        }
    }
    toks
}

/// Classify a command into a filter name. Only the first command of a
/// chain (`&&`, `;`, `|`) drives the choice; that is where the bulk of
/// the output usually comes from.
pub fn classify(cmd: &str) -> &'static str {
    let first =
        cmd.split("&&").next().unwrap_or(cmd).split("||").next().unwrap_or(cmd).split(';').next().unwrap_or(cmd);
    let piped = first.contains('|');
    let toks = head_tokens(first.split('|').next().unwrap_or(first));
    let t0 = toks.first().map_or("", String::as_str);
    let t1 = toks.get(1).map_or("", String::as_str);
    let t2 = toks.get(2).map_or("", String::as_str);
    match (t0, t1) {
        ("git", "status") if !piped && !first.contains("--porcelain") && !first.contains("-s") => "git-status",
        ("git", "diff" | "show") if !piped && !first.contains("--stat") => "git-diff",
        ("git", "log")
            if !piped && !first.contains("--oneline") && !first.contains("--format") && !first.contains("--pretty") =>
        {
            "git-log"
        }
        ("cargo", "test" | "nextest") => "cargo-test",
        ("go", "test") => "go-test",
        ("pytest", _) | ("python", "-m") if t0 == "pytest" || t2 == "pytest" => "pytest",
        ("npx", "jest" | "vitest") | ("jest" | "vitest", _) => "js-test",
        ("npm" | "pnpm" | "yarn" | "bun", "test") | ("npm" | "pnpm", "run") if t1 != "run" || t2.contains("test") => {
            "js-test"
        }
        ("grep" | "rg" | "ag", _) => "grep",
        _ => "generic",
    }
}

/// Coarse name for reports: `git status`, `cargo test`, `sed`. Leading
/// `cd`/`export` segments and `timeout N` are skipped so the family is
/// the command that produced the output.
pub fn family(cmd: &str) -> String {
    const SETUP: &[&str] = &["cd", "export", "set", "source", ".", "pushd"];
    const VERBED: &[&str] = &[
        "git",
        "cargo",
        "npm",
        "pnpm",
        "yarn",
        "bun",
        "go",
        "docker",
        "kubectl",
        "gh",
        "uv",
        "pip",
        "brew",
        "make",
        "dotnet",
        "terraform",
    ];
    for seg in cmd.split(['&', '|', ';', '\n']).map(str::trim).filter(|s| !s.is_empty()) {
        let mut toks = head_tokens(seg);
        if toks.first().is_some_and(|t| t == "timeout") {
            toks.drain(..2.min(toks.len()));
        }
        let Some(t0) = toks.first() else { continue };
        let t0 = t0.rsplit(['/', '\\']).next().unwrap_or(t0).to_string();
        if SETUP.contains(&t0.as_str()) {
            continue;
        }
        return match toks.get(1) {
            Some(t1) if VERBED.contains(&t0.as_str()) && !t1.starts_with('-') => format!("{t0} {t1}"),
            _ => t0,
        };
    }
    "?".into()
}

pub fn compress(cmd: &str, raw: &str) -> Compressed {
    let filter = classify(cmd);
    let result = std::panic::catch_unwind(|| apply_filter(filter, raw));
    match result {
        Ok(text) => Compressed { text, filter },
        Err(_) => Compressed { text: generic::strip_ansi(raw), filter: "raw" },
    }
}

fn apply_filter(filter: &str, raw: &str) -> String {
    let clean = generic::strip_ansi(raw);
    let structured = match filter {
        "git-status" => git::status(&clean),
        "git-diff" => git::diff(&clean),
        "git-log" => git::log(&clean),
        "cargo-test" => tests_runner::cargo_test(&clean),
        "go-test" => tests_runner::go_test(&clean),
        "pytest" => tests_runner::pytest(&clean),
        "js-test" => tests_runner::js_test(&clean),
        "grep" => generic::group_by_file(&clean).unwrap_or(clean.clone()),
        _ => clean.clone(),
    };
    generic::apply(&structured)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_commands() {
        assert_eq!(classify("git status"), "git-status");
        assert_eq!(classify("git status --porcelain"), "generic");
        assert_eq!(classify("git diff HEAD~1 -- src"), "git-diff");
        assert_eq!(classify("git log -n 20"), "git-log");
        assert_eq!(classify("git log --oneline"), "generic");
        assert_eq!(classify("cargo test --release"), "cargo-test");
        assert_eq!(classify("RUST_LOG=debug cargo test"), "cargo-test");
        assert_eq!(classify("npm test"), "js-test");
        assert_eq!(classify("pnpm run test:unit"), "js-test");
        assert_eq!(classify("pnpm run build"), "generic");
        assert_eq!(classify("rg foo src"), "grep");
        assert_eq!(classify("python -m pytest tests/"), "pytest");
        assert_eq!(classify("ls -la"), "generic");
    }

    #[test]
    fn families_name_the_command_that_produced_output() {
        assert_eq!(family("cd web && pnpm exec vitest run"), "pnpm exec");
        assert_eq!(family("timeout 30 cargo test -q"), "cargo test");
        assert_eq!(family("git -C x status"), "git");
        assert_eq!(family("sed -n '1,80p' src/main.rs"), "sed");
        assert_eq!(family("/usr/bin/python3 x.py"), "python3");
        assert_eq!(family("export A=1; ls -la"), "ls");
    }

    #[test]
    fn never_grows_output() {
        let raw = "x\n".repeat(50);
        let c = compress("echo", &raw);
        assert!(c.text.len() <= raw.len());
    }
}
