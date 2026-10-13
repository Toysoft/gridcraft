//! GridCraft VBA: reads the macros a workbook carries and runs them.
//!
//! - [`ovba`]: the VBA project storage (module list and source).
//! - [`lexer`], [`parser`], [`ast`]: the VBA language.
//! - [`value`]: Variants (the [`Value`] type).
//!
//! Macros only reach the outside world through the [`interp::Host`] trait the caller implements
//! (GridCraft's Excel object model and UserForms). There is no file, shell, registry, network or
//! DLL access: `Declare`d functions return 0, and `Shell`, `Kill`, `CreateObject` and friends are
//! errors. Running is opt-in per workbook (the caller's "Enable Content").
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod ast;
pub mod builtins;
pub mod forms;
pub mod interp;
pub mod lexer;
pub mod ovba;
pub mod parser;
pub mod value;

pub use interp::{Event, Host, HostArg, Interp, MODAL};
pub use value::{Array, HostObj, Object, Value};

/// Errors: a source that can't be read or parsed, or a run-time error with VBA's error number.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum VbaError {
    #[error("damaged VBA project: {0}")]
    Format(&'static str),
    #[error("syntax error on line {line}: {msg}")]
    Syntax { line: u32, msg: String },
    #[error("run-time error {number}: {description}")]
    Runtime { number: i64, description: String },
    /// `End` ran, or the run was stopped: unwinds every procedure.
    #[error("the macro was stopped")]
    Stopped,
}

impl VbaError {
    pub fn runtime(number: i64, description: impl Into<String>) -> VbaError {
        VbaError::Runtime { number, description: description.into() }
    }
}
