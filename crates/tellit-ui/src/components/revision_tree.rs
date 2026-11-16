//! Jujutsu DAG revision graph sidebar panel and pure-Rust SVG layout calculator.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use leptos::prelude::*;
use leptos::svg as lsvg;
use tellit_types::{JjChange, JjGraphRow};
use wasm_bindgen::JsCast;
use web_sys::{HtmlInputElement, SubmitEvent};

use super::review_dot::ReviewDot;
use crate::state::{REVISIONS_TREE_STORAGE_KEY, load_storage_bool, save_storage_bool};

const COL_WIDTH: f64 = 11.0;
const COMMIT_ROW_HEIGHT: f64 = 28.0;
const CONNECTOR_ROW_HEIGHT: f64 = 16.0;
const STEM_CLEARANCE: f64 = 5.0;
const UPWARD_CONNECTING_GLYPHS: &str = "│|:╯┘'╰└`├┤┴┼+";

/// Semantic node type rendered for a commit cell in the Jujutsu DAG gutter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphNodeKind {
    /// Working-copy revision (`@`).
    Working,
    /// Mutable non-working-copy revision (`○` / `o`).
    Mutable,
    /// Immutable revision (`◆` / `*`).
    Immutable,
    /// Conflicted revision (`×` / `x`).
    Conflict,
    /// Hidden revision (`◌`).
    Hidden,
}

/// Diagonal line direction across a graph cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagonalKind {
    /// Bottom-left to top-right (`/`).
    Slash,
    /// Top-left to bottom-right (`\`).
    Backslash,
}

/// Per-cell SVG geometry flags derived from a parsed `jj log` graph character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GraphCellLayout {
    /// Connects to the upper cell boundary.
    pub up: bool,
    /// Connects to the lower cell boundary.
    pub down: bool,
    /// Connects to the left cell boundary.
    pub left: bool,
    /// Connects to the right cell boundary.
    pub right: bool,
    /// Optional diagonal line across the cell.
    pub diagonal: Option<DiagonalKind>,
    /// Commit node glyph when this cell holds a commit.
    pub node_kind: Option<GraphNodeKind>,
    /// Elided revision marker (`~`).
    pub is_elided: bool,
    /// Dashed vertical line indicating elided revisions.
    pub dashed_down: bool,
}

/// Computed SVG cell instructions for a single row in [`JjRevisionTree`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRowLayout {
    /// Ordered column cells from left to right.
    pub cols: Vec<GraphCellLayout>,
}

fn node_kind_from_char(ch: char) -> Option<GraphNodeKind> {
    match ch {
        '@' => Some(GraphNodeKind::Working),
        '○' | 'o' => Some(GraphNodeKind::Mutable),
        '◆' | '*' => Some(GraphNodeKind::Immutable),
        '×' | 'x' => Some(GraphNodeKind::Conflict),
        '◌' => Some(GraphNodeKind::Hidden),
        _ => None,
    }
}

fn connects_upward_from_char(ch: char) -> bool {
    node_kind_from_char(ch).is_some() || ch == '~' || UPWARD_CONNECTING_GLYPHS.contains(ch)
}

