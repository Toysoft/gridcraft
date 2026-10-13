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
| Function names translated | — | 424 | 403 | 412 | 459 |
| Booleans | `TRUE`, `FALSE` | `VERDADERO`, `FALSO` | `WAHR`, `FALSCH` | `VERDADEIRO`, `FALSO` | `ИСТИНА`, `ЛОЖЬ` |
| Errors (e.g.) | `#VALUE!`, `#N/A` | `#¡VALOR!`, `#N/D` | `#WERT!`, `#NV` | `#VALOR!`, `#N/D` | `#ЗНАЧ!`, `#Н/Д` |
| Arguments and unions | `,` | `;` | `;` | `;` | `;` |
| Decimal separator | `.` | `,` | `,` | `,` | `,` |
| Array columns / rows | `,` / `;` | `\` / `;` | `.` / `;` | `\` / `;` | `;` / `:` |
| Table specifiers (e.g.) | `#Headers`, `#This Row` | `#Encabezados`, `#Esta fila` | `#Kopfzeilen`, `#Diese Zeile` | `#Cabeçalhos`, `#Esta Linha` | `#Заголовки`, `#Эта строка` |

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
(searched and sorted by the local name), and argument hints use the selected formula language.
Common Spanish function help is translated; other help stays English, shown under the local
function name with the language's separators. General interface labels and errors fall back to
English where translations are not provided. An edit captures its language when it starts so
switching the preference cannot reinterpret unfinished input.

Workbook/sheet names, table names, LET bindings and LAMBDA parameters take precedence over
local names. Text, sheet names and table column names are unchanged. When a localized spelling
would collide with a name, displayed formulas retain the canonical spelling to preserve meaning.

Stored formulas, XLSX import/export, engine commands, control/API calls, CLI evaluation and
clipboard exchange retain strict English syntax. This also applies to advanced dialogs such as
Name Manager and conditional-format rules. Localization happens at the cell/formula-bar editor
boundary, including filling the selected range with Ctrl+Enter; it does not change the workbook
format or teach the calculation engine ambiguous aliases.

## Where the names come from

Each table lives in `crates/formula/src/input/<language>.rs` with its sources. Function names
come from Microsoft Support's localized Excel function pages, which are machine-translated and
sometimes wrong, so an entry needs two agreeing signals on its page (title, description, syntax
paragraph aligned with the English page, examples); entries marked `reviewed` were decided by
hand against independent function-name references. Booleans, errors and table specifiers cite
their sources per item. The tables have not yet been checked against a running localized Excel
or by native speakers; corrections are one-line edits with a source.
