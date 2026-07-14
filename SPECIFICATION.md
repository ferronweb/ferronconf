# `ferron.conf` file format specification (v1.3)

## 1. Overview

The `ferron.conf` format is a domain-specific configuration language designed for custom web server configurations. It supports directives, host-based blocks, match conditions, and reusable snippets.

This specification defines the formal syntax of the format based on its EBNF grammar and the reference Rust implementation.

## 2. Lexical structure

### 2.1 Character set

The configuration file is encoded in UTF-8 and contains:
- **Alphabetic characters** - `A-Z`, `a-z`
- **Numeric digits** - `0-9`
- **Special symbols** - `{ } [ ] : . * , - = ! ~ / + _ " \ # % & ? @ ;`

### 2.2 Whitespace and comments

- **Whitespace** (spaces, tabs, newlines) is discarded by the lexer except where syntactically significant.
- **Comments** begin with `#` and extend to the end of the line.

### 2.3 Tokens

| Token Type | Description | Examples |
|------------|-------------|----------|
| `Identifier` | Alphanumeric sequence starting with a letter | `server_name`, `max_connections` |
| `Number` | Integer or decimal value (sign absorbed from `-`/`+` before digit) | `80`, `443`, `1.5`, `-10` |
| `StringQuoted` | Double-quoted string (supports escapes) | `"example.com"`, `"path/to/file"` |
| `StringRaw` | Raw double-quoted string (no escape processing, for regex) | `r"^/api/v1$"`, `r"\n"` |
| `StringBare` | Unquoted string of non-whitespace, non-structural characters | `localhost`, `index.html`, `*` |
| `Boolean` | Literal values `true` or `false` | `true`, `false` |
| `Interpolation` | Variable interpolation syntax | `${variable}`, `{{path.to.value}}` |
| `Semicolon` | Optional statement delimiter | `;` |

## 3. Syntax grammar

### 3.1 Top-level structure

```ebnf
config          ::= ( ';' | statement )* EOF

statement       ::= directive
                  | host-block
                  | match-block
                  | global-block
                  | snippet-block
```

A configuration file consists of zero or more statements at the top level. Semicolons (`;`) may appear between statements as optional delimiters.

## 4. Statement types

### 4.1 Directives

Directives define configuration parameters with optional values and blocks:

```ebnf
directive       ::= identifier value* block?
value           ::= string | number | boolean | interpolation
block           ::= '{' statement* '}'
interpolation   ::= '{{' identifier-path '}}'
identifier-path ::= identifier ( '.' identifier )*
```

**Examples:**
```ferron
server_name example.com
max_connections 1000
enabled true
cert "{{env.TLS_CERT}}"
```

### 4.2 Host blocks

Host blocks apply configuration rules to specific hosts:

```ebnf
host-block      ::= host-pattern ( ',' host-pattern )* block
host-pattern    ::= protocol? host ( ':' port )?
protocol        ::= identifier | bare-string
host            ::= '*' | hostname | ipv4 | '[' ipv6 ']'
hostname        ::= host-label ( '.' host-label )*
host-label      ::= identifier | '*'
ipv4            ::= dec-octet '.' dec-octet '.' dec-octet '.' dec-octet
dec-octet       ::= DIGIT+  /* validated as 0–255 */
ipv6            ::= ipv6-hex ( ':' ipv6-hex )*
ipv6-hex         ::= ( DIGIT | [A-Fa-f] )*
port            ::= DIGIT+
```

**Examples:**
```ferron
example.com {
    root /var/www/example
}

*.example.com:80, example.org:443 {
    tls {
        provider "acme"
        challenge http-01
    }
}

http api.example.com {
    proxy http://localhost:3000
}

[2001:db8::1]:8080 {
    root /ipv6-only
}
```

**Notes:**
- Host blocks are only allowed at the top level.
- The `*` wildcard matches any hostname or host label.
- IPv6 addresses must be enclosed in square brackets.
- Commas between host patterns are lexer-transparent (they are stripped by the lexer, not tokenized).
- Semicolons (`;`) may also appear between host patterns as optional delimiters.

### 4.3 Global blocks

Global blocks apply configuration globally:

```ebnf
global-block    ::= block
```

