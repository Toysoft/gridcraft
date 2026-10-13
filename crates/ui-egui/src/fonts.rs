//! The fonts installed on the machine, so the font list can offer them and cells can render with
//! them (#4).
//!
//! egui has no font discovery of its own and panics when asked for a family that was never bound
//! to font data. So we do both halves: scan the platform's font folders for family names (a read
//! of each font file's `name` table — damaged or unreadable files yield nothing, never a panic),
//! and bind the files a workbook actually uses into the families
//! [`theme::font_definitions`](crate::theme::font_definitions) assembles.
//!
//! Native only: on wasm there are no system fonts, every function here reports "nothing", and the
//! font list is the standard [`crate::ribbon::FONTS`] set.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::OnceLock;

/// A font file is skipped by the scan past this size: a catalog only needs each face's name, and a
/// damaged or enormous file must not turn startup into a long read.
#[cfg(not(target_arch = "wasm32"))]
// Regional CJK collections such as modern PingFang can exceed 64 MiB.
const MAX_FONT_FILE: u64 = 128 << 20;

/// Script/language coverage from font metadata, not the localized family name.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FontLanguage {
    #[default]
    All,
    TraditionalChinese,
    SimplifiedChinese,
    Japanese,
    Korean,
    Latin,
    Cyrillic,
    Greek,
    Arabic,
    Hebrew,
    Thai,
    Devanagari,
    Other,
}

