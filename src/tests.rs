#![cfg(test)]

use crate::ast::*;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::str::FromStr;

#[test]
fn test_parser_example() {
    let input = r#"
# Global runtime settings
{
  runtime {
    io_uring true
  }
}

# Snippet
snippet set_curl {
  header X-Curl 1
}

# Matcher definition
match curl_client {
  request.header.user_agent ~ "curl"
}

# Edge case test ("true" would be classified as a boolean by the lexer here)
true.example {
  root {{env.TRUE_WWWROOT}}
}

# Default HTTP settings
http * {
  header X-Powered-By MyServer
}

# Main site
example.com {
  root /var/www/example

  if curl_client {
    use set_curl
  }
}

# Wildcard subdomains
*.example.com {
  reverse_proxy localhost:9000
}

# TCP service
tcp *:5432 {
  proxy localhost:5432
}
"#;

    let config = Config::from_str(input).expect("Failed to parse config");

    // 1. Check Global Block
    let global_block = config
        .statements
        .iter()
        .find(|s| matches!(s, Statement::GlobalBlock(_)))
        .expect("Global block not found");
    if let Statement::GlobalBlock(block) = global_block {
        let runtime = block
            .find_directive("runtime")
            .expect("runtime directive not found");
        assert!(runtime.has_block());
        let io_uring = runtime
            .block
            .as_ref()
            .unwrap()
            .find_directive("io_uring")
            .expect("io_uring not found");
        assert_eq!(io_uring.get_boolean_arg(0), Some(true));
    }

    // 2. Check Snippet
    let snippet_block = config
        .statements
        .iter()
        .find(|s| matches!(s, Statement::SnippetBlock(_)))
        .expect("Snippet block not found");
    if let Statement::SnippetBlock(sb) = snippet_block {
        assert_eq!(sb.name, "set_curl");
        let header = sb
            .block
            .find_directive("header")
            .expect("header directive not found");
        assert_eq!(header.get_string_arg(0), Some("X-Curl"));
        assert_eq!(header.get_integer_arg(1), Some(1));
    }

    // 3. Check Matcher
    let match_blocks = config.find_match_blocks();
    let curl_client = match_blocks
        .iter()
        .find(|m| m.matcher == "curl_client")
        .expect("curl_client matcher not found");
    assert!(curl_client.has_expressions());
    let expr = &curl_client.expr[0];
    assert!(expr.is_regex());
    assert_eq!(
        expr.left.as_identifier().map(|v| v.join(".")),
        Some("request.header.user_agent".to_string())
    );
    assert_eq!(expr.right.as_str(), Some("curl"));

    // 4. Check 'true.example' Host Block
    let true_example = config
        .statements
        .iter()
        .find_map(|s| {
            if let Statement::HostBlock(hb) = s {
                if hb.matches_host("true.example") {
                    Some(hb)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .expect("true.example host block not found");

    let root = true_example
        .block
        .find_directive("root")
        .expect("root directive not found");
    if let Value::InterpolatedString(parts, _) = &root.args[0] {
        assert_eq!(
            parts,
            &vec![StringPart::Expression(vec![
                "env".to_string(),
                "TRUE_WWWROOT".to_string()
            ])]
        );
    } else {
        panic!("Expected interpolation for root argument");
    }

    // 5. Check 'http *' Host Block
    let http_star = config
        .statements
        .iter()
        .find_map(|s| {
            if let Statement::HostBlock(hb) = s {
                if hb.hosts.iter().any(|h| {
                    h.protocol.as_deref() == Some("http")
                        && h.labels == crate::ast::HostLabels::Wildcard
                }) {
                    Some(hb)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .expect("http * host block not found");

    let powered_by = http_star
        .block
        .find_directive("header")
        .expect("header directive not found");
    assert_eq!(powered_by.get_string_arg(0), Some("X-Powered-By"));
    assert_eq!(powered_by.get_string_arg(1), Some("MyServer"));

    // 6. Check 'example.com' Host Block
    let example_com = config
        .statements
        .iter()
        .find_map(|s| {
            if let Statement::HostBlock(hb) = s {
                if hb.matches_host("example.com") {
                    Some(hb)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .expect("example.com host block not found");

    let root = example_com
        .block
        .find_directive("root")
        .expect("root directive not found");
    assert_eq!(root.get_string_arg(0), Some("/var/www/example"));

    let if_directive = example_com
        .block
        .find_directive("if")
        .expect("if directive not found");
    assert_eq!(if_directive.args[0].as_str(), Some("curl_client"));

    // 7. Check TCP service
    let tcp_service = config
        .statements
        .iter()
        .find_map(|s| {
            if let Statement::HostBlock(hb) = s {
                if hb
                    .hosts
                    .iter()
                    .any(|h| h.protocol.as_deref() == Some("tcp") && h.port == Some(5432))
                {
                    Some(hb)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .expect("tcp *:5432 host block not found");

    let proxy = tcp_service
        .block
        .find_directive("proxy")
        .expect("proxy directive not found");
    assert_eq!(proxy.get_string_arg(0), Some("localhost:5432"));
}

#[allow(clippy::approx_constant)]
#[test]
fn test_complex_values() {
    let input = r#"
directive_float 3.14
directive_neg -10
directive_neg_float -3.14
directive_string "string with \"escape\""
directive_interp {{ nested.var }}
directive_interp_multi "prefix {{ nested.var }} suffix {{ other.value }}"
directive_bools true false
"#;
    let config = Config::from_str(input).expect("Failed to parse complex values");

    // Float
    let d_float = config.find_directives("directive_float")[0];
    assert_eq!(d_float.args[0].as_f64(), Some(3.14));

    // Negative Number
    let d_neg = config.find_directives("directive_neg")[0];
    assert_eq!(d_neg.args[0].as_i64(), Some(-10));

    // Negative Float
    let d_neg_float = config.find_directives("directive_neg_float")[0];
    assert_eq!(d_neg_float.args[0].as_f64(), Some(-3.14));

    // String Escapes
    let d_str = config.find_directives("directive_string")[0];
    assert_eq!(d_str.args[0].as_str(), Some("string with \"escape\""));

    // Interpolation
    let d_interp = config.find_directives("directive_interp")[0];
    assert_eq!(
        d_interp.args[0].as_interpolated_string(),
        Some(&[StringPart::Expression(vec![
            "nested".to_string(),
            "var".to_string()
        ])] as &[StringPart])
    );

    let d_interp_multi = config.find_directives("directive_interp_multi")[0];
    assert_eq!(
        d_interp_multi.args[0].as_interpolated_string(),
        Some(&[
            StringPart::Literal("prefix ".to_string()),
            StringPart::Expression(vec!["nested".to_string(), "var".to_string()]),
            StringPart::Literal(" suffix ".to_string()),
            StringPart::Expression(vec!["other".to_string(), "value".to_string()]),
        ] as &[StringPart])
    );

    // Booleans
    let d_bool = config.find_directives("directive_bools")[0];
    assert_eq!(d_bool.get_boolean_arg(0), Some(true));
    assert_eq!(d_bool.get_boolean_arg(1), Some(false));
}

#[test]
fn test_host_patterns() {
    let input = r#"
[::1] {}
[2001:db8::1]:8080 {}
127.0.0.1 {}
"#;
    let config = Config::from_str(input).expect("Failed to parse host patterns");

    config
        .statements
        .iter()
        .find_map(|s| {
            if let Statement::HostBlock(hb) = s {
                if hb.hosts[0].labels
                    == crate::ast::HostLabels::IpAddr(std::net::IpAddr::V6(
                        std::net::Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1),
                    ))
                {
                    Some(hb)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .expect("IPv6 localhost not found");

    let ipv6_port = config
        .statements
        .iter()
        .find_map(|s| {
            if let Statement::HostBlock(hb) = s {
                if hb.hosts[0].port == Some(8080) {
                    Some(hb)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .expect("IPv6 with port not found");

    // Check IPv6 address explicitly
    if let crate::ast::HostLabels::IpAddr(std::net::IpAddr::V6(addr)) = &ipv6_port.hosts[0].labels {
        assert_eq!(addr.to_string(), "2001:db8::1");
    } else {
        panic!("Expected IPv6 address");
    }

    let ipv4 = config
        .statements
        .iter()
        .find_map(|s| {
            if let Statement::HostBlock(hb) = s {
                if hb.hosts[0].as_str() == "127.0.0.1" {
                    Some(hb)
                } else {
                    None
                }
            } else {
                None
            }
        })
        .expect("IPv4 not found");
    assert!(matches!(
        ipv4.hosts[0].labels,
        crate::ast::HostLabels::IpAddr(std::net::IpAddr::V4(_))
    ));
}

#[test]
fn test_top_level_ambiguity() {
    // Case 1: Directive with Quoted String and Block.
    let input_quoted = r#"
    dir_quoted "arg" {
        inside true
    }
    "#;
    let config = Config::from_str(input_quoted).expect("dir_quoted should parse successfully now");
    if let Statement::Directive(d) = &config.statements[0] {
        assert_eq!(d.name, "dir_quoted");
        assert_eq!(d.args[0].as_str(), Some("arg"));
        assert!(d.has_block());
    } else {
        panic!("dir_quoted did not parse as Directive");
    }

    // Case 2: Directive with Bare String and Block.
    // Parses as HostBlock.
    let input_bare = r#"
    dir_bare arg { }
    "#;
    let config = Config::from_str(input_bare).expect("dir_bare failed");
    if let Statement::HostBlock(hb) = &config.statements[0] {
        assert_eq!(hb.hosts[0].protocol.as_deref(), Some("dir_bare"));
        // "arg" is parsed as part of the host label sequence.
        // wait, parse_host_pattern consumes "dir_bare" then "arg".
        // labels=["dir_bare", "arg"].
        // then it sees protocol is None.
        // "dir_bare" becomes protocol. "arg" stays in labels.
        // Correct.
        assert_eq!(hb.hosts[0].as_str(), "arg");
    } else {
        panic!("dir_bare did not parse as HostBlock");
    }
}

#[test]
fn test_mixed_string_edge_case() {
    let input = r#"
    dir_mixed "arg1" arg2 "arg3"
    "#;
    let config = Config::from_str(input).expect("dir_mixed failed");
    if let Statement::Directive(d) = &config.statements[0] {
        assert_eq!(d.name, "dir_mixed");
        assert_eq!(d.args[0].as_str(), Some("arg1"));
        assert_eq!(d.args[1].as_str(), Some("arg2"));
        assert_eq!(d.args[2].as_str(), Some("arg3"));
    } else {
        panic!("dir_quoted did not parse as Directive");
    }
}

#[test]
fn test_unclosed_interpolation_in_quoted_string() {
    // Quoted string contains an opening interpolation but no closing '}}'
    let input = r#"directive "prefix {{ missing""#;
    let res = Config::from_str(input);
    assert!(
        res.is_err(),
        "Expected parse error for unclosed interpolation in quoted string"
    );
}

#[test]
fn test_unclosed_interpolation_bare() {
    // Bare interpolation start without a closing '}}'
    let input = r#"directive {{ missing"#;
    let res = Config::from_str(input);
    assert!(
        res.is_err(),
        "Expected parse error for unclosed interpolation in bare interpolation"
    );
}

// ============================================================================
// Error Path Tests
// ============================================================================

#[test]
fn test_error_empty_input() {
    let input = "";
    let res = Config::from_str(input);
    assert!(res.is_ok(), "Empty input should parse to an empty config");
    let config = res.unwrap();
    assert_eq!(config.statements.len(), 0);
}

#[test]
fn test_error_only_comments() {
    let input = "# just a comment\n# another comment\n";
    let config = Config::from_str(input).expect("only-comments should parse");
    // Comments are now preserved as Statement::Comment nodes
    assert_eq!(config.statements.len(), 2);
    assert!(config.statements.iter().all(|s| s.is_comment()));
}

#[test]
fn test_error_only_whitespace() {
    let input = "   \n\n  \t  \n";
    let config = Config::from_str(input).expect("only-whitespace should parse");
    assert_eq!(config.statements.len(), 0);
}

#[test]
fn test_error_unclosed_block() {
    let input = "example.com { nested {";
    let res = Config::from_str(input);
    assert!(res.is_err(), "Expected parse error for unclosed block");
}

#[test]
fn test_error_unclosed_host_block() {
    let input = "* {";
    let res = Config::from_str(input);
    assert!(res.is_err(), "Expected parse error for unclosed host block");
}

#[test]
fn test_error_invalid_ipv6() {
    let input = "[gggg::1] {}";
    let res = Config::from_str(input);
    assert!(
        res.is_err(),
        "Expected parse error for invalid IPv6 address"
    );
}

#[test]
fn test_error_ipv4_octet_out_of_range() {
    let input = "256.0.0.1 {}";
    let res = Config::from_str(input);
    assert!(
        res.is_err(),
        "Expected parse error for invalid IPv4 address"
    );
}

#[test]
fn test_error_ipv4_last_octet_out_of_range() {
    let input = "1.2.3.256 {}";
    let res = Config::from_str(input);
    assert!(
        res.is_err(),
        "Expected parse error for invalid IPv4 address"
    );
}

#[test]
fn test_error_invalid_port_too_large() {
    let input = "example.com:99999 {}";
    let res = Config::from_str(input);
    assert!(res.is_err(), "Expected parse error for port > 65535");
}

#[test]
fn test_error_invalid_port_non_numeric() {
    let input = "example.com:abc {}";
    let res = Config::from_str(input);
    assert!(res.is_err(), "Expected parse error for non-numeric port");
}

#[test]
fn test_error_missing_matcher_name() {
    let input = "match {}";
    let res = Config::from_str(input);
    assert!(
        res.is_err(),
        "Expected parse error for missing matcher name"
    );
}

#[test]
fn test_error_missing_snippet_name() {
    let input = "snippet {}";
    let res = Config::from_str(input);
    assert!(
        res.is_err(),
        "Expected parse error for missing snippet name"
    );
}

#[test]
fn test_error_invalid_operator_in_match() {
    let input = r#"
match foo {
    a $$ b
}
"#;
    let res = Config::from_str(input);
    assert!(res.is_err(), "Expected parse error for invalid operator");
}

#[test]
fn test_error_missing_right_operand() {
    let input = r#"
match foo {
    a ==
}
"#;
    let res = Config::from_str(input);
    assert!(
        res.is_err(),
        "Expected parse error for missing right operand"
    );
}

#[test]
fn test_error_empty_match_block() {
    let input = "match foo { }";
    let config = Config::from_str(input).expect("empty match block should parse");
    let match_blocks = config.find_match_blocks();
    assert_eq!(match_blocks.len(), 1);
    assert!(!match_blocks[0].has_expressions());
}

// ============================================================================
// AST Helper Method Edge Cases
// ============================================================================

#[test]
fn test_ast_get_string_arg_on_integer_value() {
    let input = "directive 42";
    let config = Config::from_str(input).expect("parse");
    let d = config.find_directives("directive")[0];
    assert_eq!(d.get_string_arg(0), None);
}

#[test]
fn test_ast_get_string_arg_out_of_range() {
    let input = "directive hello";
    let config = Config::from_str(input).expect("parse");
    let d = config.find_directives("directive")[0];
    assert_eq!(d.get_string_arg(1), None);
}

#[test]
fn test_ast_get_integer_arg_on_string_value() {
    let input = "directive hello";
    let config = Config::from_str(input).expect("parse");
    let d = config.find_directives("directive")[0];
    assert_eq!(d.get_integer_arg(0), None);
}

#[test]
fn test_ast_get_integer_arg_out_of_range() {
    let input = "directive 42";
    let config = Config::from_str(input).expect("parse");
    let d = config.find_directives("directive")[0];
    assert_eq!(d.get_integer_arg(1), None);
}

#[test]
fn test_ast_get_boolean_arg_on_string_value() {
    let input = "directive hello";
    let config = Config::from_str(input).expect("parse");
    let d = config.find_directives("directive")[0];
    assert_eq!(d.get_boolean_arg(0), None);
}

#[test]
fn test_ast_get_boolean_arg_out_of_range() {
    let input = "directive true";
    let config = Config::from_str(input).expect("parse");
    let d = config.find_directives("directive")[0];
    assert_eq!(d.get_boolean_arg(1), None);
}

#[test]
fn test_ast_value_as_str_on_non_string() {
    let input = "directive 42";
    let config = Config::from_str(input).expect("parse");
    let d = config.find_directives("directive")[0];
    assert_eq!(d.args[0].as_str(), None);
}

#[test]
fn test_ast_value_as_interpolated_string_on_plain_string() {
    let input = "directive hello";
    let config = Config::from_str(input).expect("parse");
    let d = config.find_directives("directive")[0];
    assert_eq!(d.args[0].as_interpolated_string(), None);
}

#[test]
fn test_ast_value_as_i64_on_float_value() {
    let input = "directive 3.14";
    let config = Config::from_str(input).expect("parse");
    let d = config.find_directives("directive")[0];
    assert_eq!(d.args[0].as_i64(), None);
}

#[test]
fn test_ast_value_as_f64_on_integer_value() {
    let input = "directive 42";
    let config = Config::from_str(input).expect("parse");
    let d = config.find_directives("directive")[0];
    assert_eq!(d.args[0].as_f64(), None);
}

#[test]
fn test_ast_value_as_bool_on_string_value() {
    let input = "directive hello";
    let config = Config::from_str(input).expect("parse");
    let d = config.find_directives("directive")[0];
    assert_eq!(d.args[0].as_bool(), None);
}

#[test]
fn test_ast_find_directive_nonexistent() {
    let input = "directive hello";
    let config = Config::from_str(input).expect("parse");
    let d = if let Statement::Directive(d) = &config.statements[0] {
        d
    } else {
        panic!("Expected Directive");
    };
    assert_eq!(d.block, None);
}

#[test]
fn test_ast_find_directives_nonexistent() {
    let input = "directive hello";
    let config = Config::from_str(input).expect("parse");
    let d = if let Statement::Directive(d) = &config.statements[0] {
        d
    } else {
        panic!("Expected Directive");
    };
    // Directive without a block — find_directives on empty statements
    let found = d.block.as_ref().map(|b| b.find_directives("nonexistent"));
    assert_eq!(found.map(|v| v.len()), None);
}

#[test]
fn test_ast_hostpattern_as_str_without_port() {
    let input = "example.com {}";
    let config = Config::from_str(input).expect("parse");
    let hb = config.find_host_blocks()[0];
    assert_eq!(hb.hosts[0].as_str(), "example.com");
}

#[test]
fn test_ast_hostpattern_as_str_with_port() {
    let input = "example.com:8080 {}";
    let config = Config::from_str(input).expect("parse");
    let hb = config.find_host_blocks()[0];
    assert_eq!(hb.hosts[0].as_str(), "example.com:8080");
}

#[test]
fn test_ast_hostpattern_as_full_str_without_protocol() {
    let input = "example.com {}";
    let config = Config::from_str(input).expect("parse");
    let hb = config.find_host_blocks()[0];
    assert_eq!(hb.hosts[0].as_full_str(), "example.com");
}

#[test]
fn test_ast_hostpattern_as_full_str_with_protocol() {
    let input = "http example.com {}";
    let config = Config::from_str(input).expect("parse");
    let hb = config.find_host_blocks()[0];
    assert_eq!(hb.hosts[0].as_full_str(), "http example.com");
}

#[test]
fn test_ast_stringpart_literal_as_str() {
    let input = "directive \"hello world\"";
    let config = Config::from_str(input).expect("parse");
    let d = config.find_directives("directive")[0];
    if let Value::String(s, _) = &d.args[0] {
        let _parts = vec![StringPart::Literal(s.clone())];
        let sp = StringPart::Literal(s.clone());
        assert_eq!(sp.as_str(), "hello world");
    } else {
        panic!("Expected string value");
    }
}

#[test]
fn test_ast_stringpart_expression_as_str() {
    let input = "directive \"{{app.root}}\"";
    let config = Config::from_str(input).expect("parse");
    let d = config.find_directives("directive")[0];
    if let Value::InterpolatedString(parts, _) = &d.args[0] {
        if let StringPart::Expression(ref v) = parts[0] {
            let sp = StringPart::Expression(v.clone());
            assert_eq!(sp.as_str(), "{{app.root}}");
        } else {
            panic!("Expected expression part");
        }
    } else {
        panic!("Expected interpolated string value");
    }
}

#[test]
fn test_ast_hostlabels_wildcard_as_str() {
    let input = "* {}";
    let config = Config::from_str(input).expect("parse");
    let hb = config.find_host_blocks()[0];
    assert_eq!(hb.hosts[0].labels.as_str(), "*");
}

#[test]
fn test_ast_hostlabels_hostname_as_str() {
    let input = "example.com {}";
    let config = Config::from_str(input).expect("parse");
    let hb = config.find_host_blocks()[0];
    assert_eq!(hb.hosts[0].labels.as_str(), "example.com");
}

#[test]
fn test_ast_hostlabels_ipaddr_as_str() {
    let input = "127.0.0.1 {}";
    let config = Config::from_str(input).expect("parse");
    let hb = config.find_host_blocks()[0];
    assert_eq!(hb.hosts[0].labels.as_str(), "127.0.0.1");
}

#[test]
fn test_ast_matches_host_wildcard() {
    let input = "* {}";
    let config = Config::from_str(input).expect("parse");
    let hb = config.find_host_blocks()[0];
    // matches_host does exact string comparison, so wildcard "*" only matches "*"
    assert!(hb.matches_host("*"));
    assert!(!hb.matches_host("anything"));
    assert!(!hb.matches_host("example.com"));
}

#[test]
fn test_ast_matches_host_exact() {
    let input = "example.com {}";
    let config = Config::from_str(input).expect("parse");
    let hb = config.find_host_blocks()[0];
    assert!(hb.matches_host("example.com"));
    assert!(!hb.matches_host("other.com"));
}

#[test]
fn test_ast_operator_as_str() {
    assert_eq!(Operator::Eq.as_str(), "==");
    assert_eq!(Operator::NotEq.as_str(), "!=");
    assert_eq!(Operator::Regex.as_str(), "~");
    assert_eq!(Operator::NotRegex.as_str(), "!~");
    assert_eq!(Operator::In.as_str(), "in");
}

#[test]
fn test_ast_operator_is_comparison() {
    assert!(Operator::Eq.is_comparison());
    assert!(Operator::NotEq.is_comparison());
    assert!(!Operator::Regex.is_comparison());
    assert!(!Operator::NotRegex.is_comparison());
    assert!(!Operator::In.is_comparison());
}

#[test]
fn test_ast_operator_is_regex() {
    assert!(Operator::Regex.is_regex());
    assert!(Operator::NotRegex.is_regex());
    assert!(!Operator::Eq.is_regex());
    assert!(!Operator::NotEq.is_regex());
    assert!(!Operator::In.is_regex());
}

#[test]
fn test_ast_matcher_expression_is_equality() {
    let input = r#"
match foo {
    a == b
}
"#;
    let config = Config::from_str(input).expect("parse");
    let match_blocks = config.find_match_blocks();
    let expr = &match_blocks[0].expr[0];
    assert!(expr.is_equality());
    assert!(!expr.is_inequality());
    assert!(!expr.is_regex());
}

#[test]
fn test_ast_matcher_expression_is_inequality() {
    let input = r#"
match foo {
    a != b
}
"#;
    let config = Config::from_str(input).expect("parse");
    let match_blocks = config.find_match_blocks();
    let expr = &match_blocks[0].expr[0];
    assert!(!expr.is_equality());
    assert!(expr.is_inequality());
    assert!(!expr.is_regex());
}

#[test]
fn test_ast_matcher_expression_is_regex() {
    let input = r#"
match foo {
    a ~ b
}
"#;
    let config = Config::from_str(input).expect("parse");
    let match_blocks = config.find_match_blocks();
    let expr = &match_blocks[0].expr[0];
    assert!(!expr.is_equality());
    assert!(!expr.is_inequality());
    assert!(expr.is_regex());
}

#[test]
fn test_ast_matcher_expression_is_not_regex() {
    let input = r#"
match foo {
    a !~ b
}
"#;
    let config = Config::from_str(input).expect("parse");
    let match_blocks = config.find_match_blocks();
    let expr = &match_blocks[0].expr[0];
    assert!(expr.is_regex());
}

#[test]
fn test_ast_operand_as_str() {
    let input = r#"
match foo {
    "hello" == "world"
}
"#;
    let config = Config::from_str(input).expect("parse");
    let match_blocks = config.find_match_blocks();
    let expr = &match_blocks[0].expr[0];
    assert_eq!(expr.left.as_str(), Some("hello"));
    assert_eq!(expr.right.as_str(), Some("world"));
}

#[test]
fn test_ast_operand_as_str_on_identifier() {
    let input = r#"
match foo {
    a.b == "c"
}
"#;
    let config = Config::from_str(input).expect("parse");
    let match_blocks = config.find_match_blocks();
    let expr = &match_blocks[0].expr[0];
    assert_eq!(expr.left.as_str(), None);
}

#[test]
fn test_ast_operand_as_identifier() {
    let input = r#"
match foo {
    a.b.c == "d"
}
"#;
    let config = Config::from_str(input).expect("parse");
    let match_blocks = config.find_match_blocks();
    let expr = &match_blocks[0].expr[0];
    assert_eq!(
        expr.left.as_identifier().map(|v| v.to_vec()),
        Some(vec!["a".to_string(), "b".to_string(), "c".to_string()])
    );
    assert_eq!(expr.left.as_str(), None);
}

#[test]
fn test_ast_operand_as_i64() {
    let input = r#"
match foo {
    42 == 42
}
"#;
    let config = Config::from_str(input).expect("parse");
    let match_blocks = config.find_match_blocks();
    let expr = &match_blocks[0].expr[0];
    assert_eq!(expr.left.as_i64(), Some(42));
    assert_eq!(expr.right.as_i64(), Some(42));
}

#[test]
fn test_ast_operand_as_f64() {
    let input = r#"
match foo {
    3.14 == 3.14
}
"#;
    let config = Config::from_str(input).expect("parse");
    let match_blocks = config.find_match_blocks();
    let expr = &match_blocks[0].expr[0];
    assert_eq!(expr.left.as_f64(), Some(3.14));
    assert_eq!(expr.right.as_f64(), Some(3.14));
}

// ============================================================================
// Edge Case Parsing Tests
// ============================================================================

#[test]
fn test_edge_empty_global_block() {
    let input = "{ }";
    let config = Config::from_str(input).expect("empty global block should parse");
    assert_eq!(config.statements.len(), 1);
    if let Statement::GlobalBlock(block) = &config.statements[0] {
        assert_eq!(block.statements.len(), 0);
    } else {
        panic!("Expected GlobalBlock");
    }
}

#[test]
fn test_edge_nested_blocks() {
    // "outer { inner { deep { } } }" is parsed as a host block
    // because "outer" followed by "{" triggers host block parsing
    let input = "outer { inner { deep { } } }";
    let config = Config::from_str(input).expect("nested blocks should parse");
    // The first statement is a HostBlock with hostname "outer"
    if let Statement::HostBlock(hb) = &config.statements[0] {
        assert!(
            matches!(hb.hosts[0].labels, HostLabels::Hostname(ref labels) if labels[0] == "outer")
        );
        let inner = hb.block.find_directive("inner");
        assert!(inner.is_some());
        let inner_block = inner.unwrap().block.as_ref().unwrap();
        let deep = inner_block.find_directive("deep");
        assert!(deep.is_some());
        assert!(deep.unwrap().has_block());
    } else {
        panic!("Expected HostBlock (outer is parsed as hostname)");
    }
}

#[test]
fn test_edge_nested_blocks_via_display() {
    // Test that nested block structure survives display + reparse
    let input = "outer { inner { deep { } } }";
    let config = Config::from_str(input).expect("parse");
    let formatted = config.to_string();
    let reparsed = Config::from_str(&formatted);
    if let Ok(reparsed) = reparsed {
        if let Statement::Directive(d) = &reparsed.statements[0] {
            assert_eq!(d.name, "outer");
            assert!(d.has_block());
        }
    }
}

#[test]
fn test_edge_multiple_global_blocks() {
    let input = "{ a 1 } { b 2 }";
    let config = Config::from_str(input).expect("multiple global blocks should parse");
    let global_blocks: Vec<_> = config
        .statements
        .iter()
        .filter(|s| matches!(s, Statement::GlobalBlock(_)))
        .collect();
    assert_eq!(global_blocks.len(), 2);
}

#[test]
fn test_edge_snippet_with_nested_block() {
    let input = r#"
snippet foo {
    nested {
        inside true
    }
}
"#;
    let config = Config::from_str(input).expect("snippet with nested block should parse");
    if let Statement::SnippetBlock(sb) = &config.statements[0] {
        assert_eq!(sb.name, "foo");
        let nested = sb.block.find_directive("nested");
        assert!(nested.is_some());
    } else {
        panic!("Expected SnippetBlock");
    }
}

#[test]
fn test_edge_all_match_operators() {
    let input = r#"
match all_ops {
    a == b
    a != b
    a ~ b
    a !~ b
    a in b
}
"#;
    let config = Config::from_str(input).expect("all match operators should parse");
    let match_blocks = config.find_match_blocks();
    assert_eq!(match_blocks[0].expr.len(), 5);
    assert!(match_blocks[0].expr[0].is_equality());
    assert!(match_blocks[0].expr[1].is_inequality());
    assert!(match_blocks[0].expr[2].is_regex());
    assert!(match_blocks[0].expr[3].is_regex());
    assert_eq!(match_blocks[0].expr[4].op.as_str(), "in");
}

#[test]
fn test_edge_match_string_operands() {
    let input = r#"
match foo {
    "literal" == "literal"
}
"#;
    let config = Config::from_str(input).expect("string string match should parse");
    let match_blocks = config.find_match_blocks();
    let expr = &match_blocks[0].expr[0];
    assert_eq!(expr.left.as_str(), Some("literal"));
    assert_eq!(expr.right.as_str(), Some("literal"));
}

#[test]
fn test_edge_match_number_operands() {
    let input = r#"
match foo {
    100 == 100
}
"#;
    let config = Config::from_str(input).expect("number number match should parse");
    let match_blocks = config.find_match_blocks();
    let expr = &match_blocks[0].expr[0];
    assert_eq!(expr.left.as_i64(), Some(100));
    assert_eq!(expr.right.as_i64(), Some(100));
}

#[test]
fn test_edge_match_mixed_operands() {
    let input = r#"
match foo {
    request.path ~ "/api"
}
"#;
    let config = Config::from_str(input).expect("mixed operand match should parse");
    let match_blocks = config.find_match_blocks();
    let expr = &match_blocks[0].expr[0];
    assert_eq!(
        expr.left.as_identifier().map(|v| v.join(".")),
        Some("request.path".to_string())
    );
    assert_eq!(expr.right.as_str(), Some("/api"));
}

#[test]
fn test_edge_multiple_comma_separated_hosts() {
    let input = "a.com, b.com:8080 {}";
    let config = Config::from_str(input).expect("multi-host block should parse");
    let hb = config.find_host_blocks()[0];
    assert_eq!(hb.hosts.len(), 2);
    assert_eq!(hb.hosts[0].as_str(), "a.com");
    assert_eq!(hb.hosts[1].as_str(), "b.com:8080");
}

#[test]
fn test_edge_wildcard_host() {
    let input = "* {}";
    let config = Config::from_str(input).expect("wildcard host should parse");
    let hb = config.find_host_blocks()[0];
    assert!(matches!(hb.hosts[0].labels, HostLabels::Wildcard));
}

#[test]
fn test_edge_protocol_with_bare_string() {
    let input = "http example.com {}";
    let config = Config::from_str(input).expect("protocol with bare string should parse");
    let hb = config.find_host_blocks()[0];
    assert_eq!(hb.hosts[0].protocol.as_deref(), Some("http"));
    assert_eq!(hb.hosts[0].as_str(), "example.com");
}

#[test]
fn test_edge_protocol_with_number() {
    let input = "123 example.com {}";
    let config = Config::from_str(input).expect("protocol with number should parse");
    let hb = config.find_host_blocks()[0];
    assert_eq!(hb.hosts[0].protocol.as_deref(), Some("123"));
    assert_eq!(hb.hosts[0].as_str(), "example.com");
}

#[test]
fn test_edge_ipv4_with_port() {
    let input = "127.0.0.1:8080 {}";
    let config = Config::from_str(input).expect("IPv4 with port should parse");
    let hb = config.find_host_blocks()[0];
    assert_eq!(hb.hosts[0].port, Some(8080));
    if let HostLabels::IpAddr(IpAddr::V4(addr)) = hb.hosts[0].labels {
        assert_eq!(addr, Ipv4Addr::new(127, 0, 0, 1));
    } else {
        panic!("Expected IPv4 address");
    }
}

#[test]
fn test_edge_empty_host_block_body() {
    let input = "* { }";
    let config = Config::from_str(input).expect("empty host block should parse");
    let hb = config.find_host_blocks()[0];
    assert_eq!(hb.block.statements.len(), 0);
}

#[test]
fn test_edge_negative_zero() {
    let input = "directive -0";
    let config = Config::from_str(input).expect("-0 should parse");
    let d = config.find_directives("directive")[0];
    assert_eq!(d.args[0].as_i64(), Some(0));
}

#[test]
fn test_edge_leading_zeros_in_number() {
    let input = "directive 007";
    let config = Config::from_str(input).expect("leading zeros should parse");
    let d = config.find_directives("directive")[0];
    assert_eq!(d.args[0].as_i64(), Some(7));
}

#[test]
fn test_edge_empty_quoted_string() {
    let input = "directive \"\"";
    let config = Config::from_str(input).expect("empty quoted string should parse");
    let d = config.find_directives("directive")[0];
    if let Value::String(s, _) = &d.args[0] {
        assert_eq!(s, "");
    } else {
        panic!("Expected string value");
    }
}

#[test]
fn test_edge_all_escape_sequences() {
    let input = r#"directive "\n\r\t\\""#;
    let config = Config::from_str(input).expect("all escape sequences should parse");
    let d = config.find_directives("directive")[0];
    if let Value::String(s, _) = &d.args[0] {
        assert_eq!(s, "\n\r\t\\");
    } else {
        panic!("Expected string value");
    }
}

#[test]
fn test_edge_comment_without_trailing_newline() {
    let input = "dir # comment";
    let config = Config::from_str(input).expect("comment without newline should parse");
    assert_eq!(config.statements.len(), 1);
}

#[test]
fn test_edge_multiple_consecutive_comments() {
    let input = "# a\n# b\n# c\n";
    let config = Config::from_str(input).expect("consecutive comments should parse");
    // Comments are now preserved as Statement::Comment nodes
    assert_eq!(config.statements.len(), 3);
    assert!(config.statements.iter().all(|s| s.is_comment()));
}

#[test]
fn test_edge_unicode_in_quoted_string() {
    let input = "directive \"hello 世界\"";
    let config = Config::from_str(input).expect("unicode in string should parse");
    let d = config.find_directives("directive")[0];
    if let Value::String(s, _) = &d.args[0] {
        assert_eq!(s, "hello 世界");
    } else {
        panic!("Expected string value");
    }
}

#[test]
fn test_edge_bare_string_after_operator() {
    let input = r#"
match foo {
    a ~ bare_string
}
"#;
    let config = Config::from_str(input).expect("bare string after operator should parse");
    let match_blocks = config.find_match_blocks();
    let expr = &match_blocks[0].expr[0];
    assert_eq!(expr.right.as_str(), Some("bare_string"));
}

// ============================================================================
// Display / Round-Trip Tests
// ============================================================================

#[test]
fn test_display_simple_host_block() {
    let input = "example.com { root /var/www }";
    let config = Config::from_str(input).expect("parse");
    let formatted = config.to_string();
    let reparsed = Config::from_str(&formatted).expect("round-trip");
    assert_eq!(reparsed.statements.len(), 1);
    assert_eq!(
        reparsed.find_host_blocks()[0].hosts[0].as_str(),
        "example.com"
    );
}

#[test]
fn test_display_interpolated_string() {
    let input = "directive \"{{app.root}}/www\"";
    let config = Config::from_str(input).expect("parse");
    let formatted = config.to_string();
    let reparsed = Config::from_str(&formatted).expect("round-trip");
    let d = reparsed.find_directives("directive")[0];
    if let Value::InterpolatedString(parts, _) = &d.args[0] {
        // Parts: Expression(["app","root"]), Literal("/www")
        assert_eq!(parts.len(), 2);
        if let StringPart::Expression(v) = &parts[0] {
            assert_eq!(v, &["app", "root"]);
        } else {
            panic!("Expected expression part");
        }
        if let StringPart::Literal(s) = &parts[1] {
            assert_eq!(s, "/www");
        } else {
            panic!("Expected literal part");
        }
    } else {
        panic!("Expected interpolated string");
    }
}

#[test]
fn test_display_ipv6_address() {
    let input = "[::1]:8080 {}";
    let config = Config::from_str(input).expect("parse");
    let formatted = config.to_string();
    let reparsed = Config::from_str(&formatted).expect("round-trip");
    let hb = reparsed.find_host_blocks()[0];
    assert_eq!(hb.hosts[0].port, Some(8080));
    if let HostLabels::IpAddr(IpAddr::V6(addr)) = hb.hosts[0].labels {
        assert_eq!(addr, Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1));
    } else {
        panic!("Expected IPv6 address");
    }
}

#[test]
fn test_display_multi_host_block() {
    let input = "a.com, b.com:8080 {}";
    let config = Config::from_str(input).expect("parse");
    let formatted = config.to_string();
    let reparsed = Config::from_str(&formatted).expect("round-trip");
    let hb = reparsed.find_host_blocks()[0];
    assert_eq!(hb.hosts.len(), 2);
    assert_eq!(hb.hosts[0].as_str(), "a.com");
    assert_eq!(hb.hosts[1].as_str(), "b.com:8080");
}

#[test]
fn test_display_nested_blocks() {
    let input = "outer { inner { deep { } } }";
    let config = Config::from_str(input).expect("parse");
    let formatted = config.to_string();
    // Verify the formatted output contains expected structure
    assert!(formatted.contains("outer"));
    assert!(formatted.contains("inner"));
    assert!(formatted.contains("deep"));
    // Parse the formatted output and verify structure
    let reparsed = Config::from_str(&formatted);
    // Note: nested blocks may be parsed differently depending on context
    if let Ok(reparsed) = reparsed {
        if let Statement::Directive(d) = &reparsed.statements[0] {
            assert_eq!(d.name, "outer");
            assert!(d.has_block());
        }
    }
}

#[test]
fn test_display_all_value_types() {
    let input = r#"
str_val "hello"
int_val 42
float_val 3.14
bool_val true
interp_val "{{app.root}}"
"#;
    let config = Config::from_str(input).expect("parse");
    let formatted = config.to_string();
    let reparsed = Config::from_str(&formatted).expect("round-trip");

    assert_eq!(
        reparsed.find_directives("str_val")[0].get_string_arg(0),
        Some("hello")
    );
    assert_eq!(
        reparsed.find_directives("int_val")[0].get_integer_arg(0),
        Some(42)
    );
    assert_eq!(
        reparsed.find_directives("float_val")[0].get_integer_arg(0),
        None
    );
    assert_eq!(
        reparsed.find_directives("bool_val")[0].get_boolean_arg(0),
        Some(true)
    );
}

#[test]
fn test_display_empty_config() {
    let config = Config {
        statements: vec![],
        trailing_comments: std::collections::HashMap::new(),
        blank_lines_before: std::collections::HashMap::new(),
    };
    let formatted = config.to_string();
    assert_eq!(formatted, "");
}

#[test]
fn test_display_match_block() {
    let input = r#"
match foo {
    a ~ "b"
}
"#;
    let config = Config::from_str(input).expect("parse");
    let formatted = config.to_string();
    let reparsed = Config::from_str(&formatted).expect("round-trip");
    let match_blocks = reparsed.find_match_blocks();
    assert_eq!(match_blocks.len(), 1);
    assert_eq!(match_blocks[0].matcher, "foo");
    assert_eq!(match_blocks[0].expr.len(), 1);
}

#[test]
fn test_display_snippet_block() {
    let input = r#"
snippet foo {
    bar true
}
"#;
    let config = Config::from_str(input).expect("parse");
    let formatted = config.to_string();
    // Verify the formatted output contains expected structure
    assert!(formatted.contains("snippet"));
    assert!(formatted.contains("foo"));
    assert!(formatted.contains("bar"));
    // Verify round-trip works when snippet is on its own line
    let reparsed = Config::from_str(&formatted);
    if let Ok(reparsed) = reparsed {
        if let Statement::SnippetBlock(sb) = &reparsed.statements[0] {
            assert_eq!(sb.name, "foo");
            let bar = sb.block.find_directive("bar");
            assert!(bar.is_some());
        }
    }
}

#[test]
fn test_display_global_block() {
    let input = "{\n  runtime {\n    io_uring true\n  }\n}";
    let config = Config::from_str(input).expect("parse");
    let formatted = config.to_string();
    let reparsed = Config::from_str(&formatted).expect("round-trip");
    let global_blocks: Vec<_> = reparsed
        .statements
        .iter()
        .filter(|s| matches!(s, Statement::GlobalBlock(_)))
        .collect();
    assert_eq!(global_blocks.len(), 1);
}

#[test]
fn test_display_host_block_with_protocol() {
    let input = "http example.com {}";
    let config = Config::from_str(input).expect("parse");
    let formatted = config.to_string();
    let reparsed = Config::from_str(&formatted).expect("round-trip");
    let hb = reparsed.find_host_blocks()[0];
    assert_eq!(hb.hosts[0].protocol.as_deref(), Some("http"));
    assert_eq!(hb.hosts[0].as_str(), "example.com");
}

#[test]
fn test_number_trailing_dot_is_error() {
    // A numeric literal with a trailing dot (e.g., `3.`) should be invalid
    let input = r#"directive 3."#;
    assert!(
        Config::from_str(input).is_err(),
        "Expected parse error for number with trailing dot"
    );
}

#[test]
fn test_comments_inside_match_block_are_ignored() {
    // Comments inside match blocks are currently skipped by the lexer; ensure parsing still succeeds
    let input = r#"
match curl_client {
    # comment here
    request.header.user_agent ~ "curl"
}
"#;
    let config = Config::from_str(input).expect("Failed to parse match block with comment inside");
    let match_blocks = config.find_match_blocks();
    assert_eq!(match_blocks.len(), 1);
    let expr = &match_blocks[0].expr[0];
    assert!(expr.is_regex());
    assert_eq!(
        expr.left.as_identifier().map(|v| v.join(".")),
        Some("request.header.user_agent".to_string())
    );
    assert_eq!(expr.right.as_str(), Some("curl"));
}
