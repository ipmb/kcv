//! Parser for the `.env` dialect that dotenv libraries actually write.
//!
//! Deliberately does not strip inline comments from unquoted values: a '#' is
//! far more likely to be part of a generated password than the start of a
//! comment, and silently truncating a secret is worse than keeping a stray
//! note. Put the value in quotes if it needs a trailing comment.

use crate::envset::validate_key;

/// A parse failure, with the line it occurred on. Carries no file path so the
/// parser stays independent of the filesystem; the caller supplies the path.
#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub reason: String,
}

fn err<T>(line: usize, reason: &str) -> Result<T, ParseError> {
    Err(ParseError {
        line,
        reason: reason.to_string(),
    })
}

/// True when `word` sits at `i` and is followed by a space or tab, so that
/// `export FOO=1` is a prefix but `EXPORTED=1` is a key.
fn word_at(c: &[char], i: usize, word: &str) -> bool {
    let w: Vec<char> = word.chars().collect();
    if i + w.len() >= c.len() || c[i..i + w.len()] != w[..] {
        return false;
    }
    matches!(c[i + w.len()], ' ' | '\t')
}

/// Parses a `.env` document into ordered key/value pairs.
pub fn parse(text: &str) -> Result<Vec<(String, String)>, ParseError> {
    let c: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut out = Vec::new();

    loop {
        // Skip whitespace, counting the lines we cross.
        while i < c.len() && c[i].is_whitespace() {
            if c[i] == '\n' {
                line += 1;
            }
            i += 1;
        }
        if i >= c.len() {
            return Ok(out);
        }

        // A comment runs to the end of its line.
        if c[i] == '#' {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            continue;
        }

        let entry_line = line;

        if word_at(&c, i, "export") {
            i += "export".len();
            while i < c.len() && matches!(c[i], ' ' | '\t') {
                i += 1;
            }
        }

        // Key runs to the '='. Hitting a newline first means a malformed line.
        let key_start = i;
        while i < c.len() && c[i] != '=' && c[i] != '\n' {
            i += 1;
        }
        if i >= c.len() || c[i] == '\n' {
            return err(entry_line, "expected KEY=VALUE");
        }
        let key: String = c[key_start..i].iter().collect();
        let key = key.trim().to_string();
        i += 1; // consume '='

        while i < c.len() && matches!(c[i], ' ' | '\t') {
            i += 1;
        }

        let value = if i < c.len() && c[i] == '"' {
            i += 1;
            let mut v = String::new();
            loop {
                if i >= c.len() {
                    return err(entry_line, "unterminated double-quoted value");
                }
                match c[i] {
                    '"' => {
                        i += 1;
                        break;
                    }
                    '\\' => {
                        i += 1;
                        if i >= c.len() {
                            return err(entry_line, "unterminated double-quoted value");
                        }
                        match c[i] {
                            'n' => v.push('\n'),
                            't' => v.push('\t'),
                            'r' => v.push('\r'),
                            '"' => v.push('"'),
                            '\'' => v.push('\''),
                            '\\' => v.push('\\'),
                            // A backslash before a newline continues the line.
                            '\n' => line += 1,
                            other => {
                                v.push('\\');
                                v.push(other);
                            }
                        }
                        i += 1;
                    }
                    ch => {
                        if ch == '\n' {
                            line += 1;
                        }
                        v.push(ch);
                        i += 1;
                    }
                }
            }
            v
        } else if i < c.len() && c[i] == '\'' {
            i += 1;
            let mut v = String::new();
            loop {
                if i >= c.len() {
                    return err(entry_line, "unterminated single-quoted value");
                }
                if c[i] == '\'' {
                    i += 1;
                    break;
                }
                if c[i] == '\n' {
                    line += 1;
                }
                v.push(c[i]);
                i += 1;
            }
            v
        } else {
            let start = i;
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            let raw: String = c[start..i].iter().collect();
            raw.trim_end().to_string()
        };

        if validate_key(&key).is_err() {
            return err(entry_line, &format!("invalid variable name {key:?}"));
        }
        out.push((key, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(text: &str) -> Vec<(String, String)> {
        parse(text).expect("should parse")
    }

    #[test]
    fn reads_plain_assignments() {
        assert_eq!(
            ok("FOO=bar\nBAZ=qux\n"),
            vec![
                ("FOO".to_string(), "bar".to_string()),
                ("BAZ".to_string(), "qux".to_string())
            ]
        );
    }

    #[test]
    fn skips_blank_lines_and_comments() {
        let text = "# leading comment\n\nFOO=bar\n\n   # indented comment\nBAZ=qux\n";
        assert_eq!(ok(text).len(), 2);
    }

    #[test]
    fn accepts_an_export_prefix() {
        assert_eq!(ok("export FOO=bar\n"), vec![("FOO".into(), "bar".into())]);
    }

    #[test]
    fn does_not_treat_exported_as_a_prefix() {
        // Only `export ` followed by whitespace counts; EXPORTED is a key.
        assert_eq!(ok("EXPORTED=1\n"), vec![("EXPORTED".into(), "1".into())]);
    }

    #[test]
    fn trims_whitespace_around_key_and_value() {
        assert_eq!(ok("  FOO  =  bar  \n"), vec![("FOO".into(), "bar".into())]);
    }

    #[test]
    fn strips_double_quotes_and_processes_escapes() {
        assert_eq!(
            ok(r#"FOO="line1\nline2\ttab\"quote\\slash""#),
            vec![("FOO".into(), "line1\nline2\ttab\"quote\\slash".into())]
        );
    }

    #[test]
    fn single_quotes_are_literal() {
        assert_eq!(
            ok(r#"FOO='no \n escapes here'"#),
            vec![("FOO".into(), r"no \n escapes here".into())]
        );
    }

    #[test]
    fn double_quoted_values_may_span_lines() {
        let text = "PEM=\"-----BEGIN-----\nmiddle\n-----END-----\"\nNEXT=after\n";
        assert_eq!(
            ok(text),
            vec![
                (
                    "PEM".into(),
                    "-----BEGIN-----\nmiddle\n-----END-----".into()
                ),
                ("NEXT".into(), "after".into())
            ]
        );
    }

    #[test]
    fn single_quoted_values_may_span_lines() {
        assert_eq!(
            ok("A='one\ntwo'\nB=x\n"),
            vec![("A".into(), "one\ntwo".into()), ("B".into(), "x".into())]
        );
    }

    #[test]
    fn a_hash_inside_an_unquoted_value_is_kept() {
        // Truncating at '#' would silently corrupt a password.
        assert_eq!(
            ok("PASSWORD=hunter2#notacomment\n"),
            vec![("PASSWORD".into(), "hunter2#notacomment".into())]
        );
    }

    #[test]
    fn a_comment_may_follow_a_quoted_value() {
        assert_eq!(
            ok("FOO=\"bar\"  # trailing note\nBAZ=qux\n"),
            vec![("FOO".into(), "bar".into()), ("BAZ".into(), "qux".into())]
        );
    }

    #[test]
    fn values_may_contain_equals_signs() {
        assert_eq!(
            ok("URL=https://x/?a=b&c=d\n"),
            vec![("URL".into(), "https://x/?a=b&c=d".into())]
        );
    }

    #[test]
    fn empty_values_are_allowed() {
        assert_eq!(ok("EMPTY=\nQ=\"\"\n").len(), 2);
        assert_eq!(ok("EMPTY=\n"), vec![("EMPTY".into(), String::new())]);
    }

    #[test]
    fn a_file_without_a_trailing_newline_still_parses() {
        assert_eq!(ok("FOO=bar"), vec![("FOO".into(), "bar".into())]);
    }

    #[test]
    fn an_empty_document_yields_nothing() {
        assert_eq!(ok(""), vec![]);
        assert_eq!(ok("\n\n# just a comment\n"), vec![]);
    }

    #[test]
    fn a_line_without_equals_is_an_error_naming_its_line() {
        let err = parse("FOO=bar\ngarbage\n").unwrap_err();
        assert_eq!(err.line, 2);
        assert!(err.reason.contains("KEY=VALUE"), "{}", err.reason);
    }

    #[test]
    fn an_unterminated_quote_is_an_error_naming_its_line() {
        let err = parse("A=1\nB=\"never closed\n").unwrap_err();
        assert_eq!(err.line, 2);
        assert!(err.reason.contains("unterminated"), "{}", err.reason);

        let err = parse("B='never closed\n").unwrap_err();
        assert!(err.reason.contains("unterminated"), "{}", err.reason);
    }

    #[test]
    fn an_invalid_key_is_an_error_naming_its_line() {
        let err = parse("FOO=bar\n=novalue\n").unwrap_err();
        assert_eq!(err.line, 2);
    }

    #[test]
    fn a_backslash_newline_continues_a_quoted_value() {
        assert_eq!(
            ok("A=\"one\\\ntwo\"\n"),
            vec![("A".into(), "onetwo".into())]
        );
    }

    #[test]
    fn handles_the_remaining_escapes() {
        assert_eq!(
            ok(r#"A="carriage\rreturn and \'quote'""#),
            vec![("A".into(), "carriage\rreturn and 'quote'".into())]
        );
    }

    #[test]
    fn a_trailing_backslash_inside_quotes_is_unterminated() {
        let err = parse("A=\"oops\\").unwrap_err();
        assert!(err.reason.contains("unterminated"), "{}", err.reason);
    }

    #[test]
    fn an_unknown_escape_is_kept_verbatim() {
        assert_eq!(ok(r#"A="c:\path""#), vec![("A".into(), r"c:\path".into())]);
    }
}
