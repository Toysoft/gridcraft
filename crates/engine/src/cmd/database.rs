//! Data › Get Data › From Database: run a query through the host's [`crate::datasource::DataSource`]
//! and put the result on a new sheet as a table.

use gridcraft_core::date::serial_from_ymd;
use gridcraft_core::{CellRef, MAX_COLS, RangeRef, Value};
use gridcraft_model::Cell;
use serde_json::{Value as Json, json};

use super::*;
use crate::datasource::{DEFAULT_MAX_ROWS, DEFAULT_TIMEOUT_SECS, DRIVERS, DbRequest, DbTable, DbValue};
use crate::{EngineError, Session};

/// Longest text a cell holds (Excel's limit).
const MAX_TEXT: usize = 32_767;
/// Integers beyond this lose digits as numbers, so they are kept as text.
const MAX_EXACT_INT: i64 = 1 << 53;

pub fn specs() -> Vec<CommandSpec> {
    vec![cmd!(
        "data.fromDatabase",
        "From Database",
        ["Data", "Get & Transform Data"],
        None,
        "{driver: postgres|mysql|sqlite|odbc, connection: URL, file path or ODBC connection string (no password), query: SQL, user?, password?, sheet?: new sheet name, maxRows?: 1..1048575, timeout?: seconds 1..3600} → imports the result into a new sheet as a table",
        has_doc,
        from_database
    )]
}

fn from_database(s: &mut Session, p: &Json) -> Result<Json> {
    const ID: &str = "data.fromDatabase";
    if ["driver", "connection", "query"].iter().all(|k| str_param(p, k).is_none_or(|v| v.trim().is_empty())) {
        s.ui_requests.push(crate::UiRequest::Dialog("fromDatabase".into(), json!({})));
        return Ok(Json::Null);
    }
    let driver = str_param(p, "driver").map(|d| d.trim().to_ascii_lowercase()).unwrap_or_default();
    if !DRIVERS.contains(&driver.as_str()) {
        return Err(bad(ID, format!("`driver` must be one of {}", DRIVERS.join(", "))));
    }
    let connection = str_param(p, "connection").map(str::trim).filter(|c| !c.is_empty()).ok_or_else(|| bad(ID, "missing `connection`"))?;
    let query = str_param(p, "query").map(str::trim).filter(|q| !q.is_empty()).ok_or_else(|| bad(ID, "missing `query`"))?;
    let max_rows =
        p.get("maxRows").and_then(Json::as_u64).map_or(DEFAULT_MAX_ROWS, |n| usize::try_from(n).unwrap_or(usize::MAX).clamp(1, DEFAULT_MAX_ROWS));
    let timeout_secs = p.get("timeout").and_then(Json::as_u64).map_or(DEFAULT_TIMEOUT_SECS, |t| t.clamp(1, 3600));
    let Some(source) = s.data_source.clone() else {
        return Err(EngineError::Other("Database connections are not available in this build.".into()));
    };
    let req = DbRequest {
        driver,
        connection: connection.to_string(),
        password: str_param(p, "password").filter(|x| !x.is_empty()).map(str::to_string),
        user: str_param(p, "user").map(str::trim).filter(|x| !x.is_empty()).map(str::to_string),
        query: query.to_string(),
        max_rows,
        timeout_secs,
    };
    let table = source(&req).map_err(EngineError::Other)?;
    let name = str_param(p, "sheet").map(|n| n.chars().take(31).collect::<String>()).unwrap_or_else(|| "Query".into());
    put_table(s, &table, &name, max_rows)
}

