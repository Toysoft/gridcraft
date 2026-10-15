//! Regional number formats: how numbers and dates look, and how they are typed, in one region.
//!
//! Workbooks, formulas, files and commands keep one canonical (en-US) spelling. A
//! [`NumberLocale`] only applies where people read or type values: the cells on screen, the cell
//! editor, print, and delimited text written for people. It is the region's fixed conventions
//! ([`Region`]: date order, month and day names, the built-in formats Excel draws in the region's
//! own way) plus the decimal and thousands separators, which the user may override (Excel's
//! File › Options › Advanced › "Use system separators").
//!
//! Regions are data. de-DE was observed black-box in Excel 365 (German interface, de-DE regional
//! format) on 2026-10-13 through its automation interface: typed entries read back with their
//! value and automatic format, and values shown through format codes. Other regions use en-US
//! until their conventions are observed.

/// The order of day, month and year in a short date.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateOrder {
    /// `10/13/2026` (en-US).
    Mdy,
    /// `13.10.2026` (de-DE).
    Dmy,
}

/// The fixed conventions of one regional format.
#[derive(Debug, PartialEq, Eq)]
pub struct Region {
    /// BCP 47 tag, e.g. `de-DE`.
    pub tag: &'static str,
    /// The separators the region uses ("Use system separators").
    pub decimal: char,
    pub thousands: char,
    pub date_order: DateOrder,
    /// What `/` in a date format code shows, and what typed dates are separated by.
    pub date_separator: char,
    pub months: [&'static str; 12],
    pub months_short: [&'static str; 12],
    /// Sunday first.
    pub weekdays: [&'static str; 7],
    pub weekdays_short: [&'static str; 7],
    /// More month spellings typed dates accept (`Mär`, `Sept`), besides the names above.
    pub month_aliases: &'static [(&'static str, u32)],
    /// Built-in date and time formats the region draws its own way: `(canonical code, code used
    /// to draw it)`. Excel stores id 14 as `m/d/yyyy` and shows it as the system short date.
    pub builtin_formats: &'static [(&'static str, &'static str)],
    /// What the system long date and time tags (`[$-F800]`, `[$-F400]`, Excel's Long Date and
    /// Time formats) draw; `None` draws the code after the tag.
    pub long_date: Option<&'static str>,
    pub long_time: Option<&'static str>,
    /// A currency symbol typed after the number (`5 €`), with the formats Excel then applies
    /// (whole, two decimals).
    pub currency_after: Option<(&'static str, &'static str, &'static str)>,
}

pub const EN_US: Region = Region {
    tag: "en-US",
    decimal: '.',
    thousands: ',',
    date_order: DateOrder::Mdy,
    date_separator: '/',
    months: ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"],
    months_short: ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"],
    weekdays: ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"],
    weekdays_short: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
    month_aliases: &[],
    builtin_formats: &[],
    long_date: None,
    long_time: None,
    currency_after: None,
};

/// Deutsch (Deutschland), as observed in Excel 365 with the de-DE regional format.
pub const DE_DE: Region = Region {
    tag: "de-DE",
    decimal: ',',
    thousands: '.',
    date_order: DateOrder::Dmy,
    date_separator: '.',
    months: ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"],
    months_short: ["Jan", "Feb", "Mrz", "Apr", "Mai", "Jun", "Jul", "Aug", "Sep", "Okt", "Nov", "Dez"],
    weekdays: ["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"],
    weekdays_short: ["So", "Mo", "Di", "Mi", "Do", "Fr", "Sa"],
    month_aliases: &[("Mär", 3), ("Sept", 9)],
    // Observed: 14 `TT.MM.JJJJ`, 15 `TT. MMM JJ`, 16 `TT. MMM`, 17 `MMM JJ`, 20 `hh:mm`,
    // 21 `hh:mm:ss`, 22 `TT.MM.JJJJ hh:mm`; the AM/PM and elapsed-time ones are unchanged.
    builtin_formats: &[
        ("m/d/yyyy", "dd/mm/yyyy"),
        ("d-mmm-yy", "dd\\.\\ mmm\\ yy"),
        ("d-mmm", "dd\\.\\ mmm"),
        ("mmm-yy", "mmm\\ yy"),
        ("h:mm", "hh:mm"),
        ("h:mm:ss", "hh:mm:ss"),
        ("m/d/yyyy h:mm", "dd/mm/yyyy hh:mm"),
    ],
    // Observed: `[$-F800]dddd, mmmm dd, yyyy` shows `Dienstag, 13. Oktober 2026`; `[$-F400]h:mm:ss
    // AM/PM` shows `14:30:00`.
    long_date: Some("dddd, d\\. mmmm yyyy"),
    long_time: Some("hh:mm:ss"),
    // Observed: `5 €` gets `#.##0 €;[Rot]-#.##0 €`, `5,50 €` gets `#.##0,00 €;[Rot]-#.##0,00 €`.
    currency_after: Some(("€", "#,##0 \"€\";[Red]-#,##0 \"€\"", "#,##0.00 \"€\";[Red]-#,##0.00 \"€\"")),
};

/// How numbers and dates are shown and typed: a region's conventions and the separators in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NumberLocale {
    pub region: &'static Region,
    pub decimal: char,
    pub thousands: char,
}

impl Default for NumberLocale {
    fn default() -> Self {
        NumberLocale::EN_US
    }
}

impl NumberLocale {
    /// The canonical locale: what files, formulas and commands use.
    pub const EN_US: NumberLocale = NumberLocale::of(&EN_US);
    pub const DE_DE: NumberLocale = NumberLocale::of(&DE_DE);

    /// A region with its own separators.
    pub const fn of(region: &'static Region) -> NumberLocale {
        NumberLocale { region, decimal: region.decimal, thousands: region.thousands }
    }

    /// The same region with other separators. An unusable pair ([`separators_usable`]) keeps
    /// the region's.
    pub fn with_separators(self, decimal: char, thousands: char) -> NumberLocale {
        if separators_usable(decimal, thousands) { NumberLocale { decimal, thousands, ..self } } else { self }
    }

    /// True when shown and typed values use the canonical spelling (nothing to translate).
    pub fn is_canonical(&self) -> bool {
        *self == NumberLocale::EN_US
    }

    /// The code a built-in format is drawn with in this region (`m/d/yyyy` → `dd/mm/yyyy` in
    /// de-DE); the code itself otherwise.
    pub fn regional_code<'a>(&self, code: &'a str) -> &'a str {
        let tag = |t: &str| code.get(..t.len()).is_some_and(|p| p.eq_ignore_ascii_case(t));
        if tag("[$-F800]")
            && let Some(long) = self.region.long_date
        {
            return long;
        }
        if tag("[$-F400]")
            && let Some(long) = self.region.long_time
        {
            return long;
        }
        self.region.builtin_formats.iter().find(|(canonical, _)| *canonical == code).map_or(code, |(_, shown)| shown)
    }
}

