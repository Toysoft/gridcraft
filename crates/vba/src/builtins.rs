//! The VBA standard library (strings, maths, conversion, dates, arrays, dialogs) and the `vb…`
//! and `xl…` constants macros use.
//!
//! Functions that would reach outside the workbook (files, processes, the registry, COM) are
//! refused with run-time error 70 ("Permission denied"): GridCraft runs macros sandboxed.

use std::sync::Arc;

use crate::VbaError;
use crate::ast::Arg;
use crate::interp::{ArgVal, Host, Interp};
use crate::value::{Array, Value, date_parts, date_serial, number_text, parse_date, parse_num, round_even};

/// A built-in constant.
pub fn constant(name: &str) -> Option<Value> {
    let l = name.to_ascii_lowercase();
    let s = |t: &str| Some(Value::from(t));
    let i = |n: i64| Some(Value::Int(n));
    match l.as_str() {
        "vbcrlf" | "vbnewline" => s("\r\n"),
        "vbcr" => s("\r"),
        "vblf" => s("\n"),
        "vbtab" => s("\t"),
        "vbnullstring" => s(""),
        "vbnullchar" => s("\0"),
        "vbback" => s("\u{8}"),
        "vbokonly" => i(0),
        "vbokcancel" => i(1),
        "vbabortretryignore" => i(2),
        "vbyesnocancel" => i(3),
        "vbyesno" => i(4),
        "vbretrycancel" => i(5),
        "vbcritical" => i(16),
        "vbquestion" => i(32),
        "vbexclamation" => i(48),
        "vbinformation" => i(64),
        "vbdefaultbutton1" => i(0),
        "vbdefaultbutton2" => i(256),
        "vbdefaultbutton3" => i(512),
        "vbok" => i(1),
        "vbcancel" => i(2),
        "vbabort" => i(3),
        "vbretry" => i(4),
        "vbignore" => i(5),
        "vbyes" => i(6),
        "vbno" => i(7),
        "vbbinarycompare" => i(0),
        "vbtextcompare" => i(1),
        "vbusecompareoption" => i(-1),
        "vbsunday" => i(1),
        "vbmonday" => i(2),
        "vbtuesday" => i(3),
        "vbwednesday" => i(4),
        "vbthursday" => i(5),
        "vbfriday" => i(6),
        "vbsaturday" => i(7),
        "vbuppercase" => i(1),
        "vblowercase" => i(2),
        "vbpropercase" => i(3),
        "vbblack" => i(0),
        "vbred" => i(0xFF),
        "vbgreen" => i(0xFF00),
        "vbyellow" => i(0xFFFF),
        "vbblue" => i(0xFF_0000),
        "vbmagenta" => i(0xFF_00FF),
        "vbcyan" => i(0xFF_FF00),
        "vbwhite" => i(0xFF_FFFF),
        "vbkeyreturn" => i(13),
        "vbkeyescape" => i(27),
        "vbkeytab" => i(9),
        "vbkeyback" => i(8),
        "vbkeydelete" => i(46),
        "vbkeyup" => i(38),
        "vbkeydown" => i(40),
        "vbkeyleft" => i(37),
        "vbkeyright" => i(39),
        "vbempty" => i(0),
        "vbnull" => i(1),
        "vbinteger" => i(2),
        "vblong" => i(3),
        "vbsingle" => i(4),
        "vbdouble" => i(5),
        "vbcurrency" => i(6),
        "vbdate" => i(7),
        "vbstring" => i(8),
        "vbobject" => i(9),
        "vberror" => i(10),
        "vbboolean" => i(11),
        "vbvariant" => i(12),
        "vbarray" => i(8192),
        "vbformcontrolmenu" => i(0),
        "vbformcode" => i(1),
        "true" => Some(Value::Bool(true)),
        "false" => Some(Value::Bool(false)),
        // Excel enumerations (documented values).
        "xlup" | "xlshiftup" => i(-4162),
        "xldown" | "xlshiftdown" => i(-4121),
        "xltoleft" | "xlshifttoleft" => i(-4159),
        "xltoright" | "xlshifttoright" => i(-4161),
        "xlvalues" | "xlpastevalues" => i(-4163),
        "xlformulas" | "xlpasteformulas" => i(-4123),
        "xlcomments" | "xlpastecomments" => i(-4144),
        "xlpasteall" => i(-4104),
        "xlpasteformats" => i(-4122),
        "xlpastevaluesandnumberformats" => i(12),
        "xlpasteallexceptborders" => i(7),
        "xlpastecolumnwidths" => i(8),
        "xlwhole" => i(1),
        "xlpart" => i(2),
        "xlnone" | "xllinestylenone" => i(-4142),
        "xlno" => i(2),
        "xlyes" => i(1),
        "xlguess" => i(0),
        "xlbyrows" => i(1),
        "xlbycolumns" => i(2),
        "xlnext" => i(1),
        "xlprevious" => i(2),
        "xledgeleft" => i(7),
        "xledgetop" => i(8),
        "xledgebottom" => i(9),
        "xledgeright" => i(10),
        "xlinsidevertical" => i(11),
        "xlinsidehorizontal" => i(12),
        "xldiagonaldown" => i(5),
        "xldiagonalup" => i(6),
        "xlcontinuous" => i(1),
        "xldash" => i(-4115),
        "xldot" => i(-4118),
        "xldouble" => i(-4119),
        "xlhairline" => i(1),
        "xlthin" => i(2),
        "xlmedium" => i(-4138),
        "xlthick" => i(4),
        "xlcenter" => i(-4108),
        "xlleft" => i(-4131),
        "xlright" => i(-4152),
        "xltop" => i(-4160),
        "xlbottom" => i(-4107),
        "xlgeneral" => i(1),
        "xlascending" => i(1),
        "xldescending" => i(2),
        "xltoptobottom" => i(1),
        "xllefttoright" => i(2),
        "xlsortonvalues" => i(0),
        "xlsortnormal" => i(0),
        "xlpinyin" => i(1),
        "xlcelltypelastcell" => i(11),
        "xlcelltypeblanks" => i(4),
        "xlcelltypeconstants" => i(2),
        "xlcelltypeformulas" => i(-4123),
        "xlcelltypevisible" => i(12),
        "xlmanual" | "xlcalculationmanual" => i(-4135),
        "xlautomatic" | "xlcalculationautomatic" | "xlcolorindexautomatic" => i(-4105),
        "xlsheetvisible" => i(-1),
        "xlsheethidden" => i(0),
        "xlsheetveryhidden" => i(2),
        "xlworksheet" => i(-4167),
        "xlopenxmlworkbookmacroenabled" => i(52),
        "xlopenxmlworkbook" | "xlworkbookdefault" => i(51),
        "xlcsv" => i(6),
        "xlportrait" => i(1),
        "xllandscape" => i(2),
        "xland" => i(1),
        "xlor" => i(2),
        "xlfiltervalues" => i(7),
        "xlsolid" => i(1),
        "xlunderlinestylesingle" => i(2),
        "xlunderlinestylenone" => i(-4142),
        "xlwait" => i(2),
        "xldefault" => i(-4143),
        "xlnormal" => i(-4143),
        "xlmaximized" => i(-4137),
        "xlminimized" => i(-4140),
        "xltypepdf" => i(0),
        "xlcut" => i(2),
        "xlcopy" => i(1),
        _ => None,
    }
}

