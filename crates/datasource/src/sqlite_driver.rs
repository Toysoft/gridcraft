//! SQLite files (bundled SQLite), opened read-only.

use std::time::{Duration, Instant};

use gridcraft_engine::datasource::{DbRequest, DbTable, DbValue};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};

pub fn run(req: &DbRequest) -> Result<DbTable, String> {
    let path = req.connection.strip_prefix("sqlite://").or_else(|| req.connection.strip_prefix("sqlite:")).unwrap_or(&req.connection);
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI | OpenFlags::SQLITE_OPEN_NO_MUTEX)
        .map_err(|e| format!("Could not open the SQLite database: {e}"))?;
    let timeout = Duration::from_secs(req.timeout_secs);
    conn.busy_timeout(timeout).map_err(|e| format!("SQLite: {e}"))?;
    // Interrupt a query that runs past the timeout. This module is native only, so the clock is safe.
    #[allow(clippy::disallowed_methods)]
    let deadline = Instant::now() + timeout;
    #[allow(clippy::disallowed_methods)]
    let past_deadline = move || Instant::now() > deadline;
    conn.progress_handler(10_000, Some(past_deadline)).map_err(|e| format!("SQLite: {e}"))?;
    let mut stmt = conn.prepare(&req.query).map_err(|e| format!("SQLite: {e}"))?;
    let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
    let decl: Vec<String> = stmt.columns().iter().map(|c| c.decl_type().unwrap_or("").to_ascii_uppercase()).collect();
    let n = columns.len();
    let mut rows = stmt.query([]).map_err(|e| format!("SQLite: {e}"))?;
    let mut table = DbTable { columns, rows: Vec::new(), truncated: false };
    while let Some(row) = rows.next().map_err(|e| format!("SQLite: {e}"))? {
        if table.rows.len() >= req.max_rows {
            table.truncated = true;
            break;
        }
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let v = row.get_ref(i).map_err(|e| format!("SQLite: {e}"))?;
            out.push(value(v, decl.get(i).map_or("", String::as_str)));
        }
        table.rows.push(out);
    }
    Ok(table)
}

fn value(v: ValueRef<'_>, decl: &str) -> DbValue {
    match v {
        ValueRef::Null => DbValue::Null,
        ValueRef::Integer(n) if decl.contains("BOOL") => DbValue::Bool(n != 0),
        ValueRef::Integer(n) => DbValue::Int(n),
        ValueRef::Real(f) => DbValue::Float(f),
        ValueRef::Text(t) => {
            let s = String::from_utf8_lossy(t).into_owned();
            // SQLite has no date type: columns declared DATE/TIME hold ISO text.
            if decl.contains("DATE") || decl.contains("TIME") { crate::parse_temporal(&s).unwrap_or(DbValue::Text(s)) } else { DbValue::Text(s) }
        }
        ValueRef::Blob(b) => DbValue::Text(format!("({} bytes)", b.len())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> (std::path::PathBuf, Connection) {
        let dir = std::env::temp_dir().join(format!("gridcraft-ds-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        let _ = std::fs::remove_file(&path);
        let c = Connection::open(&path).unwrap();
        c.execute_batch(
            "CREATE TABLE t (id INTEGER, name TEXT, price REAL, born DATE, seen DATETIME, ok BOOLEAN, pic BLOB);
             INSERT INTO t VALUES (1, 'a', 2.5, '2024-01-05', '2024-01-05 10:30:00', 1, x'0102');
             INSERT INTO t VALUES (2, NULL, NULL, 'not a date', NULL, 0, NULL);
             INSERT INTO t VALUES (3, 'c', 1, '2024-02-29', NULL, NULL, NULL);",
        )
        .unwrap();
        (path, c)
    }

    fn req(path: &std::path::Path, q: &str, max_rows: usize) -> DbRequest {
        DbRequest {
            driver: "sqlite".into(),
            connection: path.display().to_string(),
            password: None,
            user: None,
            query: q.into(),
            max_rows,
            timeout_secs: 5,
        }
    }

    #[test]
    fn reads_typed_rows() {
        let (path, _c) = db();
        let t = run(&req(&path, "SELECT * FROM t ORDER BY id", 100)).unwrap();
        assert_eq!(t.columns, ["id", "name", "price", "born", "seen", "ok", "pic"]);
        assert_eq!(t.rows.len(), 3);
        assert!(!t.truncated);
        assert_eq!(
            t.rows[0],
            vec![
                DbValue::Int(1),
                DbValue::Text("a".into()),
                DbValue::Float(2.5),
                DbValue::Date(2024, 1, 5),
                DbValue::DateTime(2024, 1, 5, 10, 30, 0.0),
                DbValue::Bool(true),
                DbValue::Text("(2 bytes)".into()),
            ]
        );
        assert_eq!(t.rows[1][1], DbValue::Null);
        assert_eq!(t.rows[1][3], DbValue::Text("not a date".into()));
    }

    #[test]
    fn max_rows_truncates() {
        let (path, _c) = db();
        let t = run(&req(&path, "SELECT id FROM t", 2)).unwrap();
        assert_eq!(t.rows.len(), 2);
        assert!(t.truncated);
    }

    #[test]
    fn read_only_and_errors() {
        let (path, _c) = db();
        assert!(run(&req(&path, "DELETE FROM t", 10)).is_err());
        assert!(run(&req(&path, "SELECT nope FROM t", 10)).is_err());
        assert!(run(&req(std::path::Path::new("/nonexistent/x.db"), "SELECT 1", 10)).is_err());
    }

    #[test]
    #[allow(clippy::disallowed_methods)]
    fn runaway_query_is_interrupted() {
        let (path, _c) = db();
        let mut r = req(&path, "WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c) SELECT count(*) FROM c", 10);
        r.timeout_secs = 1;
        let started = Instant::now();
        assert!(run(&r).is_err());
        assert!(started.elapsed() < Duration::from_secs(10));
    }
}