/// Writes `table` to a new sheet (header row + rows), formats date columns and makes it a table.
pub(crate) fn put_table(s: &mut Session, table: &DbTable, name: &str, max_rows: usize) -> Result<Json> {
    let w = table.columns.len().min(MAX_COLS as usize);
    if w == 0 {
        return Err(EngineError::Other("The query returned no columns.".into()));
    }
    let rows = &table.rows[..table.rows.len().min(max_rows).min(DEFAULT_MAX_ROWS)];
    let truncated = table.truncated || rows.len() < table.rows.len();
    let r = s.execute("home.insertSheet", json!({"name": name})).or_else(|_| s.execute("home.insertSheet", json!({})))?;
    let doc = s.doc()?;
    let sheet = doc.wb.active_sheet;
    let sys = doc.wb.date_system;
    // Unique, non-empty header names, so the table keeps every column.
    let mut headers: Vec<String> = Vec::with_capacity(w);
    for (i, c) in table.columns.iter().take(w).enumerate() {
        let base = if c.trim().is_empty() { format!("Column{}", i + 1) } else { c.chars().take(255).collect() };
        let mut h = base.clone();
        let mut n = 2;
        while headers.iter().any(|x| x.eq_ignore_ascii_case(&h)) {
            h = format!("{base}{n}");
            n += 1;
        }
        headers.push(h);
    }
    // Date/time number format per column, from its first non-null value.
    let mut formats: Vec<Option<&str>> = vec![None; w];
    for (ci, f) in formats.iter_mut().enumerate() {
        *f = rows.iter().filter_map(|row| row.get(ci)).find(|v| **v != DbValue::Null).and_then(|v| match v {
            DbValue::Date(..) => Some("yyyy-mm-dd"),
            DbValue::DateTime(..) => Some("yyyy-mm-dd hh:mm:ss"),
            DbValue::Time(..) => Some("hh:mm:ss"),
            _ => None,
        });
    }
    edit(s, |cx| {
        let sh = cx.sheet_mut(sheet)?;
        for (ci, h) in headers.iter().enumerate() {
            sh.set_cell(CellRef::new(0, ci as u32), Cell { value: Value::text(h.as_str()), formula: None, style: Default::default() });
        }
        for (ri, row) in rows.iter().enumerate() {
            for (ci, v) in row.iter().take(w).enumerate() {
                let value = cell_value(v, sys);
                if value != Value::Empty {
                    sh.set_cell(CellRef::new(ri as u32 + 1, ci as u32), Cell { value, formula: None, style: Default::default() });
                }
            }
        }
        cx.structural = true;
        Ok(())
    })?;
    let last_row = rows.len() as u32;
    for (ci, f) in formats.iter().enumerate() {
        if let (Some(code), true) = (f, last_row > 0) {
            let range = RangeRef::new(CellRef::new(1, ci as u32), CellRef::new(last_row, ci as u32));
            s.execute("home.numberFormat", json!({"range": range.a1(), "code": code}))?;
        }
    }
    let range = RangeRef::new(CellRef::new(0, 0), CellRef::new(last_row, w as u32 - 1));
    let _ = s.execute("insert.table", json!({"range": range.a1(), "header": true}));
    let _ = s.execute("home.autofitColumnWidth", json!({"cols": RangeRef::cols(0, w as u32 - 1).a1()}));
    Ok(json!({"sheet": r["name"], "rows": rows.len(), "cols": w, "truncated": truncated}))
}

