//! Parser for `ferron.conf` configuration files.
//!
//! The parser consumes tokens from the [`Lexer`](crate::lexer::Lexer) and builds
//! an abstract syntax tree (AST) representing the configuration structure.
//!
//! # Error Handling
//!
//! Parse errors include source location information via [`Span`](crate::lexer::Span)
//! for precise error reporting.
//!
//! The parser is an internal implementation detail. Users should parse
//! configuration via [`Config::from_str`](crate::Config).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use crate::ast::*;
use crate::lexer::{Span, Token, TokenKind};

/// A parse error with message and source location.
///
/// Errors are returned when the parser encounters invalid syntax
/// or unexpected tokens.
#[derive(Debug, Clone)]
pub struct ParseError {
    /// A human-readable error message.
    pub message: String,
    /// The source location where the error occurred.
    pub span: Span,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Parse error at line {}, column {}: {}",
            self.span.line, self.span.column, self.message
        )
    }
}

impl std::error::Error for ParseError {}

/// The parser that builds an AST from a token stream.
///
/// The parser implements a recursive descent parsing strategy,
/// where each grammar rule is implemented as a separate method.
pub(crate) struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    blank_line_counts: Vec<usize>,
}

/// Internal enum for tracking parsed number types.
enum ParsedNumber {
    Integer(i64),
    Float(f64),
}

