//! The keytip sequence table: every Excel-style Alt access sequence and what it does.
//!
//! The matcher in [`super::KeyTips::process`] and the badge painter in `ribbon.rs` both read
//! [`SEQUENCES`], so a sequence is added in exactly one place: `keytips.json`.
//!
//! `keytips.json` is the human-edited source of truth (grouped, commented, one line per
//! sequence; `pending` reserves sequences for commands that do not exist yet). `generated.rs`
//! is produced from it by `cargo xtask keytips` — edit the JSON, not the generated file.
//!
//! Key tips follow the interface language, as in Excel: German Excel's Home tab is Alt, R and
//! Bold is Alt, R, 1. [`Table::for_language`] picks the language's own table ([`SEQUENCES_DE`],
//! measured in German Excel) or the English one. Sequences may hold Ä, Ö, Ü and ß, so the
//! helpers count characters, not bytes.

pub(crate) use super::Action;
use crate::i18n::Language;

mod generated;
pub use generated::{SEQUENCES, SEQUENCES_DE};

/// What a fully-matched sequence does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// Switch to this ribbon tab and keep the prefix: the tab's own badges take over.
    Tab(&'static str),
    /// Keep the prefix: this sequence is a group-level badge; deeper sequences finish it.
    Continue,
    /// Complete the sequence: run the action and clear the prefix.
    Finish(Action),
}

/// One language's key tips.
#[derive(Clone, Copy, Debug)]
pub struct Table(&'static [(&'static str, Entry)]);

/// English Excel's key tips.
pub const ENGLISH: Table = Table(SEQUENCES);
/// German Excel's key tips.
pub const GERMAN: Table = Table(SEQUENCES_DE);

/// The first `n` characters of `s`.
fn head(s: &'static str, n: usize) -> &'static str {
    s.char_indices().nth(n).map_or(s, |(i, _)| &s[..i])
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

fn matches_target(action: &Action, target: &str) -> bool {
    match action {
        Action::Command(id) | Action::Pending(id) | Action::Control(id) => *id == target,
        Action::Dialog(name) => format!("dialog:{name}") == target,
        Action::Tab(_) => false,
    }
}

impl Table {
    /// The key tips of the interface `language`: its own when Excel's differ and are measured,
    /// else English.
    pub fn for_language(language: Language) -> Table {
        match language {
            Language::De => GERMAN,
            _ => ENGLISH,
        }
    }

    /// Look up a complete sequence.
    pub fn lookup(self, sequence: &str) -> Option<Entry> {
        self.0.iter().find(|(seq, _)| *seq == sequence).map(|(_, entry)| *entry)
    }

    /// True if `prefix` is a strict prefix of any known sequence (badges wait for more keys).
    pub fn has_deeper(self, prefix: &str) -> bool {
        self.0.iter().any(|(seq, _)| seq.len() > prefix.len() && seq.starts_with(prefix))
    }

    /// The sequence that switches to `tab_name`, if that tab has one (may be a multi-letter unit
    /// like Draw's "JI").
    pub fn tab_sequence(self, tab_name: &str) -> Option<&'static str> {
        self.0.iter().find_map(|(seq, entry)| match entry {
            Entry::Tab(name) if *name == tab_name => Some(*seq),
            _ => None,
        })
    }

    /// True if `seq` is a group level: an explicit Continue entry, or an implicit level that only
    /// exists because deeper sequences start with it (like "J" before Draw's "JI").
    pub fn is_group_level(self, seq: &str) -> bool {
        matches!(self.lookup(seq), Some(Entry::Continue)) || (self.lookup(seq).is_none() && self.has_deeper(seq))
    }

    /// All sequences that finish by running `target` (a command id or `dialog:<name>`).
    pub fn sequences_for(self, target: &str) -> Vec<&'static str> {
        self.0
            .iter()
            .filter_map(|(seq, entry)| match entry {
                Entry::Finish(action) if matches_target(action, target) => Some(*seq),
                _ => None,
            })
            .collect()
    }

    /// Group-level letters for a menu anchor: sequences one key past `prefix` that continue toward
    /// any of `targets` (the menu's items) — the badge shown on the button that opens the menu.
    pub fn group_extension(self, prefix: &str, targets: &[&str]) -> Vec<&'static str> {
        let depth = chars(prefix) + 1;
        let mut out = Vec::new();
        for target in targets {
            for seq in self.sequences_for(target) {
                if chars(seq) <= depth || !seq.starts_with(prefix) {
                    continue;
                }
                // The intermediate one key past the prefix is the group badge, if it is a
                // group-level sequence (explicit or implicit) rather than the finish itself.
                let candidate = head(seq, depth);
                if self.is_group_level(candidate) && !out.contains(&candidate) {
                    out.push(candidate);
                }
            }
        }
        out
    }

    /// The badge for a menu item: its finish sequence exactly one key below the current prefix.
    pub fn item_letter(self, prefix: &str, target: &str) -> Option<&'static str> {
        self.sequences_for(target).into_iter().find(|seq| chars(seq) == chars(prefix) + 1 && seq.starts_with(prefix))
    }

    /// Badge sequences for a command control: every sequence for `target` that continues from the
    /// current prefix (the painter strips the prefix, so the remaining letters show).
    pub fn control_badges(self, prefix: &str, target: &str) -> Vec<&'static str> {
        if prefix.is_empty() {
            return Vec::new(); // empty prefix shows only tab badges
        }
        self.sequences_for(target).into_iter().filter(|seq| seq.len() > prefix.len() && seq.starts_with(prefix)).collect()
    }

    /// The badge for a single control: the rest of its finish sequence when it has exactly one,
    /// or the common group letter when several sequences land on the same control (menu anchors
    /// like Borders, whose HBA..HBW variants all share "HB").
    pub fn control_badge(self, prefix: &str, target: &str) -> Option<&'static str> {
        if prefix.is_empty() {
            return None; // empty prefix shows only tab badges
        }
        let seqs: Vec<&'static str> =
            self.sequences_for(target).into_iter().filter(|seq| seq.len() > prefix.len() && seq.starts_with(prefix)).collect();
        match seqs.as_slice() {
            [] => None,
            [single] => Some(single),
            [first, rest @ ..] => {
                let common = |a: &str, b: &str| a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count();
                let lcp = rest.iter().fold(chars(first), |acc, s| acc.min(common(first, s)));
                (lcp > chars(prefix)).then(|| head(first, lcp))
            }
        }
    }

    /// The single badge for a menu anchor: the direct finish one key past the prefix, or the
    /// group-level candidate that prefixes the most of `targets`' sequences. Anchors get one
    /// badge, so overlapping groups (e.g. Pictures' "NP" vs SmartArt's "NM") resolve to the
    /// dominant one.
    pub fn anchor_badge(self, prefix: &str, targets: &[&str]) -> Option<&'static str> {
        if prefix.is_empty() {
            return None; // empty prefix shows only tab badges
        }
        let all: Vec<&'static str> =
            targets.iter().flat_map(|t| self.sequences_for(t)).filter(|s| s.len() > prefix.len() && s.starts_with(prefix)).collect();
        if all.is_empty() {
            return None;
        }
        let depth = chars(prefix) + 1;
        let mut candidates: Vec<&'static str> = all.iter().filter(|s| chars(s) == depth).copied().collect();
        if candidates.is_empty() {
            for s in &all {
                let c = head(s, depth);
                if self.is_group_level(c) && !candidates.contains(&c) {
                    candidates.push(c);
                }
            }
        }
        let mut best: Option<(&'static str, usize)> = None;
        for c in candidates {
            let score = all.iter().filter(|s| s.starts_with(c)).count();
            if best.is_none_or(|(_, b)| score > b) {
                best = Some((c, score));
            }
        }
        best.map(|(c, _)| c)
    }
}

