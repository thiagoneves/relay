use crate::harness;

pub fn run(name: &str) -> anyhow::Result<i32> {
    // Fail-open by contract: a hook never blocks the harness.
    harness::run_fail_open(name, || harness::by_name(name)?.handle_hook());
    Ok(0)
}
