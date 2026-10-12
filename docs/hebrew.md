# Hebrew interface

Choose **עברית** in **View → Interface language** or **File → Options → Interface language**.
The choice is saved with the other UI preferences. On first run, a Hebrew system or browser
language selects Hebrew automatically; `he`, `he-IL`, `he_IL` and legacy `iw` tags are recognized.

Agents can select the same language through the control channel:

```json
{"id":1,"method":"engine.execute","params":{"command":"app.language.set","params":{"language":"he"}}}
```

The Hebrew catalog covers all 381 keys in the existing interface catalog: ribbon tabs, buttons,
tooltips, number-format categories, status labels, page-layout controls and the File view,
including its settings, print and export labels. Strings outside that catalog still fall back
to English, as they do for the other interface languages. Translations use logical Unicode
order; formulas keep English function names, decimal points and comma argument separators.
Language switching preserves workbook text, formulas and sheet direction.

For a grid with column A on the right, use **Page Layout → Sheet Options → Sheet Right-to-Left**
(**פריסת עמוד → גיליון מימין לשמאל**) or the `view.rightToLeft` command. This is an independent
workbook setting and is saved in XLSX. Selecting Hebrew does not toggle it.

Native builds use GridCraft's existing system-font fallback for Hebrew. Install a compatible
Hebrew font (for example, Noto Sans Hebrew) if labels appear as boxes; fonts are not bundled
with this change. Web builds do not load the native system-font fallback, so Hebrew glyph
coverage depends on the fonts supplied by the build.

This is interface localization, not full right-to-left layout support. egui shapes Hebrew runs,
but does not implement full bidirectional paragraph layout; mixed Hebrew/Latin text, punctuation,
wrapping and editor caret behavior can still be incorrect. Ribbon order, sheet tabs and print
layout are not mirrored by the language preference. PDF export still lacks non-Latin font
embedding. Native-speaker review and full script support remain follow-up work.

Related Arabic interface and RTL work is proposed in [PR #721](https://github.com/storytold/gridcraft/pull/721).
This Hebrew catalog is based on `main` and can use shared RTL infrastructure when it lands.
