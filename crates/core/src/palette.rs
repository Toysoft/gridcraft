//! Excel's legacy colour palette (`indexed` colours in files, `[ColorN]` in number formats).

/// The 64-entry legacy palette, RRGGBB. Entries 8–63 are the 56 colours a workbook can show;
/// number formats name them `[Color1]`..`[Color56]` (entry `N + 7`).
pub const INDEXED: [u32; 64] = [
    0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, // 0-7
    0x000000, 0xFFFFFF, 0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF, // 8-15
    0x800000, 0x008000, 0x000080, 0x808000, 0x800080, 0x008080, 0xC0C0C0, 0x808080, // 16-23
    0x9999FF, 0x993366, 0xFFFFCC, 0xCCFFFF, 0x660066, 0xFF8080, 0x0066CC, 0xCCCCFF, // 24-31
    0x000080, 0xFF00FF, 0xFFFF00, 0x00FFFF, 0x800080, 0x800000, 0x008080, 0x0000FF, // 32-39
    0x00CCFF, 0xCCFFFF, 0xCCFFCC, 0xFFFF99, 0x99CCFF, 0xFF99CC, 0xCC99FF, 0xFFCC99, // 40-47
    0x3366FF, 0x33CCCC, 0x99CC00, 0xFFCC00, 0xFF9900, 0xFF6600, 0x666699, 0x969696, // 48-55
    0x003366, 0x339966, 0x003300, 0x333300, 0x993300, 0x993366, 0x333399, 0x333333, // 56-63
];

/// Palette colour `[ColorN]` (1–56) as RRGGBB; black outside that range.
pub fn color_n(n: u8) -> u32 {
    INDEXED.get(usize::from(n) + 7).copied().filter(|_| (1..=56).contains(&n)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_n_reads_the_56_workbook_colours() {
        assert_eq!(color_n(1), 0x000000);
        assert_eq!(color_n(3), 0xFF0000);
        assert_eq!(color_n(10), 0x008000);
        assert_eq!(color_n(56), 0x333333);
        assert_eq!(color_n(0), 0);
        assert_eq!(color_n(57), 0);
        assert_eq!(color_n(u8::MAX), 0);
    }
}