/// Names [`call`] handles.
pub fn is_builtin(lower: &str) -> bool {
    const NAMES: &[&str] = &[
        "msgbox",
        "inputbox",
        "len",
        "lenb",
        "left",
        "right",
        "mid",
        "instr",
        "instrrev",
        "replace",
        "trim",
        "ltrim",
        "rtrim",
        "ucase",
        "lcase",
        "strconv",
        "space",
        "string",
        "strreverse",
        "strcomp",
        "asc",
        "ascw",
        "chr",
        "chrw",
        "split",
        "join",
        "format",
        "format$",
        "formatnumber",
        "formatcurrency",
        "formatpercent",
        "formatdatetime",
        "cstr",
        "cdbl",
        "csng",
        "ccur",
        "cdec",
        "cint",
        "clng",
        "clnglng",
        "clngptr",
        "cbyte",
        "cbool",
        "cdate",
        "datevalue",
        "timevalue",
        "cvar",
        "cverr",
        "val",
        "str",
        "hex",
        "oct",
        "abs",
        "int",
        "fix",
        "sgn",
        "sqr",
        "exp",
        "log",
        "sin",
        "cos",
        "tan",
        "atn",
        "round",
        "rnd",
        "randomize",
        "rgb",
        "now",
        "date",
        "time",
        "timer",
        "year",
        "month",
        "day",
        "hour",
        "minute",
        "second",
        "weekday",
        "dateserial",
        "timeserial",
        "dateadd",
        "datediff",
        "monthname",
        "weekdayname",
        "isnumeric",
        "isdate",
        "isempty",
        "isnull",
        "ismissing",
        "isarray",
        "isobject",
        "iserror",
        "typename",
        "vartype",
        "iif",
        "choose",
        "switch",
        "nz",
        "array",
        "ubound",
        "lbound",
        "filter",
        "doevents",
        "beep",
        "environ",
        "environ$",
        "error",
        "error$",
        "unload",
        "load",
        "shell",
        "kill",
        "filecopy",
        "mkdir",
        "rmdir",
        "chdir",
        "chdrive",
        "setattr",
        "createobject",
        "getobject",
        "sendkeys",
        "savesetting",
        "deletesetting",
        "getsetting",
        "getallsettings",
        "name",
        "dir",
        "curdir",
        "filelen",
        "filedatetime",
        "getattr",
        "freefile",
        "callbyname",
        "appactivate",
    ];
    NAMES.contains(&lower)
}

