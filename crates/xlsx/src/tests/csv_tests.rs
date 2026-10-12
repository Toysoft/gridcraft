use gridcraft_core::{CellRef, DateSystem, Value};
use gridcraft_model::{Sheet, Workbook};

use crate::csv::{AnsiCodePage, decode_ansi, decode_text, guess_code_page, sniff_delimiter};
use crate::{CsvOptions, Format, read_csv, sniff, write_csv};

fn v(wb: &Workbook, r: u32, c: u32) -> Value {
    wb.sheet(0).unwrap().value(CellRef::new(r, c))
}

#[test]
fn quoting_and_newlines() {
    let src = "a,\"b,c\",\"say \"\"hi\"\"\"\r\n\"multi\nline\",,3\n\nlast";
    let wb = read_csv(src.as_bytes(), &CsvOptions::default()).unwrap();
    assert_eq!(wb.sheets[0].name, "Sheet1");
    assert_eq!(v(&wb, 0, 0), Value::text("a"));
    assert_eq!(v(&wb, 0, 1), Value::text("b,c"));
    assert_eq!(v(&wb, 0, 2), Value::text("say \"hi\""));
    assert_eq!(v(&wb, 1, 0), Value::text("multi\nline"));
    assert_eq!(v(&wb, 1, 1), Value::Empty);
    assert_eq!(v(&wb, 1, 2), Value::Number(3.0));
    assert_eq!(v(&wb, 2, 0), Value::Empty);
    assert_eq!(v(&wb, 3, 0), Value::text("last"));
}

#[test]
fn values_and_formats() {
    let src = "1,2.5,\"1,234\",50%,TRUE,#N/A,2026-03-04,$5.00,'007,text";
    let wb = read_csv(src.as_bytes(), &CsvOptions::default()).unwrap();
    assert_eq!(v(&wb, 0, 0), Value::Number(1.0));
    assert_eq!(v(&wb, 0, 1), Value::Number(2.5));
    assert_eq!(v(&wb, 0, 2), Value::Number(1234.0));
    assert_eq!(v(&wb, 0, 3), Value::Number(0.5));
    assert_eq!(v(&wb, 0, 4), Value::Bool(true));
    assert!(v(&wb, 0, 5).is_error());
    assert!(v(&wb, 0, 6).is_number());
    let s = wb.sheet(0).unwrap();
    let st = wb.styles.get(s.cell(CellRef::new(0, 6)).unwrap().style);
    assert_ne!(st.num_fmt.as_str(), "General");
    assert_eq!(v(&wb, 0, 8), Value::text("007"));
    // 1904 dates differ from 1900 dates.
    let wb2 = read_csv(b"2026-03-04", &CsvOptions { date_system: DateSystem::D1904, ..Default::default() }).unwrap();
    assert_eq!(wb2.date_system, DateSystem::D1904);
    assert_eq!(v(&wb, 0, 6).as_f64().unwrap() - v(&wb2, 0, 0).as_f64().unwrap(), 1462.0);
    // parse_values off keeps text.
    let wb3 = read_csv(b"1,TRUE", &CsvOptions { parse_values: false, ..Default::default() }).unwrap();
    assert_eq!(v(&wb3, 0, 0), Value::text("1"));
}

#[test]
fn encodings() {
    let mut b = vec![0xEF, 0xBB, 0xBF];
    b.extend_from_slice("é,x".as_bytes());
    assert_eq!(decode_text(&b), "é,x");
    let mut le = vec![0xFF, 0xFE];
    for u in "ü;1".encode_utf16() {
        le.extend_from_slice(&u.to_le_bytes());
    }
    assert_eq!(decode_text(&le), "ü;1");
    let mut be = vec![0xFE, 0xFF];
    for u in "ß".encode_utf16() {
        be.extend_from_slice(&u.to_be_bytes());
    }
    assert_eq!(decode_text(&be), "ß");
    assert_eq!(decode_text(&[0x80, b'a', 0xE9]), "€aé");
    let wb = read_csv(&le, &CsvOptions { delimiter: 0, ..Default::default() }).unwrap();
    assert_eq!(v(&wb, 0, 0), Value::text("ü"));
    assert_eq!(v(&wb, 0, 1), Value::Number(1.0));
}

