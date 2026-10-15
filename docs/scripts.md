# Scripts

> **Last updated:** 2026-10-13 · **Change:** major (created: Rhai scripts with an Office Scripts-shaped object model) · **Target:** Microsoft Excel (Microsoft 365)

GridCraft scripts automate a workbook with real logic: loops, conditions, functions. They are written
in [Rhai](https://rhai.rs) (proposed in gaps.md §21, pending the maintainers' decision): it is pure Rust,
builds for the web (wasm32), and can be sandboxed. The objects a script works with follow Excel's
[Office Scripts](https://learn.microsoft.com/office/dev/scripts/) API (`workbook`,
`getActiveWorksheet()`, `getRange("A1:B3")`, `setValues(…)`, `getFormat().getFont().setBold(true)`),
so what people know from Excel carries over; the syntax is Rhai's (close to Rust and JavaScript).

```rhai
fn main(workbook) {
    let sheet = workbook.getActiveWorksheet();
    let r = sheet.getRange("A1:B3");
    r.setValues([[1, 2], [3, 4], [5, 6]]);
    r.getFormat().getFont().setBold(true);
    let total = 0;
    for row in r.getValues() { total += row[0] * row[1]; }
    sheet.getRange("C1").setValue(total);
    run("home.fillColor", #{range: "A1:C1", color: "#FFFF00"});   // any GridCraft command
    console.log("total", total);
}
```

A script may also be plain statements without `main`; `workbook` is defined at the top level too.
The value `main` (or the last statement) returns is the run's `result`.

## Where scripts live and run

| | |
|---|---|
| **Automate › Scripting Tools** | Record Actions (records what you do as a script; click again to stop; the script opens in the Code Editor), New Script, All Scripts. German: Automatisieren › Skripttools › Aktionen aufzeichnen, Neues Skript, Alle Skripts. |
| **Scripts gallery** (Automate tab) | This workbook's scripts; a click runs one. |
| **Code Editor** (task pane) | Name (rename in place), Run (saves first), Save, Delete, the code with syntax colours, and Output: what the script printed and, if it failed, the error with its line and column (click it to jump there). |
| **View › Macros** (Alt+F8) | Lists scripts (and a preserved VBA project); Run, Edit, Create, Delete. |
| **Commands** | `automate.runScript {name}` or `{code}`, `automate.newScript`, `automate.saveScript {name, code}`, `automate.getScript`, `automate.renameScript`, `automate.deleteScript`, `automate.allScripts`, `automate.recordActions`, `automate.enableScripts`, `automate.lastRun`. The control channel reaches them like any command. |
| **CLI** | `gridcraft-cli run --in book.xlsx --script totals.rhai [--allow-files]` ([cli.md](cli.md)). |
| **MCP** | `run_script {code}` or `{name}` ([mcp.md](mcp.md)). |

Scripts are saved with the workbook. In an XLSX file they are one custom XML part
(`customXml/itemN.xml`, namespace `urn:gridcraft:scripts:1`) with each script's name and code as
plain text; spreadsheet applications keep custom XML parts they don't use. Scripts recorded by
earlier GridCraft versions (lists of commands in a hidden defined name) are read into the same
list, still run, and become Rhai when they are edited and saved.

## Object model

Indexes are 0-based, addresses A1-style. An empty cell reads as `""`; whole numbers read as
integers, other numbers as floats; an error value reads as its text (`"#DIV/0!"`). Getters of a
multi-cell range report its top-left cell. Writes go through GridCraft's commands, so sheet
protection, recalculation and the journal treat them like the user's own edits.

| Object | Methods |
|---|---|
| `Workbook` | `getActiveWorksheet()`, `getWorksheet(name)` (`()` if none), `getWorksheets()`, `getFirstWorksheet()`, `getLastWorksheet()`, `addWorksheet(name?)` (at the end; the active sheet stays), `getSelectedRange()`, `getActiveCell()`, `getNamedItem(name)`, `getNames()`, `addNamedItem(name, "=Sheet1!$A$1")`, `getName()` |
| `Worksheet` | `getName()`, `setName(n)`, `getPosition()`, `activate()`, `delete()`, `getRange(address?)`, `getCell(row, col)`, `getRangeByIndexes(row, col, rows, cols)`, `getUsedRange()` (`()` if empty), `getVisibility()`, `setVisibility("Visible"\|"Hidden")`, `getTabColor()`, `setTabColor("#RRGGBB")` |
| `Range` | `getAddress()`, `getWorksheet()`, `getRowCount()`, `getColumnCount()`, `getCellCount()`, `getRowIndex()`, `getColumnIndex()`, `getValues()`/`setValues(rows)`, `getValue()`/`setValue(v)` (every cell), `getFormulas()`/`setFormulas(rows)`, `getFormula()`/`setFormula(f)`, `getText()`, `getTexts()`, `getNumberFormat()`, `getNumberFormats()`, `setNumberFormat(code)`, `clear("All"\|"Contents"\|"Formats")`, `select()`, `merge(across?)`, `unmerge()`, `getFormat()`, `getOffsetRange(dr, dc)`, `getResizedRange(dr, dc)`, `getCell(r, c)`, `getRow(i)`, `getColumn(i)`, `getEntireRow()`, `getEntireColumn()`, `getLastCell()`, `getLastRow()`, `getLastColumn()`, `getSurroundingRegion()`, `getIntersection(r)`, `getBoundingRect(r)` |
| `RangeFormat` | `getFont()`, `getFill()`, `get/setHorizontalAlignment("Left"\|"Center"\|"Right"\|"General"…)`, `get/setVerticalAlignment`, `get/setWrapText`, `get/setColumnWidth(points)`, `get/setRowHeight(points)`, `autofitColumns()`, `autofitRows()` |
| `RangeFont` | `get/setBold`, `get/setItalic`, `get/setStrikethrough`, `get/setUnderline("None"\|"Single"\|"Double"…)`, `get/setColor("#RRGGBB")`, `get/setName`, `get/setSize` |
| `RangeFill` | `getColor()` (`""` without a fill), `setColor("#RRGGBB")`, `clear()` |
| `NamedItem` | `getName()`, `getFormula()`, `getRange()`, `getScope()`, `delete()` |

`setValues` takes an array of rows that must have the range's shape. Text is entered as if typed:
`"=A1*2"` is a formula, `"5%"` a percentage, `"1/2/2024"` a date; `()` clears a cell.

`run(id)` / `run(id, #{params})` runs any command (`gridcraft-cli commands` lists them) and returns
its result as Rhai values. `print(x)`, `debug(x)` and `console.log(a, b, …)` (also `info`, `warn`,
`error`) write to the output.

## Undo, limits and sandbox

- One run is one undo step ("Run Script"), also when it stops with an error: what it did until
  then stays and undoes together. `run("edit.undo")` inside a script undoes the script's own last
  step.
- Limits stop a run with an error instead of freezing or crashing the app: 50 million operations
  and 30 seconds, 128 nested function calls, expression nesting 64, strings of 16 MB, arrays of a
  million elements and maps of 100,000 (nested ones counted), 1,000,000 cells per range read or
  write. In the browser the limits are smaller (no clock: 20 million operations; 32 calls; arrays
  of 10,000 elements). Natively a run uses a helper thread with a large stack.
- The language has no file, network, clock or module access (`import` and `eval` are off).
  Commands that open, read or write files (`file.open`, `file.save`, `file.saveAs`, exports, any
  `path` or `url` parameter) are refused, unless the CLI runs with `--allow-files`; `automate.*`
  commands are always refused inside scripts.
- A workbook opened from a file that holds scripts doesn't run them until you choose **Enable
  Content** on the message bar (`automate.enableScripts`), once per opened workbook. Code passed
  to `automate.runScript {code}` (the CLI's `.rhai` files, MCP `run_script {code}`) isn't the
  file's, so it runs.

## Differences from Office Scripts

- The syntax is Rhai: `let` without types, `fn` instead of `function`, `#{key: value}` maps,
  `` `text ${x}` `` interpolation, no `await`/`async` (calls are synchronous), no TypeScript types.
- The object model covers ranges, formats, worksheets and names. Tables, charts, PivotTables,
  shapes, comments, conditional formats and events aren't objects yet; their commands work through
  `run(…)`.
- Scripts are stored in the workbook, not in OneDrive; there are no script buttons on the sheet and
  no Power Automate flows.
