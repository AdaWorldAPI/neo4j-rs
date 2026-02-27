//! Cypher recursive descent parser.
//!
//! Parses token streams into AST nodes. Supports:
//! - MATCH / OPTIONAL MATCH with patterns
//! - WHERE, RETURN, ORDER BY, SKIP, LIMIT
//! - CREATE, DELETE / DETACH DELETE, SET
//! - CALL ... YIELD
//! - Full expression parsing with precedence

use crate::{Error, Result};
use super::ast::*;
use super::lexer::{Token, TokenKind};
use std::collections::HashMap;

/// Parser state — wraps a token slice with cursor.
struct Parser<'t> {
    tokens: &'t [Token],
    pos: usize,
}

impl<'t> Parser<'t> {
    fn new(tokens: &'t [Token]) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos.min(self.tokens.len() - 1)]
    }

    fn peek_kind(&self) -> TokenKind {
        self.peek().kind
    }

    fn advance(&mut self) -> &Token {
        let tok = &self.tokens[self.pos.min(self.tokens.len() - 1)];
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, kind: TokenKind) -> Result<&Token> {
        let tok = self.peek();
        if tok.kind == kind {
            Ok(self.advance())
        } else {
            Err(self.error(format!("Expected {:?}, got {:?} '{}'", kind, tok.kind, tok.text)))
        }
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.peek_kind() == kind
    }

