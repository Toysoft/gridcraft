//! The Excel formula language: tokenizer, parser, syntax tree, printer (A1 and R1C1) and
//! reference adjustment for copy/fill and structural edits.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod adjust;
pub mod ast;
mod input;
pub mod lexer;
pub mod parser;
pub mod printer;
pub mod table_ref;

pub use ast::{Anchor, BinOp, Expr, RefKind, Reference, SheetSel, StructItem, StructRef, TrimMode, UnOp};
pub use input::{FormulaLocale, parse_input, print_input};
pub use parser::{ParseError, normalize_function_name, parse};
pub use printer::{print, print_r1c1, quote_sheet};
pub use table_ref::{ColumnSpan, Condition, RowSel, TableRef, lower_extensions, raise_extension};

#[cfg(test)]
mod tests;
