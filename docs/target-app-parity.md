# Target-app parity: GridCraft vs Microsoft Excel

> **Last reviewed:** 2026-10-12 · **Last updated:** 2026-10-12 · **Change:** minor (audit against main: evidence text corrected for merged PRs, function count 513; scores and hours not re-derived) · **Target:** Microsoft Excel (Microsoft 365)

This is the authoritative parity assessment. [`ROADMAP.md`](../ROADMAP.md) summarizes it,
[`gaps.md`](gaps.md) lists every shortfall, and the area checklists hold the detail:
[functions](function-parity.md), [charts](chart-parity.md), [file formats](file-format-parity.md),
[UI](ui-parity.md), [hardware](hardware-parity.md), [localization](localization-parity.md). The
generated ribbon checklist is [`parity-checklist.md`](parity-checklist.md).

## Summary

| Number | Value | Kind |
|---|---|---|
| Feature breadth (weighted by use) | **~75%** | estimated from the area table below; anchored on two measurements |
| Ribbon and menu catalog | **274 / 290 (94.5%)** | measured: `cargo xtask parity` (`crates/engine/src/catalog.rs` ∩ registered command ids) |
| Worksheet functions, by name | **513 / 523 (98.1%)** | measured: registered functions vs Microsoft's published function list (method below) |
| Worksheet functions that do real work | **504 / 523 (96.4%)** | measured: excludes 9 stubs (7 CUBE functions, WEBSERVICE, FILTERXML) |
| **Ready for real work** | **~50%** | estimated; written weights below (51.6 computed) |
| **Mainstream practitioner** | **~50%** | estimated; [method](#mainstream-practitioner) (69.5 depth × 0.90 × 0.92 × 0.88 = 50.6) |
| **Essentials user** | **~63%** | estimated; [method](#essentials-user) (80.2 depth × 0.92 × 0.95 × 0.90 = 63.1) |
| Stage | **alpha** | see [Stage](#stage) |
| Remaining to beta | **~160–240 Opus 5.5 agent-hours** | estimated; [roadmap.md](roadmap.md#beta-gates) |
| Remaining to full parity | **~700–1,100 Opus 5.5 agent-hours** | estimated; sum of the rows below |

### Readiness by audience

| Audience | Ready % | Opus 5.5 agent wall-clock hours to ~95% | Work that dominates |
|---|---|---|---|
| Full target (ready for real work) | ~50% | ~650–1,050 | Get & Transform and the data model, localization (twelve languages), charts depth, file fidelity (`.xls`, corpus, pass-through), PivotTables, collaboration, mobile |
| Mainstream practitioner | ~50% | ~200–310 | File exchange with Excel users (corpus and Excel verification, pass-through, large files, `.xls`, encryption: 63–97 h), common-chart depth (30–45 h), PivotTables (20–30 h), keyboard and interaction (18–28 h), calc correctness and functions (20–33 h) |
| Essentials user | ~63% | ~60–100 | Opening files people send (`.xls` import and saving with a password 24–36 h; macOS Finder open 2–4 h), launch stability (6–10 h), basic chart resize and polish (6–10 h), print and Page Layout (8–12 h), small editing and UI fixes (10–16 h) |

Each tier is a subset of the one above, so its hours are too. Calibrated as in [Effort and
calibration](#effort-and-calibration) (~6–10 h per readiness point for the remaining, verification-heavy
work; single features measured from PRs #66, #100, #101). Parallelism: the full and mainstream
work splits ~65–70% across 4–6 agents by area; the essentials work is mostly small fixes plus one
long `.xls` track, so about half parallelizes (≈30–50 wall-clock hours with three agents).

## What we measured against, and how

- **Target:** Microsoft Excel (Microsoft 365 channel). Version measured: **Excel for Mac
  16.113.4**, installed on the owner's Mac (version read from Spotlight metadata with `mdls`).
  Where Excel for Windows has more (Power Pivot, VBA editor depth, COM add-ins, 3D Maps), the
  Windows feature counts as part of the target.
- **Clean-room limits.** This repo's rule (AGENTS.md, "Non-negotiables") forbids reading anything
  inside the Office bundle, so the bundle's `Info.plist`, `.lproj` folders and resources were
  deliberately **not** inspected, and Excel was not launched for this pass. Excel's file formats,
  functions and features come from Microsoft's public documentation, the owner's earlier
  black-box observation (`plan/excel/observed-ui.md`, local only), and knowledge of the product.
- **Function list:** Microsoft's "Excel functions (alphabetical)" support page, fetched
  2026-10-10 and parsed (506 rows), plus 15 names the page lists under combined entries (FIND/FINDB,
  LEFT/LEFTB, LEN/LENB, MID/MIDB, REPLACE/REPLACEB, RIGHT/RIGHTB, SEARCH/SEARCHB, BETA.INV) and
  COPILOT and PY: **523 functions**. Ours: every `f!(…)` spec in `crates/functions/src/*.rs` plus
  `SPECIAL_FUNCTIONS` in `crates/calc/src/eval.rs` (evaluator special forms), minus the test-only
  `ADD2`: **513**, all of them Excel names (recounted from source on 2026-10-12).
- **Ribbon catalog:** `crates/engine/src/catalog.rs` (290 unique ids across 15 tabs) against the
  313 command ids registered in `crates/engine/src/cmd/*.rs`, recomputed from source by script and
  identical to the committed `parity-checklist.md`. The catalog is our own transcription of
  Excel's main ribbon tabs; it leaves out most contextual tabs (Picture Format, Shape Format,
  Header & Footer, Sparkline, Slicer, Timeline), the Developer and Help tabs and sub-menu items, so
  the catalog's 94.5% (89% when this estimate was made) is roughly **60–65% of Excel's full command surface** (estimate not re-derived).
- **Depth and readiness:** source reading (the model types in `crates/model/src/features.rs`,
  readers and writers in `crates/xlsx/src`, the evaluator), 478 `#[test]` functions, and the
  issue tracker: 60 open issues and 50 open PRs on 2026-10-10, 82 issues filed in total.
- No cargo build or test ran for this pass (disk constraints); numbers come from source.

## Weights

Percentages are weighted by what Excel users actually do, not by item count. Feature-area
weights (sum 100) reflect a typical business, finance and analyst workload: formulas, functions,
editing and formatting dominate; Power Query, VBA and the data model matter to fewer users but
are decisive for those who use them.

Dimension weights for **ready for real work**: features 45, file formats 15, UI/UX 12,
performance 8, stability 8, localization 4, hardware 2, platforms 2, ecosystem 2, AI 2.

## Feature areas

| Area | Weight | Breadth | Ready | Hours | Evidence |
|---|---|---|---|---|---|
| Formula language and calculation | 18 | ~92% | ~72% | 20–30 | A1/R1C1 parser, reference adjustment, dependency-graph recalc, dynamic arrays and `#SPILL!`, `A1#`, LET/LAMBDA and helpers, structured references, names, 3-D references. The 10-10 correctness bugs are fixed (15-digit equality #187; stale spills #148, #157; 3-D span deletion #183; qualified names after rename #178), iterative calculation landed (#421). Single-threaded |
| Worksheet functions | 12 | 98.1% (measured) | ~82% | 16–26 | 513 / 523 names, 504 working. Missing: GETPIVOTDATA (open PR #290), IMAGE, RTD plus cloud and add-in functions. No checked-in conformance suite against Excel-computed oracles (`gridcraft-cli check`, #416, compares the cached values of any Excel-saved file); #137 and #138 are fixed (#155, #159). [function-parity.md](function-parity.md) |
| Editing, navigation, clipboard, fill | 12 | ~90% | ~70% | 15–25 | Point mode, F4, AutoComplete, Paste Special, AutoFill, Flash Fill, Find/Replace, Go To Special. Fixed since 10-10: monthly fill (#132; #160), cut across sheets (#128; #144), Insert Copied Cells (#21; #288), key tips on every tab (#411, #412). #28 (drag-select) is open; a triage comment says the recording shows a fill-handle drag. [ui-parity.md](ui-parity.md) |
| Cell formatting, styles, conditional formatting | 10 | ~95% | ~80% | 8–14 | Full number-format language, borders, 47 cell styles, 60 table styles, all CF rule kinds including data bars, colour scales and icon sets. Gaps: font weights beyond regular/bold (#168, open), themes; the system font list is fixed (#9, closes #4) |
| Sort, filter, tables, validation, outline | 8 | ~82% | ~70% | 10–16 | Multi-level sort, AutoFilter, Advanced Filter, tables with totals, validation, outline, subtotals, consolidate. Open: subtotal header (#131). Fixed: validation edits (#129; #180), table style panic (#130; #179). Table slicers landed (#193) |
| PivotTables | 7 | ~65% | ~45% | 25–40 | Insert/Recommended, field list, three layouts, date grouping, show-values-as, refresh, calculated fields, XLSX round-trip. Done since 10-10: PivotCharts (#313). Missing: PivotTable slicers (open PR #719), timelines, calculated items, value/label filters, number grouping, GETPIVOTDATA, data-model pivots. Round-trip never checked in Excel |
| Charts and sparklines | 8 | ~50% | ~30% | 40–60 | 25 chart kinds, all 2-D. The chart model holds title, legend position, a data-labels flag, gridlines flag and axis titles; no axis scaling, error bars, per-point formatting, chart sheets, 3-D, surface, map, Pareto. Trendlines landed (#424) and every kind keeps its type through XLSX (#158, #161). [chart-parity.md](chart-parity.md) |
| Page layout, print, PDF | 5 | ~80% | ~55% | 12–20 | Page setup, print titles, breaks, Page Break Preview, headers/footers, PDF export and printing. Page Layout view (#334) and first-page / odd-even headers (#404) landed; desktop printing goes through craft-print (#459), web printing through the browser (#209); the PDF writer still uses only the 14 standard fonts without embedding (no CJK or other non-Latin text in PDF) |
| What-if and analysis | 4 | ~50% | ~40% | 22–35 | Goal Seek, Scenario Manager, data tables, Analysis ToolPak (13 tools, #320), Solver (#327), Forecast Sheet (#325). Missing: Analyze Data, Forecast Start (open PR #786) |
| Get & Transform, external data, data model | 5 | ~15% | ~10% | 60–100 | Import of CSV/TSV, JSON, HTML. No Power Query editor, no From Web, database or folder sources, no connections or refresh, no relationships or data model, external workbook links are kept unchanged on save (pass-through, #333) but never followed or refreshed |
| Insert objects | 3 | ~55% | ~45% | 25–40 | Pictures (also inside cells, #105), 9 `ShapeKind` variants (six geometric shapes, text box, icon, ink; Excel has ~170 shapes), text boxes, own icon set, ink with Ink to Shape, hyperlinks, checkboxes. Missing: SmartArt, WordArt (open PR #789), equations, signature line, objects, 3-D models, screenshot |
| Review, comments, protection, collaboration | 4 | ~55% | ~40% | 35–55 | Threaded comments, notes, spelling (system dictionary), thesaurus, sheet and workbook protection, Allow Edit Ranges with Excel-compatible password hashes (#307), comment navigation and notes-to-comments conversion (#286), password-encrypted files open (#436). Missing: Show Changes, accessibility checker, co-authoring |
| Automation | 4 | ~30% | ~20% | 12–20 | Recorded, editable GridCraft scripts (Automate tab). VBA projects are kept in `.xlsm` and saved back unchanged, never run (#19); no VBA editor, no Office Scripts compatibility |
| **Weighted** | 100 | **~75%** | **~59%** | **300–485** | |

## Dimensions

| Dimension | Weight | Ready | Hours | Evidence |
|---|---|---|---|---|
| Features | 45 | ~59% | 300–485 | Table above |
| File formats | 15 | ~50% | 80–120 | XLSX read/write is broad but unproven on a real corpus and in Excel; XLSB and ODS are values-only import; legacy `.xls` opens partly (#332, read only) and encrypted workbooks open (#436); no XML Spreadsheet 2003, SYLK, DIF, PRN; VBA projects kept, not run (#19); 100 MB XLSX fails (#175). [file-format-parity.md](file-format-parity.md) |
| UI/UX fidelity | 12 | ~55% | 50–80 | Excel-style ribbon, galleries, dialogs, formula bar, status bar; shortcuts audited against Microsoft's lists (#319, see [ui-parity.md](ui-parity.md)); key tips on every tab (#411, #412); short context menu; single window. [ui-parity.md](ui-parity.md) |
| Performance | 8 | ~35% | 35–55 | [Performance](#performance) |
| Stability | 8 | ~60% | 15–25 | Never-crash rules enforced by lint, every command fuzzed with hostile params, random command sequences fuzzed against undo/redo, XLSX round-trip and full-recalc invariants, last-resort guard in `Session::execute`. Field reports fixed: Windows Intel GPU crash (#120; #315), error on close (#117; #316); open: freeze opening from Finder (#166), screen flashing (#76) |
| Localization | 4 | ~10% | 100–165 | Catalogs in nine languages (es, zh, ja, ko, ru, pt-BR, de, cs, fr; 381 entries each, about 17% of the ~2,200 strings), Spanish formulas (#98), CJK/Thai/Hebrew font fallback, Sheet Right-to-Left mirrors the grid (#310), no shaping or bidi. [localization-parity.md](localization-parity.md) |
| Hardware | 2 | ~40% | 25–40 | GPU UI via wgpu; single-threaded calc; no pen pressure; one window. [hardware-parity.md](hardware-parity.md) |
| Platforms | 2 | ~80% | 30–60 | [Platforms](#platforms) |
| Ecosystem / add-ins | 2 | ~10% | 25–40 | [Ecosystem and AI](#ecosystem-and-ai) |
| AI features | 2 | ~15% | 30–50 | [Ecosystem and AI](#ecosystem-and-ai) |
| **Ready for real work** | 100 | **~50%** (51.6 computed: 0.45 × 59.0 features + the other rows × their weights) | **~700–1,100** | |

How the full number is built: features ready is the weighted mean of the area table (59.0 with
charts at 30%); ready for real work is the weighted mean of this table with the weights written
under [Weights](#weights). Re-checked after the 10-10 merges: charts were corrected from 38% to
30% (round-trip loss, #190 chart resize) and localization rose from 5% to 10% (#31, #104, #82,
#38). The two moves cancel: 51.6 before and after, so **~50%** stands.

## Mainstream practitioner

The typical professional Excel user: an analyst, accountant or manager who builds and maintains
models, cleans and summarizes data, makes charts and pivots, and exchanges workbooks with Excel
users every week. Left out: Power Query and the data model, VBA and add-ins, Copilot and cloud
functions, co-authoring and admin, specialist hardware, and languages beyond the user's own
(localization is reported separately).

| Area (weekly use) | Weight | Depth | Basis |
|---|---|---|---|
| Formula language and calculation | 20 | 72 | Feature-area table |
| Common worksheet functions | 15 | 85 | Missing functions are niche (GETPIVOTDATA, IMAGE, cloud); the edge bugs listed on 10-10 (#137, #138, #57) are fixed |
| Editing, navigation, clipboard, fill | 15 | 70 | Feature-area table |
| Cell formatting, styles, conditional formatting | 13 | 80 | Feature-area table |
| Sort, filter, tables, validation | 10 | 70 | Feature-area table |
| PivotTables (core: fields, layouts, grouping, refresh) | 8 | 60 | Slicers and calculated fields excluded as less frequent |
| Charts (column, bar, line, pie, scatter, combo) | 9 | 45 | Common kinds work and survive save; trendlines (#424) and resize (#218) landed; no axis options |
| Page layout, print, PDF | 5 | 60 | Latin text exports |
| Comments, notes, protection | 3 | 55 | Feature-area table |
| Pictures and shapes | 2 | 45 | Feature-area table |
| **Average depth** | 100 | **69.5** | |

Discounts for what still stops real work:

| Discount | Factor | Evidence |
|---|---|---|
| Interaction fidelity | × 0.90 | Shortcut audit done (#319) and key tips on every tab (#411, #412); charts resize (#218); #28 (drag-select) open; one window (#162); the scrollbar report (#176) is closed |
| Stability on real machines | × 0.92 | Never-crash lints, fuzzed commands and the `Session::execute` guard; field reports of a startup crash on Intel UHD (#120) and an error on close (#117) are fixed (#315, #316); a freeze opening from Finder (#166) and screen flashing (#76) are open |
| File exchange with Excel users | × 0.88 | Excel opening our output is unverified across features; some unmodelled parts still dropped on save (others kept, #333); chart kinds now survive (#158, #161); 100 MB files fail (#175); `.xls` opens partly (#332) and encrypted workbooks open (#436). Harsher than a drawing app's discount because spreadsheets are exchanged constantly |

**Mainstream practitioner: 69.5 × 0.90 × 0.92 × 0.88 = 50.6 → ~50%** (estimated). Unlike most
apps it is not higher than the full number: GridCraft's breadth is wide (functions, formatting,
ribbon), so excluding the long tail removes little, while the discounts land on exactly what
mainstream users do every week (exchange files with Excel users).

## Essentials user

Someone who only touches the core: opens a workbook someone sent, types data and simple formulas,
formats, sorts, makes a basic chart, saves and prints. Advanced options, pro workflows and exchange
edge cases are left out, along with everything excluded from the mainstream number.

| Core feature | Weight | Depth | Basis |
|---|---|---|---|
| Typing data, simple formulas, AutoSum (SUM, AVERAGE, arithmetic) | 20 | 88 | Solid; 15-digit equality (#57) is rare at this level |
| Basic formatting: fonts, bold, fills, borders, currency/percent/date formats | 15 | 85 | |
| Copy, paste, fill handle, undo/redo | 15 | 78 | Monthly fill fixed (#132; #160); #28 open |
| Rows and columns: insert, delete, widths, AutoFit | 10 | 82 | AutoFit fixed (#42, #88) |
| Sort and filter | 10 | 82 | |
| Save, Save As, reopen XLSX | 10 | 85 | |
| A basic chart | 8 | 55 | Created easily; resizes from the corner handle (#218) |
| Print and PDF | 7 | 65 | Desktop prints through craft-print (#459); web PDF downloads (#182) |
| Find and replace | 5 | 85 | |
| **Average depth** | 100 | **80.2** | |

| Discount | Factor | Evidence |
|---|---|---|
| Launch and stability | × 0.92 | Reported startup crash on Intel GPUs (#120) and error on close (#117) are fixed (#315, #316); "Issue executing" (#59) is closed |
| Discoverability and UI clarity | × 0.95 | The Excel-style ribbon is familiar and labelled; system language by default (#31); small gaps fixed (#176 scrollbar via #77, #170 macOS window controls via #323) |
| Opening files people send them | × 0.90 | Opening from Finder was fixed by #164 (closes #69); #166 is still open and #167 was closed unmerged; `.xls` (partly, #332) and encrypted workbooks (#436) open |

**Essentials user: 80.2 × 0.92 × 0.95 × 0.90 = 63.1 → ~63%** (estimated).

## User evidence

Counted from all 83 GitHub issues and 109 PRs on 2026-10-10; none of the issues were filed by
the maintainer.

- **Praise:** 3 explicit ("this is brilliant. Tiny fast native binaries", #16; "Thanks for the
  great work on GridCraft", #61; "i didnt have excel kind app on my macos", #69), plus polite
  thanks in feature requests (#39, #45, #142).
- **"Switched from Excel" reports:** 0. One counter-signal: "without macro support most
  professionals cannot switch from excel" (#6).
- **Issues by kind:** 50 core-path bugs (30 open, 20 closed) and 33 niche or feature requests
  (25 open, 8 closed). 18 of the core-path bugs came from one auditing contributor (coygeek).
- **Contributors:** 104 PRs from 28 people outside the maintainer in five days, which shows
  strong interest but not yet daily use.

## Performance

Excel recalculates on every core (multi-threaded recalculation), streams large files, and handles
1,048,576 × 16,384 sheets with millions of formulas.

| Measure | GridCraft | Excel | Kind |
|---|---|---|---|
| Sheet size limits | 1,048,576 × 16,384 | same | measured (`crates/core/src/addr.rs`) |
| Recalc threads | 1 | all cores | measured (no threading in `crates/calc`) |
| Edit → recalc, 100k dependents | ~0.4 s (2026-10-07), target < 100 ms | tens of ms | measured on the owner's machine; #66 since made range-formula recalc linear, not re-timed |
| Grid scrolling | virtualised, O(log n) geometry with custom row heights | smooth | measured in code |
| Open a ~100 MB XLSX | fails (#175) | opens | user report |
| Whole-sheet operations | Fill and Remove Duplicates on a whole-sheet selection run out of memory or time (fixed in #152) | fine | merged fix |

Work: streaming XLSX reader with shared-string and style dedup, multi-threaded recalc of
independent chains (native only; WASM stays single-threaded), whole-column reference
clamping, a `cargo xtask bench` with fixed workbooks so the numbers become measured.

## Platforms

| Platform | Excel | GridCraft |
|---|---|---|
| Windows x64 / arm64 | yes | yes (signed MSI and portable zip; also x86) |
| macOS (Apple silicon, Intel) | yes | yes (signed, notarized universal DMG) |
| Web | Excel for the web | yes (WASM; local files, no cloud) |
| iOS / iPadOS / Android | yes | no |
| Linux | no | yes (AppImage, Flatpak, deb, rpm, tarball; x86_64, aarch64, riscv64) |
| FreeBSD | no | yes |

Mobile is the gap (30–60 h for a touch layout over the same engine). GridCraft is ahead on
Linux, BSD and RISC-V, where Excel doesn't run.

## Ecosystem and AI

- **Add-ins:** Excel has Office JavaScript add-ins, COM/XLL add-ins, VBA, Office Scripts and
  Power Automate. GridCraft has its own recorded scripts, plus a control channel, MCP server and
  CLI that reach every command. VBA is not run (projects are kept, #19). A plugin or custom-function
  API (for example LAMBDA libraries and WASM custom functions) is the open-equivalent path: 25–40 h.
- **AI:** Excel has Copilot, the COPILOT() function, Analyze Data, Python in Excel and Flash
  Fill. GridCraft has Flash Fill and best-in-class agent control (any MCP client can drive every
  command headless or live), but no in-app assistant, no natural-language formulas and no
  Analyze Data. Local-model equivalents: 30–50 h; model choice needs the owner.

## Stage

**Alpha.** All six core workflows in the [alpha gate](roadmap.md#alpha-gate) pass end to end on macOS with save and reopen (two pass with non-blocking fidelity loss). Core workflows exist end to end, but depth, fidelity and file compatibility are rough
(ready ~50%, inside the 40–75% alpha band). Not beta: the main format, XLSX, has not been proven
on real-world files or confirmed to open in Excel without repair, large files fail, `.xls` is
only partly read (#332) and text in several scripts doesn't render. Distance to beta: ~25 points of readiness and
~160–240 h, itemized in [roadmap.md](roadmap.md#beta-gates).

## Effort and calibration

Hours are **Opus 5.5 agent wall-clock hours**, one agent working sequentially, including tests
and verification.

Calibrated against this repo's history:

- **First pass, 2026-10-05 → 10-07:** M0–M13 (about 72,000 lines of Rust: engine, 478 functions,
  XLSX, UI, charts, PivotTables, PDF, MCP, CLI, web, packaging) landed in commits from 22:09 on
  10-05 to 02:08 on 10-06, built by several parallel agents with work that preceded the first
  commits; we put it at **~50–80 agent-hours** for 0 → ~50% ready, so **~1–1.5 h per point**
  for the broad first pass.
- **Recent single features:** ODS import (#100, +2,232 lines with tests) and XLSB import (#101,
  +1,212 lines) each fit in one agent session of roughly 2–3 h; the linear-recalc fix (#66,
  +477 lines) about 1 h; single-function fixes (ROUND, DATE 1900, SUMIF sizing) 0.3–0.5 h each.
  That is roughly **600–1,000 lines per agent-hour** including tests.
- **Tail factor:** what remains is verification-heavy (Excel as the oracle for files, functions
  and rendering), and much of it is large self-contained subsystems (Power Query, Solver,
  localization, `.xls`). We apply **5–7× the first-pass rate**: ~6–10 h per point of readiness.

**Parallelism:** ~70% of the work splits cleanly by area (functions, charts, pivots, formats,
localization, Power Query) across 4–6 agents with one integrator; with five agents, beta is about
40–60 wall-clock hours away. **Needs a human:** checking our files in Excel (the owner's Mac),
native-speaker review of every language, scope decisions (co-authoring, cloud functions,
in-app AI models, mobile), and Windows hardware for GPU-driver crashes.

## What works today

The inventory carried over from the 2026-10-07 ROADMAP.md, updated for what landed since. Presence, not parity: the tables above grade depth.

- Engine: A1/R1C1 formula parser with reference adjustment, dependency-graph recalculation with
  dynamic arrays (spill + `#SPILL!`), cycle detection, LET/LAMBDA/MAP/REDUCE/SCAN/BYROW/BYCOL/
  MAKEARRAY, structured table references, defined names, INDIRECT/OFFSET/INDEX references,
  513 worksheet functions (478 at M0) (math, statistics and distributions, text and regex, lookup incl.
  XLOOKUP/FILTER/SORT/UNIQUE/VSTACK, dates, financial incl. bonds, engineering incl. complex
  numbers and Bessel, database, information).
- Number formats: Excel's full format-code language (sections, conditions, colours, dates and
  elapsed time, fractions, scientific, accounting fill), General narrowing to column width.
- Files: XLSX read/write (styles, themes, merges, CF, validation, tables, comments, hyperlinks,
  charts, pictures, sparklines, print settings, defined names, shared/array/dynamic formulas),
  CSV/TSV with encoding and delimiter detection, HTML export, JSON; XLSB and ODS data import (values only, 10-10).
- Editing: cell entry with type detection and auto-formats, in-cell and formula-bar editing,
  point mode (click/drag/arrow references), reference colouring, F4 anchor cycling, function
  autocomplete and argument hints, copy/cut/paste and Paste Special (values, formats, transpose,
  operations, link, skip blanks), AutoFill series (numbers, dates, months, weekdays, custom lists,
  text+number), Fill Series, Flash Fill, find/replace with wildcards, Go To / Go To Special,
  undo/redo with history, format painter. Quick selection drags retain their final endpoint;
  the fill handle explains how to fill versus select cells.
- Formatting: fonts, fills, all border styles, alignment (wrap, indent, rotation, merge), number
  formats, 47 cell styles, 60 table styles, conditional formatting (cell rules, text, dates,
  duplicates, top/bottom, averages, data bars, colour scales, icon sets, formulas), automatic
  row heights, autofit, hide/unhide, freeze panes.
  Border presets include visual diagrams; Format Cells previews draft borders before applying.
- Data: sort (multi-level, by colour, custom lists), AutoFilter (values, custom, top 10,
  average, colour), tables with totals rows, remove duplicates, text to columns, data validation
  with error alerts, grouping/outline, subtotals.
- PivotTables: Insert/Recommended PivotTables, field list pane, compact/outline/tabular layouts,
  date grouping, sort, filters, show-values-as, refresh, and XLSX round-trip.
- Page layout & print: Page Break Preview, Page Layout view, print settings, PDF export and printing.
- What-if: Goal Seek, scenarios, data tables, Solver, Analysis ToolPak, Forecast Sheet. Spelling (system word list) and thesaurus.
- Draw: pen, highlighter, eraser, ink strokes and Ink to Shape. Task panes: Comments (threaded
  replies, resolve), Watch Window, Selection, Format Chart.
- Charts: column, bar, line, area, pie, doughnut, scatter, bubble, radar, histogram, waterfall,
  funnel, treemap, sunburst, box & whisker, stock and combo charts; sparklines.
- UI (egui): Excel-style title bar with Quick Access Toolbar and AutoSave, the full ribbon with
  galleries (cell styles, table styles, colour palettes, conditional formatting menus),
  contextual Table/Chart Design tabs, formula bar with Name Box, virtualised grid (smooth
  scrolling, zoom, frozen panes, text overflow), sheet tabs (rename, reorder, colour, hide),
  status bar with Sum/Count/Average and zoom, dialogs (Format Cells, Formula Builder, Find and
  Replace, Sort, Name Manager, Data Validation, Paste Special…), command palette, dark mode.
- Agents: every action is an engine command; JSON control channel in the desktop app (pointer,
  keyboard, dialogs, screenshots); MCP server (headless or bridged); CLI (`info`, `convert`,
  `eval`, `cat`, `run`, `commands`, `functions`, `mcp`, `send`).
- Platforms: macOS, Windows, Linux, FreeBSD; web (WASM via trunk). Release workflows for
  signed/notarized macOS universal DMG, signed Windows x64/x86 MSI + zip, Linux AppImage/deb/rpm/
  tar.gz + Flatpak manifest, FreeBSD tarball and a web zip.

## Not measured yet

- Pixel comparison against Excel screenshots (planned; screenshots stay in `plan/excel/`).
- Function results against a checked-in corpus of Excel-computed oracles (a corpus repo is the planned home; `gridcraft-cli check`, #416, is the mechanism).
- Recalc and open-time benchmarks on fixed workbooks (no `cargo xtask bench` yet).
- Whether Excel opens our XLSX output without repair, across features.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-12 | minor | Audit against main: evidence text corrected for merged PRs #9, #77, #98, #105, #209, #148, #152, #157, #158, #161, #178, #183, #187, #193, #218, #286, #313, #320, #325, #327, #332, #333, #334, #404, #411, #412, #424, #436, #459 (see the PR for the row-by-row list); worksheet functions recounted at 513 / 523; the weighted scores, discount factors and hour ranges were not re-derived and are probably pessimistic in places (owner re-score needed) |
| 2026-10-11 | minor | Stability: command-sequence fuzz harness (`crates/engine/tests/command_sequence_fuzz.rs`) checks random command sequences against undo/redo, XLSX round-trip, full-recalc and structure invariants; the sequence bugs it found are fixed (no score change) |
| 2026-10-11 | minor | Password-encrypted workbooks open (#11): Essentials file-opening work 24–36 h |
| 2026-10-11 | minor | Worksheet functions 505 / 523 (96.6%; 496 working): ODDFPRICE, ODDFYIELD, ODDLPRICE, ODDLYIELD |
| 2026-10-11 | minor | Worksheet functions 503 / 523 (96.2%; 494 working): PERCENTOF, EUROCONVERT |
| 2026-10-11 | minor | Worksheet functions 502 / 523 (96.0%; 493 working): BAHTTEXT |
| 2026-10-11 | minor | Worksheet functions 502 / 523 (96.0%; 493 working): TRIMRANGE |
| 2026-10-11 | minor | View › Formula Bar and Zoom 100% are engine commands (CLI/MCP); catalog 258 → 260 / 290 (#289) |
| 2026-10-11 | minor | Review › Allow Edit Ranges; catalog 258 → 259 / 290 (#307) |
| 2026-10-11 | minor | PivotTables: calculated fields (`pivot.calculatedField`) |
| 2026-10-11 | minor | PivotTables: PivotCharts (`insert.pivotChart`) |
| 2026-10-10 | minor | Readiness-by-audience table: hours to ~95% for each of the three numbers (full 650–1,050, mainstream 200–310, essentials 60–100). Full number confirmed as the additive weighted sum over the written dimension weights (no change) |
| 2026-10-10 | minor | Added Mainstream practitioner (~50%) and Essentials user (~63%) with written weights and discounts, and user-evidence counts; re-checked the full number after the 10-10 merges (charts 38→30, localization 5→10; still 51.6, ~50%) |
| 2026-10-10 | minor | Stage checked against the new alpha gate (six core workflows, all pass): stays alpha |
| 2026-10-10 | major | Created. Full re-measure against Excel for Mac 16.113.4: functions 501 / 523 measured against Microsoft's list, catalog 258 / 290, breadth ~75%, ready ~50%, alpha; replaces the estimate tables previously in ROADMAP.md and keeps its "Working today" inventory |
