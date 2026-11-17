//! Lightweight pure-Rust syntax tokenizer for Markdown fenced code blocks.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use leptos::prelude::*;

/// Token highlight category applied to a code substring inside Markdown preview blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// Normal unstyled punctuation, whitespace, or identifier.
    Plain,
    /// Language keyword (`fn`, `let`, `const`, `return`, etc.).
    Keyword,
    /// String or character literal.
    StringLit,
    /// Line or block comment.
    Comment,
    /// Numeric literal.
    Number,
    /// Capitalized type identifier.
    TypeName,
}

impl TokenKind {
    const fn css_class(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Keyword => "md-tok--kw",
            Self::StringLit => "md-tok--str",
            Self::Comment => "md-tok--cmt",
            Self::Number => "md-tok--num",
            Self::TypeName => "md-tok--typ",
        }
    }
}

/// Single highlighted token slice belonging to one source line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeToken {
    /// Exact substring from the source line.
    pub text: String,
    /// Highlight category for styling.
    pub kind: TokenKind,
}

/// Result of tokenizing one line of fenced code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenizedCodeLine {
    /// Highlighted tokens across the line in source order.
    pub tokens: Vec<CodeToken>,
    /// Whether a `/* ... */` block comment remains open after this line.
    pub in_block_comment: bool,
}

/// Tokenizes multi-line `code` cleanly into per-line syntax token sequences.
#[must_use]
pub fn tokenize_code_block(code: &str, lang: Option<&str>) -> Vec<Vec<CodeToken>> {
    let lines: Vec<&str> = code.split('\n').collect();
    let mut out = Vec::with_capacity(lines.len());
    let mut in_block_comment = false;

    for line in lines {
        let stripped = line.strip_suffix('\r').unwrap_or(line);
        let tokenized = tokenize_code_line(stripped, lang, in_block_comment);
        in_block_comment = tokenized.in_block_comment;
        out.push(tokenized.tokens);
    }
    out
}

/// Tokenizes one code line and reports whether a `/* ... */` block comment remains open.
#[must_use]
pub fn tokenize_code_line(
    line: &str,
    lang: Option<&str>,
    mut in_block_comment: bool,
) -> TokenizedCodeLine {
    let bytes = line.as_bytes();
    let mut tokens: Vec<CodeToken> = Vec::new();
    let mut i = 0usize;
    let hash_comments = uses_hash_comments(lang);
    let dash_comments = uses_dash_comments(lang);
    let rust_lang = is_rust_language(lang);

    while i < bytes.len() {
        if in_block_comment {
            let start = i;
            if let Some(end_rel) = line[start..].find("*/") {
                let end = start + end_rel + 2;
                push_token(&mut tokens, &line[start..end], TokenKind::Comment);
                in_block_comment = false;
                i = end;
            } else {
                push_token(&mut tokens, &line[start..], TokenKind::Comment);
                break;
            }
            continue;
        }

        if line[i..].starts_with("/*") {
            in_block_comment = true;
            let start = i;
            if let Some(end_rel) = line[start + 2..].find("*/") {
                let end = start + 2 + end_rel + 2;
                push_token(&mut tokens, &line[start..end], TokenKind::Comment);
                in_block_comment = false;
                i = end;
            } else {
                push_token(&mut tokens, &line[start..], TokenKind::Comment);
                break;
            }
            continue;
        }

        if line[i..].starts_with("//")
            || (hash_comments && bytes[i] == b'#')
            || (dash_comments && line[i..].starts_with("--"))
        {
            push_token(&mut tokens, &line[i..], TokenKind::Comment);
            break;
        }

        if matches!(bytes[i], b'"' | b'\'' | b'`')
            && !(bytes[i] == b'\'' && rust_lang && is_rust_lifetime(bytes, i))
        {
            let quote = bytes[i];
            let start = i;
            i += 1;
            while i < bytes.len() {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 2;
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += 1;
            }
            push_token(&mut tokens, &line[start..i], TokenKind::StringLit);
            continue;
        }

        if bytes[i].is_ascii_digit()
            && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_'))
        {
            let start = i;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'.')
            {
                i += 1;
            }
            push_token(&mut tokens, &line[start..i], TokenKind::Number);
            continue;
        }

        if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let ident = &line[start..i];
            let kind = if is_keyword(ident, lang) {
                TokenKind::Keyword
            } else if ident.bytes().next().is_some_and(|b| b.is_ascii_uppercase())
                && ident.bytes().any(|b| b.is_ascii_lowercase())
            {
                TokenKind::TypeName
            } else {
                TokenKind::Plain
            };
            push_token(&mut tokens, ident, kind);
            continue;
        }

        let ch_len = line[i..].chars().next().map_or(1, char::len_utf8);
        push_token(&mut tokens, &line[i..i + ch_len], TokenKind::Plain);
        i += ch_len;
    }

    TokenizedCodeLine {
        tokens,
        in_block_comment,
    }
}

fn push_token(tokens: &mut Vec<CodeToken>, slice: &str, kind: TokenKind) {
    if slice.is_empty() {
        return;
    }
    if let Some(last) = tokens.last_mut()
        && last.kind == kind
    {
        last.text.push_str(slice);
    } else {
        tokens.push(CodeToken {
            text: String::from(slice),
            kind,
        });
    }
}

fn is_rust_language(lang: Option<&str>) -> bool {
    lang.is_some_and(|l| l.eq_ignore_ascii_case("rust") || l.eq_ignore_ascii_case("rs"))
}

