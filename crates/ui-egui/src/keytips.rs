//! Sequential ribbon shortcuts. The host paints hints from `prefix` and runs returned actions.
//!
//! The sequence table lives in [`keytip_table`]: the match below only walks the prefix.

use egui::{Context, Event, Key, Modifiers, os::OperatingSystem};

pub mod keytip_table;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Tab(&'static str),
    Command(&'static str),
    Dialog(&'static str),
    /// The target command does not exist yet: the sequence is reserved, its badge shows, and
    /// invoking it is a silent no-op until the command lands (see keytips.json `pending`).
    Pending(&'static str),
    /// Route the keytip to the matching ribbon control: open its dropdown or run its click
    /// handler (which carries the right params), instead of firing the engine command with
    /// empty params.
    Control(&'static str),
}

#[derive(Default)]
pub struct KeyTips {
    prefix: Option<String>,
    alt_armed: bool,
    // Keep consuming a held mnemonic after its action closes keytips.
    held: Vec<Key>,
    /// The interface language of the last processed frame: its key tips are the ones in use.
    language: crate::i18n::Language,
}

impl KeyTips {
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    /// The key tips in use: the interface language's (German Excel's Home tab is Alt, R).
    pub fn table(&self) -> keytip_table::Table {
        keytip_table::Table::for_language(self.language)
    }

    pub fn clear(&mut self) {
        self.prefix = None;
        self.alt_armed = false;
        self.held.clear();
    }

    fn toggle(&mut self) {
        self.prefix = if self.prefix.is_some() { None } else { Some(String::new()) };
        self.alt_armed = false;
    }

    /// Remove only handled events before any text editors or the grid see them.
    pub fn process(&mut self, ctx: &Context, enabled: bool) -> Vec<Action> {
        if !enabled || !ctx.input(|i| i.focused) {
            self.clear();
            return Vec::new();
        }
        let mac = ctx.os() == OperatingSystem::Mac;
        let language = crate::i18n::current(ctx);
        if language != self.language {
            // Letters typed so far belong to the other language's table.
            self.prefix = None;
            self.language = language;
        }
        let table = self.table();
        let events = ctx.input_mut(|i| std::mem::take(&mut i.events));
        let pairs = text_pairs(&events);
        let mut consumed = std::collections::HashSet::new();
        let mut actions = Vec::new();
        for (index, event) in events.iter().enumerate() {
            let mut handled = false;
            match event {
                Event::PointerButton { pressed: true, .. } | Event::MouseWheel { .. } | Event::WindowFocused(false) | Event::Ime(_) => self.clear(),
                Event::Copy | Event::Cut | Event::Paste(_) => self.clear(),
                Event::ModifiersChanged(m) => {
                    if command_modifier(*m) || (mac && m.alt) {
                        self.clear();
                    } else if m.shift {
                        self.alt_armed = false;
                    }
                }
                Event::Key { key: Key::AltLeft, pressed, repeat, modifiers, .. } if !mac && !command_modifier(*modifiers) && !modifiers.shift => {
                    if *pressed {
                        if !repeat {
                            self.alt_armed = true;
                        }
                        handled = true;
                    } else if self.alt_armed {
                        self.toggle();
                        handled = true;
                    }
                }
                Event::Key { key, pressed, repeat, modifiers, .. } => {
                    self.alt_armed = false;
                    if command_modifier(*modifiers)
                        || modifiers.alt
                        || matches!(key, Key::AltLeft | Key::AltRight | Key::ControlLeft | Key::ControlRight | Key::SuperLeft | Key::SuperRight)
                    {
                        self.clear();
                    } else if !pressed {
                        handled = self.held.contains(key);
                        self.held.retain(|held| held != key);
                    } else if *repeat && (self.prefix.is_some() || self.held.contains(key)) {
                        handled = true;
                    } else if *key == Key::F10 && modifiers.is_none() {
                        self.toggle();
                        handled = true;
                    } else if self.prefix.is_some() && !matches!(key, Key::ShiftLeft | Key::ShiftRight) {
                        handled = true;
                        if *key == Key::Escape {
                            // Escape pops one level; at a single-letter level it
                            // returns to the armed root (empty prefix).
                            if let Some(prefix) = self.prefix.as_mut() {
                                if prefix.is_empty() {
                                    self.prefix = None;
                                } else {
                                    prefix.pop();
                                }
                            }
                        } else if let Some(unit) = typed_unit(*key, &events, &pairs, index) {
                            let next = format!("{}{unit}", self.prefix.as_deref().unwrap_or_default());
                            match table.lookup(&next) {
                                Some(keytip_table::Entry::Tab(tab)) => {
                                    // A tab with deeper sequences stays as an active level
                                    // (its group badges show); a terminal unit like "JI"
                                    // finishes and clears the prefix.
                                    self.prefix = table.has_deeper(&next).then_some(next);
                                    actions.push(Action::Tab(tab));
                                }
                                Some(keytip_table::Entry::Continue) => self.prefix = Some(next),
                                Some(keytip_table::Entry::Finish(action)) => self.finish(action, &mut actions),
                                // No complete sequence, but deeper ones start here: wait for
                                // more input (Excel's two-letter units like Draw's "JI").
                                None if table.has_deeper(&next) => self.prefix = Some(next),
                                // Unknown letters are consumed while a prefix is active but
                                // leave it unchanged, as Excel does.
                                None => {}
                            }
                        }
                    }
                    if handled && *pressed && !self.held.contains(key) {
                        self.held.push(*key);
                    }
                }
                Event::Text(_) => {
                    self.alt_armed = false;
                    // Paired text is decided with its key, which may occur later on web.
                    handled = self.prefix.is_some() && !pairs.iter().any(|(_, text)| *text == index);
                }
                _ => {}
            }
            if handled {
                consumed.insert(index);
                if let Some((_, text)) = pairs.iter().find(|(key, _)| *key == index) {
                    consumed.insert(*text);
                }
            }
        }
        ctx.input_mut(|i| {
            i.events = events.into_iter().enumerate().filter_map(|(index, event)| (!consumed.contains(&index)).then_some(event)).collect()
        });
        actions
    }

    fn finish(&mut self, action: Action, actions: &mut Vec<Action>) {
        self.prefix = None;
        actions.push(action);
    }
}

fn command_modifier(m: Modifiers) -> bool {
    m.ctrl || m.command || m.mac_cmd
}

/// A letter with no egui key of its own (German Ä, Ö, Ü, ß): egui reports the key's position,
/// and the layout's character arrives as text. Key tips show it in capitals, except ß.
fn local_letter(text: &str) -> Option<String> {
    let mut it = text.chars();
    let c = it.next()?;
    if it.next().is_some() || c.is_ascii() || !c.is_alphabetic() {
        return None;
    }
    Some(if c == 'ß' { c.to_string() } else { c.to_uppercase().collect() })
}

/// The key tip unit a key press types: a letter or digit by its key, or a local letter by the
/// text paired with the press.
fn typed_unit(key: Key, events: &[Event], pairs: &[(usize, usize)], index: usize) -> Option<String> {
    let text = pairs.iter().find(|(k, _)| *k == index).and_then(|(_, t)| match events.get(*t) {
        Some(Event::Text(text)) => local_letter(text),
        _ => None,
    });
    text.or_else(|| (key.name().len() == 1).then(|| key.name().to_string()))
}

/// Native integrations emit Key then Text; browsers can emit Text then Key.
fn text_pairs(events: &[Event]) -> Vec<(usize, usize)> {
    let mut pairs = Vec::new();
    for (text_index, event) in events.iter().enumerate() {
        let Event::Text(text) = event else { continue };
        let before = events.iter().enumerate().take(text_index).rev().find(|(_, e)| !matches!(e, Event::ModifiersChanged(_)));
        let after = events.iter().enumerate().skip(text_index + 1).find(|(_, e)| !matches!(e, Event::ModifiersChanged(_)));
        for (key_index, event) in before.into_iter().chain(after) {
            if let Event::Key { key, pressed: true, modifiers, .. } = event
                && (key.name().len() == 1 || modifiers.alt || command_modifier(*modifiers) || local_letter(text).is_some())
                && !pairs.iter().any(|(paired, _)| *paired == key_index)
            {
                pairs.push((key_index, text_index));
                break;
            }
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
    }

    #[test]
    fn active_mode_preserves_modified_text_before_key() {
        for (os, letter, text, modifiers) in [
            (OperatingSystem::Windows, Key::E, "€", Modifiers::CTRL | Modifiers::ALT),
            (OperatingSystem::Nix, Key::E, "€", Modifiers::ALT),
            (OperatingSystem::Mac, Key::O, "œ", Modifiers::ALT),
            (OperatingSystem::Mac, Key::C, "c", Modifiers::COMMAND),
        ] {
            let ctx = Context::default();
            ctx.set_os(os);
            let mut tips = KeyTips { prefix: Some("H".into()), ..Default::default() };
            let events = vec![Event::Text(text.into()), key(letter, true, modifiers)];
            let mut remaining = Vec::new();
            let mut output = ctx.run_ui(egui::RawInput { events: events.clone(), ..Default::default() }, |ui| {
                assert!(tips.process(ui.ctx(), true).is_empty());
                remaining = ui.input(|i| i.events.clone());
            });
            output.textures_delta.clear();
            assert_eq!(tips.prefix(), None);
            assert_eq!(remaining, events);
        }
    }

    #[test]
    fn shift_between_alt_press_and_release_does_not_activate() {
        let ctx = Context::default();
        ctx.set_os(OperatingSystem::Windows);
        let mut tips = KeyTips::default();
        let events =
            vec![key(Key::AltLeft, true, Modifiers::ALT), key(Key::ShiftLeft, true, Modifiers::NONE), key(Key::AltLeft, false, Modifiers::NONE)];
        let mut output = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
            assert!(tips.process(ui.ctx(), true).is_empty());
        });
        output.textures_delta.clear();
        assert_eq!(tips.prefix(), None);
    }
}