fn arg<'a>(args: &'a [ArgVal], i: usize, name: &str) -> Option<&'a Value> {
    args.iter()
        .find(|a| a.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(name)))
        .or_else(|| args.get(i).filter(|a| a.name.is_none()))
        .map(|a| &a.value)
        .filter(|v| !matches!(v, Value::Missing))
}

fn req<'a>(args: &'a [ArgVal], i: usize, name: &str) -> Result<&'a Value, VbaError> {
    arg(args, i, name).ok_or_else(|| VbaError::runtime(449, format!("Argument not optional: {name}")))
}

fn char_slice(s: &str, start: usize, len: usize) -> String {
    s.chars().skip(start).take(len).collect()
}

fn to_usize(v: &Value) -> Result<usize, VbaError> {
    let n = v.to_int()?;
    usize::try_from(n).map_err(|_| VbaError::runtime(5, "Invalid procedure call or argument"))
}

/// Calls a built-in function; `None` when `name` isn't one.
pub fn call(it: &mut Interp, host: &mut dyn Host, name: &str, args: &[ArgVal], _raw: &[Arg]) -> Option<Result<Value, VbaError>> {
    let l = name.to_ascii_lowercase();
    // Scalars for arguments that are objects (a Range passed to Len gives its value's length).
    let mut sargs: Vec<ArgVal> = Vec::with_capacity(args.len());
    if l != "isobject" && l != "typename" && l != "unload" && l != "load" && l != "isarray" && l != "ubound" && l != "lbound" && l != "vartype" {
        for a in args {
            let v = match it.scalar_pub(host, a.value.clone()) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            sargs.push(ArgVal { value: v, name: a.name.clone(), back: None });
        }
    }
    let a: &[ArgVal] = if sargs.len() == args.len() { &sargs } else { args };
    builtin(it, host, &l, a).transpose()
}