impl FontLanguage {
    pub const ALL: [Self; 13] = [
        Self::All,
        Self::TraditionalChinese,
        Self::SimplifiedChinese,
        Self::Japanese,
        Self::Korean,
        Self::Latin,
        Self::Cyrillic,
        Self::Greek,
        Self::Arabic,
        Self::Hebrew,
        Self::Thai,
        Self::Devanagari,
        Self::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All languages",
            Self::TraditionalChinese => "Traditional Chinese",
            Self::SimplifiedChinese => "Simplified Chinese",
            Self::Japanese => "Japanese",
            Self::Korean => "Korean",
            Self::Latin => "Latin alphabet",
            Self::Cyrillic => "Cyrillic",
            Self::Greek => "Greek",
            Self::Arabic => "Arabic",
            Self::Hebrew => "Hebrew",
            Self::Thai => "Thai",
            Self::Devanagari => "Devanagari",
            Self::Other => "Other / Unclassified",
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn language_coverage(font: &skrifa::FontRef<'_>) -> Vec<FontLanguage> {
    use skrifa::{MetadataProvider, raw::TableProvider};
    let code_pages = font.os2().ok().and_then(|os2| os2.ul_code_page_range_1()).unwrap_or(0);
    let charmap = font.charmap();
    classify_coverage(code_pages, |character| charmap.map(character).is_some_and(|glyph| glyph.to_u32() != 0))
}

/// Coverage samples disambiguate Han scripts when older fonts omit code-page metadata.
/// This is a picker aid; it does not promise full language coverage or text shaping.
#[cfg(not(target_arch = "wasm32"))]
fn classify_coverage(code_pages: u32, has: impl Fn(char) -> bool) -> Vec<FontLanguage> {
    use FontLanguage::*;
    let probes = [
        (TraditionalChinese, 20, "繁體國學臺灣龍"),
        (SimplifiedChinese, 18, "简体国学龙汉"),
        (Japanese, 17, "あいうアイウ"),
        (Korean, 19, "가나다한글"),
        (Latin, 0, "AaZz"),
        (Cyrillic, 2, "АяЖж"),
        (Greek, 3, "ΑωΣσ"),
        (Arabic, 6, "ابتث"),
        (Hebrew, 5, "אבגד"),
        (Thai, 16, "กขคง"),
        (Devanagari, 32, "अआकख"),
    ];
    let mut languages = Vec::new();
    for (language, bit, sample) in probes {
        let mapped = sample.chars().filter(|character| has(*character)).count();
        let declared = bit < 32 && code_pages & (1_u32.checked_shl(bit).unwrap_or(0)) != 0;
        if mapped == sample.chars().count() || declared && mapped > 0 {
            languages.push(language);
        }
    }
    if languages.is_empty() {
        languages.push(Other);
    }
    languages
}

/// Filter the picker using the union of all installed faces in a family.
/// Uninstalled compatibility names have unknown coverage, so only All/Other include them.
pub fn family_supports(family: &str, language: FontLanguage) -> bool {
    if language == FontLanguage::All {
        return true;
    }
    let Some(catalog) = catalog_now() else { return language == FontLanguage::Other };
    static LANGUAGES: OnceLock<std::collections::HashMap<String, Vec<FontLanguage>>> = OnceLock::new();
    let languages = LANGUAGES.get_or_init(|| {
        let mut groups: std::collections::HashMap<String, Vec<FontLanguage>> = std::collections::HashMap::new();
        for face in catalog {
            let entry = groups.entry(face.family.to_lowercase()).or_default();
            for group in &face.languages {
                if !entry.contains(group) {
                    entry.push(*group);
                }
            }
        }
        groups
    });
    match languages.get(&family.to_lowercase()) {
        Some(groups) if language == FontLanguage::Other => groups.iter().all(|group| *group == FontLanguage::Other),
        Some(groups) => groups.contains(&language),
        None => language == FontLanguage::Other,
    }
}

/// One face of one installed font file.
#[derive(Clone, Debug)]
pub struct Face {
    /// Typographic family name, e.g. `"Segoe UI"`.
    pub family: String,
    pub style: String,
    pub full_name: String,
    pub postscript_name: String,
    pub localized_styles: Vec<(String, String)>,
    /// The file the face lives in.
    pub path: PathBuf,
    /// Face index within the file (collections hold several, e.g. a family's Regular and Bold).
    pub index: u32,
    /// Weight implied by the face's style name (400 = regular).
    pub weight: f32,
    /// Whether the style name reads as italic or oblique.
    pub italic: bool,
    /// A face can support several language/script groups.
    pub languages: Vec<FontLanguage>,
    /// Localized family names from the OpenType name table (language tag, name).
    pub localized_names: Vec<(String, String)>,
}

/// The font folders of the platform (the system's and the user's). Empty on wasm.
pub fn system_font_dirs() -> Vec<PathBuf> {
    #[cfg(target_arch = "wasm32")]
    {
        Vec::new()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let mut dirs: Vec<PathBuf> = Vec::new();
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
        if cfg!(target_os = "macos") {
            dirs.extend(
                [
                    "/System/Library/Fonts",
                    "/System/Library/Fonts/Supplemental",
                    "/Library/Fonts",
                    "/System/Library/AssetsV2/com_apple_MobileAsset_Font7",
                ]
                .map(Into::into),
            );
            if let Some(h) = &home {
                dirs.push(h.join("Library/Fonts"));
            }
        } else if cfg!(windows) {
            let root = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows"));
            dirs.push(root.join("Fonts"));
            if let Some(l) = std::env::var_os("LOCALAPPDATA") {
                dirs.push(PathBuf::from(l).join("Microsoft\\Windows\\Fonts"));
            }
        } else {
            dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts", "/usr/share/fonts/TTF"].map(Into::into));
            if let Some(h) = &home {
                dirs.push(h.join(".fonts"));
                dirs.push(h.join(".local/share/fonts"));
            }
        }
        dirs
    }
}

/// Weight implied by a style name (`"Bold"` → 700, `"Light"` → 300).
#[cfg(not(target_arch = "wasm32"))]
fn style_weight(style: &str) -> f32 {
    let s: String = style.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect();
    const TABLE: &[(&str, f32)] = &[
        ("hairline", 100.0),
        ("thin", 100.0),
        ("extralight", 200.0),
        ("ultralight", 200.0),
        ("light", 300.0),
        ("medium", 500.0),
        ("demibold", 600.0),
        ("semibold", 600.0),
        ("extrabold", 800.0),
        ("ultrabold", 800.0),
        ("bold", 700.0),
        ("black", 900.0),
        ("heavy", 900.0),
    ];
    TABLE.iter().find(|(k, _)| s.contains(k)).map(|(_, w)| *w).unwrap_or(400.0)
}

/// Whether a style name reads as italic or oblique.
#[cfg(not(target_arch = "wasm32"))]
fn style_italic(style: &str) -> bool {
    let s: String = style.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect();
    s.contains("italic") || s.contains("oblique")
}

/// Every face in `data` as `(index, family, style)`. Damaged data yields nothing, never a panic.
#[cfg(not(target_arch = "wasm32"))]
fn file_faces(data: &[u8]) -> Vec<Face> {
    use skrifa::raw::{FileRef, TableProvider};
    use skrifa::{MetadataProvider, string::StringId};
    let count = match FileRef::new(data) {
        Ok(FileRef::Font(_)) => 1,
        Ok(FileRef::Collection(c)) => c.len(),
        Err(_) => return Vec::new(),
    };
    let name = |f: &skrifa::FontRef<'_>, ids: &[StringId]| -> Option<String> {
        ids.iter().find_map(|id| f.localized_strings(*id).english_or_first().map(|s| s.to_string()).filter(|s| !s.is_empty()))
    };
    (0..count)
        .filter_map(|i| {
            let f = skrifa::FontRef::from_index(data, i).ok()?;
            let family = name(&f, &[StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME])?;
            let style = name(&f, &[StringId::TYPOGRAPHIC_SUBFAMILY_NAME, StringId::SUBFAMILY_NAME]).unwrap_or_else(|| "Regular".into());
            let mut localized_names = Vec::new();
            for id in [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME] {
                for value in f.localized_strings(id) {
                    let Some(language) = value.language() else { continue };
                    if value.to_string().trim().is_empty() {
                        continue;
                    }
                    if !localized_names.iter().any(|(tag, _)| tag == language) {
                        localized_names.push((language.to_string(), value.to_string()));
                    }
                }
            }
            let mut localized_styles = Vec::new();
            for id in [StringId::TYPOGRAPHIC_SUBFAMILY_NAME, StringId::SUBFAMILY_NAME] {
                for value in f.localized_strings(id) {
                    let Some(language) = value.language() else { continue };
                    let text = value.to_string();
                    if !text.trim().is_empty() && !localized_styles.iter().any(|(tag, _)| tag == language) {
                        localized_styles.push((language.to_string(), text));
                    }
                }
            }
            Some(Face {
                full_name: name(&f, &[StringId::FULL_NAME]).unwrap_or_else(|| family.clone()),
                postscript_name: name(&f, &[StringId::POSTSCRIPT_NAME]).unwrap_or_default(),
                family,
                weight: f
                    .os2()
                    .ok()
                    .map(|table| table.us_weight_class())
                    .filter(|weight| (1..=1000).contains(weight))
                    .map(f32::from)
                    .unwrap_or_else(|| style_weight(&style)),
                italic: style_italic(&style),
                style,
                path: PathBuf::new(),
                index: i,
                languages: language_coverage(&f),
                localized_names,
                localized_styles,
            })
        })
        .collect()
}

/// Catalog every face under `dirs`, recursing into subfolders. A folder reached twice (through a
/// symlink) is read once, so a link back to a parent ends the walk.
#[cfg(not(target_arch = "wasm32"))]
pub fn scan_dirs(dirs: &[PathBuf]) -> Vec<Face> {
    let mut out: Vec<Face> = Vec::new();
    let mut stack: Vec<PathBuf> = dirs.to_vec();
    let mut seen_dirs: HashSet<PathBuf> = HashSet::new();
    while let Some(dir) = stack.pop() {
        let key = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        if !seen_dirs.insert(key) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let is_font =
                path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc" | "otc"));
            if !is_font || std::fs::metadata(&path).map(|m| m.len() > MAX_FONT_FILE).unwrap_or(true) {
                continue;
            }
            let Ok(data) = std::fs::read(&path) else { continue };
            for mut face in file_faces(&data) {
                face.path = path.clone();
                out.push(face);
            }
        }
    }
    out
}

