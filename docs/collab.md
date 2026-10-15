# Live sessions (co-editing)

> **Last reviewed:** 2026-10-15 · **Last updated:** 2026-10-15 · **Change:** minor (Share and the session's people in the title bar; previously major: created: plugin protocol 2 in GridCraft, schema 1) · **Target:** Microsoft Excel (Microsoft 365)

Several people edit one workbook at the same time, each in their own GridCraft, and see each
other's selections and edits as they happen. This is GridCraft's equivalent of Excel's
co-authoring. It's **native**: GridCraft merges the edits itself (a CRDT, Yjs through the
[`yrs`](https://crates.io/crates/yrs) crate), shows who's there and keeps a per-person undo.
Moving the bytes between people is a **transport plugin**'s job (MQTT, a local relay, a
y-websocket server, Nextcloud…), which never needs to understand a workbook. With a room key the
content is encrypted end to end, so the transport only ever sees ciphertext.

The protocol is shared with WordCraft and DeckCraft:
[craft plugin protocol 2](https://github.com/sirmmo/craft-collab/blob/main/docs/protocol-2.md)
(push channel, rooms, `collab/*` messages, encryption, limits). This page is GridCraft's part:
the shared document (**schema 1**, normative) and how the app behaves. A summary of the schema
for other implementations is craft-collab's
[`docs/schemas/gridcraft-1.md`](https://github.com/sirmmo/craft-collab/blob/main/docs/schemas/gridcraft-1.md);
the two agree, and this page has the detail that one leaves to GridCraft.

## Using it

**Share**, at the right end of the title bar (as in Excel). Not in a session, it opens the
Share dialog: pick the transport add-in, then its own *Start a Live Session* or *Join from an
Invitation…* (the add-in makes and reads invitation links), or name a room and key under *Room
name and key*. With no transport add-in installed, the dialog says a live-session add-in is
needed, names craft-mqtt from [sirmmo/craft-collab](https://github.com/sirmmo/craft-collab) as an
example, and opens Automate › Add-ins. In a session, the button shows the session's state with a
dot (green: live; amber, ringed: connecting or joining; grey: offline, editing goes on here) and
opens the session's popover: the room's title, state, room id and transport, whether it's
encrypted or view-only, the transport's message, everyone in it (circle, name, "(you)"), *Copy
invitation* when the transport's Share or Join returned a link (`result.link`), and *Leave*
(the workbook stays open, with every edit, as an ordinary workbook; not saved automatically).

To the left of Share, the people in the session are overlapping circles with their initials on
their colour (black or white text, whichever reads better), the others by name and then you,
at most four circles and then "+N" (hovering it lists the rest). Hover a circle for the name
("(you)" for yourself); click it to go to that person's selection, switching sheet if needed,
which flashes in their colour. In a narrow window the theme switch, Comments and Search give
way first, then circles; the document title is cut short with "…" before anything overlaps it.
In full screen on macOS the title bar's controls move into the traffic lights' space.

Others' selections are outlined in their colours with a name tag on their active cell, and cells
they change are highlighted for two seconds. The status bar only says why the workbook can't be
edited now ("Joining…", view only, another app's room) and the transport's or app's problems
("Wrong room key?"). The `collab.*` commands are in the command palette (they no longer have a
Review tab group).

Undo and Redo, while in a session, undo **your** changes only; others' edits never enter your
undo history. Saving works as usual: each person saves their own copy.

## Commands

| Command | Params | Does |
|---|---|---|
| `collab.share` | `{plugin, room, user?, role?, key?, title?}` | Starts a session from the active workbook (no params: the Share dialog) |
| `collab.join` | `{plugin, room, mode: share\|join, user?: {id, name, color}, role?: edit\|view, key?, path?, title?}` | Joins or starts a session (no params: the dialog). `share` seeds the room from the active workbook (or `path`); `join` opens a new workbook (or `path`, whose unshared parts are kept) filled from the room |
| `collab.leave` | `{room?}` | Leaves (default: the active workbook's room) |
| `collab.status` | `{}` | `{rooms: [{room, plugin, state, peers, listed, role, encrypted, clientId, user, title, document, message, canUndo, canRedo}], transports, user}` |
| `collab.setUser` | `{name?, color?}` | Your name (the same as File › Options › User name) and colour (`#RRGGBB`) |

A plugin command's result may carry `collab: {join: {…}}` (with protocol 1.2's `open`,
`dialog` and `message`): `plugins.run` returns it to the caller, and the interface runs
`collab.join` with it (the plugin's id as `plugin` unless given) when a button, menu item or
plugin dialog ran the command, keeping `result.link` as the room's invitation. Programmatic
calls never open dialogs.

Rust API (for embedders, tests and a future web build): `Session::collab_poll(now_ms)` runs the
sessions between frames; `Session::collab_serve_in_memory(id)` / `collab_join_in_memory`,
`collab_take_outgoing` and `collab_poll_pushed` connect a session to a transport the caller
implements; `collab_take_notices` gives the status-bar messages.

## Schema 1 (normative)

`meta.app` is `"gridcraft"`, `meta.schema` is `1` (written as an integer). All binary encodings
are Yjs v1 updates.

### Roots

| Root | Yjs type | Holds |
|---|---|---|
| `meta` | Map | `app`, `schema`, `title` (string; the shared workbook's title, `""` for an unsaved one) |
| `sheets` | Array | sheet ids, in tab order |
| `sheet:<id>` | Map | one sheet (below) |
| `names` | Map | defined names: key = the name in lower case, plus `@<sheet index>` for a sheet-local name; value = JSON text `{name, scope, formula, comment, hidden}` (`formula` without `=`, `scope` a sheet index or `null`) |
| `styles` | Map | cell styles: key = `s` + the first 16 hex digits of the SHA-256 of the style's JSON; value = that JSON text (GridCraft's `Style`: `font`, `fill`, `borders`, `align`, `num_fmt`, `protection`) |
| `book` | Map | workbook settings, each JSON text, last writer wins: `normal` (the Normal style, which unstyled cells use), `cellStyles`, `customLists`, `theme`, `dateSystem`, `calc`, `props`, `protectedStructure`, `structurePassword` (a hash, never a password) |

Styles are content-addressed: two people creating the same style write the same entry. A cell
whose style hasn't arrived yet shows unstyled until it does.

### A sheet: `sheet:<id>`

| Key | Type | Holds |
|---|---|---|
| `name` | string | the tab's name |
| `rows`, `cols` | Array | row and column ids, in order (entry `i` is row `i + 1`, column `i + 1`) |
| `rowBlocks`, `colBlocks` | number | the next unused block number (below) |
| `cells` | Map | `"<rowId>:<colId>"` → Map `{input: string, style?: string, array?: string}` |
| `rowProps`, `colProps` | Map | row or column id → JSON text `{size?, hidden?, outline?, collapsed?, custom?, style?}` (`size` in points, `style` a key of `styles`) |
| `merges` | Map | top-left `"<rowId>:<colId>"` → bottom-right `"<rowId>:<colId>"` |
| `tables`, `charts`, `cf`, `dv` | string | JSON text of the sheet's tables, charts, conditional formats and data validations (GridCraft's model), last writer wins |
| `props` | Map | other sheet settings, each JSON text, last writer wins: `defaultColWidth`, `defaultRowHeight`, `freeze`, `tabColor`, `visibility`, `showGridlines`, `showHeadings`, `showZeros`, `rightToLeft`, `autofilter`, `images`, `shapes`, `sparklines`, `slicers`, `pivots`, `protection`, `editRanges`, `print`, `comments`, `hyperlinks`, `cellPictures` (the last three as `[[A1, value], …]`) |

Unknown keys are kept and ignored. Positions inside the JSON features (`tables`, `charts`,
`comments`…) are A1 addresses, not ids: the peer that inserts rows rewrites them with its
update, and a concurrent change of the same feature keeps the later write.

### Ids

- **Sheet ids and the ids of inserted rows and columns**: 8 random characters of `0-9a-z`.
- **Growing past the end**: rows and columns beyond the arrays exist implicitly and are empty.
  When an edit (or the seed) needs one, the writer appends whole **blocks of 64** deterministic
  ids, block `b` being `~b.0` … `~b.63`, numbered from the larger of `rowBlocks`/`colBlocks` and
  one more than the highest block present, then sets the counter to the next unused number. Two
  people growing a sheet at once append the same ids, which collapse when read, so their cells
  land in the same rows. A seed grows each sheet to its used range (cells, row and column
  properties, merges): a 4-row sheet has 64 row ids, an empty one none.
- **Duplicates** (concurrent moves, concurrent growth): readers keep the first occurrence of an
  id in `sheets`, `rows` and `cols`; positions count after that. A sheet id without a
  `sheet:<id>` map (or with an empty one) is skipped.

### Cells

`input` is what the person typed, decoded exactly (no locale, no number format):

| Input | Value |
|---|---|
| empty | no value (a cell with only a style) |
| `=…` | a formula (its canonical text after `=`) |
| `'…` | text, when the text alone would read as something else (`'007`, `'=A1`, `'TRUE`) |
| `TRUE`, `FALSE` | logical |
| `#N/A`, `#DIV/0!`… (Excel's spelling) | an error value |
| decimal notation Rust's `f64` parser reads, finite (`3.5`, `-2`, `1e-7`) | a number, written in shortest round-trip form |
| anything else | text |

`style` is a key of `styles`; without it the cell uses the Normal style. `array` marks a legacy
array formula (Ctrl+Shift+Enter): the bottom-right cell of its range. Setting a cell that has a
Map updates `input` (and `style`) inside it, so a concurrent style change survives; clearing
it deletes the key. Keys whose row or column id isn't in `rows`/`cols` are ignored.

### Edits

Each committed command (the engine's undo step, outermost: a plugin command's steps are one) is
**one** transaction with GridCraft's local origin and one `collab/update`. GridCraft computes it
by comparing the workbook before and after the command:

| Edit | Transaction |
|---|---|
| Cells | set or remove the changed keys (only input, style and array count: recalculated values never travel) |
| Insert rows/columns before `p` | new random ids into the array before the `p`-th id (nothing when `p` is past the end); the cells move with their ids; the inputs Excel would rewrite (`=SUM(A1:A10)` → `=SUM(A1:A11)`) are rewritten in the same transaction |
| Delete rows/columns | delete the cells in them, remove every occurrence of their ids, rewrite shifted inputs |
| Move columns | an insert at the destination and a delete at the source |
| Add, rename, delete, move sheets | an id into `sheets` with its `sheet:<id>`; `name`; the id out of `sheets` (the map stays, so undo brings it back); out and in at the new place |
| Formats, row heights, merges, features | the changed entries |

A peer applying a remote change maps rows and columns by id and **doesn't** rewrite formulas
again. Commands that change only view state (selection, zoom, scroll, the active sheet) send
nothing; the selection goes to awareness.

### Awareness

Each peer's state is `{user: {id, name, color}, selection?, cursor?}`:
`selection` = `{sheet: <sheetId>, ranges: [[rowId, colId, rowId, colId], …]}` (top-left and
bottom-right cells), `cursor` = `{sheet, cell: [rowId, colId]}` (the active cell). Rows and
columns past the last id are clamped to it. GridCraft sends its state at most 20 times a second
and renews it every 15 s; a peer not heard from for 30 s is gone.

## Behaviour

- **Push channel.** Every running plugin's standard output is read all the time; what a plugin
  sends on its own (notifications, `collab/snapshot` and `host/log` requests) waits in a queue
  that the app's main loop takes each frame, never in the middle of a command. `host/…` requests
  during one of GridCraft's requests are still answered by it (protocol 1.1). Lines may be
  64 MiB; a plugin sending more than 1,000 messages a second for 5 s is stopped and marked failed.
- **Joining** (`mode: join`): `collab/joined`, then `collab/sync1`; the workbook is empty and
  read-only until a state for GridCraft schema 1 arrives. **Sharing**: the room is seeded from
  the workbook (origin `seed`, not undoable), then `collab/joined` and `collab/sync1`.
- **Sync.** A received `sync1` is answered with `sync2` addressed to the asker (`to` = its
  `from`); when its state vector shows changes GridCraft lacks, GridCraft asks back with its own
  `sync1`. `sync1` also goes out when the transport reports `online` after another state. An
  update that skips some of a peer's changes waits until they arrive (and GridCraft asks for
  them): yrs would otherwise apply it without reporting all its changes.
- **Remote changes** are applied between frames and recalculated (the changed cells, or
  everything after a structural change). A message that can't be decoded or applied is dropped
  and shown in the status bar; an update that writes another app's `meta` is dropped and says
  "This room is for <app> <schema>". Nothing a peer sends can crash GridCraft (panics in the
  Yjs library are caught too).
- **Encryption** (a `key`, 32 bytes): AES-256-GCM per protocol 2 section 4; `sync2` with
  `snapshot: true` and the answer to `collab/snapshot` use the method name `collab/snapshot`.
  After 10 messages that don't decrypt, the status bar asks "Wrong room key?".
- **`collab/snapshot`** is answered with the whole state (encrypted when keyed) and, with
  `file: true`, the workbook as `.xlsx` bytes, except in an encrypted room: there GridCraft
  answers `fileRefused` instead, so the plaintext never reaches the transport.
- **Offline.** When the transport reports `offline` or exits, editing goes on; an exited
  transport is started again after 3 s (a crash counts as a plugin failure) and the room rejoins
  with `sync1`, which merges both sides' edits.
- **Recovery.** Every 30 s the desktop app writes each room's whole state into
  `<settings folder>/recovery/room-<hash>.json`; joining that room again (`mode: join`) merges
  it first, so offline edits made before a crash aren't lost. Leaving deletes the file.
- **Viewers** (`role: view`) can't edit; anything that changes their workbook goes back to the
  room's state.
- **Limits.** An update is at most 16 MiB before base64 (a larger local change is announced with
  `sync1` so peers fetch it; a larger remote one is refused), awareness 64 KiB, 16 rooms per
  transport, one room per workbook.

## Tests

- `crates/collab/src/tests.rs`: two and three replicas with random concurrent edits (cells,
  styles, rows, columns, sheets, merges) and updates delivered out of order and twice converge,
  equal what a newcomer builds, and each model equals its own document; per-user undo; another
  app's room refused; garbage never crashes. `GRIDCRAFT_COLLAB_SEEDS` soaks it.
- `crates/engine/tests/collab_convergence.rs`: two and three sessions run random command
  sequences (cell edits, formats, row and column inserts and deletes, merges, sheets, per-user
  undo and redo) with updates exchanged in random order and duplicated; they end with identical
  workbooks and calculated values.
- `crates/engine/tests/collab_conformance.rs`: replays craft-collab's GridCraft schema-1
  transcripts (`CRAFT_COLLAB_DIR`, or a `craft-collab` checkout next to this one).
- `crates/engine/tests/collab_push.rs`: a fake transport (the test binary) relays between two
  sessions through the push channel: joining from a plugin command's result, sync, edits both
  ways, `collab/snapshot` with the file, awareness, undo, leaving.
- `crates/engine/tests/collab_loopback.rs`: two GridCraft processes through craft-collab's
  loopback transport (`CRAFT_COLLAB_LOOPBACK` = the built plugin).

## Not yet

- Text inside a cell is a plain string, last writer wins (two people typing in one cell keep the
  later input), not a Yjs text.
- Tables, charts, conditional formats, validation, comments and other features are shared as
  whole JSON values per sheet: two people changing the same feature at once keep the later.
- The web build has no plugins; it will speak the y-websocket binding natively.
- yrs garbage collection is off (it crashed with the undo manager in the fuzzer): deleted
  content stays in the document's history.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-15 | minor | Share and the session's people in the title bar (state dot, popover with people, Copy invitation and Leave; circles with "+N"; click to go to a person's selection); the Share dialog points at Automate › Add-ins when no transport is installed; a transport command's `collab.join` is now joined by the interface and its invitation kept; Review › Share group removed; status bar keeps only read-only reasons and problems |
| 2026-10-15 | major | Created: plugin protocol 2 (2.0.1) in GridCraft: push channel, rooms, schema 1, encryption, per-user undo, awareness, recovery, Review › Share |
