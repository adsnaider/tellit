//! YAML frontmatter extraction, canonical comparison, and table presentation for Markdown diffs.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use leptos::prelude::*;
use yaml_rust::{Yaml, YamlLoader};

/// Typed representation of a parsed YAML frontmatter value preserving key ordering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontmatterValue {
    /// YAML null (`~`, `null`, or empty scalar).
    Null,
    /// Boolean scalar.
    Bool(bool),
    /// 64-bit signed integer scalar.
    Integer(i64),
    /// Arbitrary-precision decimal/real representation.
    Real(String),
    /// Text scalar.
    String(String),
    /// Ordered sequence of nested values.
    Array(Vec<FrontmatterValue>),
    /// Ordered mapping of key-value pairs.
    Object(Vec<(String, FrontmatterValue)>),
}

impl FrontmatterValue {
    fn from_yaml(yaml: &Yaml) -> Self {
        match yaml {
            Yaml::Real(s) => Self::Real(s.clone()),
            Yaml::Integer(i) => Self::Integer(*i),
            Yaml::String(s) => Self::String(s.clone()),
            Yaml::Boolean(b) => Self::Bool(*b),
            Yaml::Array(items) => Self::Array(items.iter().map(Self::from_yaml).collect()),
            Yaml::Hash(map) => {
                let entries = map
                    .iter()
                    .map(|(k, v)| (yaml_key_string(k), Self::from_yaml(v)))
                    .collect();
                Self::Object(entries)
            }
            Yaml::Null | Yaml::BadValue | Yaml::Alias(_) => Self::Null,
        }
    }

    /// Returns a canonical representation independent of key ordering so identical maps compare equal.
    #[must_use]
    pub fn canonical_key(&self) -> String {
        match self {
            Self::Null => String::from("null"),
            Self::Bool(b) => alloc::format!("bool:{b}"),
            Self::Integer(i) => alloc::format!("int:{i}"),
            Self::Real(r) => alloc::format!("real:{r}"),
            Self::String(s) => serde_json::to_string(s).unwrap_or_else(|_| s.clone()),
            Self::Array(items) => {
                let joined: Vec<String> = items.iter().map(Self::canonical_key).collect();
                alloc::format!("[{}]", joined.join(","))
            }
            Self::Object(entries) => {
                let mut pairs: Vec<(String, String)> = entries
                    .iter()
                    .map(|(k, v)| (k.clone(), v.canonical_key()))
                    .collect();
                pairs.sort_by(|a, b| a.0.cmp(&b.0));
                let rendered: Vec<String> = pairs
                    .into_iter()
                    .map(|(k, v)| alloc::format!("{k}:{v}"))
                    .collect();
                alloc::format!("{{{}}}", rendered.join(","))
            }
        }
    }

    fn format_compact(&self) -> String {
        match self {
            Self::Null => String::from("null"),
            Self::Bool(b) => b.to_string(),
            Self::Integer(i) => i.to_string(),
            Self::Real(r) => r.clone(),
            Self::String(s) => s.clone(),
            Self::Array(items) => {
                let parts: Vec<String> = items.iter().map(Self::format_compact).collect();
                alloc::format!("[{}]", parts.join(", "))
            }
            Self::Object(entries) => {
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| alloc::format!("{k}: {}", v.format_compact()))
                    .collect();
                alloc::format!("{{{}}}", parts.join(", "))
            }
        }
    }
}

fn yaml_key_string(yaml: &Yaml) -> String {
    match yaml {
        Yaml::String(s) => s.clone(),
        Yaml::Integer(i) => i.to_string(),
        Yaml::Real(r) => r.clone(),
        Yaml::Boolean(b) => b.to_string(),
        Yaml::Null => String::from("null"),
        other => FrontmatterValue::from_yaml(other).format_compact(),
    }
}

/// Ordered key-value frontmatter entries parsed from leading YAML metadata.
pub type FrontmatterEntries = Vec<(String, FrontmatterValue)>;

/// Output from separating leading YAML frontmatter from Markdown body text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedFrontmatter {
    /// Parsed frontmatter key-value pairs when valid leading YAML exists.
    pub data: Option<FrontmatterEntries>,
    /// Remaining Markdown body after stripping valid frontmatter.
    pub content: String,
    /// Raw frontmatter lines (including `---` fences) without trailing `\r`/`\n`.
    pub raw_lines: Vec<String>,
}