#[cfg(target_arch = "wasm32")]
pub fn scan_dirs(_dirs: &[PathBuf]) -> Vec<Face> {
    Vec::new()
}

static CATALOG: OnceLock<Vec<Face>> = OnceLock::new();

/// The catalog of installed faces, scanned on first use. Blocks until the scan finishes.
pub fn catalog() -> &'static [Face] {
    CATALOG.get_or_init(|| scan_dirs(&system_font_dirs()))
}

/// The catalog if the scan has finished, else `None` — never blocks, for use while painting.
fn catalog_now() -> Option<&'static [Face]> {
    CATALOG.get().map(Vec::as_slice)
}

/// Start cataloging the installed fonts on a background thread, so the first frame doesn't wait for
/// it. A no-op on wasm and after the scan has started.
pub fn start_scan() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        #[cfg(not(target_arch = "wasm32"))]
        if CATALOG.get().is_none() {
            // A failed spawn leaves the scan to the first blocking lookup.
            let _ = std::thread::Builder::new().name("font-scan".into()).spawn(|| {
                catalog();
            });
        }
    });
}

/// Whether the background scan has finished.
pub fn scan_done() -> bool {
    CATALOG.get().is_some()
}

/// The faces of one installed family, regular-weight first (the order roles pick from).
pub fn faces_of(family: &str) -> Vec<Face> {
    let Some(cat) = catalog_now() else { return Vec::new() };
    let mut v: Vec<Face> = cat.iter().filter(|f| f.family.eq_ignore_ascii_case(family)).cloned().collect();
    if v.is_empty() {
        v = cat
            .iter()
            .filter(|face| face.full_name.eq_ignore_ascii_case(family) || face.postscript_name.eq_ignore_ascii_case(family))
            .cloned()
            .collect();
    }
    v.sort_by(|a, b| a.weight.total_cmp(&b.weight).then(a.italic.cmp(&b.italic)).then(a.index.cmp(&b.index)));
    v
}

/// The subset of `names` that is installed, sorted and deduped. These are the families whose files
/// get bound for a workbook.
pub fn installed_among(names: &[String]) -> Vec<String> {
    let Some(cat) = catalog_now() else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for wanted in names {
        if !seen.insert(wanted.to_lowercase()) {
            continue;
        }
        if let Some(face) = cat.iter().find(|face| face.family.eq_ignore_ascii_case(wanted)) {
            out.push(face.family.clone());
        } else if let Some(face) =
            cat.iter().find(|face| face.full_name.eq_ignore_ascii_case(wanted) || face.postscript_name.eq_ignore_ascii_case(wanted))
        {
            out.push(if face.full_name.eq_ignore_ascii_case(wanted) { face.full_name.clone() } else { face.postscript_name.clone() });
        }
    }
    out.sort_by_key(|a| a.to_lowercase());
    out
}

/// The font list a combo box shows: the standard [`crate::ribbon::FONTS`] names first (what Excel
/// offers, and what we substitute), then every other installed family A–Z. Never blocks: until the
/// background scan finishes it returns just the standard names. Dot-prefixed system-private
/// families stay available internally but are omitted from the picker.
pub fn families() -> Vec<String> {
    let mut out: Vec<String> = crate::ribbon::FONTS.iter().map(|s| s.to_string()).collect();
    let Some(cat) = catalog_now() else { return out };
    let mut seen: HashSet<String> = out.iter().map(|s| s.to_lowercase()).collect();
    let mut installed: Vec<String> = cat.iter().filter(|f| !f.family.trim_start().starts_with('.')).map(|f| f.family.clone()).collect();
    installed.sort_by_key(|a| a.to_lowercase());
    installed.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    for f in installed {
        if seen.insert(f.to_lowercase()) {
            out.push(f);
        }
    }
    out
}

/// One picker row: display names never replace the stable workbook family identifier.
#[derive(Clone, Debug)]
pub struct FontItem {
    pub family: String,
    pub display: String,
    pub group: FontLanguage,
}

fn locale_matches(tag: &str, group: FontLanguage) -> bool {
    use FontLanguage::*;
    let tag = tag.to_ascii_lowercase();
    match group {
        TraditionalChinese => tag == "zh-tw" || tag == "zh-hk" || tag.contains("hant"),
        SimplifiedChinese => tag == "zh-cn" || tag == "zh-sg" || tag.contains("hans"),
        Japanese => tag.starts_with("ja"),
        Korean => tag.starts_with("ko"),
        Cyrillic => tag.starts_with("ru"),
        Greek => tag.starts_with("el"),
        Arabic => tag.starts_with("ar"),
        Hebrew => tag.starts_with("he"),
        Thai => tag.starts_with("th"),
        Devanagari => tag.starts_with("hi"),
        _ => tag.starts_with("en"),
    }
}