/// Whether a decimal and thousands separator pair can be used: different characters, neither a
/// digit, a letter or a character with another meaning in typed numbers and dates (signs, `%`,
/// parentheses, quotes, `/`, `:`, `#`, `$`, `;`, control characters). The thousands separator may
/// be a space, a no-break space or an apostrophe (`1'234.5`); the decimal separator may not.
pub fn separators_usable(decimal: char, thousands: char) -> bool {
    let bad = |c: char| {
        c.is_alphanumeric() || c.is_control() || matches!(c, '+' | '-' | '%' | '(' | ')' | '"' | '#' | '$' | '/' | ':' | '@' | '[' | ']' | ';')
    };
    let space = |c: char| c.is_whitespace();
    decimal != thousands
        && !bad(decimal)
        && !space(decimal)
        && !matches!(decimal, '\'' | '\u{2019}')
        && !bad(thousands)
        && (!space(thousands) || matches!(thousands, ' ' | '\u{a0}' | '\u{202f}'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_en_us_is_canonical() {
        assert!(NumberLocale::EN_US.is_canonical());
        assert!(NumberLocale::default().is_canonical());
        assert!(!NumberLocale::DE_DE.is_canonical());
        // en-US conventions with other separators are not canonical either.
        assert!(!NumberLocale::EN_US.with_separators(',', '.').is_canonical());
    }

    #[test]
    fn separators_are_checked() {
        let de = NumberLocale::DE_DE;
        assert_eq!(de.with_separators('.', ',').decimal, '.');
        assert_eq!(de.with_separators(',', ' ').thousands, ' ');
        assert_eq!(de.with_separators(',', '\u{a0}').thousands, '\u{a0}');
        assert_eq!(de.with_separators('.', '\'').thousands, '\'');
        for (d, t) in [(',', ','), ('1', '.'), (',', 'a'), (' ', '.'), ('-', '.'), (',', '%'), ('E', '.'), ('\'', '.'), (',', '\n'), (',', ';')] {
            assert_eq!(de.with_separators(d, t), de, "{d:?} {t:?}");
        }
    }

    #[test]
    fn built_in_formats_follow_the_region() {
        assert_eq!(NumberLocale::DE_DE.regional_code("m/d/yyyy"), "dd/mm/yyyy");
        assert_eq!(NumberLocale::DE_DE.regional_code("h:mm AM/PM"), "h:mm AM/PM");
        assert_eq!(NumberLocale::DE_DE.regional_code("0.00"), "0.00");
        assert_eq!(NumberLocale::EN_US.regional_code("m/d/yyyy"), "m/d/yyyy");
        // Excel's Long Date and Time formats carry the system tags.
        assert_eq!(NumberLocale::DE_DE.regional_code("[$-F800]dddd, mmmm dd, yyyy"), "dddd, d\\. mmmm yyyy");
        assert_eq!(NumberLocale::DE_DE.regional_code("[$-f400]h:mm:ss AM/PM"), "hh:mm:ss");
        assert_eq!(NumberLocale::EN_US.regional_code("[$-F800]dddd, mmmm dd, yyyy"), "[$-F800]dddd, mmmm dd, yyyy");
    }
}
