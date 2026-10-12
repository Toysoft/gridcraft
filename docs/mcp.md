# GridCraft MCP server

`gridcraft-cli mcp` exposes GridCraft to AI agents over the [Model Context Protocol](https://modelcontextprotocol.io):
newline-delimited JSON-RPC 2.0 on stdin/stdout (protocol `2025-06-18`; `2025-03-26` and `2024-11-05` clients are
answered in their own version). Logs go to stderr.

Two backends:

| Mode | Command | What it drives |
|---|---|---|
| Headless (default) | `gridcraft-cli mcp [--in book.xlsx \| --sample budget] [--automation-read-root DIR] [--automation-write-root DIR]` | An in-process engine session — no window. |
| Remote | `gridcraft-cli mcp --connect 7979` | A running app started with `gridcraft --control 7979` (TCP JSON lines on `127.0.0.1`). |

## Claude Code

```sh
claude mcp add gridcraft -- gridcraft-cli mcp
# or drive the desktop app you are looking at:
claude mcp add gridcraft-app -- gridcraft-cli mcp --connect 7979
```

JSON config (`.mcp.json`, Claude Desktop, other clients):

```json
{
  "mcpServers": {
    "gridcraft": {
      "command": "gridcraft-cli",
      "args": ["mcp"]
    }
  }
}
```

Use `"args": ["mcp", "--connect", "7979"]` for the desktop app, or `["mcp", "--in", "/path/book.xlsx"]` to start
with a file open.

## Confining file access

```sh
gridcraft-cli mcp --automation-read-root /work/project --automation-write-root /work/project/out
```

```json
{"mcpServers": {"gridcraft": {"command": "gridcraft-cli",
  "args": ["mcp", "--automation-read-root", "/work/project", "--automation-write-root", "/work/project/out"]}}}
```

Without these flags the agent has the full file access of the user running the server. The two flags (VectorCraft
and the other Craft apps take the same ones, so one client config works across the suite) keep it to the folders
you give it:

- Every file a command reads must lie inside the **read root**: `open_workbook` (`file.open`), CSV and text import
  (`data.fromTextCsv` with a `path`), `insert.picture` with a `path`.
- Every file a command writes must lie inside the **write root**: `save_workbook`, `file.save`, `file.saveAs`,
  `file.exportCsv`, `file.exportPdf` (and a PDF through `file.save`/`file.saveAs`).
- Both are enforced in the engine's two file functions (`io::read_file` and `io::write_file`), which every
  command that reads or writes a named file goes through, so the same holds for any command added later.
- Read and write access are separate. Give the same folder twice for both; a root left out grants none of its
  access (with only `--automation-read-root`, nothing can be written to disk, though `file.saveBytes` still
  returns the bytes). Without either flag nothing changes.
- A path is made absolute against the server's working directory, then its links are followed: a file (or folder)
  link that leads out of a root is outside it. For a file that doesn't exist yet, its nearest existing folder's
  links are followed and the rest must be plain names, so `..` below a folder that doesn't exist is refused, as is
  a link that leads nowhere (writing through it would create its target). The result is compared with the root by
  whole folder names: `/work2/x` is not inside `/work`. On Windows case is ignored, UNC paths
  (`\\server\share\...`) are resolved like drive-letter ones, and device names (`CON`, `NUL`, `COM1`...) are refused.
- A refused path is an ordinary tool error the agent can read and correct, for example
  `automation path rejected: outside the write root /work/project/out: /etc/x.xlsx` or
  `automation filesystem access is not granted: write authority is absent: /work/project/out/x.xlsx`.
- `--in FILE` is your own choice and is opened before the roots come into force. The workbook it opened still
  has that path, so `save_workbook` without a path is refused unless the file lies inside the write root.
- The roots must be existing folders; a missing one stops the server with an error. Each flag is given once
  (`--flag <dir>` or `--flag=<dir>`).

The flags confine the in-process server (the default), so `--connect` with them is an error: the server would
otherwise talk to an app that isn't confined. The desktop app (`gridcraft --control`) does not take them yet.

The checks hold for every thread of the server process. A path is checked, then opened: another program that
swaps a link in between can still win that race, so the roots keep an agent to the files it was given rather than
guarding against other software on the computer.

## Tools

| Tool | Arguments | Does |
|---|---|---|
| `execute_command` | `command`, `params?` | Runs **any** engine command (`cell.set`, `home.bold`, `data.removeDuplicates`, …). |
| `list_commands` | `search?` | Command ids, labels, ribbon paths, shortcuts and parameter docs. |
| `inspect_workbook` | — | Sheets, used ranges, tables, charts, names, selection, history. |
| `read_range` | `range?`, `sheet?`, `formulas?`, `formatted?` | Rows of values (default: the used range). |
| `write_range` | `range`, `values` (2-D) | `range.setValues`; strings starting with `=` are formulas. |
| `set_cell` / `get_cell` | `cell`, `input` / `cell` | Enter one cell / read value, formula, display text, style. |
| `evaluate_formula` | `formula`, `cell?` | Evaluates without changing the sheet. |
| `select` | `range`, `active?` | Sets the selection. |
| `format_range` | `range`, `bold?`, `italic?`, `underline?`, `fontName?`, `fontSize?`, `fontColor?`, `fillColor?`, `numberFormat?`, `horizontalAlign?`, `wrapText?`, `borders?` | Composes the `home.*` formatting commands. |
| `insert_chart` | `range`, `type`, `title?`, `subtype?`, `at?` | `insert.chart`. |
| `create_table` | `range`, `style?`, `header?`, `name?` | `insert.table`. |
| `sort_range` | `keys`, `range?`, `header?` | `data.sort`; keys are `"B"`, `"B desc"` or `{column, order}`. |
| `filter` | `column`, `range?`, `values?`, `custom?`, `top?`, `blanks?`, `clear?` | `data.filterBy` (turns AutoFilter on when needed). |
| `open_workbook` / `save_workbook` / `new_workbook` | `path` / `path?` / `sample?` | Files (`.xlsx`, `.csv`, `.tsv`, `.json`, `.html`). |
| `undo` / `redo` | `steps?` | History. |
| `screenshot` | `path` | PNG of the window (Remote only). |
| `list_functions` | `search?`, `category?` | Worksheet functions with signatures. |

Results are one text block holding pretty JSON; failures set `isError: true` with a readable message. Unknown tools,
missing required arguments and wrong argument types are JSON-RPC `-32602` errors; malformed JSON is `-32700`; unknown
methods `-32601`.

## Resources

- `gridcraft://workbook` — `document.inspect` of the active workbook.
- `gridcraft://commands` — every command with its parameter docs.
- `gridcraft://functions` — every worksheet function.

`prompts/list` returns an empty list.

## Example session

```text
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}
→ {"jsonrpc":"2.0","method":"notifications/initialized"}
→ {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"write_range","arguments":{"range":"A1","values":[["Item","Cost"],["Rent",1200],["Food",400],["Total","=SUM(B2:B3)"]]}}}
→ {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"read_range","arguments":{"range":"B4"}}}
← {"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"{\n  \"range\": \"B4\", ... \"values\": [[1600.0]]\n}"}],"isError":false}}
```

## Embedding

`gridcraft-mcp` is a library: `Server::new(Box::new(Headless::new()))` or
`Server::new(Box::new(Remote::connect(&control_addr("7979"))?))`, then `Server::serve(stdin, stdout)` or
`Server::handle_line(line) -> Option<String>`. Implement the `Backend` trait (`call(method, params)`) to drive another
host.
