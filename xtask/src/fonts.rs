//! `cargo xtask subset-fonts <craft-fonts> <out>`: write a craft-fonts checkout in miniature,
//! holding only the faces the web build bakes (`crates/ui-egui/build.rs`) and each cut down to the
//! characters a spreadsheet UI and its documents need.
//!
//! The full Noto Sans CJK SC is ~16 MiB, over what hosts allow in a single file, so the web build
//! would otherwise ship boxes or a wasm the host rejects. `packaging/web/package.sh` runs this
//! before `trunk build` when `CRAFT_FONTS_DIR` is set, and the build script then reads the result
//! unchanged. Subsetting is HarfBuzz's (`hb-subset`, the engine behind `pyftsubset`): it keeps the
//! font's OpenType layout (GSUB/GPOS) so the browser still shapes text, and it handles the CFF
//! (`.otf`) outlines the pure-Rust subsetters do not.

#[cfg(not(feature = "fonts"))]
pub fn run(_args: &[&str]) -> Result<(), String> {
    Err("`subset-fonts` needs the `fonts` feature: cargo run -p xtask --features fonts -- subset-fonts <craft-fonts> <out>".to_string())
}

#[cfg(feature = "fonts")]
pub use imp::run;

#[cfg(feature = "fonts")]
mod imp {
    use std::fmt::Write as _;
    use std::path::{Path, PathBuf};

    use hb_subset::{Blob, FontFace, SubsetInput};

    /// Faces the web build bakes. Must match `WEB_FONTS` in `crates/ui-egui/build.rs`.
    const WEB_FONTS: &[(&str, &str)] = &[("Noto Sans CJK SC", "Regular")];

    pub fn run(args: &[&str]) -> Result<(), String> {
        let [src, dst] = args else {
            return Err("usage: cargo xtask subset-fonts <craft-fonts> <out>".to_string());
        };
        let (src, dst) = (PathBuf::from(src), PathBuf::from(dst));
        let manifest = src.join("fonts/manifest.txt");
        let text = read_text(&manifest)?;
        let codes = wanted_characters();

        let mut out_manifest = String::from(
            "# Written by `cargo xtask subset-fonts` from a full craft-fonts checkout: only the faces\n\
             # the web build bakes in, cut down to the characters a UI and its documents need.\n",
        );
        let mut kept = 0usize;
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
            let fields: Vec<&str> = line.split(" | ").map(str::trim).collect();
            let [family, style, rel, scripts, licence, licence_file, ..] = fields.as_slice() else {
                return Err(format!("malformed manifest line: {line}"));
            };
            if !WEB_FONTS.contains(&(*family, *style)) {
                continue;
            }
            let full = read(src.join(rel))?;
            let subset = subset(&full, &codes)?;
            let out_rel = subset_path(rel);
            write(&dst.join(&out_rel), &subset)?;
            // The licence travels with the font (craft-fonts ATTRIBUTION.md).
            copy(&src.join(licence_file), &dst.join(licence_file))?;
            let _ = writeln!(
                out_manifest,
                "{family} | {style} | {out_rel} | {scripts} | {licence} | {licence_file} | - | subset of the craft-fonts checkout",
            );
            println!("{family} {style}: {} -> {} bytes ({out_rel})", full.len(), subset.len());
            kept += 1;
        }
        if kept == 0 {
            return Err(format!("none of {WEB_FONTS:?} is in {}", manifest.display()));
        }
        write(&dst.join("fonts/manifest.txt"), out_manifest.as_bytes())?;
        println!("wrote {kept} subset font(s) to {}", dst.display());
        Ok(())
    }

    /// Keeps the glyphs for `codes` (HarfBuzz closes over the layout and composite glyphs), so the
    /// browser still shapes runs of those characters as the full font would.
    fn subset(bytes: &[u8], codes: &[char]) -> Result<Vec<u8>, String> {
        let blob = Blob::from_bytes(bytes).map_err(|e| format!("allocating font blob: {e}"))?;
        let face = FontFace::new(blob).map_err(|e| format!("loading font: {e}"))?;
        let mut input = SubsetInput::new().map_err(|e| format!("subset input: {e}"))?;
        {
            let mut set = input.unicode_set();
            for &c in codes {
                set.insert(c);
            }
        }
        let subset = input.subset_font(&face).map_err(|e| format!("subsetting: {e}"))?;
        Ok(subset.underlying_blob().to_vec())
    }

    /// What a Simplified-Chinese UI and its documents show: the GB2312 hanzi, plus the ASCII,
    /// punctuation, currency, maths and symbol ranges a spreadsheet renders. The same set the
    /// face was checked against before.
    fn wanted_characters() -> Vec<char> {
        let mut codes: Vec<char> = Vec::new();
        // GB2312 hanzi: 6,763 characters, the everyday Simplified-Chinese repertoire.
        for hi in 0xB0..=0xF7u8 {
            for lo in 0xA1..=0xFEu8 {
                let pair = [hi, lo];
                let (s, _, _) = encoding_rs::GBK.decode(&pair);
                if let Some(c) = s.chars().next().filter(|&c| c != '\u{FFFD}') {
                    codes.push(c);
                }
            }
        }
        codes.push('\u{3007}'); // 〇
        for (lo, hi) in [
            (0x0020u32, 0x007Eu32), // ASCII
            (0x00A0, 0x00FF),       // Latin-1
            (0x2000, 0x206F),       // general punctuation — – … “ ”
            (0x20A0, 0x20BF),       // currency symbols € ¥ £
            (0x2100, 0x214F),       // letterlike ™ № ℃
            (0x2190, 0x21FF),       // arrows → ⇧
            (0x2200, 0x22FF),       // maths ± × ÷ ≤ ≥ ≈
            (0x2460, 0x24FF),       // enclosed alphanumerics ① ⑵
            (0x25A0, 0x25FF),       // geometric shapes ▪ ● ▲
            (0x2600, 0x27BF),       // symbols and dingbats ★ ☑ ✔
            (0x3000, 0x303F),       // CJK punctuation 、。 「」
            (0x3040, 0x30FF),       // kana, so a Japanese label still reads
            (0x31C0, 0x31EF),       // CJK strokes
            (0x3200, 0x33FF),       // enclosed CJK ㊙ ㍿
            (0xFE10, 0xFE4F),       // CJK compatibility forms
            (0xFF00, 0xFFEF),       // full-width forms
        ] {
            for cp in lo..=hi {
                if let Some(c) = char::from_u32(cp) {
                    codes.push(c);
                }
            }
        }
        codes.sort_unstable();
        codes.dedup();
        codes
    }

    /// `fonts/x/Name.otf` -> `fonts/x/Name.subset.otf`.
    fn subset_path(rel: &str) -> String {
        match rel.rsplit_once('.') {
            Some((stem, ext)) => format!("{stem}.subset.{ext}"),
            None => format!("{rel}.subset"),
        }
    }

    fn read_text(path: impl AsRef<Path>) -> Result<String, String> {
        let path = path.as_ref();
        std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn read(path: impl AsRef<Path>) -> Result<Vec<u8>, String> {
        let path = path.as_ref();
        std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn copy(from: &Path, to: &Path) -> Result<(), String> {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::copy(from, to).map(|_| ()).map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))
    }
}
