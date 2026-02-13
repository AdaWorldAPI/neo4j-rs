//! Cypher lexer — tokenizes a query string.

use crate::{Error, Result};

/// A token from the lexer.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    pub text: String,
}

/// Source span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// Token kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    // Keywords
    Match, OptionalMatch, Where, Return, With, Unwind,
    Create, Merge, Delete, DetachDelete, Set, Remove,
    Order, By, Skip, Limit, Asc, Desc, Distinct,
    And, Or, Not, Xor, Is, Null, True, False, In,
    As, Case, When, Then, Else, End,
    Exists, All, Any, None, Single,
    StartsWith, EndsWith, Contains,
    OnCreate, OnMatch,
    Index, Constraint, Drop, On, For,
    Call, Yield,

    // Literals
    Integer, Float, StringLiteral,

    // Identifiers and parameters
    Identifier, Parameter,

    // Punctuation
    LParen, RParen, LBracket, RBracket, LBrace, RBrace,
    Dot, Comma, Colon, Semicolon, Pipe, Star,
    Arrow,      // ->
    LeftArrow,  // <-
    Dash,       // -
    DotDot,     // ..

    // Operators
    Eq, Neq, Lt, Lte, Gt, Gte,
    Plus, Minus, Slash, Percent, Caret,
    PlusEq,     // +=
    RegexMatch, // =~

    // Whitespace / EOF
    Eof,
}

