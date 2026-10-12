# Rich Text Runs (Mixed Font Colors in a Cell) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** XLSX rich text runs (per-run font properties within one cell's text) survive import, render with their colors, and are written back unchanged (#774 milestone 1).

**Architecture:** Runs live in a side-table `Sheet.cell_runs: BTreeMap<CellRef, CellRuns>` (the comments/cell-pictures pattern), NOT in `Value::Text` — formulas/calc/clipboard stay untouched. Each `Run` is a byte-range plus a `RunFont` *delta* (only the properties the file set); consumers merge the delta over the cell font. A validity guard (`slices concatenate to exactly the cell text`) gates every consumer; invalid runs fall back to today's flat behavior.

**Tech Stack:** Rust (workspace crates `gridcraft-model`, `gridcraft-xlsx`, `gridcraft-ui-egui`, `gridcraft-engine`), quick-xml (existing), egui 0.36 (`LayoutJob::append(&mut self, text, leading_space, format)`, `TextWrapping { max_width, max_rows }`).

**Spec:** `docs/superpowers/specs/2026-10-12-rich-text-runs-design.md` — the plan argues from it; read both.

## Global Constraints

- No `unwrap/expect/panic/todo/unreachable/unsafe` in non-test code (crate lints deny them).
- Run offsets/lengths/counts are hostile input: slice with `str::get(start..end)`, checked adds; caps `MAX_RUNS_PER_CELL = 8_192` and a per-workbook budget of `2^20` runs — over budget drops the runs, keeps the text, never fails the file.
- Layering: `Run`/`RunFont`/`CellRuns` live in `gridcraft-model`; no new dependencies; `cargo xtask wasm` must stay green.
- No binary fixtures: all test workbooks are built in code (the #774 repro shapes are XML strings in tests).
- Commits: message prefix `774: ` (one task id per commit). `cargo xtask ci` green before the final commit.
- Per-task test commands: `cargo test -p <crate> <filter>` with crate names `gridcraft-model`, `gridcraft-xlsx`, `gridcraft-ui-egui`, `gridcraft-engine`.

## Review Focus

1. **Runs on a cell whose value isn't text** (formula result, number, picture): guard returns false, flat fallback, no panic. → Task 2 `guard_ignores_non_text_value`.
2. **Leading spaces at run boundaries** (` this is red,` with `xml:space="preserve"`): concatenation preserves them; offsets stay on char boundaries. → Task 3 `preserves_leading_space_offsets`.
3. **Hostile counts/offsets** (8 193rd run, overlapping ranges, workbook budget exhausted): runs dropped, text kept. → Task 2 `overlap_invalid`/`wrong_total_invalid`, Task 3 `over_cell_cap_drops_runs`/`over_workbook_budget_drops`.
4. **Absent vs automatic run color**: absent inherits the cell font color (red cell font → red run); `Some(Color::Auto)` paints default black regardless. → Task 6 `absent_color_inherits_cell_color`.
5. **SST dedup with runs**: two cells sharing one rich si collapse to one entry; same text with different runs stays two. → Task 5 `sst_dedup_shares_and_splits`.

---

### Task 1: Model — `Run`, `RunFont`, `CellRuns` with validity guard and merge

**Files:**
- Create: `crates/model/src/runs.rs`
- Modify: `crates/model/src/lib.rs` (add `mod runs;` after `mod preserved;`, re-export `pub use runs::{CellRuns, Run, RunFont, MAX_RUNS_PER_CELL};`)
- Test: `crates/model/src/runs.rs` (`#[cfg(test)]` block)

**Interfaces:**
- Consumes: `Color`, `Font`, `Underline`, `VertAlign` from `crate::style`.
- Produces (later tasks rely on these exact names):
  ```rust
  pub const MAX_RUNS_PER_CELL: usize = 8_192;
  #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub struct RunFont { pub name: Option<String>, pub size: Option<f32>, pub bold: Option<bool>,
      pub italic: Option<bool>, pub underline: Option<Underline>, pub strike: Option<bool>,
      pub color: Option<Color>, pub vert: Option<VertAlign> }   // manual Hash: size via to_bits
  #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub struct Run { pub start: u32, pub len: u32, pub font: RunFont }
  #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
  pub struct CellRuns { pub runs: Vec<Run> }
  impl CellRuns {
      pub fn valid_for(&self, text: &str) -> bool;           // slices boundary-safe, non-overlapping, concatenate to exactly text
      pub fn run_text<'a>(&self, text: &'a str, i: usize) -> Option<&'a str>;  // t.get(start..start+len), checked add
  }
  impl RunFont { pub fn merged(&self, base: &Font) -> Font; } // None fields take base's
  ```
  `impl Hash for RunFont/Run/CellRuns` (f32 via `to_bits`, like `Font` at `style.rs:167-173`) — needed by Task 5's dedup key.

- [ ] **Step 1: Write the failing tests** in `runs.rs` `#[cfg(test)]`:
  `contiguous_valid` (runs `[(0,6),(6,4)]` over `"black red"` → `valid_for` true, `run_text` gives `"black"`/`" red"`); `gap_invalid`, `overlap_invalid`, `wrong_total_invalid` (total ≠ text len), `non_boundary_invalid` (start mid-UTF-8 — build with `"é"`), `empty_invalid` (empty vec → false); `merged_overrides` (delta bold+color over base font → merged has them, inherits size/name); `merged_inherits_all` (all-None delta → equals base).
- [ ] **Step 2: Run to verify failure** — `cargo test -p gridcraft-model runs` → FAIL (unresolved module).
- [ ] **Step 3: Implement** `runs.rs` per the Interfaces block. Validity: iterate runs in order, each `start` equals running total, `text.get(start..start+len).is_some()` (`checked_add`), final total `== text.len()`. Cap-aware constructor not needed here (xlsx owns caps).
- [ ] **Step 4: Run to verify pass** — `cargo test -p gridcraft-model runs` → PASS; `cargo clippy -p gridcraft-model --all-targets -- -D warnings` clean.
- [ ] **Step 5: Commit** — `git commit -m "774: model CellRuns side-table payload with validity guard"`

### Task 2: Model — `Sheet.cell_runs` side-table + edit invalidation + serde

**Files:**
- Modify: `crates/model/src/sheet.rs` (field after `comments` ~line 88; `Sheet::new` initializer ~120; `set_cell` 194-197, `remove_cell` 198-201, `take_cells` 202-206)
- Test: `crates/model/src/sheet.rs` tests (find the existing `#[cfg(test)]` block or add one)

**Interfaces:**
- Consumes: Task 1 types.
- Produces: `Sheet.cell_runs: BTreeMap<CellRef, CellRuns>` with `#[serde(default, skip_serializing_if = "BTreeMap::is_empty", with = "crate::cellmap")]` (A1 keys, same as `comments` at line 85-86). `set_cell`, `remove_cell`, `take_cells` remove entries exactly where they remove pictures (194-206); `set_value`/paste/fill inherit this via `set_cell`.

- [ ] **Step 1: Write the failing tests**: `set_cell_clears_runs` (insert runs at A1, `set_cell` a new Cell → entry gone); `take_cells_clears_range`; `serde_roundtrip_a1_keys` (`serde_json` to/from on a Sheet with one entry → key is `"A1"` in the JSON and the entry survives); `guard_ignores_non_text_value` (manual entry whose slices don't match a number value → `valid_for` false — pins Review Focus 1 at the model level).
- [ ] **Step 2: Run to verify failure** — `cargo test -p gridcraft-model sheet` → FAIL (no field).
- [ ] **Step 3: Implement** the field + `BTreeMap::new()` in `Sheet::new` (and any `Sheet { … }` literal the compiler flags) + the three removals (`self.cell_runs.remove(&c)` / `retain` mirroring `cell_pictures` lines 195/199/204).
- [ ] **Step 4: Run to verify pass** — `cargo test -p gridcraft-model` → PASS (whole crate: catches struct-literal fallout); clippy clean as in Task 1.
- [ ] **Step 5: Commit** — `git commit -m "774: Sheet.cell_runs side-table, cleared on content change"`

### Task 3: xlsx — parse `<si>` runs (`si_parts`, `read_run_font`, `si_cell_runs`)

**Files:**
- Create: `crates/xlsx/src/runs.rs`; Modify: `crates/xlsx/src/lib.rs` (`mod runs;`), `crates/xlsx/src/xml.rs` (add `si_parts` + `SiRun` near `rich_text` 69-83), `crates/xlsx/src/styles.rs` (add `read_run_font` near `read_font` 60-93)
- Test: create `crates/xlsx/src/tests/runs_tests.rs`, register `mod runs_tests;` in `crates/xlsx/src/tests/mod.rs`

**Interfaces:**
- Consumes: Task 1 types; `read_color` (styles.rs:31), `bool_prop`-style parsing, `El`.
- Produces:
  ```rust
  // xml.rs
  pub struct SiRun<'a> { pub start: u32, pub len: u32, pub rpr: Option<&'a El> }
  pub fn si_parts(e: &El) -> (String, Vec<SiRun<'_>);   // <t> and <r><t> concatenated; <rPh> skipped
  // styles.rs
  pub fn read_run_font(rpr: &El, palette: &[u32]) -> RunFont;  // delta; color: read_color then Theme(1,0)->Some(Color::Auto); sz clamped 1.0..=409.0
  // runs.rs
  pub struct Si { pub text: Arc<str>, pub runs: Option<CellRuns> }
  pub fn si_cell_runs(si: &El, palette: &[u32], budget: &mut u64) -> (Arc<str>, Option<CellRuns>);
  //     caps: parts.len() > MAX_RUNS_PER_CELL or *budget saturating_sub would underflow -> runs None, text kept
  ```

- [ ] **Step 1: Write the failing tests** in `runs_tests.rs` using `xml::parse` on the #774 repro `<si>` shapes: `si_parts_plain` (single `<t>` → no runs); `si_parts_repro_colors` (3-run si: `theme="1"` → `Some(Color::Auto)`, `indexed="2"` → `Some(Color::Rgb(0xFF0000))` via the default palette, `rgb="FF00B050"` → `Some(Color::Rgb(0x00B050))`; offsets 0/15/27 per the repro text `this is black, this is red, this is black`); `preserves_leading_space_offsets` (` this is red,` keeps its leading space; total == text.len()); `rph_ignored`; `over_cell_cap_drops_runs` (8_193 `<r>` elements → `(text, None)`); `over_workbook_budget_drops` (budget 0 → None, text kept, budget stays 0).
- [ ] **Step 2: Run to verify failure** — `cargo test -p gridcraft-xlsx runs_tests` → FAIL.
- [ ] **Step 3: Implement** the three functions per Interfaces. `si_parts` mirrors `rich_text`'s child loop, recording `(start, len)` per `<r>` and `Option<&El>` for `<rPr>`; `si_cell_runs` assembles `CellRuns` (runs only when >0 parts with any rPr) and only returns `Some` when `valid_for(text)`.
- [ ] **Step 4: Run to verify pass** — `cargo test -p gridcraft-xlsx runs_tests` → PASS; clippy clean.
- [ ] **Step 5: Commit** — `git commit -m "774: xlsx parses shared-string runs with color deltas and caps"`

### Task 4: xlsx — attach runs at read (shared strings + inline strings)

**Files:**
- Modify: `crates/xlsx/src/read.rs` (`Ctx.sst` type at ~17, streaming loop 136-152), `crates/xlsx/src/sheet_read.rs` (`read_cell` 390-454: `"s"` arm 394-403, `"inlineStr"` arm 405-407, insert after 453)
- Test: `crates/xlsx/src/tests/runs_tests.rs` (extend; add a fixtures-style package builder if `fixtures.rs` helpers fit, else inline the zip like `malformed.rs` does)

**Interfaces:**
- Consumes: Task 3 `Si`/`si_cell_runs`.
- Produces: `Ctx.sst: Vec<runs::Si>` (was `Vec<Arc<str>>`); `read_cell` inserts `sheet.cell_runs.insert(pos, runs)` only when the final value is `Value::Text` matching `runs.valid_for(&text)` (line 452 can replace the value for spills — insert after 453, guarded).

- [ ] **Step 1: Write the failing tests**: `read_attaches_shared_runs` (package whose sharedStrings holds the 4 repro `<si>`s and sheet1 references A1-A4 as in #774 → after `read_xlsx`, `sheet.cell_runs` has entries for A3 and A4 only, with the exact Task 3 colors and offsets); `read_attaches_inline_runs` (inlineStr `<is><r>…` cell → entry present; extend the shape at `fixtures.rs:18`); `plain_cells_have_no_entry`; `spill_override_leaves_no_stale_entry` (cell in a spilled range → no runs entry).
- [ ] **Step 2: Run to verify failure** — `cargo test -p gridcraft-xlsx runs_tests` → FAIL (type mismatch).
- [ ] **Step 3: Implement**: streaming callback becomes `let (text, runs) = runs::si_cell_runs(&si, &cx.palette, &mut cx.run_budget)` (add `run_budget: u64` to `Ctx`); `"s"` arm clones `Si` text and carries runs to the post-453 insert; `"inlineStr"` arm calls `si_cell_runs` directly. Fix every other `cx.sst` use the compiler flags (they consume text only).
- [ ] **Step 4: Run to verify pass** — `cargo test -p gridcraft-xlsx` → PASS (whole crate: catches all `sst` consumers); clippy clean.
- [ ] **Step 5: Commit** — `git commit -m "774: xlsx read attaches rich runs to shared and inline string cells"`

### Task 5: xlsx — write runs back (`SstEntry`, `run_rpr_xml`, round-trip)

**Files:**
- Modify: `crates/xlsx/src/write.rs` (`Sst` 87-105, emission 327-333), `crates/xlsx/src/styles.rs` (`run_rpr_xml` + `si_runs_xml` near `font_xml` 399-435), `crates/xlsx/src/sheet_write.rs` (`write_cell` signature 602-612 + `Value::Text` arm 689-692 + its caller loop)
- Test: `crates/xlsx/src/tests/runs_tests.rs` (extend)

**Interfaces:**
- Consumes: Task 1 `Hash`/`Eq`, Task 2 side-table.
- Produces:
  ```rust
  // write.rs
  pub struct SstEntry { pub text: String, pub runs: Option<CellRuns> }
  pub struct Sst { list: Vec<SstEntry>, map: HashMap<(String, Option<CellRuns>), u32>, pub count: u64 }
  impl Sst { pub fn index(&mut self, s: &str) -> u32; pub fn index_rich(&mut self, s: &str, runs: &CellRuns) -> u32; }
  // styles.rs
  pub fn run_rpr_xml(rf: &RunFont) -> String;                 // <rPr>…</rPr>, only set fields; <rFont val> not <name>; color via color_attrs (Auto -> auto="1")
  pub fn si_runs_xml(text: &str, runs: &CellRuns) -> Option<String>;  // <r><rPr>..</rPr>{t_el(slice)}</r>…; None when !valid_for
  // sheet_write.rs: write_cell gains param `runs: Option<&CellRuns>`; caller passes sh.cell_runs.get(&c)
  ```

- [ ] **Step 1: Write the failing tests**: `runs_roundtrip` (build wb in code: cells A3/A4 from the repro + `cell_runs` entries → `write_xlsx` → unzip `xl/sharedStrings.xml`, assert it contains `<r><rPr>` with `auto="1"`, `rgb="FFFF0000"`, `rgb="FF00B050"`, and `xml:space="preserve"` on the `" this is red,"` slice → `read_xlsx` → `cell_runs` equals the input); `sst_dedup_shares_and_splits` (two cells with identical text+runs → `uniqueCount` counts one si; same text different colors → two); `invalid_runs_write_plain` (entry not matching text → plain `<t>`); existing `roundtrip.rs` suite still green.
- [ ] **Step 2: Run to verify failure** — `cargo test -p gridcraft-xlsx runs_tests` → FAIL.
- [ ] **Step 3: Implement** per Interfaces; emission loop at 328-330 becomes a match on `entry.runs` (`si_runs_xml` → rich, else `t_el`). `Value::Text` arm: `let i = match runs { Some(r) if r.valid_for(t) => out.sst.index_rich(t, r), _ => out.sst.index(t) };`.
- [ ] **Step 4: Run to verify pass** — `cargo test -p gridcraft-xlsx` → PASS; clippy clean.
- [ ] **Step 5: Commit** — `git commit -m "774: xlsx writes rich runs to sharedStrings with dedup"`

### Task 6: ui-egui — render runs (`LayoutJob` builder + grid integration)

**Files:**
- Create: `crates/ui-egui/src/cell_runs.rs`; Modify: `crates/ui-egui/src/lib.rs` (module), `crates/ui-egui/src/grid.rs` (wrapped path 990-1007, single-line galley 1016, paint/underline/strike/rotated 1095-1114)
- Test: `crates/ui-egui/src/cell_runs.rs` (`#[cfg(test)]`)

**Interfaces:**
- Consumes: Task 1 `CellRuns`/`RunFont::merged`; `Color::resolve(&Theme)`; `theme::cell_family(name, bold, italic)` (grid.rs:964 usage).
- Produces:
  ```rust
  pub fn job(text: &str, runs: &CellRuns, base: &Font, base_color: Color32, theme: &Theme, z: f32, wrap: Option<f32>)
      -> Option<egui::text::LayoutJob>;   // None when runs empty/invalid for text
  ```
  Per run: `RunFont::merged(base)` → `TextFormat` (FontId size `merged.size * 96/72 * z`, family via `cell_family`, color `merged.color` resolved: `None`→base_color, `Some(Auto)`→Color32::BLACK, `Some(c)`→resolve→black fallback); `LayoutJob::append(slice, 0.0, format)`; `wrap == None` → `job.wrap.max_width = f32::INFINITY` (single-row behavior like `layout_no_wrap`).

- [ ] **Step 1: Write the failing tests**: `job_none_for_invalid`; `job_segments_and_colors` (base font red via `base_color`, runs [inherit-none, Some(Auto), Some(Rgb(0x00B050))] → three sections with those colors — pins Review Focus 4 — and `job.text == text`); `bold_run_overrides_family`; `wrapped_job_honors_width` (`wrap = Some(50.0)` → `job.wrap.max_width == 50.0`).
- [ ] **Step 2: Run to verify failure** — `cargo test -p gridcraft-ui-egui cell_runs` → FAIL.
- [ ] **Step 3: Implement** the builder per Interfaces. Grid integration: fetch `let cell_runs = sh.cell_runs.get(&c);` in the text loop (after 962); compute `let rjob = cell_runs.and_then(|r| job(&text, r, &st.font, lk.text_color, &wb.theme, z, wrap_width_option))`; **precedence**: `ncolor` (numfmt section color, line 976) applies only when `rjob.is_none()`; wrapped path uses the job (halign per 993-997); single-line path uses `p.layout_job(job)` instead of `p.layout_no_wrap` at 1016 when the job exists **and** none of the flat-only transformations apply (shrink-to-fit 1011-1015, `####` 1019-1023, fill char 1025-1037 — those keep today's flat path); underline/strike lines (1101-1114) draw per job section when the merged font asks for them (iterate `job.sections` byte ranges over the galley rows); rotated path (1095-1097) consumes the same galley.
- [ ] **Step 4: Run to verify pass** — `cargo test -p gridcraft-ui-egui` → PASS; clippy clean. Eyeball: run the app on the reporter's file (`cargo run -p gridcraft -- "/tmp/colors test.xlsx" --control 7979`, then `ui.screenshot` per AGENTS.md; if the binary rejects a file argument, use `cargo run -p gridcraft-ui-egui --example snapshot` extended with a `--file` flag) and confirm A3 black/red/black, A4 black/red/green.
- [ ] **Step 5: Commit** — `git commit -m "774: grid renders per-run colors via multi-segment LayoutJob"`

### Task 7: engine proof, structural translation, fuzz, docs

**Files:**
- Modify: `crates/engine/src/tests.rs` (near `reopen` 1767), `crates/engine/src/cmd/insert.rs` (+ any other `cell_pictures` translation sites — grep `cell_pictures` across `crates/engine/src/cmd/`), `crates/engine/tests/command_sequence_fuzz.rs` (fixture pool), `docs/gaps.md`, `docs/file-format-parity.md`, `ROADMAP.md`
- Test: `crates/engine/src/tests.rs`

**Interfaces:**
- Consumes: everything above. No new public interfaces.

- [ ] **Step 1: Write the failing tests** in engine `tests.rs`: `runs_survive_reopen` (build session with a runs cell, `reopen` at 1767, assert `cell_runs` intact); `edit_clears_runs_and_undo_restores` (`cell.set` on the rich cell → entry gone; `undo` → back); `row_insert_translates_runs` (rich cell A3, insert row 2 → entry now at A4).
- [ ] **Step 2: Run to verify failure** — `cargo test -p gridcraft-engine runs_` → FAIL.
- [ ] **Step 3: Implement**: extend every `cell_pictures` shift/retain site found in `cmd/` (the pattern at `insert.rs:270-276`) to shift `cell_runs` identically — **spec deviation, already amended in the spec file**: runs follow the pictures translation, not comment parity, because comments aren't translated on row insert while pictures are and runs share the keyed-at-address problem. Fix the tests' expectations from actual behavior.
- [ ] **Step 4: Fuzz + gates**: add a runs workbook to the `command_sequence_fuzz` fixture pool; `GRIDCRAFT_FUZZ_SEEDS=64 cargo test -p gridcraft-engine --test command_sequence_fuzz`; then `cargo xtask ci` (fmt, clippy, tests, assets, layers, wasm) → all green.
- [ ] **Step 5: Docs** — `docs/gaps.md`: add rich text runs to gap #3's Missing list with "landed for #774" evidence + a revision-history row; `docs/file-format-parity.md`: XLSX row gains "rich text runs (per-run font colors) read, rendered, written" + revision row; `ROADMAP.md` progress log line. Commit — `git commit -m "774: engine keeps runs through edits and structure; fuzz and docs"`.
- [ ] **Step 6: PR prep** — push `fix/774-rich-text-runs`, open the PR with "Closes #774" (mention #708 item 1 and #776 as follow-up). Per AGENTS.md contributor rules, check `contributors/people.toml` for the human's GitHub username entry and ask once whether they want name credit before opening the PR.
