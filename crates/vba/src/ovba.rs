//! VBA project storage ([MS-OVBA]): the compression used for the `dir` and module streams, the
//! `dir` stream's project information and module records, and the `PROJECT` stream that says
//! which modules are classes, documents and UserForms.
//!
//! The compound file itself is read by the caller (the XLSX reader owns a CFB reader); this
//! module only needs a way to fetch a stream by path. Everything here treats its input as hostile:
//! sizes are checked, records are bounded, and damage gives an error or fewer modules, never a
//! panic.

use crate::VbaError;

/// Bytes a decompressed stream may grow to (VBA modules are tens of KB; 64 MB is generous).
const MAX_DECOMPRESSED: usize = 64 << 20;

/// Decompresses a [MS-OVBA] 2.4.1 compressed container.
pub fn decompress(data: &[u8]) -> Result<Vec<u8>, VbaError> {
    let (&sig, mut rest) = data.split_first().ok_or(VbaError::Format("empty compressed container"))?;
    if sig != 1 {
        return Err(VbaError::Format("bad compressed container signature"));
    }
    let mut out: Vec<u8> = Vec::with_capacity(data.len().saturating_mul(2).min(MAX_DECOMPRESSED));
    while rest.len() >= 2 {
        let header = u16::from_le_bytes([rest[0], rest[1]]);
        rest = &rest[2..];
        // Chunk size includes the header; the data holds at most 4096 decompressed bytes.
        let size = usize::from(header & 0x0FFF) + 1;
        let compressed = header & 0x8000 != 0;
        let chunk = rest.get(..size).unwrap_or(rest);
        rest = rest.get(chunk.len()..).unwrap_or(&[]);
        let start = out.len();
        if !compressed {
            out.extend_from_slice(chunk);
        } else {
            let mut i = 0usize;
            while i < chunk.len() {
                let flags = chunk[i];
                i += 1;
                for bit in 0..8 {
                    if i >= chunk.len() {
                        break;
                    }
                    if flags & (1 << bit) == 0 {
                        out.push(chunk[i]);
                        i += 1;
                    } else {
                        let Some(tok) = chunk.get(i..i + 2) else { break };
                        let token = u16::from_le_bytes([tok[0], tok[1]]);
                        i += 2;
                        let pos = out.len() - start;
                        // Bits used for the offset grow with the position in the chunk (min 4).
                        let bit_count = (usize::BITS - pos.saturating_sub(1).leading_zeros()).max(4);
                        let length_mask = 0xFFFFu16 >> bit_count;
                        let offset = usize::from(token >> (16 - bit_count)) + 1;
                        let length = usize::from(token & length_mask) + 3;
                        if offset > pos {
                            return Err(VbaError::Format("copy token points before the chunk"));
                        }
                        for _ in 0..length {
                            let b = out[out.len() - offset];
                            out.push(b);
                        }
                    }
                }
            }
        }
        if out.len() > MAX_DECOMPRESSED {
            return Err(VbaError::Format("decompressed stream too large"));
        }
    }
    Ok(out)
}

/// What kind of module a module is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleKind {
    /// A standard module (`Module1`).
    Standard,
    /// A class module.
    Class,
    /// A document module (`ThisWorkbook`, `Sheet1`).
    Document,
    /// A UserForm's code.
    Form,
}

/// One module's source.
#[derive(Clone, Debug, PartialEq)]
pub struct Module {
    pub name: String,
    /// The stream holding the module (usually the name).
    pub stream: String,
    pub kind: ModuleKind,
    /// Source text, attribute lines included.
    pub code: String,
}

/// A VBA project's modules.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Project {
    pub name: String,
    /// Windows code page the source is stored in.
    pub code_page: u16,
    pub modules: Vec<Module>,
}

impl Project {
    pub fn module(&self, name: &str) -> Option<&Module> {
        self.modules.iter().find(|m| m.name.eq_ignore_ascii_case(name))
    }
}

/// Text in a Windows code page: UTF-8 when it is valid, Windows-1252 otherwise (the common
/// case; other single-byte code pages read as their Latin-1 subset).
pub fn decode(bytes: &[u8], code_page: u16) -> String {
    if code_page == 65001
        && let Ok(s) = std::str::from_utf8(bytes)
    {
        return s.to_string();
    }
    if bytes.is_ascii() {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    if code_page != 1252
        && let Ok(s) = std::str::from_utf8(bytes)
    {
        return s.to_string();
    }
    bytes.iter().map(|&b| cp1252(b)).collect()
}

fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—',
        '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    match b {
        0x80..=0x9F => HIGH[usize::from(b - 0x80)],
        b => char::from(b),
    }
}

fn utf16(bytes: &[u8]) -> String {
    let units = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c));
    char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect()
}