fn cell_value(v: &DbValue, sys: gridcraft_core::date::DateSystem) -> Value {
    let text = |t: &str| Value::text(t.chars().take(MAX_TEXT).collect::<String>());
    let time = |h: u32, m: u32, sec: f64| (f64::from(h) * 3600.0 + f64::from(m) * 60.0 + sec) / 86_400.0;
    match v {
        DbValue::Null => Value::Empty,
        DbValue::Bool(b) => Value::Bool(*b),
        DbValue::Int(i) if (-MAX_EXACT_INT..=MAX_EXACT_INT).contains(i) => Value::number(*i as f64),
        DbValue::Int(i) => text(&i.to_string()),
        DbValue::Float(f) if f.is_finite() => Value::number(*f),
        DbValue::Float(f) => text(&f.to_string()),
        DbValue::Text(t) => text(t),
        DbValue::Date(y, m, d) => {
            serial_from_ymd(sys, i64::from(*y), i64::from(*m), i64::from(*d)).map_or_else(|| text(&format!("{y:04}-{m:02}-{d:02}")), Value::number)
        }
        DbValue::DateTime(y, mo, d, h, mi, sec) => serial_from_ymd(sys, i64::from(*y), i64::from(*mo), i64::from(*d))
            .map_or_else(|| text(&format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{sec:02.0}")), |day| Value::number(day + time(*h, *mi, *sec))),
        DbValue::Time(h, m, sec) => Value::number(time(*h, *m, *sec)),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::datasource::DataSource;

    fn session_with(table: DbTable) -> (Session, Arc<Mutex<Vec<DbRequest>>>) {
        let mut s = Session::new();
        s.execute("file.new", json!({})).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let source: DataSource = Arc::new(move |r: &DbRequest| {
            log.lock().unwrap().push(r.clone());
            Ok(table.clone())
        });
        s.data_source = Some(source);
        (s, seen)
    }

    fn sample() -> DbTable {
        DbTable {
            columns: vec!["id".into(), "name".into(), "born".into(), "".into(), "name".into()],
            rows: vec![
                vec![DbValue::Int(1), DbValue::Text("00123".into()), DbValue::Date(2024, 1, 5), DbValue::Bool(true), DbValue::Null],
                vec![DbValue::Int(i64::MAX), DbValue::Text("b".into()), DbValue::Null, DbValue::Float(f64::NAN), DbValue::Float(2.5)],
            ],
            truncated: false,
        }
    }

    fn text_at(s: &Session, a1: &str) -> String {
        let d = s.active().unwrap();
        let sh = d.wb.active().unwrap();
        crate::display::cell_text(&d.wb, sh, CellRef::parse(a1).unwrap())
    }

    #[test]
    fn imports_rows_into_a_new_table_sheet() {
        let (mut s, seen) = session_with(sample());
        let r = s
            .execute(
                "data.fromDatabase",
                json!({"driver": "SQLite", "connection": "/x.db", "query": "select 1", "password": "pw", "sheet": "People"}),
            )
            .unwrap();
        assert_eq!(r["sheet"], "People");
        assert_eq!(r["rows"], 2);
        assert_eq!(r["cols"], 5);
        let req = seen.lock().unwrap()[0].clone();
        assert_eq!(req.driver, "sqlite");
        assert_eq!(req.password.as_deref(), Some("pw"));
        let sh = s.active().unwrap().wb.active().unwrap();
        assert_eq!(sh.name, "People");
        assert_eq!(sh.tables.len(), 1);
        // Text stays text, even when it looks like a number.
        assert_eq!(sh.value(CellRef::parse("B2").unwrap()), Value::text("00123"));
        assert_eq!(text_at(&s, "C2"), "2024-01-05");
        // Blank and duplicate headers get unique names.
        assert_eq!(text_at(&s, "D1"), "Column4");
        assert_eq!(text_at(&s, "E1"), "name2");
        // Integers beyond 2^53 keep every digit.
        assert_eq!(sh.value(CellRef::parse("A3").unwrap()), Value::text(i64::MAX.to_string()));
        assert_eq!(sh.value(CellRef::parse("E3").unwrap()), Value::number(2.5));
    }

    #[test]
    fn password_is_left_out_of_the_journal() {
        let (mut s, _) = session_with(sample());
        s.execute("data.fromDatabase", json!({"driver": "postgres", "connection": "postgres://u@h/db", "query": "select 1", "password": "secret"}))
            .unwrap();
        let journaled = s.journal.iter().find(|(id, _)| id == "data.fromDatabase").unwrap();
        assert!(journaled.1.get("password").is_none());
    }

    #[test]
    fn max_rows_truncates_and_reports_it() {
        let (mut s, seen) = session_with(sample());
        let r = s.execute("data.fromDatabase", json!({"driver": "mysql", "connection": "mysql://u@h/db", "query": "q", "maxRows": 1})).unwrap();
        assert_eq!(r["rows"], 1);
        assert_eq!(r["truncated"], true);
        assert_eq!(seen.lock().unwrap()[0].max_rows, 1);
    }

    #[test]
    fn errors_without_host_bad_driver_or_driver_failure() {
        let mut s = Session::new();
        s.execute("file.new", json!({})).unwrap();
        let p = json!({"driver": "sqlite", "connection": "/x.db", "query": "select 1"});
        assert!(s.execute("data.fromDatabase", p.clone()).unwrap_err().to_string().contains("not available"));
        assert!(s.execute("data.fromDatabase", json!({"driver": "oracle", "connection": "x", "query": "q"})).is_err());
        assert!(s.execute("data.fromDatabase", json!({"driver": "sqlite", "connection": "/x.db"})).is_err());
        s.data_source = Some(Arc::new(|_: &DbRequest| Err("connection refused".to_string())));
        assert!(s.execute("data.fromDatabase", p).unwrap_err().to_string().contains("connection refused"));
        let (mut s, _) = session_with(DbTable::default());
        assert!(s.execute("data.fromDatabase", json!({"driver": "sqlite", "connection": "/x.db", "query": "q"})).is_err());
    }

    #[test]
    fn empty_params_open_the_dialog_but_headless_calls_fail() {
        let (mut s, seen) = session_with(sample());
        s.execute("data.fromDatabase", json!({})).unwrap();
        assert!(s.take_ui_requests().iter().any(|q| matches!(q, crate::UiRequest::Dialog(n, _) if n == "fromDatabase")));
        assert!(s.execute_headless("data.fromDatabase", json!({})).is_err());
        assert!(seen.lock().unwrap().is_empty());
    }
}
