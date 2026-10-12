---
title: Which file formats can GridCraft open?
description: GridCraft 0.5.0 opens .xlsx, .xlsm, .csv, .tsv and .txt fully, reads only the values of .xlsb and .ods, and does not open .xls or password-protected files.
order: 10
verified: 0.5.0
---

GridCraft 0.5.0 opens `.xlsx` files fully and saves to them. It also opens `.csv`, `.tsv` and `.txt`. It reads only the saved values of `.xlsb` and `.ods` files. It does not open old `.xls` files or password-protected workbooks.

| Format                          | Opens in 0.5.0                                            |
|---------------------------------|-----------------------------------------------------------|
| `.xlsx` Excel workbook          | Yes. This is the format GridCraft saves to.               |
| `.xlsm` macro-enabled workbook  | Yes, but the macros are dropped with a warning.           |
| `.xlsb` Excel binary workbook   | Values only: no formulas, formatting or charts.           |
| `.ods` OpenDocument spreadsheet | Values only: no formulas, formatting or charts.           |
| `.csv`, `.tsv`, `.txt`          | Yes.                                                      |
| `.xls` Excel 97-2003            | No. GridCraft recognises it and refuses with a message.   |
| Password-protected workbooks    | No. GridCraft recognises them and refuses with a message. |

Support for `.xls` files was added to the development version after 0.5.0 ([issue #121](https://github.com/storytold/gridcraft/issues/121)), so it is not in a release yet. For now, open the file in another app and save it as `.xlsx` first.

## Very large workbooks

A workbook of around 100 MB may fail to open. GridCraft limits one part of an `.xlsx` file to 512 MiB once unpacked, and a 74 MB file can unpack to more than that ([issue #175](https://github.com/storytold/gridcraft/issues/175)). The issue is still open.

## Related

[Open a workbook](../guides/open-a-workbook.md)
