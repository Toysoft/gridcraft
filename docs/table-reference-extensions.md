# Table reference extensions

> **Last updated:** 2026-10-14 · **Change:** major (`Table[rows][columns]`: rows and columns as separate brackets; several columns) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

GridCraft reads and writes Excel's structured references (`Table1[Qty]`, `Table1[@Qty]`,
`Table1[[#Headers],[Qty]]`) as Excel does, and extends them. Every extension is stored in `.xlsx`
files as an ordinary Excel formula that computes the same result, and GridCraft evaluates exactly
that formula, so a workbook gives the same numbers in both programs. When a file is opened,
exactly those formula shapes are read back as the GridCraft forms; anything else stays as its
author wrote it.

Part of [function-parity.md](function-parity.md) (Formula language).

## Syntax

```
Table[ROWS][COLUMNS]          also written  Table[[ROWS],[COLUMNS]]
Table[COLUMNS]                one pair of brackets is always columns (as in Excel)
```

Inside the table itself the table name may be left out, as for `[@Qty]`: `[#Prev Row][Total]`.

**Rows** (first brackets):

| Rows | Meaning | Example |
|---|---|---|
| Excel's items | `#Headers`, `#Data`, `#Totals`, `#All`, `#This Row` (also `@`) | `Products[#Headers][Price, Stock]` |
| `#Prev Row`, `#Next Row` | the row above / below the formula's | `[@Qty]+[#Prev Row][Total]` |
| a key | the first row whose **first column** matches (exact, case-insensitive), one row | `Products["Bolt"][Price]`, `Products[A2][Price]`, `Products[[@Item]][Price]` |
| conditions | every row meeting all of them; `=`, `<>`, `<`, `>`, `<=`, `>=`; comma = AND | `Products[Stock>400, Product<>"Nut"][Price]` |
| nothing | the data rows | `Products[][Price, Stock]` |

A key or value is any expression. A bare word in the row brackets is an expression (a cell, a
defined name, a LET variable); a column is named only on the left of a comparison. With several
conditions each names its column. No match is `#N/A`.

**Columns** (second brackets, or the only brackets):

| Columns | Meaning | Example |
|---|---|---|
| a name | bare, `[bracketed]` or `"quoted"` | `[Price]`, `[[Unit Price]]`, `["Unit Price"]` |
| a list | several columns side by side, in the order named; returns an array | `[Price, Stock]` |
| a span | both columns and every one between | `[Product:Price]`, `["Unit Price":Stock]` |
| `$` | the column stays when the formula is filled sideways (one column only) | `[$Price]` |
| `[]` | every column | `Products["Bolt"][]` (the whole Bolt row) |

Lists, spans and quoted names combine: `Products[Stock>400]["Unit Price", Product:Stock]`.

One pair of brackets is always columns (`Products[Price, Stock]`, `Products["Unit Price"]`), or one
of Excel's own forms (`Table1[#Headers]`, `Table1[@Qty]`); it never selects rows by a key or a
condition, so nothing depends on guessing what its content means.

A reference Excel's syntax can express is shown in Excel's spelling: `Products[#Headers][Qty]` is
`Products[[#Headers],[Qty]]`, `Products[@][Qty]` is `Products[@Qty]`, `Products[Price:Stock]` is
`Products[[Price]:[Stock]]`.

## Excel behaviour GridCraft reproduces

Observed in Excel for Mac 16.113.4 with synthetic workbooks (clean-room: behaviour only).

| Behaviour | Excel | GridCraft before | GridCraft now |
|---|---|---|---|
| Filling a formula sideways (fill handle, Fill Right) moves the columns of its structured references along the table: `Table1[Price]` → `Table1[Stock]` → … | yes; past the last column it wraps to the first | columns never moved | same as Excel, for lists, spans and conditions too |
| Copy and paste keeps structured references as they are | yes | yes | yes |
| `Table1[[Qty]:[Qty]]` (a one-column range) does not move when filled sideways: Excel users' way to anchor a column | yes | n/a (nothing moved) | yes, shown as `Table1[$Qty]` |
| Inside its own table a reference may leave out the table name, also in files (`[[#This Row],[Qty]]`, `[Qty]`, `[]`) | yes | yes | yes, for the extensions too |
| Renaming a table or a column (by editing its header) updates every structured reference to it | yes | cell formulas only | cell formulas and defined names |

## What files hold

