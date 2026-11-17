//! Word-token detection, word-click highlighting, and mixed syntax/word-diff line rendering.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Range;

use leptos::ev;
use leptos::prelude::*;
use tellit_types::{DiffLineKind, SyntaxSpan, WordDiffSegment};

use crate::state::{self, ResolvedTheme};

const MIN_WORD_LEN: usize = 2;
const MAX_WORD_LEN: usize = 100;

/// Single styled slice within a diff code line combining syntax color, word-diff emphasis, and clickable word token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRun<'a> {
    /// Substring of the line content covered by this slice.
    pub text: &'a str,
    /// Optional foreground RGB hex color from `syntect`.
    pub fg_hex: Option<&'a str>,
    /// `true` when this slice belongs to an emphasized word-diff token.
    pub word_emphasized: bool,
    /// Clickable identifier token if this slice belongs to a valid word token.
    pub word_token: Option<&'a str>,
}

/// Byte interval for an identifier/word token inside a code line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TokenInterval<'a> {
    range: Range<usize>,
    word: &'a str,
}

/// Byte interval covered by one [`SyntaxSpan`].
#[derive(Debug, Clone)]
struct SyntaxInterval<'a> {
    range: Range<usize>,
    fg_hex: Option<&'a str>,
}

/// Returns `true` when `token` is an ASCII word token of valid highlightable length (`2..=100`).
#[must_use]
pub fn is_valid_word_token(token: &str) -> bool {
    let len = token.len();
    (MIN_WORD_LEN..=MAX_WORD_LEN).contains(&len)
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Normalizes a clicked word token for case-insensitive matching across diff lines.
#[must_use]
pub fn normalize_word(word: &str) -> String {
    word.to_ascii_lowercase()
}

fn detect_word_intervals(text: &str) -> Vec<TokenInterval<'_>> {
    let bytes = text.as_bytes();
    let mut intervals = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        if bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &text[start..i];
            if is_valid_word_token(word) {
                intervals.push(TokenInterval {
                    range: start..i,
                    word,
                });
            }
        } else {
            i += 1;
        }
    }

    intervals
}

fn build_syntax_intervals<'a>(
    content: &str,
    syntax_spans: &'a [SyntaxSpan],
) -> Vec<SyntaxInterval<'a>> {
    let mut intervals = Vec::new();
    let mut cursor = 0usize;

    for span in syntax_spans {
        let end = cursor.saturating_add(span.text.len()).min(content.len());
        if end > cursor
            && content.is_char_boundary(cursor)
            && content.is_char_boundary(end)
            && let Some(fg) = span.fg_hex.as_deref()
        {
            intervals.push(SyntaxInterval {
                range: cursor..end,
                fg_hex: Some(fg),
            });
        }
        cursor = end;
    }

    intervals
}

fn build_word_diff_intervals(content: &str, segments: &[WordDiffSegment]) -> Vec<Range<usize>> {
    let mut intervals = Vec::new();
    let mut cursor = 0usize;

    for seg in segments {
        let end = cursor.saturating_add(seg.text.len()).min(content.len());
        if seg.emphasized
            && end > cursor
            && content.is_char_boundary(cursor)
            && content.is_char_boundary(end)
        {
            intervals.push(cursor..end);
        }
        cursor = end;
    }

    intervals
}

/// Splits `content` at all syntax, word-diff, and word-token boundaries for unified rendering.
#[must_use]
pub fn build_line_runs<'a>(
    content: &'a str,
    syntax_spans: &'a [SyntaxSpan],
    word_segments: &[WordDiffSegment],
) -> Vec<LineRun<'a>> {
    if content.is_empty() {
        return Vec::new();
    }

    let word_intervals = detect_word_intervals(content);
    let syntax_intervals = build_syntax_intervals(content, syntax_spans);
    let diff_intervals = build_word_diff_intervals(content, word_segments);

    let mut boundaries = Vec::new();
    boundaries.push(0usize);
    boundaries.push(content.len());

    for syn in &syntax_intervals {
        boundaries.push(syn.range.start);
        boundaries.push(syn.range.end);
    }

    for diff in &diff_intervals {
        boundaries.push(diff.start);
        boundaries.push(diff.end);
    }

    for tok in &word_intervals {
        boundaries.push(tok.range.start);
        boundaries.push(tok.range.end);
    }

    boundaries.sort_unstable();
    boundaries.dedup();

    let mut runs = Vec::with_capacity(boundaries.len().saturating_sub(1));
    for win in boundaries.windows(2) {
        let start = win[0];
        let end = win[1];
        if start >= end {
            continue;
        }

        let fg_hex = syntax_intervals
            .iter()
            .find(|s| s.range.start <= start && end <= s.range.end)
            .and_then(|s| s.fg_hex);
        let word_emphasized = diff_intervals
            .iter()
            .any(|d| d.start <= start && end <= d.end);
        let word_token = word_intervals
            .iter()
            .find(|w| w.range.start <= start && end <= w.range.end)
            .map(|w| w.word);

        runs.push(LineRun {
            text: &content[start..end],
            fg_hex,
            word_emphasized,
            word_token,
        });
    }

    runs
}

