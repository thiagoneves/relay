//! Routine development tasks relay approves when it rewrites them: tests,
//! builds, type checks and linters that report on the code without
//! publishing, installing or deploying anything. They do run project
//! code (build scripts, tests); the user chose to accept that in exchange
//! for compressing their output.

/// Package-script names that only check or build.
const SCRIPTS: &[&str] =
    &["test", "tests", "lint", "build", "check", "typecheck", "type-check", "tsc", "format:check", "fmt:check"];

/// Tools run through npx, pnpm exec and the like.
const TOOLS: &[&str] =
    &["tsc", "eslint", "vitest", "jest", "prettier", "mypy", "pytest", "ruff", "playwright", "phpunit", "credo"];

pub fn is_dev_task(program: &str, args: &[&str]) -> bool {
    let sub = args.first().copied().unwrap_or("");
    match program {
        "cargo" => cargo(sub, args),
        "npm" | "pnpm" | "yarn" | "bun" => package_manager(sub, args),
        "npx" | "bunx" => tool(sub, &args[1.min(args.len())..]),
        "go" => matches!(sub, "test" | "build" | "vet"),
        "python" | "python3" => {
            sub == "-m" && args.get(1).is_some_and(|m| matches!(*m, "pytest" | "mypy" | "unittest"))
        }
        "gradle" | "gradlew" | "mvn" | "mvnw" => {
            !args.is_empty() && args.iter().all(|a| a.starts_with('-') || JVM_GOALS.contains(a))
        }
        "dotnet" | "swift" | "sbt" | "zig" => matches!(sub, "test" | "build" | "compile"),
        "mix" => {
            matches!(sub, "test" | "compile" | "credo" | "dialyzer") || (sub == "format" && args.contains(&"--check"))
        }
        "deno" => matches!(sub, "test" | "lint" | "check") || (sub == "fmt" && args.contains(&"--check")),
        "dart" | "flutter" => matches!(sub, "test" | "analyze" | "build"),
        "composer" => matches!(sub, "test" | "lint" | "validate"),
        _ => tool(program, args),
    }
}

const JVM_GOALS: &[&str] = &["test", "build", "check", "compile", "verify", "assemble", "clean"];

fn cargo(sub: &str, args: &[&str]) -> bool {
    match sub {
        "test" | "build" | "check" | "clippy" | "nextest" | "doc" | "bench" | "tree" | "metadata" => {
            !args.contains(&"--fix")
        }
        "fmt" => args.contains(&"--check"),
        _ => false,
    }
}

fn package_manager(sub: &str, args: &[&str]) -> bool {
    match sub {
        "test" | "t" => true,
        "run" | "run-script" => args.get(1).is_some_and(|s| SCRIPTS.contains(s)),
        "exec" | "x" | "dlx" => args.get(1).is_some_and(|t| tool(t, &args[2..])),
        _ => SCRIPTS.contains(&sub),
    }
}

/// A checker run directly. Flags that rewrite files or keep running
/// disqualify it.
fn tool(program: &str, args: &[&str]) -> bool {
    let fixes = args.iter().any(|a| matches!(*a, "--fix" | "--write" | "-w" | "--watch" | "-u" | "--update"));
    match program {
        "prettier" => args.contains(&"--check") && !fixes,
        "ruff" => matches!(args.first(), Some(&"check")) && !fixes,
        "playwright" => matches!(args.first(), Some(&"test")),
        _ => TOOLS.contains(&program) && !fixes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(cmd: &str) -> bool {
        let words: Vec<&str> = cmd.split_whitespace().collect();
        is_dev_task(words[0], &words[1..])
    }

    #[test]
    fn checks_and_builds_are_tasks() {
        for cmd in [
            "cargo test -q",
            "cargo clippy --all-targets",
            "cargo fmt --check",
            "npm test",
            "pnpm run lint",
            "yarn build",
            "pnpm exec vitest run",
            "npx tsc --noEmit",
            "pytest -x",
            "python3 -m pytest tests",
            "go test ./...",
            "eslint src",
            "prettier --check .",
            "ruff check src",
            "gradlew test",
            "mvn -q verify",
            "dotnet build",
            "mix test",
            "deno test",
            "deno fmt --check",
            "flutter analyze",
            "composer test",
            "phpunit tests",
            "sbt compile",
            "zig build",
        ] {
            assert!(dev(cmd), "{cmd}");
        }
    }

    #[test]
    fn anything_that_changes_or_ships_is_not() {
        for cmd in [
            "cargo run",
            "cargo install ripgrep",
            "cargo publish",
            "cargo fmt",
            "cargo clippy --fix",
            "npm install",
            "npm publish",
            "pnpm run deploy",
            "npx some-package",
            "eslint --fix src",
            "prettier --write .",
            "ruff format src",
            "jest -u",
            "mvn deploy",
            "go run main.go",
            "mix phx.server",
            "deno fmt",
            "deno run app.ts",
            "composer install",
            "python3 script.py",
        ] {
            assert!(!dev(cmd), "{cmd}");
        }
    }
}
