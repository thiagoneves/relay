use std::io::Write;

use crate::core::exec;

pub fn run(cmd: &str, raw: bool, session: Option<&str>) -> anyhow::Result<i32> {
    let out = exec::run(cmd, raw, session)?;
    let mut stdout = std::io::stdout().lock();
    if !out.printed.is_empty() {
        let _ = writeln!(stdout, "{}", out.printed);
    }
    let _ = stdout.flush();
    Ok(out.exit)
}
