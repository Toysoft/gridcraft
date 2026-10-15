# Formula language

View → Interface language is the existing, persisted language preference, and it also picks the
formula language, as in Excel: function names follow the display language. On first run the
preference follows the system language. The separators follow the Regional format (File ›
Options), as Excel takes them from the Windows regional settings; see
[Separators](#separators).

| Interface language | Formula language |
| --- | --- |
| Español | Spanish |
| Deutsch | German |
| Português (Brasil) | Brazilian Portuguese |
| Русский | Russian |
| Français | French |
| English, 日本語, 中文, 한국어 | English (Japanese, Chinese and Korean Excel use English function names) |
| Čeština | English for now: Czech has no function table yet |

## What changes per language

| | English | Spanish | German | Portuguese (Brazil) | Russian | French |
| --- | --- | --- | --- | --- | --- | --- |
| Example | `SUM`, `IF`, `VLOOKUP` | `SUMA`, `SI`, `BUSCARV` | `SUMME`, `WENN`, `SVERWEIS` | `SOMA`, `SE`, `PROCV` | `СУММ`, `ЕСЛИ`, `ВПР` | `SOMME`, `SI`, `RECHERCHEV` |
| Function names translated | — | 424 | 403 | 412 | 459 | 405 |
| Booleans | `TRUE`, `FALSE` | `VERDADERO`, `FALSO` | `WAHR`, `FALSCH` | `VERDADEIRO`, `FALSO` | `ИСТИНА`, `ЛОЖЬ` | `VRAI`, `FAUX` |
| Errors (e.g.) | `#VALUE!`, `#N/A` | `#¡VALOR!`, `#N/D` | `#WERT!`, `#NV` | `#VALOR!`, `#N/D` | `#ЗНАЧ!`, `#Н/Д` | `#VALEUR!`, `#N/A` |
| Array columns / rows (preferred) | `,` / `;` | `\` / `;` | `.` / `;` | `\` / `;` | `;` / `:` | `.` / `;` |
| Table specifiers (e.g.) | `#Headers`, `#This Row` | `#Encabezados`, `#Esta fila` | `#Kopfzeilen`, `#Diese Zeile` | `#Cabeçalhos`, `#Esta Linha` | `#Заголовки`, `#Эта строка` | `#En-têtes`, `#Cette ligne` |

For example, with each language's own region, `=SI(VERDADERO;SUMA(1,5;2,25);0)` (Spanish),
`=WENN(WAHR;SUMME({1.2;3,5.4});#NV)` (German) and `=ЕСЛИ(ИСТИНА;СУММ({1;2:3;4});#Н/Д)`
(Russian) are stored as `IF(TRUE,SUM(1.5,2.25),0)`, `IF(TRUE,SUM({1,2;3.5,4}),#N/A)` and
`IF(TRUE,SUM({1,2;3,4}),#N/A)`.

Names, booleans and errors are case-insensitive in any script (`zählenwenn`, `сумм`). Functions
without a local name keep their English name, and canonical English names, booleans and errors
are also accepted with the region's separators. English mode does not silently translate local
names. A function passed by name (`BYROW(A1:C3;SOMA)`, `GROUPBY(…;SUMME)`) uses the same local
name as its call.

## Separators

The argument separator and the decimal separator are the region's, whatever the interface
language: `;` and `,` in es-ES, de-DE, fr-FR, it-IT, pt-BR, pt-PT, ru-RU and cs-CZ, `,` and `.`
in en-US, en-GB, en-IN, ja-JP, ko-KR, zh-CN and zh-TW. English Excel with a Croatian region
takes `=SUM(1;2)` and `=1,5*1,5` (#447). Array constants use the language's preferred separators
above, except that a column separator that is the region's decimal becomes `\`. That reproduces
every combination measured in Excel:

| Interface | Region | `INDEX({1,2;3,4.1},2,2)` is typed as |
| --- | --- | --- |
| French | France | `=INDEX({1.2;3.4,1};2;2)` |
| French | United States | `=INDEX({1\2;3\4.1},2,2)` |
| English | France | `=INDEX({1\2;3\4,1};2;2)` |
| English | United States | `=INDEX({1,2;3,4.1},2,2)` |

English Excel uses `\` between array columns with every decimal-comma region (Stack Overflow
57120417, which also measured the French rows). French Excel 2024 with the Windows regional
format "English (Europe)" (`.` decimal, system separators on) takes `=CENTILE.EXCLURE(A1:A6,0.5)`
and `=RECHERCHEX(3,A1:A6,A1:A6)` (#443). Excel's `TEXT` format codes follow the region the same
way: that Excel takes `"dd/mm/yyyy"` and refuses `"jj/mm/aaaa"`, which French Excel 2013 with a
French region required (#191, #443). GridCraft's format codes are English in every region. Czech,
Japanese, Chinese and Korean formulas use English names with their region's separators
(`=SUM(1,5;{1\2;3\4})` in cs-CZ).

## Where it applies

The cell editor, formula bar, Show Formulas, autocomplete, the Formula ribbon, Insert Function
(searched and sorted by the local name), and argument hints use the selected formula language.
Common Spanish function help is translated; other help stays English, shown under the local
function name with the region's separators. General interface labels and errors fall back to
English where translations are not provided. An edit captures its language and separators when
it starts, so switching either preference cannot reinterpret unfinished input; formulas already
in cells are shown in the new syntax.

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
their sources per item. A French user checked the French separators, `SOMME`, `TEXTE`,
`AUJOURDHUI`, `LIGNES`, `#VALEUR!` and four table specifiers in French Excel 2013, and 16 more
function names in French Excel 2024, three of which were wrong and are fixed (#191); the other
tables have not yet been checked against a running localized Excel or by native speakers;
corrections are one-line edits with a source.
