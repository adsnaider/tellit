//! Pure-Rust Mermaid diagram rendering via `mermaid-svg` with defensive SVG sanitization and theme sync.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use leptos::prelude::*;
use tellit_types::LineRangeSpec;

use crate::components::markdown_diff::is_safe_url;
use crate::components::markdown_diff::syntax::{
    render_tokens_view, tokenize_code_block, tokenize_code_line,
};
use crate::state::ResolvedTheme;

/// Maximum Mermaid source length (32 KiB) auto-rendered on the UI thread.
pub const MAX_MERMAID_SOURCE_BYTES: usize = 32 * 1024;

static SVG_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Diff classification for single-sided Mermaid blocks in Markdown Preview mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MermaidDiffStatus {
    /// Newly added Mermaid block.
    Added,
    /// Deleted Mermaid block.
    Deleted,
}

/// Diff line kind carried into [`MermaidDiffBlock`] for source reconstruction and Source Diff view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MermaidDiffLineKind {
    /// Unchanged context line present in both before and after diagrams.
    Context,
    /// Added or replacement line present only in the after diagram.
    Add,
    /// Deleted line present only in the before diagram.
    Delete,
}

/// One diffed source line inside a Mermaid fenced code block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MermaidDiffLine {
    /// Diff classification of the line.
    pub kind: MermaidDiffLineKind,
    /// Raw line text without trailing newline.
    pub content: String,
    /// Preferred 1-based line number (`new_line.or(old_line)`) for scroll anchoring.
    pub display_line: Option<u32>,
}

/// Reconstructed Mermaid diagram sources extracted from a diffed fenced code block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MermaidDiffSources {
    /// Source for the Before diagram (`Context` + `Delete` lines), when applicable.
    pub old_source: Option<String>,
    /// Source for the After diagram (`Context` + `Add` lines), when applicable.
    pub new_source: Option<String>,
    /// `true` when the block represents a modification with distinct before and after sources.
    pub is_modified: bool,
}

/// Result of attempting to render and sanitize one Mermaid diagram to SVG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MermaidRenderResult {
    /// Sanitized, ID-scoped SVG ready to mount via `inner_html`.
    Svg(String),
    /// Input exceeded [`MAX_MERMAID_SOURCE_BYTES`]; auto-rendering was skipped.
    TooLarge {
        /// Byte length of the Mermaid source.
        bytes: usize,
        /// Configured byte cap.
        limit: usize,
    },
    /// Parse error, unsupported syntax, or unsafe SVG output rejected by the sanitizer.
    Error(String),
}

/// Returns `true` when `lang` (an optional fence info string) selects Mermaid (case-insensitive).
#[must_use]
pub fn is_mermaid_lang(lang: Option<&str>) -> bool {
    lang.and_then(|raw| raw.split_whitespace().next())
        .is_some_and(|tok| tok.eq_ignore_ascii_case("mermaid"))
}

/// Reconstructs Before and After Mermaid sources from diff lines inside a fenced block.
#[must_use]
pub fn reconstruct_mermaid_diff_sources(lines: &[MermaidDiffLine]) -> MermaidDiffSources {
    let mut has_ctx = false;
    let mut has_add = false;
    let mut has_del = false;
    let mut old_lines = Vec::new();
    let mut new_lines = Vec::new();

    for line in lines {
        match line.kind {
            MermaidDiffLineKind::Context => {
                has_ctx = true;
                old_lines.push(line.content.as_str());
                new_lines.push(line.content.as_str());
            }
            MermaidDiffLineKind::Delete => {
                has_del = true;
                old_lines.push(line.content.as_str());
            }
            MermaidDiffLineKind::Add => {
                has_add = true;
                new_lines.push(line.content.as_str());
            }
        }
    }

    if !has_add && !has_del {
        return MermaidDiffSources {
            old_source: None,
            new_source: (!new_lines.is_empty()).then(|| new_lines.join("\n")),
            is_modified: false,
        };
    }
    if !has_ctx && has_add && !has_del {
        return MermaidDiffSources {
            old_source: None,
            new_source: Some(new_lines.join("\n")),
            is_modified: false,
        };
    }
    if !has_ctx && !has_add && has_del {
        return MermaidDiffSources {
            old_source: Some(old_lines.join("\n")),
            new_source: None,
            is_modified: false,
        };
    }

    MermaidDiffSources {
        old_source: (!old_lines.is_empty()).then(|| old_lines.join("\n")),
        new_source: (!new_lines.is_empty()).then(|| new_lines.join("\n")),
        is_modified: true,
    }
}

fn mermaid_theme_for(resolved_theme: ResolvedTheme) -> mermaid_svg::Theme {
    match resolved_theme {
        ResolvedTheme::Dark => mermaid_svg::Theme::dark(),
        ResolvedTheme::Light => mermaid_svg::Theme::default(),
    }
}