struct SplitFrontmatter<'a> {
    raw_block: &'a str,
    yaml_body: &'a str,
    rest: &'a str,
}

fn no_frontmatter(text: &str) -> ExtractedFrontmatter {
    ExtractedFrontmatter {
        data: None,
        content: String::from(text),
        raw_lines: Vec::new(),
    }
}

/// Parses optional leading YAML frontmatter (`--- ... ---`) out of `text`.
#[must_use]
pub fn extract_frontmatter(text: &str) -> ExtractedFrontmatter {
    let Some(split) = split_leading_frontmatter(text) else {
        return no_frontmatter(text);
    };

    let raw_lines: Vec<String> = split
        .raw_block
        .trim_end_matches(['\r', '\n'])
        .split('\n')
        .map(|line| String::from(line.strip_suffix('\r').unwrap_or(line)))
        .collect();

    if split.yaml_body.trim().is_empty() {
        return ExtractedFrontmatter {
            data: None,
            content: String::from(split.rest),
            raw_lines,
        };
    }

    let Ok(docs) = YamlLoader::load_from_str(split.yaml_body) else {
        return no_frontmatter(text);
    };
    let Some(Yaml::Hash(map)) = docs.first() else {
        return no_frontmatter(text);
    };

    let entries: FrontmatterEntries = map
        .iter()
        .map(|(k, v)| (yaml_key_string(k), FrontmatterValue::from_yaml(v)))
        .collect();

    ExtractedFrontmatter {
        data: Some(entries),
        content: String::from(split.rest),
        raw_lines,
    }
}

fn split_leading_frontmatter(text: &str) -> Option<SplitFrontmatter<'_>> {
    let after_open = text
        .strip_prefix("---\r\n")
        .or_else(|| text.strip_prefix("---\n"))?;

    // Immediate empty frontmatter (`---\n---`).
    if let Some(rest) = after_open.strip_prefix("---")
        && (rest.is_empty() || rest.starts_with('\n') || rest.starts_with("\r\n"))
    {
        let consumed = consume_line_ending(text, text.len() - rest.len());
        return Some(SplitFrontmatter {
            raw_block: &text[..consumed],
            yaml_body: "",
            rest: &text[consumed..],
        });
    }

    let mut offset = text.len() - after_open.len();
    for raw_line in after_open.split_inclusive('\n') {
        let line_no_nl = raw_line.trim_end_matches(['\r', '\n']);
        if line_no_nl == "---" {
            let yaml_start = text.len() - after_open.len();
            let yaml_end = offset.saturating_sub(1);
            let yaml_end = if yaml_end >= yaml_start && text.as_bytes()[yaml_end] == b'\r' {
                yaml_end
            } else {
                offset
            };
            let block_end = offset + raw_line.len();
            return Some(SplitFrontmatter {
                raw_block: &text[..block_end],
                yaml_body: text.get(yaml_start..yaml_end.max(yaml_start)).unwrap_or(""),
                rest: &text[block_end..],
            });
        }
        offset += raw_line.len();
    }

    None
}

fn consume_line_ending(text: &str, idx: usize) -> usize {
    if text[idx..].starts_with("\r\n") {
        idx + 2
    } else if text[idx..].starts_with('\n') {
        idx + 1
    } else {
        idx
    }
}

/// Diff status of a single YAML frontmatter key across base and target revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrontmatterDiffStatus {
    /// Key added in target frontmatter.
    Added,
    /// Key removed from base frontmatter.
    Removed,
    /// Key present in both with different canonical values.
    Modified,
    /// Key unchanged between base and target.
    Unchanged,
}

/// Single row comparing one frontmatter key across base and target versions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontmatterDiffEntry {
    /// Top-level YAML key name.
    pub key: String,
    /// Change status for this key.
    pub status: FrontmatterDiffStatus,
    /// Base revision value when present.
    pub old_value: Option<FrontmatterValue>,
    /// Target revision value when present.
    pub new_value: Option<FrontmatterValue>,
}

