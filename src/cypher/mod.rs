//! # Cypher Language
//!
//! Full openCypher parser producing a clean AST.
//! Pure functions — no I/O, no state, no storage dependency.

pub mod ast;
pub mod lexer;
pub mod parser;

use crate::{Error, Result};
use ast::Statement;

/// Parse a Cypher query string into an AST.
pub fn parse(query: &str) -> Result<Statement> {
    let tokens = lexer::tokenize(query)?;
    parser::parse_statement(&tokens)
}
