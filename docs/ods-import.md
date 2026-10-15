# OpenDocument spreadsheets

Open an `.ods` file with File → Open, drag it onto GridCraft, or pass it to the CLI. Save a
workbook as `.ods` with File → Save As (OpenDocument Spreadsheet), File → Export → OpenDocument
Spreadsheet, the `file.exportOds` command, or `gridcraft-cli convert book.xlsx book.ods`.

## What is read and written

| Feature | Open | Save |
|---|---|---|
| Sheets, names, hidden sheets, tab colours | yes | yes |
| Numbers, text, booleans, errors, dates, times, percentages | yes | yes |
| Formulas (OpenFormula `of:`, older `oooc:`, Excel-syntax `msoxl:`), with saved results | yes, live | yes |
| Array formulas | yes | yes |
| Named ranges and named expressions (workbook and sheet scope) | yes | yes |
| Cell styles: font, size, bold, italic, underline, strikethrough, colour, super/subscript | yes | yes |
| Fill colour, borders (style, width, colour, diagonals) | yes | yes |
| Alignment: horizontal, vertical, wrap, shrink to fit, rotation, indent | yes | yes |
| Cell protection flags | yes | yes |
| Number formats (ODF data styles, including negative sections and colours) | yes | yes |
| Column widths, row heights, hidden rows and columns, column default styles | yes | yes |
| Merged cells | yes | yes |
| Charts, images, shapes, comments, conditional formats, validation | no (warning) | no |

Formulas stay live: editing an input recalculates them. A formula GridCraft can't translate keeps
its saved result as a value and the import shows a warning. Excel functions that OpenFormula has
no name for (`IFS`, `XLOOKUP`, `TEXTJOIN`…) are written with the `COM.MICROSOFT.` prefix other
ODF applications use; structured table references are written in Excel syntax (`msoxl:`).

Formatting a writer spreads across huge empty areas (for example a fill on every row to the end of
the sheet) is skipped with a warning rather than expanded into millions of cells.

## Saving

An opened `.ods` file is not used as the workbook's save destination: Save opens Save As and
suggests a new `.xlsx`, so features ODS can't hold (and features GridCraft doesn't read from
ODS) are never lost by accident. Saving as `.ods` is an export, like CSV: the workbook keeps its
own file and unsaved state.

```sh
gridcraft-cli info source.ods
gridcraft-cli convert source.ods converted.xlsx
gridcraft-cli convert book.xlsx book.ods
```

CLI import warnings go to stderr, leaving structured output on stdout available for scripts.
