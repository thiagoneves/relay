use crate::harness::{self, HarnessId};

#[expect(clippy::unnecessary_wraps, reason = "every command handler returns an exit code the same way")]
pub fn run(id: HarnessId) -> anyhow::Result<i32> {
    // Fail-open by contract: a hook never blocks the harness.
    harness::run_fail_open(id.stored(), || harness::protocol::hook::run(id.adapter().as_ref()));
    Ok(0)
}
