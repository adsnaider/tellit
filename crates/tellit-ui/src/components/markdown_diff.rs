//! Pure-Rust GFM Markdown diff viewer using `pulldown-cmark` and YAML frontmatter diffing.

pub mod frontmatter;
pub mod scroll_anchor;
pub mod syntax;

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ops::Range;

use leptos::prelude::*;
use leptos::task::spawn_local;
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use tellit_types::{DiffFile, DiffLineKind, FileStatus, LineRangeSpec};

use self::frontmatter::{
    FrontmatterDiffStatus, FrontmatterDiffTable, FrontmatterEntries, FrontmatterSnapshotTable,
    compute_frontmatter_diff, extract_frontmatter,
};
use self::syntax::{render_tokens_view, tokenize_code_block, tokenize_code_line};
use crate::api;
use crate::components::text_diff::expanded::MergedChunk;

/// Active presentation tab on a Markdown file diff card.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MarkdownTab {
    /// Diff-annotated GFM Markdown blocks plus YAML frontmatter diff table.
    #[default]
    Preview,
    /// Full rendered target (or deleted base) document preview.
    Full,
    /// Raw unified/split text diff view with inline review commenting.
    Code,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewBlockKind {
    Add,
    Delete,
    Change,
    Context,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewSourceLine {
    content: String,
    old_line: Option<u32>,
    new_line: Option<u32>,
}

impl PreviewSourceLine {
    fn display_line(&self) -> Option<u32> {
        self.new_line.or(self.old_line)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewBlock {
    chunk_index: usize,
    kind: PreviewBlockKind,
    lines: Vec<PreviewSourceLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewCodeLine {
    kind: PreviewBlockKind,
    content: String,
    old_line: Option<u32>,
    new_line: Option<u32>,
}

impl PreviewCodeLine {
    fn display_line(&self) -> Option<u32> {
        self.new_line.or(self.old_line)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreviewRenderBlock {
    Markdown {
        kind: PreviewBlockKind,
        lines: Vec<PreviewSourceLine>,
    },
    FencedCode {
        lang: Option<String>,
        lines: Vec<PreviewCodeLine>,
        line_range: Option<LineRangeSpec>,
    },
}

#[derive(Debug, Clone)]
struct FenceInfo {
    marker: char,
    length: usize,
    open_line: PreviewSourceLine,
    lang: Option<String>,
}

/// Validates hyperlink and image URLs to prevent `javascript:`, `data:`, and `vbscript:` injection.
#[must_use]
pub fn is_safe_url(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("https://")
        || lower.starts_with("http://")
        || lower.starts_with("mailto:")
        || lower.starts_with('#')
        || lower.starts_with('/')
        || lower.starts_with("./")
        || lower.starts_with("../")
    {
        return true;
    }
    // Relative paths (`docs/guide.md`) have no scheme `:` before any `/`, `?`, or `#`.
    let before_sep = trimmed.split(['/', '?', '#']).next().unwrap_or(trimmed);
    !before_sep.contains(':')
}

#[derive(Debug, Clone, PartialEq)]
struct MdTableRow {
    cells: Vec<MdNode>,
    line_range: Option<LineRangeSpec>,
}

#[derive(Debug, Clone, PartialEq)]
enum MdNode {
    Heading {
        level: u8,
        children: Vec<MdNode>,
        line_range: Option<LineRangeSpec>,
    },
    Paragraph {
        children: Vec<MdNode>,
        line_range: Option<LineRangeSpec>,
    },
    BlockQuote {
        children: Vec<MdNode>,
        line_range: Option<LineRangeSpec>,
    },
    CodeBlock {
        lang: Option<String>,
        code: String,
        line_range: Option<LineRangeSpec>,
    },
    List {
        ordered_start: Option<u64>,
        items: Vec<MdNode>,
        line_range: Option<LineRangeSpec>,
    },
    ListItem {
        checked: Option<bool>,
        children: Vec<MdNode>,
        line_range: Option<LineRangeSpec>,
    },
    Table {
        alignments: Vec<Alignment>,
        head: Vec<MdNode>,
        head_line_range: Option<LineRangeSpec>,
        body: Vec<MdTableRow>,
        line_range: Option<LineRangeSpec>,
    },
    TableCell {
        is_header: bool,
        children: Vec<MdNode>,
    },
    Rule {
        line_range: Option<LineRangeSpec>,
    },
    Text(String),
    InlineCode(String),
    Strong(Vec<MdNode>),
    Emphasis(Vec<MdNode>),
    Strikethrough(Vec<MdNode>),
    Link {
        url: String,
        title: String,
        children: Vec<MdNode>,
    },
    Image {
        url: String,
        alt: String,
        title: String,
    },
    FootnoteDef {
        label: String,
        children: Vec<MdNode>,
        line_range: Option<LineRangeSpec>,
    },
    FootnoteRef(String),
    SoftBreak,
    HardBreak,
}

enum Frame {
    Root(Vec<MdNode>),
    Heading {
        level: u8,
        children: Vec<MdNode>,
        line_range: Option<LineRangeSpec>,
    },
    Paragraph {
        children: Vec<MdNode>,
        line_range: Option<LineRangeSpec>,
    },
    BlockQuote {
        children: Vec<MdNode>,
        line_range: Option<LineRangeSpec>,
    },
    CodeBlock {
        lang: Option<String>,
        code: String,
        line_range: Option<LineRangeSpec>,
    },
    List {
        ordered_start: Option<u64>,
        items: Vec<MdNode>,
        line_range: Option<LineRangeSpec>,
    },
    ListItem {
        checked: Option<bool>,
        children: Vec<MdNode>,
        line_range: Option<LineRangeSpec>,
    },
    Table {
        alignments: Vec<Alignment>,
        head: Vec<MdNode>,
        head_line_range: Option<LineRangeSpec>,
        body: Vec<MdTableRow>,
        line_range: Option<LineRangeSpec>,
    },
    TableHead {
        cells: Vec<MdNode>,
        line_range: Option<LineRangeSpec>,
    },
    TableRow {
        cells: Vec<MdNode>,
        line_range: Option<LineRangeSpec>,
    },
    TableCell {
        is_header: bool,
        children: Vec<MdNode>,
    },
    Strong(Vec<MdNode>),
    Emphasis(Vec<MdNode>),
    Strikethrough(Vec<MdNode>),
    Link {
        url: String,
        title: String,
        children: Vec<MdNode>,
    },
    Image {
        url: String,
        title: String,
        alt: String,
    },
    FootnoteDef {
        label: String,
        children: Vec<MdNode>,
        line_range: Option<LineRangeSpec>,
    },
}

impl Frame {
    fn push_node(&mut self, node: MdNode) {
        match self {
            Self::Root(v)
            | Self::Heading { children: v, .. }
            | Self::Paragraph { children: v, .. }
            | Self::BlockQuote { children: v, .. }
            | Self::ListItem { children: v, .. }
            | Self::TableHead { cells: v, .. }
            | Self::TableRow { cells: v, .. }
            | Self::TableCell { children: v, .. }
            | Self::Strong(v)
            | Self::Emphasis(v)
            | Self::Strikethrough(v)
            | Self::Link { children: v, .. }
            | Self::FootnoteDef { children: v, .. } => v.push(node),
            Self::List { items, .. } => items.push(node),
            Self::Table { body, .. } => {
                if let Some(last_row) = body.last_mut() {
                    last_row.cells.push(node);
                }
            }
            Self::CodeBlock { code, .. } => {
                if let MdNode::Text(t) | MdNode::InlineCode(t) = node {
                    code.push_str(&t);
                }
            }
            Self::Image { alt, .. } => {
                if let MdNode::Text(t) | MdNode::InlineCode(t) = node {
                    alt.push_str(&t);
                }
            }
        }
    }

    fn push_text(&mut self, txt: &str) {
        match self {
            Self::CodeBlock { code, .. } => code.push_str(txt),
            Self::Image { alt, .. } => alt.push_str(txt),
            _ => self.push_node(MdNode::Text(String::from(txt))),
        }
    }
}

#[derive(Clone, Copy)]
enum LineMapper<'a> {
    Offset(u32),
    Table(&'a [Option<u32>]),
}

impl LineMapper<'_> {
    fn map_indices(self, start_idx: usize, end_idx: usize) -> Option<LineRangeSpec> {
        match self {
            Self::Offset(offset) => {
                let start = offset
                    .saturating_add(u32::try_from(start_idx).unwrap_or(u32::MAX))
                    .saturating_add(1);
                let end = offset
                    .saturating_add(u32::try_from(end_idx).unwrap_or(u32::MAX))
                    .saturating_add(1);
                Some(LineRangeSpec {
                    start,
                    end: end.max(start),
                })
            }
            Self::Table(lines) => {
                if lines.is_empty() || start_idx >= lines.len() {
                    return None;
                }
                let clamped_end = end_idx.min(lines.len() - 1);
                range_from_line_numbers(&lines[start_idx..=clamped_end])
            }
        }
    }
}

fn range_from_line_numbers(lines: &[Option<u32>]) -> Option<LineRangeSpec> {
    let mut min_line: Option<u32> = None;
    let mut max_line: Option<u32> = None;
    for &line_opt in lines {
        if let Some(n) = line_opt {
            min_line = Some(min_line.map_or(n, |cur| cur.min(n)));
            max_line = Some(max_line.map_or(n, |cur| cur.max(n)));
        }
    }
    match (min_line, max_line) {
        (Some(start), Some(end)) => Some(LineRangeSpec { start, end }),
        _ => None,
    }
}

fn compute_line_starts(src: &str) -> Vec<usize> {
    let mut starts = alloc::vec![0];
    for (idx, b) in src.bytes().enumerate() {
        if b == b'\n' && idx + 1 < src.len() {
            starts.push(idx + 1);
        }
    }
    starts
}

fn byte_offset_to_line_index(line_starts: &[usize], byte_offset: usize) -> usize {
    match line_starts.binary_search(&byte_offset) {
        Ok(idx) => idx,
        Err(insert) => insert.saturating_sub(1),
    }
}

fn byte_range_to_line_indices(
    src: &str,
    line_starts: &[usize],
    range: Range<usize>,
) -> Option<(usize, usize)> {
    if src.is_empty() || range.start >= src.len() {
        return None;
    }
    let start = range.start.min(src.len() - 1);
    let mut end = range.end.min(src.len());
    let bytes = src.as_bytes();
    while end > start && matches!(bytes[end - 1], b'\n' | b'\r') {
        end -= 1;
    }
    let end_inclusive = if end > start { end - 1 } else { start };
    let start_idx = byte_offset_to_line_index(line_starts, start);
    let end_idx = byte_offset_to_line_index(line_starts, end_inclusive).max(start_idx);
    Some((start_idx, end_idx))
}

fn heading_level_num(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Parses GFM Markdown into an XSS-safe [`MdNode`] tree annotated with source line ranges.
fn parse_markdown_nodes_with_lines(markdown: &str, mapper: LineMapper<'_>) -> Vec<MdNode> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let line_starts = compute_line_starts(markdown);
    let parser = Parser::new_ext(markdown, options).into_offset_iter();
    let mut stack: Vec<Frame> = alloc::vec![Frame::Root(Vec::new())];

    let map_range = |range: Range<usize>| {
        byte_range_to_line_indices(markdown, &line_starts, range)
            .and_then(|(s, e)| mapper.map_indices(s, e))
    };

    for (event, range) in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => stack.push(Frame::Paragraph {
                    children: Vec::new(),
                    line_range: map_range(range),
                }),
                Tag::Heading { level, .. } => stack.push(Frame::Heading {
                    level: heading_level_num(level),
                    children: Vec::new(),
                    line_range: map_range(range),
                }),
                Tag::BlockQuote(_) => stack.push(Frame::BlockQuote {
                    children: Vec::new(),
                    line_range: map_range(range),
                }),
                Tag::CodeBlock(kind) => {
                    let lang = match kind {
                        CodeBlockKind::Fenced(info) => info
                            .split_whitespace()
                            .next()
                            .filter(|s| !s.is_empty())
                            .map(String::from),
                        CodeBlockKind::Indented => None,
                    };
                    stack.push(Frame::CodeBlock {
                        lang,
                        code: String::new(),
                        line_range: map_range(range),
                    });
                }
                Tag::List(ordered_start) => stack.push(Frame::List {
                    ordered_start,
                    items: Vec::new(),
                    line_range: map_range(range),
                }),
                Tag::Item => stack.push(Frame::ListItem {
                    checked: None,
                    children: Vec::new(),
                    line_range: map_range(range),
                }),
                Tag::Table(alignments) => stack.push(Frame::Table {
                    alignments,
                    head: Vec::new(),
                    head_line_range: None,
                    body: Vec::new(),
                    line_range: map_range(range),
                }),
                Tag::TableHead => stack.push(Frame::TableHead {
                    cells: Vec::new(),
                    line_range: map_range(range),
                }),
                Tag::TableRow => stack.push(Frame::TableRow {
                    cells: Vec::new(),
                    line_range: map_range(range),
                }),
                Tag::TableCell => {
                    let is_header = matches!(stack.last(), Some(Frame::TableHead { .. }));
                    stack.push(Frame::TableCell {
                        is_header,
                        children: Vec::new(),
                    });
                }
                Tag::Emphasis => stack.push(Frame::Emphasis(Vec::new())),
                Tag::Strong => stack.push(Frame::Strong(Vec::new())),
                Tag::Strikethrough => stack.push(Frame::Strikethrough(Vec::new())),
                Tag::Link {
                    dest_url, title, ..
                } => stack.push(Frame::Link {
                    url: dest_url.into_string(),
                    title: title.into_string(),
                    children: Vec::new(),
                }),
                Tag::Image {
                    dest_url, title, ..
                } => stack.push(Frame::Image {
                    url: dest_url.into_string(),
                    title: title.into_string(),
                    alt: String::new(),
                }),
                Tag::FootnoteDefinition(label) => stack.push(Frame::FootnoteDef {
                    label: label.into_string(),
                    children: Vec::new(),
                    line_range: map_range(range),
                }),
                _ => {}
            },
            Event::End(end_tag) => {
                if stack.len() <= 1 {
                    continue;
                }
                if !frame_matches_end(stack.last(), end_tag) {
                    continue;
                }
                let Some(popped) = stack.pop() else {
                    continue;
                };
                match popped {
                    Frame::TableHead { cells, line_range } => {
                        if let Some(Frame::Table {
                            head,
                            head_line_range,
                            ..
                        }) = stack.last_mut()
                        {
                            *head = cells;
                            *head_line_range = line_range;
                        }
                    }
                    Frame::TableRow { cells, line_range } => {
                        if let Some(Frame::Table { body, .. }) = stack.last_mut() {
                            body.push(MdTableRow { cells, line_range });
                        }
                    }
                    other => {
                        if let Some(node) = frame_into_node(other)
                            && let Some(parent) = stack.last_mut()
                        {
                            parent.push_node(node);
                        }
                    }
                }
            }
            Event::Text(text) => {
                if let Some(top) = stack.last_mut() {
                    top.push_text(&text);
                }
            }
            Event::Code(code) => {
                if let Some(top) = stack.last_mut() {
                    top.push_node(MdNode::InlineCode(code.into_string()));
                }
            }
            Event::TaskListMarker(checked) => {
                if let Some(Frame::ListItem { checked: slot, .. }) = stack
                    .iter_mut()
                    .rev()
                    .find(|f| matches!(f, Frame::ListItem { .. }))
                {
                    *slot = Some(checked);
                }
            }
            Event::SoftBreak => {
                if let Some(top) = stack.last_mut() {
                    top.push_node(MdNode::SoftBreak);
                }
            }
            Event::HardBreak => {
                if let Some(top) = stack.last_mut() {
                    top.push_node(MdNode::HardBreak);
                }
            }
            Event::Rule => {
                if let Some(top) = stack.last_mut() {
                    top.push_node(MdNode::Rule {
                        line_range: map_range(range),
                    });
                }
            }
            Event::FootnoteReference(label) => {
                if let Some(top) = stack.last_mut() {
                    top.push_node(MdNode::FootnoteRef(label.into_string()));
                }
            }
            // Escape inline/block raw HTML safely as plain text unless it's an HTML comment.
            Event::Html(html) | Event::InlineHtml(html) => {
                let trimmed = html.trim();
                if !(trimmed.starts_with("<!--") && trimmed.ends_with("-->"))
                    && let Some(top) = stack.last_mut()
                {
                    top.push_text(&html);
                }
            }
            Event::InlineMath(math) | Event::DisplayMath(math) => {
                if let Some(top) = stack.last_mut() {
                    top.push_node(MdNode::InlineCode(math.into_string()));
                }
            }
        }
    }

    match stack.into_iter().next() {
        Some(Frame::Root(nodes)) => nodes,
        _ => Vec::new(),
    }
}

/// Parses GFM Markdown into an XSS-safe [`MdNode`] tree without passing raw HTML through.
#[cfg(test)]
fn parse_markdown_nodes(markdown: &str) -> Vec<MdNode> {
    parse_markdown_nodes_with_lines(markdown, LineMapper::Offset(0))
}

fn frame_matches_end(frame: Option<&Frame>, end: TagEnd) -> bool {
    matches!(
        (frame, end),
        (Some(Frame::Paragraph { .. }), TagEnd::Paragraph)
            | (Some(Frame::Heading { .. }), TagEnd::Heading(_))
            | (Some(Frame::BlockQuote { .. }), TagEnd::BlockQuote(_))
            | (Some(Frame::CodeBlock { .. }), TagEnd::CodeBlock)
            | (Some(Frame::List { .. }), TagEnd::List(_))
            | (Some(Frame::ListItem { .. }), TagEnd::Item)
            | (Some(Frame::Table { .. }), TagEnd::Table)
            | (Some(Frame::TableHead { .. }), TagEnd::TableHead)
            | (Some(Frame::TableRow { .. }), TagEnd::TableRow)
            | (Some(Frame::TableCell { .. }), TagEnd::TableCell)
            | (Some(Frame::Emphasis(_)), TagEnd::Emphasis)
            | (Some(Frame::Strong(_)), TagEnd::Strong)
            | (Some(Frame::Strikethrough(_)), TagEnd::Strikethrough)
            | (Some(Frame::Link { .. }), TagEnd::Link)
            | (Some(Frame::Image { .. }), TagEnd::Image)
            | (Some(Frame::FootnoteDef { .. }), TagEnd::FootnoteDefinition)
    )
}

fn frame_into_node(frame: Frame) -> Option<MdNode> {
    match frame {
        Frame::Heading {
            level,
            children,
            line_range,
        } => Some(MdNode::Heading {
            level,
            children,
            line_range,
        }),
        Frame::Paragraph {
            children,
            line_range,
        } => Some(MdNode::Paragraph {
            children,
            line_range,
        }),
        Frame::BlockQuote {
            children,
            line_range,
        } => Some(MdNode::BlockQuote {
            children,
            line_range,
        }),
        Frame::CodeBlock {
            lang,
            code,
            line_range,
        } => Some(MdNode::CodeBlock {
            lang,
            code,
            line_range,
        }),
        Frame::List {
            ordered_start,
            items,
            line_range,
        } => Some(MdNode::List {
            ordered_start,
            items,
            line_range,
        }),
        Frame::ListItem {
            checked,
            children,
            line_range,
        } => Some(MdNode::ListItem {
            checked,
            children,
            line_range,
        }),
        Frame::Table {
            alignments,
            head,
            head_line_range,
            body,
            line_range,
        } => Some(MdNode::Table {
            alignments,
            head,
            head_line_range,
            body,
            line_range,
        }),
        Frame::TableCell {
            is_header,
            children,
        } => Some(MdNode::TableCell {
            is_header,
            children,
        }),
        Frame::Strong(children) => Some(MdNode::Strong(children)),
        Frame::Emphasis(children) => Some(MdNode::Emphasis(children)),
        Frame::Strikethrough(children) => Some(MdNode::Strikethrough(children)),
        Frame::Link {
            url,
            title,
            children,
        } => Some(MdNode::Link {
            url,
            title,
            children,
        }),
        Frame::Image { url, title, alt } => Some(MdNode::Image { url, alt, title }),
        Frame::FootnoteDef {
            label,
            children,
            line_range,
        } => Some(MdNode::FootnoteDef {
            label,
            children,
            line_range,
        }),
        Frame::Root(_) | Frame::TableHead { .. } | Frame::TableRow { .. } => None,
    }
}

fn line_range_attrs(range: Option<LineRangeSpec>) -> (Option<String>, Option<String>) {
    match range {
        Some(lr) => (Some(lr.start.to_string()), Some(lr.end.to_string())),
        None => (None, None),
    }
}

fn render_nodes_to_view(nodes: Vec<MdNode>) -> AnyView {
    nodes
        .into_iter()
        .map(render_single_node)
        .collect_view()
        .into_any()
}

fn alignment_class(align: Alignment) -> &'static str {
    match align {
        Alignment::None => "",
        Alignment::Left => "md-align-left",
        Alignment::Center => "md-align-center",
        Alignment::Right => "md-align-right",
    }
}

fn render_single_node(node: MdNode) -> AnyView {
    match node {
        MdNode::Heading {
            level,
            children,
            line_range,
        } => {
            let inner = render_nodes_to_view(children);
            let (ls, le) = line_range_attrs(line_range);
            match level {
                1 => view! { <h1 class="md-h md-h1" data-md-line=ls data-md-line-end=le>{inner}</h1> }.into_any(),
                2 => view! { <h2 class="md-h md-h2" data-md-line=ls data-md-line-end=le>{inner}</h2> }.into_any(),
                3 => view! { <h3 class="md-h md-h3" data-md-line=ls data-md-line-end=le>{inner}</h3> }.into_any(),
                4 => view! { <h4 class="md-h md-h4" data-md-line=ls data-md-line-end=le>{inner}</h4> }.into_any(),
                5 => view! { <h5 class="md-h md-h5" data-md-line=ls data-md-line-end=le>{inner}</h5> }.into_any(),
                _ => view! { <h6 class="md-h md-h6" data-md-line=ls data-md-line-end=le>{inner}</h6> }.into_any(),
            }
        }
        MdNode::Paragraph {
            children,
            line_range,
        } => {
            let (ls, le) = line_range_attrs(line_range);
            view! {
                <p class="md-p" data-md-line=ls data-md-line-end=le>{render_nodes_to_view(children)}</p>
            }
            .into_any()
        }
        MdNode::BlockQuote {
            children,
            line_range,
        } => {
            let (ls, le) = line_range_attrs(line_range);
            view! {
                <blockquote class="md-blockquote" data-md-line=ls data-md-line-end=le>{render_nodes_to_view(children)}</blockquote>
            }
            .into_any()
        }
        MdNode::CodeBlock {
            lang,
            code,
            line_range,
        } => {
            let normalized = code.strip_suffix('\n').unwrap_or(&code);
            let lines = tokenize_code_block(normalized, lang.as_deref());
            let (ls, le) = line_range_attrs(line_range);
            view! {
                <pre class="md-pre" data-md-line=ls data-md-line-end=le>
                    {lang.map(|l| view! { <div class="md-code-lang">{l}</div> })}
                    <code class="md-pre-code">
                        {lines.into_iter().map(|toks| view! {
                            <div class="md-code-line">{render_tokens_view(toks)}</div>
                        }).collect_view()}
                    </code>
                </pre>
            }
            .into_any()
        }
        MdNode::List {
            ordered_start,
            items,
            line_range,
        } => {
            let rendered_items = items.into_iter().map(render_single_node).collect_view();
            let (ls, le) = line_range_attrs(line_range);
            match ordered_start {
                Some(start) => view! {
                    <ol class="md-list md-ol" start=start.to_string() data-md-line=ls data-md-line-end=le>{rendered_items}</ol>
                }
                .into_any(),
                None => view! {
                    <ul class="md-list md-ul" data-md-line=ls data-md-line-end=le>{rendered_items}</ul>
                }
                .into_any(),
            }
        }
        MdNode::ListItem {
            checked,
            children,
            line_range,
        } => {
            let cls = if checked.is_some() {
                "md-li md-li--task"
            } else {
                "md-li"
            };
            let (ls, le) = line_range_attrs(line_range);
            view! {
                <li class=cls data-md-line=ls data-md-line-end=le>
                    {checked.map(|is_done| view! {
                        <input
                            type="checkbox"
                            class="md-task-box"
                            checked=is_done
                            disabled=true
                        />
                    })}
                    {render_nodes_to_view(children)}
                </li>
            }
            .into_any()
        }
        MdNode::Table {
            alignments,
            head,
            head_line_range,
            body,
            line_range,
        } => {
            let (table_ls, table_le) = line_range_attrs(line_range);
            let (head_ls, head_le) = line_range_attrs(head_line_range);
            let head_cells = head
                .into_iter()
                .enumerate()
                .map(|(idx, cell)| render_table_cell(cell, alignments.get(idx).copied()))
                .collect_view();
            let body_rows = body
                .into_iter()
                .map(|row| {
                    let (row_ls, row_le) = line_range_attrs(row.line_range);
                    let cells = row
                        .cells
                        .into_iter()
                        .enumerate()
                        .map(|(idx, cell)| render_table_cell(cell, alignments.get(idx).copied()))
                        .collect_view();
                    view! { <tr data-md-line=row_ls data-md-line-end=row_le>{cells}</tr> }
                })
                .collect_view();
            view! {
                <div class="md-table-wrap" data-md-line=table_ls data-md-line-end=table_le>
                    <table class="md-table">
                        <thead><tr data-md-line=head_ls data-md-line-end=head_le>{head_cells}</tr></thead>
                        <tbody>{body_rows}</tbody>
                    </table>
                </div>
            }
            .into_any()
        }
        MdNode::TableCell { .. } => ().into_any(),
        MdNode::Rule { line_range } => {
            let (ls, le) = line_range_attrs(line_range);
            view! { <hr class="md-hr" data-md-line=ls data-md-line-end=le /> }.into_any()
        }
        MdNode::Text(text) => view! { <>{text}</> }.into_any(),
        MdNode::InlineCode(code) => view! { <code class="md-inline-code">{code}</code> }.into_any(),
        MdNode::Strong(children) => {
            view! { <strong>{render_nodes_to_view(children)}</strong> }.into_any()
        }
        MdNode::Emphasis(children) => {
            view! { <em>{render_nodes_to_view(children)}</em> }.into_any()
        }
        MdNode::Strikethrough(children) => {
            view! { <del>{render_nodes_to_view(children)}</del> }.into_any()
        }
        MdNode::Link {
            url,
            title,
            children,
        } => {
            let inner = render_nodes_to_view(children);
            if !is_safe_url(&url) {
                return view! { <span>{inner}</span> }.into_any();
            }
            let is_external = url.starts_with("http://") || url.starts_with("https://");
            let title_attr = (!title.is_empty()).then_some(title);
            view! {
                <a
                    href=url
                    title=title_attr
                    class="md-link"
                    target=is_external.then_some("_blank")
                    rel=is_external.then_some("noopener noreferrer")
                >
                    {inner}
                </a>
            }
            .into_any()
        }
        MdNode::Image { url, alt, title } => {
            if !is_safe_url(&url) {
                return ().into_any();
            }
            let title_attr = (!title.is_empty()).then_some(title);
            view! {
                <img
                    src=url
                    alt=alt
                    title=title_attr
                    loading="lazy"
                    class="md-img"
                />
            }
            .into_any()
        }
        MdNode::FootnoteDef {
            label,
            children,
            line_range,
        } => {
            let (ls, le) = line_range_attrs(line_range);
            view! {
                <div class="md-footnote-def" data-md-line=ls data-md-line-end=le>
                    <span class="md-footnote-label">{alloc::format!("[^{label}]:")}</span>
                    <div>{render_nodes_to_view(children)}</div>
                </div>
            }
            .into_any()
        }
        MdNode::FootnoteRef(label) => {
            view! { <sup class="md-footnote-ref">{alloc::format!("[{label}]")}</sup> }.into_any()
        }
        MdNode::SoftBreak => view! { <>" "</> }.into_any(),
        MdNode::HardBreak => view! { <br /> }.into_any(),
    }
}

fn render_table_cell(cell: MdNode, align: Option<Alignment>) -> AnyView {
    let MdNode::TableCell {
        is_header,
        children,
    } = cell
    else {
        return ().into_any();
    };
    let cls = alignment_class(align.unwrap_or(Alignment::None));
    let inner = render_nodes_to_view(children);
    if is_header {
        view! { <th class=cls>{inner}</th> }.into_any()
    } else {
        view! { <td class=cls>{inner}</td> }.into_any()
    }
}

fn render_markdown_to_view_with_offset(markdown: &str, line_offset: u32) -> AnyView {
    let nodes = parse_markdown_nodes_with_lines(markdown, LineMapper::Offset(line_offset));
    render_nodes_to_view(nodes)
}

fn render_markdown_to_view_with_table(markdown: &str, line_numbers: &[Option<u32>]) -> AnyView {
    let nodes = parse_markdown_nodes_with_lines(markdown, LineMapper::Table(line_numbers));
    render_nodes_to_view(nodes)
}

fn append_preview_line(
    blocks: &mut Vec<PreviewBlock>,
    chunk_index: usize,
    kind: PreviewBlockKind,
    line: PreviewSourceLine,
) {
    if let Some(last) = blocks.last_mut()
        && last.chunk_index == chunk_index
        && last.kind == kind
    {
        last.lines.push(line);
        return;
    }
    blocks.push(PreviewBlock {
        chunk_index,
        kind,
        lines: alloc::vec![line],
    });
}

fn flush_deletes(
    blocks: &mut Vec<PreviewBlock>,
    chunk_index: usize,
    pending: &mut Vec<PreviewSourceLine>,
) {
    for line in pending.drain(..) {
        append_preview_line(blocks, chunk_index, PreviewBlockKind::Delete, line);
    }
}

fn build_preview_blocks(chunks: &[MergedChunk]) -> Vec<PreviewBlock> {
    let mut blocks = Vec::new();
    let mut pending_deletes = Vec::new();
    let mut in_change = false;

    for (chunk_index, chunk) in chunks.iter().enumerate() {
        for line in chunk.iter_lines() {
            let src_line = PreviewSourceLine {
                content: line.content.clone(),
                old_line: line.old_line_number,
                new_line: line.new_line_number,
            };
            match line.kind {
                DiffLineKind::Delete => {
                    pending_deletes.push(src_line);
                }
                DiffLineKind::Add => {
                    if !pending_deletes.is_empty() {
                        flush_deletes(&mut blocks, chunk_index, &mut pending_deletes);
                        in_change = true;
                    }
                    let kind = if in_change {
                        PreviewBlockKind::Change
                    } else {
                        PreviewBlockKind::Add
                    };
                    append_preview_line(&mut blocks, chunk_index, kind, src_line);
                }
                DiffLineKind::Context => {
                    flush_deletes(&mut blocks, chunk_index, &mut pending_deletes);
                    in_change = false;
                    append_preview_line(
                        &mut blocks,
                        chunk_index,
                        PreviewBlockKind::Context,
                        src_line,
                    );
                }
            }
        }
        flush_deletes(&mut blocks, chunk_index, &mut pending_deletes);
        in_change = false;
    }

    blocks
}

fn strip_frontmatter_blocks(
    blocks: Vec<PreviewBlock>,
    base_lines: &[String],
    target_lines: &[String],
) -> Vec<PreviewBlock> {
    if base_lines.is_empty() && target_lines.is_empty() {
        return blocks;
    }

    let mut base_idx = 0usize;
    let mut target_idx = 0usize;
    let mut stripping = true;
    let mut result = Vec::new();

    for block in blocks {
        if !stripping {
            result.push(block);
            continue;
        }

        let mut kept = Vec::new();
        for src_line in block.lines {
            if !stripping {
                kept.push(src_line);
                continue;
            }
            let line = src_line
                .content
                .strip_suffix('\r')
                .unwrap_or(&src_line.content);
            let base_matches = base_lines.get(base_idx).is_some_and(|b| b == line);
            let target_matches = target_lines.get(target_idx).is_some_and(|t| t == line);

            let matched = match block.kind {
                PreviewBlockKind::Delete => {
                    if base_matches {
                        base_idx += 1;
                    }
                    base_matches
                }
                PreviewBlockKind::Add | PreviewBlockKind::Change => {
                    if target_matches {
                        target_idx += 1;
                    }
                    target_matches
                }
                PreviewBlockKind::Context => {
                    let ok = (base_lines.is_empty() || base_matches)
                        && (target_lines.is_empty() || target_matches);
                    if ok {
                        if base_matches {
                            base_idx += 1;
                        }
                        if target_matches {
                            target_idx += 1;
                        }
                    }
                    ok
                }
            };

            if !matched {
                stripping = false;
                kept.push(src_line);
                continue;
            }

            if base_idx >= base_lines.len() && target_idx >= target_lines.len() {
                stripping = false;
            }
        }

        if !kept.is_empty() {
            result.push(PreviewBlock {
                chunk_index: block.chunk_index,
                kind: block.kind,
                lines: kept,
            });
        }
    }

    result
}

fn parse_fence_start(open_line: &PreviewSourceLine) -> Option<FenceInfo> {
    let line = open_line.content.as_str();
    let trimmed = line
        .strip_prefix("   ")
        .or_else(|| line.strip_prefix("  "))
        .or_else(|| line.strip_prefix(' '))
        .unwrap_or(line);
    let first = trimmed.chars().next()?;
    if first != '`' && first != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|&c| c == first).count();
    if count < 3 {
        return None;
    }
    let rest = &trimmed[count..];
    if first == '`' && rest.contains('`') {
        return None;
    }
    let lang = rest
        .split_whitespace()
        .next()
        .filter(|s| !s.is_empty())
        .map(String::from);
    Some(FenceInfo {
        marker: first,
        length: count,
        open_line: open_line.clone(),
        lang,
    })
}

fn is_fence_end(line: &str, fence: &FenceInfo) -> bool {
    let trimmed = line.trim();
    if trimmed.len() < fence.length {
        return false;
    }
    trimmed.chars().all(|c| c == fence.marker) && trimmed.chars().count() >= fence.length
}

fn build_render_blocks(blocks: Vec<PreviewBlock>) -> Vec<PreviewRenderBlock> {
    let mut out = Vec::new();
    let mut current_md: Option<(PreviewBlockKind, Vec<PreviewSourceLine>)> = None;
    let mut current_fence: Option<(FenceInfo, Vec<PreviewCodeLine>)> = None;
    let mut active_chunk_index: Option<usize> = None;

    let flush_md =
        |out: &mut Vec<PreviewRenderBlock>,
         current_md: &mut Option<(PreviewBlockKind, Vec<PreviewSourceLine>)>| {
            if let Some((kind, lines)) = current_md.take() {
                out.push(PreviewRenderBlock::Markdown { kind, lines });
            }
        };

    let push_md_line = |out: &mut Vec<PreviewRenderBlock>,
                        current_md: &mut Option<(PreviewBlockKind, Vec<PreviewSourceLine>)>,
                        kind: PreviewBlockKind,
                        line: PreviewSourceLine| {
        if let Some((cur_kind, lines)) = current_md.as_mut()
            && *cur_kind == kind
        {
            lines.push(line);
            return;
        }
        flush_md(out, current_md);
        *current_md = Some((kind, alloc::vec![line]));
    };

    let flush_fence = |out: &mut Vec<PreviewRenderBlock>,
                       current_md: &mut Option<(PreviewBlockKind, Vec<PreviewSourceLine>)>,
                       current_fence: &mut Option<(FenceInfo, Vec<PreviewCodeLine>)>,
                       closing: Option<PreviewSourceLine>| {
        let Some((fence, lines)) = current_fence.take() else {
            return;
        };
        let only_ctx = lines.iter().all(|l| l.kind == PreviewBlockKind::Context);
        if only_ctx && let Some(close_line) = closing {
            push_md_line(out, current_md, PreviewBlockKind::Context, fence.open_line);
            for l in lines {
                push_md_line(
                    out,
                    current_md,
                    PreviewBlockKind::Context,
                    PreviewSourceLine {
                        content: l.content,
                        old_line: l.old_line,
                        new_line: l.new_line,
                    },
                );
            }
            push_md_line(out, current_md, PreviewBlockKind::Context, close_line);
        } else {
            let mut fence_nums = Vec::with_capacity(lines.len() + 2);
            fence_nums.push(fence.open_line.display_line());
            for l in &lines {
                fence_nums.push(l.display_line());
            }
            if let Some(ref c) = closing {
                fence_nums.push(c.display_line());
            }
            let line_range = range_from_line_numbers(&fence_nums);
            flush_md(out, current_md);
            out.push(PreviewRenderBlock::FencedCode {
                lang: fence.lang,
                lines,
                line_range,
            });
        }
    };

    for block in blocks {
        if active_chunk_index.is_some_and(|prev| prev != block.chunk_index) {
            flush_fence(&mut out, &mut current_md, &mut current_fence, None);
            flush_md(&mut out, &mut current_md);
        }
        active_chunk_index = Some(block.chunk_index);

        for line in block.lines {
            if let Some((ref fence, ref mut fence_lines)) = current_fence {
                if is_fence_end(&line.content, fence) {
                    flush_fence(&mut out, &mut current_md, &mut current_fence, Some(line));
                } else {
                    fence_lines.push(PreviewCodeLine {
                        kind: block.kind,
                        content: line.content,
                        old_line: line.old_line,
                        new_line: line.new_line,
                    });
                }
                continue;
            }
            if let Some(fence) = parse_fence_start(&line) {
                flush_md(&mut out, &mut current_md);
                current_fence = Some((fence, Vec::new()));
                continue;
            }
            push_md_line(&mut out, &mut current_md, block.kind, line);
        }
    }

    flush_fence(&mut out, &mut current_md, &mut current_fence, None);
    flush_md(&mut out, &mut current_md);
    out
}

fn is_non_renderable_md_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return true;
    }
    if trimmed.starts_with("<!--") && trimmed.ends_with("-->") {
        return true;
    }
    if trimmed.starts_with('[') && trimmed.contains("]: ") {
        return true;
    }
    false
}