/// Computes per-row, per-column SVG rendering instructions from parsed `jj log` graph rows.
#[must_use]
pub fn compute_graph_layout(rows: &[JjGraphRow]) -> Vec<GraphRowLayout> {
    let mut layout: Vec<GraphRowLayout> = rows
        .iter()
        .map(|row| {
            let (graph_cols, down_cols, is_commit, bridges_elided) = match row {
                JjGraphRow::Commit {
                    graph_cols,
                    down_cols,
                    ..
                } => (graph_cols.as_slice(), down_cols.as_slice(), true, false),
                JjGraphRow::Connector {
                    graph_cols,
                    bridges_elided,
                    ..
                } => (graph_cols.as_slice(), &[][..], false, *bridges_elided),
            };

            let cols = graph_cols
                .iter()
                .enumerate()
                .map(|(col_idx, &ch)| {
                    let mut cell = GraphCellLayout::default();
                    if let Some(kind) = node_kind_from_char(ch) {
                        cell.node_kind = Some(kind);
                        cell.down = is_commit && down_cols.contains(&col_idx);
                        return cell;
                    }
                    if ch == '~' {
                        cell.is_elided = true;
                        cell.dashed_down = !is_commit && bridges_elided;
                        return cell;
                    }

                    match ch {
                        '│' | '|' => {
                            cell.up = true;
                            cell.down = if is_commit {
                                down_cols.contains(&col_idx)
                            } else {
                                true
                            };
                        }
                        ':' => {
                            cell.up = true;
                            cell.down = if is_commit {
                                down_cols.contains(&col_idx)
                            } else {
                                true
                            };
                            cell.dashed_down = true;
                        }
                        '─' | '-' => {
                            cell.left = true;
                            cell.right = true;
                        }
                        '╭' | '┌' => {
                            cell.down = true;
                            cell.right = true;
                        }
                        '╮' | '┐' => {
                            cell.down = true;
                            cell.left = true;
                        }
                        '╯' | '┘' | '\'' => {
                            cell.up = true;
                            cell.left = true;
                        }
                        '╰' | '└' | '`' => {
                            cell.up = true;
                            cell.right = true;
                        }
                        '├' => {
                            cell.up = true;
                            cell.down = true;
                            cell.right = true;
                        }
                        '┤' => {
                            cell.up = true;
                            cell.down = true;
                            cell.left = true;
                        }
                        '┬' => {
                            cell.down = true;
                            cell.left = true;
                            cell.right = true;
                        }
                        '┴' => {
                            cell.up = true;
                            cell.left = true;
                            cell.right = true;
                        }
                        '┼' | '+' => {
                            cell.up = true;
                            cell.down = true;
                            cell.left = true;
                            cell.right = true;
                        }
                        '/' => cell.diagonal = Some(DiagonalKind::Slash),
                        '\\' => cell.diagonal = Some(DiagonalKind::Backslash),
                        _ => {}
                    }
                    cell
                })
                .collect();

            GraphRowLayout { cols }
        })
        .collect();

    for r in 0..rows.len() {
        for c in 0..layout[r].cols.len() {
            let cell = layout[r].cols[c];
            if (cell.node_kind.is_some() || cell.is_elided)
                && r > 0
                && layout[r - 1].cols.get(c).is_some_and(|prev| prev.down)
            {
                layout[r].cols[c].up = true;
            }

            if cell.is_elided && r + 1 < rows.len() {
                let next_cols = match &rows[r + 1] {
                    JjGraphRow::Commit { graph_cols, .. }
                    | JjGraphRow::Connector { graph_cols, .. } => graph_cols,
                };
                let next_char = next_cols.get(c).copied().unwrap_or(' ');
                if connects_upward_from_char(next_char) {
                    layout[r].cols[c].down = true;
                    if let JjGraphRow::Connector { bridges_elided, .. } = rows[r]
                        && bridges_elided
                    {
                        layout[r].cols[c].dashed_down = true;
                    }
                }
            }
        }
    }

    layout
}