/// Renders Mermaid `source` into a sanitized, ID-scoped SVG matching `resolved_theme`.
#[must_use]
pub fn render_mermaid_svg(source: &str, resolved_theme: ResolvedTheme) -> MermaidRenderResult {
    let bytes = source.len();
    if bytes > MAX_MERMAID_SOURCE_BYTES {
        return MermaidRenderResult::TooLarge {
            bytes,
            limit: MAX_MERMAID_SOURCE_BYTES,
        };
    }
    if source.trim().is_empty() {
        return MermaidRenderResult::Error(String::from("Empty Mermaid diagram"));
    }

    let theme = mermaid_theme_for(resolved_theme);
    match mermaid_svg::render_with(source, &theme) {
        Ok(raw_svg) => {
            let seq = SVG_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
            let prefix = alloc::format!("mmd-{seq}-");
            match sanitize_svg(&raw_svg, &prefix) {
                Ok(safe_svg) => MermaidRenderResult::Svg(safe_svg),
                Err(reason) => MermaidRenderResult::Error(reason),
            }
        }
        Err(err) => MermaidRenderResult::Error(err.to_string()),
    }
}

fn is_allowed_svg_tag(tag_lower: &str) -> bool {
    matches!(
        tag_lower,
        "svg"
            | "g"
            | "defs"
            | "symbol"
            | "use"
            | "path"
            | "rect"
            | "circle"
            | "ellipse"
            | "line"
            | "polyline"
            | "polygon"
            | "text"
            | "tspan"
            | "textpath"
            | "marker"
            | "lineargradient"
            | "radialgradient"
            | "stop"
            | "pattern"
            | "clippath"
            | "mask"
            | "filter"
            | "fegaussianblur"
            | "feoffset"
            | "feblend"
            | "fecolormatrix"
            | "femerge"
            | "femergenode"
            | "fedropshadow"
            | "feflood"
            | "fecomposite"
            | "title"
            | "desc"
            | "style"
            | "a"
            | "switch"
    )
}

fn is_valid_svg_tag_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    first.is_ascii_alphabetic() && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

fn is_valid_svg_attr_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == b'_' || first == b':')
        && bytes
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b':' || b == b'-')
}

fn decode_xml_entities(input: &str) -> String {
    if !input.contains('&') {
        return String::from(input);
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        if let Some(semi) = after.find(';') {
            let entity = &after[..semi];
            let decoded = match entity {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                _ if entity.starts_with("#x") || entity.starts_with("#X") => {
                    u32::from_str_radix(&entity[2..], 16)
                        .ok()
                        .and_then(char::from_u32)
                }
                _ if entity.starts_with('#') => {
                    entity[1..].parse::<u32>().ok().and_then(char::from_u32)
                }
                _ => None,
            };
            if let Some(ch) = decoded {
                out.push(ch);
                rest = &after[semi + 1..];
                continue;
            }
        }
        out.push('&');
        rest = after;
    }
    out.push_str(rest);
    out
}

fn sanitize_css_urls(css: &str, id_prefix: &str) -> Option<String> {
    let lower = css.to_ascii_lowercase();
    if lower.contains("javascript:")
        || lower.contains("vbscript:")
        || lower.contains("expression(")
        || lower.contains("behavior:")
        || lower.contains("@import")
        || lower.contains("image-set(")
        || lower.contains('\\')
    {
        return None;
    }
    if !lower.contains("url(") {
        return Some(String::from(css));
    }

    // Rewrite every `url(#id)` reference to `url(#<prefix>id)` and reject external/non-fragment `url(...)`.
    let mut out = String::with_capacity(css.len() + 16);
    let mut cursor = 0usize;
    while let Some(rel) = lower[cursor..].find("url(") {
        let start = cursor + rel;
        out.push_str(&css[cursor..start]);
        let after_open = start + 4;
        let close_rel = css[after_open..].find(')')?;
        let close = after_open + close_rel;
        let inner = css[after_open..close]
            .trim()
            .trim_matches(|c| c == '"' || c == '\'');
        let frag = inner.strip_prefix('#')?;
        if frag.is_empty() || frag.chars().any(char::is_whitespace) {
            return None;
        }
        out.push_str("url(#");
        out.push_str(id_prefix);
        out.push_str(frag);
        out.push(')');
        cursor = close + 1;
    }
    out.push_str(&css[cursor..]);
    Some(out)
}

fn rewrite_url_fragments(raw_value: &str, id_prefix: &str) -> Option<String> {
    let decoded = decode_xml_entities(raw_value);
    let sanitized = sanitize_css_urls(&decoded, id_prefix)?;
    Some(escape_xml_attr(&sanitized))
}