fn is_rust_lifetime(bytes: &[u8], quote_idx: usize) -> bool {
    let next = bytes.get(quote_idx + 1).copied();
    let after = bytes.get(quote_idx + 2).copied();
    matches!(next, Some(b) if b.is_ascii_alphabetic() || b == b'_') && after != Some(b'\'')
}

fn uses_hash_comments(lang: Option<&str>) -> bool {
    let Some(l) = lang.map(str::to_ascii_lowercase) else {
        return true;
    };
    matches!(
        l.as_str(),
        "py" | "python"
            | "sh"
            | "bash"
            | "zsh"
            | "fish"
            | "nix"
            | "toml"
            | "yaml"
            | "yml"
            | "rb"
            | "ruby"
            | "perl"
            | "r"
            | "dockerfile"
            | "makefile"
            | "conf"
            | "ini"
    )
}

fn uses_dash_comments(lang: Option<&str>) -> bool {
    let Some(l) = lang.map(str::to_ascii_lowercase) else {
        return false;
    };
    matches!(l.as_str(), "sql" | "hs" | "haskell" | "lua")
}

fn is_keyword(ident: &str, lang: Option<&str>) -> bool {
    let common = matches!(
        ident,
        "fn" | "let"
            | "mut"
            | "pub"
            | "const"
            | "static"
            | "struct"
            | "enum"
            | "impl"
            | "trait"
            | "type"
            | "use"
            | "mod"
            | "where"
            | "match"
            | "if"
            | "else"
            | "for"
            | "while"
            | "loop"
            | "in"
            | "break"
            | "continue"
            | "return"
            | "async"
            | "await"
            | "move"
            | "self"
            | "Self"
            | "super"
            | "crate"
            | "true"
            | "false"
            | "null"
            | "None"
            | "Some"
            | "Ok"
            | "Err"
            | "function"
            | "export"
            | "import"
            | "from"
            | "as"
            | "default"
            | "interface"
            | "extends"
            | "class"
            | "new"
            | "try"
            | "catch"
            | "throw"
            | "def"
            | "elif"
            | "with"
            | "yield"
            | "pass"
            | "lambda"
            | "func"
            | "package"
            | "defer"
            | "select"
            | "chan"
            | "go"
            | "var"
            | "inherit"
            | "rec"
    );
    if common {
        return true;
    }
    let Some(l) = lang else {
        return false;
    };
    if l.eq_ignore_ascii_case("sql") {
        matches!(
            ident.to_ascii_uppercase().as_str(),
            "SELECT"
                | "FROM"
                | "WHERE"
                | "JOIN"
                | "LEFT"
                | "RIGHT"
                | "INNER"
                | "ON"
                | "GROUP"
                | "BY"
                | "ORDER"
                | "INSERT"
                | "INTO"
                | "VALUES"
                | "UPDATE"
                | "SET"
                | "DELETE"
                | "CREATE"
                | "TABLE"
                | "AND"
                | "OR"
                | "NOT"
                | "AS"
        )
    } else {
        false
    }
}

/// Renders one already-tokenized line of code into Leptos `<span>` nodes.
#[must_use]
pub fn render_tokens_view(tokens: Vec<CodeToken>) -> AnyView {
    if tokens.is_empty() {
        return view! { <span>"\u{00a0}"</span> }.into_any();
    }
    tokens
        .into_iter()
        .map(|tok| {
            let cls = tok.kind.css_class();
            if cls.is_empty() {
                view! { <span>{tok.text}</span> }.into_any()
            } else {
                view! { <span class=cls>{tok.text}</span> }.into_any()
            }
        })
        .collect_view()
        .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_rust_strings_comments_keywords_and_preserves_text() {
        let code = "pub fn demo() {\n    let s = \"hi\"; // note\n    let n = 42;\n}";
        let lines = tokenize_code_block(code, Some("rust"));
        assert_eq!(lines.len(), 4);

        let rebuilt: Vec<String> = lines
            .iter()
            .map(|row| row.iter().map(|t| t.text.as_str()).collect())
            .collect();
        assert_eq!(rebuilt.join("\n"), code);

        assert!(
            lines[0]
                .iter()
                .any(|t| t.text == "pub" && t.kind == TokenKind::Keyword)
        );
        assert!(
            lines[1]
                .iter()
                .any(|t| t.text == "\"hi\"" && t.kind == TokenKind::StringLit)
        );
        assert!(
            lines[1]
                .iter()
                .any(|t| t.text == "// note" && t.kind == TokenKind::Comment)
        );
        assert!(
            lines[2]
                .iter()
                .any(|t| t.text == "42" && t.kind == TokenKind::Number)
        );
    }

    #[test]
    fn highlights_single_quoted_strings_outside_rust_and_keeps_rust_lifetimes() {
        for lang in [Some("python"), Some("js"), Some("sql"), None] {
            let py = tokenize_code_line("x = 'hello'", lang, false);
            assert!(
                py.tokens
                    .iter()
                    .any(|t| t.text == "'hello'" && t.kind == TokenKind::StringLit),
                "expected StringLit for lang={lang:?}, got {:?}",
                py.tokens
            );
        }

        let rust_life = tokenize_code_line("const S: &'static str = \"ok\";", Some("rs"), false);
        assert!(
            !rust_life
                .tokens
                .iter()
                .any(|t| t.text.contains("static") && t.kind == TokenKind::StringLit)
        );
    }
}