/// English [`Table::lookup`].
pub fn lookup(sequence: &str) -> Option<Entry> {
    ENGLISH.lookup(sequence)
}

/// English [`Table::has_deeper`].
pub fn has_deeper(prefix: &str) -> bool {
    ENGLISH.has_deeper(prefix)
}

/// English [`Table::tab_sequence`].
pub fn tab_sequence(tab_name: &str) -> Option<&'static str> {
    ENGLISH.tab_sequence(tab_name)
}

/// English [`Table::is_group_level`].
pub fn is_group_level(seq: &str) -> bool {
    ENGLISH.is_group_level(seq)
}

/// English [`Table::sequences_for`].
pub fn sequences_for(target: &str) -> Vec<&'static str> {
    ENGLISH.sequences_for(target)
}

/// English [`Table::group_extension`].
pub fn group_extension(prefix: &str, targets: &[&str]) -> Vec<&'static str> {
    ENGLISH.group_extension(prefix, targets)
}

/// English [`Table::item_letter`].
pub fn item_letter(prefix: &str, target: &str) -> Option<&'static str> {
    ENGLISH.item_letter(prefix, target)
}

/// English [`Table::control_badges`].
pub fn control_badges(prefix: &str, target: &str) -> Vec<&'static str> {
    ENGLISH.control_badges(prefix, target)
}

/// English [`Table::control_badge`].
pub fn control_badge(prefix: &str, target: &str) -> Option<&'static str> {
    ENGLISH.control_badge(prefix, target)
}