fn picker_item(family: &str, faces: &[Face]) -> FontItem {
    use FontLanguage::*;
    let supports = |group| faces.iter().any(|face| face.languages.contains(&group));
    // Region suffixes are font-family metadata (not filenames); multilingual regional
    // collections share coverage but should each appear once in their intended group.
    let region = [
        (" TC", TraditionalChinese),
        (" HK", TraditionalChinese),
        (" MO", TraditionalChinese),
        (" SC", SimplifiedChinese),
        (" GB", SimplifiedChinese),
        (" JP", Japanese),
        (" KR", Korean),
    ]
    .iter()
    .find(|(suffix, group)| family.ends_with(suffix) && supports(*group))
    .map(|(_, group)| *group);
    let names: Vec<_> = faces.iter().flat_map(|face| face.localized_names.iter()).collect();
    let native_group = [Japanese, Korean, TraditionalChinese, SimplifiedChinese, Arabic, Hebrew, Thai, Devanagari, Cyrillic, Greek]
        .into_iter()
        .find(|group| supports(*group) && names.iter().any(|(tag, name)| locale_matches(tag, *group) && !name.is_ascii()));
    let group = region.or(native_group).or_else(|| FontLanguage::ALL.into_iter().find(|group| *group != All && supports(*group))).unwrap_or(Other);
    let display = names
        .iter()
        .find(|(tag, name)| locale_matches(tag, group) && !name.trim().is_empty())
        .map(|(_, name)| name.clone())
        .unwrap_or_else(|| family.to_string());
    FontItem { family: family.to_string(), display, group }
}

/// Grouped, localized picker rows; each family appears once and hidden fonts stay hidden.
pub fn grouped_families() -> Vec<FontItem> {
    let Some(catalog) = catalog_now() else {
        return families().into_iter().map(|family| FontItem { display: family.clone(), family, group: FontLanguage::Other }).collect();
    };
    static ITEMS: OnceLock<Vec<FontItem>> = OnceLock::new();
    ITEMS
        .get_or_init(|| {
            let mut by_family: std::collections::HashMap<String, Vec<Face>> = std::collections::HashMap::new();
            for face in catalog {
                by_family.entry(face.family.to_lowercase()).or_default().push(face.clone());
            }
            let mut items: Vec<_> = families()
                .into_iter()
                .map(|family| picker_item(&family, by_family.get(&family.to_lowercase()).map(Vec::as_slice).unwrap_or_default()))
                .filter(|item| !item.family.trim().is_empty() && !item.display.trim().is_empty())
                .collect();
            let order = |group| match group {
                FontLanguage::Latin => 0,
                FontLanguage::TraditionalChinese => 1,
                FontLanguage::SimplifiedChinese => 2,
                FontLanguage::Japanese => 3,
                FontLanguage::Korean => 4,
                _ => FontLanguage::ALL.iter().position(|candidate| *candidate == group).unwrap_or(usize::MAX) + 5,
            };
            items.sort_by_key(|item| (order(item.group), item.display.to_lowercase(), item.family.to_lowercase()));
            items
        })
        .clone()
}

/// A installed face choice. Keys are UI-only; name/bold/italic are the workbook values.
#[derive(Clone, Debug)]
pub struct FontVariant {
    pub key: String,
    pub label: String,
    pub name: String,
    pub bold: bool,
    pub italic: bool,
    pub source_name: String,
}

fn variants_from_faces(family: &str, group: FontLanguage, faces: &[Face]) -> Vec<FontVariant> {
    let mut faces = faces.to_vec();
    faces.sort_by(|a, b| {
        let regular = |face: &Face| !face.italic && (face.weight - 400.0).abs() < 1.0;
        regular(b).cmp(&regular(a)).then(a.weight.total_cmp(&b.weight)).then(a.italic.cmp(&b.italic)).then(a.style.cmp(&b.style))
    });
    let all_faces = faces.clone();
    let mut binding_faces = faces.clone();
    binding_faces.sort_by(|a, b| a.weight.total_cmp(&b.weight).then(a.italic.cmp(&b.italic)).then(a.index.cmp(&b.index)));
    let mut seen = HashSet::new();
    faces
        .into_iter()
        .filter_map(|face| {
            if !seen.insert((face.style.to_lowercase(), face.full_name.to_lowercase())) {
                return None;
            }
            let standard_weight = (face.weight - 400.0).abs() < 1.0 || (face.weight - 700.0).abs() < 1.0;
            let standard = standard_weight
                && pick_face(&binding_faces, face.weight >= 600.0, face.italic)
                    .is_some_and(|default| default.path == face.path && default.index == face.index);
            let base = if face.italic {
                all_faces.iter().find(|candidate| !candidate.italic && (candidate.weight - face.weight).abs() < 1.0).unwrap_or(&face)
            } else {
                &face
            };
            let alias = if !base.full_name.eq_ignore_ascii_case(family) { base.full_name.clone() } else { base.postscript_name.clone() };
            if !standard && (alias.trim().is_empty() || alias.eq_ignore_ascii_case(family)) {
                return None;
            }
            let label = face
                .localized_styles
                .iter()
                .find(|(tag, _)| locale_matches(tag, group))
                .map(|(_, name)| name.clone())
                .unwrap_or_else(|| face.style.clone());
            Some(FontVariant {
                key: format!("font-face:{family}:{}", if face.postscript_name.is_empty() { &face.full_name } else { &face.postscript_name }),
                label,
                source_name: face.full_name,
                name: if standard { family.to_string() } else { alias },
                bold: standard && face.weight >= 600.0,
                italic: face.italic,
            })
        })
        .collect()
}