**Example:**
```ferron
{
    runtime {
        io_uring true
    }

    tcp {
        listen "::"
    }

    default_http_port 8080
    default_https_port 8443
}
```

**Notes:**
- Global blocks are only allowed at the top level.
- They contain statements that apply to all hosts unless overridden.

### 4.4 Snippet blocks

Snippet blocks define reusable configuration fragments:

```ebnf
snippet-block   ::= 'snippet' identifier block
```

**Example:**
```ferron
snippet tls_acme {
    tls {
        provider "acme"
        challenge http-01
        contact "admin@example.com"
    }
}
```

Snippets can be referenced elsewhere in the configuration (implementation-dependent).

### 4.5 Match blocks

Match blocks define conditional logic based on request attributes:

```ebnf
match-block     ::= 'match' identifier matcher-block
matcher-block   ::= '{' matcher-expression* '}'
matcher-expression
                ::= operand operator operand
operator        ::= '==' | '!=' | '~' | '!~' | 'in'
operand         ::= identifier-path | string | number
```

**Examples:**
```ferron
match curl_client {
    request.header.user_agent ~ "curl"
}

match api_request {
    request.uri.path ~ "/api"
    request.method in "GET,POST"
}

match english_language {
    "en" in request.header.accept_language
}
```

**Operators:**
| Operator | Meaning | Example |
|----------|---------|---------|
| `==` | String equality | `request.method == "GET"` |
| `!=` | String inequality | `request.scheme != "https"` |
| `~` | Regex match | `request.header.user-agent ~ "Chrome.*"` |
| `!~` | Negated regex | `request.header.host !~ "^test\\."` |
| `in` | Membership / language match | `request.method in "GET,POST"` |

## 5. Data types

### 5.1 Strings

Strings can be specified as:
- **Quoted strings** - enclosed in double quotes, support escape sequences (`\n`, `\r`, `\t`, `\\`, `\"`). Invalid escape sequences (e.g., `\z`, `\$`) are a parse error.
- **Raw strings** - prefixed with `r`, enclosed in double quotes. No escape processing — all characters are literal. Useful for regex patterns (e.g., `r"^/api/v1(?:/|$)"`).
- **Bare strings** - unquoted sequences of any characters except whitespace, `{`, `}`, `"`, `#`, `,`, and `;`. Including characters like `.`, `:`, `*`, `/`, `[`, and `]` directly. Note that `[`/`]` brackets are structural at line start for IPv6 host patterns, but part of bare strings in directive arguments.

**Escape sequences (quoted strings only):**
| Escape | Character |
|--------|-----------|
| `\n` | newline |
| `\r` | carriage return |
| `\t` | tab |
| `\\` | backslash |
| `\"` | double quote |

Other escape sequences (e.g., `\z`, `\$`, `\x41`) are **invalid** and produce a parse error. Use a raw string (`r"..."`) if you need literal backslashes.

### 5.2 Numbers

Numbers support integers and decimals:
```ebnf
number ::= ( '-' | '+' )? DIGIT+ ( '.' DIGIT+ )?
```

**Examples:** `80`, `443`, `1.5`, `-10`

### 5.3 Booleans

Boolean literals are case-sensitive:
- `true` — enabled/positive value
- `false` — disabled/negative value

## 6. Interpolation

Interpolation allows referencing variables, environment variables, or configuration values:

```ebnf
interpolation ::= '{{' identifier-path '}}'
identifier-path ::= identifier ( '.' identifier )*
```

**Examples:**
```ferron
cert "{{env.TLS_CERT}}"
key "{{env.TLS_KEY}}"
header +X-Client-IP "{{remote_address}}"
timeout {{config.defaults.timeout}}
```

Common interpolation variables:

| Variable | Description |
|----------|-------------|
| `{{env.NAME}}` | Environment variable `NAME` |
| `{{remote_address}}` | Client IP address |
| `{{local_address}}` | Server listening address |
| `{{hostname}}` | Matched hostname |
| `{{scheme}}` | Request scheme (`http` or `https`) |

Unresolved variables are left as `{{name}}` in the output.

## 7. Syntax examples

### Complete configuration example