fn has_active_text_selection() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let Ok(Some(sel)) = window.get_selection() else {
        return false;
    };
    !sel.is_collapsed()
}

/// Renders one diff code line with syntax colors, word-level diff highlights, and clickable word tokens.
#[component]
pub fn CodeLineContent(
    line_kind: DiffLineKind,
    content: String,
    syntax_spans: Vec<SyntaxSpan>,
    word_segments: Vec<WordDiffSegment>,
    #[prop(into)] highlighted_word: Signal<Option<String>>,
    #[prop(into)] resolved_theme: Signal<ResolvedTheme>,
    on_toggle_word: Callback<String>,
) -> impl IntoView {
    if content.is_empty() {
        return view! { <code class="diff-code-text">"\u{00a0}"</code> }.into_any();
    }

    let runs = build_line_runs(&content, &syntax_spans, &word_segments);
    let rendered = runs
        .into_iter()
        .map(|run| {
            let text = run.text.to_owned();
            let fg_hex = run.fg_hex.map(String::from);
            let fg_style_tok = fg_hex.clone();
            let fg_style_plain = fg_hex;
            let token_norm = run.word_token.map(normalize_word);
            let token_click = token_norm.clone();

            let mut base_classes = Vec::new();
            if run.word_emphasized {
                match line_kind {
                    DiffLineKind::Add => base_classes.push("word-diff--added"),
                    DiffLineKind::Delete => base_classes.push("word-diff--removed"),
                    DiffLineKind::Context => {}
                }
            }
            if token_norm.is_some() {
                base_classes.push("word-token");
            }
            let base_class = base_classes.join(" ");

            if let Some(norm) = token_norm {
                let norm_for_class = norm;
                let base_for_class = base_class;
                view! {
                    <span
                        class=move || {
                            if highlighted_word.get().as_deref() == Some(norm_for_class.as_str()) {
                                alloc::format!("{base_for_class} word-token--active")
                            } else {
                                base_for_class.clone()
                            }
                        }
                        style=move || {
                            fg_style_tok.as_deref().map(|fg| {
                                let adjusted = state::theme_adjusted_fg(fg, resolved_theme.get());
                                alloc::format!("color: {adjusted}")
                            })
                        }
                        on:click=move |ev: ev::MouseEvent| {
                            if has_active_text_selection() {
                                return;
                            }
                            ev.stop_propagation();
                            if let Some(ref tok) = token_click {
                                on_toggle_word.run(tok.clone());
                            }
                        }
                    >
                        {text}
                    </span>
                }
                .into_any()
            } else {
                view! {
                    <span
                        class=base_class
                        style=move || {
                            fg_style_plain.as_deref().map(|fg| {
                                let adjusted = state::theme_adjusted_fg(fg, resolved_theme.get());
                                alloc::format!("color: {adjusted}")
                            })
                        }
                    >
                        {text}
                    </span>
                }
                .into_any()
            }
        })
        .collect_view();

    view! {
        <code class="diff-code-text">{rendered}</code>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_line_runs_combining_syntax_word_diff_and_tokens() {
        let content = "let alpha_1 = beta;";
        let spans = alloc::vec![
            SyntaxSpan {
                text: String::from("let "),
                fg_hex: Some(String::from("#ff7b72")),
            },
            SyntaxSpan {
                text: String::from("alpha_1"),
                fg_hex: Some(String::from("#79c0ff")),
            },
            SyntaxSpan {
                text: String::from(" = beta;"),
                fg_hex: None,
            },
        ];
        let diff_segs = alloc::vec![
            WordDiffSegment {
                text: String::from("let alpha_1 = "),
                emphasized: false,
            },
            WordDiffSegment {
                text: String::from("beta"),
                emphasized: true,
            },
            WordDiffSegment {
                text: String::from(";"),
                emphasized: false,
            },
        ];

        let runs = build_line_runs(content, &spans, &diff_segs);
        let reconstructed: String = runs.iter().map(|r| r.text).collect();
        assert_eq!(reconstructed, content);

        let alpha_run = runs.iter().find(|r| r.text == "alpha_1").unwrap();
        assert_eq!(alpha_run.fg_hex, Some("#79c0ff"));
        assert_eq!(alpha_run.word_token, Some("alpha_1"));
        assert!(!alpha_run.word_emphasized);

        let beta_run = runs.iter().find(|r| r.text == "beta").unwrap();
        assert!(beta_run.word_emphasized);
        assert_eq!(beta_run.word_token, Some("beta"));
    }

    #[test]
    fn handles_utf8_and_single_char_tokens_safely() {
        let content = "let a = \"café\";";
        let runs = build_line_runs(content, &[], &[]);
        let rebuilt: String = runs.iter().map(|r| r.text).collect();
        assert_eq!(rebuilt, content);
        assert!(runs.iter().all(|r| r.word_token != Some("a")));
    }
}
