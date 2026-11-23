//! Word-token detection, word-click highlighting, and mixed syntax/word-diff line rendering.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Range;

use leptos::ev;
use leptos::prelude::*;
use tellit_types::{
    DiffLineKind, MAX_HIGHLIGHT_LINE_BYTES, SYNTAX_PALETTE_SIZE, SyntaxSpan, WordDiffSegment,
};

const MIN_WORD_LEN: usize = 2;
const MAX_WORD_LEN: usize = 100;

/// Single styled slice within a diff code line combining syntax palette class, word-diff emphasis,
/// and clickable word token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineRun<'a> {
    /// Substring of the line content covered by this slice.
    pub text: &'a str,
    /// CSS syntax palette index (`0` = default foreground, `1..=SYNTAX_PALETTE_SIZE` = `.syn-<n>`).
    pub syntax_style: u8,
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
#[derive(Debug, Clone, Copy)]
struct SyntaxInterval {
    start: usize,
    end: usize,
    style: u8,
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

fn syntax_style_class(style: u8) -> Option<&'static str> {
    match style {
        1 => Some("syn-1"),
        2 => Some("syn-2"),
        3 => Some("syn-3"),
        4 => Some("syn-4"),
        5 => Some("syn-5"),
        6 => Some("syn-6"),
        7 => Some("syn-7"),
        8 => Some("syn-8"),
        9 => Some("syn-9"),
        10 => Some("syn-10"),
        11 => Some("syn-11"),
        12 => Some("syn-12"),
        13 => Some("syn-13"),
        14 => Some("syn-14"),
        15 => Some("syn-15"),
        16 => Some("syn-16"),
        17 => Some("syn-17"),
        18 => Some("syn-18"),
        19 => Some("syn-19"),
        _ => None,
    }
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

fn build_syntax_intervals(content: &str, syntax_spans: &[SyntaxSpan]) -> Vec<SyntaxInterval> {
    let mut intervals = Vec::with_capacity(syntax_spans.len());
    let mut cursor = 0usize;

    for span in syntax_spans {
        let end = cursor.saturating_add(span.len as usize).min(content.len());
        if span.style > 0
            && span.style <= SYNTAX_PALETTE_SIZE
            && end > cursor
            && content.is_char_boundary(cursor)
            && content.is_char_boundary(end)
        {
            intervals.push(SyntaxInterval {
                start: cursor,
                end,
                style: span.style,
            });
        }
        cursor = end;
        if cursor >= content.len() {
            break;
        }
    }

    intervals
}

fn build_word_diff_intervals(content: &str, segments: &[WordDiffSegment]) -> Vec<Range<usize>> {
    let mut intervals = Vec::new();
    let mut cursor = 0usize;

    for seg in segments {
        let end = cursor.saturating_add(seg.len as usize).min(content.len());
        if seg.emphasized
            && end > cursor
            && content.is_char_boundary(cursor)
            && content.is_char_boundary(end)
        {
            intervals.push(cursor..end);
        }
        cursor = end;
        if cursor >= content.len() {
            break;
        }
    }

    intervals
}

/// Splits `content` at all syntax, word-diff, and word-token boundaries in linear `O(S + D + W)`
/// time using a 3-pointer sweep.
///
/// Lines longer than [`MAX_HIGHLIGHT_LINE_BYTES`] take a plain-text fast path without splitting.
#[must_use]
pub fn build_line_runs<'a>(
    content: &'a str,
    syntax_spans: &[SyntaxSpan],
    word_segments: &[WordDiffSegment],
) -> Vec<LineRun<'a>> {
    if content.is_empty() {
        return Vec::new();
    }
    if content.len() > MAX_HIGHLIGHT_LINE_BYTES {
        return alloc::vec![LineRun {
            text: content,
            syntax_style: 0,
            word_emphasized: false,
            word_token: None,
        }];
    }

    let syntax_intervals = build_syntax_intervals(content, syntax_spans);
    let diff_intervals = build_word_diff_intervals(content, word_segments);
    let word_intervals = detect_word_intervals(content);

    let mut runs = Vec::with_capacity(
        syntax_intervals.len() + diff_intervals.len() + word_intervals.len() + 1,
    );
    let mut pos = 0usize;
    let mut si = 0usize;
    let mut di = 0usize;
    let mut wi = 0usize;

    while pos < content.len() {
        while si < syntax_intervals.len() && syntax_intervals[si].end <= pos {
            si += 1;
        }
        while di < diff_intervals.len() && diff_intervals[di].end <= pos {
            di += 1;
        }
        while wi < word_intervals.len() && word_intervals[wi].range.end <= pos {
            wi += 1;
        }

        let mut next_pos = content.len();

        let syntax_style = if let Some(syn) = syntax_intervals.get(si) {
            if syn.start <= pos {
                next_pos = next_pos.min(syn.end);
                syn.style
            } else {
                next_pos = next_pos.min(syn.start);
                0
            }
        } else {
            0
        };

        let word_emphasized = if let Some(diff) = diff_intervals.get(di) {
            if diff.start <= pos {
                next_pos = next_pos.min(diff.end);
                true
            } else {
                next_pos = next_pos.min(diff.start);
                false
            }
        } else {
            false
        };

        let word_token = if let Some(tok) = word_intervals.get(wi) {
            if tok.range.start <= pos {
                next_pos = next_pos.min(tok.range.end);
                Some(tok.word)
            } else {
                next_pos = next_pos.min(tok.range.start);
                None
            }
        } else {
            None
        };

        // Every interval endpoint is verified to be a valid UTF-8 char boundary in `0..=content.len()`.
        debug_assert!(next_pos > pos);
        runs.push(LineRun {
            text: &content[pos..next_pos],
            syntax_style,
            word_emphasized,
            word_token,
        });
        pos = next_pos;
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

/// Renders one diff code line with syntax palette classes, word-level diff highlights, and
/// clickable word tokens.
#[component]
pub fn CodeLineContent(
    line_kind: DiffLineKind,
    content: String,
    syntax_spans: Vec<SyntaxSpan>,
    word_segments: Vec<WordDiffSegment>,
    #[prop(into)] highlighted_word: Signal<Option<String>>,
    on_toggle_word: Callback<String>,
) -> impl IntoView {
    if content.is_empty() {
        return view! { <code class="diff-code-text">"\u{00a0}"</code> }.into_any();
    }
    if content.len() > MAX_HIGHLIGHT_LINE_BYTES {
        return view! { <code class="diff-code-text">{content}</code> }.into_any();
    }

    let runs = build_line_runs(&content, &syntax_spans, &word_segments);
    let rendered = runs
        .into_iter()
        .map(|run| {
            let text = run.text.to_owned();
            let token_norm = run.word_token.map(normalize_word);
            let token_click = token_norm.clone();

            let mut base_classes = Vec::with_capacity(3);
            if let Some(syn_class) = syntax_style_class(run.syntax_style) {
                base_classes.push(syn_class);
            }
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
            } else if base_class.is_empty() {
                view! { <span>{text}</span> }.into_any()
            } else {
                view! { <span class=base_class>{text}</span> }.into_any()
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
            SyntaxSpan { len: 4, style: 14 }, // "let "
            SyntaxSpan { len: 7, style: 13 }, // "alpha_1"
            SyntaxSpan { len: 8, style: 0 },  // " = beta;"
        ];
        let diff_segs = alloc::vec![
            WordDiffSegment {
                len: 14,
                emphasized: false,
            }, // "let alpha_1 = "
            WordDiffSegment {
                len: 4,
                emphasized: true,
            }, // "beta"
            WordDiffSegment {
                len: 1,
                emphasized: false,
            }, // ";"
        ];

        let runs = build_line_runs(content, &spans, &diff_segs);
        let reconstructed: String = runs.iter().map(|r| r.text).collect();
        assert_eq!(reconstructed, content);

        let alpha_run = runs.iter().find(|r| r.text == "alpha_1").unwrap();
        assert_eq!(alpha_run.syntax_style, 13);
        assert_eq!(alpha_run.word_token, Some("alpha_1"));
        assert!(!alpha_run.word_emphasized);

        let beta_run = runs.iter().find(|r| r.text == "beta").unwrap();
        assert_eq!(beta_run.syntax_style, 0);
        assert!(beta_run.word_emphasized);
        assert_eq!(beta_run.word_token, Some("beta"));
    }

    #[test]
    fn handles_utf8_invalid_span_boundaries_and_single_char_tokens_safely() {
        let content = "let a = \"café\";";
        // Intentionally split mid-character inside the 2-byte UTF-8 'é' (bytes 12..14) and
        // overflow past `content.len()` to verify slicing never panics.
        let malformed_spans = alloc::vec![
            SyntaxSpan { len: 13, style: 11 },
            SyntaxSpan { len: 99, style: 8 },
        ];
        let malformed_diff = alloc::vec![WordDiffSegment {
            len: 13,
            emphasized: true,
        }];
        let runs = build_line_runs(content, &malformed_spans, &malformed_diff);
        let rebuilt: String = runs.iter().map(|r| r.text).collect();
        assert_eq!(rebuilt, content);
        assert!(runs.iter().all(|r| r.word_token != Some("a")));
    }

    #[test]
    fn long_lines_take_plain_text_fast_path() {
        let long = "token_name ".repeat(MAX_HIGHLIGHT_LINE_BYTES / 10 + 5);
        assert!(long.len() > MAX_HIGHLIGHT_LINE_BYTES);
        let spans = alloc::vec![SyntaxSpan {
            len: u32::try_from(long.len()).unwrap(),
            style: 11,
        }];
        let runs = build_line_runs(&long, &spans, &[]);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, long.as_str());
        assert_eq!(runs[0].syntax_style, 0);
        assert!(!runs[0].word_emphasized);
        assert_eq!(runs[0].word_token, None);
    }
}
