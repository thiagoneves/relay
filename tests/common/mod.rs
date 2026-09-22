#![allow(dead_code, reason = "shared by several test crates, each uses a subset")]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

pub fn relay() -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_relay"));
    c.env_remove("RELAY_DISABLE");
    c
}

/// A throwaway git repo with one commit, removed on drop.
pub struct Repo {
    pub root: PathBuf,
}

impl Repo {
    pub fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("relay-it-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["-c", "user.email=a@b.c", "-c", "user.name=t", "commit", "-q", "--allow-empty", "-m", "init"],
        ] {
            assert!(Command::new("git").args(args).current_dir(&root).status().unwrap().success());
        }
        Self { root }
    }

    pub fn run(&self, args: &[&str]) -> Output {
        self.isolated(relay()).args(args).output().unwrap()
    }

    /// Deliver one hook event the way a harness does: JSON on stdin.
    pub fn hook(&self, harness: &str, event: serde_json::Value) -> String {
        let mut event = event;
        event["cwd"] = self.root.display().to_string().into();
        let mut child = self
            .isolated(relay())
            .args(["hook", harness])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(event.to_string().as_bytes()).unwrap();
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success(), "hook must never fail");
        String::from_utf8(out.stdout).unwrap()
    }

    /// Run inside the repo with every home-like variable pointing at it.
    fn isolated(&self, mut c: Command) -> Command {
        c.current_dir(&self.root);
        for var in ["HOME", "USERPROFILE", "LOCALAPPDATA", "XDG_DATA_HOME", "CODEX_HOME", "CLAUDE_CONFIG_DIR"] {
            c.env(var, &self.root);
        }
        c
    }

    pub fn path(&self, rel: &str) -> String {
        Path::new(&self.root).join(rel).display().to_string()
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