/// Computes per-key frontmatter differences between `old_data` and `new_data`.
#[must_use]
pub fn compute_frontmatter_diff(
    old_data: Option<&[(String, FrontmatterValue)]>,
    new_data: Option<&[(String, FrontmatterValue)]>,
) -> Vec<FrontmatterDiffEntry> {
    match (old_data, new_data) {
        (None, None) => Vec::new(),
        (Some(old_entries), None) => old_entries
            .iter()
            .map(|(k, v)| FrontmatterDiffEntry {
                key: k.clone(),
                status: FrontmatterDiffStatus::Removed,
                old_value: Some(v.clone()),
                new_value: None,
            })
            .collect(),
        (None, Some(new_entries)) => new_entries
            .iter()
            .map(|(k, v)| FrontmatterDiffEntry {
                key: k.clone(),
                status: FrontmatterDiffStatus::Added,
                old_value: None,
                new_value: Some(v.clone()),
            })
            .collect(),
        (Some(old_entries), Some(new_entries)) => {
            let mut out = Vec::new();
            for (k, new_val) in new_entries {
                if let Some((_, old_val)) = old_entries.iter().find(|(ok, _)| ok == k) {
                    let status = if old_val.canonical_key() == new_val.canonical_key() {
                        FrontmatterDiffStatus::Unchanged
                    } else {
                        FrontmatterDiffStatus::Modified
                    };
                    out.push(FrontmatterDiffEntry {
                        key: k.clone(),
                        status,
                        old_value: Some(old_val.clone()),
                        new_value: Some(new_val.clone()),
                    });
                } else {
                    out.push(FrontmatterDiffEntry {
                        key: k.clone(),
                        status: FrontmatterDiffStatus::Added,
                        old_value: None,
                        new_value: Some(new_val.clone()),
                    });
                }
            }
            for (k, old_val) in old_entries {
                if !new_entries.iter().any(|(nk, _)| nk == k) {
                    out.push(FrontmatterDiffEntry {
                        key: k.clone(),
                        status: FrontmatterDiffStatus::Removed,
                        old_value: Some(old_val.clone()),
                        new_value: None,
                    });
                }
            }
            out
        }
    }
}