fn escape_xml_attr(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

fn parse_tag_attrs(mut s: &str) -> Result<Vec<(&str, &str)>, String> {
    let mut attrs = Vec::new();
    loop {
        s = s.trim_start();
        if s.is_empty() {
            break;
        }
        let name_end = s
            .find(|c: char| c.is_whitespace() || c == '=')
            .unwrap_or(s.len());
        let name = &s[..name_end];
        if !is_valid_svg_attr_name(name) {
            return Err(alloc::format!("Invalid SVG attribute name `{name}`"));
        }
        s = s[name_end..].trim_start();
        if let Some(after_eq) = s.strip_prefix('=') {
            let after_eq = after_eq.trim_start();
            if after_eq.is_empty() {
                return Err(String::from("Missing SVG attribute value"));
            }
            if let Some(rest) = after_eq.strip_prefix('"') {
                let Some(end) = rest.find('"') else {
                    return Err(String::from("Unterminated double-quoted SVG attribute"));
                };
                let rem = &rest[end + 1..];
                if !rem.is_empty() && !rem.starts_with(char::is_whitespace) {
                    return Err(String::from("Missing whitespace after SVG attribute value"));
                }
                attrs.push((name, &rest[..end]));
                s = rem;
            } else if let Some(rest) = after_eq.strip_prefix('\'') {
                let Some(end) = rest.find('\'') else {
                    return Err(String::from("Unterminated single-quoted SVG attribute"));
                };
                let rem = &rest[end + 1..];
                if !rem.is_empty() && !rem.starts_with(char::is_whitespace) {
                    return Err(String::from("Missing whitespace after SVG attribute value"));
                }
                attrs.push((name, &rest[..end]));
                s = rem;
            } else {
                let end = after_eq.find(char::is_whitespace).unwrap_or(after_eq.len());
                let val = &after_eq[..end];
                if val.is_empty() || val.contains(['"', '\'', '<', '>', '=', '`', '/']) {
                    return Err(String::from("Malformed unquoted SVG attribute value"));
                }
                attrs.push((name, val));
                s = &after_eq[end..];
            }
        } else {
            attrs.push((name, ""));
        }
    }
    Ok(attrs)
}

/// Sanitizes an SVG string emitted by `mermaid-svg` and scopes internal IDs with `id_prefix`.
///
/// Allows only safe SVG presentation elements inside a single root `<svg>...</svg>`, rejects
/// malformed tag/attribute syntax, strips `on*` event handler attributes, restricts `href` /
/// `xlink:href` to internal `#id` fragments (plus safe `http(s)`/`mailto` links on `<a>`),
/// rejects external CSS `url(...)` in attributes and `<style>` blocks, and prefixes internal
/// `id` / `url(#...)` / `aria-labelledby` / `aria-describedby` references so multiple inline
/// diagrams never collide.
///
/// # Errors
///
/// Returns `Err` when `raw` is not a single well-formed `<svg>...</svg>` document or contains a
/// disallowed element, malformed markup, or unsafe stylesheet directive.
pub fn sanitize_svg(raw: &str, id_prefix: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if !trimmed.starts_with("<svg") {
        return Err(String::from("Renderer did not produce an SVG root element"));
    }

    let bytes = trimmed.as_bytes();
    let mut out = String::with_capacity(trimmed.len() + 64);
    let mut i = 0usize;
    let mut tag_stack: Vec<String> = Vec::new();
    let mut root_closed = false;

    while i < bytes.len() {
        if bytes[i] != b'<' {
            if root_closed || tag_stack.is_empty() {
                return Err(String::from("Unexpected text outside root <svg>"));
            }
            let next_lt = trimmed[i..].find('<').map_or(bytes.len(), |rel| i + rel);
            let text = &trimmed[i..next_lt];
            if tag_stack.last().is_some_and(|t| t == "style") {
                let decoded = decode_xml_entities(text);
                if decoded.contains('<') {
                    return Err(String::from("Blocked '<' inside SVG <style>"));
                }
                let Some(safe_css) = sanitize_css_urls(&decoded, id_prefix) else {
                    return Err(String::from("Blocked unsafe CSS inside SVG <style>"));
                };
                out.push_str(&safe_css);
            } else {
                out.push_str(text);
            }
            i = next_lt;
            continue;
        }

        // Strip XML comments (`<!-- ... -->`).
        if trimmed[i..].starts_with("<!--") {
            if root_closed || tag_stack.is_empty() {
                return Err(String::from("Unexpected comment outside root <svg>"));
            }
            let Some(end_rel) = trimmed[i + 4..].find("-->") else {
                return Err(String::from("Unterminated XML comment in SVG"));
            };
            i = i + 4 + end_rel + 3;
            continue;
        }

        // Reject XML processing instructions, DOCTYPE, ENTITY, or CDATA sections.
        if trimmed[i..].starts_with("<!") || trimmed[i..].starts_with("<?") {
            return Err(String::from("Blocked XML directive in SVG"));
        }

        // Locate closing `>` while respecting quoted attribute values and rejecting stray `<`.
        let mut j = i + 1;
        let mut quote: Option<u8> = None;
        while j < bytes.len() {
            let c = bytes[j];
            match quote {
                Some(q) => {
                    if c == q {
                        quote = None;
                    }
                }
                None => {
                    if c == b'"' || c == b'\'' {
                        quote = Some(c);
                    } else if c == b'<' {
                        return Err(String::from("Unexpected '<' inside SVG tag"));
                    } else if c == b'>' {
                        break;
                    }
                }
            }
            j += 1;
        }
        if j >= bytes.len() || quote.is_some() {
            return Err(String::from("Unterminated XML tag in SVG"));
        }

        let raw_inner = &trimmed[i + 1..j];
        if raw_inner.is_empty() {
            return Err(String::from("Empty XML tag in SVG"));
        }
        let is_close = raw_inner.starts_with('/');
        if is_close {
            let tag_name = raw_inner[1..].trim_end();
            if !is_valid_svg_tag_name(tag_name) {
                return Err(alloc::format!("Invalid closing SVG tag </{tag_name}>"));
            }
            let tag_lower = tag_name.to_ascii_lowercase();
            if !is_allowed_svg_tag(&tag_lower) {
                return Err(alloc::format!("Blocked unsafe SVG element <{tag_lower}>"));
            }
            let Some(expected) = tag_stack.pop() else {
                return Err(alloc::format!("Unexpected closing tag </{tag_name}>"));
            };
            if expected != tag_lower {
                return Err(alloc::format!(
                    "Mismatched closing SVG tag </{tag_name}> for <{expected}>"
                ));
            }
            if tag_stack.is_empty() {
                root_closed = true;
            }
            out.push_str("</");
            out.push_str(tag_name);
            out.push('>');
            i = j + 1;
            continue;
        }

        if root_closed {
            return Err(String::from("Multiple root elements in SVG"));
        }

        let is_self_closing = raw_inner.ends_with('/');
        let body = if is_self_closing {
            raw_inner[..raw_inner.len() - 1].trim_end()
        } else {
            raw_inner.trim_end()
        };

        let name_end = body.find(char::is_whitespace).unwrap_or(body.len());
        let tag_name = &body[..name_end];
        if !is_valid_svg_tag_name(tag_name) {
            return Err(alloc::format!("Invalid SVG tag name `{tag_name}`"));
        }
        let tag_lower = tag_name.to_ascii_lowercase();
        if !is_allowed_svg_tag(&tag_lower) {
            return Err(alloc::format!("Blocked unsafe SVG element <{tag_lower}>"));
        }
        if tag_stack.is_empty() && tag_lower != "svg" {
            return Err(String::from("Root element must be <svg>"));
        }

        let raw_attrs = &body[name_end..];
        let parsed_attrs = parse_tag_attrs(raw_attrs)?;

        if is_self_closing {
            if tag_stack.is_empty() {
                root_closed = true;
            }
        } else {
            tag_stack.push(tag_lower.clone());
        }

        out.push('<');
        out.push_str(tag_name);

        let mut kept_external_link = false;

        for (attr_name, attr_val) in parsed_attrs {
            let attr_lower = attr_name.to_ascii_lowercase();
            // Strip every inline event handler attribute (`onclick`, `onload`, `onerror`, ...).
            if attr_lower.starts_with("on") || attr_lower == "srcdoc" || attr_lower == "formaction"
            {
                continue;
            }

            // Always normalize `target` / `rel` on `<a>` below when keeping an external link.
            if tag_lower == "a" && (attr_lower == "target" || attr_lower == "rel") {
                continue;
            }

            if attr_lower == "href" || attr_lower == "xlink:href" {
                let decoded = decode_xml_entities(attr_val);
                let compact: String = decoded
                    .chars()
                    .filter(|c| !c.is_control() && !c.is_whitespace())
                    .collect();
                if compact.is_empty() {
                    continue;
                }
                let trimmed_href = decoded.trim();
                if trimmed_href.chars().any(char::is_control) {
                    continue;
                }
                let rewritten = if let Some(frag) = trimmed_href.strip_prefix('#') {
                    if frag.is_empty() || frag.chars().any(char::is_whitespace) {
                        continue;
                    }
                    escape_xml_attr(&alloc::format!("#{id_prefix}{frag}"))
                } else if tag_lower == "a" && is_safe_url(&compact) {
                    let href_lower = trimmed_href.to_ascii_lowercase();
                    if href_lower.starts_with("http://") || href_lower.starts_with("https://") {
                        kept_external_link = true;
                    }
                    escape_xml_attr(trimmed_href)
                } else {
                    continue;
                };
                out.push(' ');
                out.push_str(attr_name);
                out.push_str("=\"");
                out.push_str(&rewritten);
                out.push('"');
                continue;
            }

            if attr_lower == "id" && !attr_val.is_empty() {
                let decoded = decode_xml_entities(attr_val);
                let escaped = escape_xml_attr(&alloc::format!("{id_prefix}{decoded}"));
                out.push_str(" id=\"");
                out.push_str(&escaped);
                out.push('"');
                continue;
            }

            if attr_lower == "aria-labelledby" || attr_lower == "aria-describedby" {
                let decoded = decode_xml_entities(attr_val);
                let prefixed: Vec<String> = decoded
                    .split_whitespace()
                    .map(|tok| alloc::format!("{id_prefix}{tok}"))
                    .collect();
                let escaped = escape_xml_attr(&prefixed.join(" "));
                out.push(' ');
                out.push_str(attr_name);
                out.push_str("=\"");
                out.push_str(&escaped);
                out.push('"');
                continue;
            }

            if matches!(
                attr_lower.as_str(),
                "style"
                    | "marker-start"
                    | "marker-mid"
                    | "marker-end"
                    | "fill"
                    | "stroke"
                    | "filter"
                    | "clip-path"
                    | "mask"
            ) {
                if let Some(safe_val) = rewrite_url_fragments(attr_val, id_prefix) {
                    out.push(' ');
                    out.push_str(attr_name);
                    out.push_str("=\"");
                    out.push_str(&safe_val);
                    out.push('"');
                }
                continue;
            }

            let decoded = decode_xml_entities(attr_val);
            let escaped = escape_xml_attr(&decoded);
            out.push(' ');
            out.push_str(attr_name);
            out.push_str("=\"");
            out.push_str(&escaped);
            out.push('"');
        }

        if kept_external_link {
            out.push_str(" target=\"_blank\" rel=\"noopener noreferrer\"");
        }

        if is_self_closing {
            out.push_str("/>");
        } else {
            out.push('>');
        }
        i = j + 1;
    }

    if !root_closed || !tag_stack.is_empty() {
        return Err(String::from("Unclosed SVG root element"));
    }

    Ok(out)
}

fn format_kb(bytes: usize) -> String {
    alloc::format!("{:.1} KB", bytes as f64 / 1024.0)
}

fn render_mermaid_source_view(source: &str) -> AnyView {
    let normalized = source.strip_suffix('\n').unwrap_or(source);
    let lines = tokenize_code_block(normalized, Some("mermaid"));
    view! {
        <pre class="md-pre md-mermaid__source">
            <code class="md-pre-code">
                {lines.into_iter().map(|toks| view! {
                    <div class="md-code-line">{render_tokens_view(toks)}</div>
                }).collect_view()}
            </code>
        </pre>
    }
    .into_any()
}

fn render_mermaid_diff_source_view(lines: &[MermaidDiffLine]) -> AnyView {
    let mut in_block_comment = false;
    let rendered_lines = lines
        .iter()
        .map(|line| {
            let cls = match line.kind {
                MermaidDiffLineKind::Add => "md-diff-code-line md-diff-code-line--add",
                MermaidDiffLineKind::Delete => "md-diff-code-line md-diff-code-line--del",
                MermaidDiffLineKind::Context => "md-diff-code-line",
            };
            let line_attr = line.display_line.map(|n| n.to_string());
            let tokenized = tokenize_code_line(&line.content, Some("mermaid"), in_block_comment);
            in_block_comment = tokenized.in_block_comment;
            view! {
                <div
                    class=cls
                    data-md-line=line_attr.clone()
                    data-md-line-end=line_attr
                >
                    {render_tokens_view(tokenized.tokens)}
                </div>
            }
        })
        .collect_view();
    view! {
        <div class="md-diff-code-box md-mermaid__source-diff">{rendered_lines}</div>
    }
    .into_any()
}

fn render_mermaid_result_panel(result: MermaidRenderResult, source: &str) -> AnyView {
    match result {
        MermaidRenderResult::Svg(svg) => view! {
            <div class="md-mermaid__canvas" inner_html=svg></div>
        }
        .into_any(),
        MermaidRenderResult::TooLarge { bytes, limit } => {
            let notice = alloc::format!(
                "Mermaid preview skipped: diagram is {} (limit {}). Showing source.",
                format_kb(bytes),
                format_kb(limit),
            );
            view! {
                <div class="md-mermaid__fallback">
                    <div class="md-mermaid__notice">{notice}</div>
                    {render_mermaid_source_view(source)}
                </div>
            }
            .into_any()
        }
        MermaidRenderResult::Error(msg) => {
            let notice = alloc::format!("Mermaid preview unavailable: {msg}");
            view! {
                <div class="md-mermaid__fallback">
                    <div class="md-mermaid__notice">{notice}</div>
                    {render_mermaid_source_view(source)}
                </div>
            }
            .into_any()
        }
    }
}

fn line_range_attrs(range: Option<LineRangeSpec>) -> (Option<String>, Option<String>) {
    match range {
        Some(lr) => (Some(lr.start.to_string()), Some(lr.end.to_string())),
        None => (None, None),
    }
}

/// Interactive single Mermaid diagram block with `Diagram | Source` toggle and theme reactivity.
#[component]
pub fn MermaidDiagramBlock(
    source: String,
    #[prop(default = None)] diff_status: Option<MermaidDiffStatus>,
    #[prop(default = None)] line_range: Option<LineRangeSpec>,
) -> impl IntoView {
    let show_source = RwSignal::new(false);
    let theme_ctx = use_context::<Signal<ResolvedTheme>>();
    let (ls, le) = line_range_attrs(line_range);
    let source = StoredValue::new(source);
    let rendered = Memo::new(move |_| {
        let theme = theme_ctx.map_or(ResolvedTheme::Dark, |sig| sig.get());
        source.with_value(|src| render_mermaid_svg(src, theme))
    });

    let container_class = match diff_status {
        Some(MermaidDiffStatus::Added) => "md-mermaid md-diff-block md-diff-block--add",
        Some(MermaidDiffStatus::Deleted) => "md-mermaid md-diff-block md-diff-block--del",
        None => "md-mermaid",
    };
    let status_pill = match diff_status {
        Some(MermaidDiffStatus::Added) => Some(("pill-tag pill-tag--ok", "Added")),
        Some(MermaidDiffStatus::Deleted) => Some(("pill-tag pill-tag--del", "Deleted")),
        None => None,
    };

    view! {
        <div class=container_class data-md-line=ls data-md-line-end=le>
            <div class="md-mermaid__header">
                <div class="md-mermaid__title">
                    <span class="md-mermaid__badge">"Mermaid"</span>
                    {status_pill.map(|(cls, lbl)| view! { <span class=cls>{lbl}</span> })}
                </div>
                <div class="segmented" role="group" aria-label="Mermaid view mode">
                    <button
                        type="button"
                        class=move || if !show_source.get() { "segmented__btn segmented__btn--active" } else { "segmented__btn" }
                        aria-pressed=move || !show_source.get()
                        on:click=move |_| show_source.set(false)
                    >
                        "Diagram"
                    </button>
                    <button
                        type="button"
                        class=move || if show_source.get() { "segmented__btn segmented__btn--active" } else { "segmented__btn" }
                        aria-pressed=move || show_source.get()
                        on:click=move |_| show_source.set(true)
                    >
                        "Source"
                    </button>
                </div>
            </div>
            <div class="md-mermaid__body">
                {move || {
                    if show_source.get() {
                        source.with_value(|src| render_mermaid_source_view(src))
                    } else {
                        let result = rendered.get();
                        source.with_value(|src| render_mermaid_result_panel(result, src))
                    }
                }}
            </div>
        </div>
    }
}

/// Interactive Before/After Mermaid comparison block for modified diagrams in Preview mode.
#[component]
pub fn MermaidDiffBlock(
    old_source: String,
    new_source: String,
    diff_lines: Vec<MermaidDiffLine>,
    #[prop(default = None)] line_range: Option<LineRangeSpec>,
) -> impl IntoView {
    let show_source = RwSignal::new(false);
    let theme_ctx = use_context::<Signal<ResolvedTheme>>();
    let (ls, le) = line_range_attrs(line_range);
    let old_source = StoredValue::new(old_source);
    let new_source = StoredValue::new(new_source);
    let rendered_pair = Memo::new(move |_| {
        let theme = theme_ctx.map_or(ResolvedTheme::Dark, |sig| sig.get());
        let old_res = old_source.with_value(|src| render_mermaid_svg(src, theme));
        let new_res = new_source.with_value(|src| render_mermaid_svg(src, theme));
        (old_res, new_res)
    });

    view! {
        <div class="md-mermaid md-mermaid--diff md-diff-block md-diff-block--code" data-md-line=ls data-md-line-end=le>
            <div class="md-mermaid__header">
                <div class="md-mermaid__title">
                    <span class="md-mermaid__badge">"Mermaid"</span>
                    <span class="pill-tag">"Modified"</span>
                </div>
                <div class="segmented" role="group" aria-label="Mermaid diff view mode">
                    <button
                        type="button"
                        class=move || if !show_source.get() { "segmented__btn segmented__btn--active" } else { "segmented__btn" }
                        aria-pressed=move || !show_source.get()
                        on:click=move |_| show_source.set(false)
                    >
                        "Diagram"
                    </button>
                    <button
                        type="button"
                        class=move || if show_source.get() { "segmented__btn segmented__btn--active" } else { "segmented__btn" }
                        aria-pressed=move || show_source.get()
                        on:click=move |_| show_source.set(true)
                    >
                        "Source Diff"
                    </button>
                </div>
            </div>
            <div class="md-mermaid__body">
                {move || {
                    if show_source.get() {
                        return render_mermaid_diff_source_view(&diff_lines);
                    }
                    let (old_res, new_res) = rendered_pair.get();
                    if let (MermaidRenderResult::Error(err), MermaidRenderResult::Error(_)) =
                        (&old_res, &new_res)
                    {
                        let notice = alloc::format!("Mermaid preview unavailable: {err}");
                        return view! {
                            <div class="md-mermaid__fallback">
                                <div class="md-mermaid__notice">{notice}</div>
                                {render_mermaid_diff_source_view(&diff_lines)}
                            </div>
                        }
                        .into_any();
                    }
                    let old_panel =
                        old_source.with_value(|src| render_mermaid_result_panel(old_res, src));
                    let new_panel =
                        new_source.with_value(|src| render_mermaid_result_panel(new_res, src));
                    view! {
                        <div class="md-mermaid__diff-grid">
                            <div class="md-mermaid__diff-pane md-mermaid__diff-pane--before">
                                <div class="md-mermaid__pane-label">"Before"</div>
                                {old_panel}
                            </div>
                            <div class="md-mermaid__diff-pane md-mermaid__diff-pane--after">
                                <div class="md-mermaid__pane-label">"After"</div>
                                {new_panel}
                            </div>
                        </div>
                    }
                    .into_any()
                }}
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_mermaid_language_tags_case_insensitively() {
        assert!(is_mermaid_lang(Some("mermaid")));
        assert!(is_mermaid_lang(Some("MERMAID")));
        assert!(is_mermaid_lang(Some("  Mermaid title=flow  ")));
        assert!(!is_mermaid_lang(Some("rust")));
        assert!(!is_mermaid_lang(Some("mermaidjs")));
        assert!(!is_mermaid_lang(Some("")));
        assert!(!is_mermaid_lang(None));
    }

    #[test]
    fn reconstructs_before_and_after_sources_from_diff_lines() {
        let mk = |kind: MermaidDiffLineKind, content: &str| MermaidDiffLine {
            kind,
            content: String::from(content),
            display_line: Some(1),
        };

        // Pure addition: single After diagram.
        let added = reconstruct_mermaid_diff_sources(&[
            mk(MermaidDiffLineKind::Add, "flowchart TD"),
            mk(MermaidDiffLineKind::Add, "  A --> B"),
        ]);
        assert!(!added.is_modified);
        assert_eq!(added.old_source, None);
        assert_eq!(added.new_source.as_deref(), Some("flowchart TD\n  A --> B"));

        // Pure deletion: single Before diagram.
        let deleted = reconstruct_mermaid_diff_sources(&[
            mk(MermaidDiffLineKind::Delete, "flowchart TD"),
            mk(MermaidDiffLineKind::Delete, "  A --> B"),
        ]);
        assert!(!deleted.is_modified);
        assert_eq!(
            deleted.old_source.as_deref(),
            Some("flowchart TD\n  A --> B")
        );
        assert_eq!(deleted.new_source, None);

        // Modified diagram: Context + Delete vs Context + Add.
        let modified = reconstruct_mermaid_diff_sources(&[
            mk(MermaidDiffLineKind::Context, "flowchart TD"),
            mk(MermaidDiffLineKind::Delete, "  A --> B"),
            mk(MermaidDiffLineKind::Add, "  A --> C"),
        ]);
        assert!(modified.is_modified);
        assert_eq!(
            modified.old_source.as_deref(),
            Some("flowchart TD\n  A --> B")
        );
        assert_eq!(
            modified.new_source.as_deref(),
            Some("flowchart TD\n  A --> C")
        );
    }

    #[test]
    fn renders_common_diagram_types_in_dark_and_light_themes() {
        let samples = [
            "flowchart TD\n  A[Start] --> B[End]",
            "sequenceDiagram\n  Alice->>Bob: Hello\n  Bob-->>Alice: Hi",
            "stateDiagram-v2\n  [*] --> Idle\n  Idle --> Active",
            "classDiagram\n  Animal <|-- Duck\n  Animal : +int age",
            "erDiagram\n  USER ||--o{ ORDER : places",
        ];

        for src in samples {
            for theme in [ResolvedTheme::Dark, ResolvedTheme::Light] {
                let res = render_mermaid_svg(src, theme);
                let MermaidRenderResult::Svg(svg) = res else {
                    panic!("expected SVG for {theme:?} sample:\n{src}\ngot: {res:?}");
                };
                assert!(svg.starts_with("<svg"));
                assert!(svg.ends_with("</svg>"));
            }
        }
    }

    #[test]
    fn falls_back_gracefully_on_invalid_mermaid_and_oversized_input() {
        let invalid = render_mermaid_svg("not_a_real_diagram ???\n@@@ bad", ResolvedTheme::Dark);
        assert!(matches!(invalid, MermaidRenderResult::Error(_)));

        let empty = render_mermaid_svg("   \n\t  ", ResolvedTheme::Light);
        assert!(matches!(empty, MermaidRenderResult::Error(_)));

        let huge = alloc::format!(
            "flowchart TD\n  A[{}]",
            "x".repeat(MAX_MERMAID_SOURCE_BYTES)
        );
        let too_large = render_mermaid_svg(&huge, ResolvedTheme::Dark);
        assert!(matches!(
            too_large,
            MermaidRenderResult::TooLarge { bytes, limit }
                if bytes > MAX_MERMAID_SOURCE_BYTES && limit == MAX_MERMAID_SOURCE_BYTES
        ));
    }

    #[test]
    fn sanitizes_xss_vectors_and_scopes_internal_svg_ids() {
        // Click callback (`onclick=`), unsafe `javascript:` href, and inline `<script>` / `<a href>`.
        let hostile = r#"flowchart TD
  A["<script>alert(1)</script><a href='javascript:alert(2)'>SafeText</a>"] --> B[Target]
  click A call alert(3)
  click B href "javascript:alert(4)"
"#;
        let MermaidRenderResult::Svg(svg) = render_mermaid_svg(hostile, ResolvedTheme::Dark) else {
            panic!("expected sanitized SVG for hostile flowchart");
        };
        let lower = svg.to_ascii_lowercase();
        assert!(!lower.contains("<script"), "unexpected <script> in {svg}");
        assert!(
            !lower.contains("<foreignobject"),
            "unexpected <foreignObject> in {svg}"
        );
        assert!(!lower.contains("onclick="), "unexpected onclick in {svg}");
        assert!(!lower.contains("onload="), "unexpected onload in {svg}");
        assert!(
            !lower.contains("javascript:"),
            "unexpected javascript: URI in {svg}"
        );
        assert!(svg.contains("SafeText"));

        // Direct sanitizer checks against synthetic hostile SVG payloads.
        assert!(sanitize_svg("<svg><script>alert(1)</script></svg>", "p-").is_err());
        assert!(
            sanitize_svg(
                "<svg><foreignObject><div>x</div></foreignObject></svg>",
                "p-"
            )
            .is_err()
        );
        let cleaned = sanitize_svg(
            r#"<svg onload="evil()"><defs><marker id="arrow"><path d="M0 0"/></marker></defs><path marker-end="url(#arrow)" onclick="bad()"/><a href="&#106;avascript:alert(1)">x</a><a href="https://example.com">ok</a></svg>"#,
            "mmd-7-",
        )
        .unwrap();
        let cleaned_lower = cleaned.to_ascii_lowercase();
        assert!(!cleaned_lower.contains("onload="));
        assert!(!cleaned_lower.contains("onclick="));
        assert!(!cleaned_lower.contains("javascript:"));
        assert!(cleaned.contains("id=\"mmd-7-arrow\""));
        assert!(cleaned.contains("marker-end=\"url(#mmd-7-arrow)\""));
        assert!(cleaned.contains("href=\"https://example.com\""));
        assert!(cleaned.contains("rel=\"noopener noreferrer\""));

        // Slash-delimited HTML5 tag bypasses, disallowed elements, multi-root payloads, and external <style> urls.
        assert!(sanitize_svg("<svg/onload=alert(1)></svg>", "p-").is_err());
        assert!(sanitize_svg("<svg><script/x>alert(1)</script></svg>", "p-").is_err());
        assert!(
            sanitize_svg(
                r#"<svg><image href="https://example.com/x.png"/></svg>"#,
                "p-"
            )
            .is_err()
        );
        assert!(sanitize_svg("<svg></svg><svg></svg>", "p-").is_err());
        assert!(
            sanitize_svg(
                "<svg><style>rect { fill: url(https://example.com/evil.svg); }</style></svg>",
                "p-"
            )
            .is_err()
        );
        assert!(
            sanitize_svg(
                "<svg><style>rect { background: url(data:image/svg+xml,foo); }</style></svg>",
                "p-"
            )
            .is_err()
        );

        // Single-quoted attribute containing `"` must be XML-escaped and never break out into an attribute.
        let escaped_attr = sanitize_svg(
            r#"<svg><rect data-x='a" onload="alert(1)' fill='red" onclick="evil()'/></svg>"#,
            "mmd-9-",
        )
        .unwrap();
        assert!(
            escaped_attr.contains(r#"data-x="a&quot; onload=&quot;alert(1)""#),
            "expected escaped data-x in {escaped_attr}"
        );
        assert!(
            escaped_attr.contains(r#"fill="red&quot; onclick=&quot;evil()""#),
            "expected escaped fill in {escaped_attr}"
        );

        // Safe <style> preserves raw CSS quotes and scopes `url(#...)` fragment references.
        let safe_style = sanitize_svg(
            r#"<svg><style>text { font-family: "Inter", sans-serif; } path { marker-end: url(#arr); }</style></svg>"#,
            "mmd-11-",
        )
        .unwrap();
        assert!(safe_style.contains(r#"font-family: "Inter", sans-serif;"#));
        assert!(safe_style.contains("url(#mmd-11-arr)"));

        // Non-<a> elements cannot hold external hrefs (only internal #fragment references).
        let use_sanitized = sanitize_svg(
            r##"<svg><use href="https://example.com/evil.svg#x"/><use href="#sym"/></svg>"##,
            "mmd-12-",
        )
        .unwrap();
        assert!(!use_sanitized.contains("https://example.com"));
        assert!(use_sanitized.contains(r##"href="#mmd-12-sym""##));
    }
}