    fn _at_eof(&self) -> bool {
        self.at(TokenKind::Eof) || self.at(TokenKind::Semicolon)
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn error(&self, msg: String) -> Error {
        Error::SyntaxError {
            position: self.peek().span.start,
            message: msg,
        }
    }

    /// Check if current token is a keyword that starts a new clause.
    fn _at_clause_start(&self) -> bool {
        matches!(self.peek_kind(),
            TokenKind::Match | TokenKind::OptionalMatch | TokenKind::Where |
            TokenKind::Return | TokenKind::With | TokenKind::Create |
            TokenKind::Delete | TokenKind::DetachDelete | TokenKind::Set |
            TokenKind::Remove | TokenKind::Order | TokenKind::Skip |
            TokenKind::Limit | TokenKind::Unwind | TokenKind::Call |
            TokenKind::Merge | TokenKind::Eof | TokenKind::Semicolon
        )
    }
}

/// Parse a complete Cypher statement from tokens.
pub fn parse_statement(tokens: &[Token]) -> Result<Statement> {
    let mut p = Parser::new(tokens);

    let stmt = match p.peek_kind() {
        TokenKind::Match | TokenKind::OptionalMatch => parse_query_stmt(&mut p)?,
        TokenKind::Create => {
            // Peek ahead: CREATE INDEX / CREATE CONSTRAINT → schema
            let saved = p.pos;
            p.advance(); // eat CREATE
            if p.at(TokenKind::Index) || p.at(TokenKind::Constraint) {
                p.pos = saved;
                parse_schema_stmt(&mut p)?
            } else {
                p.pos = saved;
                parse_create_stmt(&mut p)?
            }
        }
        TokenKind::Merge => parse_merge_stmt(&mut p)?,
        TokenKind::Delete | TokenKind::DetachDelete => parse_delete_stmt(&mut p)?,
        TokenKind::Call => parse_call_stmt(&mut p)?,
        TokenKind::Drop => parse_schema_stmt(&mut p)?,
        kind => {
            // Try to parse as a query with UNWIND or WITH as starting clause
            if kind == TokenKind::Unwind || kind == TokenKind::With {
                parse_query_stmt(&mut p)?
            } else {
                return Err(p.error(format!("Unexpected token {:?} at start of statement", kind)));
            }
        }
    };

    // Allow optional semicolon + EOF
    p.eat(TokenKind::Semicolon);
    if !p.at(TokenKind::Eof) {
        return Err(p.error(format!("Unexpected token after statement: {:?}", p.peek_kind())));
    }

    Ok(stmt)
}

// ============================================================================
// Statement parsers
// ============================================================================

fn parse_query_stmt(p: &mut Parser) -> Result<Statement> {
    let mut matches = Vec::new();
    let mut where_clause = None;
    let mut with_clauses: Vec<WithClause> = Vec::new();

    // Parse MATCH/WITH clauses in a loop to allow interleaving
    loop {
        // Parse MATCH clauses
        while p.at(TokenKind::Match) || p.at(TokenKind::OptionalMatch) {
            let optional = if p.at(TokenKind::OptionalMatch) {
                p.advance(); // consume OPTIONAL
                // Check if next is MATCH
                if p.at(TokenKind::Match) {
                    p.advance();
                }
                true
            } else {
                p.advance(); // consume MATCH
                false
            };

            let patterns = parse_pattern_list(p)?;
            matches.push(MatchClause { optional, patterns });

            // WHERE after MATCH
            if p.at(TokenKind::Where) {
                p.advance();
                where_clause = Some(parse_expr(p)?);
            }
        }

        // Check for WITH clause
        if p.at(TokenKind::With) {
            p.advance();
            let with = parse_with_clause(p)?;
            with_clauses.push(with);
            // After WITH, continue to parse more MATCH/WITH/RETURN clauses
            continue;
        }

        break;
    }

    // If we hit SET after MATCH, it's a MATCH...SET
    if p.at(TokenKind::Set) {
        p.advance();
        let items = parse_set_items(p)?;
        let return_clause = if p.at(TokenKind::Return) {
            p.advance();
            Some(parse_return_clause(p)?)
        } else {
            None
        };
        return Ok(Statement::Set(SetClause {
            matches,
            where_clause,
            items,
            return_clause,
        }));
    }

    // If we hit DELETE/DETACH DELETE after MATCH, it's a MATCH...DELETE
    if p.at(TokenKind::Delete) || p.at(TokenKind::DetachDelete) {
        let detach = p.at(TokenKind::DetachDelete);
        p.advance();
        if detach && p.at(TokenKind::Delete) {
            p.advance(); // consume DELETE after DETACH
        }
        let variables = parse_variable_list(p)?;
        return Ok(Statement::Delete(DeleteClause {
            matches,
            where_clause,
            variables,
            detach,
        }));
    }

    // If we hit REMOVE after MATCH, it's a MATCH...REMOVE
    if p.at(TokenKind::Remove) {
        return parse_remove_after_match(p, matches, where_clause);
    }

    // Must have RETURN
    if !p.at(TokenKind::Return) {
        return Err(p.error("Expected RETURN clause".into()));
    }
    p.advance();

    let return_clause = parse_return_clause(p)?;

    // ORDER BY
    let order_by = if p.at(TokenKind::Order) {
        p.advance();
        p.expect(TokenKind::By)?;
        Some(parse_order_by(p)?)
    } else {
        None
    };

    // SKIP
    let skip = if p.at(TokenKind::Skip) {
        p.advance();
        Some(parse_expr(p)?)
    } else {
        None
    };

    // LIMIT
    let limit = if p.at(TokenKind::Limit) {
        p.advance();
        Some(parse_expr(p)?)
    } else {
        None
    };

    Ok(Statement::Query(Query {
        matches,
        where_clause,
        with_clauses,
        return_clause,
        order_by,
        skip,
        limit,
    }))
}

fn parse_create_stmt(p: &mut Parser) -> Result<Statement> {
    p.expect(TokenKind::Create)?;
    let patterns = parse_pattern_list(p)?;

    let return_clause = if p.at(TokenKind::Return) {
        p.advance();
        Some(parse_return_clause(p)?)
    } else {
        None
    };

    Ok(Statement::Create(CreateClause { patterns, return_clause }))
}

fn parse_merge_stmt(p: &mut Parser) -> Result<Statement> {
    p.expect(TokenKind::Merge)?;
    let patterns = parse_pattern_list(p)?;

    // The first pattern is the merge pattern
    let pattern = patterns.into_iter().next()
        .ok_or_else(|| p.error("MERGE requires a pattern".into()))?;

    // Parse ON CREATE SET and ON MATCH SET
    let mut on_create = Vec::new();
    let mut on_match = Vec::new();

    while p.at(TokenKind::On) {
        p.advance(); // consume ON
        // ON CREATE SET or ON MATCH SET
        // CREATE and MATCH are keywords, so they get their own TokenKind
        if p.at(TokenKind::Create) {
            p.advance(); // consume CREATE
            p.expect(TokenKind::Set)?;
            on_create.extend(parse_set_items(p)?);
        } else if p.at(TokenKind::Match) {
            p.advance(); // consume MATCH
            p.expect(TokenKind::Set)?;
            on_match.extend(parse_set_items(p)?);
        } else {
            return Err(p.error(format!("Expected CREATE or MATCH after ON, got '{}'", p.peek().text)));
        }
    }

    let return_clause = if p.at(TokenKind::Return) {
        p.advance();
        Some(parse_return_clause(p)?)
    } else {
        None
    };

    Ok(Statement::Merge(MergeClause {
        pattern,
        on_create,
        on_match,
        return_clause,
    }))
}

fn parse_schema_stmt(p: &mut Parser) -> Result<Statement> {
    if p.at(TokenKind::Create) {
        p.advance(); // CREATE
        if p.at(TokenKind::Index) {
            p.advance(); // INDEX
            // CREATE INDEX [name] FOR (n:Label) ON (n.property)
            // or CREATE INDEX ON :Label(property)
            parse_create_index(p)
        } else if p.at(TokenKind::Constraint) {
            p.advance(); // CONSTRAINT
            parse_create_constraint(p)
        } else {
            Err(p.error("Expected INDEX or CONSTRAINT after CREATE".into()))
        }
    } else if p.at(TokenKind::Drop) {
        p.advance(); // DROP
        if p.at(TokenKind::Index) {
            p.advance(); // INDEX
            parse_drop_index(p)
        } else if p.at(TokenKind::Constraint) {
            p.advance(); // CONSTRAINT
            parse_drop_constraint(p)
        } else {
            Err(p.error("Expected INDEX or CONSTRAINT after DROP".into()))
        }
    } else {
        Err(p.error("Expected CREATE or DROP for schema command".into()))
    }
}

fn parse_create_index(p: &mut Parser) -> Result<Statement> {
    // CREATE INDEX [name] FOR (n:Label) ON (n.property)
    // or simplified: CREATE INDEX ON :Label(property)

    // Optional index name (identifier)
    let _name = if p.at(TokenKind::Identifier) && !p.at(TokenKind::On) && !p.at(TokenKind::For) {
        let tok = p.advance();
        Some(tok.text.clone())
    } else {
        None
    };

    // Optional index type: BTREE | TEXT | RANGE | POINT | VECTOR
    let index_type = None;

    if p.at(TokenKind::On) {
        p.advance(); // ON
        // :Label(property) syntax
        p.expect(TokenKind::Colon)?;
        let label_tok = p.advance();
        let label = label_tok.text.clone();
        p.expect(TokenKind::LParen)?;
        let prop_tok = p.advance();
        let property = prop_tok.text.clone();
        p.expect(TokenKind::RParen)?;

        return Ok(Statement::Schema(SchemaCommand::CreateIndex {
            label,
            property,
            index_type,
        }));
    }

    if p.at(TokenKind::For) {
        p.advance(); // FOR
        p.expect(TokenKind::LParen)?;
        // (n:Label)
        let _alias = p.advance(); // variable
        p.expect(TokenKind::Colon)?;
        let label_tok = p.advance();
        let label = label_tok.text.clone();
        p.expect(TokenKind::RParen)?;

        p.expect(TokenKind::On)?;
        p.expect(TokenKind::LParen)?;
        // (n.property)
        let _alias2 = p.advance(); // variable
        p.expect(TokenKind::Dot)?;
        let prop_tok = p.advance();
        let property = prop_tok.text.clone();
        p.expect(TokenKind::RParen)?;

        // Optional OPTIONS
        if p.at(TokenKind::Identifier) && p.peek().text.eq_ignore_ascii_case("OPTIONS") {
            p.advance();
            // Skip options block for now
            if p.at(TokenKind::LBrace) {
                let _ = skip_braced(p);
            }
        }

        return Ok(Statement::Schema(SchemaCommand::CreateIndex {
            label,
            property,
            index_type,
        }));
    }

    Err(p.error("Expected ON or FOR after CREATE INDEX".into()))
}

fn parse_create_constraint(p: &mut Parser) -> Result<Statement> {
    // CREATE CONSTRAINT [name] FOR (n:Label) REQUIRE n.property IS UNIQUE
    // or CREATE CONSTRAINT ON (n:Label) ASSERT n.property IS UNIQUE

    // Optional name
    let _name = if p.at(TokenKind::Identifier)
        && !p.at(TokenKind::On)
        && !p.at(TokenKind::For)
    {
        let tok = p.advance();
        Some(tok.text.clone())
    } else {
        None
    };

    // FOR or ON
    if p.at(TokenKind::For) || p.at(TokenKind::On) {
        p.advance();
    } else {
        return Err(p.error("Expected FOR or ON after CONSTRAINT [name]".into()));
    }

    p.expect(TokenKind::LParen)?;
    let _alias = p.advance(); // variable name
    p.expect(TokenKind::Colon)?;
    let label_tok = p.advance();
    let label = label_tok.text.clone();
    p.expect(TokenKind::RParen)?;

    // REQUIRE or ASSERT (these are identifier tokens, not keywords)
    let _req_tok = p.advance(); // REQUIRE / ASSERT
    let _alias2 = p.advance(); // variable
    p.expect(TokenKind::Dot)?;
    let prop_tok = p.advance();
    let property = prop_tok.text.clone();

    // IS [NOT NULL | UNIQUE]
    let constraint_type = if p.at(TokenKind::Is) {
        p.advance(); // IS
        let type_tok = p.advance();
        type_tok.text.to_uppercase()
    } else {
        "UNIQUE".to_string()
    };

    Ok(Statement::Schema(SchemaCommand::CreateConstraint {
        label,
        property,
        constraint_type,
    }))
}

fn parse_drop_index(p: &mut Parser) -> Result<Statement> {
    // DROP INDEX ON :Label(property)
    // or DROP INDEX name

    if p.at(TokenKind::On) {
        p.advance();
        p.expect(TokenKind::Colon)?;
        let label_tok = p.advance();
        let label = label_tok.text.clone();
        p.expect(TokenKind::LParen)?;
        let prop_tok = p.advance();
        let property = prop_tok.text.clone();
        p.expect(TokenKind::RParen)?;
        Ok(Statement::Schema(SchemaCommand::DropIndex { label, property }))
    } else {
        // DROP INDEX name — we need the index name to resolve to label/property
        let name_tok = p.advance();
        Ok(Statement::Schema(SchemaCommand::DropIndex {
            label: name_tok.text.clone(),
            property: String::new(),
        }))
    }
}

fn parse_drop_constraint(p: &mut Parser) -> Result<Statement> {
    // DROP CONSTRAINT ON (n:Label) ASSERT n.property IS UNIQUE
    // or DROP CONSTRAINT name

    if p.at(TokenKind::On) {
        p.advance();
        p.expect(TokenKind::LParen)?;
        let _alias = p.advance();
        p.expect(TokenKind::Colon)?;
        let label_tok = p.advance();
        let label = label_tok.text.clone();
        p.expect(TokenKind::RParen)?;
        // Skip ASSERT ... IS UNIQUE/NOT NULL
        while !p.at(TokenKind::Eof) && !p.at(TokenKind::Semicolon) {
            p.advance();
        }
        Ok(Statement::Schema(SchemaCommand::DropConstraint {
            label,
            property: String::new(),
        }))
    } else {
        let name_tok = p.advance();
        Ok(Statement::Schema(SchemaCommand::DropConstraint {
            label: name_tok.text.clone(),
            property: String::new(),
        }))
    }
}

/// Skip a brace-delimited block `{ ... }`.
fn skip_braced(p: &mut Parser) -> Result<()> {
    p.expect(TokenKind::LBrace)?;
    let mut depth = 1u32;
    while depth > 0 && !p.at(TokenKind::Eof) {
        if p.at(TokenKind::LBrace) { depth += 1; }
        if p.at(TokenKind::RBrace) { depth -= 1; }
        if depth > 0 { p.advance(); }
    }
    p.expect(TokenKind::RBrace)?;
    Ok(())
}

fn parse_delete_stmt(p: &mut Parser) -> Result<Statement> {
    let detach = p.at(TokenKind::DetachDelete);
    p.advance();
    if detach && p.at(TokenKind::Delete) {
        p.advance();
    }
    let variables = parse_variable_list(p)?;

    Ok(Statement::Delete(DeleteClause {
        matches: Vec::new(),
        where_clause: None,
        variables,
        detach,
    }))
}

fn parse_call_stmt(p: &mut Parser) -> Result<Statement> {
    p.expect(TokenKind::Call)?;

    // Parse procedure name: name or name.name.name
    let mut name = p.expect(TokenKind::Identifier)?.text.clone();
    while p.eat(TokenKind::Dot) {
        let part = p.expect(TokenKind::Identifier)?.text.clone();
        name = format!("{name}.{part}");
    }

    // Arguments
    p.expect(TokenKind::LParen)?;
    let mut args = Vec::new();
    if !p.at(TokenKind::RParen) {
        args.push(parse_expr(p)?);
        while p.eat(TokenKind::Comma) {
            args.push(parse_expr(p)?);
        }
    }
    p.expect(TokenKind::RParen)?;

    // YIELD
    let mut yields = Vec::new();
    if p.eat(TokenKind::Yield) {
        yields.push(p.expect(TokenKind::Identifier)?.text.clone());
        while p.eat(TokenKind::Comma) {
            yields.push(p.expect(TokenKind::Identifier)?.text.clone());
        }
    }

    // Build a Query wrapping the CALL
    let return_items: Vec<ReturnItem> = yields.iter().map(|y| ReturnItem {
        expr: Expr::Variable(y.clone()),
        alias: None,
    }).collect();

    let return_clause = if return_items.is_empty() {
        ReturnClause { distinct: false, items: vec![ReturnItem { expr: Expr::Star, alias: None }] }
    } else {
        ReturnClause { distinct: false, items: return_items }
    };

    // For now, wrap as Query with no MATCH
    Ok(Statement::Query(Query {
        matches: Vec::new(),
        where_clause: None,
        with_clauses: Vec::new(),
        return_clause,
        order_by: None,
        skip: None,
        limit: None,
    }))
}

// ============================================================================
// WITH clause parsing
// ============================================================================

fn parse_with_clause(p: &mut Parser) -> Result<WithClause> {
    // Parse return items (same syntax as RETURN items)
    let mut items = Vec::new();
    if p.at(TokenKind::Star) {
        p.advance();
        items.push(ReturnItem { expr: Expr::Star, alias: None });
    } else {
        items.push(parse_return_item(p)?);
        while p.eat(TokenKind::Comma) {
            items.push(parse_return_item(p)?);
        }
    }

    // Optional WHERE after WITH items
    let where_clause = if p.at(TokenKind::Where) {
        p.advance();
        Some(parse_expr(p)?)
    } else {
        None
    };

    Ok(WithClause { items, where_clause })
}

// ============================================================================
// REMOVE statement parsing
// ============================================================================

fn _parse_remove_stmt(p: &mut Parser) -> Result<Statement> {
    let mut matches = Vec::new();
    let mut where_clause = None;

    // Parse MATCH clauses
    while p.at(TokenKind::Match) || p.at(TokenKind::OptionalMatch) {
        let optional = if p.at(TokenKind::OptionalMatch) {
            p.advance();
            if p.at(TokenKind::Match) {
                p.advance();
            }
            true
        } else {
            p.advance();
            false
        };

        let patterns = parse_pattern_list(p)?;
        matches.push(MatchClause { optional, patterns });

        if p.at(TokenKind::Where) {
            p.advance();
            where_clause = Some(parse_expr(p)?);
        }
    }

    p.expect(TokenKind::Remove)?;
    let items = parse_remove_items(p)?;

    let return_clause = if p.at(TokenKind::Return) {
        p.advance();
        Some(parse_return_clause(p)?)
    } else {
        None
    };

    Ok(Statement::Remove(RemoveClause {
        matches,
        where_clause,
        items,
        return_clause,
    }))
}

fn parse_remove_after_match(p: &mut Parser, matches: Vec<MatchClause>, where_clause: Option<Expr>) -> Result<Statement> {
    p.expect(TokenKind::Remove)?;
    let items = parse_remove_items(p)?;

    let return_clause = if p.at(TokenKind::Return) {
        p.advance();
        Some(parse_return_clause(p)?)
    } else {
        None
    };

    Ok(Statement::Remove(RemoveClause {
        matches,
        where_clause,
        items,
        return_clause,
    }))
}

fn parse_remove_items(p: &mut Parser) -> Result<Vec<RemoveItem>> {
    let mut items = Vec::new();
    items.push(parse_remove_item(p)?);
    while p.eat(TokenKind::Comma) {
        items.push(parse_remove_item(p)?);
    }
    Ok(items)
}

fn parse_remove_item(p: &mut Parser) -> Result<RemoveItem> {
    let name = p.expect(TokenKind::Identifier)?.text.clone();

    if p.eat(TokenKind::Dot) {
        // REMOVE n.prop
        let key = p.expect(TokenKind::Identifier)?.text.clone();
        Ok(RemoveItem::Property { variable: name, key })
    } else if p.at(TokenKind::Colon) {
        // REMOVE n:Label
        p.advance();
        let label = p.expect(TokenKind::Identifier)?.text.clone();
        Ok(RemoveItem::Label { variable: name, label })
    } else {
        Err(p.error("Expected '.' or ':' after REMOVE variable".into()))
    }
}

// ============================================================================
// Pattern parsing
// ============================================================================

fn parse_pattern_list(p: &mut Parser) -> Result<Vec<Pattern>> {
    let mut patterns = Vec::new();
    patterns.push(parse_pattern(p)?);
    while p.eat(TokenKind::Comma) {
        patterns.push(parse_pattern(p)?);
    }
    Ok(patterns)
}

fn parse_pattern(p: &mut Parser) -> Result<Pattern> {
    let mut elements = Vec::new();

    // A pattern starts with a node
    elements.push(PatternElement::Node(parse_node_pattern(p)?));

    // Then alternating: relationship, node, relationship, node, ...
    while p.at(TokenKind::Dash) || p.at(TokenKind::LeftArrow) {
        let (rel, _dir_hint) = parse_rel_pattern(p)?;
        elements.push(PatternElement::Relationship(rel));
        elements.push(PatternElement::Node(parse_node_pattern(p)?));
    }

    Ok(Pattern { elements })
}

fn parse_node_pattern(p: &mut Parser) -> Result<NodePattern> {
    p.expect(TokenKind::LParen)?;

    let mut alias = None;
    let mut labels = Vec::new();
    let mut properties = HashMap::new();

    // Optional alias
    if p.at(TokenKind::Identifier) {
        alias = Some(p.advance().text.clone());
    }

    // Labels: :Label1:Label2
    while p.at(TokenKind::Colon) {
        p.advance();
        let label = p.expect(TokenKind::Identifier)?.text.clone();
        labels.push(label);
    }

    // Properties: {key: value, ...}
    if p.at(TokenKind::LBrace) {
        properties = parse_map_literal_inner(p)?;
    }

    p.expect(TokenKind::RParen)?;

    Ok(NodePattern { alias, labels, properties })
}

fn parse_rel_pattern(p: &mut Parser) -> Result<(RelPattern, PatternDirection)> {
    let direction;

    // <-[...]- or -[...]-> or -[...]-
    let left_arrow = p.eat(TokenKind::LeftArrow);
    if !left_arrow {
        p.expect(TokenKind::Dash)?;
    }

    let mut alias = None;
    let mut rel_types = Vec::new();
    let mut properties = HashMap::new();
    let mut var_length = None;

    // Optional [details]
    if p.at(TokenKind::LBracket) {
        p.advance();

        // Optional alias
        if p.at(TokenKind::Identifier) {
            alias = Some(p.advance().text.clone());
        }

        // Rel types: :TYPE1|TYPE2
        if p.at(TokenKind::Colon) {
            p.advance();
            rel_types.push(p.expect(TokenKind::Identifier)?.text.clone());
            while p.eat(TokenKind::Pipe) {
                rel_types.push(p.expect(TokenKind::Identifier)?.text.clone());
            }
        }

        // Variable length: *min..max
        if p.eat(TokenKind::Star) {
            let min = if p.at(TokenKind::Integer) {
                Some(p.advance().text.parse::<usize>().unwrap_or(1))
            } else {
                None
            };
            if p.eat(TokenKind::DotDot) {
                let max = if p.at(TokenKind::Integer) {
                    Some(p.advance().text.parse::<usize>().unwrap_or(100))
                } else {
                    None
                };
                var_length = Some(VarLength { min, max });
            } else if let Some(n) = min {
                var_length = Some(VarLength { min: Some(n), max: Some(n) });
            } else {
                var_length = Some(VarLength { min: None, max: None });
            }
        }

        // Properties
        if p.at(TokenKind::LBrace) {
            properties = parse_map_literal_inner(p)?;
        }

        p.expect(TokenKind::RBracket)?;
    }

    // Determine direction
    if left_arrow {
        // <-[...]- (incoming)
        if p.eat(TokenKind::Dash) {
            // <-[...]-  (could be undirected if no arrow on right)
        }
        direction = PatternDirection::Left;
    } else if p.eat(TokenKind::Arrow) {
        direction = PatternDirection::Right;
    } else if p.eat(TokenKind::Dash) {
        direction = PatternDirection::Both;
    } else {
        direction = PatternDirection::Right; // default
    }

    Ok((RelPattern {
        alias,
        rel_types,
        direction,
        properties,
        var_length,
    }, direction))
}

// ============================================================================
// RETURN / ORDER BY / SET helpers
// ============================================================================

fn parse_return_clause(p: &mut Parser) -> Result<ReturnClause> {
    let distinct = p.eat(TokenKind::Distinct);
    let mut items = Vec::new();

    if p.at(TokenKind::Star) {
        p.advance();
        items.push(ReturnItem { expr: Expr::Star, alias: None });
    } else {
        items.push(parse_return_item(p)?);
        while p.eat(TokenKind::Comma) {
            items.push(parse_return_item(p)?);
        }
    }

    Ok(ReturnClause { distinct, items })
}

fn parse_return_item(p: &mut Parser) -> Result<ReturnItem> {
    let expr = parse_expr(p)?;
    let alias = if p.eat(TokenKind::As) {
        Some(p.expect(TokenKind::Identifier)?.text.clone())
    } else {
        None
    };
    Ok(ReturnItem { expr, alias })
}

fn parse_order_by(p: &mut Parser) -> Result<Vec<OrderExpr>> {
    let mut exprs = Vec::new();
    exprs.push(parse_order_expr(p)?);
    while p.eat(TokenKind::Comma) {
        exprs.push(parse_order_expr(p)?);
    }
    Ok(exprs)
}

fn parse_order_expr(p: &mut Parser) -> Result<OrderExpr> {
    let expr = parse_expr(p)?;
    let ascending = if p.eat(TokenKind::Desc) {
        false
    } else {
        p.eat(TokenKind::Asc);
        true
    };
    Ok(OrderExpr { expr, ascending })
}

fn parse_set_items(p: &mut Parser) -> Result<Vec<SetItem>> {
    let mut items = Vec::new();
    items.push(parse_set_item(p)?);
    while p.eat(TokenKind::Comma) {
        items.push(parse_set_item(p)?);
    }
    Ok(items)
}

fn parse_set_item(p: &mut Parser) -> Result<SetItem> {
    let name = p.expect(TokenKind::Identifier)?.text.clone();

    if p.eat(TokenKind::Dot) {
        // SET n.prop = expr
        let key = p.expect(TokenKind::Identifier)?.text.clone();
        p.expect(TokenKind::Eq)?;
        let value = parse_expr(p)?;
        Ok(SetItem::Property { variable: name, key, value })
    } else if p.eat(TokenKind::PlusEq) {
        // SET n += {map}
        let value = parse_expr(p)?;
        Ok(SetItem::MergeProperties { variable: name, value })
    } else if p.eat(TokenKind::Eq) {
        // SET n = {map}
        let value = parse_expr(p)?;
        Ok(SetItem::AllProperties { variable: name, value })
    } else if p.at(TokenKind::Colon) {
        // SET n:Label
        p.advance();
        let label = p.expect(TokenKind::Identifier)?.text.clone();
        Ok(SetItem::Label { variable: name, label })
    } else {
        Err(p.error(format!("Expected '.', '=', '+=', or ':' after SET variable")))
    }
}

fn parse_variable_list(p: &mut Parser) -> Result<Vec<String>> {
    let mut vars = Vec::new();
    vars.push(p.expect(TokenKind::Identifier)?.text.clone());
    while p.eat(TokenKind::Comma) {
        vars.push(p.expect(TokenKind::Identifier)?.text.clone());
    }
    Ok(vars)
}

// ============================================================================
// Expression parsing (precedence climbing)
// ============================================================================

fn parse_expr(p: &mut Parser) -> Result<Expr> {
    parse_or_expr(p)
}

fn parse_or_expr(p: &mut Parser) -> Result<Expr> {
    let mut left = parse_xor_expr(p)?;
    while p.at(TokenKind::Or) {
        p.advance();
        let right = parse_xor_expr(p)?;
        left = Expr::BinaryOp { left: Box::new(left), op: BinaryOp::Or, right: Box::new(right) };
    }
    Ok(left)
}

fn parse_xor_expr(p: &mut Parser) -> Result<Expr> {
    let mut left = parse_and_expr(p)?;
    while p.at(TokenKind::Xor) {
        p.advance();
        let right = parse_and_expr(p)?;
        left = Expr::BinaryOp { left: Box::new(left), op: BinaryOp::Xor, right: Box::new(right) };
    }
    Ok(left)
}

fn parse_and_expr(p: &mut Parser) -> Result<Expr> {
    let mut left = parse_not_expr(p)?;
    while p.at(TokenKind::And) {
        p.advance();
        let right = parse_not_expr(p)?;
        left = Expr::BinaryOp { left: Box::new(left), op: BinaryOp::And, right: Box::new(right) };
    }
    Ok(left)
}

fn parse_not_expr(p: &mut Parser) -> Result<Expr> {
    if p.eat(TokenKind::Not) {
        let expr = parse_not_expr(p)?;
        Ok(Expr::UnaryOp { op: UnaryOp::Not, expr: Box::new(expr) })
    } else {
        parse_comparison(p)
    }
}

fn parse_comparison(p: &mut Parser) -> Result<Expr> {
    let mut left = parse_string_op(p)?;

    // IS NULL / IS NOT NULL
    if p.at(TokenKind::Is) {
        p.advance();
        let negated = p.eat(TokenKind::Not);
        p.expect(TokenKind::Null)?;
        return Ok(Expr::IsNull { expr: Box::new(left), negated });
    }

    // IN
    if p.at(TokenKind::In) {
        p.advance();
        let list = parse_addition(p)?;
        return Ok(Expr::In { expr: Box::new(left), list: Box::new(list) });
    }

    // Comparison operators
    let op = match p.peek_kind() {
        TokenKind::Eq => Some(BinaryOp::Eq),
        TokenKind::Neq => Some(BinaryOp::Neq),
        TokenKind::Lt => Some(BinaryOp::Lt),
        TokenKind::Lte => Some(BinaryOp::Lte),
        TokenKind::Gt => Some(BinaryOp::Gt),
        TokenKind::Gte => Some(BinaryOp::Gte),
        TokenKind::RegexMatch => Some(BinaryOp::RegexMatch),
        _ => None,
    };

    if let Some(op) = op {
        p.advance();
        let right = parse_string_op(p)?;
        left = Expr::BinaryOp { left: Box::new(left), op, right: Box::new(right) };
    }

    Ok(left)
}

fn parse_string_op(p: &mut Parser) -> Result<Expr> {
    let left = parse_addition(p)?;

    // STARTS WITH / ENDS WITH / CONTAINS
    if p.at(TokenKind::StartsWith) {
        // "STARTS" token — need to check for "WITH" after
        // Actually, our lexer produces StartsWith as a single token
        p.advance();
        let right = parse_addition(p)?;
        return Ok(Expr::StringOp { left: Box::new(left), op: StringOp::StartsWith, right: Box::new(right) });
    }
    if p.at(TokenKind::EndsWith) {
        p.advance();
        let right = parse_addition(p)?;
        return Ok(Expr::StringOp { left: Box::new(left), op: StringOp::EndsWith, right: Box::new(right) });
    }
    if p.at(TokenKind::Contains) {
        p.advance();
        let right = parse_addition(p)?;
        return Ok(Expr::StringOp { left: Box::new(left), op: StringOp::Contains, right: Box::new(right) });
    }

    Ok(left)
}

fn parse_addition(p: &mut Parser) -> Result<Expr> {
    let mut left = parse_multiplication(p)?;
    loop {
        let op = match p.peek_kind() {
            TokenKind::Plus => BinaryOp::Add,
            TokenKind::Minus => BinaryOp::Sub,
            _ => break,
        };
        p.advance();
        let right = parse_multiplication(p)?;
        left = Expr::BinaryOp { left: Box::new(left), op, right: Box::new(right) };
    }
    Ok(left)
}

fn parse_multiplication(p: &mut Parser) -> Result<Expr> {
    let mut left = parse_power(p)?;
    loop {
        let op = match p.peek_kind() {
            TokenKind::Star => BinaryOp::Mul,
            TokenKind::Slash => BinaryOp::Div,
            TokenKind::Percent => BinaryOp::Mod,
            _ => break,
        };
        p.advance();
        let right = parse_power(p)?;
        left = Expr::BinaryOp { left: Box::new(left), op, right: Box::new(right) };
    }
    Ok(left)
}

fn parse_power(p: &mut Parser) -> Result<Expr> {
    let left = parse_unary(p)?;
    if p.eat(TokenKind::Caret) {
        let right = parse_power(p)?; // right-associative
        Ok(Expr::BinaryOp { left: Box::new(left), op: BinaryOp::Pow, right: Box::new(right) })
    } else {
        Ok(left)
    }
}

fn parse_unary(p: &mut Parser) -> Result<Expr> {
    if p.eat(TokenKind::Minus) {
        let expr = parse_property_access(p)?;
        Ok(Expr::UnaryOp { op: UnaryOp::Negate, expr: Box::new(expr) })
    } else {
        parse_property_access(p)
    }
}

fn parse_property_access(p: &mut Parser) -> Result<Expr> {
    let mut expr = parse_primary(p)?;

    // Property access chain: n.name, n.address.city
    while p.at(TokenKind::Dot) {
        p.advance();
        let key = p.expect(TokenKind::Identifier)?.text.clone();
        expr = Expr::Property { expr: Box::new(expr), key };
    }

    // Label check: n:Person
    if p.at(TokenKind::Colon) {
        // Only if expr is a variable
        if let Expr::Variable(_) = &expr {
            p.advance();
            let label = p.expect(TokenKind::Identifier)?.text.clone();
            expr = Expr::HasLabel { expr: Box::new(expr), label };
        }
    }

    Ok(expr)
}

fn parse_primary(p: &mut Parser) -> Result<Expr> {
    match p.peek_kind() {
        // Literals
        TokenKind::Integer => {
            let tok = p.advance();
            let val = tok.text.parse::<i64>().map_err(|_| {
                Error::SyntaxError { position: tok.span.start, message: "Invalid integer".into() }
            })?;
            Ok(Expr::Literal(Literal::Int(val)))
        }
        TokenKind::Float => {
            let tok = p.advance();
            let val = tok.text.parse::<f64>().map_err(|_| {
                Error::SyntaxError { position: tok.span.start, message: "Invalid float".into() }
            })?;
            Ok(Expr::Literal(Literal::Float(val)))
        }
        TokenKind::StringLiteral => {
            let tok = p.advance();
            Ok(Expr::Literal(Literal::String(tok.text.clone())))
        }
        TokenKind::True => {
            p.advance();
            Ok(Expr::Literal(Literal::Bool(true)))
        }
        TokenKind::False => {
            p.advance();
            Ok(Expr::Literal(Literal::Bool(false)))
        }
        TokenKind::Null => {
            p.advance();
            Ok(Expr::Literal(Literal::Null))
        }

        // Parameter
        TokenKind::Parameter => {
            let tok = p.advance();
            Ok(Expr::Parameter(tok.text.clone()))
        }

        // Star (for RETURN *)
        TokenKind::Star => {
            p.advance();
            Ok(Expr::Star)
        }

        // Parenthesized expression
        TokenKind::LParen => {
            p.advance();
            let expr = parse_expr(p)?;
            p.expect(TokenKind::RParen)?;
            Ok(expr)
        }

        // List literal
        TokenKind::LBracket => {
            p.advance();
            let mut items = Vec::new();
            if !p.at(TokenKind::RBracket) {
                items.push(parse_expr(p)?);
                while p.eat(TokenKind::Comma) {
                    items.push(parse_expr(p)?);
                }
            }
            p.expect(TokenKind::RBracket)?;
            Ok(Expr::List(items))
        }

        // Map literal
        TokenKind::LBrace => {
            let map = parse_map_literal_inner(p)?;
            Ok(Expr::MapLiteral(map))
        }

        // CASE expression
        TokenKind::Case => {
            p.advance();
            let operand = if !p.at(TokenKind::When) {
                Some(Box::new(parse_expr(p)?))
            } else {
                None
            };
            let mut whens = Vec::new();
            while p.eat(TokenKind::When) {
                let when_expr = parse_expr(p)?;
                p.expect(TokenKind::Then)?;
                let then_expr = parse_expr(p)?;
                whens.push((when_expr, then_expr));
            }
            let else_expr = if p.eat(TokenKind::Else) {
                Some(Box::new(parse_expr(p)?))
            } else {
                None
            };
            p.expect(TokenKind::End)?;
            Ok(Expr::Case { operand, whens, else_expr })
        }

        // EXISTS
        TokenKind::Exists => {
            p.advance();
            p.expect(TokenKind::LParen)?;
            // Simplified: parse a pattern as a match clause
            let patterns = parse_pattern_list(p)?;
            p.expect(TokenKind::RParen)?;
            Ok(Expr::Exists(Box::new(MatchClause { optional: false, patterns })))
        }

        // Identifier — could be variable or function call
        TokenKind::Identifier => {
            let tok = p.advance().clone();
            if p.at(TokenKind::LParen) {
                // Function call: name(args)
                p.advance(); // consume (
                let mut args = Vec::new();
                let mut distinct = false;

                // Check for DISTINCT in function call: count(DISTINCT n)
                if p.eat(TokenKind::Distinct) {
                    distinct = true;
                }

                if p.at(TokenKind::Star) {
                    // count(*)
                    p.advance();
                } else if !p.at(TokenKind::RParen) {
                    args.push(parse_expr(p)?);
                    while p.eat(TokenKind::Comma) {
                        args.push(parse_expr(p)?);
                    }
                }
                p.expect(TokenKind::RParen)?;
                Ok(Expr::FunctionCall { name: tok.text, args, distinct })
            } else {
                Ok(Expr::Variable(tok.text))
            }
        }

        _ => Err(p.error(format!("Unexpected token in expression: {:?} '{}'", p.peek_kind(), p.peek().text))),
    }
}

fn parse_map_literal_inner(p: &mut Parser) -> Result<HashMap<String, Expr>> {
    p.expect(TokenKind::LBrace)?;
    let mut map = HashMap::new();
    if !p.at(TokenKind::RBrace) {
        let key = p.expect(TokenKind::Identifier)?.text.clone();
        p.expect(TokenKind::Colon)?;
        let value = parse_expr(p)?;
        map.insert(key, value);
        while p.eat(TokenKind::Comma) {
            let key = p.expect(TokenKind::Identifier)?.text.clone();
            p.expect(TokenKind::Colon)?;
            let value = parse_expr(p)?;
            map.insert(key, value);
        }
    }
    p.expect(TokenKind::RBrace)?;
    Ok(map)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cypher::lexer::tokenize;

    fn parse(query: &str) -> Result<Statement> {
        let tokens = tokenize(query)?;
        parse_statement(&tokens)
    }

    #[test]
    fn test_simple_match_return() {
        let stmt = parse("MATCH (n:Person) RETURN n").unwrap();
        match stmt {
            Statement::Query(q) => {
                assert_eq!(q.matches.len(), 1);
                assert_eq!(q.matches[0].patterns.len(), 1);
                assert_eq!(q.return_clause.items.len(), 1);
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn test_match_with_where() {
        let stmt = parse("MATCH (n:Person) WHERE n.age > 30 RETURN n.name").unwrap();
        match stmt {
            Statement::Query(q) => {
                assert!(q.where_clause.is_some());
                assert_eq!(q.return_clause.items.len(), 1);
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn test_create_node() {
        let stmt = parse("CREATE (n:Person {name: 'Ada', age: 3})").unwrap();
        match stmt {
            Statement::Create(c) => {
                assert_eq!(c.patterns.len(), 1);
                let elem = &c.patterns[0].elements[0];
                if let PatternElement::Node(np) = elem {
                    assert_eq!(np.labels, vec!["Person"]);
                    assert_eq!(np.properties.len(), 2);
                }
            }
            _ => panic!("Expected Create"),
        }
    }

    #[test]
    fn test_create_return() {
        let stmt = parse("CREATE (n:Person {name: 'Ada'}) RETURN n").unwrap();
        match stmt {
            Statement::Create(c) => {
                assert!(c.return_clause.is_some());
            }
            _ => panic!("Expected Create"),
        }
    }

    #[test]
    fn test_relationship_pattern() {
        let stmt = parse("MATCH (a:Person)-[:KNOWS]->(b:Person) RETURN a, b").unwrap();
        match stmt {
            Statement::Query(q) => {
                assert_eq!(q.matches[0].patterns[0].elements.len(), 3);
                assert_eq!(q.return_clause.items.len(), 2);
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn test_match_set() {
        let stmt = parse("MATCH (n:Person) WHERE n.name = 'Ada' SET n.age = 4").unwrap();
        match stmt {
            Statement::Set(s) => {
                assert_eq!(s.matches.len(), 1);
                assert!(s.where_clause.is_some());
                assert_eq!(s.items.len(), 1);
            }
            _ => panic!("Expected Set"),
        }
    }

    #[test]
    fn test_match_delete() {
        let stmt = parse("MATCH (n:Person) WHERE n.name = 'Ada' DETACH DELETE n").unwrap();
        match stmt {
            Statement::Delete(d) => {
                assert!(d.detach);
                assert_eq!(d.variables, vec!["n"]);
            }
            _ => panic!("Expected Delete"),
        }
    }

    #[test]
    fn test_return_with_limit() {
        let stmt = parse("MATCH (n:Person) RETURN n LIMIT 10").unwrap();
        match stmt {
            Statement::Query(q) => {
                assert!(q.limit.is_some());
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn test_count_aggregate() {
        let stmt = parse("MATCH (n:Person) RETURN count(n)").unwrap();
        match stmt {
            Statement::Query(q) => {
                if let Expr::FunctionCall { name, .. } = &q.return_clause.items[0].expr {
                    assert_eq!(name, "count");
                } else {
                    panic!("Expected function call");
                }
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn test_string_literal_property() {
        let stmt = parse("MATCH (n:Person) WHERE n.name = 'Ada' RETURN n").unwrap();
        assert!(matches!(stmt, Statement::Query(_)));
    }

    #[test]
    fn test_parameter() {
        let stmt = parse("MATCH (n:Person) WHERE n.name = $name RETURN n").unwrap();
        match stmt {
            Statement::Query(q) => {
                if let Some(Expr::BinaryOp { right, .. }) = &q.where_clause {
                    assert!(matches!(right.as_ref(), Expr::Parameter(_)));
                }
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn test_multiple_labels() {
        let stmt = parse("MATCH (n:Person:Employee) RETURN n").unwrap();
        match stmt {
            Statement::Query(q) => {
                if let PatternElement::Node(np) = &q.matches[0].patterns[0].elements[0] {
                    assert_eq!(np.labels, vec!["Person", "Employee"]);
                }
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn test_order_by() {
        let stmt = parse("MATCH (n:Person) RETURN n.name ORDER BY n.name DESC").unwrap();
        match stmt {
            Statement::Query(q) => {
                assert!(q.order_by.is_some());
                let order = q.order_by.as_ref().unwrap();
                assert!(!order[0].ascending);
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn test_return_alias() {
        let stmt = parse("MATCH (n:Person) RETURN n.name AS name").unwrap();
        match stmt {
            Statement::Query(q) => {
                assert_eq!(q.return_clause.items[0].alias.as_deref(), Some("name"));
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn test_boolean_expression() {
        let stmt = parse("MATCH (n) WHERE n.active = true AND n.age > 18 RETURN n").unwrap();
        match stmt {
            Statement::Query(q) => {
                assert!(matches!(q.where_clause, Some(Expr::BinaryOp { op: BinaryOp::And, .. })));
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn test_list_literal() {
        let stmt = parse("MATCH (n) WHERE n.id IN [1, 2, 3] RETURN n").unwrap();
        match stmt {
            Statement::Query(q) => {
                assert!(matches!(q.where_clause, Some(Expr::In { .. })));
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn test_is_null() {
        let stmt = parse("MATCH (n) WHERE n.email IS NOT NULL RETURN n").unwrap();
        match stmt {
            Statement::Query(q) => {
                if let Some(Expr::IsNull { negated, .. }) = &q.where_clause {
                    assert!(*negated);
                } else {
                    panic!("Expected IsNull expression");
                }
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn test_return_star() {
        let stmt = parse("MATCH (n:Person) RETURN *").unwrap();
        match stmt {
            Statement::Query(q) => {
                assert!(matches!(&q.return_clause.items[0].expr, Expr::Star));
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn test_with_clause() {
        let input = "MATCH (n:Person) WITH n.name AS name RETURN name";
        let result = super::super::parse(input);
        assert!(result.is_ok());
        let stmt = result.unwrap();
        match stmt {
            Statement::Query(q) => {
                assert_eq!(q.with_clauses.len(), 1);
                assert_eq!(q.with_clauses[0].items.len(), 1);
                assert_eq!(q.with_clauses[0].items[0].alias.as_deref(), Some("name"));
                assert!(q.with_clauses[0].where_clause.is_none());
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn test_with_clause_where() {
        let input = "MATCH (n:Person) WITH n.name AS name WHERE name = 'Alice' RETURN name";
        let result = super::super::parse(input);
        assert!(result.is_ok());
        let stmt = result.unwrap();
        match stmt {
            Statement::Query(q) => {
                assert_eq!(q.with_clauses.len(), 1);
                assert!(q.with_clauses[0].where_clause.is_some());
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn test_with_clause_multiple() {
        let input = "MATCH (n:Person) WITH n.name AS name WITH name RETURN name";
        let result = super::super::parse(input);
        assert!(result.is_ok());
        let stmt = result.unwrap();
        match stmt {
            Statement::Query(q) => {
                assert_eq!(q.with_clauses.len(), 2);
            }
            _ => panic!("Expected Query"),
        }
    }

    #[test]
    fn test_remove_property() {
        let input = "MATCH (n:Person) WHERE n.name = 'Alice' REMOVE n.age";
        let result = super::super::parse(input);
        assert!(result.is_ok());
        let stmt = result.unwrap();
        match stmt {
            Statement::Remove(r) => {
                assert_eq!(r.matches.len(), 1);
                assert!(r.where_clause.is_some());
                assert_eq!(r.items.len(), 1);
                match &r.items[0] {
                    RemoveItem::Property { variable, key } => {
                        assert_eq!(variable, "n");
                        assert_eq!(key, "age");
                    }
                    _ => panic!("Expected RemoveItem::Property"),
                }
            }
            _ => panic!("Expected Remove"),
        }
    }

    #[test]
    fn test_remove_label() {
        let input = "MATCH (n:Person) REMOVE n:Employee";
        let result = super::super::parse(input);
        assert!(result.is_ok());
        let stmt = result.unwrap();
        match stmt {
            Statement::Remove(r) => {
                assert_eq!(r.matches.len(), 1);
                assert_eq!(r.items.len(), 1);
                match &r.items[0] {
                    RemoveItem::Label { variable, label } => {
                        assert_eq!(variable, "n");
                        assert_eq!(label, "Employee");
                    }
                    _ => panic!("Expected RemoveItem::Label"),
                }
            }
            _ => panic!("Expected Remove"),
        }
    }

    #[test]
    fn test_remove_multiple_items() {
        let input = "MATCH (n:Person) REMOVE n.age, n:Employee";
        let result = super::super::parse(input);
        assert!(result.is_ok());
        let stmt = result.unwrap();
        match stmt {
            Statement::Remove(r) => {
                assert_eq!(r.items.len(), 2);
                assert!(matches!(&r.items[0], RemoveItem::Property { .. }));
                assert!(matches!(&r.items[1], RemoveItem::Label { .. }));
            }
            _ => panic!("Expected Remove"),
        }
    }
}
