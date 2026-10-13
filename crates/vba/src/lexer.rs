//! VBA tokens. Line continuations (` _` at a line end) are joined, comments dropped, and each
//! logical line ends with a `Newline` token (`:` separates statements on one line).

use crate::VbaError;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    /// An identifier or keyword as written (keywords are matched case-insensitively by the
    /// parser); a type suffix (`$`, `%`, `&`, `!`, `#`, `@`) is dropped.
    Ident(String),
    /// A `[bracketed]` identifier: never a keyword.
    Bracketed(String),
    Int(i64),
    Num(f64),
    Str(String),
    /// A `#date#` literal, as written.
    Date(String),
    /// Punctuation and operators: `( ) , . ! = < > <= >= <> + - * / \ ^ & : :=`.
    Op(&'static str),
    Newline,
}

/// A token with the 1-based source line it starts on (for error messages).
#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
}

/// Longest source accepted (a few MB of VBA is far beyond any real project).
const MAX_SOURCE: usize = 16 << 20;

pub fn lex(src: &str) -> Result<Vec<Token>, VbaError> {
    if src.len() > MAX_SOURCE {
        return Err(VbaError::Syntax { line: 0, msg: "module too large".into() });
    }
    let chars: Vec<char> = src.chars().collect();
    let mut out: Vec<Token> = Vec::new();
    let mut i = 0usize;
    let mut line = 1u32;
    let n = chars.len();
    let at = |k: usize| chars.get(k).copied().unwrap_or('\0');
    let push = |out: &mut Vec<Token>, tok: Tok, line: u32| out.push(Token { tok, line });
    while i < n {
        let c = chars[i];
        match c {
            '\r' | '\n' => {
                if c == '\r' && at(i + 1) == '\n' {
                    i += 1;
                }
                i += 1;
                if !matches!(out.last(), Some(Token { tok: Tok::Newline, .. }) | None) {
                    push(&mut out, Tok::Newline, line);
                }
                line += 1;
            }
            ' ' | '\t' | '\u{a0}' => i += 1,
            // Line continuation: whitespace, `_`, then end of line.
            '_' if (i == 0 || matches!(at(i - 1), ' ' | '\t')) && rest_is_line_end(&chars, i + 1) => {
                i += 1;
                while matches!(at(i), ' ' | '\t') {
                    i += 1;
                }
                if at(i) == '\r' {
                    i += 1;
                }
                if at(i) == '\n' {
                    i += 1;
                }
                line += 1;
            }
            '\'' => {
                // Comments run to the end of the line (a continuation continues them too).
                i = skip_comment(&chars, i, &mut line);
            }
            '"' => {
                let start_line = line;
                let mut s = String::new();
                i += 1;
                loop {
                    match chars.get(i) {
                        None | Some('\r') | Some('\n') => break,
                        Some('"') if at(i + 1) == '"' => {
                            s.push('"');
                            i += 2;
                        }
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some(&ch) => {
                            s.push(ch);
                            i += 1;
                        }
                    }
                }
                push(&mut out, Tok::Str(s), start_line);
            }
            '[' => {
                let start = i + 1;
                let mut j = start;
                while j < n && chars[j] != ']' && chars[j] != '\n' {
                    j += 1;
                }
                let name: String = chars.get(start..j).unwrap_or(&[]).iter().collect();
                push(&mut out, Tok::Bracketed(name), line);
                i = (j + 1).min(n);
            }
            '#' if is_date_literal(&chars, i) => {
                let mut j = i + 1;
                while j < n && chars[j] != '#' {
                    j += 1;
                }
                let lit: String = chars.get(i + 1..j).unwrap_or(&[]).iter().collect();
                push(&mut out, Tok::Date(lit), line);
                i = (j + 1).min(n);
            }
            '&' if matches!(at(i + 1), 'H' | 'h' | 'O' | 'o') && at(i + 2).is_ascii_alphanumeric() => {
                let radix = if matches!(at(i + 1), 'H' | 'h') { 16 } else { 8 };
                let mut j = i + 2;
                while at(j).is_digit(radix) {
                    j += 1;
                }
                let digits: String = chars.get(i + 2..j).unwrap_or(&[]).iter().collect();
                // `&` or `%` suffixes; `&HFFFF` is -1 as an Integer, `&HFFFF&` 65535.
                let long = at(j) == '&';
                if matches!(at(j), '&' | '%' | '^') {
                    j += 1;
                }
                let v = u64::from_str_radix(&digits, radix).unwrap_or(0);
                let v = if !long && v <= 0xFFFF && digits.len() <= 4 && radix == 16 {
                    i64::from(v as u16 as i16)
                } else if v <= 0xFFFF_FFFF {
                    i64::from(v as u32 as i32)
                } else {
                    v as i64
                };
                push(&mut out, Tok::Int(v), line);
                i = j;
            }
            c if c.is_ascii_digit() || (c == '.' && at(i + 1).is_ascii_digit()) => {
                let mut j = i;
                while at(j).is_ascii_digit() {
                    j += 1;
                }
                let mut float = false;
                if at(j) == '.' && at(j + 1).is_ascii_digit() {
                    float = true;
                    j += 1;
                    while at(j).is_ascii_digit() {
                        j += 1;
                    }
                } else if at(j) == '.' && !at(j + 1).is_alphabetic() {
                    // `1.` is a float.
                    float = true;
                    j += 1;
                }
                if matches!(at(j), 'e' | 'E' | 'd' | 'D')
                    && (at(j + 1).is_ascii_digit() || (matches!(at(j + 1), '+' | '-') && at(j + 2).is_ascii_digit()))
                {
                    float = true;
                    j += 2;
                    while at(j).is_ascii_digit() {
                        j += 1;
                    }
                }
                let text: String = chars.get(i..j).unwrap_or(&[]).iter().map(|c| if matches!(c, 'd' | 'D') { 'e' } else { *c }).collect();
                let suffix = at(j);
                if matches!(suffix, '#' | '!' | '@') {
                    float = true;
                    j += 1;
                } else if matches!(suffix, '%' | '&' | '^') {
                    j += 1;
                }
                let tok = if float {
                    Tok::Num(text.parse().unwrap_or(0.0))
                } else {
                    match text.parse::<i64>() {
                        Ok(v) => Tok::Int(v),
                        Err(_) => Tok::Num(text.parse().unwrap_or(0.0)),
                    }
                };
                push(&mut out, tok, line);
                i = j;
            }
            c if c.is_alphabetic() || c == '_' => {
                let mut j = i;
                while at(j).is_alphanumeric() || at(j) == '_' {
                    j += 1;
                }
                let word: String = chars.get(i..j).unwrap_or(&[]).iter().collect();
                // `Rem` comments.
                if word.eq_ignore_ascii_case("rem") && matches!(at(j), ' ' | '\t' | '\r' | '\n' | '\0') {
                    i = skip_comment(&chars, j, &mut line);
                    continue;
                }
                // Type suffix (`Left$`, `count%`); `!` only when not a bang member access.
                let suffix = matches!(at(j), '$' | '%' | '@')
                    || (at(j) == '&' && !at(j + 1).is_alphanumeric() && at(j + 1) != '"' && at(j + 1) != ' ')
                    || (at(j) == '#' && !is_date_literal(&chars, j) && !at(j + 1).is_alphanumeric());
                if suffix {
                    j += 1;
                }
                push(&mut out, Tok::Ident(word), line);
                i = j;
            }
            _ => {
                let two: String = [c, at(i + 1)].iter().collect();
                let op: Option<&'static str> = match two.as_str() {
                    "<=" => Some("<="),
                    ">=" => Some(">="),
                    "<>" => Some("<>"),
                    "=<" => Some("<="),
                    "=>" => Some(">="),
                    ":=" => Some(":="),
                    _ => None,
                };
                if let Some(op) = op {
                    push(&mut out, Tok::Op(op), line);
                    i += 2;
                    continue;
                }
                let op: &'static str = match c {
                    '(' => "(",
                    ')' => ")",
                    ',' => ",",
                    '.' => ".",
                    '!' => "!",
                    '=' => "=",
                    '<' => "<",
                    '>' => ">",
                    '+' => "+",
                    '-' => "-",
                    '*' => "*",
                    '/' => "/",
                    '\\' => "\\",
                    '^' => "^",
                    '&' => "&",
                    ':' => ":",
                    ';' => ";",
                    '#' => "#",
                    _ => return Err(VbaError::Syntax { line, msg: format!("unexpected character `{c}`") }),
                };
                push(&mut out, Tok::Op(op), line);
                i += 1;
            }
        }
    }
    if !matches!(out.last(), Some(Token { tok: Tok::Newline, .. }) | None) {
        push(&mut out, Tok::Newline, line);
    }
    Ok(out)
}

