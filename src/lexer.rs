//! Lexer for `ferron.conf` configuration files.
//!
//! The lexer converts raw input text into a sequence of [`Token`]s,
//! discarding whitespace and comments. Tokens are then consumed by
//! the [`Parser`](crate::parser::Parser) to build an AST.
//!
//! The lexer is an internal implementation detail. Users should parse
//! configuration via [`Config::from_str`](crate::Config).

#[cfg_attr(feature = "lexer-public", visibility::make(pub))]
#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) enum TokenKind {
    /// An identifier (e.g., directive name, hostname label).
    Identifier,
    /// A numeric literal (integer or decimal part).
    Number,
    /// A double-quoted string (e.g., `"hello world"`).
    StringQuoted,
    /// A raw double-quoted string (e.g., `r"^/api/v1$"`) — no escape processing.
    StringRaw,
    /// A bare (unquoted) string (e.g., `example.com`).
    StringBare,
    /// A boolean literal (`true` or `false`).
    Boolean,

    /// Left brace `{`.
    LBrace,
    /// Right brace `}`.
    RBrace,
    /// Left bracket `[`.
    LBracket,
    /// Right bracket `]`.
    RBracket,

    /// Statement delimiter `;` (optional separator between statements).
    Semicolon,

    /// Equality operator `==`.
    OpEq,
    /// Inequality operator `!=`.
    OpNeq,
    /// Regex match operator `~`.
    OpRegex,
    /// Negated regex operator `!~`.
    OpNotRegex,
    /// Membership operator `in`.
    OpIn,

    /// The `match` keyword.
    Match,
    /// The `snippet` keyword.
    Snippet,

    /// Interpolation start `{{`.
    InterpStart,
    /// Interpolation end `}}`.
    InterpEnd,

    /// A comment (skipped during parsing).
    Comment,
    /// A trailing comment (on the same line as a statement).
    TrailingComment,
    /// End of file marker.
    #[allow(clippy::upper_case_acronyms)]
    EOF,
}

/// A source location (line and column) for error reporting.
///
/// Spans are attached to tokens and AST nodes to track their
/// position in the original source file.
#[derive(Copy, Debug, Clone)]
pub struct Span {
    /// The 1-indexed line number.
    pub line: usize,
    /// The 1-indexed column number.
    pub column: usize,
}

impl PartialEq for Span {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

/// A token produced by the lexer.
#[cfg_attr(feature = "lexer-public", visibility::make(pub))]
#[derive(Debug, Clone)]
pub(crate) struct Token {
    /// The type of this token.
    pub kind: TokenKind,
    /// The optional lexeme text (for tokens with values).
    pub lexeme: Option<String>,
    /// The source location of this token.
    pub span: Span,
    /// Whether this token was preceded by whitespace in the source.
    ///
    /// This acts as a "whitespace token" marker: it lets the parser detect
    /// when two tokens are jammed together without separation (e.g.
    /// `34[::1]34`), which is invalid bare-string syntax.
    pub had_whitespace: bool,
}

impl Token {
    /// Creates a token without a lexeme (for punctuation and keywords).
    fn bare(kind: TokenKind, span: Span) -> Self {
        Token {
            kind,
            lexeme: None,
            span,
            had_whitespace: false,
        }
    }

    /// Creates a token with a lexeme value.
    fn with_lexeme(kind: TokenKind, lexeme: String, span: Span) -> Self {
        Token {
            kind,
            lexeme: Some(lexeme),
            span,
            had_whitespace: false,
        }
    }
}

/// The lexer that converts source text into tokens.
///
/// The lexer implements [`Iterator`] to produce tokens one at a time.
/// It tracks position (line/column) for error reporting and handles:
/// - Whitespace skipping
/// - Comment skipping (lines starting with `#`)
/// - String parsing (quoted and bare)
/// - Number parsing
/// - Identifier and keyword recognition
#[cfg_attr(feature = "lexer-public", visibility::make(pub))]
pub(crate) struct Lexer<'a> {
    chars: std::str::Chars<'a>,
    current: Option<char>,
    next: Option<char>,
    line: usize,
    column: usize,
    prev_token: Option<TokenKind>,
    last_non_comment_line: usize,
    /// Blank line counts collected during lexing, one per token produced.
    blank_line_counts: Vec<usize>,
}

impl<'a> Lexer<'a> {
    /// Creates a new lexer for the given input string.
    ///
    /// # Arguments
    ///
    /// * `input` - The source code to tokenize
    pub fn new(input: &'a str) -> Self {
        let mut chars = input.chars();
        let current = chars.next();
        let next = chars.next();

        Lexer {
            chars,
            current,
            next,
            line: 1,
            column: 1,
            prev_token: None,
            last_non_comment_line: 0,
            blank_line_counts: Vec::new(),
        }
    }

    fn advance(&mut self) {
        if let Some(c) = self.current {
            if c == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
        }

        self.current = self.next;
        self.next = self.chars.next();
    }

