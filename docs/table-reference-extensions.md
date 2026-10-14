# Table reference extensions

> **Last updated:** 2026-10-14 · **Change:** new (keyed references, filters, neighbouring rows, anchored columns, A1 → table names) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

GridCraft reads and writes Excel's structured references (`Table1[Qty]`, `Table1[@Qty]`,
`Table1[[#Headers],[Qty]]`) as Excel does, and adds a few forms of its own on top of them. Every
addition is stored in `.xlsx` files as an ordinary Excel formula that computes the same result, and
GridCraft evaluates exactly that formula, so a workbook gives the same numbers in both programs.
When a file is opened, exactly those formula shapes are read back as the GridCraft forms; anything
else stays as its author wrote it.

Part of [function-parity.md](function-parity.md) (Formula language).

## What Excel does, and GridCraft now reproduces

Observed in Excel for Mac 16.113.4 with synthetic workbooks (clean-room: behaviour only).

| Behaviour | Excel | GridCraft before | GridCraft now |
|---|---|---|---|
| Filling a formula sideways (fill handle, Fill Right) moves the columns of its structured references along the table: `Table1[Price]` → `Table1[Stock]` → … | yes; past the last column it wraps to the first | columns never moved | same as Excel |
| Copy and paste keeps structured references as they are | yes | yes | yes |
| `Table1[[Qty]:[Qty]]` (a one-column range) does not move when filled sideways: Excel users' way to anchor a column | yes | n/a (nothing moved) | yes, shown as `Table1[$Qty]` |
| A reference inside its own table may leave out the table name: `[@Qty]`, also in files | yes (reads files with bare `[[#This Row],[Qty]]`, `[Qty]`, `[]`) | yes | yes, also for the forms below |
| Renaming a table or a column (by editing its header) updates every structured reference to it | yes | cells only | cells and defined names |

## What is new (not in Excel)

### Keyed references: a cell by its row's key

```
=Products[Price,"Bolt"]
```

The `Price` of the row whose first column is `"Bolt"`: an exact, case-insensitive match, the first
matching row (like VLOOKUP). The key is any expression: `Products[Price,[@Item]]`,
`Products[Price,A2&"-x"]`, even another keyed reference. No match is `#N/A`; a column the table
doesn't have is `#REF!`, as for `Products[Colour]`.

### Filters: every row that meets conditions

```
=Products[Product,Stock>400,Price<0.2,Product<>"Nut"]
```

Conditions name their column and compare it with `=`, `<>`, `<`, `>`, `<=` or `>=`; any number of
them, joined by AND; values are any expressions (`Qty>=[@Qty]`). The result is the column's cells
of every matching row: one value, or an array that spills down (or feeds `SUM(...)` and the like).
No match is `#N/A`. A bare key is allowed only on its own; with several conditions each names its
column.

### Neighbouring rows

```
=[@Qty]+[Total,[#Prev Row]]        running total
=[Qty,[#Next Row]]-[@Qty]          change to the next row
=Log[Qty,[#Prev Row]]              from outside the table, on one of its rows
```

The cell of the column one row above or below the formula's row. Like Excel's `OFFSET`, which it is
stored as: above the first data row is the header, below the last data row is the totals row or
the cell under the table. `Table1[Qty,[#This Row]]` is accepted as another spelling of
`Table1[@Qty]`, and Excel's item-first order `Table1[[#Prev Row],[Qty]]` is accepted too.

### Anchored columns

```
=SUM(Products[$Price])     =Products[$Stock,"Bolt"]     =Products[@$Qty]     =Products[$Price,$Stock>400]
```

A `$` before a column keeps it when the formula is filled sideways; unanchored columns move along
the table (see above). One column only, not a range of columns. A column whose name starts with
`$` is written in its own brackets, `Table1[[$ Amount]]`, as Excel requires anyway.

### Editing

- **Shift+F4** (`formulas.useTableNames`; `formulas.tableNames` returns the text without changing
  anything) rewrites the A1 references of the formula being edited, or of the selected cells, that
  name part of a table: `C2:C9` → `Products[Stock]`, the formula's own row → `[@Stock]`, the row
  above or below → `[Stock,[#Prev Row]]` / `[Stock,[#Next Row]]`, a cell of another row →
  `Products[Stock,"Bolt"]` by its key (only when the key is a text, number or boolean that no
  earlier row repeats and holds no wildcard). Anything else stays A1. Undoable.