/// Tokenize a Cypher query string.
pub fn tokenize(input: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = input.char_indices().peekable();

    while let Some(&(pos, ch)) = chars.peek() {
        match ch {
            // Skip whitespace
            c if c.is_whitespace() => { chars.next(); }

            // Skip block comments /* ... */
            '/' if matches!(chars.clone().nth(1), Some((_, '*'))) => {
                let comment_start = pos;
                chars.next(); // skip '/'
                chars.next(); // skip '*'
                loop {
                    match chars.next() {
                        Some((_, '*')) => {
                            if matches!(chars.peek(), Some(&(_, '/'))) {
                                chars.next(); // skip '/'
                                break;
                            }
                        }
                        Some(_) => {}
                        None => {
                            return Err(Error::SyntaxError {
                                position: comment_start,
                                message: "Unterminated block comment".into(),
                            });
                        }
                    }
                }
            }

            // Skip line comments
            '/' if matches!(chars.clone().nth(1), Some((_, '/'))) => {
                while chars.peek().map_or(false, |&(_, c)| c != '\n') {
                    chars.next();
                }
            }

            // String literals
            '\'' | '"' => {
                let quote = ch;
                chars.next(); // consume opening quote
                let start = pos;
                let mut s = String::new();
                loop {
                    match chars.next() {
                        Some((_, '\\')) => {
                            if let Some((_, escaped)) = chars.next() {
                                match escaped {
                                    'n' => s.push('\n'),
                                    't' => s.push('\t'),
                                    '\\' => s.push('\\'),
                                    c if c == quote => s.push(c),
                                    c => { s.push('\\'); s.push(c); }
                                }
                            }
                        }
                        Some((end, c)) if c == quote => {
                            tokens.push(Token {
                                kind: TokenKind::StringLiteral,
                                span: Span { start, end: end + 1 },
                                text: s,
                            });
                            break;
                        }
                        Some((_, c)) => s.push(c),
                        None => return Err(Error::SyntaxError {
                            position: start,
                            message: "Unterminated string literal".into(),
                        }),
                    }
                }
            }

            // Numbers
            c if c.is_ascii_digit() => {
                let start = pos;
                let mut num = String::new();
                let mut is_float = false;
                while let Some(&(_, c)) = chars.peek() {
                    if c.is_ascii_digit() {
                        num.push(c);
                        chars.next();
                    } else if c == '.' && !is_float {
                        is_float = true;
                        num.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(Token {
                    kind: if is_float { TokenKind::Float } else { TokenKind::Integer },
                    span: Span { start, end: start + num.len() },
                    text: num,
                });
            }

            // Parameter: $name
            '$' => {
                chars.next();
                let start = pos;
                let mut name = String::new();
                while let Some(&(_, c)) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        name.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                tokens.push(Token {
                    kind: TokenKind::Parameter,
                    span: Span { start, end: start + name.len() + 1 },
                    text: name,
                });
            }

            // Identifiers and keywords
            c if c.is_alphabetic() || c == '_' => {
                let start = pos;
                let mut ident = String::new();
                while let Some(&(_, c)) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        ident.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let kind = keyword_or_ident(&ident);
                tokens.push(Token {
                    kind,
                    span: Span { start, end: start + ident.len() },
                    text: ident,
                });
            }

            // Punctuation
            '(' => { chars.next(); tokens.push(punct(TokenKind::LParen, pos, "(")); }
            ')' => { chars.next(); tokens.push(punct(TokenKind::RParen, pos, ")")); }
            '[' => { chars.next(); tokens.push(punct(TokenKind::LBracket, pos, "[")); }
            ']' => { chars.next(); tokens.push(punct(TokenKind::RBracket, pos, "]")); }
            '{' => { chars.next(); tokens.push(punct(TokenKind::LBrace, pos, "{")); }
            '}' => { chars.next(); tokens.push(punct(TokenKind::RBrace, pos, "}")); }
            ',' => { chars.next(); tokens.push(punct(TokenKind::Comma, pos, ",")); }
            ':' => { chars.next(); tokens.push(punct(TokenKind::Colon, pos, ":")); }
            ';' => { chars.next(); tokens.push(punct(TokenKind::Semicolon, pos, ";")); }
            '|' => { chars.next(); tokens.push(punct(TokenKind::Pipe, pos, "|")); }
            '*' => { chars.next(); tokens.push(punct(TokenKind::Star, pos, "*")); }
            '.' => {
                chars.next();
                if matches!(chars.peek(), Some(&(_, '.'))) {
                    chars.next();
                    tokens.push(punct(TokenKind::DotDot, pos, ".."));
                } else {
                    tokens.push(punct(TokenKind::Dot, pos, "."));
                }
            }
            '+' => {
                chars.next();
                if matches!(chars.peek(), Some(&(_, '='))) {
                    chars.next();
                    tokens.push(punct(TokenKind::PlusEq, pos, "+="));
                } else {
                    tokens.push(punct(TokenKind::Plus, pos, "+"));
                }
            }
            '/' => { chars.next(); tokens.push(punct(TokenKind::Slash, pos, "/")); }
            '%' => { chars.next(); tokens.push(punct(TokenKind::Percent, pos, "%")); }
            '^' => { chars.next(); tokens.push(punct(TokenKind::Caret, pos, "^")); }
            '=' => {
                chars.next();
                if matches!(chars.peek(), Some(&(_, '~'))) {
                    chars.next();
                    tokens.push(punct(TokenKind::RegexMatch, pos, "=~"));
                } else {
                    tokens.push(punct(TokenKind::Eq, pos, "="));
                }
            }
            '<' => {
                chars.next();
                if matches!(chars.peek(), Some(&(_, '='))) {
                    chars.next();
                    tokens.push(punct(TokenKind::Lte, pos, "<="));
                } else if matches!(chars.peek(), Some(&(_, '-'))) {
                    chars.next();
                    tokens.push(punct(TokenKind::LeftArrow, pos, "<-"));
                } else if matches!(chars.peek(), Some(&(_, '>'))) {
                    chars.next();
                    tokens.push(punct(TokenKind::Neq, pos, "<>"));
                } else {
                    tokens.push(punct(TokenKind::Lt, pos, "<"));
                }
            }
            '>' => {
                chars.next();
                if matches!(chars.peek(), Some(&(_, '='))) {
                    chars.next();
                    tokens.push(punct(TokenKind::Gte, pos, ">="));
                } else {
                    tokens.push(punct(TokenKind::Gt, pos, ">"));
                }
            }
            '-' => {
                chars.next();
                if matches!(chars.peek(), Some(&(_, '>'))) {
                    chars.next();
                    tokens.push(punct(TokenKind::Arrow, pos, "->"));
                } else {
                    tokens.push(punct(TokenKind::Dash, pos, "-"));
                }
            }

            other => {
                return Err(Error::SyntaxError {
                    position: pos,
                    message: format!("Unexpected character: '{other}'"),
                });
            }
        }
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span { start: input.len(), end: input.len() },
        text: String::new(),
    });

    Ok(tokens)
}

fn punct(kind: TokenKind, pos: usize, text: &str) -> Token {
    Token {
        kind,
        span: Span { start: pos, end: pos + text.len() },
        text: text.to_string(),
    }
}

fn keyword_or_ident(s: &str) -> TokenKind {
    match s.to_uppercase().as_str() {
        "MATCH" => TokenKind::Match,
        "OPTIONAL" => TokenKind::OptionalMatch,
        "WHERE" => TokenKind::Where,
        "RETURN" => TokenKind::Return,
        "WITH" => TokenKind::With,
        "UNWIND" => TokenKind::Unwind,
        "CREATE" => TokenKind::Create,
        "MERGE" => TokenKind::Merge,
        "DELETE" => TokenKind::Delete,
        "DETACH" => TokenKind::DetachDelete,
        "SET" => TokenKind::Set,
        "REMOVE" => TokenKind::Remove,
        "ORDER" => TokenKind::Order,
        "BY" => TokenKind::By,
        "SKIP" => TokenKind::Skip,
        "LIMIT" => TokenKind::Limit,
        "ASC" | "ASCENDING" => TokenKind::Asc,
        "DESC" | "DESCENDING" => TokenKind::Desc,
        "DISTINCT" => TokenKind::Distinct,
        "AND" => TokenKind::And,
        "OR" => TokenKind::Or,
        "NOT" => TokenKind::Not,
        "XOR" => TokenKind::Xor,
        "IS" => TokenKind::Is,
        "NULL" => TokenKind::Null,
        "TRUE" => TokenKind::True,
        "FALSE" => TokenKind::False,
        "IN" => TokenKind::In,
        "AS" => TokenKind::As,
        "CASE" => TokenKind::Case,
        "WHEN" => TokenKind::When,
        "THEN" => TokenKind::Then,
        "ELSE" => TokenKind::Else,
        "END" => TokenKind::End,
        "EXISTS" => TokenKind::Exists,
        "INDEX" => TokenKind::Index,
        "CONSTRAINT" => TokenKind::Constraint,
        "DROP" => TokenKind::Drop,
        "ON" => TokenKind::On,
        "FOR" => TokenKind::For,
        "CALL" => TokenKind::Call,
        "YIELD" => TokenKind::Yield,
        _ => TokenKind::Identifier,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_match() {
        let tokens = tokenize("MATCH (n:Person) RETURN n").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![
            TokenKind::Match,
            TokenKind::LParen,
            TokenKind::Identifier, // n
            TokenKind::Colon,
            TokenKind::Identifier, // Person
            TokenKind::RParen,
            TokenKind::Return,
            TokenKind::Identifier, // n
            TokenKind::Eof,
        ]);
    }

    #[test]
    fn test_relationship_pattern() {
        let tokens = tokenize("(a)-[:KNOWS]->(b)").unwrap();
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind).collect();
        assert_eq!(kinds, vec![
            TokenKind::LParen,
            TokenKind::Identifier, // a
            TokenKind::RParen,
            TokenKind::Dash,
            TokenKind::LBracket,
            TokenKind::Colon,
            TokenKind::Identifier, // KNOWS
            TokenKind::RBracket,
            TokenKind::Arrow,
            TokenKind::LParen,
            TokenKind::Identifier, // b
            TokenKind::RParen,
            TokenKind::Eof,
        ]);
    }

    #[test]
    fn test_string_literal() {
        let tokens = tokenize("'hello world'").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::StringLiteral);
        assert_eq!(tokens[0].text, "hello world");
    }

    #[test]
    fn test_parameter() {
        let tokens = tokenize("$name").unwrap();
        assert_eq!(tokens[0].kind, TokenKind::Parameter);
        assert_eq!(tokens[0].text, "name");
    }

    #[test]
    fn test_block_comment() {
        let tokens = tokenize("MATCH /* this is a comment */ (n) RETURN n").unwrap();
        // Should have: MATCH, (, Ident("n"), ), RETURN, Ident("n"), Eof
        let kinds: Vec<_> = tokens.iter().map(|t| &t.kind).collect();
        assert_eq!(kinds[0], &TokenKind::Match);
        assert_eq!(kinds[1], &TokenKind::LParen);
    }

    #[test]
    fn test_block_comment_multiline() {
        let tokens = tokenize("MATCH /* multi\nline\ncomment */ (n)").unwrap();
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Match));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::LParen));
    }

    #[test]
    fn test_unterminated_block_comment() {
        let result = tokenize("MATCH /* unterminated");
        assert!(result.is_err());
    }

    #[test]
    fn test_parameter_span() {
        let tokens = tokenize("$myParam").unwrap();
        let param_token = &tokens[0];
        assert_eq!(param_token.span.start, 0);
        assert_eq!(param_token.span.end, 8); // $ + myParam = 8 chars
    }
}
