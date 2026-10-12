//! Rich-text runs for a cell: per-range font deltas over a plain text value (issue #774).
//! The cell's `Value::Text` stays a plain string; the runs live in a side-table and only
//! describe how to draw ranges of it.

use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};

use crate::style::{Color, Font, Underline, VertAlign};

/// Upper bound on runs stored for one cell. Enforced where runs enter the model (xlsx import,
/// editing commands); `CellRuns::valid_for` only checks geometry.
pub const MAX_RUNS_PER_CELL: usize = 8_192;

/// A font delta: `None` fields inherit the cell style's font via [`RunFont::merged`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RunFont {
    pub name: Option<String>,
    pub size: Option<f32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<Underline>,
    pub strike: Option<bool>,
    pub color: Option<Color>,
    pub vert: Option<VertAlign>,
}

impl Eq for RunFont {}
impl Hash for RunFont {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.name.hash(h);
        self.size.map(f32::to_bits).hash(h);
        (self.bold, self.italic, self.underline, self.strike, self.color, self.vert).hash(h);
    }
}

impl RunFont {
    /// This delta applied over `base`: `None` fields take `base`'s value.
    pub fn merged(&self, base: &Font) -> Font {
        Font {
            name: self.name.clone().unwrap_or_else(|| base.name.clone()),
            size: self.size.unwrap_or(base.size),
            bold: self.bold.unwrap_or(base.bold),
            italic: self.italic.unwrap_or(base.italic),
            underline: self.underline.unwrap_or(base.underline),
            strike: self.strike.unwrap_or(base.strike),
            color: self.color.unwrap_or(base.color),
            vert: self.vert.unwrap_or(base.vert),
        }
    }
}

/// One run: `len` bytes of the cell's text starting at byte `start`, drawn with `font`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub start: u32,
    pub len: u32,
    pub font: RunFont,
}

impl Hash for Run {
    fn hash<H: Hasher>(&self, h: &mut H) {
        (self.start, self.len).hash(h);
        self.font.hash(h);
    }
}

/// The runs of one cell, covering its text value (see [`CellRuns::valid_for`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellRuns {
    pub runs: Vec<Run>,
}

impl Hash for CellRuns {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.runs.hash(h);
    }
}

/// Error from [`CellRuns::format_range`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunFormatError {
    /// The result would need more than [`MAX_RUNS_PER_CELL`] runs.
    Cap,
}

/// `delta` field-wise over `base`: every `Some` field of `delta` wins, `None` keeps `base`'s.
fn overlay(base: &RunFont, delta: &RunFont) -> RunFont {
    RunFont {
        name: delta.name.clone().or_else(|| base.name.clone()),
        size: delta.size.or(base.size),
        bold: delta.bold.or(base.bold),
        italic: delta.italic.or(base.italic),
        underline: delta.underline.or(base.underline),
        strike: delta.strike.or(base.strike),
        color: delta.color.or(base.color),
        vert: delta.vert.or(base.vert),
    }
}

impl CellRuns {
    /// True when the runs are in order, non-overlapping, on UTF-8 boundaries and concatenate to
    /// exactly `text`. An empty run list is never valid.
    pub fn valid_for(&self, text: &str) -> bool {
        if self.runs.is_empty() {
            return false;
        }
        let mut total: usize = 0;
        for run in &self.runs {
            if run.start as usize != total {
                return false;
            }
            let end = match total.checked_add(run.len as usize) {
                Some(end) => end,
                None => return false,
            };
            if text.get(total..end).is_none() {
                return false;
            }
            total = end;
        }
        total == text.len()
    }