fn variant_catalog() -> Option<&'static std::collections::HashMap<String, Vec<FontVariant>>> {
    let catalog = catalog_now()?;
    static VARIANTS: OnceLock<std::collections::HashMap<String, Vec<FontVariant>>> = OnceLock::new();
    Some(VARIANTS.get_or_init(|| {
        let mut faces: std::collections::HashMap<String, Vec<Face>> = std::collections::HashMap::new();
        for face in catalog {
            faces.entry(face.family.clone()).or_default().push(face.clone());
        }
        grouped_families()
            .into_iter()
            .map(|item| {
                let variants = variants_from_faces(&item.family, item.group, faces.get(&item.family).map(Vec::as_slice).unwrap_or_default());
                (item.family, variants)
            })
            .collect()
    }))
}

pub fn variants_of(family: &str) -> Vec<FontVariant> {
    variant_catalog().and_then(|catalog| catalog.get(family)).cloned().unwrap_or_default()
}

pub fn variant_for_key(key: &str) -> Option<FontVariant> {
    variant_catalog()?.values().flatten().find(|variant| variant.key == key).cloned()
}

pub fn variant_key_for_selection(name: &str, bold: bool, italic: bool) -> Option<String> {
    variant_catalog()?
        .values()
        .flatten()
        .find(|variant| variant.name.eq_ignore_ascii_case(name) && variant.bold == bold && variant.italic == italic)
        .map(|variant| variant.key.clone())
}

pub fn is_favorite(favorites: &[String], family: &str) -> bool {
    favorites.iter().any(|name| name.eq_ignore_ascii_case(family))
}

/// Favorites first, preserving the existing native-name/script order within each section.
pub fn ordered_families(favorites: &[String]) -> Vec<FontItem> {
    let mut items = grouped_families();
    items.sort_by_key(|item| !is_favorite(favorites, &item.family));
    items
}

pub(crate) fn favorite_button(ui: &mut egui::Ui, family: &str, display: &str, favorites: &[String]) -> bool {
    let favorite = is_favorite(favorites, family);
    let language = crate::i18n::current(ui.ctx());
    let label = format!("{} {display}", language.tr(if favorite { "Remove from Favorites" } else { "Add to Favorites" }));
    let color = if favorite { crate::theme::Tokens::get(ui.ctx()).accent } else { crate::theme::Tokens::get(ui.ctx()).text_dim };
    let response = ui.add(egui::Button::new(egui::RichText::new(if favorite { "★" } else { "☆" }).color(color)).frame(false)).on_hover_text(&label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &label));
    response.clicked()
}

pub fn display_name(family: &str) -> String {
    let rows = grouped_families();
    if let Some(item) = rows.iter().find(|item| item.family.eq_ignore_ascii_case(family)) {
        return item.display.clone();
    }
    if let Some(face) = catalog_now().and_then(|catalog| {
        catalog.iter().find(|face| face.full_name.eq_ignore_ascii_case(family) || face.postscript_name.eq_ignore_ascii_case(family))
    }) && let Some(item) = rows.iter().find(|item| item.family == face.family)
    {
        let style =
            face.localized_styles.iter().find(|(tag, _)| locale_matches(tag, item.group)).map(|(_, value)| value.as_str()).unwrap_or(&face.style);
        return format!("{} {style}", item.display);
    }
    family.to_string()
}

/// The egui family name for one role of one family: `fnt:{family}:{b|n}{i|n}`.
pub fn role_key(family: &str, bold: bool, italic: bool) -> String {
    format!("fnt:{family}:{}{}", if bold { 'b' } else { 'n' }, if italic { 'i' } else { 'n' })
}

/// The face to use for a role, or `None` when the family has no faces. Missing bold/italic faces
/// fall back to the family's regular face, so every bound role has data behind it.
fn pick_face(faces: &[Face], bold: bool, italic: bool) -> Option<&Face> {
    let regular = || faces.iter().find(|f| !f.italic && (f.weight - 400.0).abs() < 1.0);
    let exact = |wanted_bold: bool, wanted_italic: bool| {
        let weight = if wanted_bold { 700.0 } else { 400.0 };
        faces
            .iter()
            .filter(|f| f.italic == wanted_italic && if wanted_bold { f.weight >= 600.0 } else { f.weight < 600.0 })
            .min_by(|a, b| (a.weight - weight).abs().total_cmp(&(b.weight - weight).abs()))
    };
    (match (bold, italic) {
        (true, true) => exact(true, true).or_else(|| exact(true, false)).or_else(|| exact(false, true)),
        (true, false) => exact(true, false),
        (false, true) => exact(false, true),
        (false, false) => None,
    })
    .or_else(regular)
    .or_else(|| faces.first())
}

/// A stable key for a face's font data, unique per file/index within the process.
fn face_key(family: &str, face: &Face) -> String {
    let file = face.path.file_name().and_then(|s| s.to_str()).unwrap_or("font");
    format!("fnt-src:{family}:{file}:{}", face.index)
}