fn diff_block_border_class(kind: PreviewBlockKind) -> &'static str {
    match kind {
        PreviewBlockKind::Add | PreviewBlockKind::Change => "md-diff-block md-diff-block--add",
        PreviewBlockKind::Delete => "md-diff-block md-diff-block--del",
        PreviewBlockKind::Context => "md-diff-block md-diff-block--ctx",
    }
}

fn code_diff_line_class(kind: PreviewBlockKind) -> &'static str {
    match kind {
        PreviewBlockKind::Add | PreviewBlockKind::Change => {
            "md-diff-code-line md-diff-code-line--add"
        }
        PreviewBlockKind::Delete => "md-diff-code-line md-diff-code-line--del",
        PreviewBlockKind::Context => "md-diff-code-line",
    }
}

struct FrontmatterDiffRender {
    table: Option<AnyView>,
    strip_base: Vec<String>,
    strip_target: Vec<String>,
}

fn render_frontmatter_for_diff(
    base_content: Option<&str>,
    target_content: Option<&str>,
    file_status: FileStatus,
    diff_starts_at_top: bool,
) -> FrontmatterDiffRender {
    let base_ext = base_content.map(extract_frontmatter);
    let target_ext = target_content.map(extract_frontmatter);

    let can_diff = match file_status {
        FileStatus::Added => target_content.is_some(),
        FileStatus::Deleted => base_content.is_some(),
        FileStatus::Modified | FileStatus::Renamed => {
            base_content.is_some() && target_content.is_some()
        }
    };

    if can_diff {
        let strip_base = if !diff_starts_at_top || file_status == FileStatus::Added {
            Vec::new()
        } else {
            base_ext
                .as_ref()
                .map(|e| e.raw_lines.clone())
                .unwrap_or_default()
        };
        let strip_target = if !diff_starts_at_top || file_status == FileStatus::Deleted {
            Vec::new()
        } else {
            target_ext
                .as_ref()
                .map(|e| e.raw_lines.clone())
                .unwrap_or_default()
        };

        let old_data: Option<&FrontmatterEntries> = if file_status == FileStatus::Added {
            None
        } else {
            base_ext.as_ref().and_then(|e| e.data.as_ref())
        };
        let new_data: Option<&FrontmatterEntries> = if file_status == FileStatus::Deleted {
            None
        } else {
            target_ext.as_ref().and_then(|e| e.data.as_ref())
        };

        let entries =
            compute_frontmatter_diff(old_data.map(Vec::as_slice), new_data.map(Vec::as_slice));
        let has_changes = entries
            .iter()
            .any(|e| e.status != FrontmatterDiffStatus::Unchanged);
        let fm_line_end = u32::try_from(strip_target.len().max(strip_base.len()))
            .ok()
            .filter(|&n| n > 0);
        let table = has_changes.then(|| {
            view! {
                <FrontmatterDiffTable
                    entries=entries
                    label=String::from("Frontmatter")
                    line_end=fm_line_end
                />
            }
            .into_any()
        });
        return FrontmatterDiffRender {
            table,
            strip_base,
            strip_target,
        };
    }

    if let Some(ext) = target_ext
        && let Some(data) = ext.data
    {
        let fm_line_end = u32::try_from(ext.raw_lines.len()).ok().filter(|&n| n > 0);
        let table = view! {
            <FrontmatterSnapshotTable
                entries=data
                label=String::from("Frontmatter (target snapshot)")
                line_end=fm_line_end
            />
        }
        .into_any();
        return FrontmatterDiffRender {
            table: Some(table),
            strip_base: Vec::new(),
            strip_target: Vec::new(),
        };
    }
    if let Some(ext) = base_ext
        && let Some(data) = ext.data
    {
        let fm_line_end = u32::try_from(ext.raw_lines.len()).ok().filter(|&n| n > 0);
        let table = view! {
            <FrontmatterSnapshotTable
                entries=data
                label=String::from("Frontmatter (base snapshot)")
                line_end=fm_line_end
            />
        }
        .into_any();
        return FrontmatterDiffRender {
            table: Some(table),
            strip_base: Vec::new(),
            strip_target: Vec::new(),
        };
    }

    FrontmatterDiffRender {
        table: None,
        strip_base: Vec::new(),
        strip_target: Vec::new(),
    }
}

