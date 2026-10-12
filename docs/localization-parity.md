# Localization parity

> **Last reviewed:** 2026-10-10 · **Last updated:** 2026-10-11 · **Change:** minor (ten Indian languages, Indian-script fonts, shaping re-checked) · **Target:** Microsoft Excel (Microsoft 365), Excel for Mac 16.113.4

Per-language status against Excel, which ships its UI in dozens of languages (Windows language
packs cover 100+), with localized function names, argument separators, number and date formats,
right-to-left sheets and full complex-script support. Part of
[target-app-parity.md](target-app-parity.md). **Localization: ~10% ready, 100–165 h** (estimated).

## Where we are (measured from source on `main`, after the 2026-10-10 merges)

- **A first catalog exists, for the ribbon only.** `crates/ui-egui/src/i18n.rs` (merged 10-10 in
  #31, #104, #82; German added 10-11) adds an interface-language preference that defaults to the
  system language (closes #20) and a View-tab switcher, with English-keyed lookup tables:
  the Japanese reference table now holds 381 labels (ribbon, status bar, Backstage), and every
  shipped language (es, zh, ja, ko, ru, pt-BR, de, cs, fr and the ten Indian languages) is tested
  to cover each key. The tables are used by the ribbon, sheet tabs, Backstage and widgets (`ribbon.rs`, `tabs.rs`, `widgets.rs`); dialogs, menus,
  context menus, task panes, status messages, command labels and function descriptions are still
  English literals. Untranslated labels fall back to English.
- **About 2,200 user-visible strings** in all (heuristic source scan: ~1,700 UI and command
  strings plus ~500 function descriptions), so each shipped language covers **~17%** (measured
  count / estimated total).
- **en-US only formulas and formats.** Function names, `,` argument separator, `.` decimal
  separator, en-US number and date formats (`crates/numfmt` says "en-US only"). Users ask for
  localized function names and `;` separators (#45). German with localized numbers and formulas
  is in PR #163 (open).
- **Indian languages (2026-10-11).** Hindi, Bengali, Marathi, Telugu, Tamil, Gujarati, Kannada,
  Malayalam, Odia and Punjabi (Gurmukhi) catalogs (`i18n/{hi,bn,mr,te,ta,gu,kn,ml,or,pa}.rs`), each
  covering every key of the reference table (381 labels, ~17%), with their locale tags
  (`hi-IN`…; Shahmukhi `pa-Arab`/`pa-PK` stays English). Machine-translated, no native review yet.
- **Script support.** System-font fallback for CJK, Thai and Hebrew landed in #38 (closes #32;
  #142 is still open pending confirmation), and for the nine Indian scripts (Devanagari, Bengali,
  Gurmukhi, Gujarati, Odia, Tamil, Telugu, Kannada, Malayalam) on 2026-10-11: Nirmala UI and the
  per-script faces on Windows, Kohinoor/Noto/Sangam MN on macOS, Noto Sans or Lohit on Linux and
  the BSDs. **Text is shaped**: epaint 0.36 runs every run through harfrust (OpenType GSUB/GPOS),
  so conjuncts, reordered and split vowel signs render correctly wherever a face is present
  (tested: `theme::tests::indic_text_is_shaped_not_drawn_letter_by_letter`). The web build has no
  system fonts, so CJK and Indian-script interfaces are not auto-selected there and Indian text
  shows as boxes until craft-fonts ships Noto faces for these scripts. There is no bidirectional
  layout: Arabic and Hebrew runs shape but don't reorder. **Sheet Right-to-Left**
  (`view.rightToLeft`, Page Layout › Sheet Options, XLSX `rightToLeft`) mirrors the grid (#146):
  column A and the row headers on the right, the vertical scrollbar on the left; hit-testing,
  arrow keys, editor, popups and floating objects follow; text stays left-to-right inside its box
  (no bidi), and the sheet tabs, horizontal scrollbar and printing/PDF are not mirrored yet. IME input goes
  through egui/winit and is unverified for Chinese, Japanese and Korean.
- **PDF export** uses only the 14 standard PDF fonts: non-Latin text cannot export.

## Languages

UI strings translated are measured counts against the ~2,200 estimate. Estimates to `full`
include the rest of the catalog work below (10–20 h, counted once), localized function names,
separators, number/date formats, machine translation with agent review, and script work.

| Language | Code | UI strings translated | Dialogs / tooltips / help | Script support | Native review | Status | Estimate to full |
|---|---|---|---|---|---|---|---|
| English | en | ~2,200 / ~2,200 (100%, source) | yes | Latin: yes | yes | full | — |
| Simplified Chinese (Mandarin) | zh-Hans | 266 (~12%) | no | CJK system-font fallback (#38); IME unverified; PDF can't export | no | menus only | 8–13 h |
| Spanish | es | 381 (~17%) | no | Latin: yes | no | menus only | 6–10 h |
| Hindi | hi | 381 (~17%) | no | Devanagari: shaped, system faces; web: none | no | menus only | 6–10 h |
| Arabic | ar | 0 (0%) | no | **no shaping, no RTL UI**; Sheet Right-to-Left mirrors the grid (#146) | no | none | 25–35 h |
| French | fr | 381 (~17%) | no | Latin: yes | no | menus only | 6–10 h |
| Portuguese | pt-BR | 266 (~12%) | no | Latin: yes | no | menus only | 4–8 h |
| Indonesian | id | 0 (0%) | no | Latin: yes | no | none | 5–8 h |
| Japanese | ja | 268 (~12%) | no | CJK system-font fallback; IME unverified; no furigana display | no | menus only | 8–12 h |
| German | de | 266 (~12%) | no | Latin: yes | no | menus only | 4–8 h |
| Korean | ko | 266 (~12%) | no | Hangul via the CJK fallback; IME unverified | no | menus only | 6–10 h |
| Vietnamese | vi | 0 (0%) | no | Latin with stacked diacritics: unverified | no | none | 6–10 h |

Other shipped languages: Russian (ru) and Czech (cs), menus only. Spanish (es) and French (fr)
catalogs also ship (381 labels).

Other Indian languages (menus only, 381 labels ~17% each, shaped with system faces, no native
review; 4–8 h each to full once the catalog covers dialogs):

| Language | Code | Script | Faces (Windows · macOS · Linux/BSD) |
|---|---|---|---|
| Bengali | bn | Bengali | Nirmala UI, Vrinda · Kohinoor Bangla · Noto Sans Bengali, Lohit |
| Marathi | mr | Devanagari | Nirmala UI, Mangal · Kohinoor Devanagari · Noto Sans Devanagari, Lohit |
| Telugu | te | Telugu | Nirmala UI, Gautami · Kohinoor Telugu · Noto Sans Telugu, Lohit |
| Tamil | ta | Tamil | Nirmala UI, Latha · Tamil Sangam MN · Noto Sans Tamil, Lohit |
| Gujarati | gu | Gujarati | Nirmala UI, Shruti · Kohinoor Gujarati · Noto Sans Gujarati, Lohit |
| Kannada | kn | Kannada | Nirmala UI, Tunga · Noto Sans Kannada · Noto Sans Kannada, Lohit |
| Malayalam | ml | Malayalam | Nirmala UI, Kartika · Malayalam Sangam MN · Noto Sans Malayalam, Lohit |
| Odia | or | Odia | Nirmala UI, Kalinga · Noto Sans Oriya · Noto Sans Oriya, Lohit |
| Punjabi | pa | Gurmukhi | Nirmala UI, Raavi · Gurmukhi Sangam MN · Noto Sans Gurmukhi, Lohit |

Urdu (Arabic script, right-to-left) waits for bidirectional layout and an RTL interface.

**Total:** catalog work 10–20 h + languages 86–136 h ≈ **95–155 h** (Hindi no longer needs shaping work). Parallelizes almost fully
once the catalog covers dialogs and commands. **Needs a human:** native-speaker review of every
language (none so far) and of localized function names.

## Infrastructure (do first)

1. Extend the catalog from the ribbon to every user-visible string (dialogs, menus, task panes,
   status and error messages, command labels, function descriptions), with a missing-key check
   in `cargo xtask ci`. Language switching and the system-language default already exist (#31).
2. Locale-aware formulas: localized function-name tables and separators at the formula-bar
   boundary only (files keep en-US invariant names), locale number and date formats, list
   separators in CSV.
3. Script support: shaping is done (epaint/harfrust) and system faces cover CJK, Thai, Hebrew and
   the Indian scripts. Left: bidirectional runs, right-to-left sheet tabs and printing, script
   faces for the web build (craft-fonts), embedded subset fonts in PDF.

## Revision history

| Date | Change | Summary |
|---|---|---|
| 2026-10-11 | minor | Ten Indian languages (hi, bn, mr, te, ta, gu, kn, ml, or, pa; 381 labels each), Indian-script system faces on Windows/macOS/Linux/BSD; corrected "no shaping": epaint 0.36 shapes with harfrust. Hindi 12–18 h → 6–10 h; total 95–155 h |
| 2026-10-11 | minor | German ribbon catalog (`i18n/de.rs`, 266 labels, ~12%); `de` locale tags (de-DE, de-AT, de-CH…) resolve to German. PR #163 (localized German formulas/numbers) still open |
| 2026-10-11 | minor | Sheet Right-to-Left mirrors the grid (#146) |
| 2026-10-11 | minor | Czech (cs) ribbon table: 266 labels (~12%), menus only |
| 2026-10-10 | minor | Re-measured after #31, #104, #82, #38 merged: ribbon translated into ja, zh, ko, pt-BR and ru (266–268 labels, ~12%), system-language default, CJK/Thai/Hebrew font fallback; ~10% ready, 100–165 h |
| 2026-10-10 | major | Created: English only, no catalog, ~2,200 strings, script-support gaps, open language PRs |