fn builtin(it: &mut Interp, host: &mut dyn Host, l: &str, a: &[ArgVal]) -> Result<Option<Value>, VbaError> {
    let text = |i: usize, n: &str| -> Result<String, VbaError> {
        match arg(a, i, n) {
            Some(Value::Null) => Ok(String::new()),
            Some(v) => v.to_text(),
            None => Ok(String::new()),
        }
    };
    let num = |i: usize, n: &str| -> Result<f64, VbaError> { req(a, i, n)?.to_num() };
    let opt_int = |i: usize, n: &str, d: i64| -> Result<i64, VbaError> {
        match arg(a, i, n) {
            Some(v) => v.to_int(),
            None => Ok(d),
        }
    };
    Ok(Some(match l {
        // ---------------------------------------------------------- dialogs
        "msgbox" => {
            let prompt = text(0, "Prompt")?;
            let buttons = opt_int(1, "Buttons", 0)?;
            let title = text(2, "Title")?;
            Value::Int(host.msgbox(&prompt, buttons, if title.is_empty() { "GridCraft" } else { &title })?)
        }
        "inputbox" => {
            let r = host.inputbox(&text(0, "Prompt")?, &text(1, "Title")?, &text(2, "Default")?)?;
            Value::from(r.unwrap_or_default())
        }
        // ---------------------------------------------------------- strings
        "len" | "lenb" => match arg(a, 0, "String") {
            Some(Value::Null) => Value::Null,
            Some(v) => Value::Int(v.to_text()?.chars().count() as i64),
            None => Value::Int(0),
        },
        "left" => {
            let s = text(0, "String")?;
            Value::from(char_slice(&s, 0, to_usize(req(a, 1, "Length")?)?))
        }
        "right" => {
            let s = text(0, "String")?;
            let n = to_usize(req(a, 1, "Length")?)?;
            let len = s.chars().count();
            Value::from(char_slice(&s, len.saturating_sub(n), n))
        }
        "mid" => {
            let s = text(0, "String")?;
            let start = to_usize(req(a, 1, "Start")?)?;
            if start == 0 {
                return Err(VbaError::runtime(5, "Invalid procedure call or argument"));
            }
            let len = match arg(a, 2, "Length") {
                Some(v) => to_usize(v)?,
                None => usize::MAX,
            };
            Value::from(char_slice(&s, start - 1, len))
        }
        "instr" => {
            // InStr([start,] string1, string2[, compare]).
            let (start, s1, s2, cmp) = if a.len() >= 3 {
                (to_usize(req(a, 0, "Start")?)?, text(1, "String1")?, text(2, "String2")?, opt_int(3, "Compare", 0)?)
            } else {
                (1, text(0, "String1")?, text(1, "String2")?, 0)
            };
            Value::Int(instr(&s1, &s2, start, cmp == 1).map_or(0, |p| p as i64))
        }
        "instrrev" => {
            let s1 = text(0, "StringCheck")?;
            let s2 = text(1, "StringMatch")?;
            let hay: Vec<char> = s1.chars().collect();
            let needle: Vec<char> = s2.chars().collect();
            let start = match arg(a, 2, "Start") {
                Some(v) if v.to_int()? > 0 => to_usize(v)?.min(hay.len()),
                _ => hay.len(),
            };
            let mut found = 0;
            if needle.len() <= start {
                for i in (0..=start - needle.len()).rev() {
                    if hay.get(i..i + needle.len()) == Some(&needle[..]) {
                        found = i + 1;
                        break;
                    }
                }
            }
            Value::Int(found as i64)
        }
        "replace" => {
            let s = text(0, "Expression")?;
            let find = text(1, "Find")?;
            let with = text(2, "Replace")?;
            if find.is_empty() {
                Value::from(s)
            } else if opt_int(5, "Compare", 0)? == 1 {
                Value::from(replace_ci(&s, &find, &with))
            } else {
                Value::from(s.replace(&find, &with))
            }
        }
        "trim" => Value::from(text(0, "String")?.trim_matches(' ').to_string()),
        "ltrim" => Value::from(text(0, "String")?.trim_start_matches(' ').to_string()),
        "rtrim" => Value::from(text(0, "String")?.trim_end_matches(' ').to_string()),
        "ucase" => Value::from(text(0, "String")?.to_uppercase()),
        "lcase" => Value::from(text(0, "String")?.to_lowercase()),
        "strconv" => {
            let s = text(0, "String")?;
            match opt_int(1, "Conversion", 0)? {
                1 => Value::from(s.to_uppercase()),
                2 => Value::from(s.to_lowercase()),
                3 => Value::from(proper(&s)),
                _ => Value::from(s),
            }
        }
        "space" => Value::from(" ".repeat(to_usize(req(a, 0, "Number")?)?.min(10_000_000))),
        "string" => {
            let n = to_usize(req(a, 0, "Number")?)?.min(10_000_000);
            let c = match req(a, 1, "Character")? {
                Value::Str(s) => s.chars().next().unwrap_or(' '),
                v => char::from_u32(v.to_int()? as u32).unwrap_or(' '),
            };
            Value::from(std::iter::repeat_n(c, n).collect::<String>())
        }
        "strreverse" => Value::from(text(0, "Expression")?.chars().rev().collect::<String>()),
        "strcomp" => {
            let (x, y) = (text(0, "String1")?, text(1, "String2")?);
            let o = if opt_int(2, "Compare", 0)? == 1 { x.to_lowercase().cmp(&y.to_lowercase()) } else { x.cmp(&y) };
            Value::Int(match o {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            })
        }
        "asc" | "ascw" => {
            Value::Int(text(0, "String")?.chars().next().map(|c| c as i64).ok_or(VbaError::runtime(5, "Invalid procedure call or argument"))?)
        }
        "chr" | "chrw" => {
            let n = req(a, 0, "CharCode")?.to_int()?;
            Value::from(
                char::from_u32(u32::try_from(n).map_err(|_| VbaError::runtime(5, "Invalid procedure call or argument"))?)
                    .unwrap_or('\u{FFFD}')
                    .to_string(),
            )
        }
        "split" => {
            let s = text(0, "Expression")?;
            let d = match arg(a, 1, "Delimiter") {
                Some(v) => v.to_text()?,
                None => " ".into(),
            };
            let parts: Vec<Value> = if s.is_empty() {
                vec![]
            } else if d.is_empty() {
                vec![Value::from(s)]
            } else {
                s.split(d.as_str()).map(Value::from).collect()
            };
            Value::Array(Arc::new(Array::from_vec(0, parts)))
        }
        "join" => {
            let Some(Value::Array(arr)) = arg(a, 0, "SourceArray") else { return Err(VbaError::runtime(13, "Type mismatch")) };
            let d = match arg(a, 1, "Delimiter") {
                Some(v) => v.to_text()?,
                None => " ".into(),
            };
            let parts: Result<Vec<String>, VbaError> = arr.data.iter().map(|v| v.to_text()).collect();
            Value::from(parts?.join(&d))
        }
        "format" | "format$" => {
            let v = req(a, 0, "Expression")?.clone();
            let f = text(1, "Format")?;
            Value::from(format_value(&v, &f)?)
        }
        "formatnumber" => {
            let n = num(0, "Expression")?;
            let d = opt_int(1, "NumDigitsAfterDecimal", 2)?.clamp(0, 15) as usize;
            let code = if d == 0 { "#,##0".to_string() } else { format!("#,##0.{}", "0".repeat(d)) };
            Value::from(format_value(&Value::Double(n), &code)?)
        }
        "formatcurrency" => Value::from(format_value(&Value::Double(num(0, "Expression")?), "$#,##0.00")?),
        "formatpercent" => Value::from(format_value(&Value::Double(num(0, "Expression")?), "0.00%")?),
        "formatdatetime" => Value::from(crate::value::date_text(req(a, 0, "Date")?.to_date()?)),
        // ---------------------------------------------------------- conversion
        "cstr" => Value::from(req(a, 0, "Expression")?.to_text()?),
        "cdbl" | "csng" | "ccur" | "cdec" => Value::Double(num(0, "Expression")?),
        "cint" => crate::interp::coerce(req(a, 0, "Expression")?.clone(), &crate::ast::TypeName::Integer)?,
        "clng" | "clnglng" | "clngptr" => crate::interp::coerce(req(a, 0, "Expression")?.clone(), &crate::ast::TypeName::Long)?,
        "cbyte" => crate::interp::coerce(req(a, 0, "Expression")?.clone(), &crate::ast::TypeName::Byte)?,
        "cbool" => Value::Bool(req(a, 0, "Expression")?.to_bool()?),
        "cdate" | "datevalue" => Value::Date(req(a, 0, "Date")?.to_date()?),
        "timevalue" => Value::Date(req(a, 0, "Time")?.to_date()?.fract()),
        "cvar" => req(a, 0, "Expression")?.clone(),
        "cverr" => Value::Error(req(a, 0, "ErrorNumber")?.to_int()?),
        "val" => Value::Double(val(&text(0, "String")?)),
        "str" => {
            let n = num(0, "Number")?;
            let t = number_text(n);
            Value::from(if n >= 0.0 { format!(" {t}") } else { t })
        }
        "hex" => Value::from(format!("{:X}", req(a, 0, "Number")?.to_int()?)),
        "oct" => Value::from(format!("{:o}", req(a, 0, "Number")?.to_int()?)),
        // ---------------------------------------------------------- maths
        "abs" => match req(a, 0, "Number")? {
            Value::Int(n) => Value::Int(n.abs()),
            v => Value::Double(v.to_num()?.abs()),
        },
        "int" => Value::Double(num(0, "Number")?.floor()),
        "fix" => Value::Double(num(0, "Number")?.trunc()),
        "sgn" => Value::Int(num(0, "Number")?.partial_cmp(&0.0).map_or(0, |o| o as i64)),
        "sqr" => {
            let n = num(0, "Number")?;
            if n < 0.0 {
                return Err(VbaError::runtime(5, "Invalid procedure call or argument"));
            }
            Value::Double(n.sqrt())
        }
        "exp" => Value::Double(num(0, "Number")?.exp()),
        "log" => {
            let n = num(0, "Number")?;
            if n <= 0.0 {
                return Err(VbaError::runtime(5, "Invalid procedure call or argument"));
            }
            Value::Double(n.ln())
        }
        "sin" => Value::Double(num(0, "Number")?.sin()),
        "cos" => Value::Double(num(0, "Number")?.cos()),
        "tan" => Value::Double(num(0, "Number")?.tan()),
        "atn" => Value::Double(num(0, "Number")?.atan()),
        "round" => {
            let n = num(0, "Number")?;
            let d = opt_int(1, "NumDigitsAfterDecimal", 0)?.clamp(0, 15) as i32;
            let p = 10f64.powi(d);
            Value::Double(round_even(n * p) / p)
        }
        "rnd" => Value::Double(rnd(host)),
        "randomize" => Value::Empty,
        "rgb" => {
            let c = |i: usize, n: &str| -> Result<i64, VbaError> { Ok(req(a, i, n)?.to_int()?.clamp(0, 255)) };
            Value::Int(c(0, "Red")? | (c(1, "Green")? << 8) | (c(2, "Blue")? << 16))
        }
        // ---------------------------------------------------------- dates
        "now" => Value::Date(host.now()),
        "date" => Value::Date(host.now().floor()),
        "time" => Value::Date(host.now().fract()),
        "timer" => Value::Double(host.now().fract() * 86_400.0),
        "year" => Value::Int(i64::from(date_parts(req(a, 0, "Date")?.to_date()?).0)),
        "month" => Value::Int(i64::from(date_parts(req(a, 0, "Date")?.to_date()?).1)),
        "day" => Value::Int(i64::from(date_parts(req(a, 0, "Date")?.to_date()?).2)),
        "hour" => Value::Int(i64::from(date_parts(req(a, 0, "Time")?.to_date()?).3)),
        "minute" => Value::Int(i64::from(date_parts(req(a, 0, "Time")?.to_date()?).4)),
        "second" => Value::Int(i64::from(date_parts(req(a, 0, "Time")?.to_date()?).5)),
        "weekday" => {
            let d = req(a, 0, "Date")?.to_date()?.floor() as i64;
            let first = opt_int(1, "FirstDayOfWeek", 1)?.clamp(1, 7);
            // Serial 1 (1899-12-31) was a Sunday.
            Value::Int((d - 1 - (first - 1)).rem_euclid(7) + 1)
        }
        "dateserial" => Value::Date(date_serial(req(a, 0, "Year")?.to_int()?, req(a, 1, "Month")?.to_int()?, req(a, 2, "Day")?.to_int()?)),
        "timeserial" => Value::Date((num(0, "Hour")? * 3600.0 + num(1, "Minute")? * 60.0 + num(2, "Second")?) / 86_400.0),
        "dateadd" => {
            let unit = text(0, "Interval")?.to_ascii_lowercase();
            let n = num(1, "Number")?;
            let d = req(a, 2, "Date")?.to_date()?;
            Value::Date(date_add(&unit, n, d)?)
        }
        "datediff" => {
            let unit = text(0, "Interval")?.to_ascii_lowercase();
            let (d1, d2) = (req(a, 1, "Date1")?.to_date()?, req(a, 2, "Date2")?.to_date()?);
            Value::Int(date_diff(&unit, d1, d2)?)
        }
        "monthname" => {
            const M: [&str; 12] =
                ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
            let m = req(a, 0, "Month")?.to_int()?;
            Value::from(M.get(usize::try_from(m - 1).unwrap_or(99)).copied().ok_or(VbaError::runtime(5, "Invalid procedure call or argument"))?)
        }
        "weekdayname" => {
            const D: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
            let w = req(a, 0, "Weekday")?.to_int()?;
            Value::from(D.get(usize::try_from(w - 1).unwrap_or(99)).copied().ok_or(VbaError::runtime(5, "Invalid procedure call or argument"))?)
        }
        // ---------------------------------------------------------- information
        "isnumeric" => {
            Value::Bool(arg(a, 0, "Expression").is_some_and(|v| v.is_numeric() && !matches!(v, Value::Empty) || matches!(v, Value::Empty)))
        }
        "isdate" => Value::Bool(match arg(a, 0, "Expression") {
            Some(Value::Date(_)) => true,
            Some(Value::Str(s)) => parse_date(s).is_some(),
            _ => false,
        }),
        "isempty" => Value::Bool(matches!(arg(a, 0, "Expression"), Some(Value::Empty) | None)),
        "isnull" => Value::Bool(matches!(arg(a, 0, "Expression"), Some(Value::Null))),
        "ismissing" => Value::Bool(a.first().is_none_or(|x| matches!(x.value, Value::Missing))),
        "isarray" => Value::Bool(matches!(arg(a, 0, "VarName"), Some(Value::Array(_)))),
        "isobject" => Value::Bool(arg(a, 0, "Expression").is_some_and(|v| v.is_object())),
        "iserror" => Value::Bool(matches!(arg(a, 0, "Expression"), Some(Value::Error(_)))),
        "typename" => Value::from(arg(a, 0, "VarName").map_or("Empty".into(), |v| v.type_name())),
        "vartype" => Value::Int(match arg(a, 0, "VarName") {
            None | Some(Value::Empty) => 0,
            Some(Value::Null) => 1,
            Some(Value::Int(n)) if i16::try_from(*n).is_ok() => 2,
            Some(Value::Int(_)) => 3,
            Some(Value::Double(_)) => 5,
            Some(Value::Date(_)) => 7,
            Some(Value::Str(_)) => 8,
            Some(Value::Object(_) | Value::Nothing) => 9,
            Some(Value::Error(_) | Value::Missing) => 10,
            Some(Value::Bool(_)) => 11,
            Some(Value::Array(_)) => 8204,
        }),
        "iif" => {
            if req(a, 0, "Expression")?.to_bool()? {
                req(a, 1, "TruePart")?.clone()
            } else {
                req(a, 2, "FalsePart")?.clone()
            }
        }
        "choose" => {
            let i = req(a, 0, "Index")?.to_int()?;
            a.get(usize::try_from(i).unwrap_or(0)).filter(|_| i >= 1).map(|x| x.value.clone()).unwrap_or(Value::Null)
        }
        "switch" => {
            let mut r = Value::Null;
            for pair in a.chunks(2) {
                if let [c, v] = pair
                    && c.value.to_bool()?
                {
                    r = v.value.clone();
                    break;
                }
            }
            r
        }
        "nz" => match arg(a, 0, "Value") {
            Some(Value::Null) | None => arg(a, 1, "ValueIfNull").cloned().unwrap_or(Value::from("")),
            Some(v) => v.clone(),
        },
        // ---------------------------------------------------------- arrays
        "array" => Value::Array(Arc::new(Array::from_vec(0, a.iter().map(|x| x.value.clone()).collect()))),
        "ubound" | "lbound" => {
            let Some(Value::Array(arr)) = arg(a, 0, "ArrayName") else { return Err(VbaError::runtime(13, "Type mismatch")) };
            let dim = usize::try_from(opt_int(1, "Dimension", 1)? - 1).map_err(|_| VbaError::runtime(9, "Subscript out of range"))?;
            if arr.dims.get(dim).is_some_and(|d| d.1 == 0) && l == "ubound" {
                return Ok(Some(Value::Int(arr.lbound(dim).unwrap_or(0) - 1)));
            }
            Value::Int(if l == "ubound" { arr.ubound(dim) } else { arr.lbound(dim) }.ok_or(VbaError::runtime(9, "Subscript out of range"))?)
        }
        "filter" => {
            let Some(Value::Array(arr)) = arg(a, 0, "SourceArray") else { return Err(VbaError::runtime(13, "Type mismatch")) };
            let m = text(1, "Match")?;
            let include = match arg(a, 2, "Include") {
                Some(v) => v.to_bool()?,
                None => true,
            };
            let v: Vec<Value> = arr.data.iter().filter(|x| x.to_text().map(|t| t.contains(&m) == include).unwrap_or(false)).cloned().collect();
            Value::Array(Arc::new(Array::from_vec(0, v)))
        }
        // ---------------------------------------------------------- misc
        "doevents" | "beep" | "randomize " => Value::Empty,
        "environ" | "environ$" => Value::from(""),
        "error" | "error$" => Value::from(it.err.description.clone()),
        "unload" | "load" => {
            // `Unload UserForm1`: the host closes the form.
            if let Some(Value::Object(crate::Object::Host(h))) = arg(a, 0, "Object") {
                let r = host.get(h, if l == "unload" { "_Unload" } else { "_Load" }, &[])?;
                drop(r);
            }
            Value::Empty
        }
        // ---------------------------------------------------------- sandbox
        "shell" | "kill" | "filecopy" | "mkdir" | "rmdir" | "chdir" | "chdrive" | "setattr" | "createobject" | "getobject" | "sendkeys"
        | "savesetting" | "deletesetting" | "getsetting" | "getallsettings" | "name" | "dir" | "curdir" | "filelen" | "filedatetime" | "getattr"
        | "freefile" | "callbyname" | "appactivate" => {
            return Err(VbaError::runtime(70, format!("Permission denied: {l} is not available to macros in GridCraft")));
        }
        _ => return Ok(None),
    }))
}

