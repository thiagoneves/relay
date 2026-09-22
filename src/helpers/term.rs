//! Terminal output: ANSI colours only when stdout is a terminal and
//! `NO_COLOR` is unset, and bars drawn with block characters.

use std::io::IsTerminal;

use super::env::{self, Var};

#[derive(Clone, Copy)]
pub struct Paint {
    on: bool,
}

#[derive(Clone, Copy)]
pub enum Color {
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
}

impl Paint {
    pub fn stdout() -> Self {
        Self { on: std::io::stdout().is_terminal() && !env::is_set(Var::NoColor) }
    }

    fn wrap(self, code: &str, s: &str) -> String {
        if self.on { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
    }

    pub fn bold(self, s: &str) -> String {
        self.wrap("1", s)
    }

    pub fn dim(self, s: &str) -> String {
        self.wrap("2", s)
    }

    pub fn color(self, c: Color, s: &str) -> String {
        let code = match c {
            Color::Red => "31",
            Color::Green => "32",
            Color::Yellow => "33",
            Color::Blue => "34",
            Color::Magenta => "35",
            Color::Cyan => "36",
        };
        self.wrap(code, s)
    }
}

const EIGHTHS: [char; 8] = ['▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];

/// A bar `share` (0–1) of `width` cells long, in eighths of a cell.
pub fn bar(share: f64, width: usize) -> String {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss)]
    let eighths = (share.clamp(0.0, 1.0) * (width * 8) as f64).round() as usize;
    let mut s = "█".repeat(eighths / 8);
    if !eighths.is_multiple_of(8) {
        s.push(EIGHTHS[eighths % 8 - 1]);
    }
    s
}

/// Whole cells for each share, summing to `width`; the largest remainders
/// get the leftover cells.
pub fn split(shares: &[f64], width: usize) -> Vec<usize> {
    #[allow(clippy::cast_precision_loss)]
    let exact: Vec<f64> = shares.iter().map(|s| s.max(0.0) * width as f64).collect();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let mut cells: Vec<usize> = exact.iter().map(|e| e.floor() as usize).collect();
    let mut order: Vec<usize> = (0..shares.len()).collect();
    order.sort_by(|&a, &b| (exact[b] - exact[b].floor()).total_cmp(&(exact[a] - exact[a].floor())));
    for i in order.into_iter().cycle().take(width.saturating_sub(cells.iter().sum())) {
        cells[i] += 1;
    }
    cells
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_use_eighths() {
        assert_eq!(bar(0.5, 4), "██");
        assert_eq!(bar(1.0 / 32.0, 4), "▏");
        assert_eq!(bar(0.0, 4), "");
    }

    #[test]
    fn split_fills_the_width() {
        assert_eq!(split(&[0.5, 0.25, 0.25], 10).iter().sum::<usize>(), 10);
        assert_eq!(split(&[0.27, 0.33, 0.38, 0.02], 40), [11, 13, 15, 1]);
    }
}