/// Interactive Markdown preview panel rendering diff blocks or full document with YAML frontmatter.
#[component]
pub fn MarkdownPreviewBody(
    file: Arc<DiffFile>,
    #[prop(into)] merged_chunks: Signal<Arc<Vec<MergedChunk>>>,
    base_commit: String,
    target_commit: String,
    #[prop(into)] active_tab: Signal<MarkdownTab>,
    #[prop(optional)] full_ready: Option<RwSignal<bool>>,
) -> impl IntoView {
    let base_content = RwSignal::new(Option::<String>::None);
    let target_content = RwSignal::new(Option::<String>::None);
    let loading = RwSignal::new(false);
    let fetch_error = RwSignal::new(Option::<String>::None);

    let need_base = file.status != FileStatus::Added && !base_commit.is_empty();
    let need_target = file.status != FileStatus::Deleted && !target_commit.is_empty();
    let base_path = file.old_path.clone().unwrap_or_else(|| file.path.clone());
    let target_path = file.path.clone();
    let alive = StoredValue::new_local(true);
    on_cleanup(move || {
        let _ = alive.try_set_value(false);
    });

    if need_base || need_target {
        loading.set(true);
        if let Some(sig) = full_ready {
            let _ = sig.try_set(false);
        }
        spawn_local(async move {
            let mut errs = Vec::new();
            if need_base {
                match api::fetch_blob_text(&base_path, &base_commit).await {
                    Ok(txt) => {
                        if !alive.try_get_value().unwrap_or(false) {
                            return;
                        }
                        let _ = base_content.try_set(Some(txt));
                    }
                    Err(err) => errs.push(err.to_string()),
                }
            }
            if !alive.try_get_value().unwrap_or(false) {
                return;
            }
            if need_target {
                match api::fetch_blob_text(&target_path, &target_commit).await {
                    Ok(txt) => {
                        if !alive.try_get_value().unwrap_or(false) {
                            return;
                        }
                        let _ = target_content.try_set(Some(txt));
                    }
                    Err(err) => errs.push(err.to_string()),
                }
            }
            if !alive.try_get_value().unwrap_or(false) {
                return;
            }
            if !errs.is_empty()
                && base_content.try_get_untracked().flatten().is_none()
                && target_content.try_get_untracked().flatten().is_none()
            {
                let _ = fetch_error.try_set(Some(errs.join("; ")));
            }
            let _ = loading.try_set(false);
            if let Some(sig) = full_ready {
                let _ = sig.try_set(true);
            }
        });
    } else if let Some(sig) = full_ready {
        let _ = sig.try_set(true);
    }

    view! {
        <div class="md-viewer-body">
            {move || {
                let tab = active_tab.get();
                let base = base_content.get();
                let target = target_content.get();

                if tab == MarkdownTab::Full {
                    if loading.get() {
                        return view! { <div class="md-loading">"Loading full Markdown preview..."</div> }.into_any();
                    }
                    if let Some(err) = fetch_error.get() {
                        return view! { <div class="md-error">{err}</div> }.into_any();
                    }
                    let full_text = if file.status == FileStatus::Deleted {
                        base
                    } else {
                        target
                    };
                    let Some(doc) = full_text else {
                        return view! { <div class="md-muted">"Full preview unavailable."</div> }.into_any();
                    };
                    let extracted = extract_frontmatter(&doc);
                    let fm_offset = u32::try_from(extracted.raw_lines.len()).unwrap_or(u32::MAX);
                    let fm_line_end = (fm_offset > 0).then_some(fm_offset);
                    return view! {
                        <div class="md-preview-stack">
                            {extracted.data.map(|d| view! {
                                <FrontmatterSnapshotTable entries=d line_end=fm_line_end />
                            })}
                            <div class="md-rendered">
                                {render_markdown_to_view_with_offset(&extracted.content, fm_offset)}
                            </div>
                        </div>
                    }.into_any();
                }

                let (diff_starts_at_top, raw_blocks) = merged_chunks.with(|chunks| {
                    let starts_top = chunks
                        .first()
                        .is_some_and(|c| c.old_start <= 1 || c.new_start <= 1);
                    (starts_top, build_preview_blocks(chunks))
                });
                let fm = render_frontmatter_for_diff(
                    base.as_deref(),
                    target.as_deref(),
                    file.status,
                    diff_starts_at_top,
                );
                let stripped = strip_frontmatter_blocks(raw_blocks, &fm.strip_base, &fm.strip_target);
                let render_blocks = build_render_blocks(stripped);

                view! {
                    <div class="md-preview-stack">
                        {fm.table}
                        {render_blocks.into_iter().map(|block| match block {
                            PreviewRenderBlock::FencedCode { lang, lines, line_range } => {
                                let (block_ls, block_le) = line_range_attrs(line_range);
                                let mut in_block_comment = false;
                                let rendered_lines = lines
                                    .into_iter()
                                    .map(|line| {
                                        let cls = code_diff_line_class(line.kind);
                                        let line_attr = line.display_line().map(|n| n.to_string());
                                        let tokenized = tokenize_code_line(
                                            &line.content,
                                            lang.as_deref(),
                                            in_block_comment,
                                        );
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
                                    <div
                                        class="md-diff-block md-diff-block--code"
                                        data-md-line=block_ls
                                        data-md-line-end=block_le
                                    >
                                        <div class="md-diff-code-box">
                                            {lang.map(|l| view! { <div class="md-code-lang">{l}</div> })}
                                            {rendered_lines}
                                        </div>
                                    </div>
                                }.into_any()
                            }
                            PreviewRenderBlock::Markdown { kind, lines } => {
                                let joined: String = lines
                                    .iter()
                                    .map(|l| l.content.as_str())
                                    .collect::<Vec<_>>()
                                    .join("\n");
                                if joined.trim().is_empty() {
                                    return ().into_any();
                                }
                                let line_nums: Vec<Option<u32>> =
                                    lines.iter().map(PreviewSourceLine::display_line).collect();
                                let (block_ls, block_le) =
                                    line_range_attrs(range_from_line_numbers(&line_nums));
                                let cls = diff_block_border_class(kind);
                                let plain = lines
                                    .iter()
                                    .all(|l| is_non_renderable_md_line(&l.content));
                                if plain {
                                    view! {
                                        <div
                                            class=cls
                                            data-md-line=block_ls
                                            data-md-line-end=block_le
                                        >
                                            <pre class="md-plain-def">{joined.trim().to_owned()}</pre>
                                        </div>
                                    }.into_any()
                                } else {
                                    view! {
                                        <div
                                            class=cls
                                            data-md-line=block_ls
                                            data-md-line-end=block_le
                                        >
                                            <div class="md-rendered">
                                                {render_markdown_to_view_with_table(&joined, &line_nums)}
                                            </div>
                                        </div>
                                    }.into_any()
                                }
                            }
                        }).collect_view()}
                    </div>
                }.into_any()
            }}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tellit_types::DiffLine;

    #[test]
    fn validates_safe_urls_and_blocks_xss_schemes() {
        assert!(is_safe_url("https://example.com/a?x=1"));
        assert!(is_safe_url("http://localhost:4966"));
        assert!(is_safe_url("mailto:dev@example.com"));
        assert!(is_safe_url("#section-1"));
        assert!(is_safe_url("./images/pic.png"));
        assert!(is_safe_url("docs/readme.md"));

        assert!(!is_safe_url("javascript:alert(1)"));
        assert!(!is_safe_url("  JaVaScRiPt:alert(1)"));
        assert!(!is_safe_url("data:text/html,<script>alert(1)</script>"));
        assert!(!is_safe_url("vbscript:msgbox(1)"));
        assert!(!is_safe_url("java\nscript:alert(1)"));
    }

    #[test]
    fn parses_gfm_tables_tasklists_and_ignores_html_comments() {
        let md = "# Title\n\n<!-- hidden -->\n\n- [x] Done\n- [ ] Todo\n\n| A | B |\n| --- | ---: |\n| 1 | ~~old~~ |\n\n<script>bad</script>\n";
        let nodes = parse_markdown_nodes(md);

        assert!(matches!(nodes[0], MdNode::Heading { level: 1, .. }));
        assert!(nodes.iter().any(|n| matches!(
            n,
            MdNode::List { items, .. } if matches!(items.first(), Some(MdNode::ListItem { checked: Some(true), .. }))
        )));
        assert!(nodes.iter().any(|n| matches!(n, MdNode::Table { .. })));
        // HTML comment skipped, raw tag kept as literal text rather than raw HTML.
        let flat = alloc::format!("{nodes:?}");
        assert!(!flat.contains("hidden"));
        assert!(flat.contains("Text(\"<script>bad</script>"));
    }

    #[test]
    fn groups_fenced_code_blocks_and_strips_frontmatter_in_diff_preview() {
        let file = Arc::new(DiffFile {
            path: String::from("README.md"),
            old_path: None,
            status: FileStatus::Modified,
            additions: 2,
            deletions: 1,
            is_generated: false,
            chunks: alloc::vec![tellit_types::DiffChunk {
                old_start: 1,
                old_lines: 6,
                new_start: 1,
                new_lines: 6,
                header: String::from("@@ -1,6 +1,6 @@"),
                lines: alloc::vec![
                    DiffLine {
                        kind: DiffLineKind::Context,
                        content: String::from("---"),
                        old_line_number: Some(1),
                        new_line_number: Some(1),
                        syntax_spans: Vec::new(),
                        word_diff: Vec::new(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Delete,
                        content: String::from("title: Old"),
                        old_line_number: Some(2),
                        new_line_number: None,
                        syntax_spans: Vec::new(),
                        word_diff: Vec::new(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Add,
                        content: String::from("title: New"),
                        old_line_number: None,
                        new_line_number: Some(2),
                        syntax_spans: Vec::new(),
                        word_diff: Vec::new(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Context,
                        content: String::from("---"),
                        old_line_number: Some(3),
                        new_line_number: Some(3),
                        syntax_spans: Vec::new(),
                        word_diff: Vec::new(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Context,
                        content: String::from("```rust"),
                        old_line_number: Some(4),
                        new_line_number: Some(4),
                        syntax_spans: Vec::new(),
                        word_diff: Vec::new(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Add,
                        content: String::from("let x = 1;"),
                        old_line_number: None,
                        new_line_number: Some(5),
                        syntax_spans: Vec::new(),
                        word_diff: Vec::new(),
                    },
                    DiffLine {
                        kind: DiffLineKind::Context,
                        content: String::from("```"),
                        old_line_number: Some(5),
                        new_line_number: Some(6),
                        syntax_spans: Vec::new(),
                        word_diff: Vec::new(),
                    },
                ],
            }],
            chunks_loaded: true,
        });
        let chunks = crate::components::text_diff::expanded::merge_chunks_for_file(&file, None);

        let raw = build_preview_blocks(&chunks);
        let stripped = strip_frontmatter_blocks(
            raw,
            &[
                String::from("---"),
                String::from("title: Old"),
                String::from("---"),
            ],
            &[
                String::from("---"),
                String::from("title: New"),
                String::from("---"),
            ],
        );
        let rendered = build_render_blocks(stripped);
        assert_eq!(rendered.len(), 1);
        assert!(matches!(
            &rendered[0],
            PreviewRenderBlock::FencedCode {
                lang: Some(l),
                lines,
                line_range: Some(LineRangeSpec { start: 4, end: 6 }),
            } if l == "rust" && lines.len() == 1 && lines[0].kind == PreviewBlockKind::Add && lines[0].new_line == Some(5)
        ));
    }

    #[test]
    fn maps_markdown_blocks_to_source_lines_with_frontmatter_offset_and_crlf() {
        let doc = "---\r\ntitle: Demo\r\n---\r\n# Heading\r\n\r\nPara line 1\r\nPara line 2\r\n\r\n- Item A\r\n- Item B\r\n\r\n```rust\r\nlet x = 1;\r\n```\r\n\r\n| K | V |\r\n| - | - |\r\n| a | b |\r\n\r\n---\r\n";
        let extracted = extract_frontmatter(doc);
        let fm_offset = u32::try_from(extracted.raw_lines.len()).unwrap();
        assert_eq!(fm_offset, 3);

        let nodes =
            parse_markdown_nodes_with_lines(&extracted.content, LineMapper::Offset(fm_offset));
        assert!(matches!(
            nodes[0],
            MdNode::Heading {
                level: 1,
                line_range: Some(LineRangeSpec { start: 4, end: 4 }),
                ..
            }
        ));
        assert!(matches!(
            nodes[1],
            MdNode::Paragraph {
                line_range: Some(LineRangeSpec { start: 6, end: 7 }),
                ..
            }
        ));
        assert!(matches!(
            &nodes[2],
            MdNode::List {
                items,
                line_range: Some(LineRangeSpec { start: 9, end: 10 }),
                ..
            } if matches!(
                items.first(),
                Some(MdNode::ListItem { line_range: Some(LineRangeSpec { start: 9, end: 9 }), .. })
            ) && matches!(
                items.get(1),
                Some(MdNode::ListItem { line_range: Some(LineRangeSpec { start: 10, end: 10 }), .. })
            )
        ));
        assert!(matches!(
            nodes[3],
            MdNode::CodeBlock {
                line_range: Some(LineRangeSpec { start: 12, end: 14 }),
                ..
            }
        ));
        assert!(matches!(
            &nodes[4],
            MdNode::Table {
                head_line_range: Some(LineRangeSpec { start: 16, end: 16 }),
                body,
                line_range: Some(LineRangeSpec { start: 16, end: 18 }),
                ..
            } if body.len() == 1 && body[0].line_range == Some(LineRangeSpec { start: 18, end: 18 })
        ));
        assert!(matches!(
            nodes[5],
            MdNode::Rule {
                line_range: Some(LineRangeSpec { start: 20, end: 20 }),
            }
        ));
    }

    #[test]
    fn preview_blocks_split_at_chunk_boundaries_and_preserve_old_and_new_lines() {
        let mk =
            |kind: DiffLineKind, content: &str, old_n: Option<u32>, new_n: Option<u32>| DiffLine {
                kind,
                content: String::from(content),
                old_line_number: old_n,
                new_line_number: new_n,
                syntax_spans: Vec::new(),
                word_diff: Vec::new(),
            };
        let file = Arc::new(DiffFile {
            path: String::from("GUIDE.md"),
            old_path: None,
            status: FileStatus::Modified,
            additions: 1,
            deletions: 1,
            is_generated: false,
            chunks: alloc::vec![
                tellit_types::DiffChunk {
                    old_start: 10,
                    old_lines: 2,
                    new_start: 10,
                    new_lines: 1,
                    header: String::from("@@ -10,2 +10,1 @@"),
                    lines: alloc::vec![
                        mk(DiffLineKind::Context, "Chunk 1 context", Some(10), Some(10)),
                        mk(DiffLineKind::Delete, "Deleted paragraph", Some(11), None),
                    ],
                },
                tellit_types::DiffChunk {
                    old_start: 50,
                    old_lines: 1,
                    new_start: 49,
                    new_lines: 2,
                    header: String::from("@@ -50,1 +49,2 @@"),
                    lines: alloc::vec![
                        mk(DiffLineKind::Context, "Chunk 2 context", Some(50), Some(49)),
                        mk(DiffLineKind::Add, "Added paragraph", None, Some(50)),
                    ],
                },
            ],
            chunks_loaded: true,
        });
        let chunks = crate::components::text_diff::expanded::merge_chunks_for_file(&file, None);
        let blocks = build_preview_blocks(&chunks);
        // Chunks never merge across the hidden gap between line 11 and line 49.
        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[0].chunk_index, 0);
        assert_eq!(blocks[1].chunk_index, 0);
        assert_eq!(blocks[2].chunk_index, 1);
        assert_eq!(blocks[3].chunk_index, 1);

        let rendered = build_render_blocks(blocks);
        assert_eq!(rendered.len(), 4);

        // Deleted block falls back to old_line (11) when new_line is None.
        let PreviewRenderBlock::Markdown {
            lines: del_lines, ..
        } = &rendered[1]
        else {
            panic!("expected deleted markdown block");
        };
        let del_nums: Vec<Option<u32>> = del_lines
            .iter()
            .map(PreviewSourceLine::display_line)
            .collect();
        assert_eq!(del_nums, vec![Some(11)]);
        let del_nodes =
            parse_markdown_nodes_with_lines("Deleted paragraph", LineMapper::Table(&del_nums));
        assert!(matches!(
            del_nodes[0],
            MdNode::Paragraph {
                line_range: Some(LineRangeSpec { start: 11, end: 11 }),
                ..
            }
        ));

        // Second chunk preserves its exact new line numbers (49 and 50).
        let PreviewRenderBlock::Markdown {
            lines: add_lines, ..
        } = &rendered[3]
        else {
            panic!("expected added markdown block");
        };
        assert_eq!(add_lines[0].new_line, Some(50));
    }
}
