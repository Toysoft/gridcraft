# Gaps

> **Last reviewed:** 2026-10-12 · **Last updated:** 2026-10-12 · **Change:** minor (audit against main: stale entries corrected with evidence; §15 reduced to one line) · **Target:** Microsoft Excel (Microsoft 365)

Every known shortfall against Excel, one entry each, ranked by user impact (what blocks someone
using GridCraft for real work first). This is the work list: pick from the top, check the open
PRs first (225 open on 2026-10-12; several already address entries here), and update the entry
when you land work. Estimates are Opus 5.5 agent-hours. Area detail lives in the linked parity
docs; the summary is in [target-app-parity.md](target-app-parity.md).

Kinds: **F** feature · **U** UI/UX · **FF** file format · **H** hardware · **L** localization ·
**P** performance · **S** stability.

## Beta blockers

No alpha blockers: every core workflow passes the [alpha gate](roadmap.md#alpha-gate).


### 1. XLSX output not proven in Excel or on real files (FF)
- **Missing:** a run over a real-world XLSX corpus, and confirmation that Excel opens what we save
  with no repair prompt (PivotTables, charts, CF, validation, dynamic arrays, tables).
- **Evidence:** round-trip tests cover our own files only (`crates/xlsx/src/tests/`); PivotTable
  round-trip noted as "unverified in real Excel" since 10-07; #64 (dynamic-array flag) shows the
  class of bug (#267 writes `@` as `_xlfn.SINGLE`; the issue is still open). `gridcraft-cli check`
  (#416) now measures recalculation against the cached values and the round trip (package
  validation, sheets, formulas, formats) on any set of Excel-saved files, but no corpus run is
  recorded and Excel itself has not confirmed our output.
- **Impact:** the main format; a repair prompt or silent loss destroys trust.
- **Estimate:** 15–25 h, plus owner time in Excel. [file-format-parity.md](file-format-parity.md)

### 2. Large workbooks don't open; recalc is single-threaded (P, H)
- **Missing:** streaming read for ~100 MB XLSX; multi-threaded recalc; 100k-dependent edit under
  100 ms (was ~0.4 s).
- **Evidence:** #175 is open: `MAX_PART` caps one decompressed XLSX part at 512 MiB
  (`crates/xlsx/src/package.rs`); no crate manifest depends on `rayon` and `crates/calc` has no
  thread use. Merged: #66 (range-formula recalc linear), #151 (current region past 10,000 rows),
  #152 (whole-sheet Fill and Remove Duplicates), #154 (repeated headers in linear time), #352
  (no double range copy for aggregations).
- **Impact:** finance and data users routinely have big workbooks.
- **Estimate:** 25–40 h (not re-estimated since the merges above). [target-app-parity.md](target-app-parity.md#performance), [hardware-parity.md](hardware-parity.md)

### 3. Some unmodelled XLSX content is dropped on save (FF)
- **Missing:** slicers and their caches, timelines, the data model, query tables attached to
  tables, unmodelled `extLst` entries, chart details. Pass-through (#333, for #330) keeps custom XML,
  external links, connections, sheet query tables, printer settings (#324), form and ActiveX
  controls, OLE objects and sheet backgrounds; never checked in Excel (repair prompts unknown).
- **Evidence:** `crates/xlsx/src/passthrough.rs` (what is kept, and why the rest isn't; slicer
  parts are on its not-kept list). Table slicers exist in the app since #193
  (`crates/engine/src/cmd/slicer.rs`), but `crates/xlsx` has no code that writes them.
- **Impact:** open-edit-save still simplifies workbooks with slicers, timelines or Power Pivot.
- **Estimate:** 6–10 h. [file-format-parity.md](file-format-parity.md)

### 4. Charts lose options on round trip; shallow chart model (F, FF)
- **Missing:** axis scaling, label options, error bars, per-point formatting, chart sheets, 3-D
  and surface kinds. (Every kind the model has now survives XLSX.)
- **Evidence:** `Chart` holds title, legend position, two flags, axis titles, style, source
  range and a PivotChart binding; `Series` adds colour, smooth and trendlines
  (`crates/model/src/features.rs`). Landed: kinds that survive XLSX (combo, stock #158;
  histogram, box, waterfall, funnel, treemap, sunburst #161), chart resize (#218, closes #190),
  smooth lines (#62), Switch Row/Column (#116), trendlines read, written and drawn (#424),
  PivotCharts (#313), Recommended Charts dialog (#301), Select Data axis labels (#303).
- **Impact:** charts are in most business workbooks.
- **Estimate:** 25–35 h for beta (40–60 h full); not re-estimated since the merges above. [chart-parity.md](chart-parity.md)

### 5. Text in non-Latin scripts doesn't render or export (L)
- **Missing:** complex shaping (Arabic, Devanagari, Thai), bidi, non-Latin PDF.
- **Evidence:** system-font fallback for CJK, Thai and Hebrew landed in #38 (closed #32; the CJK
  fallback in #25); #142 (Chinese text) is still open, awaiting confirmation; #54 (symbols as
  boxes on Fedora) is still open, with symbol fallback faces in #298; formula bar and dialogs
  fall back to the cell font (#402); no shaping for Arabic or Devanagari; the PDF writer uses the
  14 standard fonts only (`Helvetica` metrics in `crates/pdf`, no embedded font code).
- **Impact:** blocks most of the world's users even with an English UI.
- **Estimate:** 12–20 h. [localization-parity.md](localization-parity.md)

### 6. Calculation correctness bugs (F)
- **Missing:** nothing from the 10-10 list is open on main. Remaining risk is unmeasured edge
  cases (there is no checked-in Excel-computed oracle corpus), and iterative calculation has no
  dedicated Options dialog (command only, per function-parity.md).
- **Evidence:** fixed and merged: 15-significant-digit equality (#187 for #57; #248 ties at the
  16th digit), stale spills (#148 for #133 and #139; #157 blocked spills; #155 ranges past the
  used range), legacy array range fill (#156), 3-D span deletion (#183 for #140), qualified names
  after sheet rename (#178 for #134), iterative calculation for circular references (#421).
- **Impact:** wrong numbers are worse than missing features.
- **Estimate:** 10–15 h was the 10-10 figure; not re-estimated now that the listed bugs are fixed. [function-parity.md](function-parity.md#formula-language)

### 7. Legacy `.xls` only partly read; encrypted workbooks can't be saved with a password (FF)
- **Evidence:** #121 (PR #332): Excel 97-2003 (BIFF8) `.xls` files open (values, formulas, number
  formats, merges, sizes, names); fonts, fills, borders, charts, pictures, comments, conditional
  formats, validation and BIFF5 files are not read, and `.xls` is never written. Password-encrypted
  `.xlsx` workbooks (Agile and Standard encryption) open with their password since #11 (PR #436),
  but are saved without it: GridCraft can't write an encrypted file yet. Encrypted `.xls`
  (FILEPASS) and RC4-encrypted files are still refused with a message.
- **Impact:** common in finance, government and anything older than 2007; encrypted files are
  routine in business.
- **Estimate:** 14–21 h (`.xls` styles and objects 10–15, encryption 4–6). [file-format-parity.md](file-format-parity.md)

### 8. PivotTables: no PivotTable slicers, timelines or calculated items; unverified in Excel (F)
- **Evidence:** `parity-checklist.md` lists `insert.timeline` as the only missing slicer/pivot id;
  `insert.slicer` and `table.insertSlicer` are table slicers (#193), and `insert.pivotChart`
  (#313) and calculated fields (#306) are done. PivotTable slicers from XLSX files are in open PR #719; GETPIVOTDATA
  is in open PR #290; calculated items are missing.
- **Impact:** pivots are the analyst's main tool.
- **Estimate:** 20–30 h for beta (25–40 h full); not re-estimated.

### 9. Keyboard-first users: shortcuts and context menus (U)
- **Evidence:** key tips are complete: 364 sequences across all tabs (#411, #412; initial #43),
  closing #33 and #49; the shortcut audit (#319) closed #110; #28 (selection) is open; a triage comment
  says the recording shows a fill-handle drag, which behaves as in Excel. Still missing: shortcuts for features that don't
  exist yet (listed in [ui-parity.md](ui-parity.md#shortcut-audit-110)), a fuller context menu,
  Quick Analysis (open PR #795), paste-option icons.
- **Estimate:** 12–20 h was the 10-10 figure; the key-tips part is done, not re-estimated. [ui-parity.md](ui-parity.md)

### 10. Crashes and freezes reported from the field (S)
- **Evidence:** fixed on main: #120 (Intel UHD crash on Windows; #315), #117 (error on close,
  macOS; #316), #130 (caught panic in table styles; #179), #69 (documents from Finder; #164),
  #339 (DXC runtime; #382), #226 (startup font panic; #277). Open: #166 (freeze opening from
  Finder; #164 landed but the issue is open and #167 was closed unmerged), #76 (flashing on
  Windows 11). Command-sequence fuzzing against undo/redo, XLSX round trip and full recalculation landed (#425).
- **Estimate:** 8–12 h plus an autosave/recovery soak (not re-estimated).

## After beta

### 11. Localization is partial (L)
- Interface catalogs in nine languages besides English (es, zh, ja, ko, ru, pt-BR, de, cs, fr;
  381 entries each, ja 377: about 17% of the ~2,200 strings; #31, #104, #82, #242, #271, #335,
  #413); Spanish function names and `;` separators (#98, closes #45). No other formula language,
  no localized number/date formats; German formulas (#163) were closed unmerged. More languages and
  dialog translation are open PRs (#650, #663, #713, #743, #721, #778, #739, #676, #652, #669).
  **Estimate:** 100–165 h including #5 (not re-estimated). [localization-parity.md](localization-parity.md)

### 12. Get & Transform, external data, data model (F)
- No Power Query editor, connections, refresh, From Web/database/folder sources, relationships or
  data model (`data.fromWeb`, `data.relationships` missing). **Estimate:** 60–100 h.

### 13. Analysis tools: Analyze Data, Forecast Start (F)
- Solver (#327), the Analysis ToolPak with 13 tools (#320), Forecast Sheet (#325) and the
  FORECAST.ETS family (#317) are in. Missing: `data.analyzeData`; the Forecast Start (hindcast)
  option is open PR #786. **Estimate:** 15–25 h was the figure with Solver and ToolPak still missing (not re-estimated).

### 14. Page Layout view and print polish (U, F)
- Page Layout view landed (#334): paper view with rulers and in-place header/footer editing;
  Different First Page and Odd & Even pages landed (#404). Remaining: pictures in headers/footers,
  dragging margins on the ruler (open PR #794). Desktop submits directly through craft-print with
  printer choice, copies, duplex and grayscale (#459); browser printing stays available (#209).
  Print page ranges and the driver tray/options UI remain. **Estimate:** 5–10 h.

### 15. Remaining functions and an oracle suite (F)
- 10 missing (GETPIVOTDATA, IMAGE, RTD, STOCKHISTORY, DETECTLANGUAGE, TRANSLATE, CALL,
  REGISTER.ID, COPILOT, PY) and 9 stubs (7 CUBE functions, WEBSERVICE, FILTERXML): 513 / 523 by
  name, 504 working, counted from the registry on main ([function-parity.md](function-parity.md);
  GETPIVOTDATA is open PR #290). No checked-in Excel-computed oracle corpus (`gridcraft-cli check`,
  #416, compares the cached values of any Excel-saved workbook); #137 and #138 are fixed (#155,
  #159). **Estimate:** 16–26 h (function-parity.md's figure, not re-derived).

### 16. Collaboration and review (F)
- Landed: comment navigation, Show Comments, Show All Notes, Convert to Comments (#286),
  threaded comments in XLSX (#302), Allow Edit Ranges (#308), Protect Workbook and Protect Sheet
  with Excel-compatible hashes (#384, #387). Missing: co-authoring, Show Changes
  (`review.showChanges`); cloud storage requested (#86, open). **Estimate:** 35–55 h (co-authoring scope needs the owner; not re-estimated).

### 17. Insert objects (F)
- Missing ids: `insert.smartArt`, `insert.screenshot`, `insert.signatureLine`, `insert.object`,
  `insert.equation`, `insert.wordArt` (open PR #789), `insert.map`; no 3-D models. `ShapeKind` has 9
  variants (six geometric shapes, text box, icon, ink) vs ~170 Excel shapes. Pictures in cells
  landed (#105); #61 (place a picture in a cell) is still open. **Estimate:** 25–40 h.

### 18. Accessibility (U)
- AccessKit on, but no accessible grid/ribbon tree, no high contrast, no checker. **Estimate:** 12–18 h.
  [ui-parity.md](ui-parity.md#accessibility)

### 19. XLSB/ODS beyond values; minor formats (FF)
- XLSB and ODS import values only, no write (ODS formulas are open PR #765); no `.prn`, `.dif`,
  `.slk`, XML Spreadsheet 2003. CSV/TSV now write displayed values, and templates and `.xlsm` are
  saved with their own content types (#422). **Estimate:** 15–23 h (not re-estimated).

### 20. Multiple windows; pen pressure (H)
- One window (#162, PR #171); no pressure or tilt in ink. **Estimate:** 10–14 h.
  [hardware-parity.md](hardware-parity.md)

### 21. Automation and add-ins (F)
- VBA projects are kept in `.xlsm` and written back unchanged, never run (#19, closes #6); no VBA
  editor, no Office Scripts compatibility, no plugin or custom function API, no Python in Excel (`PY` is
  missing). **Estimate:** 25–40 h plus an owner decision.

### 22. AI features (F)
- No in-app assistant, natural-language formulas or Analyze Data; agent control via MCP is
  already strong. **Estimate:** 30–50 h; model choice needs the owner.

### 23. Mobile (platform)
- No iOS, iPadOS or Android layout. **Estimate:** 30–60 h.

### 24. Fonts and look (U)
- Font weights beyond regular/bold (#168, open), Calibri metrics (#29, open), Linux title bar
  (#54, open), icon consistency across apps (#173, open). Fixed: system font list (#4; #9),
  macOS Dock icon (#169; #222) and window controls (#170; #323). **Estimate:** 7–12 h (not re-estimated).

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-12 | minor | Audit against main (docs only, evidence per row in the PR): §1, §2, §3, §4, §5, §6, §7, §8, §9, §10, §11, §13, §14, §15 (four contradictory bullets reduced to one: 10 missing, 9 stubs), §16, §17, §19, §21, §24 corrected for merged PRs; estimates and scores not re-derived |
| 2026-10-12 | minor | Desktop File > Print now discovers printers and submits PDF jobs through pinned craft-print with printer choice, copies, duplex and grayscale; browser printing and PDF export remain available. Remaining print parity includes page ranges and driver-specific tray/options UI; physical printer fidelity needs hardware validation. |
| 2026-10-11 | minor | §6: iterative calculation for circular references landed (Maximum Iterations, Maximum Change) |
| 2026-10-11 | minor | §15: trim-reference operators (`A1:.E10`, `A1.:E10`, `A1.:.E10`) parse and evaluate |
| 2026-10-11 | minor | §4: trendlines (six types, forecast, equation and R²) read, written and drawn |
| 2026-10-11 | minor | §7: password-encrypted workbooks open with their password; saving with one remains (#11) |
| 2026-10-11 | minor | §16: comment navigation, Show Comments, Show All Notes and Convert to Comments landed (#281) |
| 2026-10-11 | minor | §7: `.xls` (BIFF8) import of values, formulas and number formats (#121) |
| 2026-10-11 | minor | §3: XLSX pass-through keeps custom XML, external links, connections, query tables, printer settings, controls and OLE objects (#330, #324) |
| 2026-10-11 | minor | §13: Forecast Sheet landed (#322) |
| 2026-10-11 | minor | §15: ODDFPRICE, ODDFYIELD, ODDLPRICE, ODDLYIELD added (18 functions missing) |
| 2026-10-11 | minor | §15: PERCENTOF and EUROCONVERT added (20 functions missing) |
| 2026-10-11 | minor | §15: BAHTTEXT added (21 functions missing) |
| 2026-10-11 | minor | §15: TRIMRANGE added (21 functions missing; trim-reference operators still missing) |
| 2026-10-11 | minor | §16: Allow Edit Ranges landed (#307) |
| 2026-10-11 | minor | §8: calculated fields added |
| 2026-10-11 | minor | §8: PivotCharts added |
| 2026-10-11 | minor | §13, §15: FORECAST.ETS family landed (#314) |
| 2026-10-10 | minor | Updated for the 10-10 merges (#17, #31, #38, #62, #82, #104, #116) and the new chart resize bug (#190) |
| 2026-10-10 | minor | Noted that no gap blocks the alpha gate |
| 2026-10-10 | major | Created: 24 ranked gaps with evidence, from the full re-measure and the 60 open issues |
