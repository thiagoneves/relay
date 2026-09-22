#!/usr/bin/env bash
# Regenerate tests/corpus from real tool runs. Each fixture is
# <name>.cmd (the command), <name>.out (its combined output). The
# hand-written <name>.keep files are left untouched.
# Needs: cargo, git, rg, python3 + pytest, go, node.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/tests/corpus"
WORK="$(cd "$(mktemp -d)" && pwd -P)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$OUT"

capture() { # name, dir, command
  local name=$1 dir=$2 cmd=$3
  printf '%s\n' "$cmd" > "$OUT/$name.cmd"
  (cd "$dir" && NO_COLOR=1 CARGO_TERM_COLOR=never bash -c "$cmd" 2>&1) | sed "s#$WORK#/work#g" > "$OUT/$name.out"
}

# cargo test: 80 passing tests, 2 failing with panics.
cargo new -q --lib "$WORK/calc" && {
  {
    echo '#[cfg(test)] mod tests {'
    for i in $(seq 1 80); do echo "  #[test] fn adds_$i() { assert_eq!($i + 1, $((i + 1))); }"; done
    echo '  #[test] fn divides_by_zero() { let d = 0; assert_eq!(10 / std::hint::black_box(d), 0); }'
    echo '  #[test] fn parses_config() { let v: Result<u8, _> = "300".parse(); v.expect("config port must fit in u8"); }'
    echo '}'
  } > "$WORK/calc/src/lib.rs"
  (cd "$WORK/calc" && cargo build -q --tests 2>/dev/null)
  capture cargo-test-fail "$WORK/calc" "cargo test"
}

# cargo build with type errors.
cargo new -q "$WORK/broken" && {
  cat > "$WORK/broken/src/main.rs" <<'RS'
struct Config { port: u16, host: String }
fn load() -> Config { Config { port: "8080", host: 42 } }
fn main() { let c = load(); println!("{}", c.missing); }
RS
  capture cargo-build-error "$WORK/broken" "cargo build"
}

# pytest: 60 passing, 2 failing.
mkdir -p "$WORK/py" && {
  { for i in $(seq 1 60); do echo "def test_ok_$i(): assert $i == $i"; done
    echo 'def test_totals(): assert sum([1, 2, 3]) == 7, "invoice total mismatch"'
    echo 'def test_lookup(): assert {"a": 1}["b"] == 1'
  } > "$WORK/py/test_app.py"
  capture pytest-fail "$WORK/py" "pytest"
}

# go test -v: 40 passing, 1 failing.
mkdir -p "$WORK/gomod" && {
  (cd "$WORK/gomod" && go mod init example.com/shop >/dev/null 2>&1)
  { echo 'package shop'; echo 'import "testing"'
    for i in $(seq 1 40); do echo "func TestOk$i(t *testing.T) {}"; done
    echo 'func TestDiscount(t *testing.T) { got := 90; if got != 85 { t.Fatalf("discount: got %d, want %d", got, 85) } }'
  } > "$WORK/gomod/shop_test.go"
  capture go-test-fail "$WORK/gomod" "go test -v ./..."
}

# node --test: 30 passing, 1 failing.
mkdir -p "$WORK/node" && {
  { echo "const test = require('node:test'); const assert = require('node:assert');"
    for i in $(seq 1 30); do echo "test('ok $i', () => assert.ok(true));"; done
    echo "test('rejects expired token', () => assert.strictEqual(Date.now() > 0, false, 'token accepted after expiry'));"
  } > "$WORK/node/app.test.js"
  capture node-test-fail "$WORK/node" "node --test"
}

# git: a repo with history, a dirty tree and a multi-file diff.
mkdir -p "$WORK/repo" && (
  cd "$WORK/repo" && git init -q && git config user.email a@b.c && git config user.name dev
  mkdir -p src docs
  for i in $(seq 1 25); do
    printf 'line %s\n' $(seq 1 40) > "src/mod_$((i % 6)).rs"; echo "change $i" >> "src/mod_$((i % 6)).rs"
    git add -A && git commit -q -m "Change $i: adjust module $((i % 6))" -m "Longer body for change $i."
  done
  for f in 0 1 2 3; do sed -i '' "s/^line 1\$/line one/; s/^line 20\$/return Err(Error::Timeout) \/\/ line 20/" "src/mod_$f.rs"; done
  for i in $(seq 1 12); do echo "draft $i" > "docs/note_$i.md"; done
  git rm -q src/mod_5.rs
)
capture git-status "$WORK/repo" "git status"
capture git-diff "$WORK/repo" "git diff"
capture git-log "$WORK/repo" "git log -n 20"

# ripgrep over relay's own sources.
capture rg-fn "$ROOT" "rg -n 'pub fn' src"

# A long build log with one error in the middle (webpack/gradle shape).
capture long-build-log "$WORK" "for i in \$(seq 1 1500); do echo \"[build] compiled module \$i/3000 in 12ms\"; if [ \$i = 700 ]; then echo 'ERROR in src/pages/checkout.tsx:42:7 TS2322: Type string is not assignable to type number'; fi; done"

# git: file statuses, a SQL comment removed, a rename, a binary; and
# shapes that are not a plain patch.
mkdir -p "$WORK/shapes" && (
  cd "$WORK/shapes" && git init -q && git config user.email a@b.c && git config user.name dev
  mkdir -p db src
  { echo 'create table users (id int);'; echo '-- drop users table'; for i in $(seq 1 30); do echo "insert into users values ($i);"; done; } > db/schema.sql
  for i in $(seq 1 12); do printf 'fn f%s() {}\n' $(seq 1 20) > "src/mod_$i.rs"; done
  printf 'fn old() {}\n' > src/legacy.rs
  printf 'fn keep() {}\n%.0s' $(seq 1 30) > src/moved.rs
  git add -A && git commit -q -m "Initial schema and modules"
  for i in $(seq 1 6); do
    echo "fn extra_$i() {}" >> "src/mod_$i.rs"
    git add -A && git commit -q -m "Add extra_$i" -m "Body of change $i."
  done
  sed -i '' '/^-- drop users table$/d' db/schema.sql
  for i in $(seq 1 12); do sed -i '' "s/^fn f5() {}$/fn f5() -> Result<(), Error> { Err(Error::Timeout) }/" "src/mod_$i.rs"; done
  git rm -q src/legacy.rs
  git mv src/moved.rs src/renamed.rs
  printf '\x89PNG\r\n\x1a\n\x00\x00' > logo.png
  git add -A
)
capture git-diff-shapes "$WORK/shapes" "git diff --cached"
capture git-diff-name-only "$WORK/shapes" "git diff --cached --name-only"
capture git-log-patch "$WORK/shapes" "git log -p -n 3"

# A chain: a diff, then a failing cargo test.
cargo new -q --lib "$WORK/chain" && (
  cd "$WORK/chain" && git init -q && git config user.email a@b.c && git config user.name dev
  {
    echo '#[cfg(test)] mod tests {'
    for i in $(seq 1 60); do echo "  #[test] fn adds_$i() { assert_eq!($i + 1, $((i + 1))); }"; done
    echo '  #[test] fn rounds_totals() { assert_eq!(7 / 2, 4, "rounding must go up"); }'
    echo '}'
  } > src/lib.rs
  git add -A && git commit -q -m init
  sed -i '' 's/adds_1()/adds_one()/' src/lib.rs
  cargo build -q --tests 2>/dev/null
)
capture chain-diff-test "$WORK/chain" "git diff && cargo test"

echo "corpus written to $OUT"