#[test]
fn delimiter_sniffing() {
    assert_eq!(sniff_delimiter("a;b;c\n1;2;3\n", '"'), b';');
    assert_eq!(sniff_delimiter("a\tb\n1\t2\n", '"'), b'\t');
    assert_eq!(sniff_delimiter("a,b\n\"x;y;z\",2\n", '"'), b',');
    assert_eq!(sniff_delimiter("single", '"'), b',');
    // A delimiter missing from some lines is text, not a separator (single-column export round trip).
    assert_eq!(sniff_delimiter("alpha;beta\r\ngamma\r\n", '"'), b',');
    assert_eq!(sniff_delimiter("alpha\tbeta\r\ngamma\r\n", '"'), b',');
    assert_eq!(sniff_delimiter("a;b\r\n\r\nc;d\r\n", '"'), b';');
    let wb = read_csv(b"x;y\n1,5;2", &CsvOptions { delimiter: 0, ..Default::default() }).unwrap();
    assert_eq!(v(&wb, 1, 1), Value::Number(2.0));
    // The workbook remembers the separator it was read with, sniffed or given.
    assert_eq!(wb.csv_delimiter, Some(b';'));
    assert_eq!(read_csv(b"a\tb", &CsvOptions { delimiter: 0, ..Default::default() }).unwrap().csv_delimiter, Some(b'\t'));
    assert_eq!(read_csv(b"a;b", &CsvOptions::default()).unwrap().csv_delimiter, Some(b','));
    assert_eq!(Workbook::new().csv_delimiter, None);
    assert!(read_csv(b"a", &CsvOptions { delimiter: b'"', ..Default::default() }).is_err());
}

#[test]
fn writing() {
    let mut wb = Workbook::new();
    let mut s = Sheet::new("S");
    s.set_value(CellRef::new(1, 1), Value::text("a,b"));
    s.set_value(CellRef::new(1, 2), Value::text("q\"t"));
    s.set_value(CellRef::new(2, 1), Value::Number(0.1));
    s.set_value(CellRef::new(2, 2), Value::Bool(false));
    s.set_value(CellRef::new(3, 3), Value::text("x\ny"));
    s.set_value(CellRef::new(3, 1), Value::Error(gridcraft_core::CellError::Div0));
    wb.sheets = vec![std::sync::Arc::new(s)];
    // The caller supplies each field's text; quoting and line ends are the writer's.
    let sh = wb.sheet(0).unwrap();
    let raw = |c: CellRef| match sh.value(c) {
        Value::Empty => String::new(),
        Value::Number(n) => gridcraft_core::number_to_text(n),
        Value::Text(t) => t.to_string(),
        Value::Bool(b) => if b { "TRUE" } else { "FALSE" }.into(),
        Value::Error(e) => e.as_str().into(),
        Value::Array(_) => String::new(),
    };
    let out = String::from_utf8(write_csv(sh, b',', raw)).unwrap();
    assert_eq!(out, "\"a,b\",\"q\"\"t\",\r\n0.1,FALSE,\r\n#DIV/0!,,\"x\ny\"\r\n");
    let semi = String::from_utf8(write_csv(sh, b';', raw)).unwrap();
    assert!(semi.starts_with("a,b;"));
    assert!(write_csv(&Sheet::new("E"), b',', raw).is_empty());
    // Round trip.
    let back = read_csv(out.as_bytes(), &CsvOptions::default()).unwrap();
    assert_eq!(v(&back, 0, 0), Value::text("a,b"));
    assert_eq!(v(&back, 2, 2), Value::text("x\ny"));
}

#[test]
fn sniffing_formats() {
    assert_eq!(sniff(b"PK\x03\x04rest"), Format::Xlsx);
    assert_eq!(sniff(b"a,b\n1,2"), Format::Csv);
    // An Excel password-protected file is a CFB/OLE2 compound file with an EncryptedPackage
    // stream; any other compound file is a legacy binary (.xls), not an encrypted workbook.
    assert_eq!(sniff(&crate::cfb::tests::build(&["EncryptionInfo", "EncryptedPackage"])), Format::Encrypted);
    assert_eq!(sniff(&crate::cfb::tests::build(&["Workbook"])), Format::LegacyBinary);
    assert_eq!(sniff(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0]), Format::LegacyBinary);
    assert_eq!(sniff(&[0xD0, 0xCF, 0x11, 0xE0, 0, 0, 1, 2]), Format::Unknown);
    assert_eq!(sniff(b""), Format::Unknown);
    assert_eq!(sniff(&[0xFF, 0xFE, b'a', 0]), Format::Csv);
}