    fn peek(&self) -> Option<char> {
        self.next
    }

    fn skip_whitespace(&mut self) -> (bool, bool, usize) {
        let mut had_newlines = false;
        let mut had_whitespace = false;
        let mut consecutive_newlines: usize = 0;
        while matches!(self.current, Some(c) if c.is_whitespace()) {
            had_whitespace = true;
            if matches!(self.current, Some('\n') | Some('\r')) {
                had_newlines = true;
                consecutive_newlines += 1;
            }
            self.advance();
        }
        // blank lines = newlines - 1 (e.g., 2 newlines = 1 blank line)
        let blank_lines = consecutive_newlines.saturating_sub(1);
        (had_newlines, had_whitespace, blank_lines)
    }

    /// Returns the blank line counts collected during lexing.
    ///
    /// Each entry corresponds to the token at the same index in the token stream.
    pub fn into_blank_line_counts(self) -> Vec<usize> {
        self.blank_line_counts
    }

    fn read_identifier(&mut self) -> String {
        let mut s = String::new();

        while let Some(c) = self.current {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }

        s
    }

    fn identifier_token(&self, s: &str) -> TokenKind {
        match s {
            "match" => TokenKind::Match,
            "snippet" => TokenKind::Snippet,
            "true" | "false" => TokenKind::Boolean,
            "in" => TokenKind::OpIn,
            _ => TokenKind::Identifier,
        }
    }