/// Bind one installed family's faces into `fonts`, one egui family per cell role, and return the
/// role keys that are now bound. A role with no exact face chains to the family's regular face, so
/// no role is ever left pointing at missing data (egui panics on that). A family with no faces
/// binds nothing.
///
/// Each role's chain ends with egui's default proportional fonts, so glyphs the installed font
/// lacks (emoji, a stray script) still render.
pub fn bind_faces(fonts: &mut egui::FontDefinitions, family: &str, faces: &[Face]) -> Vec<String> {
    bind_faces_with_preferred(fonts, family, faces, None)
}

fn bind_faces_with_preferred(fonts: &mut egui::FontDefinitions, family: &str, faces: &[Face], preferred: Option<&Face>) -> Vec<String> {
    if faces.is_empty() {
        return Vec::new();
    }
    let base: Vec<String> = fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default();
    let mut bound: Vec<String> = Vec::new();
    for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
        let primary = if let Some(preferred) = preferred {
            let weight = if bold { preferred.weight.max(700.0) } else { preferred.weight };
            faces
                .iter()
                .filter(|face| face.italic == (preferred.italic || italic))
                .min_by(|a, b| (a.weight - weight).abs().total_cmp(&(b.weight - weight).abs()))
                .or(Some(preferred))
        } else {
            pick_face(faces, bold, italic)
        };
        let Some(primary) = primary else { continue };
        let mut chain: Vec<String> = Vec::new();
        for face in [Some(primary), pick_face(faces, false, false)] {
            let Some(face) = face else { continue };
            let key = face_key(family, face);
            if chain.contains(&key) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&face.path) else { continue };
            let mut fd = egui::FontData::from_owned(bytes);
            fd.index = face.index;
            fonts.font_data.entry(key.clone()).or_insert_with(|| std::sync::Arc::new(fd));
            chain.push(key);
        }
        if chain.is_empty() {
            continue;
        }
        chain.extend(base.iter().cloned());
        let role = role_key(family, bold, italic);
        fonts.families.insert(egui::FontFamily::Name(role.clone().into()), chain);
        bound.push(role);
    }
    bound
}

/// Bind every installed face of `family` (see [`bind_faces`]).
pub fn bind_family(fonts: &mut egui::FontDefinitions, family: &str) -> Vec<String> {
    let matches = faces_of(family);
    if let Some(preferred) = matches.first().filter(|face| !face.family.eq_ignore_ascii_case(family)) {
        bind_faces_with_preferred(fonts, family, &faces_of(&preferred.family), Some(preferred))
    } else {
        bind_faces(fonts, family, &matches)
    }
}

/// The role families currently bound to font data. `cell_family` consults this, so it never names a
/// family egui hasn't been given.
static BOUND: OnceLock<std::sync::RwLock<HashSet<String>>> = OnceLock::new();

fn bound() -> &'static std::sync::RwLock<HashSet<String>> {
    BOUND.get_or_init(|| std::sync::RwLock::new(HashSet::new()))
}

/// Record the role families a freshly built [`egui::FontDefinitions`] bound.
pub fn set_bound(keys: impl IntoIterator<Item = String>) {
    *bound().write().unwrap_or_else(|e| e.into_inner()) = keys.into_iter().collect();
}

/// Is this exact role family bound to font data?
pub fn is_bound(role: &str) -> bool {
    bound().read().unwrap_or_else(|e| e.into_inner()).contains(role)
}