fn rnd(host: &mut dyn Host) -> f64 {
    // A time-seeded xorshift: macros only use Rnd for jitter and sampling.
    use std::sync::atomic::{AtomicU64, Ordering};
    static STATE: AtomicU64 = AtomicU64::new(0);
    let mut x = STATE.load(Ordering::Relaxed);
    if x == 0 {
        x = (host.now() * 1e9) as u64 | 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    STATE.store(x, Ordering::Relaxed);
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// 1-based char position of `needle` in `hay` from `start`, `None` when absent.
fn instr(hay: &str, needle: &str, start: usize, ignore_case: bool) -> Option<usize> {
    let fold = |s: &str| if ignore_case { s.to_lowercase() } else { s.to_string() };
    let h: Vec<char> = fold(hay).chars().collect();
    let n: Vec<char> = fold(needle).chars().collect();
    let start = start.max(1);
    if n.is_empty() {
        return (start <= h.len() + 1).then_some(start);
    }
    (start - 1..h.len().saturating_sub(n.len()) + 1).find(|&i| h.get(i..i + n.len()) == Some(&n[..])).map(|i| i + 1)
}

fn replace_ci(s: &str, find: &str, with: &str) -> String {
    let lower = s.to_lowercase();
    let f = find.to_lowercase();
    if lower.len() != s.len() {
        return s.replace(find, with);
    }
    let mut out = String::new();
    let mut i = 0;
    while let Some(p) = lower.get(i..).and_then(|r| r.find(&f)) {
        out.push_str(s.get(i..i + p).unwrap_or(""));
        out.push_str(with);
        i += p + f.len();
    }
    out.push_str(s.get(i..).unwrap_or(""));
    out
}

fn proper(s: &str) -> String {
    let mut out = String::new();
    let mut start = true;
    for c in s.chars() {
        if c.is_alphabetic() {
            if start {
                out.extend(c.to_uppercase());
            } else {
                out.extend(c.to_lowercase());
            }
            start = false;
        } else {
            out.push(c);
            start = true;
        }
    }
    out
}

/// `Val`: the leading number of a string (spaces ignored), 0 when there is none.
fn val(s: &str) -> f64 {
    let t: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if let Some(h) = t.strip_prefix("&H").or_else(|| t.strip_prefix("&h")) {
        let digits: String = h.chars().take_while(|c| c.is_ascii_hexdigit()).collect();
        return i64::from_str_radix(&digits, 16).map(|v| v as f64).unwrap_or(0.0);
    }
    let mut end = 0;
    let mut seen_dot = false;
    let mut seen_e = false;
    for (i, c) in t.char_indices() {
        let ok = c.is_ascii_digit()
            || (i == 0 && matches!(c, '+' | '-'))
            || (c == '.' && !seen_dot && !seen_e)
            || (matches!(c, 'e' | 'E') && !seen_e && i > 0)
            || (matches!(c, '+' | '-') && seen_e && t[..i].ends_with(['e', 'E']));
        if !ok {
            break;
        }
        seen_dot |= c == '.';
        seen_e |= matches!(c, 'e' | 'E');
        end = i + c.len_utf8();
    }
    let mut num = &t[..end];
    while !num.is_empty() && num.parse::<f64>().is_err() {
        num = &num[..num.len() - 1];
    }
    num.parse().unwrap_or(0.0)
}

/// `Format(value, code)`: named formats, else Excel number/date format codes.
pub fn format_value(v: &Value, code: &str) -> Result<String, VbaError> {
    let named = match code.to_ascii_lowercase().as_str() {
        "" => return v.to_text(),
        "general number" => "General",
        "currency" => "$#,##0.00",
        "fixed" => "0.00",
        "standard" => "#,##0.00",
        "percent" => "0.00%",
        "scientific" => "0.00E+00",
        "yes/no" => return Ok(if v.to_bool()? { "Yes" } else { "No" }.into()),
        "true/false" => return Ok(if v.to_bool()? { "True" } else { "False" }.into()),
        "on/off" => return Ok(if v.to_bool()? { "On" } else { "Off" }.into()),
        "short date" => "m/d/yyyy",
        "long date" => "dddd, mmmm d, yyyy",
        "medium date" => "dd-mmm-yy",
        "short time" => "hh:mm",
        "medium time" => "hh:mm AM/PM",
        "long time" => "h:mm:ss AM/PM",
        "general date" => return Ok(crate::value::date_text(v.to_date()?)),
        _ => code,
    };
    // VBA's Format accepts lower-case `mm` for minutes after `h` and `nn` for minutes.
    let named = named.replace("nn", "mm");
    let cv = match v {
        Value::Str(s) => match parse_num(s).or_else(|| parse_date(s)) {
            Some(n) => gridcraft_core::Value::Number(n),
            None => gridcraft_core::Value::Text(s.to_string().into()),
        },
        Value::Empty => gridcraft_core::Value::Number(0.0),
        Value::Bool(b) => gridcraft_core::Value::Number(if *b { -1.0 } else { 0.0 }),
        other => gridcraft_core::Value::Number(other.to_num()?),
    };
    gridcraft_numfmt::text_function(&cv, &named, gridcraft_core::DateSystem::D1900).map_err(|_| VbaError::runtime(13, "Type mismatch"))
}

fn date_add(unit: &str, n: f64, d: f64) -> Result<f64, VbaError> {
    let (y, m, day, ..) = date_parts(d);
    let time = d - d.floor();
    let n_i = n.trunc() as i64;
    Ok(match unit {
        "yyyy" | "q" | "m" => {
            let months = match unit {
                "yyyy" => n_i * 12,
                "q" => n_i * 3,
                _ => n_i,
            };
            let total = i64::from(y) * 12 + i64::from(m) - 1 + months;
            let (ny, nm) = (total.div_euclid(12), total.rem_euclid(12) + 1);
            // Clamp the day to the new month's length.
            let last = gridcraft_core::date::days_in_month(i32::try_from(ny).unwrap_or(2000), nm as u32);
            date_serial(ny, nm, i64::from(day.min(last))) + time
        }
        "d" | "y" | "w" => d + n.trunc(),
        "ww" => d + n.trunc() * 7.0,
        "h" => d + n / 24.0,
        "n" => d + n / 1440.0,
        "s" => d + n / 86_400.0,
        _ => return Err(VbaError::runtime(5, "Invalid procedure call or argument")),
    })
}

fn date_diff(unit: &str, d1: f64, d2: f64) -> Result<i64, VbaError> {
    let (y1, m1, ..) = date_parts(d1);
    let (y2, m2, ..) = date_parts(d2);
    Ok(match unit {
        "yyyy" => i64::from(y2 - y1),
        "q" => (i64::from(y2) * 4 + i64::from((m2 - 1) / 3)) - (i64::from(y1) * 4 + i64::from((m1 - 1) / 3)),
        "m" => (i64::from(y2) * 12 + i64::from(m2)) - (i64::from(y1) * 12 + i64::from(m1)),
        "d" | "y" => (d2.floor() - d1.floor()) as i64,
        "w" | "ww" => ((d2.floor() - d1.floor()) / 7.0).trunc() as i64,
        "h" => ((d2 - d1) * 24.0).round() as i64,
        "n" => ((d2 - d1) * 1440.0).round() as i64,
        "s" => ((d2 - d1) * 86_400.0).round() as i64,
        _ => return Err(VbaError::runtime(5, "Invalid procedure call or argument")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(instr("abcabc", "c", 1, false), Some(3));
        assert_eq!(instr("abcabc", "c", 4, false), Some(6));
        assert_eq!(instr("abc", "X", 1, true), None);
        assert_eq!(instr("ABC", "b", 1, true), Some(2));
        assert_eq!(val("  12abc"), 12.0);
        assert_eq!(val("1.5e2x"), 150.0);
        assert_eq!(val("x"), 0.0);
        assert_eq!(val("&HFF"), 255.0);
        assert_eq!(replace_ci("aAa", "a", "b"), "bbb");
        assert_eq!(proper("hello wORLD"), "Hello World");
        assert_eq!(format_value(&Value::Double(1234.5), "#,##0.00").unwrap(), "1,234.50");
        assert_eq!(format_value(&Value::Date(43838.0), "dd/mm/yyyy").unwrap(), "08/01/2020");
        assert_eq!(date_add("m", 1.0, date_serial(2020, 1, 31)).unwrap(), date_serial(2020, 2, 29));
        assert_eq!(date_diff("d", 1.0, 11.0).unwrap(), 10);
        assert_eq!(constant("xlUp"), Some(Value::Int(-4162)));
        assert_eq!(constant("vbCrLf"), Some(Value::from("\r\n")));
    }
}