/// English [`Table::anchor_badge`].
pub fn anchor_badge(prefix: &str, targets: &[&str]) -> Option<&'static str> {
    ENGLISH.anchor_badge(prefix, targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_letters_exist_for_standard_tabs() {
        for (tab, letter) in [
            ("Home", "H"),
            ("Insert", "N"),
            ("Page Layout", "P"),
            ("Formulas", "M"),
            ("Data", "A"),
            ("Review", "R"),
            ("View", "W"),
            ("Draw", "JI"), // two-letter unit, shown as one badge
        ] {
            assert_eq!(tab_sequence(tab), Some(letter), "tab {tab}");
        }
        assert_eq!(tab_sequence("Automate"), None);
    }

    #[test]
    fn german_table_has_german_tab_letters_and_handles_umlauts() {
        let de = Table::for_language(Language::De);
        for (tab, letter) in
            [("Home", "R"), ("Insert", "I"), ("Draw", "JI"), ("Page Layout", "S"), ("Formulas", "O"), ("Data", "V"), ("Review", "P"), ("View", "F")]
        {
            assert_eq!(de.tab_sequence(tab), Some(letter), "tab {tab}");
        }
        assert_eq!(de.lookup("R1"), Some(Entry::Finish(Action::Command("home.bold"))));
        assert_eq!(Table::for_language(Language::Fr).lookup("H1"), Some(Entry::Finish(Action::Command("home.bold"))));
        // Ä, Ö, Ü and ß are two bytes: the helpers cut sequences by characters.
        assert_eq!(de.group_extension("R", &["edit.fillDown"]), vec!["RÜ"]);
        assert_eq!(de.item_letter("RÜ", "edit.fillDown"), Some("RÜU"));
        assert_eq!(de.anchor_badge("R", &["home.mergeCenter", "home.mergeAcross", "home.mergeCells", "home.unmergeCells"]), Some("Rß"));
        assert_eq!(de.control_badge("R", "edit.clearContents"), Some("RLÖH"));
        assert!(de.is_group_level("RLÖ"));
    }

    #[test]
    fn sequences_for_finds_command_and_dialog_targets() {
        assert_eq!(sequences_for("home.alignRight"), vec!["HAR"]);
        assert_eq!(sequences_for("dialog:pasteSpecial"), vec!["HVS", "ES"]);
        assert_eq!(sequences_for("home.autofitColumnWidth"), vec!["HOI", "OCA"]);
    }

    #[test]
    fn implicit_levels_wait_for_deeper_sequences() {
        assert!(has_deeper("J"), "JI is reachable only through the implicit J level");
        assert!(is_group_level("J"), "implicit levels are group levels");
        assert!(is_group_level("HV"), "explicit Continue levels are group levels");
        assert!(!is_group_level("H"), "a terminal tab is not a group level");
        assert!(!is_group_level("HVS"), "a finish is not a group level");
        assert!(!has_deeper("X"));
    }

    #[test]
    fn group_extension_paints_only_continue_levels() {
        assert_eq!(group_extension("H", &["dialog:pasteSpecial"]), vec!["HV"]);
        assert!(group_extension("", &["dialog:pasteSpecial"]).is_empty(), "tab letters paint on tabs, not buttons");
        assert!(group_extension("E", &["dialog:pasteSpecial"]).is_empty(), "the finish letter paints on the menu item");
    }

    #[test]
    fn item_letter_is_one_level_deep() {
        assert_eq!(item_letter("HV", "dialog:pasteSpecial"), Some("HVS"));
        assert_eq!(item_letter("HV", "edit.paste"), Some("HVP"));
        assert_eq!(item_letter("H", "dialog:pasteSpecial"), None, "the item badge waits for its group letter");
        assert_eq!(item_letter("HO", "home.autofitColumnWidth"), Some("HOI"));
    }

    #[test]
    fn control_badges_require_a_known_prefix() {
        assert!(control_badges("", "home.alignRight").is_empty(), "empty prefix shows only tab badges");
        assert_eq!(control_badges("HA", "home.alignRight"), vec!["HAR"]);
        assert!(control_badges("HZ", "home.alignRight").is_empty(), "unknown prefixes show nothing");
    }

    #[test]
    fn control_badge_shows_the_rest_or_the_group_letter() {
        assert_eq!(control_badge("H", "home.bold"), Some("H1"), "a unique sequence paints its full rest");
        assert_eq!(control_badge("H", "home.fontColor"), Some("HFC"));
        assert_eq!(control_badge("H", "home.borders"), Some("HB"), "many variants collapse to the group letter");
        assert!(control_badge("", "home.bold").is_none(), "empty prefix shows only tab badges");
        assert!(control_badge("HZ", "home.bold").is_none(), "unknown prefixes show nothing");
        assert!(control_badge("H", "view.zoom").is_none(), "diverging sequences at the next letter show nothing");
    }

    #[test]
    fn anchor_badge_resolves_to_the_dominant_group() {
        assert_eq!(anchor_badge("H", &["edit.paste", "edit.pasteSpecial", "dialog:pasteSpecial"]), Some("HV"));
        assert_eq!(anchor_badge("P", &["pageLayout.orientation"]), Some("PO"), "a direct finish paints itself");
        assert_eq!(anchor_badge("N", &["insert.picture"]), Some("NP"), "NP dominates the lone NM (SmartArt) entry");
        assert_eq!(anchor_badge("H", &["table.style"]), Some("HT"));
        assert!(anchor_badge("", &["edit.paste"]).is_none(), "empty prefix shows only tab badges");
        assert!(anchor_badge("H", &["view.zoom"]).is_none(), "no sequences under this prefix");
    }
}