- **F4** on a table reference toggles `$` on its column; on a filter it cycles both / column only /
  conditions only / none, like the `$` of a cell address. Elsewhere F4 cycles `$A$1` as before.
- **Suggestions** while typing a formula: table names (before the function names), a table's columns
  and `#` items inside its brackets, and a column's entries while a key or a condition's value is
  typed. Tab accepts.
- Not done: clicking a table cell in point mode still inserts an A1 reference.

## Backward compatibility: what files hold

| GridCraft | In the `.xlsx` file | Excel version |
|---|---|---|
| `Products[Price,"Bolt"]` | `INDEX(Products[Price],MATCH("Bolt",INDEX(Products[],0,1),0))` | every version |
| `Products[Price,Stock>400,Product<>"Nut"]` | `_xlfn._xlws.FILTER(Products[Price],(Products[Stock]>400)*(Products[Product]<>"Nut"),NA())` as a dynamic-array formula | 2021 / 365 |
| `Products[Price,Stock=410]` (one condition) | `_xlfn._xlws.FILTER(Products[Price],Products[Stock]=410,NA())` | 2021 / 365 |
| `[Total,[#Prev Row]]`, `[Qty,[#Next Row]]` | `OFFSET([[#This Row],[Total]],-1,0)`, `OFFSET([[#This Row],[Qty]],1,0)` | every version |
| `Products[$Price]`, `Products[@$Qty]` | `Products[[Price]:[Price]]`, `Products[[#This Row],[Qty]:[Qty]]` (Excel's own idiom) | every version |
| `Products[$Price,"Bolt"]` | `INDEX(Products[[Price]:[Price]],MATCH("Bolt",INDEX(Products[],0,1),0))` | every version |

- Every stored form names columns only through structured references, never in a text, so
  renaming the table or a column in Excel updates them (checked: `Products` → `Parts`, `Price` →
  `Cost`, `Stock` → `Qty`, values unchanged).
- Excel calculates the same values (checked for each form above, including the name-less forms
  inside the table and the boundary rows of `[#Prev Row]` / `[#Next Row]`).
- Reading back is by exact shape, so hand-written formulas are not reinterpreted: a plain
  `INDEX(Products[Price],MATCH(A1,Products[Product],0))`, `FILTER(...)` without the `NA()`, or
  `OFFSET(...)` by other than one row stays as written. A file whose author wrote exactly one of
  the shapes above is shown in the GridCraft form, which computes the same and is written back
  unchanged.
- Files saved by GridCraft builds from 2026-10-13 stored lookups by key as
  `VLOOKUP(key,Products[],MATCH("Price",Products[#Headers],0),FALSE)` (with `0` for `FALSE` when the
  column was anchored); those read back as keyed references and are written in the INDEX form on the
  next save.
- Inside the brackets, keys and values are always in file syntax (`,` separators, `.` decimals),
  whatever the interface language.

## Semantics changed during development

- Filling sideways now moves unanchored table columns (Excel's behaviour); before, nothing moved.
- A condition on a named column (`Products[Price,Stock=410]`) filters, returning every match;
  for a few hours on 2026-10-13 it meant the first match (stored as INDEX/MATCH with a trailing
  `,1`, which is no longer read back).
- A missing column in a lookup by key is `#REF!` (was `#N/A` while the column was found by its
  header text).

## Where it lives

`crates/formula/src/ast.rs` (`KeyedRef`, `Condition`, `StructRef::lower`, `lower_extensions`,
`raise_extension`), parser and printer in the same crate; evaluation in `crates/calc/src/eval.rs`
(by lowering); files in `crates/xlsx/src/fmla.rs` and `array_formula.rs`; fill in
`crates/formula/src/adjust.rs` (`shift_table_columns`) and `crates/engine/src/fill.rs`; renames in
`crates/engine/src/tables.rs` and `cmd/insert.rs`; Shift+F4 in `crates/engine/src/table_refs.rs`;
editor (F4, Shift+F4, suggestions) in `crates/ui-egui/src/editor.rs` and `formula_bar.rs`.
