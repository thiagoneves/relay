use crate::harness;

#[expect(clippy::unnecessary_wraps, reason = "every command handler returns an exit code the same way")]
pub fn run(name: &str) -> anyhow::Result<i32> {
    // Fail-open by contract: a hook never blocks the harness.
    harness::run_fail_open(name, || harness::by_name(name)?.handle_hook());
    Ok(0)
}
