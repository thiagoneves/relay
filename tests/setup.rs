//! `relay setup` on a fresh machine: installs itself, registers hooks,
//! is idempotent, and never edits a startup file it was not asked to.

mod common;

use common::Repo;

#[test]
fn setup_installs_hooks_once_and_leaves_profiles_alone() {
    let repo = Repo::new("setup");
    // Only detection looks at it, so an empty file is enough; `.cmd` is
    // what `which` matches through PATHEXT on Windows.
    let bin = repo.root.join("fakebin");
    std::fs::create_dir_all(&bin).unwrap();
    for name in ["claude", "claude.cmd"] {
        std::fs::write(bin.join(name), "").unwrap();
    }
    let path = std::env::join_paths(
        std::iter::once(bin).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())),
    )
    .unwrap();
    let setup = || repo.isolated(common::relay()).arg("setup").env("PATH", &path).output().unwrap();

    let first = setup();
    let out = String::from_utf8_lossy(&first.stdout);
    assert!(first.status.success(), "{out}{}", String::from_utf8_lossy(&first.stderr));
    assert!(out.contains("claude hooks installed"), "{out}");
    let exe = repo.root.join(".local").join("bin").join(format!("relay{}", std::env::consts::EXE_SUFFIX));
    assert!(exe.is_file(), "stable copy missing");
    let settings = std::fs::read_to_string(repo.root.join("settings.json")).unwrap();
    assert!(settings.contains("SessionStart") && settings.contains("relay"), "{settings}");

    let second = setup();
    let out = String::from_utf8_lossy(&second.stdout);
    assert!(second.status.success() && out.contains("claude hooks already current"), "{out}");
    assert!(!String::from_utf8_lossy(&second.stderr).contains("installed"), "binary recopied on an unchanged run");

    for f in [".zshrc", ".bashrc", ".bash_profile", ".profile"] {
        assert!(!repo.root.join(f).exists(), "{f} written with an unknown shell");
    }
}
