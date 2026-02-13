//! Cypher recursive descent parser.
//!
//! TODO: Full implementation. This is the architectural placeholder.
//! The lexer is functional; the parser needs the full openCypher grammar.

use crate::{Error, Result};
use super::ast::*;
use super::lexer::{Token, TokenKind};

/// Parse a complete Cypher statement from tokens.
pub fn parse_statement(_tokens: &[Token]) -> Result<Statement> {
    // TODO: Full recursive descent parser
    // This is the highest-priority implementation task after the architecture
    // is validated. The AST types in ast.rs define the target structure.
    Err(Error::SyntaxError {
        position: 0,
        message: "Parser not yet implemented — AST types and lexer are complete".into(),
    })
}
