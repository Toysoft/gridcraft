//! Shared-string rich-text runs: assembling a parsed `<si>` (text + run parts) into the model's
//! [`CellRuns`] side-table payload, under the per-cell and per-workbook caps (issue #774).

// Nothing in the crate proper consumes this module yet — the shared-strings reader is wired to
// it by the next task; tests exercise it in the meantime.
#![allow(dead_code)]

use std::sync::Arc;

use gridcraft_model::{CellRuns, MAX_RUNS_PER_CELL, Run, RunFont};

use crate::styles::read_run_font;
use crate::xml::{El, si_parts};

/// A shared-string item: the plain text every cell sharing the item stores, and its rich-text
/// runs when the item has any worth keeping.
pub struct Si {
    pub text: Arc<str>,
    pub runs: Option<CellRuns>,
}

impl Si {
    /// Parses one `<si>`: the text as [`crate::xml::rich_text`] gives it, plus the runs when at
    /// least one carries an `<rPr>` delta, they fit [`MAX_RUNS_PER_CELL`] and `budget`, and they
    /// line up with the text. Over a cap (or misaligned) the runs are dropped and the text kept;
    /// the budget only shrinks by runs actually kept.
    pub fn parse(si: &El, palette: &[u32], budget: &mut u64) -> Si {
        let (text, parts) = si_parts(si);
        let runs = cell_runs(&text, &parts, palette, budget);
        Si { text: Arc::from(text), runs }
    }
}

/// [`Si::parse`] as a tuple.
pub fn si_cell_runs(si: &El, palette: &[u32], budget: &mut u64) -> (Arc<str>, Option<CellRuns>) {
    let Si { text, runs } = Si::parse(si, palette, budget);
    (text, runs)
}

/// Assembles the parsed run parts, enforcing the caps. Never an error: hostile input loses its
/// runs and keeps its text.
fn cell_runs(text: &str, parts: &[crate::xml::SiRun<'_>], palette: &[u32], budget: &mut u64) -> Option<CellRuns> {
    // Runs without a single <rPr> carry no formatting information.
    if parts.is_empty() || !parts.iter().any(|p| p.rpr.is_some()) {
        return None;
    }
    let n = parts.len() as u64;
    if parts.len() > MAX_RUNS_PER_CELL || *budget < n {
        return None;
    }
    let runs: Vec<Run> = parts
        .iter()
        .map(|p| Run {
            start: p.start,
            len: p.len,
            font: match p.rpr {
                Some(rpr) => read_run_font(rpr, palette),
                None => RunFont { name: None, size: None, bold: None, italic: None, underline: None, strike: None, color: None, vert: None },
            },
        })
        .collect();
    let cr = CellRuns { runs };
    if !cr.valid_for(text) {
        return None;
    }
    *budget -= n;
    Some(cr)
}