#[test]
fn hostile_csv() {
    let _ = read_csv(&[0xFF; 1000], &CsvOptions::default()).unwrap();
    let _ = read_csv(b"\"unterminated,quote\n1,2", &CsvOptions::default()).unwrap();
    let wide = ",".repeat(20_000);
    let wb = read_csv(format!("{wide}x").as_bytes(), &CsvOptions::default()).unwrap();
    assert!(wb.sheet(0).unwrap().cells.is_empty(), "fields past column XFD are dropped");
}

/// `s` in a single-byte code page, through the decoder's own table (every char must be in it).
fn encode_ansi(s: &str, page: AnsiCodePage) -> Vec<u8> {
    s.chars().map(|c| (0..=255u8).find(|&b| decode_ansi(&[b], page).starts_with(c)).unwrap_or_else(|| panic!("{c:?} not in {page:?}"))).collect()
}

#[test]
fn ansi_tables_match_known_bytes() {
    // Arabic Windows: "اسم" is C7 D3 E3; Arabic comma A1, question mark BF, lam E1, yeh ED.
    assert_eq!(decode_ansi(&[0xC7, 0xD3, 0xE3], AnsiCodePage::Windows1256), "اسم");
    assert_eq!(decode_ansi(&[0xA1, 0xBF, 0xE1, 0xED, 0xDC, 0x81], AnsiCodePage::Windows1256), "،؟ليـپ");
    assert_eq!(decode_ansi(&[0xE9, 0xD7, 0xF7], AnsiCodePage::Windows1256), "é×÷");
    // Cyrillic Windows: "Привет" is CF F0 E8 E2 E5 F2; Ё A8, ё B8, № B9.
    assert_eq!(decode_ansi(&[0xCF, 0xF0, 0xE8, 0xE2, 0xE5, 0xF2], AnsiCodePage::Windows1251), "Привет");
    assert_eq!(decode_ansi(&[0xA8, 0xB8, 0xB9], AnsiCodePage::Windows1251), "Ёё№");
    // Every byte decodes in every page without panicking.
    let all: Vec<u8> = (0..=255).collect();
    for page in [AnsiCodePage::Windows1252, AnsiCodePage::Windows1256, AnsiCodePage::Windows1251] {
        assert_eq!(decode_ansi(&all, page).chars().count(), 256);
    }
}

#[test]
fn ansi_csv_code_page_is_guessed_from_its_text() {
    // An Arabic accounts list as Excel saves "CSV (Comma delimited)" on Arabic Windows.
    let arabic = "اسم الحساب;المجموع\r\n10102-customer-1111 - عبدالسلام;-506\r\n10102-customer-0085 - شركة جودة الاعتناء؛ فرع 2;26387.5\r\nمقهى فليم الصحافة;2346.2\r\n";
    let bytes = encode_ansi(arabic, AnsiCodePage::Windows1256);
    assert_eq!(guess_code_page(&bytes), AnsiCodePage::Windows1256);
    assert_eq!(decode_text(&bytes), arabic);
    let wb = read_csv(&bytes, &CsvOptions { delimiter: 0, ..Default::default() }).unwrap();
    assert_eq!(v(&wb, 0, 0), Value::text("اسم الحساب"));
    assert_eq!(v(&wb, 1, 0), Value::text("10102-customer-1111 - عبدالسلام"));
    assert_eq!(v(&wb, 1, 1), Value::Number(-506.0));
    // A single short Arabic word still reads as Arabic.
    assert_eq!(decode_text(&encode_ansi("نعم,1", AnsiCodePage::Windows1256)), "نعم,1");

    // Russian on Cyrillic Windows.
    let russian = "Наименование;Количество;Цена\r\nСтол письменный;2;15000\r\nКресло офисное;4;7500,50\r\n";
    let bytes = encode_ansi(russian, AnsiCodePage::Windows1251);
    assert_eq!(guess_code_page(&bytes), AnsiCodePage::Windows1251);
    assert_eq!(decode_text(&bytes), russian);

    // Western European stays Windows-1252, as before.
    for western in ["Élève;café;Größe;Ñandú\r\n", "Prénom,Nom\r\nFrançois,Lefèvre\r\nZoë,Müller\r\n", "Preis: 5 €\r\n"] {
        let bytes = encode_ansi(western, AnsiCodePage::Windows1252);
        assert_eq!(guess_code_page(&bytes), AnsiCodePage::Windows1252, "{western:?}");
        assert_eq!(decode_text(&bytes), western);
    }
}