fn render_frontmatter_value(value: Option<&FrontmatterValue>, depth: usize) -> AnyView {
    let Some(val) = value else {
        return view! { <span class="fm-empty">""</span> }.into_any();
    };
    match val {
        FrontmatterValue::Null => view! { <span class="fm-muted">"null"</span> }.into_any(),
        FrontmatterValue::Bool(b) => view! { <span>{b.to_string()}</span> }.into_any(),
        FrontmatterValue::Integer(i) => view! { <span>{i.to_string()}</span> }.into_any(),
        FrontmatterValue::Real(r) => view! { <span>{r.clone()}</span> }.into_any(),
        FrontmatterValue::String(s) if s.is_empty() => {
            view! { <span class="fm-muted">r#""""#</span> }.into_any()
        }
        FrontmatterValue::String(s) => view! { <span>{s.clone()}</span> }.into_any(),
        FrontmatterValue::Array(_) => {
            view! { <code class="fm-inline-code">{val.format_compact()}</code> }.into_any()
        }
        FrontmatterValue::Object(entries) if depth == 0 => view! {
            <table class="fm-nested-table">
                <tbody>
                    {entries.iter().map(|(k, v)| {
                        let key = k.clone();
                        let rendered_val = render_frontmatter_value(Some(v), depth + 1);
                        view! {
                            <tr>
                                <td class="fm-nested-key">{key}</td>
                                <td class="fm-nested-val">{rendered_val}</td>
                            </tr>
                        }
                    }).collect_view()}
                </tbody>
            </table>
        }
        .into_any(),
        FrontmatterValue::Object(_) => {
            view! { <code class="fm-inline-code">{val.format_compact()}</code> }.into_any()
        }
    }
}

/// Renders either a single snapshot table or a Before/After diff table for YAML frontmatter.
#[component]
pub fn FrontmatterDiffTable(
    entries: Vec<FrontmatterDiffEntry>,
    #[prop(optional)] label: Option<String>,
) -> impl IntoView {
    if entries.is_empty() {
        return ().into_any();
    }

    view! {
        <div class="fm-wrap">
            {label.map(|lbl| view! { <div class="fm-label">{lbl}</div> })}
            <table class="fm-table">
                <thead>
                    <tr>
                        <th class="fm-col-key">"Key"</th>
                        <th>"Before"</th>
                        <th>"After"</th>
                    </tr>
                </thead>
                <tbody>
                    {entries.into_iter().map(|entry| {
                        let before_cls = match entry.status {
                            FrontmatterDiffStatus::Removed | FrontmatterDiffStatus::Modified => "fm-cell fm-cell--del",
                            _ => "fm-cell",
                        };
                        let after_cls = match entry.status {
                            FrontmatterDiffStatus::Added | FrontmatterDiffStatus::Modified => "fm-cell fm-cell--add",
                            _ => "fm-cell",
                        };
                        let before_val = if entry.status == FrontmatterDiffStatus::Added {
                            None
                        } else {
                            entry.old_value.as_ref()
                        };
                        let after_val = if entry.status == FrontmatterDiffStatus::Removed {
                            None
                        } else {
                            entry.new_value.as_ref()
                        };
                        let rendered_before = render_frontmatter_value(before_val, 0);
                        let rendered_after = render_frontmatter_value(after_val, 0);
                        view! {
                            <tr>
                                <td class="fm-key">{entry.key}</td>
                                <td class=before_cls>{rendered_before}</td>
                                <td class=after_cls>{rendered_after}</td>
                            </tr>
                        }
                    }).collect_view()}
                </tbody>
            </table>
        </div>
    }
    .into_any()
}

/// Renders a single-revision snapshot table for YAML frontmatter in Full Preview mode.
#[component]
pub fn FrontmatterSnapshotTable(
    entries: Vec<(String, FrontmatterValue)>,
    #[prop(optional)] label: Option<String>,
) -> impl IntoView {
    if entries.is_empty() {
        return ().into_any();
    }

    view! {
        <div class="fm-wrap">
            {label.map(|lbl| view! { <div class="fm-label">{lbl}</div> })}
            <table class="fm-table">
                <thead>
                    <tr>
                        <th class="fm-col-key">"Key"</th>
                        <th>"Value"</th>
                    </tr>
                </thead>
                <tbody>
                    {entries.into_iter().map(|(key, val)| {
                        let rendered = render_frontmatter_value(Some(&val), 0);
                        view! {
                            <tr>
                                <td class="fm-key">{key}</td>
                                <td class="fm-cell">{rendered}</td>
                            </tr>
                        }
                    }).collect_view()}
                </tbody>
            </table>
        </div>
    }
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_and_diffs_yaml_frontmatter_and_rejects_body_rules() {
        let base_md =
            "---\ntitle: Old Title\ntags:\n  - a\n  - b\nmeta:\n  x: 1\n  y: 2\n---\n# Hello\n";
        let target_md = "---\ntitle: New Title\ntags:\n  - a\n  - b\nmeta:\n  y: 2\n  x: 1\nauthor: alice\n---\n# Hello\n";

        let old_ext = extract_frontmatter(base_md);
        let new_ext = extract_frontmatter(target_md);
        assert_eq!(old_ext.content, "# Hello\n");
        assert_eq!(new_ext.content, "# Hello\n");
        assert_eq!(old_ext.raw_lines.first().map(String::as_str), Some("---"));
        assert_eq!(old_ext.raw_lines.last().map(String::as_str), Some("---"));

        let diff = compute_frontmatter_diff(old_ext.data.as_deref(), new_ext.data.as_deref());
        let status_of = |key: &str| diff.iter().find(|e| e.key == key).map(|e| e.status);
        assert_eq!(status_of("title"), Some(FrontmatterDiffStatus::Modified));
        assert_eq!(status_of("tags"), Some(FrontmatterDiffStatus::Unchanged));
        assert_eq!(status_of("meta"), Some(FrontmatterDiffStatus::Unchanged));
        assert_eq!(status_of("author"), Some(FrontmatterDiffStatus::Added));

        let plain = "# Heading\n\n---\nNot frontmatter\n";
        assert!(extract_frontmatter(plain).data.is_none());

        let hr_wrapped = "---\n\n# Heading\n\nBody text\n\n---\nTrailing\n";
        let hr_ext = extract_frontmatter(hr_wrapped);
        assert!(hr_ext.data.is_none());
        assert_eq!(hr_ext.content, hr_wrapped);
        assert!(hr_ext.raw_lines.is_empty());

        let comment_wrapped = "---\n# just a comment\n---\n# After\n";
        let comment_ext = extract_frontmatter(comment_wrapped);
        assert!(comment_ext.data.is_none());
        assert_eq!(comment_ext.content, comment_wrapped);
        assert!(comment_ext.raw_lines.is_empty());
    }
}