impl Parser {
    /// Creates a new parser for the given tokens.
    ///
    /// # Arguments
    ///
    /// * `tokens` - The token stream from the lexer
    /// * `blank_line_counts` - Blank line counts from the lexer, one per token
    pub fn new(tokens: Vec<Token>, blank_line_counts: Vec<usize>) -> Parser {
        Parser {
            tokens,
            pos: 0,
            blank_line_counts,
        }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn advance(&mut self) -> &Token {
        self.pos += 1;
        &self.tokens[self.pos - 1]
    }

    fn advance_owned(&mut self) -> Token {
        let span = self.tokens[self.pos].span;
        let token = std::mem::replace(
            &mut self.tokens[self.pos],
            Token {
                kind: TokenKind::EOF,
                lexeme: None,
                span,
                had_whitespace: false,
            },
        );
        self.pos += 1;
        token
    }

    fn skip_ignorable_tokens(&mut self) {
        while matches!(self.peek().kind, TokenKind::Comment | TokenKind::Semicolon) {
            self.advance();
        }
    }

    /// Collects standalone comment tokens as `Statement::Comment` nodes.
    ///
    /// Returns the collected comment statements. Does not consume `TrailingComment` tokens.
    fn collect_comment_statements(&mut self) -> Vec<Statement> {
        let mut comments = Vec::new();
        while self.peek().kind == TokenKind::Comment {
            let token = self.advance_owned();
            let text = token.lexeme.unwrap_or_default();
            comments.push(Statement::Comment(text, token.span));
        }
        comments
    }

    /// Checks if the next token is a `TrailingComment` and consumes it.
    ///
    /// Returns the trailing comment text if present.
    fn consume_trailing_comment(&mut self) -> Option<String> {
        if self.peek().kind == TokenKind::TrailingComment {
            let token = self.advance_owned();
            token.lexeme
        } else {
            None
        }
    }

    fn check(&self, kind: TokenKind) -> bool {
        self.peek().kind == kind
    }

    fn blank_lines_before_current(&self) -> usize {
        self.blank_line_counts.get(self.pos).copied().unwrap_or(0)
    }

    fn token_text(token: &Token) -> Result<&str, ParseError> {
        token.lexeme.as_deref().ok_or(ParseError {
            message: format!("Missing token text for {:?}", token.kind),
            span: token.span,
        })
    }

    fn parse_integer(text: &str, span: Span) -> Result<i64, ParseError> {
        text.parse::<i64>().map_err(|_| ParseError {
            message: "Invalid number".into(),
            span,
        })
    }

    fn parse_decimal(
        integer: i64,
        decimal_text: &str,
        negative: bool,
        span: Span,
    ) -> Result<f64, ParseError> {
        let decimal = Self::parse_integer(decimal_text, span)?;
        let number = integer as f64 + decimal as f64 / 10.0_f64.powi(decimal_text.len() as i32);
        Ok(if negative { -number } else { number })
    }

    fn parse_number_literal(
        &mut self,
        integer_text: &str,
        span: Span,
        _negative: bool,
    ) -> Result<ParsedNumber, ParseError> {
        let (negative, digits) = if let Some(rest) = integer_text.strip_prefix('-') {
            (true, rest)
        } else if let Some(rest) = integer_text.strip_prefix('+') {
            (false, rest)
        } else {
            (false, integer_text)
        };
        let integer = Self::parse_integer(digits, span)?;

        if self.check(TokenKind::StringBare)
            && self
                .peek()
                .lexeme
                .as_deref()
                .is_some_and(|l| l.starts_with('.'))
        {
            let rest = &self.peek().lexeme.as_deref().unwrap()[1..];
            if rest.chars().all(|c| c.is_ascii_digit()) {
                let token = self.advance_owned();
                let decimal_text = Self::token_text(&token)?;
                let decimal_text = decimal_text.strip_prefix('.').ok_or_else(|| ParseError {
                    message: "Invalid number".into(),
                    span: token.span,
                })?;
                let number = Self::parse_decimal(integer, decimal_text, negative, token.span)?;
                Ok(ParsedNumber::Float(number))
            } else {
                Ok(ParsedNumber::Integer(if negative {
                    -integer
                } else {
                    integer
                }))
            }
        } else {
            Ok(ParsedNumber::Integer(if negative {
                -integer
            } else {
                integer
            }))
        }
    }

    fn is_value_start(&self) -> bool {
        matches!(
            self.peek().kind,
            TokenKind::StringBare
                | TokenKind::StringQuoted
                | TokenKind::StringRaw
                | TokenKind::Number
                | TokenKind::Boolean
                | TokenKind::InterpStart
                | TokenKind::LBracket
        )
    }

    fn expect(&mut self, kind: TokenKind) -> Result<(), ParseError> {
        if self.check(kind) {
            self.advance();
            Ok(())
        } else {
            Err(ParseError {
                message: format!("Expected {:?}, got {:?}", kind, self.peek().kind),
                span: self.peek().span,
            })
        }
    }

    fn expect_identifier(&mut self) -> Result<String, ParseError> {
        let token = self.advance_owned();

        if token.kind == TokenKind::Identifier {
            Ok(token.lexeme.ok_or(ParseError {
                message: "Missing token text for Identifier".into(),
                span: token.span,
            })?)
        } else {
            Err(ParseError {
                message: format!("Expected identifier, got {:?}", token.kind),
                span: token.span,
            })
        }
    }

    fn parse_operator(&mut self) -> Result<Operator, ParseError> {
        let token = self.advance();

        match token.kind {
            TokenKind::OpEq => Ok(Operator::Eq),
            TokenKind::OpNeq => Ok(Operator::NotEq),
            TokenKind::OpRegex => Ok(Operator::Regex),
            TokenKind::OpNotRegex => Ok(Operator::NotRegex),
            TokenKind::OpIn => Ok(Operator::In),

            // Bare string "in" is treated as the `in` operator
            TokenKind::StringBare if token.lexeme.as_deref() == Some("in") => Ok(Operator::In),

            _ => Err(ParseError {
                message: "Invalid operator".into(),
                span: token.span,
            }),
        }
    }

    fn parse_operand(&mut self) -> Result<Operand, ParseError> {
        let token = self.advance_owned();
        let span = token.span;

        match token.kind {
            TokenKind::Identifier => {
                let mut group = Vec::new();
                group.push(token.lexeme.ok_or(ParseError {
                    message: "Missing token text for Identifier".into(),
                    span,
                })?);

                while self.check(TokenKind::StringBare)
                    && self
                        .peek()
                        .lexeme
                        .as_deref()
                        .is_some_and(|l| l.starts_with('.'))
                {
                    let token = self.advance_owned();
                    let lexeme = token.lexeme.ok_or(ParseError {
                        message: "Missing token text for StringBare".into(),
                        span: token.span,
                    })?;
                    for part in lexeme.split('.').filter(|s| !s.is_empty()) {
                        group.push(part.to_string());
                    }
                }

                Ok(Operand::Identifier(group, span))
            }

            TokenKind::StringBare | TokenKind::StringQuoted | TokenKind::StringRaw => {
                Ok(Operand::String(
                    token.lexeme.ok_or(ParseError {
                        message: format!("Missing token text for {:?}", token.kind),
                        span,
                    })?,
                    span,
                ))
            }

            TokenKind::Number => {
                let integer_text = token.lexeme.as_deref().ok_or(ParseError {
                    message: "Missing token text for Number".into(),
                    span,
                })?;
                match self.parse_number_literal(integer_text, span, false)? {
                    ParsedNumber::Integer(integer) => Ok(Operand::Integer(integer, span)),
                    ParsedNumber::Float(number) => Ok(Operand::Float(number, span)),
                }
            }

            _ => Err(ParseError {
                message: "Invalid operand".into(),
                span: token.span,
            }),
        }
    }

    fn parse_match_expression(&mut self) -> Result<MatcherExpression, ParseError> {
        self.skip_ignorable_tokens();

        let left = self.parse_operand()?;
        let op = self.parse_operator()?;
        let right = self.parse_operand()?;
        let span = left.span();

        Ok(MatcherExpression {
            left,
            op,
            right,
            span,
        })
    }

    fn parse_match_block(&mut self) -> Result<MatchBlock, ParseError> {
        let start_span = self.peek().span; // Span of the 'match' keyword
        self.expect(TokenKind::Match)?;

        let matcher = self.expect_identifier()?;
        let mut expr = Vec::new();

        self.expect(TokenKind::LBrace)?;

        while !self.check(TokenKind::RBrace) {
            expr.push(self.parse_match_expression()?);
        }

        self.expect(TokenKind::RBrace)?;

        Ok(MatchBlock {
            matcher,
            expr,
            span: start_span,
            trailing_comment: None,
        })
    }

    fn parse_snippet_block(&mut self) -> Result<SnippetBlock, ParseError> {
        let start_span = self.peek().span; // Span of the 'snippet' keyword
        self.expect(TokenKind::Snippet)?;

        let name = self.expect_identifier()?;
        let block = self.parse_block()?;

        Ok(SnippetBlock {
            name,
            block,
            span: start_span,
            trailing_comment: None,
        })
    }

    fn parse_block(&mut self) -> Result<Block, ParseError> {
        let start_span = self.peek().span; // Span of the '{'
        self.expect(TokenKind::LBrace)?;

        let mut statements = Vec::new();
        let mut trailing_comments = std::collections::HashMap::new();
        let mut blank_lines_before = std::collections::HashMap::new();

        while !self.check(TokenKind::RBrace) {
            let blank = self.blank_lines_before_current();
            let stmts = self.parse_statement(false)?;
            for (i, stmt) in stmts.into_iter().enumerate() {
                let idx = statements.len();
                if blank > 0 && i == 0 {
                    blank_lines_before.insert(idx, blank);
                }
                // Extract trailing comment before pushing
                if let Some(tc) = Self::get_trailing_comment(&stmt) {
                    trailing_comments.insert(idx, tc.to_string());
                }
                statements.push(stmt);
            }
        }

        self.expect(TokenKind::RBrace)?;

        Ok(Block {
            statements,
            span: start_span,
            trailing_comments,
            blank_lines_before,
        })
    }

    fn get_trailing_comment(stmt: &Statement) -> Option<&str> {
        match stmt {
            Statement::Directive(d) => d.trailing_comment.as_deref(),
            Statement::HostBlock(h) => h.trailing_comment.as_deref(),
            Statement::MatchBlock(m) => m.trailing_comment.as_deref(),
            Statement::GlobalBlock(_) => None,
            Statement::SnippetBlock(s) => s.trailing_comment.as_deref(),
            Statement::Comment(_, _) => None,
        }
    }

    fn parse_string_with_interpolation(input: &str, span: Span) -> Result<Value, ParseError> {
        let mut parts = Vec::new();
        let bytes = input.as_bytes();
        let mut cursor = 0;
        let mut literal_start = 0;

        while cursor + 1 < bytes.len() {
            if bytes[cursor] == b'{' && bytes[cursor + 1] == b'{' {
                if literal_start < cursor {
                    parts.push(StringPart::Literal(
                        input[literal_start..cursor].to_string(),
                    ));
                }

                let expr_start = cursor + 2;
                cursor = expr_start;

                while cursor + 1 < bytes.len()
                    && !(bytes[cursor] == b'}' && bytes[cursor + 1] == b'}')
                {
                    cursor += 1;
                }

                if cursor + 1 >= bytes.len() {
                    return Err(ParseError {
                        message: format!(
                            "Expected {{ ... }} pair, got {:?}",
                            &input[expr_start - 2..]
                        ),
                        span,
                    });
                }

                let expr = input[expr_start..cursor]
                    .trim()
                    .split('.')
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>();
                parts.push(StringPart::Expression(expr));

                cursor += 2;
                literal_start = cursor;
            } else {
                cursor += 1;
            }
        }

        if parts.is_empty() {
            return Ok(Value::String(input.to_string(), span));
        }

        if literal_start < input.len() {
            parts.push(StringPart::Literal(input[literal_start..].to_string()));
        }

        Ok(Value::InterpolatedString(parts, span))
    }

    fn parse_value(&mut self) -> Result<Value, ParseError> {
        let token = self.advance_owned();
        let span = token.span;

        match token.kind {
            TokenKind::StringBare | TokenKind::StringQuoted | TokenKind::StringRaw => {
                let text = token.lexeme.as_deref().ok_or(ParseError {
                    message: format!("Missing token text for {:?}", token.kind),
                    span,
                })?;
                let mut value = if token.kind == TokenKind::StringRaw {
                    Value::String(text.to_string(), token.span)
                } else {
                    Self::parse_string_with_interpolation(text, token.span)?
                };

                // Check for bracket continuation (e.g., http://[::1]:8080/path)
                if self.check(TokenKind::LBracket) {
                    if let Value::String(ref s, _) = value {
                        let mut full_value = s.clone();
                        full_value.push('[');
                        self.advance(); // skip [
                        while !self.check(TokenKind::RBracket) && !self.check(TokenKind::EOF) {
                            if let Some(lexeme) = &self.peek().lexeme {
                                full_value.push_str(lexeme);
                            }
                            self.advance();
                        }
                        if self.check(TokenKind::RBracket) {
                            full_value.push(']');
                            self.advance();
                        }
                        // Insert ':' before Number after ']' (port syntax)
                        if full_value.ends_with(']') && self.check(TokenKind::Number) {
                            full_value.push(':');
                            if let Some(lexeme) = &self.peek().lexeme {
                                full_value.push_str(lexeme);
                            }
                            self.advance();
                        }
                        // Continue reading trailing bare strings
                        while self.check(TokenKind::StringBare) || self.check(TokenKind::Number) {
                            if let Some(lexeme) = &self.peek().lexeme {
                                full_value.push_str(lexeme);
                            }
                            self.advance();
                        }
                        value = Value::String(full_value, span);
                    }
                }

                Ok(value)
            }

            TokenKind::LBracket => {
                // Read [content] as a bare string value (e.g., [::1]:8080 as directive arg)
                let mut s = String::new();
                s.push('[');
                while !self.check(TokenKind::RBracket) && !self.check(TokenKind::EOF) {
                    if let Some(lexeme) = &self.peek().lexeme {
                        s.push_str(lexeme);
                    }
                    self.advance();
                }
                if self.check(TokenKind::RBracket) {
                    s.push(']');
                    self.advance();
                }
                // Insert ':' before Number after ']' (port syntax)
                if s.ends_with(']') && self.check(TokenKind::Number) {
                    s.push(':');
                    if let Some(lexeme) = &self.peek().lexeme {
                        s.push_str(lexeme);
                    }
                    self.advance();
                }
                // Continue reading trailing bare strings
                while self.check(TokenKind::StringBare) || self.check(TokenKind::Number) {
                    if let Some(lexeme) = &self.peek().lexeme {
                        s.push_str(lexeme);
                    }
                    self.advance();
                }
                Ok(Value::String(s, span))
            }

            TokenKind::InterpStart => {
                let mut parts = Vec::new();

                loop {
                    let advanced = self.advance_owned();
                    if advanced.kind == TokenKind::InterpEnd {
                        break;
                    } else if advanced.kind == TokenKind::Identifier {
                        parts.push(advanced.lexeme.ok_or(ParseError {
                            message: "Missing token text for Identifier".into(),
                            span: advanced.span,
                        })?);
                    } else if advanced.kind == TokenKind::StringBare {
                        // Dotted path continuation within interpolation
                        if let Some(lexeme) = &advanced.lexeme {
                            for part in lexeme.split('.').filter(|s| !s.is_empty()) {
                                parts.push(part.to_string());
                            }
                        }
                    } else {
                        return Err(ParseError {
                            message: "Invalid interpolation".into(),
                            span: advanced.span,
                        });
                    }
                }

                Ok(Value::InterpolatedString(
                    vec![StringPart::Expression(parts)],
                    span,
                ))
            }

            TokenKind::Number => {
                if self.check(TokenKind::LBracket) && !self.peek().had_whitespace {
                    return Err(ParseError {
                        message:
                            "Invalid bare string: a number cannot be directly followed by '[' without whitespace"
                                .into(),
                        span,
                    });
                }

                let integer_text = token.lexeme.as_deref().ok_or(ParseError {
                    message: "Missing token text for Number".into(),
                    span,
                })?;

                // Check if this number is the start of an IP address (e.g., 192.168.1.1)
                // by looking for StringBare starting with '.' that's NOT a pure decimal
                if self.check(TokenKind::StringBare)
                    && self
                        .peek()
                        .lexeme
                        .as_deref()
                        .is_some_and(|l| l.starts_with('.'))
                    && !self
                        .peek()
                        .lexeme
                        .as_deref()
                        .is_some_and(|l| l[1..].chars().all(|c| c.is_ascii_digit()))
                {
                    let mut full = integer_text.to_string();
                    while self.check(TokenKind::StringBare) {
                        if let Some(lexeme) = &self.peek().lexeme {
                            full.push_str(lexeme);
                        }
                        self.advance();
                    }
                    Ok(Value::String(full, span))
                } else {
                    match self.parse_number_literal(integer_text, span, false)? {
                        ParsedNumber::Integer(integer) => Ok(Value::Integer(integer, span)),
                        ParsedNumber::Float(number) => Ok(Value::Float(number, span)),
                    }
                }
            }

            TokenKind::Boolean => Ok(Value::Boolean(
                token.lexeme.as_deref().ok_or(ParseError {
                    message: "Missing token text for Boolean".into(),
                    span,
                })? == "true",
                span,
            )),

            _ => Err(ParseError {
                message: "Invalid value".into(),
                span: token.span,
            }),
        }
    }

    fn parse_directive(&mut self) -> Result<Directive, ParseError> {
        let name_token = self.advance_owned();
        let span = name_token.span;
        let name = name_token.lexeme.ok_or(ParseError {
            message: "Missing token text for Identifier".into(),
            span,
        })?;
        let mut args = Vec::new();

        while self.is_value_start() {
            // Reject adjacent value tokens without whitespace between them.
            // This prevents silent token jamming (e.g. `"a""a"`, `+3.5+3.5`).
            if !args.is_empty() && !self.peek().had_whitespace {
                return Err(ParseError {
                    message: "Values must be separated by whitespace".into(),
                    span: self.peek().span,
                });
            }
            args.push(self.parse_value()?);
        }

        let block = if self.check(TokenKind::LBrace) {
            Some(self.parse_block()?)
        } else {
            None
        };

        Ok(Directive {
            name,
            args,
            block,
            span,
            trailing_comment: None,
        })
    }

    fn parse_host_pattern(&mut self) -> Result<HostPattern, ParseError> {
        let start_span = self.peek().span; // Span of the first token in the host pattern
        let mut labels = Vec::new();
        let mut port = None;
        let mut protocol = None;
        let mut is_ipv6 = false;

        loop {
            let token = self.advance_owned();

            match &token.kind {
                TokenKind::LBracket => {
                    is_ipv6 = true;
                }

                TokenKind::RBracket if is_ipv6 => {
                    labels = vec![labels.join("")];
                    is_ipv6 = false;
                    if self.check(TokenKind::Number) {
                        let port_token = self.advance_owned();
                        port =
                            Some(Self::token_text(&port_token)?.parse::<u16>().map_err(|_| {
                                ParseError {
                                    message: "Invalid port number".into(),
                                    span: port_token.span,
                                }
                            })?);
                        break;
                    }
                }

                TokenKind::Identifier
                | TokenKind::Number
                | TokenKind::Match
                | TokenKind::Snippet
                | TokenKind::Boolean
                | TokenKind::StringRaw => {
                    labels.push(token.lexeme.ok_or(ParseError {
                        message: format!("Missing token text for {:?}", token.kind),
                        span: token.span,
                    })?);
                }

                TokenKind::StringBare if is_ipv6 => {
                    labels.push(token.lexeme.ok_or(ParseError {
                        message: "Missing token text for StringBare".into(),
                        span: token.span,
                    })?);
                }

                TokenKind::StringBare => {
                    let lexeme = token.lexeme.ok_or(ParseError {
                        message: "Missing token text for StringBare".into(),
                        span: token.span,
                    })?;

                    if is_ipv6 {
                        // Inside IPv6 brackets, bare strings are just concatenated
                        labels.push(lexeme);
                    } else if lexeme.starts_with('[') && lexeme.contains(']') {
                        // Handle IPv6 in brackets: [addr] or [addr]:port
                        if let Some(close_pos) = lexeme.find(']') {
                            let addr_str = &lexeme[1..close_pos];
                            let rest = &lexeme[close_pos + 1..];

                            if rest.is_empty() {
                                let addr: Ipv6Addr = addr_str.parse().map_err(|_| ParseError {
                                    message: "Invalid IPv6 address".into(),
                                    span: token.span,
                                })?;
                                return Ok(HostPattern {
                                    labels: HostLabels::IpAddr(IpAddr::V6(addr)),
                                    port: None,
                                    protocol,
                                    span: start_span,
                                });
                            } else if rest.starts_with(':') && rest.len() > 1 {
                                let port_str = &rest[1..];
                                let port_num: u16 = port_str.parse().map_err(|_| ParseError {
                                    message: "Invalid port number".into(),
                                    span: token.span,
                                })?;
                                let addr: Ipv6Addr = addr_str.parse().map_err(|_| ParseError {
                                    message: "Invalid IPv6 address".into(),
                                    span: token.span,
                                })?;
                                return Ok(HostPattern {
                                    labels: HostLabels::IpAddr(IpAddr::V6(addr)),
                                    port: Some(port_num),
                                    protocol,
                                    span: start_span,
                                });
                            }
                        }
                        labels.extend(lexeme.split('.').map(|s| s.to_string()));
                    } else {
                        // Split bare string by dots and commas (not colons)
                        for part in lexeme.split(['.', ',']) {
                            if part.is_empty() {
                                continue;
                            }
                            if let Some((host_part, port_str)) = part.split_once(':') {
                                if !host_part.is_empty() {
                                    labels.push(host_part.to_string());
                                }
                                port = Some(port_str.parse::<u16>().map_err(|_| ParseError {
                                    message: "Invalid port number".into(),
                                    span: token.span,
                                })?);
                            } else {
                                labels.push(part.to_string());
                            }
                        }

                        // Protocol detection: if next StringBare doesn't start with '.',
                        // it's a protocol transition
                        if !labels.is_empty()
                            && labels.len() <= 1
                            && protocol.is_none()
                            && self.check(TokenKind::StringBare)
                            && self
                                .peek()
                                .lexeme
                                .as_deref()
                                .is_some_and(|l| !l.starts_with('.') && !l.starts_with(':'))
                        {
                            protocol = Some(labels.pop().ok_or_else(|| ParseError {
                                message: "Invalid host label".into(),
                                span: self.peek().span,
                            })?);
                        }
                    }
                }

                _ => {
                    return Err(ParseError {
                        message: "Invalid host label".into(),
                        span: token.span,
                    });
                }
            }

            if is_ipv6 {
                continue;
            }

            if self.check(TokenKind::StringBare) || self.check(TokenKind::Number) {
                if !is_ipv6 && labels.len() == 1 {
                    let is_host_continuation = self.peek().kind == TokenKind::StringBare
                        && self
                            .peek()
                            .lexeme
                            .as_deref()
                            .is_some_and(|l| l.starts_with('.') || l.starts_with(':'));
                    if !is_host_continuation {
                        protocol = Some(labels.pop().ok_or_else(|| ParseError {
                            message: "Invalid host label".into(),
                            span: self.peek().span,
                        })?);
                    }
                } else {
                    break;
                }
            } else if !is_ipv6 && self.check(TokenKind::LBracket) {
                if protocol.is_none() && labels.len() == 1 {
                    protocol = Some(labels.pop().ok_or_else(|| ParseError {
                        message: "Invalid host label".into(),
                        span: self.peek().span,
                    })?);
                } else {
                    return Err(ParseError {
                        message: "Invalid host label".into(),
                        span: self.peek().span,
                    });
                }
            } else if is_ipv6 && self.check(TokenKind::RBracket) {
                // IPv6 addresses are enclosed in brackets
            } else {
                break;
            }
        }

        let labels = if labels.len() == 1 && labels[0].contains(":") {
            let label = labels.pop().ok_or_else(|| ParseError {
                message: "Invalid host format".into(),
                span: self.peek().span,
            })?;
            let Ok(addr) = label.parse::<Ipv6Addr>() else {
                return Err(ParseError {
                    message: "Invalid IPv6 address".into(),
                    span: self.peek().span,
                });
            };
            HostLabels::IpAddr(IpAddr::V6(addr))
        } else if labels.len() == 4 {
            let mut octets = [0; 4];
            let mut is_ipv4 = true;
            let mut looks_like_ipv4 = true;
            for (i, label) in labels.iter().enumerate() {
                if is_ipv4 {
                    match label.parse::<u8>() {
                        Ok(octet) => octets[i] = octet,
                        Err(_) => {
                            is_ipv4 = false;
                        }
                    }
                }

                if !is_ipv4 {
                    if label.chars().any(|c| !c.is_ascii_digit()) {
                        looks_like_ipv4 = false;
                    }
                    if !looks_like_ipv4 {
                        break;
                    }
                }
            }

            if is_ipv4 {
                HostLabels::IpAddr(IpAddr::V4(Ipv4Addr::from(octets)))
            } else if looks_like_ipv4 {
                return Err(ParseError {
                    message: "Invalid IPv4 address".into(),
                    span: self.peek().span,
                });
            } else {
                HostLabels::Hostname(labels)
            }
        } else if labels.len() == 1 && labels[0] == "*" {
            HostLabels::Wildcard
        } else {
            HostLabels::Hostname(labels)
        };

        Ok(HostPattern {
            labels,
            port,
            protocol,
            span: start_span,
        })
    }

    fn parse_host_block(&mut self) -> Result<HostBlock, ParseError> {
        let start_span = self.peek().span;
        let mut hosts = Vec::new();
        loop {
            // Skip optional `;` delimiters between host patterns.
            while self.check(TokenKind::Semicolon) {
                self.advance();
            }

            let host = self.parse_host_pattern()?;
            hosts.push(host);

            if matches!(
                self.peek().kind,
                TokenKind::Identifier
                    | TokenKind::Number
                    | TokenKind::StringBare
                    | TokenKind::LBracket
            ) {
                continue;
            }
            break;
        }
        let block = self.parse_block()?;

        Ok(HostBlock {
            hosts,
            block,
            span: start_span,
            trailing_comment: None,
        })
    }

    /// Performs a lookahead scan to determine if tokens form a host block.
    ///
    /// This is needed to disambiguate between directives and host blocks
    /// at the top level.
    fn scans_as_host_block(&self) -> bool {
        let mut i = self.pos;
        let mut in_ipv6 = false;

        while i < self.tokens.len() {
            let token = &self.tokens[i];
            match token.kind {
                TokenKind::LBrace => return true,
                TokenKind::Semicolon => {
                    i += 1;
                    continue;
                }
                TokenKind::LBracket => {
                    in_ipv6 = true;
                }
                TokenKind::RBracket => {
                    in_ipv6 = false;
                    i += 1;
                    continue;
                }
                _ => {}
            }

            if in_ipv6 {
                i += 1;
                continue;
            }

            if i > self.pos {
                let prev = &self.tokens[i - 1];
                match prev.kind {
                    TokenKind::Identifier
                    | TokenKind::Match
                    | TokenKind::Snippet
                    | TokenKind::Boolean
                    | TokenKind::Number
                    | TokenKind::StringBare
                    | TokenKind::StringRaw
                    | TokenKind::RBracket => match token.kind {
                        TokenKind::LBracket
                        | TokenKind::LBrace
                        | TokenKind::StringBare
                        | TokenKind::StringRaw
                        | TokenKind::Number
                        | TokenKind::Identifier
                        | TokenKind::Boolean => {}
                        _ => return false,
                    },
                    _ => {}
                }
            }
            i += 1;
        }
        false
    }

    /// Convenience wrapper for [`scans_as_host_block`](Self::scans_as_host_block).
    fn looks_like_host(&self) -> bool {
        self.scans_as_host_block()
    }

    /// Parses a single statement at the current position, including any
    /// leading comments and trailing comment.
    ///
    /// # Arguments
    ///
    /// * `top_level` - If `true`, host blocks are allowed; if `false`, only
    ///   directives, match blocks, snippet blocks, and nested blocks are allowed
    ///
    /// # Returns
    ///
    /// A vector of [`Statement`]s (leading comments + the statement) on success,
    /// or a [`ParseError`] if parsing fails. The trailing comment is stored
    /// in the last statement of the returned vector.
    fn parse_statement(&mut self, top_level: bool) -> Result<Vec<Statement>, ParseError> {
        // Skip leading semicolon delimiters (`;`) and collect any comments
        // that precede this statement as leading `Statement::Comment` nodes.
        let mut leading_comments = Vec::new();
        loop {
            match self.peek().kind {
                TokenKind::Comment => {
                    let token = self.advance_owned();
                    leading_comments.push(Statement::Comment(
                        token.lexeme.unwrap_or_default(),
                        token.span,
                    ));
                }
                TokenKind::Semicolon => {
                    self.advance();
                }
                _ => break,
            }
        }

        // After collecting comments, check what's next
        if self.check(TokenKind::EOF) {
            if top_level {
                return Ok(leading_comments);
            }
            // Inside a block, EOF means unclosed block
            return Err(ParseError {
                message: "Unexpected end of file, expected '}'".into(),
                span: self.peek().span,
            });
        }

        if self.check(TokenKind::RBrace) && !top_level {
            return Ok(leading_comments);
        }

        let mut stmt = match self.peek().kind {
            TokenKind::Number
            | TokenKind::StringBare
            | TokenKind::Identifier
            | TokenKind::Boolean
            | TokenKind::LBracket
                if top_level && self.looks_like_host() =>
            {
                Statement::HostBlock(self.parse_host_block()?)
            }

            TokenKind::Match => Statement::MatchBlock(self.parse_match_block()?),

            TokenKind::Identifier => Statement::Directive(self.parse_directive()?),

            TokenKind::LBrace => Statement::GlobalBlock(self.parse_block()?),

            TokenKind::Snippet => Statement::SnippetBlock(self.parse_snippet_block()?),

            _ => {
                return Err(ParseError {
                    message: format!(
                        "Unexpected token {:?}, expected 'match', 'identifier', '[', '@', 'number' or '*'",
                        self.peek().kind
                    ),
                    span: self.peek().span,
                });
            }
        };

        // Check for trailing comment
        if let Some(trailing) = self.consume_trailing_comment() {
            self.set_trailing_comment(&mut stmt, trailing);
        }

        let mut result = leading_comments;
        result.push(stmt);
        Ok(result)
    }

    /// Sets the trailing comment on a statement.
    fn set_trailing_comment(&self, stmt: &mut Statement, comment: String) {
        match stmt {
            Statement::Directive(d) => d.trailing_comment = Some(comment),
            Statement::HostBlock(h) => h.trailing_comment = Some(comment),
            Statement::MatchBlock(m) => m.trailing_comment = Some(comment),
            Statement::GlobalBlock(_) => {} // Global blocks don't support trailing comments
            Statement::SnippetBlock(s) => s.trailing_comment = Some(comment),
            Statement::Comment(_, _) => {}
        }
    }

    /// Parses a complete configuration file into a [`Config`] AST.
    ///
    /// This is the main entry point for parsing. It handles:
    /// - Leading comments
    /// - Multiple top-level statements
    /// - EOF validation
    ///
    /// # Returns
    ///
    /// A [`Config`] containing all parsed statements, or a [`ParseError`]
    /// if parsing fails.
    pub fn parse_config(&mut self) -> Result<Config, ParseError> {
        let mut statements = Vec::new();
        let mut trailing_comments = std::collections::HashMap::new();
        let mut blank_lines_before = std::collections::HashMap::new();

        // Skip leading comments (they become Statement::Comment nodes)
        let leading = self.collect_comment_statements();
        for stmt in leading {
            statements.push(stmt);
        }

        while !self.check(TokenKind::EOF) {
            let blank = self.blank_lines_before_current();
            let stmts = self.parse_statement(true)?;
            for (i, stmt) in stmts.into_iter().enumerate() {
                let idx = statements.len();
                if blank > 0 && i == 0 {
                    blank_lines_before.insert(idx, blank);
                }
                // Extract trailing comment before pushing
                if let Some(tc) = Self::get_trailing_comment(&stmt) {
                    trailing_comments.insert(idx, tc.to_string());
                }
                statements.push(stmt);
            }
        }

        Ok(Config {
            statements,
            trailing_comments,
            blank_lines_before,
        })
    }
}
