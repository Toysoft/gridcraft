//! The regional format people see and type numbers in, and the display locale built from it.
//!
//! Numbers follow the interface language's regional format (German: `1.234,56`, `13.10.2026`)
//! unless File › Options › Advanced › "Use system separators" is off, in which case its Decimal
//! and Thousands separator fields apply, as in Excel. A preference kept in `ui.json`, never in a
//! workbook. Formulas follow the interface language ([`crate::i18n::Language::formula_locale`]).
//!
//! The interface language itself starts from the system's on first run, so "system" here means
//! the regional format of the language the interface uses.

use gridcraft_engine::core::NumberLocale;
use gridcraft_engine::display::DisplayLocale;
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use crate::i18n::Language;

/// File › Options › Advanced › Use system separators, and the separators used without it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Separators {
    pub use_system: bool,
    /// One character each; empty or unusable falls back to the regional format's.
    pub decimal: String,
    pub thousands: String,
}

impl Default for Separators {
    fn default() -> Self {
        Separators { use_system: true, decimal: String::new(), thousands: String::new() }
    }
}

/// Reads a saved preference leniently: anything malformed is the default, rather than failing the
/// whole `UiState` (which would reset every other preference with it).
pub fn lenient<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Separators, D::Error> {
    let v = Json::deserialize(d)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

/// The regional format of an interface language. Languages whose regional format hasn't been
/// observed yet use en-US.
pub fn region_of(language: Language) -> NumberLocale {
    match language {
        Language::De => NumberLocale::DE_DE,
        _ => NumberLocale::EN_US,
    }
}

fn one_char(s: &str) -> Option<char> {
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

/// The number locale numbers are shown and typed in.
pub fn number_locale(language: Language, separators: &Separators) -> NumberLocale {
    let region = region_of(language);
    if separators.use_system {
        return region;
    }
    let decimal = one_char(&separators.decimal).unwrap_or(region.decimal);
    let thousands = one_char(&separators.thousands).unwrap_or(region.thousands);
    region.with_separators(decimal, thousands)
}

/// How values are shown to people in this interface: numbers in [`number_locale`], booleans,
/// errors and formulas in the language's formula syntax.
pub fn display_locale(language: Language, separators: &Separators) -> DisplayLocale {
    DisplayLocale { number: number_locale(language, separators), formula: language.formula_locale() }
}

/// `options.separators {useSystem?, decimal?, thousands?}`: changes the preference and answers
/// with it and the separators in effect. A pair that can't be used (a digit, a letter, the same
/// character twice…) is refused and nothing changes.
pub fn command(language: Language, separators: &mut Separators, p: &Json) -> Result<Json, String> {
    let mut next = separators.clone();
    if let Some(on) = p.get("useSystem").and_then(Json::as_bool) {
        next.use_system = on;
    }
    for (key, slot) in [("decimal", &mut next.decimal), ("thousands", &mut next.thousands)] {
        if let Some(v) = p.get(key) {
            let s = v.as_str().ok_or_else(|| format!("`{key}` must be one character"))?;
            if one_char(s).is_none() {
                return Err(format!("`{key}` must be one character"));
            }
            *slot = s.to_string();
        }
    }
    if !next.use_system {
        let region = region_of(language);
        let d = one_char(&next.decimal).unwrap_or(region.decimal);
        let t = one_char(&next.thousands).unwrap_or(region.thousands);
        if !gridcraft_engine::core::locale::separators_usable(d, t) {
            return Err(format!("{d:?} and {t:?} can't be used as the decimal and thousands separators"));
        }
    }
    *separators = next;
    let loc = number_locale(language, separators);
    Ok(json!({"useSystem": separators.use_system, "decimal": loc.decimal.to_string(), "thousands": loc.thousands.to_string()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_follow_the_language_unless_separators_are_set() {
        let system = Separators::default();
        assert_eq!(number_locale(Language::De, &system), NumberLocale::DE_DE);
        assert_eq!(number_locale(Language::En, &system), NumberLocale::EN_US);
        assert_eq!(number_locale(Language::Fr, &system), NumberLocale::EN_US);
        let own = Separators { use_system: false, decimal: ".".into(), thousands: "'".into() };
        let swiss = number_locale(Language::De, &own);
        assert_eq!((swiss.decimal, swiss.thousands, swiss.region.tag), ('.', '\'', "de-DE"));
        // Unusable or missing fields fall back to the region's.
        let odd = Separators { use_system: false, decimal: "ab".into(), thousands: String::new() };
        assert_eq!(number_locale(Language::De, &odd), NumberLocale::DE_DE);
        assert!(display_locale(Language::De, &system).formula == gridcraft_engine::formula::FormulaLocale::De);
        assert!(display_locale(Language::En, &system).is_canonical());
    }

    #[test]
    fn the_command_validates_and_reports() {
        let mut s = Separators::default();
        let r = command(Language::De, &mut s, &json!({})).unwrap();
        assert_eq!(r, json!({"useSystem": true, "decimal": ",", "thousands": "."}));
        let r = command(Language::De, &mut s, &json!({"useSystem": false, "decimal": ".", "thousands": ","})).unwrap();
        assert_eq!(r, json!({"useSystem": false, "decimal": ".", "thousands": ","}));
        for bad in [json!({"decimal": "1"}), json!({"decimal": ".."}), json!({"thousands": "."}), json!({"decimal": 5}), json!({"thousands": ""})] {
            assert!(command(Language::De, &mut s, &bad).is_err(), "{bad}");
        }
        assert_eq!(s.decimal, ".", "a refused change changes nothing");
        // Malformed saved preferences read as the default.
        let back: Separators = lenient(json!(5)).unwrap();
        assert_eq!(back, Separators::default());
        let back: Separators = lenient(json!({"useSystem": false, "decimal": ";"})).unwrap();
        assert!(!back.use_system);
    }
}
