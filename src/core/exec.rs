//! `relay x -- <cmd>`: run a shell command, print a compressed view, keep
//! the original. The core primitive of relay.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::compress;
use crate::core::condense;
use crate::core::paths::Paths;
use crate::core::spool;
use crate::helpers::env::Var;
use crate::helpers::shell;

pub struct Outcome {
    pub exit: i32,
    pub printed: String,
}

/// How to run one command.
#[derive(Default)]
pub struct Options<'a> {
    /// Print the raw output instead of the compressed view.
    pub raw: bool,
    /// The session the output belongs to, when the hook knows it.
    pub session: Option<&'a str>,
    /// Stop the command after this long and show what it printed so far.
    pub stop_after: Option<Duration>,
}

/// What `timeout(1)` exits with when it stops a command.
const STOPPED_EXIT: i32 = 124;

pub fn run(cmd: &str, opts: &Options) -> Result<Outcome> {
    let paths = Paths::from_cwd().ok();
    // Resolved before the command runs: the shared pointer follows
    // whichever session touched the worktree last, and a long command
    // gives a parallel session time to move it.
    let session = opts.session.map(str::to_string).or_else(|| paths.as_ref().and_then(spool::current_session));
    let finished = run_shell(cmd, opts.stop_after)?;
    let mut raw = finished.output;
    if let Some(after) = finished.stopped_after {
        raw.push_str(&format!(
            "\n[relay: stopped after {after:.0?}, before the harness's own timeout; the output so far is above]\n"
        ));
    }
    if opts.raw {
        return Ok(Outcome { exit: finished.exit, printed: compress::generic::strip_ansi(&raw) });
    }
    let cwd = std::env::current_dir().map(|p| p.display().to_string()).unwrap_or_default();
    let run = condense::Run { cmd, cwd: &cwd, exit: finished.exit, session };
    Ok(Outcome { exit: finished.exit, printed: condense::view_of(paths.as_ref(), run, &raw) })
}

struct Finished {
    exit: i32,
    output: String,
    /// Set when relay stopped the command itself.
    stopped_after: Option<Duration>,
}

/// Exit code and combined output. stderr is merged into stdout inside the
/// user's shell, so compilers and test runners keep their natural
/// interleaving. With `stop_after`, the command runs in its own process
/// group so stopping it also stops what it spawned (test binaries, build
/// jobs), which would otherwise keep the output pipe open.
fn run_shell(cmd: &str, stop_after: Option<Duration>) -> Result<Finished> {
    let mut command = if let Some(sh) = shell::posix_shell() {
        let mut c = Command::new(sh);
        c.arg("-c").arg(format!("{{\n{cmd}\n}} 2>&1"));
        c
    } else {
        cmd_shell(cmd)
    };
    command.env(Var::RelayActive.name(), "1").stdin(Stdio::inherit()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let stop_after = stop_after.filter(|_| own_group(&mut command));
    let mut child = command.spawn()?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let (exit, stopped_after) = wait(&mut child, stop_after)?;
    let mut output = String::from_utf8_lossy(&stdout.join().unwrap_or_default()).into_owned();
    output.push_str(&String::from_utf8_lossy(&stderr.join().unwrap_or_default()));
    Ok(Finished { exit, output, stopped_after })
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = pipe {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    })
}

fn wait(child: &mut Child, stop_after: Option<Duration>) -> Result<(i32, Option<Duration>)> {
    let Some(limit) = stop_after else { return Ok((child.wait()?.code().unwrap_or(1), None)) };
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok((status.code().unwrap_or(1), None));
        }
        if Instant::now() >= deadline {
            stop_group(child);
            let _ = child.wait();
            return Ok((STOPPED_EXIT, Some(limit)));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Put the command in its own process group. Only Unix can then stop the
/// whole group without a new dependency, so elsewhere relay never stops a
/// command and leaves timeouts to the harness.
#[cfg(unix)]
fn own_group(command: &mut Command) -> bool {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    true
}

#[cfg(not(unix))]
fn own_group(_: &mut Command) -> bool {
    false
}

/// TERM to the whole group, then KILL to whatever ignored it.
#[cfg(unix)]
fn stop_group(child: &mut Child) {
    let group = format!("-{}", child.id());
    let kill = |signal: &str| Command::new("kill").args([signal, "--", &group]).stderr(Stdio::null()).status();
    let _ = kill("-TERM");
    for _ in 0..20 {
        if child.try_wait().is_ok_and(|s| s.is_some()) {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = kill("-KILL");
}

#[cfg(not(unix))]
fn stop_group(child: &mut Child) {
    let _ = child.kill();
}

/// Windows without Git Bash: the harness ran it in cmd, so do we. The
/// line goes through verbatim; `Command::arg` would apply C-runtime
/// quoting, which cmd does not undo (`--format="%h %s"` would break).
#[cfg(windows)]
fn cmd_shell(cmd: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let mut c = Command::new("cmd");
    c.arg("/C").raw_arg(cmd);
    c
}

/// Unreachable off Windows: `posix_shell` always finds one there.
#[cfg(not(windows))]
fn cmd_shell(cmd: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(cmd);
    c
}

#[cfg(all(test, unix))]
mod unix_tests {
    use super::*;

    #[test]
    fn a_command_past_its_limit_is_stopped_with_what_it_printed() {
        let started = Instant::now();
        let f = run_shell("echo early; sleep 5 & wait; echo late", Some(Duration::from_millis(400))).unwrap();
        assert!(started.elapsed() < Duration::from_secs(3), "the whole group must stop, children included");
        assert_eq!(f.exit, STOPPED_EXIT);
        assert!(f.output.contains("early") && !f.output.contains("late"), "{}", f.output);
    }

    #[test]
    fn a_command_within_its_limit_is_untouched() {
        let f = run_shell("echo done; exit 3", Some(Duration::from_secs(10))).unwrap();
        assert_eq!((f.exit, f.output.trim(), f.stopped_after), (3, "done", None));
    }
}

#[cfg(all(test, windows))]
mod tests {
    #[test]
    fn cmd_gets_quotes_verbatim() {
        let out = super::cmd_shell(r#"echo "a b""#).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), r#""a b""#);
    }
}
