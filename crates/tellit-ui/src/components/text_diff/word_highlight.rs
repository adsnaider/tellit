//! Static syntax/word-diff line rendering and CSS Custom Highlight API word-click matching.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::ops::Range;

use leptos::prelude::*;
use tellit_types::{
    DiffLine, DiffLineKind, MAX_HIGHLIGHT_LINE_BYTES, SYNTAX_PALETTE_SIZE, SyntaxSpan,
    WordDiffSegment,
};
use wasm_bindgen::{JsCast, JsValue};

const MIN_WORD_LEN: usize = 2;
const MAX_WORD_LEN: usize = 100;
const HIGHLIGHT_REGISTRY_KEY: &str = "tellit-word-match";
const MAX_HIGHLIGHT_RANGES: usize = 10_000;

/// Single styled slice within a diff code line combining syntax palette class and word-diff emphasis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineRun<'a> {
    /// Substring of the line content covered by this slice.
    pub text: &'a str,
    /// CSS syntax palette index (`0` = default foreground, `1..=SYNTAX_PALETTE_SIZE` = `.syn-<n>`).
    pub syntax_style: u8,
    /// `true` when this slice belongs to an emphasized word-diff token.
    pub word_emphasized: bool,
}

/// Byte interval covered by one [`SyntaxSpan`].
#[derive(Debug, Clone, Copy)]
struct SyntaxInterval {
    start: usize,
    end: usize,
    style: u8,
}

#[inline]
const fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Returns `true` when `token` is an ASCII word token of valid highlightable length (`2..=100`).
#[must_use]
pub fn is_valid_word_token(token: &str) -> bool {
    let len = token.len();
    (MIN_WORD_LEN..=MAX_WORD_LEN).contains(&len) && token.bytes().all(is_word_byte)
}

/// Normalizes a clicked word token for case-insensitive matching across diff lines.
#[must_use]
pub fn normalize_word(word: &str) -> String {
    word.to_ascii_lowercase()
}

/// Converts a DOM UTF-16 code-unit offset into a valid UTF-8 byte offset inside `text`.
#[must_use]
pub fn utf16_offset_to_byte_offset(text: &str, utf16_offset: usize) -> usize {
    let mut seen_utf16 = 0usize;
    for (byte_idx, ch) in text.char_indices() {
        if seen_utf16 >= utf16_offset {
            return byte_idx;
        }
        seen_utf16 += ch.len_utf16();
    }
    text.len()
}

/// Converts a UTF-8 byte offset inside `text` into a DOM UTF-16 code-unit offset.
#[must_use]
pub fn byte_offset_to_utf16_offset(text: &str, byte_offset: usize) -> usize {
    let clamped = byte_offset.min(text.len());
    let mut utf16 = 0usize;
    for (idx, ch) in text.char_indices() {
        if idx >= clamped {
            break;
        }
        utf16 += ch.len_utf16();
    }
    utf16
}

/// Extracts the highlightable ASCII word token covering `byte_offset` inside `text`.
#[must_use]
pub fn extract_word_at_byte_offset(text: &str, byte_offset: usize) -> Option<&str> {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return None;
    }

    let anchor = if byte_offset < bytes.len() && is_word_byte(bytes[byte_offset]) {
        byte_offset
    } else if byte_offset > 0 && byte_offset <= bytes.len() && is_word_byte(bytes[byte_offset - 1])
    {
        byte_offset - 1
    } else {
        return None;
    };

    let mut start = anchor;
    while start > 0 && is_word_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = anchor + 1;
    while end < bytes.len() && is_word_byte(bytes[end]) {
        end += 1;
    }

    let word = &text[start..end];
    is_valid_word_token(word).then_some(word)
}