fn render_cell_svg(
    cell: GraphCellLayout,
    col_idx: usize,
    row_height: f64,
    is_selected: bool,
) -> impl IntoView {
    let x_left = (col_idx as f64) * COL_WIDTH;
    let x_right = x_left + COL_WIDTH;
    let cx = x_left + COL_WIDTH / 2.0;
    let cy = row_height / 2.0;

    let arm_count = usize::from(cell.up)
        + usize::from(cell.down)
        + usize::from(cell.left)
        + usize::from(cell.right);
    let is_pure_corner = cell.node_kind.is_none()
        && !cell.is_elided
        && cell.diagonal.is_none()
        && arm_count == 2
        && (cell.up || cell.down)
        && (cell.left || cell.right);

    let has_node_or_elision = cell.node_kind.is_some() || cell.is_elided;
    let up_end = if has_node_or_elision {
        (cy - STEM_CLEARANCE).max(0.0)
    } else {
        cy
    };
    let down_start = if has_node_or_elision {
        (cy + STEM_CLEARANCE).min(row_height)
    } else {
        cy
    };

    let edges_view = if is_pure_corner {
        let start_y = if cell.up { 0.0 } else { row_height };
        let end_x = if cell.left { x_left } else { x_right };
        let d = alloc::format!("M {cx} {start_y} Q {cx} {cy} {end_x} {cy}");
        lsvg::path()
            .attr("d", d)
            .attr("fill", "none")
            .attr("class", "jj-graph-edge")
            .attr("stroke-width", "1.5")
            .attr("stroke-linecap", "round")
            .into_any()
    } else {
        let up_line = cell.up.then(|| {
            let dash = (cell.dashed_down && !has_node_or_elision).then_some("2 2");
            lsvg::line()
                .attr("x1", cx)
                .attr("y1", 0.0)
                .attr("x2", cx)
                .attr("y2", up_end)
                .attr("class", "jj-graph-edge")
                .attr("stroke-width", "1.5")
                .attr("stroke-dasharray", dash)
        });
        let down_line = cell.down.then(|| {
            let dash = cell.dashed_down.then_some("2 2");
            lsvg::line()
                .attr("x1", cx)
                .attr("y1", down_start)
                .attr("x2", cx)
                .attr("y2", row_height)
                .attr("class", "jj-graph-edge")
                .attr("stroke-width", "1.5")
                .attr("stroke-dasharray", dash)
        });
        let left_line = cell.left.then(|| {
            lsvg::line()
                .attr("x1", x_left)
                .attr("y1", cy)
                .attr("x2", cx)
                .attr("y2", cy)
                .attr("class", "jj-graph-edge")
                .attr("stroke-width", "1.5")
        });
        let right_line = cell.right.then(|| {
            lsvg::line()
                .attr("x1", cx)
                .attr("y1", cy)
                .attr("x2", x_right)
                .attr("y2", cy)
                .attr("class", "jj-graph-edge")
                .attr("stroke-width", "1.5")
        });
        lsvg::g()
            .child((up_line, down_line, left_line, right_line))
            .into_any()
    };

    let diagonal_view = match cell.diagonal {
        Some(DiagonalKind::Slash) => Some(
            lsvg::line()
                .attr("x1", x_left)
                .attr("y1", row_height)
                .attr("x2", x_right)
                .attr("y2", 0.0)
                .attr("class", "jj-graph-edge")
                .attr("stroke-width", "1.5"),
        ),
        Some(DiagonalKind::Backslash) => Some(
            lsvg::line()
                .attr("x1", x_left)
                .attr("y1", 0.0)
                .attr("x2", x_right)
                .attr("y2", row_height)
                .attr("class", "jj-graph-edge")
                .attr("stroke-width", "1.5"),
        ),
        None => None,
    };

    let elided_view = cell.is_elided.then(|| {
        lsvg::text()
            .attr("x", cx)
            .attr("y", cy)
            .attr("text-anchor", "middle")
            .attr("dominant-baseline", "central")
            .attr("class", "jj-graph-elided")
            .child("~")
    });

    let node_view = match cell.node_kind {
        Some(GraphNodeKind::Working) => Some(
            lsvg::text()
                .attr("x", cx)
                .attr("y", cy)
                .attr("text-anchor", "middle")
                .attr("dominant-baseline", "central")
                .attr("class", "jj-graph-node-working")
                .child("@")
                .into_any(),
        ),
        Some(GraphNodeKind::Mutable) => Some(
            lsvg::circle()
                .attr("cx", cx)
                .attr("cy", cy)
                .attr("r", "3.5")
                .attr(
                    "class",
                    if is_selected {
                        "jj-graph-node-mutable jj-graph-node-mutable--selected"
                    } else {
                        "jj-graph-node-mutable"
                    },
                )
                .attr("stroke-width", "1.6")
                .into_any(),
        ),
        Some(GraphNodeKind::Immutable) => {
            let pts = alloc::format!(
                "{cx},{top} {right},{cy} {cx},{bottom} {left},{cy}",
                top = cy - 4.0,
                right = cx + 4.0,
                bottom = cy + 4.0,
                left = cx - 4.0,
            );
            Some(
                lsvg::polygon()
                    .attr("points", pts)
                    .attr("class", "jj-graph-node-immutable")
                    .attr("stroke-width", "1")
                    .into_any(),
            )
        }
        Some(GraphNodeKind::Conflict) => {
            let d = alloc::format!(
                "M {lx} {ty} L {rx} {by} M {rx} {ty} L {lx} {by}",
                lx = cx - 3.0,
                rx = cx + 3.0,
                ty = cy - 3.0,
                by = cy + 3.0,
            );
            Some(
                lsvg::path()
                    .attr("d", d)
                    .attr("class", "jj-graph-node-conflict")
                    .attr("stroke-width", "1.75")
                    .attr("stroke-linecap", "round")
                    .into_any(),
            )
        }
        Some(GraphNodeKind::Hidden) => Some(
            lsvg::circle()
                .attr("cx", cx)
                .attr("cy", cy)
                .attr("r", "3.5")
                .attr("class", "jj-graph-node-hidden")
                .attr("stroke-width", "1.5")
                .attr("stroke-dasharray", "2 1.5")
                .into_any(),
        ),
        None => None,
    };

    lsvg::g().child((edges_view, diagonal_view, elided_view, node_view))
}

