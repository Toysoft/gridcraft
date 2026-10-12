# Rich text runs (mixed font colors within a cell) — milestone 1: import, render, round-trip

- **Date:** 2026-10-12
- **Status:** design approved in chat (storage, scope, delta fonts, numfmt precedence); awaiting implementation plan
- **Issues:** fixes [#774](https://github.com/storytold/gridcraft/issues/774) (extracted from #708 item 1); editing UI is follow-up [#776](https://github.com/storytold/gridcraft/issues/776)
- **Scope agreed with:** eduardo

## Problem

XLSX cells whose text is stored as rich text runs (`<si><r><rPr>…</rPr><t>…</t></r>…</si>`) open in
GridCraft as a single-format string and are written back without the runs: per-run font properties
are neither rendered nor preserved. Verified visually (test renders of the #774 repro workbook:
A3/A4 entirely black in GridCraft, black/red/black and black/red/green in ONLYOFFICE) and in code:

- `crates/xlsx/src/xml.rs:69` `rich_text()` concatenates `<t>`/`<r><t>` into a plain `String`,
  discarding every `<rPr>`.
- `crates/core/src/value.rs:150` `Value::Text(Arc<str>)` — the model has no run-level formatting.
- `crates/xlsx/src/write.rs:329` writes `<si><t>…</t></si>`, no `<r>` runs.
- `.xls` (BIFF8) reader flattens identically (out of scope here; noted in §Non-goals).

## Goals

1. Import: per-run font properties (color first-class; also family, size, bold, italic, underline,
   strike, vertAlign) survive XLSX read as deltas.
2. Render: cells with runs paint each run with the run's properties merged over the cell font.
3. Round-trip: literal text cells with runs are written back as `<si>` runs, unchanged.
4. Editing a cell replaces its runs with the cell-level format (agreed milestone-1 behavior).

## Non-goals (milestone 1)

- No run-editing UI (formula bar / in-cell selection, Font Color on a character range) — #776.
- No new engine commands; no changes to `Value`, calc, functions, clipboard, CSV, PDF.
- No `.xls`/`.ods`/`.xlsb` run import (unchanged flattening there).
- No live re-resolution subtleties beyond delta semantics defined below.

## Decision record

| Decision | Choice | Rejected alternatives |
|---|---|---|
| Where runs live | **Side-table** `Sheet.cell_runs: BTreeMap<CellRef, CellRuns>`, like `comments` (`sheet.rs:86`) and `cell_pictures` (`sheet.rs:56`) | `Value::RichText` variant (~110 match sites / 37 files, forces formula flattening semantics); runs inside `Style` (styles are content-agnostic and whole-interned; runs are content-scoped) |
| Run font shape | **Delta** (`Option` fields); unspecified properties follow the cell font at render/write time | Resolved-absolute fonts stored at import (simpler, but changes to the cell font would not propagate, and round-trip would balloon partial `rPr` to full) |
| numfmt color vs run color | numfmt section colors apply **only when the cell has no valid runs** | numfmt always wins (would repaint imported runs) |
| Edit behavior | `Sheet::set_cell` drops the entry | Splitting/merging runs around typed text (that is #776) |

## Design

### 1. Data model (`crates/model`)

```rust
// crates/model/src/cell.rs (or a new runs.rs), next to CellPicture
pub struct CellRuns { pub runs: Vec<Run> }        // shared via Arc
pub struct Run {
    pub start: u32,        // byte offset into the cell's text
    pub len: u32,          // byte length of this run's slice
    pub font: RunFont,     // delta: None = follow the cell font
}
pub struct RunFont {        // mirrors Font (style.rs:154) with Options; Eq + Hash
    pub name: Option<String>,
    pub size: Option<f32>,          // Hash/Eq via to_bits, like Font
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<Underline>,
    pub strike: Option<bool>,
    pub color: Option<Color>,       // model Color (style.rs:17): Auto | Rgb | Theme+tint
    pub vert: Option<VertAlign>,
}
```

- `Sheet.cell_runs: BTreeMap<CellRef, CellRuns>` (plain values, like `comments`), serialized with the existing A1-key serde
  helper (`crate::cellmap`, as `comments`). If `Arc` needs serde `rc` support not enabled for
  `comments`-style plain structs, mirror the `picture_map` approach (`sheet.rs:446`).
- `CellRuns` participates in `Cell`/sheet equality only through the sheet's diff path the way
  comments do (verify `store.rs diff` treats side-tables; undo is full snapshots, so history works
  regardless).
- **Validity guard** (the never-crash backbone): an entry is *valid* for a cell iff the cell value
  is `Value::Text(t)`, every `start..start+len` is a char-boundary-safe `t.get()` slice, and the
  runs' slices concatenate to exactly `t`. Consumers (render, export) check validity and fall back
  to flat behavior when it fails. Import produces valid entries or drops them.

### 2. Import (`crates/xlsx`)

- `xml.rs`: new `si_parts(e) -> (String, Vec<RawRun>)` next to `rich_text()` (which remains for
  comments/phonetic). `RawRun { start, len, rpr: Option<&El> }`; `<rPh>` ignored, `xml:space`
  preserved per-`<t>` (the concatenated text must keep inner spaces).
- `styles.rs`: `read_run_font(rpr, palette) -> RunFont` — a delta-reading sibling of `read_font`
  (styles.rs:60). Color via existing `read_color` (styles.rs:31): `rgb` → `Rgb`, `theme`+`tint` →
  `Theme(t, tint‰)`, `indexed` → palette / `INDEXED` table → `Rgb`, `auto` → `Auto`. The #774 repro
  exercises all three encodings (`theme="1"`, `indexed="2"`, `rgb="FF00B050"`).
- SST (`read.rs:136-152`): `Ctx.sst: Vec<Si>` with `Si { text: Arc<str>, runs: Option<Vec<Run>> }`
  (runs resolved against the palette at parse time; run fonts are deltas so no cell style needed
  yet).
- `sheet_read.rs` `read_cell` (390-454): `t="s"` and `inlineStr` (405-407) attach
  `Arc<CellRuns>` to the sheet when the si carried runs and the validity guard passes.
- **Caps** (hostile-file bounds): ≤ 8 192 runs per `<si>`, ≤ 2²⁰ runs per workbook. Over budget →
  keep the text, drop the runs (import never fails because of runs).

### 3. Export (`crates/xlsx`)

- `write.rs` `Sst` (87-105): entries become `SstEntry { text: String, runs: Option<CellRuns> }`,
  dedup key = (text, runs hash). Rich entries emit
  `<si><r><rPr>…</rPr><t xml:space="preserve">…</t></r>…</si>`; only fields set in the delta are
  emitted. `font_xml` (styles.rs:399) gains a mode to emit `<rFont val>` instead of `<name val>`;
  `color_attrs` (styles.rs:385) reused as-is.
- `sheet_write.rs` `write_cell` (602-700): literal `Value::Text` (689-692) looks up
  `sheet.cell_runs`; valid → rich SST index. Formula cached results (`t="str"`) never carry runs
  (OOXML-correct).
- Deleted/empty cells: entries die with `set_cell` (below) and `Sheet` pruning.

### 4. Rendering (`crates/ui-egui`)

- `grid.rs` text loop (899-1115): when the cell has valid runs, build a multi-segment
  `egui::text::LayoutJob` instead of `LayoutJob::simple` (992) / `layout_no_wrap` (1016):
  per run, merge `RunFont` over the resolved cell `Font` → `TextFormat` (font id via
  `theme::cell_family`, color via `col32`/`Color::resolve`). Pattern precedent:
  `code_editor.rs:206 layout_job()`.
- Wrapped path: job honors the wrap width. Single-line path: no-wrap layout of the same job
  (verify the pinned egui version's mechanism; fallback `wrap_width = f32::INFINITY`).
- Rotated text (1095-1097 `TextShape`): consume the job's galley identically.
- Underline/strike segments (1101-1114): drawn per run using the merged font's flags.
- Precedence: numfmt section colors (`display_text`, 433-466) apply only when the cell has no
  valid runs. Invalid runs → today's flat painting, no warnings in-render.

### 5. Edit behavior

- `Sheet::set_cell` (sheet.rs:194-197) removes the `cell_runs` entry at the address, mirroring
  the cell-picture invalidation. All edit paths (`cell.set`, paste, fill, `range.setValues`,
  clear) funnel through `set_cell`.
- Structural edits (insert/delete rows/cols, cut, sort, sheet ops): runs follow the `cell_pictures`
  translation pattern wherever it is applied (e.g. `insert.rs:270-276`); comments are not translated
  on row insert today, and runs share the pictures' keyed-at-address problem, so pictures are the
  correct precedent. Stale entries are always harmless: the validity guard checks the text.

### 6. Bounds and never-crash

- Run offsets/lengths are input-derived numbers: all slicing via `t.get(start..end)`; mismatched
  or non-boundary offsets invalidate the entry (flat fallback), never panic.
- Caps as in §2. No `unwrap/expect/panic` in new code (crate lints already deny them).
- Existing fuzz (`tests::every_command_survives_empty_params`,
  `crates/engine/tests/command_sequence_fuzz.rs`) must pass; the sequence-fuzz fixture pool gains
  a workbook with runs.

## Test plan (TDD)

1. **Unit (xlsx):** `si_parts` on the #774 repro shapes — plain si; 3-run theme/indexed/rgb;
   `xml:space="preserve"` inner spaces; `<rPh>` ignored. Malformed runs (bad offsets, overlapping,
   out-of-bounds, over-cap) → runs dropped, text kept, no panic.
2. **Round-trip (`tests/roundtrip.rs` pattern, built in code):** rich workbook → `write_xlsx` →
   `read_xlsx` → runs equal (offsets + deltas); golden XML contains `<r><rPr>` with only set
   fields; plain strings still dedupe through the SST. No binary fixtures in the repo (craftrules
   test-corpora standard); the #774 workbook shapes are rebuilt in code.
3. **Engine:** `reopen()` keeps runs; editing the cell clears them; undo restores them; a
   structural edit (row insert above) matches comment translation.
4. **UI:** unit-test the runs→`LayoutJob` builder (segments, merged fonts/colors, invalid-runs
   fallback); offscreen snapshot (`--example snapshot`) with a runs sample for eyeball comparison
   against `plan/excel/observed-ui.md` conventions (screenshots stay local to `plan/`).
5. **Fuzz/soak:** `GRIDCRAFT_FUZZ_SEEDS` run with the runs fixture in the pool.

## Documentation updates when landing

- `docs/gaps.md`: extend gap #3 (or add a gap entry) + revision history row.
- `docs/file-format-parity.md`: XLSX row — rich text runs read/written.
- `ROADMAP.md` progress log per craftrules `progress-docs.md`.
- Close #774 in the PR; mention #776 remains open.

## Acceptance criteria

- The #774 repro workbook (rebuilt in code and, manually, the reporter's file) renders A3
  black/red/black and A4 black/red/green in GridCraft.
- Open → save → reopen in GridCraft preserves the runs byte-comparably (`<r>` runs with the same
  colors); opening the saved file in Excel/ONLYOFFICE shows mixed colors.
- Editing any rich cell replaces it with cell-format text; undo restores the runs.
- `cargo xtask ci` green (fmt, clippy -D warnings, tests, assets, layers, wasm).

## Open items to verify during implementation

- egui no-wrap `LayoutJob` mechanics on the pinned version (§4).
- Exact comment translation on structural edits (§5) — replicate for runs.
- serde handling of `Arc<CellRuns>` vs the `comments` pattern (§1).