fn rest_is_line_end(chars: &[char], mut i: usize) -> bool {
    while matches!(chars.get(i), Some(' ' | '\t')) {
        i += 1;
    }
    matches!(chars.get(i), None | Some('\r' | '\n'))
}

/// Skips a comment from `i` to the end of its line (following continuations); returns the
/// index of the line end.
fn skip_comment(chars: &[char], mut i: usize, line: &mut u32) -> usize {
    loop {
        while i < chars.len() && chars[i] != '\n' && chars[i] != '\r' {
            i += 1;
        }
        // A comment ending in ` _` continues on the next line.
        let mut k = i;
        while k > 0 && matches!(chars.get(k - 1), Some(' ' | '\t')) {
            k -= 1;
        }
        let continued = k >= 2 && chars.get(k - 1) == Some(&'_') && matches!(chars.get(k - 2), Some(' ' | '\t'));
        if !continued || i >= chars.len() {
            return i;
        }
        if chars.get(i) == Some(&'\r') {
            i += 1;
        }
        if chars.get(i) == Some(&'\n') {
            i += 1;
        }
        *line += 1;
    }
}

/// Whether `#` at `i` opens a date literal (`#1/2/2020#`, `#12:00 PM#`).
fn is_date_literal(chars: &[char], i: usize) -> bool {
    let mut j = i + 1;
    let mut seen = false;
    while let Some(&c) = chars.get(j) {
        match c {
            '#' => return seen,
            '\r' | '\n' => return false,
            c if c.is_ascii_digit() => seen = true,
            '/' | '-' | ':' | ' ' | ',' | '.' => {}
            c if c.is_ascii_alphabetic() => {}
            _ => return false,
        }
        j += 1;
        if j - i > 40 {
            return false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(s: &str) -> Vec<Tok> {
        lex(s).unwrap().into_iter().map(|t| t.tok).collect()
    }

    #[test]
    fn statements_strings_numbers_and_continuations() {
        let t = toks("x = \"a\"\"b\" & 12 + 1.5 ' note\r\nSelection.PasteSpecial Paste:=xlValues, _\r\n  Transpose:=False\n");
        assert_eq!(
            t,
            vec![
                Tok::Ident("x".into()),
                Tok::Op("="),
                Tok::Str("a\"b".into()),
                Tok::Op("&"),
                Tok::Int(12),
                Tok::Op("+"),
                Tok::Num(1.5),
                Tok::Newline,
                Tok::Ident("Selection".into()),
                Tok::Op("."),
                Tok::Ident("PasteSpecial".into()),
                Tok::Ident("Paste".into()),
                Tok::Op(":="),
                Tok::Ident("xlValues".into()),
                Tok::Op(","),
                Tok::Ident("Transpose".into()),
                Tok::Op(":="),
                Tok::Ident("False".into()),
                Tok::Newline,
            ]
        );
    }

    #[test]
    fn literals_suffixes_and_comments() {
        assert_eq!(toks("&HFF"), vec![Tok::Int(255), Tok::Newline]);
        assert_eq!(toks("&HFFFF"), vec![Tok::Int(-1), Tok::Newline]);
        assert_eq!(toks("&HFFFF&"), vec![Tok::Int(65535), Tok::Newline]);
        assert_eq!(toks("#1/2/2020#"), vec![Tok::Date("1/2/2020".into()), Tok::Newline]);
        assert_eq!(toks("Left$(s, 2)")[0], Tok::Ident("Left".into()));
        assert_eq!(toks("Rem all of this\nx"), vec![Tok::Ident("x".into()), Tok::Newline]);
        assert_eq!(toks("a!b")[1], Tok::Op("!"));
        assert_eq!(toks("[A1]"), vec![Tok::Bracketed("A1".into()), Tok::Newline]);
        assert_eq!(toks("1E3"), vec![Tok::Num(1000.0), Tok::Newline]);
        assert_eq!(
            toks("x = a & b"),
            vec![Tok::Ident("x".into()), Tok::Op("="), Tok::Ident("a".into()), Tok::Op("&"), Tok::Ident("b".into()), Tok::Newline]
        );
    }

    #[test]
    fn hostile_text_never_panics() {
        for s in ["\"unterminated", "#1/2", "[open", "&H", "_", " _", "' _\n _", "\u{0}", "a$$$", "1e", "&O9"] {
            let _ = lex(s);
        }
    }
}