fn render_row_svg(
    row_layout: &GraphRowLayout,
    gutter_width: f64,
    row_height: f64,
    is_selected: bool,
) -> impl IntoView + use<> {
    let cells = row_layout
        .cols
        .iter()
        .copied()
        .enumerate()
        .map(|(col_idx, cell)| render_cell_svg(cell, col_idx, row_height, is_selected))
        .collect::<Vec<_>>();

    lsvg::svg()
        .attr("width", gutter_width)
        .attr("height", row_height)
        .attr("class", "jj-graph-svg")
        .attr("aria-hidden", "true")
        .child(cells)
}

struct ShortestIdParts {
    pub prefix: String,
    pub suffix: String,
}

fn split_shortest_id(change: &JjChange) -> ShortestIdParts {
    let display = if change.short_change_id.is_empty() {
        change.change_id.chars().take(8).collect::<String>()
    } else {
        change.short_change_id.clone()
    };
    let prefix_chars = change.shortest_change_id.chars().count();
    let display_chars = display.chars().count();
    let split_len = prefix_chars.min(display_chars);
    if split_len == 0 {
        return ShortestIdParts {
            prefix: display,
            suffix: String::new(),
        };
    }
    ShortestIdParts {
        prefix: display.chars().take(split_len).collect(),
        suffix: display.chars().skip(split_len).collect(),
    }
}

/// Interactive list of Jujutsu DAG graph rows with shortest change ID prefixes, bookmarks, and review dots.
#[component]
pub fn JjRevisionTree(
    #[prop(into)] graph_rows: Signal<Vec<JjGraphRow>>,
    #[prop(into)] selected_change_id: Signal<String>,
    on_select_change: Callback<String>,
    #[prop(into, default = Signal::derive(|| false))] loading: Signal<bool>,
    #[prop(optional, default = "jj-tree-list")] list_class: &'static str,
) -> impl IntoView {
    let rows_with_layout = Memo::new(move |_| {
        let rows = graph_rows.get();
        let layout = compute_graph_layout(&rows);
        let max_cols = rows
            .iter()
            .map(|r| match r {
                JjGraphRow::Commit { graph_cols, .. }
                | JjGraphRow::Connector { graph_cols, .. } => graph_cols.len(),
            })
            .max()
            .unwrap_or(1)
            .max(1);
        let gutter_width = (max_cols as f64) * COL_WIDTH;
        let combined: Vec<(JjGraphRow, GraphRowLayout)> = rows.into_iter().zip(layout).collect();
        (combined, gutter_width)
    });

    view! {
        {move || {
            let (rows, gutter_width) = rows_with_layout.get();
            if loading.get() && rows.is_empty() {
                return view! {
                    <div class="jj-tree-empty">"Loading revisions..."</div>
                }.into_any();
            }
            if rows.is_empty() {
                return view! {
                    <div class="jj-tree-empty">"No revisions found"</div>
                }.into_any();
            }

            view! {
                <ul class=list_class aria-label="Jujutsu revisions">
                    <For
                        each=move || rows_with_layout.get().0
                        key=|(row, _)| match row {
                            JjGraphRow::Commit { key, change, .. } => alloc::format!(
                                "{key}:{}:{}:{}:{}",
                                change.comment_count,
                                change.unresolved_comment_count,
                                change.approved,
                                change.clean_rebase_approved,
                            ),
                            JjGraphRow::Connector { key, .. } => key.clone(),
                        }
                        children=move |(row, row_layout)| {
                            let gutter_w = gutter_width;
                            match row {
                                JjGraphRow::Connector { .. } => view! {
                                    <li class="jj-tree-connector" aria-hidden="true">
                                        {render_row_svg(&row_layout, gutter_w, CONNECTOR_ROW_HEIGHT, false)}
                                    </li>
                                }.into_any(),
                                JjGraphRow::Commit { change, .. } => {
                                    let change_val = *change;
                                    let cid = change_val.change_id.clone();
                                    let cid_click = cid.clone();
                                    let cid_selected = cid.clone();
                                    let is_selected_sig = Signal::derive(move || {
                                        selected_change_id.get() == cid_selected
                                    });
                                    let is_muted = change_val.is_empty && !change_val.has_description;
                                    let id_parts = split_shortest_id(&change_val);
                                    let id_prefix = id_parts.prefix;
                                    let id_suffix = id_parts.suffix;
                                    let review = change_val.review_status();
                                    let status_sig = Signal::derive(move || review);
                                    let bookmarks = change_val.bookmarks.clone();
                                    let description = change_val.description.clone();
                                    let is_empty = change_val.is_empty;
                                    let has_desc = change_val.has_description;
                                    let comment_count = change_val.comment_count;
                                    let unresolved_count = change_val.unresolved_comment_count;

                                    view! {
                                        <li>
                                            <button
                                                type="button"
                                                class=move || {
                                                    if is_selected_sig.get() {
                                                        "jj-tree-row jj-tree-row--selected"
                                                    } else if is_muted {
                                                        "jj-tree-row jj-tree-row--muted"
                                                    } else {
                                                        "jj-tree-row"
                                                    }
                                                }
                                                aria-current=move || is_selected_sig.get().then_some("true")
                                                on:click=move |_| on_select_change.run(cid_click.clone())
                                            >
                                                {move || render_row_svg(&row_layout, gutter_w, COMMIT_ROW_HEIGHT, is_selected_sig.get()).into_any()}
                                                <span class="jj-change-id">
                                                    <span class="jj-change-id__shortest">{id_prefix}</span>
                                                    {(!id_suffix.is_empty()).then(|| view! {
                                                        <span class="jj-change-id__rest">{id_suffix}</span>
                                                    })}
                                                </span>
                                                {bookmarks.into_iter().map(|bm| {
                                                    let title_bm = bm.clone();
                                                    view! {
                                                        <span class="jj-bookmark-pill" title=title_bm>{bm}</span>
                                                    }
                                                }).collect_view()}
                                                {is_empty.then(|| view! {
                                                    <span class="jj-empty-tag">"(empty)"</span>
                                                })}
                                                <span class={if has_desc {
                                                    "jj-tree-desc"
                                                } else {
                                                    "jj-tree-desc jj-tree-desc--none"
                                                }}>
                                                    {description}
                                                </span>
                                                <ReviewDot
                                                    status=status_sig
                                                    comment_count=comment_count
                                                    unresolved_count=unresolved_count
                                                />
                                            </button>
                                        </li>
                                    }.into_any()
                                }
                            }
                        }
                    />
                </ul>
            }.into_any()
        }}
    }
}

