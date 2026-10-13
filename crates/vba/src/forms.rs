//! UserForm designs ([MS-OFORMS]): the form's caption, size, colours and font, and each control's
//! kind, name, position, size, caption, value, colours and font, read from a form storage's
//! `f` (form and site table) and `o` (control data) streams and its `\x03VBFrame` text.
//!
//! Positions and sizes are in points. Unknown controls keep their name, kind index and bounds;
//! damaged data gives fewer controls, never a panic.

use crate::ovba::decode;

/// What a control is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    Label,
    TextBox,
    ListBox,
    ComboBox,
    CheckBox,
    OptionButton,
    ToggleButton,
    CommandButton,
    SpinButton,
    ScrollBar,
    Frame,
    Image,
    MultiPage,
    TabStrip,
    Other(u16),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FontSpec {
    pub name: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ControlDesign {
    pub name: String,
    pub kind: ControlKind,
    pub id: i32,
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    pub tab_index: i32,
    pub visible: bool,
    pub tab_stop: bool,
    pub enabled: bool,
    pub locked: bool,
    pub multiline: bool,
    pub word_wrap: bool,
    pub caption: String,
    pub value: String,
    /// 0xRRGGBB.
    pub fore: Option<u32>,
    pub back: Option<u32>,
    /// Transparent background (`BackStyle = fmBackStyleTransparent`).
    pub transparent: bool,
    pub font: Option<FontSpec>,
    /// Horizontal text alignment: 1 left, 2 right, 3 centre.
    pub align: u8,
    pub password_char: Option<char>,
    pub max_length: u32,
    pub column_count: i32,
    /// ComboBox style: true for a drop-down list (no typing).
    pub drop_list: bool,
    pub row_source: String,
    pub control_source: String,
    pub min: i32,
    pub max: i32,
    pub position: i32,
    pub small_change: i32,
    pub default: bool,
    pub cancel: bool,
    pub tip: String,
}

impl ControlDesign {
    fn new(name: String, kind: ControlKind) -> ControlDesign {
        ControlDesign {
            name,
            kind,
            id: 0,
            left: 0.0,
            top: 0.0,
            width: 72.0,
            height: 18.0,
            tab_index: 0,
            visible: true,
            tab_stop: true,
            enabled: true,
            locked: false,
            multiline: false,
            word_wrap: true,
            caption: String::new(),
            value: String::new(),
            fore: None,
            back: None,
            transparent: false,
            font: None,
            align: 1,
            password_char: None,
            max_length: 0,
            column_count: 1,
            drop_list: false,
            row_source: String::new(),
            control_source: String::new(),
            min: 0,
            max: 100,
            position: 0,
            small_change: 1,
            default: false,
            cancel: false,
            tip: String::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FormDesign {
    pub name: String,
    pub caption: String,
    /// Client area, points.
    pub width: f32,
    pub height: f32,
    pub back: Option<u32>,
    pub fore: Option<u32>,
    pub font: Option<FontSpec>,
    /// `StartUpPosition`: 0 manual, 1 centre owner, 2 centre screen, 3 Windows default.
    pub start_position: u8,
    pub controls: Vec<ControlDesign>,
}

/// HIMETRIC (0.01 mm) to points.
fn himetric(v: i32) -> f32 {
    (v as f32 * 72.0 / 2540.0).clamp(-100_000.0, 100_000.0)
}

/// An OLE colour as RGB: system colours (high bit) map to classic Windows defaults.
fn ole_color(v: u32) -> u32 {
    if v & 0x8000_0000 != 0 {
        return match v & 0xFF {
            0x05 => 0xFF_FFFF,                      // window
            0x08 | 0x12 | 0x07 | 0x09 => 0x00_0000, // window text, button text, menu text, caption text
            0x0F | 0x16 | 0x04 => 0xF0_F0F0,        // button face, 3D light, menu
            0x10 => 0xA0_A0A0,                      // button shadow
            0x11 => 0x6D_6D6D,                      // grey text
            0x0D => 0x00_78D7,                      // highlight
            0x0E => 0xFF_FFFF,                      // highlight text
            0x06 => 0x64_6464,                      // window frame
            0x14 => 0xFF_FFFF,                      // 3D highlight
            0x15 => 0x69_6969,                      // 3D dark shadow
            _ => 0xF0_F0F0,
        };
    }
    // 0x00BBGGRR → 0xRRGGBB.
    ((v & 0xFF) << 16) | (v & 0xFF00) | ((v >> 16) & 0xFF)
}

/// A bounds-checked little-endian reader; reads past the end give zeros and set `bad`.
struct Rd<'a> {
    d: &'a [u8],
    pos: usize,
    /// Start of the current data block (fields align to their size from here).
    base: usize,
    bad: bool,
}

impl<'a> Rd<'a> {
    fn new(d: &'a [u8], pos: usize) -> Rd<'a> {
        Rd { d, pos, base: pos, bad: false }
    }
    fn align(&mut self, n: usize) {
        let off = self.pos.saturating_sub(self.base);
        let r = off % n;
        if r != 0 {
            self.pos += n - r;
        }
    }
    fn bytes(&mut self, n: usize) -> &'a [u8] {
        match self.d.get(self.pos..self.pos.saturating_add(n)) {
            Some(b) => {
                self.pos += n;
                b
            }
            None => {
                self.bad = true;
                self.pos = self.d.len();
                &[]
            }
        }
    }
    fn u8(&mut self) -> u8 {
        self.bytes(1).first().copied().unwrap_or(0)
    }
    fn u16(&mut self) -> u16 {
        self.align(2);
        let b = self.bytes(2);
        if b.len() == 2 { u16::from_le_bytes([b[0], b[1]]) } else { 0 }
    }
    fn u32(&mut self) -> u32 {
        self.align(4);
        let b = self.bytes(4);
        if b.len() == 4 { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { 0 }
    }
    fn i32(&mut self) -> i32 {
        self.u32() as i32
    }
    /// Unaligned reads (outside data blocks).
    fn raw_u16(&mut self) -> u16 {
        let b = self.bytes(2);
        if b.len() == 2 { u16::from_le_bytes([b[0], b[1]]) } else { 0 }
    }
    fn raw_u32(&mut self) -> u32 {
        let b = self.bytes(4);
        if b.len() == 4 { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { 0 }
    }
    /// A string whose length-and-compression word was read earlier, padded to 4 bytes.
    fn string(&mut self, len_word: u32, code_page: u16) -> String {
        let compressed = len_word & 0x8000_0000 != 0;
        let n = (len_word & 0x7FFF_FFFF) as usize;
        if n > 1 << 20 {
            self.bad = true;
            return String::new();
        }
        self.align(4);
        let b = self.bytes(n);
        let s = if compressed {
            decode(b, code_page)
        } else {
            let units = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c));
            char::decode_utf16(units).map(|r| r.unwrap_or('\u{FFFD}')).collect()
        };
        self.align(4);
        s
    }
}

/// `StdFont` or `TextProps` after a GUID (a form's font).
fn guid_and_font(r: &mut Rd<'_>, code_page: u16) -> Option<FontSpec> {
    let guid = r.bytes(16);
    // {0BE35203-8F91-11CE-9DE3-00AA004BB851}: StdFont.
    if guid.first() == Some(&0x03) && guid.get(1) == Some(&0x52) {
        let _version = r.u8();
        let _charset = r.raw_u16();
        let flags = r.u8();
        let weight = r.raw_u16();
        let height = r.raw_u32();
        let len = usize::from(r.u8());
        let name = decode(r.bytes(len), code_page);
        return Some(FontSpec { name, size: height as f32 / 10_000.0, bold: weight >= 600, italic: flags & 2 != 0, underline: flags & 4 != 0 });
    }
    text_props(r, code_page)
}

/// A `TextProps` block (control fonts).
fn text_props(r: &mut Rd<'_>, code_page: u16) -> Option<FontSpec> {
    let start = r.pos;
    let _minor = r.u8();
    let _major = r.u8();
    let cb = usize::from(r.raw_u16());
    let end = r.pos + cb;
    let mask = r.raw_u32();
    r.base = r.pos;
    let name_len = if mask & 1 != 0 { r.u32() } else { 0 };
    let effects = if mask & 2 != 0 { r.u32() } else { 0 };
    let height = if mask & 4 != 0 { r.u32() } else { 160 };
    if mask & 16 != 0 {
        r.u8();
    }
    if mask & 32 != 0 {
        r.u8();
    }
    let align = if mask & 64 != 0 { r.u8() } else { 1 };
    let weight = if mask & 128 != 0 { r.u16() } else { 400 };
    r.align(4);
    r.base = r.pos;
    let name = if mask & 1 != 0 { r.string(name_len, code_page) } else { String::new() };
    r.pos = end.max(start);
    let _ = align;
    Some(FontSpec {
        name,
        size: height as f32 / 20.0,
        bold: effects & 1 != 0 || weight >= 600,
        italic: effects & 2 != 0,
        underline: effects & 4 != 0,
    })
}

/// Skips a `GuidAndPicture` (an icon or picture we don't draw).
fn skip_picture(r: &mut Rd<'_>) {
    r.bytes(16);
    let _preamble = r.raw_u32();
    let size = r.raw_u32() as usize;
    r.bytes(size.min(64 << 20));
}

/// Parses a form from its streams: `vb_frame` (text), `f` and `o`.
pub fn read_form(name: &str, vb_frame: &[u8], f: &[u8], o: &[u8], code_page: u16) -> FormDesign {
    let mut form = FormDesign { name: name.to_string(), ..Default::default() };
    // The text part: caption, client size (twips), start position.
    for line in decode(vb_frame, code_page).lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let v = v.trim();
        let num = || v.split_whitespace().next().and_then(|n| n.parse::<f32>().ok());
        match k.trim() {
            "Caption" => form.caption = v.trim_matches('"').replace("\"\"", "\""),
            "ClientWidth" => form.width = num().unwrap_or(0.0) / 20.0,
            "ClientHeight" => form.height = num().unwrap_or(0.0) / 20.0,
            "StartUpPosition" => form.start_position = num().unwrap_or(1.0) as u8,
            _ => {}
        }
    }
    let mut r = Rd::new(f, 0);
    let _minor = r.u8();
    let _major = r.u8();
    let cb = usize::from(r.raw_u16());
    let mask = r.raw_u32();
    r.base = r.pos;
    let mut bool_props = 0x4u32;
    if mask & (1 << 1) != 0 {
        form.back = Some(ole_color(r.u32()));
    }
    if mask & (1 << 2) != 0 {
        form.fore = Some(ole_color(r.u32()));
    }
    if mask & (1 << 3) != 0 {
        r.u32();
    }
    if mask & (1 << 6) != 0 {
        bool_props = r.u32();
    }
    if mask & (1 << 7) != 0 {
        r.u8();
    }
    if mask & (1 << 8) != 0 {
        r.u8();
    }
    if mask & (1 << 9) != 0 {
        r.u8();
    }
    if mask & (1 << 13) != 0 {
        r.u32();
    }
    let mouse_icon = if mask & (1 << 15) != 0 { r.u16() } else { 0 };
    if mask & (1 << 16) != 0 {
        r.u8();
    }
    if mask & (1 << 17) != 0 {
        r.u8();
    }
    if mask & (1 << 18) != 0 {
        r.u32();
    }
    let caption_len = if mask & (1 << 19) != 0 { r.u32() } else { 0 };
    let font = if mask & (1 << 20) != 0 { r.u16() } else { 0 };
    let picture = if mask & (1 << 21) != 0 { r.u16() } else { 0 };
    if mask & (1 << 22) != 0 {
        r.u32();
    }
    if mask & (1 << 23) != 0 {
        r.u8();
    }
    if mask & (1 << 25) != 0 {
        r.u8();
    }
    if mask & (1 << 26) != 0 {
        r.u32();
    }
    if mask & (1 << 27) != 0 {
        r.u32();
    }
    // Extra data block.
    r.align(4);
    r.base = r.pos;
    if mask & (1 << 10) != 0 {
        let (w, h) = (r.i32(), r.i32());
        if form.width == 0.0 {
            form.width = himetric(w);
            form.height = himetric(h);
        }
    }
    if mask & (1 << 11) != 0 {
        r.i32();
        r.i32();
    }
    if mask & (1 << 12) != 0 {
        r.i32();
        r.i32();
    }
    if mask & (1 << 19) != 0 {
        let c = r.string(caption_len, code_page);
        if form.caption.is_empty() {
            form.caption = c;
        }
    }
    // Stream data.
    r.pos = r.pos.max(4 + cb).min(f.len());
    if mask & (1 << 15) != 0 && mouse_icon == 0xFFFF {
        skip_picture(&mut r);
    }
    if mask & (1 << 20) != 0 && font == 0xFFFF {
        form.font = guid_and_font(&mut r, code_page);
    }
    if mask & (1 << 21) != 0 && picture == 0xFFFF {
        skip_picture(&mut r);
    }
    // Site data: the class table, then the sites.
    if bool_props & 0x8000 == 0 {
        let classes = r.raw_u16();
        for _ in 0..classes.min(1000) {
            let _v = r.raw_u16();
            let cb = usize::from(r.raw_u16());
            r.bytes(cb);
        }
    }
    let count = r.raw_u32().min(10_000) as usize;
    let _bytes = r.raw_u32();
    // Depths and types, run-length coded, padded to 4 bytes from their start.
    r.base = r.pos;
    let mut seen = 0usize;
    while seen < count && !r.bad {
        let _depth = r.u8();
        let t = r.u8();
        if t & 0x80 != 0 {
            let _ty = r.u8();
            seen += usize::from(t & 0x7F);
        } else {
            seen += 1;
        }
    }
    r.align(4);
    let mut sites: Vec<(ControlDesign, u32)> = Vec::new();
    for _ in 0..count {
        if r.bad {
            break;
        }
        let start = r.pos;
        let _version = r.raw_u16();
        let cb = usize::from(r.raw_u16());
        let end = start + 4 + cb;
        let m = r.raw_u32();
        r.base = r.pos;
        let name_len = if m & 1 != 0 { r.u32() } else { 0 };
        let tag_len = if m & 2 != 0 { r.u32() } else { 0 };
        let id = if m & 4 != 0 { r.i32() } else { 0 };
        if m & 8 != 0 {
            r.u32();
        }
        let bit_flags = if m & 16 != 0 { r.u32() } else { 0x33 };
        let stream_size = if m & 32 != 0 { r.u32() } else { 0 };
        let tab_index = if m & 64 != 0 { r.u16() as i16 as i32 } else { 0 };
        let clsid = if m & 128 != 0 { r.u16() } else { 0x7FFF };
        if m & 512 != 0 {
            r.u16();
        }
        let tip_len = if m & 2048 != 0 { r.u32() } else { 0 };
        let lic_len = if m & 4096 != 0 { r.u32() } else { 0 };
        let src_len = if m & 8192 != 0 { r.u32() } else { 0 };
        let row_len = if m & 16384 != 0 { r.u32() } else { 0 };
        r.align(4);
        r.base = r.pos;
        let name = if m & 1 != 0 { r.string(name_len, code_page) } else { String::new() };
        if m & 2 != 0 {
            r.string(tag_len, code_page);
        }
        let (left, top) = if m & 256 != 0 { (r.i32(), r.i32()) } else { (0, 0) };
        let tip = if m & 2048 != 0 { r.string(tip_len, code_page) } else { String::new() };
        if m & 4096 != 0 {
            r.string(lic_len, code_page);
        }
        let control_source = if m & 8192 != 0 { r.string(src_len, code_page) } else { String::new() };
        let row_source = if m & 16384 != 0 { r.string(row_len, code_page) } else { String::new() };
        let kind = match clsid & 0x7FFF {
            12 => ControlKind::Image,
            14 => ControlKind::Frame,
            16 => ControlKind::SpinButton,
            17 => ControlKind::CommandButton,
            18 => ControlKind::TabStrip,
            21 => ControlKind::Label,
            23 => ControlKind::TextBox,
            24 => ControlKind::ListBox,
            25 => ControlKind::ComboBox,
            26 => ControlKind::CheckBox,
            27 => ControlKind::OptionButton,
            28 => ControlKind::ToggleButton,
            47 => ControlKind::ScrollBar,
            57 => ControlKind::MultiPage,
            other => ControlKind::Other(other),
        };
        let mut c = ControlDesign::new(name, kind);
        c.id = id;
        c.left = himetric(left);
        c.top = himetric(top);
        c.tab_index = tab_index;
        c.tab_stop = bit_flags & 1 != 0;
        c.visible = bit_flags & 2 != 0;
        c.default = bit_flags & 4 != 0;
        c.cancel = bit_flags & 8 != 0;
        c.tip = tip;
        c.control_source = control_source;
        c.row_source = row_source;
        sites.push((c, stream_size));
        r.pos = end.min(f.len());
        if end > f.len() {
            break;
        }
    }
    // Control data, in site order.
    let mut off = 0usize;
    for (mut c, size) in sites {
        let size = size as usize;
        if let Some(block) = o.get(off..off.saturating_add(size)) {
            read_control(&mut c, block, code_page);
        }
        off = off.saturating_add(size);
        form.controls.push(c);
    }
    form
}

/// An ActiveX control placed on a worksheet (`xl/activeX/activeXN.bin`): its class id, then the
/// control's persisted properties in the same format UserForms use. `None` for other classes.
pub fn read_activex(name: &str, bin: &[u8], code_page: u16) -> Option<ControlDesign> {
    let clsid = bin.get(..16)?;
    let d1 = u32::from_le_bytes([clsid[0], clsid[1], clsid[2], clsid[3]]);
    let kind = match d1 {
        0x8BD2_1D10 => ControlKind::TextBox,
        0x8BD2_1D20 => ControlKind::ListBox,
        0x8BD2_1D30 => ControlKind::ComboBox,
        0x8BD2_1D40 => ControlKind::CheckBox,
        0x8BD2_1D50 => ControlKind::OptionButton,
        0x8BD2_1D60 => ControlKind::ToggleButton,
        0xD705_3240 => ControlKind::CommandButton,
        0x978C_9E23 => ControlKind::Label,
        0x7917_6FB0 => ControlKind::SpinButton,
        0xDFD1_81E0 => ControlKind::ScrollBar,
        0x4C59_9241 => ControlKind::Image,
        _ => return None,
    };
    let mut c = ControlDesign::new(name.to_string(), kind);
    read_control(&mut c, bin.get(16..)?, code_page);
    Some(c)
}

fn read_control(c: &mut ControlDesign, d: &[u8], code_page: u16) {
    let mut r = Rd::new(d, 0);
    let _minor = r.u8();
    let _major = r.u8();
    let cb = usize::from(r.raw_u16());
    let after_header = 4 + cb;
    match c.kind {
        ControlKind::TextBox
        | ControlKind::ListBox
        | ControlKind::ComboBox
        | ControlKind::CheckBox
        | ControlKind::OptionButton
        | ControlKind::ToggleButton => {
            let lo = r.raw_u32() as u64;
            let hi = r.raw_u32() as u64;
            let m = lo | (hi << 32);
            r.base = r.pos;
            let bit = |b: u32| m & (1u64 << b) != 0;
            let various = if bit(0) { r.u32() } else { 0x2C80_081B };
            if bit(1) {
                c.back = Some(ole_color(r.u32()));
            }
            if bit(2) {
                c.fore = Some(ole_color(r.u32()));
            }
            if bit(3) {
                c.max_length = r.u32();
            }
            if bit(4) {
                r.u8();
            }
            if bit(5) {
                r.u8();
            }
            let display = if bit(6) { r.u8() } else { 0 };
            if bit(7) {
                r.u8();
            }
            if bit(9) {
                c.password_char = char::from_u32(u32::from(r.u16())).filter(|ch| *ch != '\0');
            }
            if bit(10) {
                r.u32();
            }
            if bit(11) {
                r.u16();
            }
            if bit(12) {
                r.u16();
            }
            if bit(13) {
                c.column_count = i32::from(r.u16() as i16);
            }
            if bit(14) {
                r.u16();
            }
            if bit(15) {
                r.u16();
            }
            if bit(16) {
                r.u8();
            }
            if bit(17) {
                r.u8();
            }
            if bit(18) {
                r.u8();
            }
            if bit(20) {
                r.u8();
            }
            if bit(21) {
                r.u8();
            }
            let value_len = if bit(22) { r.u32() } else { 0 };
            let caption_len = if bit(23) { r.u32() } else { 0 };
            if bit(24) {
                r.u32();
            }
            if bit(25) {
                r.u32();
            }
            if bit(26) {
                r.u32();
            }
            let mouse_icon = if bit(27) { r.u16() } else { 0 };
            let picture = if bit(28) { r.u16() } else { 0 };
            if bit(29) {
                r.u16();
            }
            let group_len = if bit(32) { r.u32() } else { 0 };
            r.align(4);
            r.base = r.pos;
            if bit(8) {
                c.width = himetric(r.i32());
                c.height = himetric(r.i32());
            }
            if bit(22) {
                c.value = r.string(value_len, code_page);
            }
            if bit(23) {
                c.caption = r.string(caption_len, code_page);
            }
            if bit(32) {
                r.string(group_len, code_page);
            }
            c.enabled = various & 2 != 0;
            c.locked = various & 4 != 0;
            c.transparent = various & 8 == 0;
            c.word_wrap = various & (1 << 23) != 0;
            c.multiline = various & (1 << 28) != 0;
            // A morph control's display style can make it another kind (7: drop-down list).
            c.kind = match display {
                1 => ControlKind::TextBox,
                2 => ControlKind::ListBox,
                3 => ControlKind::ComboBox,
                4 => ControlKind::CheckBox,
                5 => ControlKind::OptionButton,
                6 => ControlKind::ToggleButton,
                7 => {
                    c.drop_list = true;
                    ControlKind::ComboBox
                }
                _ => c.kind,
            };
            r.pos = after_header.min(d.len());
            if mouse_icon == 0xFFFF {
                skip_picture(&mut r);
            }
            if picture == 0xFFFF {
                skip_picture(&mut r);
            }
            c.font = text_props(&mut r, code_page);
        }
        ControlKind::CommandButton | ControlKind::Label => {
            let m = r.raw_u32();
            r.base = r.pos;
            let bit = |b: u32| m & (1 << b) != 0;
            let label = c.kind == ControlKind::Label;
            if bit(0) {
                c.fore = Some(ole_color(r.u32()));
            }
            if bit(1) {
                c.back = Some(ole_color(r.u32()));
            }
            let various = if bit(2) { r.u32() } else { 0x0080_001B };
            let caption_len = if bit(3) { r.u32() } else { 0 };
            if bit(4) {
                r.u32();
            }
            if bit(6) {
                r.u8();
            }
            let (mut picture, mut mouse_icon) = (0, 0);
            if label {
                if bit(7) {
                    r.u32();
                }
                if bit(8) {
                    r.u16();
                }
                if bit(9) {
                    r.u16();
                }
                if bit(10) {
                    picture = r.u16();
                }
                if bit(11) {
                    r.u16();
                }
                if bit(12) {
                    mouse_icon = r.u16();
                }
            } else {
                if bit(7) {
                    picture = r.u16();
                }
                if bit(8) {
                    r.u16();
                }
                if bit(10) {
                    mouse_icon = r.u16();
                }
            }
            r.align(4);
            r.base = r.pos;
            if bit(3) {
                c.caption = r.string(caption_len, code_page);
            }
            if bit(5) {
                c.width = himetric(r.i32());
                c.height = himetric(r.i32());
            }
            c.enabled = various & 2 != 0;
            c.transparent = various & 8 == 0;
            c.word_wrap = various & (1 << 23) != 0;
            r.pos = after_header.min(d.len());
            if picture == 0xFFFF {
                skip_picture(&mut r);
            }
            if mouse_icon == 0xFFFF {
                skip_picture(&mut r);
            }
            c.font = text_props(&mut r, code_page);
        }
        ControlKind::SpinButton | ControlKind::ScrollBar => {
            let m = r.raw_u32();
            r.base = r.pos;
            let bit = |b: u32| m & (1 << b) != 0;
            if bit(0) {
                c.fore = Some(ole_color(r.u32()));
            }
            if bit(1) {
                c.back = Some(ole_color(r.u32()));
            }
            let various = if bit(2) { r.u32() } else { 0x1B };
            if c.kind == ControlKind::SpinButton {
                if bit(5) {
                    c.min = r.i32();
                }
                if bit(6) {
                    c.max = r.i32();
                }
                if bit(7) {
                    c.position = r.i32();
                }
                if bit(8) {
                    r.u32();
                }
                if bit(9) {
                    r.u32();
                }
                if bit(10) {
                    c.small_change = r.i32();
                }
                if bit(11) {
                    r.i32();
                }
                if bit(12) {
                    r.i32();
                }
                if bit(13) {
                    r.u16();
                }
                if bit(14) {
                    r.u8();
                }
                r.align(4);
                r.base = r.pos;
                if bit(3) {
                    c.width = himetric(r.i32());
                    c.height = himetric(r.i32());
                }
            }
            c.enabled = various & 2 != 0;
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A form with a label and a text box, laid out the way the designer writes them.
    fn sample() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let vb = b"VERSION 5.00\r\nBegin {C62A69F0-16DC-11CE-9E98-00AA00574A4F} Pay \r\n   Caption         =   \"Paymode\"\r\n   ClientHeight    =   5295\r\n   ClientWidth     =   6090\r\n   StartUpPosition =   1  'CenterOwner\r\nEnd\r\n".to_vec();
        let mut f: Vec<u8> = vec![0x00, 0x04, 0x0C, 0x00];
        // PropMask: BackColor (1), DisplayedSize (10).
        f.extend_from_slice(&((1u32 << 1) | (1 << 10)).to_le_bytes());
        f.extend_from_slice(&0x00C0_0000u32.to_le_bytes()); // blue, BGR
        f.extend_from_slice(&10742i32.to_le_bytes());
        f.extend_from_slice(&9340i32.to_le_bytes());
        // Class table (none), 2 sites.
        f.extend_from_slice(&0u16.to_le_bytes());
        f.extend_from_slice(&2u32.to_le_bytes());
        f.extend_from_slice(&0u32.to_le_bytes());
        f.extend_from_slice(&[0x00, 0x82, 0x01, 0x00]);
        for (name, id, clsid, size, left) in [("Label1", 1i32, 21u16, 32u32, 1482i32), ("TextBox1", 2, 23, 32, 5503)] {
            let mut site: Vec<u8> = Vec::new();
            // Name, ID, ObjectStreamSize, TabIndex, ClsidCacheIndex, Position.
            site.extend_from_slice(&(1u32 | 4 | 32 | 64 | 128 | 256).to_le_bytes());
            site.extend_from_slice(&(0x8000_0000u32 | name.len() as u32).to_le_bytes());
            site.extend_from_slice(&id.to_le_bytes());
            site.extend_from_slice(&size.to_le_bytes());
            site.extend_from_slice(&(id as u16).to_le_bytes());
            site.extend_from_slice(&clsid.to_le_bytes());
            site.extend_from_slice(name.as_bytes());
            while !site.len().is_multiple_of(4) {
                site.push(0);
            }
            site.extend_from_slice(&left.to_le_bytes());
            site.extend_from_slice(&1693i32.to_le_bytes());
            f.extend_from_slice(&0u16.to_le_bytes());
            f.extend_from_slice(&(site.len() as u16).to_le_bytes());
            f.extend_from_slice(&site);
        }
        let mut o: Vec<u8> = Vec::new();
        // Label: ForeColor, Caption "Pay" (UTF-16), Size.
        let mut lab: Vec<u8> = Vec::new();
        lab.extend_from_slice(&(1u32 | 8 | 32).to_le_bytes());
        lab.extend_from_slice(&0x00FF_FFFFu32.to_le_bytes());
        lab.extend_from_slice(&6u32.to_le_bytes());
        lab.extend_from_slice(&[b'P', 0, b'a', 0, b'y', 0, 0, 0]);
        lab.extend_from_slice(&2540i32.to_le_bytes());
        lab.extend_from_slice(&1058i32.to_le_bytes());
        o.extend_from_slice(&[0, 2]);
        o.extend_from_slice(&(lab.len() as u16).to_le_bytes());
        o.extend_from_slice(&lab);
        o.resize(32, 0);
        // TextBox (morph data): Value "12", Size.
        let mut tb: Vec<u8> = Vec::new();
        tb.extend_from_slice(&((1u64 << 8) | (1 << 22)).to_le_bytes());
        tb.extend_from_slice(&(0x8000_0002u32).to_le_bytes());
        tb.extend_from_slice(&4022i32.to_le_bytes());
        tb.extend_from_slice(&1191i32.to_le_bytes());
        tb.extend_from_slice(b"12\0\0");
        o.extend_from_slice(&[0, 2]);
        o.extend_from_slice(&(tb.len() as u16).to_le_bytes());
        o.extend_from_slice(&tb);
        o.resize(64, 0);
        (vb, f, o)
    }

    #[test]
    fn reads_the_form_and_its_controls() {
        let (vb, f, o) = sample();
        let form = read_form("Pay", &vb, &f, &o, 1252);
        assert_eq!(form.caption, "Paymode");
        assert_eq!((form.width, form.height), (304.5, 264.75));
        assert_eq!(form.back, Some(0x0000C0));
        assert_eq!(form.start_position, 1);
        assert_eq!(form.controls.len(), 2);
        let l = &form.controls[0];
        assert_eq!((l.name.as_str(), l.kind, l.caption.as_str()), ("Label1", ControlKind::Label, "Pay"));
        assert_eq!(l.fore, Some(0xFFFFFF));
        assert!((l.left - 42.0).abs() < 0.1 && (l.top - 48.0).abs() < 0.1, "{} {}", l.left, l.top);
        assert!((l.width - 72.0).abs() < 0.1);
        let t = &form.controls[1];
        assert_eq!((t.name.as_str(), t.kind, t.value.as_str()), ("TextBox1", ControlKind::TextBox, "12"));
        assert!((t.width - 114.0).abs() < 0.1);
    }

    #[test]
    fn activex_combo_box_on_a_sheet() {
        // ComboBox class id, then morph data: display style 7 (drop-down list) and a value.
        let mut b: Vec<u8> = vec![0x30, 0x1d, 0xd2, 0x8b, 0x42, 0xec, 0xce, 0x11, 0x9e, 0x0d, 0x00, 0xaa, 0x00, 0x60, 0x02, 0xf3];
        let mut body: Vec<u8> = Vec::new();
        body.extend_from_slice(&((1u64 << 6) | (1 << 22)).to_le_bytes());
        body.extend_from_slice(&[7, 0, 0, 0]);
        body.extend_from_slice(&0x8000_0003u32.to_le_bytes());
        body.extend_from_slice(b"Ann\0");
        b.extend_from_slice(&[0, 2]);
        b.extend_from_slice(&(body.len() as u16).to_le_bytes());
        b.extend_from_slice(&body);
        let c = read_activex("ComboBox1", &b, 1252).unwrap();
        assert_eq!(c.kind, ControlKind::ComboBox);
        assert!(c.drop_list);
        assert_eq!(c.value, "Ann");
        assert!(read_activex("x", &[0u8; 20], 1252).is_none());
        assert!(read_activex("x", &[1, 2], 1252).is_none());
    }

    #[test]
    fn damaged_streams_give_what_was_readable() {
        let (vb, f, o) = sample();
        for cut in 0..f.len() {
            let _ = read_form("Pay", &vb, &f[..cut], &o, 1252);
        }
        for cut in 0..o.len() {
            let _ = read_form("Pay", &vb, &f, &o[..cut], 1252);
        }
        let junk: Vec<u8> = (0..400u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        let _ = read_form("X", &junk, &junk, &junk, 1252);
    }
}