/// Serializes tests that mutate the process-wide bound-family registry.
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn installed_fonts_are_selectable_and_bound_for_cells() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let catalog = catalog();
        let families = families();
        let rows = grouped_families();
        let identifiers: HashSet<_> = rows.iter().map(|row| &row.family).collect();
        assert_eq!(identifiers.len(), rows.len(), "a multilingual font appears only once");
        assert_eq!(rows.len(), families.len());
        assert!(families.iter().all(|family| !family.trim_start().starts_with('.')));
        for face in catalog {
            if face.family.trim_start().starts_with('.') {
                assert!(!families.iter().any(|family| family.eq_ignore_ascii_case(&face.family)));
                assert!(!faces_of(&face.family).is_empty(), "private fonts remain available internally");
            } else {
                assert!(families.iter().any(|family| family.eq_ignore_ascii_case(&face.family)));
            }
        }
        let Some(face) = catalog.iter().find(|face| face.family == "PingFang TC").or_else(|| catalog.first()) else { return };
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        let mut app = crate::SheetApp::new(session, Default::default());
        app.run("home.fontName", serde_json::json!({"name": face.family})).unwrap();
        let used = app.used_fonts();
        assert!(used.contains(&face.family));
        let definitions = crate::theme::font_definitions_with(crate::i18n::Language::ZhTw, &used);
        for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
            let role = role_key(&face.family, bold, italic);
            assert_eq!(crate::theme::cell_family(&face.family, bold, italic), egui::FontFamily::Name(role.clone().into()));
            let chain = &definitions.families[&egui::FontFamily::Name(role.into())];
            let data = &definitions.font_data[chain.first().unwrap()];
            let selected = skrifa::FontRef::from_index(&data.font, data.index).unwrap();
            use skrifa::{MetadataProvider, string::StringId};
            let name = selected
                .localized_strings(StringId::TYPOGRAPHIC_FAMILY_NAME)
                .english_or_first()
                .or_else(|| selected.localized_strings(StringId::FAMILY_NAME).english_or_first())
                .unwrap()
                .to_string();
            assert_eq!(name, face.family);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn metadata_classification_handles_multilingual_and_unknown_fonts() {
        use FontLanguage::*;
        let characters = "AaZz繁體國學臺灣龍简体国学龙汉あいうアイウ";
        let groups = classify_coverage(0, |character| characters.contains(character));
        assert!(groups.contains(&Latin));
        assert!(groups.contains(&TraditionalChinese));
        assert!(groups.contains(&SimplifiedChinese));
        assert!(groups.contains(&Japanese));
        assert!(!groups.contains(&Korean));
        assert_eq!(classify_coverage(u32::MAX, |_| false), vec![Other], "declarations without glyphs are not coverage");
        assert_eq!(classify_coverage(0, |character| character == '國'), vec![Other], "a stray Han symbol is not Chinese support");
        assert!(classify_coverage(1 << 20, |character| character == '國').contains(&TraditionalChinese));
        assert!(!family_supports("Definitely Uninstalled Font", TraditionalChinese));
        assert!(family_supports("Definitely Uninstalled Font", Other));
        assert!(family_supports("Definitely Uninstalled Font", All));
    }

    #[test]
    fn grouped_picker_uses_native_names_without_changing_family_ids() {
        let face = Face {
            family: "Example TC".into(),
            style: "Regular".into(),
            full_name: "Example TC Regular".into(),
            postscript_name: "ExampleTC-Regular".into(),
            localized_styles: Vec::new(),
            path: PathBuf::new(),
            index: 0,
            weight: 400.0,
            italic: false,
            languages: vec![FontLanguage::Latin, FontLanguage::TraditionalChinese, FontLanguage::SimplifiedChinese],
            localized_names: vec![("en-US".into(), "Example TC".into()), ("zh-TW".into(), "範例繁體".into()), ("zh-CN".into(), "范例繁体".into())],
        };
        let item = picker_item("Example TC", &[face]);
        assert_eq!(item.family, "Example TC");
        assert_eq!(item.display, "範例繁體");
        assert_eq!(item.group, FontLanguage::TraditionalChinese);
        let missing = picker_item("Uninstalled", &[]);
        assert_eq!(missing.display, "Uninstalled");
        assert_eq!(missing.group, FontLanguage::Other);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn face_variants_bind_exact_weight_and_survive_xlsx_roundtrip() {
        use skrifa::{MetadataProvider, string::StringId};
        let _guard = TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        catalog();
        let variants = variants_of("PingFang TC");
        let Some(light) = variants.iter().find(|variant| variant.source_name.contains("Light") && !variant.source_name.contains("Ultralight")) else {
            return;
        };
        let resolved = variant_for_key(&light.key).unwrap();
        assert_eq!(resolved.name, light.name);
        assert_ne!(light.name, "PingFang TC", "light weight needs an exact face alias");
        let definitions = crate::theme::font_definitions_with(crate::i18n::Language::ZhTw, std::slice::from_ref(&light.name));
        let role = role_key(&light.name, false, false);
        let chain = &definitions.families[&egui::FontFamily::Name(role.into())];
        let data = &definitions.font_data[chain.first().unwrap()];
        let font = skrifa::FontRef::from_index(&data.font, data.index).unwrap();
        assert_eq!(font.localized_strings(StringId::FULL_NAME).english_or_first().unwrap().to_string(), light.source_name);
        let bold_role = role_key(&light.name, true, false);
        let bold_chain = &definitions.families[&egui::FontFamily::Name(bold_role.into())];
        let bold_data = &definitions.font_data[bold_chain.first().unwrap()];
        assert_ne!(data.index, bold_data.index, "bold selects another installed face, not the same light face");
        let mut session = gridcraft_engine::Session::new();
        session.new_workbook();
        session
            .execute("home.formatCells", serde_json::json!({"style": {"font": {"name": light.name, "bold": light.bold, "italic": light.italic}}}))
            .unwrap();
        let bytes = gridcraft_xlsx::write_xlsx(&session.active().unwrap().wb).unwrap();
        let (workbook, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
        let sheet = workbook.active().unwrap();
        let style = workbook.styles.get(sheet.style_id(Default::default()));
        assert_eq!(style.font.name, light.name);
        assert_eq!(style.font.bold, light.bold);
        assert_eq!(style.font.italic, light.italic);
        assert!(!faces_of(&style.font.name).is_empty(), "reopened face resolves against local metadata");
        let regular = variants.iter().find(|variant| variant.source_name.ends_with("Regular")).unwrap();
        assert_eq!(regular.name, "PingFang TC");
        assert!(!regular.bold && !regular.italic);
        let keys: HashSet<_> = variants.iter().map(|variant| &variant.key).collect();
        assert_eq!(keys.len(), variants.len());
    }

    #[test]
    fn weighted_italic_variants_keep_an_editable_base_face() {
        let light = Face {
            family: "Example".into(),
            style: "Light".into(),
            full_name: "Example Light".into(),
            postscript_name: "Example-Light".into(),
            localized_styles: Vec::new(),
            path: PathBuf::new(),
            index: 0,
            weight: 300.0,
            italic: false,
            languages: vec![FontLanguage::Latin],
            localized_names: Vec::new(),
        };
        let mut italic = light.clone();
        italic.style = "Light Italic".into();
        italic.full_name = "Example Light Italic".into();
        italic.postscript_name = "Example-LightItalic".into();
        italic.index = 1;
        italic.italic = true;
        let variants = variants_from_faces("Example", FontLanguage::Latin, &[light, italic]);
        let choice = variants.iter().find(|variant| variant.source_name == "Example Light Italic").unwrap();
        assert_eq!(choice.name, "Example Light");
        assert!(choice.italic);
        assert!(!choice.bold);
    }

    #[test]
    fn same_weight_condensed_faces_keep_their_exact_identity() {
        let regular = Face {
            family: "Example".into(),
            style: "Regular".into(),
            full_name: "Example Regular".into(),
            postscript_name: "Example-Regular".into(),
            localized_styles: Vec::new(),
            path: PathBuf::new(),
            index: 0,
            weight: 400.0,
            italic: false,
            languages: vec![FontLanguage::Latin],
            localized_names: Vec::new(),
        };
        let mut condensed = regular.clone();
        condensed.style = "Condensed".into();
        condensed.full_name = "Example Condensed".into();
        condensed.postscript_name = "Example-Condensed".into();
        condensed.index = 1;
        let choices = variants_from_faces("Example", FontLanguage::Latin, &[regular, condensed]);
        assert_eq!(choices.iter().find(|choice| choice.source_name == "Example Regular").unwrap().name, "Example");
        assert_eq!(choices.iter().find(|choice| choice.source_name == "Example Condensed").unwrap().name, "Example Condensed");
    }

    #[test]
    fn style_names_map_to_weight_and_slant() {
        assert_eq!(style_weight("Bold"), 700.0);
        assert_eq!(style_weight("SemiBold Italic"), 600.0);
        assert_eq!(style_weight("Regular"), 400.0);
        assert_eq!(style_weight("Light Oblique"), 300.0);
        assert!(style_italic("Bold Italic") && style_italic("Oblique"));
        assert!(!style_italic("Bold"));
        // The weight table is checked in order, so "extralight" wins over "light".
        assert_eq!(style_weight("ExtraLight"), 200.0);
    }

    #[test]
    fn role_keys_are_stable_and_distinct() {
        assert_eq!(role_key("Segoe UI", false, false), "fnt:Segoe UI:nn");
        assert_eq!(role_key("Segoe UI", true, false), "fnt:Segoe UI:bn");
        assert_eq!(role_key("Segoe UI", false, true), "fnt:Segoe UI:ni");
        assert_eq!(role_key("Segoe UI", true, true), "fnt:Segoe UI:bi");
    }

    #[test]
    fn nothing_is_bound_until_font_definitions_says_so() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // A role key is only safe to name after font_definitions bound it.
        let k = role_key("Some Installed Font", true, false);
        assert!(!is_bound(&k));
        set_bound([k.clone()]);
        assert!(is_bound(&k));
        set_bound([]);
        assert!(!is_bound(&k));
    }

    #[test]
    fn damaged_or_non_font_files_contribute_nothing() {
        assert!(file_faces(b"ttcf\x00\x01\x00\x00\xff\xff\xff\xff").is_empty());
        assert!(file_faces(b"not a font at all").is_empty());
        assert!(file_faces(&[]).is_empty());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_scan_catalogs_fonts_and_reads_each_folder_once() {
        let dir = std::env::temp_dir().join(format!("gridcraft-fonts-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Sub")).expect("temp dir");
        std::fs::write(dir.join("Ubuntu-Light.ttf"), epaint_default_fonts::UBUNTU_LIGHT).expect("write");
        std::fs::write(dir.join("damaged.ttf"), b"ttcf\x00\x01\x00\x00\xff\xff\xff\xff").expect("write");
        std::fs::write(dir.join("readme.txt"), b"not a font").expect("write");
        // The same folder listed twice (once through `Sub/..`) is read once.
        let faces = scan_dirs(&[dir.clone(), dir.join("Sub/..")]);
        let families: Vec<&str> = faces.iter().map(|f| f.family.as_str()).collect();
        assert_eq!(families, ["Ubuntu"], "one face, family read from the name table");
        assert_eq!(faces[0].weight, 300.0, "“Light” reads as weight 300");
        assert!(!faces[0].italic);
        assert!(faces[0].languages.contains(&FontLanguage::Latin));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_back_to_a_parent_folder_ends_the_scan() {
        let dir = std::env::temp_dir().join(format!("gridcraft-fonts-loop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Sub")).expect("temp dir");
        std::fs::write(dir.join("Sub/Ubuntu-Light.ttf"), epaint_default_fonts::UBUNTU_LIGHT).expect("write");
        std::os::unix::fs::symlink(&dir, dir.join("Sub/loop")).expect("symlink");
        assert_eq!(scan_dirs(std::slice::from_ref(&dir)).len(), 1, "the loop doesn't multiply faces");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn binding_a_family_binds_all_four_roles_and_never_stores_missing_data() {
        let dir = std::env::temp_dir().join(format!("gridcraft-fonts-bind-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("Ubuntu-Light.ttf");
        std::fs::write(&path, epaint_default_fonts::UBUNTU_LIGHT).expect("write");
        let face = Face {
            family: "Ubuntu".into(),
            style: "Light".into(),
            full_name: "Ubuntu Light".into(),
            postscript_name: "Ubuntu-Light".into(),
            localized_styles: Vec::new(),
            path,
            index: 0,
            weight: 300.0,
            italic: false,
            languages: vec![FontLanguage::Latin],
            localized_names: Vec::new(),
        };

        let mut fonts = egui::FontDefinitions::default();
        let bound = bind_faces(&mut fonts, "Ubuntu", std::slice::from_ref(&face));
        assert_eq!(bound.len(), 4, "regular, bold, italic and bold-italic roles");
        for key in &bound {
            let chain = fonts.families.get(&egui::FontFamily::Name(key.as_str().into())).expect("role family");
            assert!(chain.iter().all(|k| fonts.font_data.contains_key(k)), "every chained key has data");
        }
        // A family with no faces binds nothing and never touches the map.
        assert!(bind_faces(&mut fonts, "No Such Font", &[]).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