/// Collapsible sidebar panel hosting [`JjRevisionTree`] and a custom revset lookup input.
#[component]
pub fn JjRevisionTreePanel(
    #[prop(into)] changes: Signal<Vec<JjChange>>,
    #[prop(into)] graph_rows: Signal<Vec<JjGraphRow>>,
    #[prop(into)] selected_change_id: Signal<String>,
    on_select_change: Callback<String>,
    on_lookup_query: Callback<String>,
    #[prop(into)] lookup_error: Signal<Option<String>>,
    #[prop(into)] loading: Signal<bool>,
) -> impl IntoView {
    let is_open = RwSignal::new(load_storage_bool(REVISIONS_TREE_STORAGE_KEY, true));
    let show_search = RwSignal::new(false);
    let search_input = RwSignal::new(String::new());

    let toggle_open = move |_| {
        let next = !is_open.get_untracked();
        is_open.set(next);
        save_storage_bool(REVISIONS_TREE_STORAGE_KEY, next);
    };

    let selected_change = Signal::derive(move || {
        let sel = selected_change_id.get();
        changes.get().into_iter().find(|c| c.change_id == sel)
    });

    let on_submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        let query = search_input.get_untracked().trim().to_string();
        if !query.is_empty() {
            on_lookup_query.run(query);
        }
    };

    view! {
        <section class="revisions-panel" aria-label="Revisions">
            <div class="panel-header">
                <button
                    type="button"
                    class="panel-toggle-btn"
                    aria-expanded=move || is_open.get()
                    on:click=toggle_open
                >
                    <span class="chevron">{move || if is_open.get() { "▾" } else { "▸" }}</span>
                    <span class="panel-title">"Revisions"</span>
                    {move || (!is_open.get()).then(|| {
                        selected_change.get().map(|c| {
                            let review = c.review_status();
                            let status = Signal::derive(move || review);
                            view! {
                                <span class="panel-collapsed-id">
                                    <span>{c.short_change_id}</span>
                                    <ReviewDot
                                        status=status
                                        comment_count=c.comment_count
                                        unresolved_count=c.unresolved_comment_count
                                    />
                                </span>
                            }
                        })
                    }).flatten()}
                </button>
                <button
                    type="button"
                    class="icon-btn"
                    title="Jump to Change ID or Revset"
                    aria-label="Search revision"
                    on:click=move |_| {
                        if !is_open.get_untracked() {
                            is_open.set(true);
                            save_storage_bool(REVISIONS_TREE_STORAGE_KEY, true);
                        }
                        show_search.update(|v| *v = !*v);
                    }
                >
                    "⌕"
                </button>
            </div>

            {move || (is_open.get() && show_search.get()).then(|| view! {
                <div class="rev-lookup-bar">
                    <form class="rev-lookup-form" on:submit=on_submit>
                        <input
                            type="text"
                            class="rev-lookup-input"
                            placeholder="e.g. kkmpptxz or @-"
                            prop:value=move || search_input.get()
                            on:input=move |ev| {
                                if let Some(target) = ev.target()
                                    && let Ok(input) = target.dyn_into::<HtmlInputElement>()
                                {
                                    search_input.set(input.value());
                                }
                            }
                        />
                        <button type="submit" class="btn btn--accent-sm">"Go"</button>
                        <button
                            type="button"
                            class="icon-btn"
                            title="Cancel"
                            aria-label="Cancel search"
                            on:click=move |_| show_search.set(false)
                        >
                            "×"
                        </button>
                    </form>
                    {move || lookup_error.get().map(|err| view! {
                        <div class="inline-error">{err}</div>
                    })}
                </div>
            })}

            {move || is_open.get().then(|| view! {
                <JjRevisionTree
                    graph_rows=graph_rows
                    selected_change_id=selected_change_id
                    on_select_change=on_select_change
                    loading=loading
                    list_class="jj-tree-list jj-tree-list--sidebar"
                />
            })}
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use alloc::vec;

    fn sample_change(id: &str) -> Box<JjChange> {
        Box::new(JjChange {
            change_id: String::from(id),
            short_change_id: id.chars().take(8).collect(),
            shortest_change_id: id.chars().take(1).collect(),
            commit_id: alloc::format!("commit-{id}"),
            short_commit_id: String::from("commit"),
            is_working_copy: false,
            is_empty: false,
            has_description: true,
            is_immutable: false,
            is_conflict: false,
            bookmarks: Vec::new(),
            parent_commit_ids: Vec::new(),
            author: String::from("dev"),
            timestamp: String::from("2026-11-17 10:00:00"),
            description: String::from("change"),
            comment_count: 0,
            unresolved_comment_count: 0,
            approved: false,
            clean_rebase_approved: false,
        })
    }

    #[test]
    fn compute_graph_layout_connects_nodes_branches_and_elided_rows() {
        let rows = vec![
            JjGraphRow::Commit {
                key: String::from("c1"),
                graph_cols: vec!['@'],
                down_cols: vec![0],
                change: sample_change("aaaa1111"),
            },
            JjGraphRow::Connector {
                key: String::from("conn-1"),
                graph_cols: vec!['├', '─', '╮'],
                is_elided: false,
                bridges_elided: false,
            },
            JjGraphRow::Commit {
                key: String::from("c2"),
                graph_cols: vec!['│', ' ', '○'],
                down_cols: vec![0, 2],
                change: sample_change("bbbb2222"),
            },
            JjGraphRow::Connector {
                key: String::from("conn-2"),
                graph_cols: vec!['~'],
                is_elided: true,
                bridges_elided: true,
            },
            JjGraphRow::Commit {
                key: String::from("c3"),
                graph_cols: vec!['◆'],
                down_cols: Vec::new(),
                change: sample_change("cccc3333"),
            },
        ];

        let layout = compute_graph_layout(&rows);
        assert_eq!(layout.len(), 5);
        assert_eq!(layout[0].cols[0].node_kind, Some(GraphNodeKind::Working));
        assert!(layout[0].cols[0].down);

        // Branch connector row: tee + horizontal + corner
        assert!(layout[1].cols[0].up && layout[1].cols[0].down && layout[1].cols[0].right);
        assert!(layout[1].cols[1].left && layout[1].cols[1].right);
        assert!(layout[1].cols[2].down && layout[1].cols[2].left);

        // Second commit receives vertical stem at column 2 from the corner above
        assert!(layout[2].cols[2].up);
        assert_eq!(layout[2].cols[2].node_kind, Some(GraphNodeKind::Mutable));

        // Elided connector bridges vertically onto the immutable node below
        assert!(layout[3].cols[0].is_elided);
        assert!(layout[3].cols[0].up);
        assert!(layout[3].cols[0].down);
        assert!(layout[3].cols[0].dashed_down);

        assert_eq!(layout[4].cols[0].node_kind, Some(GraphNodeKind::Immutable));
    }
}
