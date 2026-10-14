# Bitmap-only EMF and import-note reproduction

Generate the fixture from this source checkout:

```sh
cargo run --release -p gridcraft-ui-egui --example bitmap_emf_repro -- synthetic-bitmap-emf.xlsx
```

The example writes an XLSX using GridCraft's own writer. All cells, XML and image pixels are
generated in code. No real organization, person, vendor artwork, Office-created workbook or
font file is included. The single external-link target is the non-resolving
`https://example.invalid/synthetic-source.xlsx`; no cell formula depends on it.

Open the generated file using File > Open, or a control/MCP open command. On unmodified
`main` at `84acf6293b1dc1fc09837524e035d674eb4d6741` (0.6.1):

- The image anchored at B6 appears as a gray placeholder instead of four colored quadrants.
- A modal displays the external-link preservation warning even though the workbook loaded.
- A fresh raw `app.open` returns `ok: false` with that warning although it loaded the workbook.

With this fix, the image shows red/green above blue/yellow and a dismissible information bar
states that external links are preserved but this version does not read or automatically update
linked workbooks. The Chinese interface translates the notice, and changing language updates
the already-visible notice. There is no enable/refresh action. Raw `app.open` returns success;
actual read failures still fail. Save to a new XLSX and reopen to verify that the picture persists.

The decoder accepts only complete header/STRETCHDIBITS/EOF bitmap EMFs with uncompressed
24/32-bit DIB pixels and SRCCOPY. It does not render arbitrary vector EMF/WMF or implement
external-link calculation/refresh. Original embedded picture bytes are retained when saving.
The unit fixtures separately cover padded 24-bit rows, 32-bit BGRX, top-down/bottom-up rows,
transparent source extents, truncated records, hostile dimensions/offsets and vector refusal.

For automation, keep control on loopback inside an isolated environment and stop it after the
test. The published before/after screenshots are of this synthetic file only.