/// Reads a project through `stream`, which returns a stream of the `VBA` storage's parent by
/// path (`"VBA/dir"`, `"VBA/Module1"`, `"PROJECT"`).
pub fn read_project(stream: &mut dyn FnMut(&str) -> Option<Vec<u8>>) -> Result<Project, VbaError> {
    let dir = decompress(&stream("VBA/dir").ok_or(VbaError::Format("no VBA/dir stream"))?)?;
    let mut p = Project { code_page: 1252, ..Default::default() };
    struct Pending {
        name: String,
        unicode_name: Option<String>,
        stream: String,
        offset: u32,
        procedural: bool,
    }
    let mut pending: Option<Pending> = None;
    let mut found: Vec<Pending> = Vec::new();
    let mut i = 0usize;
    while i + 6 <= dir.len() {
        let id = u16::from_le_bytes([dir[i], dir[i + 1]]);
        let size = u32::from_le_bytes([dir[i + 2], dir[i + 3], dir[i + 4], dir[i + 5]]) as usize;
        i += 6;
        // PROJECTVERSION's "size" is a reserved 4 with 6 bytes of data after it.
        let size = if id == 0x0009 { 6 } else { size };
        let Some(data) = dir.get(i..i.saturating_add(size)) else { break };
        i += size;
        match id {
            0x0003 if data.len() >= 2 => p.code_page = u16::from_le_bytes([data[0], data[1]]),
            0x0004 => p.name = decode(data, p.code_page),
            0x0019 => {
                if let Some(m) = pending.take() {
                    found.push(m);
                }
                pending = Some(Pending { name: decode(data, p.code_page), unicode_name: None, stream: String::new(), offset: 0, procedural: true });
            }
            0x0047 => {
                if let Some(m) = pending.as_mut() {
                    m.unicode_name = Some(utf16(data));
                }
            }
            0x001A => {
                if let Some(m) = pending.as_mut() {
                    m.stream = decode(data, p.code_page);
                }
            }
            0x0032 => {
                if let Some(m) = pending.as_mut() {
                    m.stream = utf16(data);
                }
            }
            0x0031 if data.len() >= 4 => {
                if let Some(m) = pending.as_mut() {
                    m.offset = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
                }
            }
            0x0021 | 0x0022 => {
                if let Some(m) = pending.as_mut() {
                    m.procedural = id == 0x0021;
                }
            }
            0x002B => {
                if let Some(m) = pending.take() {
                    found.push(m);
                }
            }
            0x0010 => break,
            _ => {}
        }
        if found.len() > 10_000 {
            return Err(VbaError::Format("too many modules"));
        }
    }
    if let Some(m) = pending.take() {
        found.push(m);
    }
    // The PROJECT stream says which non-procedural modules are documents, classes and forms.
    let project_text = stream("PROJECT").map(|b| decode(&b, p.code_page)).unwrap_or_default();
    let kind_of = |name: &str, procedural: bool| {
        if procedural {
            return ModuleKind::Standard;
        }
        for line in project_text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.split('/').next().unwrap_or(value).trim();
            if value.eq_ignore_ascii_case(name) {
                match key.trim() {
                    "Document" => return ModuleKind::Document,
                    "BaseClass" => return ModuleKind::Form,
                    "Class" => return ModuleKind::Class,
                    _ => {}
                }
            }
        }
        ModuleKind::Class
    };
    for m in found {
        let name = m.unicode_name.filter(|n| !n.is_empty()).unwrap_or(m.name);
        let stream_name = if m.stream.is_empty() { name.clone() } else { m.stream };
        let Some(raw) = stream(&format!("VBA/{stream_name}")) else { continue };
        let code = match raw.get(m.offset as usize..).map(decompress) {
            Some(Ok(b)) => decode(&b, p.code_page),
            // A module whose source can't be read is skipped, not the project.
            _ => continue,
        };
        let kind = kind_of(&name, m.procedural);
        p.modules.push(Module { name, stream: stream_name, kind, code });
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compresses as a literal-only container (valid [MS-OVBA] input for the decompressor).
    pub(crate) fn store(data: &[u8]) -> Vec<u8> {
        let mut out = vec![1u8];
        for chunk in data.chunks(3584) {
            // Compressed chunk of literal runs: a flag byte of 0 before each 8 literals.
            let mut body = Vec::new();
            for group in chunk.chunks(8) {
                body.push(0);
                body.extend_from_slice(group);
            }
            let header = 0x8000u16 | 0x3000 | (body.len() as u16 + 2 - 3);
            out.extend_from_slice(&header.to_le_bytes());
            out.extend_from_slice(&body);
        }
        out
    }

    #[test]
    fn decompresses_literals_and_copy_tokens() {
        let text = b"Attribute VB_Name = \"Module1\"\r\nSub A()\r\nEnd Sub\r\n";
        assert_eq!(decompress(&store(text)).unwrap(), text);
        // The [MS-OVBA] 3.2.3 example: "#aaabcdefaaaaghijaaaaaklaaamnopqaaaaaaaaaaaarstuvwxyzaaa".
        let compressed: [u8; 55] = [
            0x01, 0x2F, 0xB0, 0x00, 0x23, 0x61, 0x61, 0x61, 0x62, 0x63, 0x64, 0x65, 0x82, 0x66, 0x00, 0x70, 0x61, 0x67, 0x68, 0x69, 0x6A, 0x01, 0x38,
            0x08, 0x61, 0x6B, 0x6C, 0x00, 0x30, 0x6D, 0x6E, 0x6F, 0x70, 0x06, 0x71, 0x02, 0x70, 0x04, 0x10, 0x72, 0x73, 0x74, 0x75, 0x76, 0x10, 0x77,
            0x78, 0x79, 0x7A, 0x00, 0x3C, 0, 0, 0, 0,
        ];
        let out = decompress(&compressed[..51]).unwrap();
        assert_eq!(out, b"#aaabcdefaaaaghijaaaaaklaaamnopqaaaaaaaaaaaarstuvwxyzaaa");
    }

    #[test]
    fn hostile_input_is_an_error_not_a_panic() {
        assert!(decompress(&[]).is_err());
        assert!(decompress(&[2, 0, 0]).is_err());
        // A copy token at the start of a chunk points before it.
        assert!(decompress(&[1, 0x03, 0xB0, 0x01, 0x00, 0x10]).is_err());
        for seed in 0u32..2000 {
            let mut x = seed.wrapping_mul(2_654_435_761) | 1;
            let bytes: Vec<u8> = (0..64)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    x as u8
                })
                .collect();
            let mut b = bytes.clone();
            b[0] = 1;
            let _ = decompress(&b);
            let _ = read_project(&mut |_| Some(b.clone()));
        }
    }

    fn rec(id: u16, data: &[u8]) -> Vec<u8> {
        let mut v = id.to_le_bytes().to_vec();
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v.extend_from_slice(data);
        v
    }

    #[test]
    fn reads_modules_and_their_kinds() {
        let mut dir = Vec::new();
        dir.extend(rec(0x0003, &1252u16.to_le_bytes()));
        dir.extend(rec(0x0004, b"VBAProject"));
        // PROJECTVERSION: reserved 4 then 6 bytes.
        dir.extend_from_slice(&[0x09, 0x00, 4, 0, 0, 0, 1, 0, 0, 0, 2, 0]);
        dir.extend(rec(0x000F, &3u16.to_le_bytes()));
        for (name, procedural) in [("Module1", true), ("Sheet1", false), ("UserForm1", false)] {
            dir.extend(rec(0x0019, name.as_bytes()));
            dir.extend(rec(0x001A, name.as_bytes()));
            dir.extend(rec(0x0031, &0u32.to_le_bytes()));
            dir.extend(rec(if procedural { 0x0021 } else { 0x0022 }, &[]));
            dir.extend(rec(0x002B, &[]));
        }
        dir.extend(rec(0x0010, &[]));
        let project = b"ID=\"{0}\"\r\nDocument=Sheet1/&H00000000\r\nModule=Module1\r\nPackage={x}\r\nBaseClass=UserForm1\r\n".to_vec();
        let p = read_project(&mut |path| match path {
            "VBA/dir" => Some(store(&dir)),
            "PROJECT" => Some(project.clone()),
            "VBA/Module1" => Some(store(b"Sub Hello()\r\nEnd Sub\r\n")),
            "VBA/Sheet1" => Some(store(b"' sheet\r\n")),
            "VBA/UserForm1" => Some(store(b"' form\r\n")),
            _ => None,
        })
        .unwrap();
        assert_eq!(p.name, "VBAProject");
        let kinds: Vec<(&str, ModuleKind)> = p.modules.iter().map(|m| (m.name.as_str(), m.kind)).collect();
        assert_eq!(kinds, [("Module1", ModuleKind::Standard), ("Sheet1", ModuleKind::Document), ("UserForm1", ModuleKind::Form)]);
        assert!(p.module("module1").is_some_and(|m| m.code.starts_with("Sub Hello()")));
    }

    #[test]
    fn code_pages() {
        assert_eq!(decode(b"abc", 1252), "abc");
        assert_eq!(decode(&[0x80, 0xE9], 1252), "€é");
        assert_eq!(decode("ဘ".as_bytes(), 65001), "ဘ");
    }
}
