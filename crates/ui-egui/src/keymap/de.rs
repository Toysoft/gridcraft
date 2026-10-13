//! Excel's German shortcuts (Microsoft 365, German interface, German keyboard) where they differ
//! from the English ones in [`super::lookup`]. Measured by pressing each key twice in Excel, not
//! taken from Microsoft's German list, which is wrong in places: it names Strg+0 for Hide Columns
//! (Excel uses Strg+8) and Strg+Umschalt+_ for removing the border (Excel adds one).
//!
//! Keys that Excel leaves free in German keep their English binding (Strg+B stays Bold), unless
//! the English one would do something unexpected there ([`Shortcut::Unbound`]).

use egui::{Key, Modifiers};
use serde_json::json;

use super::{Shortcut, borders, number_format, run};

/// The German binding for a key press (Windows and Linux; Excel for Mac isn't measured). Letters
/// go by `key`, the letter the layout types (Z and Y swap on QWERTZ). Digits and symbol keys go
/// by `physical`, the key's position named after a US keyboard: Strg+# is the key left of Enter
/// (`Backslash`), `+` is `CloseBracket`, `-` is `Slash`, `^` is `Backtick`, `´` is `Equals` and `<`
/// is `IntlBackslash`.
pub fn lookup(key: Key, physical: Option<Key>, m: Modifiers) -> Option<Shortcut> {
    use Key::*;
    let letter = key.name().len() == 1 && key.name().chars().all(|c| c.is_ascii_uppercase());
    let k = if letter { key } else { physical.unwrap_or(key) };
    let ctrl = m.command && !m.alt;
    let alt = m.alt && !m.ctrl && !m.command;
    let ctrl_alt = m.alt && m.command;
    let s = m.shift;
    Some(match k {
        // Strg+Umschalt+F / K / U: Fett, Kursiv, Unterstrichen; Strg+U: Unten ausfüllen.
        F if ctrl && s => run("home.bold"),
        K if ctrl && s => run("home.italic"),
        U if ctrl && s => run("home.underline"),
        U if ctrl => run("edit.fillDown"),
        A | P if ctrl && s => Shortcut::Dialog("formatCells", json!({"tab": "Font"})),
        // Strg+. date, Strg+: time; Strg+, formula and Strg+; value of the cell above.
        Period if ctrl && s => Shortcut::Time,
        Period if ctrl => Shortcut::Date,
        Comma if ctrl && s => Shortcut::ValueAbove,
        Comma if ctrl => Shortcut::FormulaAbove,
        // Strg+# date and Strg+^ time format; Strg+` (Umschalt+´) shows formulas.
        Backslash if ctrl && !s => number_format("d-mmm-yy"),
        Backtick if ctrl && !s => number_format("h:mm"),
        Equals if ctrl && s => run("formulas.showFormulas"),
        // Strg++ inserts, Strg+- deletes, Strg+* (Umschalt++) selects the current region, Strg+_
        // (Umschalt+-) draws the outline border.
        CloseBracket if ctrl && s => run("selection.currentRegion"),
        CloseBracket if ctrl => Shortcut::Dialog("insertCells", json!({})),
        Slash if ctrl && s => borders("outside"),
        Slash if ctrl => Shortcut::Dialog("deleteCells", json!({})),
        // Strg+< / Strg+>: Unten / Rechts ausfüllen.
        IntlBackslash if ctrl && s => run("edit.fillRight"),
        IntlBackslash if ctrl => run("edit.fillDown"),
        // Number formats by digit: 2 Scientific and 6 General differ from English (@ and ^).
        Num2 if ctrl && s => number_format("0.00E+00"),
        Num6 if ctrl && s => Shortcut::Run("home.numberFormat", json!({"format": "General"})),
        // Strg+( and Strg+) unhide rows and columns; Strg+8 hides columns. Strg+Umschalt+0 types
        // `=` and does nothing in Excel; the English Ctrl+Shift+= behind it would insert cells.
        Num8 if ctrl && s => run("home.unhideRows"),
        Num9 if ctrl && s => run("home.unhideColumns"),
        Num0 if ctrl && s => Shortcut::Unbound,
        Num8 if ctrl => run("home.hideColumns"),
        // Alt+= (Alt+Umschalt+0) is AutoSum; Strg+Alt+= / Strg+Alt+- zoom.
        Num0 if alt && s => run("formulas.autoSum"),
        Num0 if ctrl_alt && s => Shortcut::Zoom(10),
        Slash if ctrl_alt && !s => Shortcut::Zoom(-10),
        _ => return None,
    })
}