    /// The text of run `i`: `None` when the index is out of bounds or the run's byte range is
    /// out of bounds or not on UTF-8 boundaries.
    pub fn run_text<'a>(&self, text: &'a str, i: usize) -> Option<&'a str> {
        let run = self.runs.get(i)?;
        let start = run.start as usize;
        let end = start.checked_add(run.len as usize)?;
        text.get(start..end)
    }

    /// Formats byte `range` of the cell's text by overlaying `delta` field-wise on the runs it
    /// covers, splitting runs that straddle the range edges (the pieces keep the parent font)
    /// and coalescing neighbours that end up equal. Returns `Ok(None)` when the result is flat —
    /// an empty list, or one run covering all of `text` with an all-`None` font — and
    /// [`RunFormatError::Cap`] when the result would exceed [`MAX_RUNS_PER_CELL`]. Never panics:
    /// pieces are plain byte arithmetic, so a misuse (range off the text or its boundaries)
    /// just reassembles the runs.
    pub fn format_range(&self, text: &str, range: std::ops::Range<usize>, delta: &RunFont) -> Result<Option<CellRuns>, RunFormatError> {
        let mut runs: Vec<Run> = Vec::with_capacity(self.runs.len() + 2);
        for old in &self.runs {
            let rs = old.start as usize;
            let re = rs.saturating_add(old.len as usize);
            let from = range.start.clamp(rs, re);
            let to = range.end.clamp(from, re);
            if from > rs {
                runs.push(Run { start: old.start, len: (from - rs) as u32, font: old.font.clone() });
            }
            if to > from {
                runs.push(Run { start: from as u32, len: (to - from) as u32, font: overlay(&old.font, delta) });
            }
            if re > to {
                runs.push(Run { start: to as u32, len: (re - to) as u32, font: old.font.clone() });
            }
        }
        let mut coalesced: Vec<Run> = Vec::with_capacity(runs.len());
        for r in runs {
            match coalesced.last_mut() {
                Some(last) if last.font == r.font && last.start as usize + last.len as usize == r.start as usize => {
                    last.len = last.len.saturating_add(r.len);
                }
                _ => coalesced.push(r),
            }
        }
        let flat = match coalesced.as_slice() {
            [] => true,
            [one] => one.start == 0 && one.len as usize == text.len() && one.font == RunFont::default(),
            _ => false,
        };
        if flat {
            return Ok(None);
        }
        if coalesced.len() > MAX_RUNS_PER_CELL {
            return Err(RunFormatError::Cap);
        }
        Ok(Some(CellRuns { runs: coalesced }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none_font() -> RunFont {
        RunFont { name: None, size: None, bold: None, italic: None, underline: None, strike: None, color: None, vert: None }
    }

    fn run(start: u32, len: u32) -> Run {
        Run { start, len, font: none_font() }
    }

    #[test]
    fn contiguous_valid() {
        let cr = CellRuns { runs: vec![run(0, 5), run(5, 4)] };
        assert!(cr.valid_for("black red"));
        assert_eq!(cr.run_text("black red", 0), Some("black"));
        assert_eq!(cr.run_text("black red", 1), Some(" red"));
        assert_eq!(cr.run_text("black red", 2), None);
    }

    #[test]
    fn gap_invalid() {
        let cr = CellRuns { runs: vec![run(0, 2), run(3, 1)] };
        assert!(!cr.valid_for("abcd"));
    }

    #[test]
    fn overlap_invalid() {
        let cr = CellRuns { runs: vec![run(0, 3), run(2, 2)] };
        assert!(!cr.valid_for("abcd"));
    }

    #[test]
    fn wrong_total_invalid() {
        let cr = CellRuns { runs: vec![run(0, 2)] };
        assert!(!cr.valid_for("abcd"));
    }

    #[test]
    fn non_boundary_invalid() {
        // "é" is two bytes; the first run cuts through the character.
        let cr = CellRuns { runs: vec![run(0, 1), run(1, 1)] };
        assert!(!cr.valid_for("é"));
    }

    #[test]
    fn empty_invalid() {
        let cr = CellRuns { runs: vec![] };
        assert!(!cr.valid_for("abc"));
        assert!(!cr.valid_for(""));
    }

    #[test]
    fn merged_overrides() {
        let base = Font { name: "Georgia".into(), size: 14.0, italic: true, ..Font::default() };
        let delta = RunFont { bold: Some(true), color: Some(Color::rgb(255, 0, 0)), ..none_font() };
        let m = delta.merged(&base);
        assert!(m.bold);
        assert_eq!(m.color, Color::rgb(255, 0, 0));
        assert_eq!(m.name, "Georgia");
        assert_eq!(m.size, 14.0);
        assert!(m.italic);
    }

    #[test]
    fn merged_inherits_all() {
        let base = Font { name: "Georgia".into(), size: 14.0, bold: true, ..Font::default() };
        assert_eq!(none_font().merged(&base), base);
    }

    fn run_font(start: u32, len: u32, font: RunFont) -> Run {
        Run { start, len, font }
    }

    fn bold_font() -> RunFont {
        RunFont { bold: Some(true), ..none_font() }
    }

    fn red_font() -> RunFont {
        RunFont { color: Some(Color::rgb(255, 0, 0)), ..none_font() }
    }

    #[test]
    fn format_overrides_only_given_fields() {
        // "black red": bold on "black" survives; the red overlay on "red" (bytes 6..9) sets
        // only `color`.
        let cr = CellRuns { runs: vec![run_font(0, 5, bold_font()), run(5, 4)] };
        let out = cr.format_range("black red", 6..9, &red_font()).unwrap();
        assert_eq!(out, Some(CellRuns { runs: vec![run_font(0, 5, bold_font()), run(5, 1), run_font(6, 3, red_font())] }));
    }

    #[test]
    fn format_splits_inside_run() {
        // "abcdef": colouring bytes 2..4 inside the bold run splits it; both halves and the
        // covered middle keep the parent's bold.
        let cr = CellRuns { runs: vec![run_font(0, 6, bold_font())] };
        let out = cr.format_range("abcdef", 2..4, &red_font()).unwrap();
        assert_eq!(
            out,
            Some(CellRuns {
                runs: vec![
                    run_font(0, 2, bold_font()),
                    run_font(2, 2, RunFont { color: Some(Color::rgb(255, 0, 0)), ..bold_font() }),
                    run_font(4, 2, bold_font())
                ]
            })
        );
    }

    #[test]
    fn format_coalesces_neighbors() {
        // The bold delta over two adjacent bold runs yields a single run.
        let cr = CellRuns { runs: vec![run_font(0, 2, bold_font()), run_font(2, 2, bold_font())] };
        let out = cr.format_range("abcd", 0..4, &bold_font()).unwrap();
        assert_eq!(out, Some(CellRuns { runs: vec![run_font(0, 4, bold_font())] }));
    }

    #[test]
    fn format_whole_text_empty_delta_is_flat() {
        let cr = CellRuns { runs: vec![run(0, 9)] };
        assert_eq!(cr.format_range("black red", 0..9, &none_font()).unwrap(), None);
    }

    #[test]
    fn format_partial_empty_delta_is_noop() {
        let cr = CellRuns { runs: vec![run_font(0, 2, bold_font()), run(2, 2)] };
        let out = cr.format_range("abcd", 1..3, &none_font()).unwrap();
        assert_eq!(out, Some(cr));
    }

    #[test]
    fn format_cap_errors() {
        // 8 192 one-byte runs (at the cap, alternating fonts so they can't coalesce) plus a
        // two-byte tail run over "a".repeat(8194); splitting the tail would exceed the cap.
        let mut runs = Vec::with_capacity(MAX_RUNS_PER_CELL + 1);
        for i in 0..MAX_RUNS_PER_CELL as u32 {
            runs.push(run_font(i, 1, if i % 2 == 0 { bold_font() } else { none_font() }));
        }
        runs.push(run_font(MAX_RUNS_PER_CELL as u32, 2, bold_font()));
        let cr = CellRuns { runs };
        let text = "a".repeat(MAX_RUNS_PER_CELL + 2);
        assert!(cr.valid_for(&text));
        let out = cr.format_range(&text, (MAX_RUNS_PER_CELL + 1)..(MAX_RUNS_PER_CELL + 2), &red_font());
        assert_eq!(out, Err(RunFormatError::Cap));
    }

    #[test]
    fn format_multibyte_boundaries() {
        // "é ok è" is 8 bytes: é = 0..2, " ok " = 2..6, è = 6..8; every range stays on the
        // characters' byte boundaries.
        let cr = CellRuns { runs: vec![run_font(0, 8, bold_font())] };
        let out = cr.format_range("é ok è", 2..6, &red_font()).unwrap();
        assert_eq!(
            out,
            Some(CellRuns {
                runs: vec![
                    run_font(0, 2, bold_font()),
                    run_font(2, 4, RunFont { color: Some(Color::rgb(255, 0, 0)), ..bold_font() }),
                    run_font(6, 2, bold_font())
                ]
            })
        );
        assert!(out.unwrap().valid_for("é ok è"));
        let out = cr.format_range("é ok è", 6..8, &red_font()).unwrap();
        assert_eq!(
            out,
            Some(CellRuns { runs: vec![run_font(0, 6, bold_font()), run_font(6, 2, RunFont { color: Some(Color::rgb(255, 0, 0)), ..bold_font() })] })
        );
        assert!(out.unwrap().valid_for("é ok è"));
    }

    #[test]
    fn serde_roundtrip() {
        let cr = CellRuns {
            runs: vec![Run { start: 0, len: 5, font: RunFont { name: Some("Georgia".into()), size: Some(14.0), bold: Some(true), ..none_font() } }],
        };
        let json = serde_json::to_string(&cr).unwrap();
        assert_eq!(serde_json::from_str::<CellRuns>(&json).unwrap(), cr);
    }
}
