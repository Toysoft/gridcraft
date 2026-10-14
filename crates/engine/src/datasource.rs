//! External databases (Data › Get Data › From Database).
//!
//! The engine stays network-free and wasm-clean: the host (desktop app, CLI) injects a
//! [`DataSource`] into [`crate::Session::data_source`] that runs a query and returns a
//! [`DbTable`]. Without one, `data.fromDatabase` returns an error.

use std::sync::Arc;

/// Rows imported by default (the sheet's row count minus the header row).
pub const DEFAULT_MAX_ROWS: usize = 1_048_575;
/// Columns imported at most (the sheet's column count).
pub const MAX_COLS: usize = 16_384;
/// Connect and query timeout, in seconds, when the caller gives none.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// One query against one database.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DbRequest {
    /// `postgres`, `mysql`, `sqlite` or `odbc`.
    pub driver: String,
    /// URL, file path or ODBC connection string, without the password.
    pub connection: String,
    /// Never stored in the workbook or the journal.
    pub password: Option<String>,
    pub user: Option<String>,
    pub query: String,
    pub max_rows: usize,
    pub timeout_secs: u64,
}

/// A database value, already reduced to what a cell can hold.
#[derive(Clone, Debug, PartialEq)]
pub enum DbValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    /// Calendar date (year, month, day).
    Date(i32, u32, u32),
    /// Date and time (year, month, day, hour, minute, second with fraction).
    DateTime(i32, u32, u32, u32, u32, f64),
    /// Time of day (hour, minute, second with fraction).
    Time(u32, u32, f64),
}

/// A query result: column names and rows (each row as long as `columns` or shorter).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DbTable {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<DbValue>>,
    /// More rows were available than `max_rows`.
    pub truncated: bool,
}

/// Runs a query. Implementations must honour `max_rows` and `timeout_secs` and never panic.
pub type DataSource = Arc<dyn Fn(&DbRequest) -> Result<DbTable, String> + Send + Sync>;

/// Drivers a host may support; the command rejects anything else before calling the host.
pub const DRIVERS: &[&str] = &["postgres", "mysql", "sqlite", "odbc"];
