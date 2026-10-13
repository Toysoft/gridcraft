//! Traditional Chinese (Taiwan) UI translations.
//! Terminology follows common Taiwanese spreadsheet conventions.

pub(super) fn translate(text: &str) -> Option<&'static str> {
    Some(match text {
        "Paste" => "貼上",
        "Paste Values" => "貼上值",
        "Paste Formulas" => "貼上公式",
        "Paste Formatting" => "貼上格式",
        "Transpose" => "轉置",
        "Paste Link" => "貼上連結",
        "Paste Special…" => "選擇性貼上…",
        "Insert Copied Cells" => "插入複製的儲存格",
        "Insert Copied Cells…" => "插入複製的儲存格…",
        "Insert Cut Cells" => "插入剪下的儲存格",
        "Cut (⌘X)" => "剪下 (⌘X)",
        "Copy (⌘C)" => "複製 (⌘C)",
        "Format Painter (double-click to keep it on)" => "複製格式（按兩下可持續使用）",
        "Bold (⌘B)" => "粗體 (⌘B)",
        "Italic (⌘I)" => "斜體 (⌘I)",
        "Underline (⌘U)" => "底線 (⌘U)",
        "Double Underline" => "雙底線",
        "Strikethrough" => "刪除線",
        "Underline" => "底線",
        "Wrap Text" => "自動換列",
        "Merge & Center" => "合併及置中",
        "Merge Cells" => "合併儲存格",
        "Unmerge Cells" => "取消合併儲存格",
        "Fill Color" => "填滿色彩",
        "Font Color" => "字型色彩",
        "Borders" => "框線",
        "Conditional Formatting" => "條件式格式設定",
        "Format Cells…" => "儲存格格式…",
        "Insert" => "插入",
        _ => return None,
    })
}
