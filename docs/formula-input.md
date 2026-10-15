# Formula language

View → Interface language is the existing, persisted language preference, and it also picks the
formula language, as in Excel: function names follow the display language. On first run the
preference follows the system language.

| Interface language | Formula language |
| --- | --- |
| Español | Spanish |
| Deutsch | German |
| Português (Brasil) | Brazilian Portuguese |
| Русский | Russian |
| English, 日本語, 中文, 한국어 | English (Japanese, Chinese and Korean Excel use English function names) |
| Čeština, Français | English for now: these languages have no function table yet |

## What changes per language

| | English | Spanish | German | Portuguese (Brazil) | Russian |
| --- | --- | --- | --- | --- | --- |
| Example | `SUM`, `IF`, `VLOOKUP` | `SUMA`, `SI`, `BUSCARV` | `SUMME`, `WENN`, `SVERWEIS` | `SOMA`, `SE`, `PROCV` | `СУММ`, `ЕСЛИ`, `ВПР` |
| Function names translated | — | 424 | 419 | 412 | 459 |
| Booleans | `TRUE`, `FALSE` | `VERDADERO`, `FALSO` | `WAHR`, `FALSCH` | `VERDADEIRO`, `FALSO` | `ИСТИНА`, `ЛОЖЬ` |
| Errors (e.g.) | `#VALUE!`, `#N/A` | `#¡VALOR!`, `#N/D` | `#WERT!`, `#NV`, `#ÜBERLAUF!` | `#VALOR!`, `#N/D` | `#ЗНАЧ!`, `#Н/Д` |
| Arguments and unions | `,` | `;` | `;` | `;` | `;` |
| Decimal separator | `.` | `,` | `,` | `,` | `,` |
| Array columns / rows | `,` / `;` | `\` / `;` | `.` / `;` | `\` / `;` | `;` / `:` |
| Table specifiers (e.g.) | `#Headers`, `#This Row` | `#Encabezados`, `#Esta fila` | `#Kopfzeilen`, `#Diese Zeile` (shown as `@`) | `#Cabeçalhos`, `#Esta Linha` | `#Заголовки`, `#Эта строка` |

For example, `=SI(VERDADERO;SUMA(1,5;2,25);0)` (Spanish), `=WENN(WAHR;SUMME({1.2;3,5.4});#NV)`
(German) and `=ЕСЛИ(ИСТИНА;СУММ({1;2:3;4});#Н/Д)` (Russian) are stored as
`IF(TRUE,SUM(1.5,2.25),0)`, `IF(TRUE,SUM({1,2;3.5,4}),#N/A)` and `IF(TRUE,SUM({1,2;3,4}),#N/A)`.

Names, booleans and errors are case-insensitive in any script (`zählenwenn`, `сумм`). Functions
without a local name keep their English name, and canonical English names, booleans and errors
are also accepted with the language's separators. English mode retains comma arguments, decimal
points and comma array columns; it does not silently translate local names. A function passed by
name (`BYROW(A1:C3;SOMA)`, `GROUPBY(…;SUMME)`) uses the same local name as its call.

## Where it applies

The cell editor, formula bar, Show Formulas, autocomplete, the Formula ribbon, Insert Function
(searched and sorted by the local name), argument hints, and the formula fields of dialogs (Name
Manager's Refers to, data validation sources, bounds and custom formulas, conditional-format
values and formulas, calculated fields) use the selected formula language. Cells show booleans
and error values in its words (`WAHR`, `#NV`), and typing those words enters the value; the
English words typed into a cell stay text there, as in German Excel.
Common Spanish function help is translated; other help stays English, shown under the local
function name with the language's separators. General interface labels and errors fall back to
English where translations are not provided. An edit captures its language when it starts so
switching the preference cannot reinterpret unfinished input.

Workbook/sheet names, table names, LET bindings and LAMBDA parameters take precedence over
local names. Text, sheet names and table column names are unchanged. When a localized spelling
would collide with a name, displayed formulas retain the canonical spelling to preserve meaning.

Stored formulas, XLSX import/export, engine commands, control/API calls, CLI evaluation and
clipboard exchange retain strict English syntax. Localization happens where people type and read:
the editor (including filling the selected range with Ctrl+Enter), dialog fields (which the
control channel's `ui.dialog.set` fills as a person would), and what cells, print and PDF show;
it does not change the workbook format or teach the calculation engine ambiguous aliases.

## Numbers and dates

Numbers follow the interface language's regional format, which German Excel takes from the
system's: with German, cells show `1.234,56`, `13.10.2026`, `13. Okt 26`, `Dienstag, 13. Oktober
2026`, and typing reads `1,5`, `1.234,5`, `-3,2%`, `1,5E+3`, `5 €`, `13.10.2026`, `13.10.26`,
`13.10` (this year), `13. Okt 2026`, `14:30` and `13.10.2026 14:30`, each with the number format
German Excel gives it. File › Options › Advanced › *Use system separators* (German *Trennzeichen
vom Betriebssystem übernehmen*) can be turned off to set the decimal and thousands separators by
hand; it is a preference (`ui.json`), never part of a workbook. Other languages keep en-US
numbers until their regional format is observed (Spanish, Portuguese and Russian still read a
typed decimal comma).

Format codes stay canonical (`#,##0.00`, `m/d/yyyy`): the region decides how they draw. Excel's
built-in short date (id 14, stored as `m/d/yyyy`) and ids 15–17 and 20–22 draw in the region's
own way, `/` in a date code is the region's date separator, month and day names follow the
region unless a `[$-409]`-style tag picks another, and the Long Date and Time formats follow the
region's system formats.

Print and PDF show what the grid shows. Save As CSV, TSV and Web Page write values as displayed;
a new CSV is separated by `;` where the decimal separator is `,` (Excel uses the regional list
separator), and a CSV opened there prefers `;` and reads German numbers, dates, booleans and
errors, unless its fields are separated by commas. The CLI, MCP and engine commands always use
en-US.

Not localized yet: format codes typed into Format Cells (German Excel shows `#.##0,00`,
`TT.MM.JJJJ`), `TEXT()`'s and `VALUE()`'s locale-dependent strings (German Excel's
`=TEXT(A1;"TT.MM.JJJJ")`), numbers turned into text by formulas (`=1,5&""` is `1,5` in German
Excel, `1.5` here), chart axis labels, filter lists, R1C1 notation (German `Z1S1`), and the
ribbon's Currency format (German Excel uses `€`).

## Where the names come from

Each table lives in `crates/formula/src/input/<language>.rs` with its sources.

German was observed black-box in Excel 365 (German interface, de-DE regional format) through its
automation interface on 2026-10-13: every function GridCraft knows, plus four it doesn't yet, was
entered in canonical form and read back as German Excel shows it, and so were the separators,
booleans, errors, table specifiers and a data table's `TABLE` formula (`MEHRFACHOPERATION`).
`crates/formula/tests/german_observed.rs` checks all 519 calls in both directions. The observation corrected two words Microsoft's German help pages give:
`MAP` stays `MAP`, and German Excel shows `#CALC!` (not `#KALK!`).

The Spanish, Portuguese and Russian names come from Microsoft Support's localized Excel function
pages, which are machine-translated and sometimes wrong, so an entry needs two agreeing signals
on its page (title, description, syntax paragraph aligned with the English page, examples);
entries marked `reviewed` were decided by hand against independent function-name references.
Booleans, errors and table specifiers cite their sources per item. These tables have not yet been
checked against a running localized Excel or by native speakers; corrections are one-line edits
with a source.