```ferron
# Global defaults
{
    runtime {
        io_uring true
    }

    tcp {
        listen "::"
    }

    default_http_port 80
    default_https_port 443

    admin {
        listen "127.0.0.1:8081"
        health true
        status true
    }
}

# Snippet definition
snippet tls_acme {
    tls {
        provider "acme"
        challenge http-01
        contact "admin@example.com"
    }
}

# Host-specific configuration
example.com:443 {
    use tls_acme

    root /var/www/example
    index index.html index.htm
    directory_listing
    compressed

    log "access" {
        format "combined"
    }
}

# Wildcard with DNS-01 challenge
*.example.com {
    tls {
        provider "acme"
        challenge dns-01
        contact "admin@example.com"
        dns "cloudflare" {
            api_key "EXAMPLE_API_KEY"
        }
    }

    root /var/www/multi-tenant
}

# Reverse proxy with load balancing
api.example.com {
    proxy http://localhost:3000 http://localhost:3001 {
        lb_algorithm two_random
        keepalive true
        http2 true

        request_header +X-Real-IP "{{remote_address}}"
    }

    rate_limit {
        rate 100
        burst 50
        key remote_address
    }

    cors {
        origins "https://app.example.com"
        methods GET POST PUT DELETE
        headers "Content-Type" "Authorization"
        credentials true
    }
}

# Match-based routing
match api_request {
    request.uri.path ~ "/api"
    request.method in "GET,POST"
}

match curl_client {
    request.header.user_agent ~ "curl"
}

# Location and conditional blocks
example.com {
    root /var/www/example

    location /static {
        file_cache_control "public, max-age=31536000"
    }

    location /admin {
        if curl_client {
            status 403 {
                body "Forbidden"
            }
        }
    }
}
```

## 8. Error handling

### 8.1 Parse errors

The reference parser reports errors with:
- **Message** - description of the error
- **Span** - line and column position where the error occurred

### 8.2 Validation rules

- IPv4 octets must be in range 0–255 (validated by parser)
- Host patterns require proper formatting
- Match expressions require valid operands and operators

## 9. Implementation notes

### 9.1 Lexer behavior

- Bare strings are only allowed after certain token types (identifiers, numbers, quoted strings, raw strings, operators) to avoid ambiguity.
- The lexer is case-sensitive for keywords (`match`, `snippet`) and boolean values.
- Characters `.`, `:`, `*`, `-`, `+`, `/`, `%`, `&`, `?`, `@` are absorbed into bare strings or numbers; they no longer have dedicated tokens.
- The `*` wildcard in host patterns produces a `StringBare("*")` token.
- Commas (`,`) and semicolons (`;`) between tokens are discarded by the lexer.
- IPv6 addresses use `[`/`]` as bracket tokens; `:8080` after `]` is parsed as a `Number` token for the port.
- `[` and `]` are structural at line start (for IPv6 host patterns) but part of bare strings in directive arguments.
- Each token records a `had_whitespace` flag indicating whether whitespace preceded it. This is used by the parser for adjacency validation.
- Invalid escape sequences in quoted strings (e.g., `\z`, `\$`) are a lexer error.
- Raw strings (`r"..."`) are tokenized as `StringRaw` tokens with the content between the quotes taken literally — no escape processing.

### 9.2 Parser behavior

- Host patterns can be comma-separated in host blocks.
- `;` is an optional delimiter that can appear between statements (at top level or inside blocks) and also between host patterns.  
- Interpolation syntax uses double braces `{{ }}`. Raw strings (`r"..."`) do **not** support interpolation — `{{...}}` inside a raw string is literal text.
- Match expressions are evaluated sequentially within a match block.
- Host pattern protocol detection uses lookahead: if a single label precedes a non-dotted bare string, it becomes the protocol.
- Dotted paths in identifiers and interpolation split on `.` to produce path segments.
- Number literals that include a `.` are split into a `Number` token and a `StringBare` continuation (e.g., `3.14` → `Number("3")` + `StringBare(".14")`).
- A `Number` token immediately followed by `[` without whitespace is rejected as an error (e.g., `34[::1]34`), preventing silent token jamming.
- Two value tokens adjacent without whitespace are rejected as an error (e.g., `"a""a"`, `80-443`).

## 10. Backward compatibility

This specification defines version 1.3 of the Ferron configuration format. Future versions may extend the grammar with additional features while maintaining backward compatibility where possible.