/// Finds all case-insensitive whole-word byte ranges in `text` matching `normalized_word`.
#[must_use]
pub fn find_word_match_ranges(text: &str, normalized_word: &str) -> Vec<Range<usize>> {
    if !is_valid_word_token(normalized_word) || text.len() < normalized_word.len() {
        return Vec::new();
    }

    let target_len = normalized_word.len();
    let bytes = text.as_bytes();
    let mut matches = Vec::new();
    let mut i = 0usize;

    while i < bytes.len() {
        if is_word_byte(bytes[i]) {
            let start = i;
            while i < bytes.len() && is_word_byte(bytes[i]) {
                i += 1;
            }
            if i - start == target_len && text[start..i].eq_ignore_ascii_case(normalized_word) {
                matches.push(start..i);
            }
        } else {
            i += 1;
        }
    }

    matches
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

fn word_diff_class(line_kind: DiffLineKind) -> Option<&'static str> {
    match line_kind {
        DiffLineKind::Add => Some("word-diff--added"),
        DiffLineKind::Delete => Some("word-diff--removed"),
        DiffLineKind::Context => None,
    }
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

/// Splits `content` at syntax and word-diff boundaries in linear `O(S + D)` time using a
/// 2-pointer sweep without splitting identifiers into individual DOM tokens.
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
    if content.len() > MAX_HIGHLIGHT_LINE_BYTES
        || (syntax_spans.is_empty() && word_segments.is_empty())
    {
        return alloc::vec![LineRun {
            text: content,
            syntax_style: 0,
            word_emphasized: false,
        }];
    }

    let syntax_intervals = build_syntax_intervals(content, syntax_spans);
    let diff_intervals = build_word_diff_intervals(content, word_segments);
    if syntax_intervals.is_empty() && diff_intervals.is_empty() {
        return alloc::vec![LineRun {
            text: content,
            syntax_style: 0,
            word_emphasized: false,
        }];
    }

    let mut runs = Vec::with_capacity(syntax_intervals.len() + diff_intervals.len() + 1);
    let mut pos = 0usize;
    let mut si = 0usize;
    let mut di = 0usize;

    while pos < content.len() {
        while si < syntax_intervals.len() && syntax_intervals[si].end <= pos {
            si += 1;
        }
        while di < diff_intervals.len() && diff_intervals[di].end <= pos {
            di += 1;
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

        // Every interval endpoint is verified to be a valid UTF-8 char boundary in `0..=content.len()`.
        debug_assert!(next_pos > pos);
        runs.push(LineRun {
            text: &content[pos..next_pos],
            syntax_style,
            word_emphasized,
        });
        pos = next_pos;
    }

    runs
}

/// Renders one diff code line as static syntax/word-diff nodes with zero per-token listeners or effects.
pub fn render_code_line(line: &DiffLine) -> AnyView {
    if line.content.is_empty() {
        return view! { <code class="diff-code-text">"\u{00a0}"</code> }.into_any();
    }
    if line.content.len() > MAX_HIGHLIGHT_LINE_BYTES
        || (line.syntax_spans.is_empty() && line.word_diff.is_empty())
    {
        let text = line.content.clone();
        return view! { <code class="diff-code-text">{text}</code> }.into_any();
    }

    let runs = build_line_runs(&line.content, &line.syntax_spans, &line.word_diff);
    if runs.len() == 1 && runs[0].syntax_style == 0 && !runs[0].word_emphasized {
        let text = line.content.clone();
        return view! { <code class="diff-code-text">{text}</code> }.into_any();
    }

    let wd_cls = word_diff_class(line.kind);
    let rendered = runs
        .into_iter()
        .map(|run| {
            let text = run.text.to_owned();
            let syn_cls = syntax_style_class(run.syntax_style);
            let emphasis_cls = run.word_emphasized.then_some(wd_cls).flatten();

            match (syn_cls, emphasis_cls) {
                (None, None) => text.into_any(),
                (Some(cls), None) | (None, Some(cls)) => {
                    view! { <span class=cls>{text}</span> }.into_any()
                }
                (Some(s), Some(w)) => {
                    let combined = alloc::format!("{s} {w}");
                    view! { <span class=combined>{text}</span> }.into_any()
                }
            }
        })
        .collect_view();

    view! {
        <code class="diff-code-text">{rendered}</code>
    }
    .into_any()
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

fn collect_text_nodes(node: &web_sys::Node, out: &mut Vec<(web_sys::Node, String)>) {
    let mut child = node.first_child();
    while let Some(cur) = child {
        if cur.node_type() == web_sys::Node::TEXT_NODE {
            if let Some(val) = cur.node_value()
                && !val.is_empty()
            {
                out.push((cur.clone(), val));
            }
        } else {
            collect_text_nodes(&cur, out);
        }
        child = cur.next_sibling();
    }
}

fn caret_node_and_offset(
    doc: &web_sys::Document,
    client_x: f64,
    client_y: f64,
) -> Option<(web_sys::Node, usize)> {
    // Prefer standard `document.caretPositionFromPoint(x, y)` (Firefox, Chrome 128+, Safari).
    if let Ok(func_val) = js_sys::Reflect::get(doc, &JsValue::from_str("caretPositionFromPoint"))
        && let Ok(func) = func_val.dyn_into::<js_sys::Function>()
        && let Ok(pos) = func.call2(
            doc,
            &JsValue::from_f64(client_x),
            &JsValue::from_f64(client_y),
        )
        && !pos.is_null()
        && !pos.is_undefined()
    {
        let node = js_sys::Reflect::get(&pos, &JsValue::from_str("offsetNode"))
            .ok()
            .and_then(|v| v.dyn_into::<web_sys::Node>().ok())?;
        let offset = js_sys::Reflect::get(&pos, &JsValue::from_str("offset"))
            .ok()
            .and_then(|v| v.as_f64())
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| n as usize)?;
        return Some((node, offset));
    }

    // Fallback to legacy `document.caretRangeFromPoint(x, y)`.
    if let Ok(func_val) = js_sys::Reflect::get(doc, &JsValue::from_str("caretRangeFromPoint"))
        && let Ok(func) = func_val.dyn_into::<js_sys::Function>()
        && let Ok(range_val) = func.call2(
            doc,
            &JsValue::from_f64(client_x),
            &JsValue::from_f64(client_y),
        )
        && let Ok(range) = range_val.dyn_into::<web_sys::Range>()
        && let Ok(node) = range.start_container()
        && let Ok(offset) = range.start_offset()
    {
        return Some((node, offset as usize));
    }

    None
}

/// Resolves the normalized identifier token under a delegated click inside `code_el`.
///
/// Returns `None` when the user has a non-collapsed text selection or clicked outside a valid word.
#[must_use]
pub fn extract_word_at_click_point(
    code_el: &web_sys::Element,
    client_x: i32,
    client_y: i32,
) -> Option<String> {
    if has_active_text_selection() {
        return None;
    }
    let doc = web_sys::window()?.document()?;
    let (caret_node, caret_utf16_offset) =
        caret_node_and_offset(&doc, f64::from(client_x), f64::from(client_y))?;
    if caret_node.node_type() != web_sys::Node::TEXT_NODE {
        return None;
    }

    let mut text_nodes = Vec::new();
    collect_text_nodes(code_el.as_ref(), &mut text_nodes);
    if text_nodes.is_empty() {
        return None;
    }

    let mut line_text = String::new();
    let mut target_byte_offset = None;
    for (node, text) in &text_nodes {
        if node.is_same_node(Some(&caret_node)) {
            let local_byte = utf16_offset_to_byte_offset(text, caret_utf16_offset);
            target_byte_offset = Some(line_text.len() + local_byte);
        }
        line_text.push_str(text);
    }

    let byte_offset = target_byte_offset?;
    extract_word_at_byte_offset(&line_text, byte_offset).map(normalize_word)
}

fn css_highlights_registry() -> Option<js_sys::Object> {
    let window = web_sys::window()?;
    let css = js_sys::Reflect::get(&window, &JsValue::from_str("CSS")).ok()?;
    if css.is_undefined() || css.is_null() {
        return None;
    }
    let highlights = js_sys::Reflect::get(&css, &JsValue::from_str("highlights")).ok()?;
    highlights.dyn_into::<js_sys::Object>().ok()
}

/// Removes the active word-match entry from the browser CSS Custom Highlight registry.
pub fn clear_css_word_highlight() {
    let Some(highlights) = css_highlights_registry() else {
        return;
    };
    if let Ok(del_val) = js_sys::Reflect::get(&highlights, &JsValue::from_str("delete"))
        && let Ok(del_fn) = del_val.dyn_into::<js_sys::Function>()
    {
        let _ = del_fn.call1(&highlights, &JsValue::from_str(HIGHLIGHT_REGISTRY_KEY));
    }
}

/// Highlights every whole-word occurrence of `normalized_word` inside `.diff-main .diff-code-text`
/// via the CSS Custom Highlight API (`::highlight(tellit-word-match)`).
///
/// Silently no-ops on browsers that do not implement `CSS.highlights`.
pub fn apply_css_word_highlight(normalized_word: &str) {
    if !is_valid_word_token(normalized_word) {
        clear_css_word_highlight();
        return;
    }
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(doc) = window.document() else {
        return;
    };
    let Some(highlights) = css_highlights_registry() else {
        return;
    };
    let Ok(ctor_val) = js_sys::Reflect::get(&window, &JsValue::from_str("Highlight")) else {
        return;
    };
    let Ok(ctor) = ctor_val.dyn_into::<js_sys::Function>() else {
        return;
    };
    let Ok(highlight_obj) = js_sys::Reflect::construct(&ctor, &js_sys::Array::new()) else {
        return;
    };
    let Ok(add_val) = js_sys::Reflect::get(&highlight_obj, &JsValue::from_str("add")) else {
        return;
    };
    let Ok(add_fn) = add_val.dyn_into::<js_sys::Function>() else {
        return;
    };
    let Ok(set_val) = js_sys::Reflect::get(&highlights, &JsValue::from_str("set")) else {
        return;
    };
    let Ok(set_fn) = set_val.dyn_into::<js_sys::Function>() else {
        return;
    };

    let Ok(code_nodes) = doc.query_selector_all(".diff-main .diff-code-text") else {
        return;
    };

    let mut total_ranges = 0usize;
    let mut text_nodes: Vec<(web_sys::Node, String)> = Vec::new();
    let mut line_buf = String::new();
    let mut node_spans: Vec<(usize, usize)> = Vec::new();

    for idx in 0..code_nodes.length() {
        if total_ranges >= MAX_HIGHLIGHT_RANGES {
            break;
        }
        let Some(code_node) = code_nodes.item(idx) else {
            continue;
        };

        text_nodes.clear();
        collect_text_nodes(&code_node, &mut text_nodes);
        if text_nodes.is_empty() {
            continue;
        }

        if text_nodes.len() == 1 {
            let (ref node, ref text) = text_nodes[0];
            let matches = find_word_match_ranges(text, normalized_word);
            for m in matches {
                if total_ranges >= MAX_HIGHLIGHT_RANGES {
                    break;
                }
                let start_u16 = byte_offset_to_utf16_offset(text, m.start);
                let end_u16 = byte_offset_to_utf16_offset(text, m.end);
                if let (Ok(range), Ok(s), Ok(e)) = (
                    doc.create_range(),
                    u32::try_from(start_u16),
                    u32::try_from(end_u16),
                ) && range.set_start(node, s).is_ok()
                    && range.set_end(node, e).is_ok()
                {
                    let _ = add_fn.call1(&highlight_obj, &range);
                    total_ranges += 1;
                }
            }
            continue;
        }

        line_buf.clear();
        node_spans.clear();
        for (_, text) in &text_nodes {
            let start = line_buf.len();
            line_buf.push_str(text);
            node_spans.push((start, line_buf.len()));
        }

        let matches = find_word_match_ranges(&line_buf, normalized_word);
        if matches.is_empty() {
            continue;
        }

        let mut node_cursor = 0usize;
        for m in matches {
            if total_ranges >= MAX_HIGHLIGHT_RANGES {
                break;
            }
            while node_cursor < node_spans.len() && node_spans[node_cursor].1 <= m.start {
                node_cursor += 1;
            }
            let Some(&(s_base, _)) = node_spans.get(node_cursor) else {
                break;
            };
            let mut end_cursor = node_cursor;
            while end_cursor < node_spans.len() && node_spans[end_cursor].1 < m.end {
                end_cursor += 1;
            }
            let Some(&(e_base, _)) = node_spans.get(end_cursor) else {
                break;
            };

            let (ref start_node, ref start_text) = text_nodes[node_cursor];
            let (ref end_node, ref end_text) = text_nodes[end_cursor];
            let start_u16 = byte_offset_to_utf16_offset(start_text, m.start - s_base);
            let end_u16 = byte_offset_to_utf16_offset(end_text, m.end - e_base);
            if let (Ok(range), Ok(s), Ok(e)) = (
                doc.create_range(),
                u32::try_from(start_u16),
                u32::try_from(end_u16),
            ) && range.set_start(start_node, s).is_ok()
                && range.set_end(end_node, e).is_ok()
            {
                let _ = add_fn.call1(&highlight_obj, &range);
                total_ranges += 1;
            }
        }
    }

    if total_ranges == 0 {
        clear_css_word_highlight();
    } else {
        let _ = set_fn.call2(
            &highlights,
            &JsValue::from_str(HIGHLIGHT_REGISTRY_KEY),
            &highlight_obj,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_line_runs_combining_syntax_and_word_diff_without_token_splitting() {
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

        // No extra identifier splitting: 5 runs total ("let ", "alpha_1", " = ", "beta", ";").
        assert_eq!(runs.len(), 5);
        assert_eq!(
            runs[0],
            LineRun {
                text: "let ",
                syntax_style: 14,
                word_emphasized: false,
            }
        );
        assert_eq!(
            runs[1],
            LineRun {
                text: "alpha_1",
                syntax_style: 13,
                word_emphasized: false,
            }
        );
        assert_eq!(
            runs[3],
            LineRun {
                text: "beta",
                syntax_style: 0,
                word_emphasized: true,
            }
        );
    }

    #[test]
    fn handles_utf8_invalid_span_boundaries_safely() {
        let content = "let a = \"café\";";
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
    }

    #[test]
    fn long_lines_and_plain_lines_take_single_run_fast_path() {
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

        let plain = "fn helper(foo_bar: usize) -> usize { foo_bar + 1 }";
        let plain_runs = build_line_runs(plain, &[], &[]);
        assert_eq!(plain_runs.len(), 1);
        assert_eq!(plain_runs[0].text, plain);
    }

    #[test]
    fn extracts_words_at_byte_offsets_and_rejects_short_or_punctuation() {
        let line = "let x = alpha_1 + beta(gamma);";
        assert_eq!(extract_word_at_byte_offset(line, 0), Some("let"));
        assert_eq!(extract_word_at_byte_offset(line, 3), Some("let"));
        assert_eq!(extract_word_at_byte_offset(line, 4), None); // single-char "x"
        assert_eq!(extract_word_at_byte_offset(line, 10), Some("alpha_1"));
        assert_eq!(extract_word_at_byte_offset(line, 16), None); // whitespace
        assert_eq!(extract_word_at_byte_offset(line, 23), Some("gamma"));
        assert_eq!(extract_word_at_byte_offset(line, line.len()), None); // trailing ';'
    }

    #[test]
    fn finds_case_insensitive_whole_word_matches_only() {
        let line = "foo Foo_bar FOO (foo) barfoo foo2";
        let ranges = find_word_match_ranges(line, "foo");
        let matched: Vec<&str> = ranges.iter().map(|r| &line[r.clone()]).collect();
        assert_eq!(matched, alloc::vec!["foo", "FOO", "foo"]);
        assert_eq!(ranges, alloc::vec![0..3, 12..15, 17..20]);
    }

    #[test]
    fn converts_utf16_and_utf8_offsets_across_multibyte_and_surrogate_chars() {
        // 'é' = 2 UTF-8 bytes, 1 UTF-16 unit; '🦀' = 4 UTF-8 bytes, 2 UTF-16 units.
        let line = "café 🦀 rust_lang";
        let rust_byte = line.find("rust_lang").unwrap();
        let rust_utf16 = byte_offset_to_utf16_offset(line, rust_byte);
        assert_eq!(rust_utf16, 4 + 1 + 2 + 1); // "café" (4) + " " (1) + "🦀" (2) + " " (1)
        assert_eq!(utf16_offset_to_byte_offset(line, rust_utf16), rust_byte);
        assert_eq!(
            extract_word_at_byte_offset(line, utf16_offset_to_byte_offset(line, rust_utf16 + 2)),
            Some("rust_lang")
        );
    }
}
