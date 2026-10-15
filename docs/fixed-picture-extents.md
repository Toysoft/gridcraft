# Fixed picture extents and native-reference display limits

A fixed-size 320 × 160-pixel image (3048000 × 1524000 EMU) must keep those extents when
an importing application substitutes a font and uses different column widths. Previously,
the writer emitted two-cell markers for every drawing, even `editAs="absolute"` or
`editAs="oneCell"`. LibreOffice Calc 24.2.7.2 interpreted those markers using its own column
metrics: an absolute image moved from 2540 to 3048 hundredths of a millimetre on X, and its
width changed from approximately 8467 to 9991 hundredths of a millimetre on a synthetic
GridCraft save/reopen. Fixed drawings now use standard absolute/one-cell anchors with
explicit extents. Move-and-size drawings keep their cell-relative range semantics.

Generate the original fixture with:

```sh
cargo run --release -p gridcraft-ui-egui --example fixed_picture_repro -- synthetic.xlsx
```

The example exercises the native absolute-anchor writer with explicit drawing extents. It uses only original generated values and solid PNG pixels. Open/save it
in GridCraft and compare the original and saved files in LibreOffice, with macros/link updates
disabled. Check the public UNO shape size/position and captures at 50%, 100%, 150%, not just
the worksheet zoom label. No vendor code or real organization/person workbook was used.

The previously reported additional 3–4% difference at equal zoom is a LibreOffice *view*
difference, not an imported image-width loss: the original absolute-anchor file's UNO transform
is 8467 × 4233 hundredths of a millimetre (320 × 160 pixels at 96 dpi), but Calc renders it
as 309 × 160 screen pixels at 100%. Its public device conversion still gives 96 × 96 pixels
per inch. Seven Normal-font/size variants render widths 309–310 while the image transform is
unchanged. In a separate test profile, enabling the documented `UsePrinterMetrics` option
changes the reference picture to 318 × 160 (and 159 × 80 / 481 × 240 at 50% / 150%), without
changing the stored image dimensions. Small rounding/layout differences remain.

Therefore do not apply a guessed 0.97 horizontal image scale in GridCraft: that would alter
correct physical geometry and distort the aspect ratio. Reference comparisons must separate
file/model geometry from the reference application's screen transform. This source change
fixes actual saved-file geometry; it does not change LibreOffice's implementation, install
fonts, modify desktop DPI or claim all screen pixels match.

Sources: public [DrawingML worksheet anchors](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.drawing.spreadsheet.worksheetdrawing),
[shape extents](https://learn.microsoft.com/en-us/dotnet/api/documentformat.openxml.drawing.spreadsheet.extent),
[LibreOffice device-unit conversion](https://api.libreoffice.org/docs/idl/ref/interfacecom_1_1sun_1_1star_1_1awt_1_1XUnitConversion.html),
and [UsePrinterMetrics](https://api.libreoffice.org/docs/idl/ref/servicecom_1_1sun_1_1star_1_1sheet_1_1GlobalSheetSettings.html).