| GridCraft | In the `.xlsx` file | Excel |
|---|---|---|
| `Products["Bolt"][Price]` | `INDEX(Products[Price],MATCH("Bolt",INDEX(Products[],0,1),0))` | every version |
| `Products["Bolt"][Product:Price]`, `Products["Bolt"][]` | `INDEX(Products[[Product]:[Price]],MATCH(…),0)`, `INDEX(Products[],MATCH(…),0)` | 365 / 2021 (older: Ctrl+Shift+Enter) |
| `Products[Stock>400, Product<>"Nut"][Price]` | `_xlfn._xlws.FILTER(Products[Price],(Products[Stock]>400)*(Products[Product]<>"Nut"),NA())` | 2021 / 365 |
| `Products[Stock=410][Product]` (one condition) | `_xlfn._xlws.FILTER(Products[Product],Products[Stock]=410,NA())` | 2021 / 365 |
| `Products[…][Price, Stock]` (a list) | `HSTACK(Products[Price],Products[Stock])` in place of the one column | 2024 / 365 |
| `[#Prev Row][Total]`, `[#Next Row][Qty]` | `OFFSET([[#This Row],[Total]],-1,0)`, `OFFSET([[#This Row],[Qty]],1,0)` | every version |
| `Products[$Price]`, `Products[@$Qty]` | `Products[[Price]:[Price]]`, `Products[[#This Row],[Qty]:[Qty]]` (Excel's own idiom) | every version |

- Every stored form names columns only through structured references, never in a text, so
  renaming the table or a column in Excel updates them (checked: `Products` → `Parts`,
  `Price` → `Cost`, values unchanged).
- Excel calculates the same values (checked for each form above, including the name-less forms
  inside the table).
- `#Prev Row` / `#Next Row` behave as OFFSET does: above the first data row is the header, below
  the last data row the totals row or the cell under the table. OFFSET is volatile, so these
  recalculate on every change.
- Reading back is by exact shape, so hand-written formulas are not reinterpreted: a plain
  `INDEX(Products[Price],MATCH(A1,Products[Product],0))`, `FILTER(…)` without the `NA()`, or an
  `OFFSET(…)` of other than one row stays as written. A file whose author wrote exactly one of the
  shapes above is shown in the GridCraft form, which computes the same and is written back
  unchanged. `HSTACK` of columns of the same table and rows is read as a list of columns.
- Lookups by key saved by earlier development builds as
  `VLOOKUP(key,Products[],MATCH("Price",Products[#Headers],0),FALSE)` (or `…,0)` when the column
  was anchored) read back too, and are written in the INDEX form on the next save.
- Inside the brackets, keys and values are always in file syntax (`,` separators, `.` decimals),
  whatever the interface language.

## Editing

- **Which form a row gets** depends on where the formula is:
  - **inside the table**, rows relative to the formula's own: `[@Qty]`, `[#Prev Row][Qty]`,
    `[#Next Row][Qty]`, which fill down with the formula; any other row by its key,
    `["Bolt"][Qty]` (inside its table a reference leaves out the table name, as Excel writes it);
  - **outside the table**, rows by key only, `Products["Bolt"][Qty]`, even on the formula's own row.

  A key is used only when the table's first column is unique (no entry repeated, texts compared
  ignoring case) and the row's entry is a text, number or boolean without `*`, `?` or `~`;
  otherwise the cell stays an address.
- **Shift+F4** (`formulas.useTableNames`; `formulas.tableNames` returns the text without changing
  anything) rewrites the A1 references of the formula being edited, or of the selected cells, that
  name part of a table: `C2:C9` → `Products[Stock]`, a row as above, `A3:C3` → `Products["Bolt"][]`.
  Anything else (a block of rows) stays A1. Undoable.
- **F4** on a reference to one row goes through its row and column anchors like `A1` → `$A$1` →
  `A$1` → `$A1`: the row anchored is its key, relative its `@` / `#Prev Row` / `#Next Row`
  (`[@Qty]` → `["Bolt"][$Qty]` → `["Bolt"][Qty]` → `[@$Qty]`). A row with no other form (a key
  from outside the table, or two rows away) toggles only the column's `$`. On other table
  references F4 toggles `$` on the columns; with conditions it cycles both / columns only /
  conditions only / none. Elsewhere F4 cycles `$A$1`.
- **Suggestions** while typing a formula: table names (before the function names); in the row
  brackets the columns, the row items and the first column's entries as keys; after a comparison
  that column's entries; in the column brackets the columns, written bare, `[bracketed]` or
  `"quoted"` as started. Tab accepts.
- **Colours**, as Excel's: while a formula is edited, each table reference is coloured like a cell
  reference and the cells it covers are outlined in the same colour on the sheet: a column's data
  (`Products[Stock]`), the cell a key finds (`Products["Bolt"][Price]`), the row's cell
  (`[@Price]`), the table's data (`Products`); a filter outlines the columns it returns. The same
  reference text has the same colour.
- **Point mode** writes table references by default, as Excel's *Use table names in formulas*:
  clicking, dragging or arrowing over cells of a table while typing a formula writes the same
  forms as Shift+F4. A reference keeps the way it started while arrows or a drag carry it on:
  one started on a cell outside the table stays an address when it moves into the table, the
  usual way to point at a table's cells by address; one started in the table goes back to a
  table reference once it covers one again (`B3:B5` → `[Qty]`). File ▸ Options ▸ Editing ▸
  *Use table names in formulas* (`options.tableNames`) turns it off.

## History

- 2026-10-13: keyed references `Table1[Price,"Bolt"]` (column first, then the key), stored as
  VLOOKUP; filling sideways starts moving table columns; `$` anchors.
- 2026-10-14: conditions and several matches (FILTER); lookups by key stored as INDEX/MATCH so
  Excel renames them; `#Prev Row` / `#Next Row` (OFFSET); then the syntax became
  `Table[rows][columns]`, matching the order of Excel's `[[#This Row],[Qty]]`, with lists and spans
  of columns and `[]` for every column. The column-first spelling of the first day is gone; files
  are unaffected, as they hold the same Excel formulas.

## Where it lives

`crates/formula/src/table_ref.rs` (`TableRef`, `RowSel`, `ColumnSpan`, `Condition`, lowering and
reading back), parser, lexer and printer in the same crate; evaluation in `crates/calc/src/eval.rs`
(by lowering); files in `crates/xlsx/src/fmla.rs` and `array_formula.rs`; fill in
`crates/formula/src/adjust.rs` (`shift_table_columns`) and `crates/engine/src/fill.rs`; renames in
`crates/engine/src/tables.rs` and `cmd/insert.rs`; Shift+F4 in `crates/engine/src/table_refs.rs`;
editor (F4, Shift+F4, suggestions) in `crates/ui-egui/src/editor.rs` and `formula_bar.rs`.