    fn read_number(&mut self) -> String {
        let mut s = String::new();

        while let Some(c) = self.current {
            if c.is_ascii_digit() {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }

        s
    }

    fn read_string(&mut self) -> Result<String, String> {
        let mut s = String::new();

        self.advance(); // skip opening "

        while let Some(c) = self.current {
            if c == '"' {
                break;
            }

            if c == '\\' {
                let escape_start = (self.line, self.column);
                self.advance();
                match self.current {
                    Some('n') => s.push('\n'),
                    Some('r') => s.push('\r'),
                    Some('t') => s.push('\t'),
                    Some('\\') => s.push('\\'),
                    Some('"') => s.push('"'),
                    Some(other) => {
                        return Err(format!(
                            "Invalid escape sequence \\{} at line {}, column {}",
                            other, escape_start.0, escape_start.1
                        ))
                    }
                    None => {
                        return Err(format!(
                            "Invalid escape sequence at end of file (line {}, column {})",
                            escape_start.0, escape_start.1
                        ))
                    }
                }
            } else {
                s.push(c);
            }

            self.advance();
        }

        self.advance(); // closing "
        Ok(s)
    }

    fn read_raw_string(&mut self) -> String {
        let mut s = String::new();

        self.advance(); // skip opening "

        while let Some(c) = self.current {
            if c == '"' {
                break;
            }
            s.push(c);
            self.advance();
        }

        self.advance(); // closing "
        s
    }

    fn read_comment(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self.current {
            if c == '\n' {
                break;
            }
            s.push(c);
            self.advance();
        }
        s
    }

    fn is_bare_string_char(c: char) -> bool {
        !c.is_whitespace() && !matches!(c, '{' | '}' | '"' | '#' | '[' | ']' | ',' | ';')
    }

    fn read_bare_string(&mut self) -> String {
        let mut s = String::new();

        while let Some(c) = self.current {
            if Self::is_bare_string_char(c) {
                s.push(c);
                self.advance();
            } else {
                break;
            }
        }

        s
    }

    /// Returns `true` if a bare string is allowed at this position.
    ///
    /// Bare strings are only allowed after certain token types to avoid
    /// ambiguity with host blocks.
    fn allow_bare_string(&self) -> bool {
        matches!(
            self.prev_token,
            Some(TokenKind::Identifier)
                | Some(TokenKind::Number)
                | Some(TokenKind::StringQuoted)
                | Some(TokenKind::StringRaw)
                | Some(TokenKind::StringBare)
                | Some(TokenKind::Boolean)
                | Some(TokenKind::OpRegex)
                | Some(TokenKind::OpNotRegex)
                | Some(TokenKind::OpEq)
                | Some(TokenKind::OpNeq)
                | Some(TokenKind::OpIn)
                | Some(TokenKind::LBracket)
        )
    }

    /// Tokenizes the next token from the input.
    ///
    /// This is the main tokenization loop that:
    /// 1. Skips whitespace
    /// 2. Matches the next token based on the current character
    /// 3. Updates position tracking
    ///
    /// # Returns
    ///
    /// The next [`Token`], or `EOF` if at the end of input.
    pub fn next_token(&mut self) -> Result<Token, String> {
        loop {
            let (had_newlines, had_whitespace, blank_lines) = self.skip_whitespace();
            self.blank_line_counts.push(blank_lines);

            let span = Span {
                line: self.line,
                column: self.column,
            };

            let mut token = match self.current {
                Some('{') if self.peek() == Some('{') => {
                    self.advance();
                    self.advance();
                    Ok::<_, String>(Token::bare(TokenKind::InterpStart, span))
                }

                Some('}') if self.peek() == Some('}') => {
                    self.advance();
                    self.advance();
                    Ok(Token::bare(TokenKind::InterpEnd, span))
                }

                Some('{') => {
                    self.advance();
                    Ok(Token::bare(TokenKind::LBrace, span))
                }

                Some('}') => {
                    self.advance();
                    Ok(Token::bare(TokenKind::RBrace, span))
                }

                Some('[') => {
                    self.advance();
                    Ok(Token::bare(TokenKind::LBracket, span))
                }

                Some(']') => {
                    self.advance();
                    Ok(Token::bare(TokenKind::RBracket, span))
                }

                Some(';') => {
                    self.advance();
                    Ok(Token::bare(TokenKind::Semicolon, span))
                }

                Some('r') if self.peek() == Some('"') => {
                    self.advance(); // skip 'r', now at '"'
                    let value = self.read_raw_string();
                    Ok(Token::with_lexeme(TokenKind::StringRaw, value, span))
                }

                Some('"') => {
                    let value = self.read_string()?;
                    Ok(Token::with_lexeme(TokenKind::StringQuoted, value, span))
                }

                Some('#') => {
                    let comment_text = self.read_comment();
                    let is_trailing = !had_newlines
                        && self.last_non_comment_line == span.line
                        && self.prev_token.is_some();
                    let kind = if is_trailing {
                        TokenKind::TrailingComment
                    } else {
                        TokenKind::Comment
                    };
                    let mut t = Token::with_lexeme(kind, comment_text, span);
                    t.had_whitespace = had_whitespace;
                    return Ok(t);
                }

                Some('=') if self.peek() == Some('=') => {
                    self.advance();
                    self.advance();
                    Ok(Token::bare(TokenKind::OpEq, span))
                }

                Some('!') if self.peek() == Some('=') => {
                    self.advance();
                    self.advance();
                    Ok(Token::bare(TokenKind::OpNeq, span))
                }

                Some('~') => {
                    self.advance();
                    Ok(Token::bare(TokenKind::OpRegex, span))
                }

                Some('!') if self.peek() == Some('~') => {
                    self.advance();
                    self.advance();
                    Ok(Token::bare(TokenKind::OpNotRegex, span))
                }

                Some(c) if c.is_ascii_digit() => {
                    let n = self.read_number();
                    Ok(Token::with_lexeme(TokenKind::Number, n, span))
                }

                Some('-') if self.peek().is_some_and(|p| p.is_ascii_digit()) => {
                    self.advance();
                    let n = self.read_number();
                    Ok(Token::with_lexeme(TokenKind::Number, format!("-{n}"), span))
                }

                Some('+') if self.peek().is_some_and(|p| p.is_ascii_digit()) => {
                    self.advance();
                    let n = self.read_number();
                    Ok(Token::with_lexeme(TokenKind::Number, n, span))
                }

                Some('*') if !self.allow_bare_string() || had_newlines => {
                    self.advance();
                    Ok(Token::with_lexeme(TokenKind::StringBare, "*".to_string(), span))
                }

                Some(c)
                    if Self::is_bare_string_char(c)
                        && self.allow_bare_string()
                        && !had_newlines =>
                {
                    let value = self.read_bare_string();
                    let kind = if value == "true" || value == "false" {
                        TokenKind::Boolean
                    } else {
                        TokenKind::StringBare
                    };
                    Ok(Token::with_lexeme(kind, value, span))
                }

                Some(c) if c.is_alphabetic() => {
                    let id = self.read_identifier();
                    let kind = self.identifier_token(&id);
                    Ok(Token::with_lexeme(kind, id, span))
                }

                None => Ok(Token::bare(TokenKind::EOF, span)),

                _ => {
                    self.advance();
                    continue;
                }
            }?;

            token.had_whitespace = had_whitespace;
            self.prev_token = Some(token.kind);
            self.last_non_comment_line = span.line;
            return Ok(token);
        }
    }
}

impl Lexer<'_> {
    /// Returns the next token in the input.
    ///
    /// Returns `None` after EOF has been returned once.
    /// Returns `Err` on lexer errors (e.g., invalid escape sequences).
    pub fn next_or_error(&mut self) -> Result<Option<Token>, String> {
        if self
            .prev_token
            .as_ref()
            .is_some_and(|k| *k == TokenKind::EOF)
        {
            return Ok(None);
        }
        Ok(Some(self.next_token()?))
    }
}

impl Iterator for Lexer<'_> {
    type Item = Token;

    /// Returns the next token in the input.
    ///
    /// Returns `None` after EOF has been returned once.
    /// Panics on lexer errors (use `next_or_error` for fallible iteration).
    fn next(&mut self) -> Option<Self::Item> {
        self.next_or_error()
            .expect("lexer error during non-fallible iteration")
    }
}
