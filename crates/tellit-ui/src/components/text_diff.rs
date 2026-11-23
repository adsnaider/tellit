//! Unified and split text diff viewer with context expansion, word highlighting, and inline review threads.

extern crate alloc;

pub(crate) mod expanded;
pub(crate) mod split;
pub(crate) mod word_highlight;

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use leptos::task::spawn_local;
use tellit_types::{
    CommentSide, DiffFile, DiffLine, DiffLineKind, FileStatus, JjCommentThread, LineRangeSpec,
};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use crate::components::comments::{
    CommentComposer, CommentReplyEvent, CommentSubmitEvent, CommentThreadCard,
};
use crate::components::file_list::status_badge_class;
use crate::components::image_diff::ImageDiffBody;
use crate::components::is_plain_primary_click;
use crate::components::markdown_diff::scroll_anchor::{
    TabScrollAnchor, clear_chunk_content_visibility_overrides, finish_code_scroll_restore,
    required_window_limit_for_line, restore_tab_scroll_anchor, switch_markdown_tab,
};
use crate::components::markdown_diff::{MarkdownPreviewBody, MarkdownTab};
use crate::state::{self, AutoCollapseReason, DiffViewMode};

use expanded::{
    DEFAULT_EXPAND_COUNT, ExpandDirection, ExpandedLinesMap, FileExpandedState, MergedChunk,
    merge_chunks_for_file,
};
use split::{SideBySideRow, pair_side_by_side_rows};
use word_highlight::render_code_line;

const COMMIT_MESSAGE_PATH: &str = "Commit Message";

/// Total merged line count above which an expanded file renders progressively in chunks.
pub const LARGE_FILE_WINDOW_THRESHOLD: usize = 500;
/// Initial line budget and incremental step size when progressively rendering a large file.
pub const CHUNK_WINDOW_STEP_LINES: usize = 150;

/// Progressive rendering budget across merged chunks of a large file diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkWindowPlan {
    /// Maximum number of rows/lines to render from each merged chunk (`0` skips the chunk).
    pub chunk_limits: Vec<usize>,
    /// Total renderable rows/lines across all chunks in the file.
    pub total_lines: usize,
    /// Number of rows/lines rendered in the current window.
    pub shown_lines: usize,
    /// Remaining unrendered rows/lines deferred behind `"Show more"` / `"Show all"`.
    pub remaining_lines: usize,
}

/// Computes per-chunk render limits for `chunk_row_counts` under `visible_limit`, gating
/// progressive windowing on `total_merged_lines > LARGE_FILE_WINDOW_THRESHOLD`.
///
/// Split view pairs delete and add runs into fewer rows than source lines, so gating on
/// `total_merged_lines` avoids bypassing progressive rendering on large replacement hunks.
#[must_use]
pub fn compute_chunk_window(
    chunk_row_counts: &[usize],
    total_merged_lines: usize,
    visible_limit: usize,
) -> ChunkWindowPlan {
    let total_rows: usize = chunk_row_counts.iter().copied().sum();
    if total_merged_lines <= LARGE_FILE_WINDOW_THRESHOLD || visible_limit >= total_rows {
        return ChunkWindowPlan {
            chunk_limits: chunk_row_counts.to_vec(),
            total_lines: total_rows,
            shown_lines: total_rows,
            remaining_lines: 0,
        };
    }

    let mut budget = visible_limit;
    let mut chunk_limits = Vec::with_capacity(chunk_row_counts.len());
    let mut shown_lines = 0usize;
    for &count in chunk_row_counts {
        let take = count.min(budget);
        chunk_limits.push(take);
        budget = budget.saturating_sub(take);
        shown_lines = shown_lines.saturating_add(take);
    }

    ChunkWindowPlan {
        chunk_limits,
        total_lines: total_rows,
        shown_lines,
        remaining_lines: total_rows.saturating_sub(shown_lines),
    }
}

/// Event emitted when the user submits a new inline or file-level review comment.
#[derive(Clone)]
pub struct CreateThreadEvent {
    /// Repository-relative file path (or `"Commit Message"`).
    pub file_path: String,
    /// Diff side (`Old` or `New`) where the line range was selected.
    pub side: CommentSide,
    /// Inclusive 1-based line range, or `None` for whole-file comments.
    pub line_range: Option<LineRangeSpec>,
    /// Comment Markdown content.
    pub body: String,
    /// Completion callback passed back from [`CommentComposer`].
    pub on_done: Callback<Result<(), String>>,
}

/// Request emitted to expand hidden context lines around an original hunk index.
#[derive(Clone)]
pub struct ExpandLinesEvent {
    /// Diff file descriptor needing context lines.
    pub file: Arc<DiffFile>,
    /// Original index inside `file.chunks`.
    pub chunk_index: usize,
    /// Direction (`Up` or `Down`), or `None` when expanding an entire gap before `chunk_index`.
    pub direction: Option<ExpandDirection>,
    /// Number of lines to reveal (or full hidden gap when `direction` is `None`).
    pub count: u32,
}

/// Active line-range drag state for one diff card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineSelectionDrag {
    /// Diff side (`Old` or `New`) anchored by the drag start.
    pub side: CommentSide,
    /// 1-based line number where dragging began.
    pub anchor_line: u32,
    /// 1-based line number currently under the pointer.
    pub current_line: u32,
}

/// Active inline comment composer anchored to a line range inside one diff card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveComposer {
    /// Diff side (`Old` or `New`) where the comment attaches.
    pub side: CommentSide,
    /// Inclusive 1-based line range covered by the comment.
    pub range: LineRangeSpec,
    /// Extracted source code lines shown as context inside the composer.
    pub code_snippet: String,
}

/// Parses a `data-gutter-side` DOM attribute value (`"old"` / `"new"`).
#[must_use]
pub fn parse_comment_side(raw: &str) -> Option<CommentSide> {
    match raw {
        "old" => Some(CommentSide::Old),
        "new" => Some(CommentSide::New),
        _ => None,
    }
}

/// Extracts the newline-joined source snippet for `range` on `side` across `file` and its expanded lines.
#[must_use]
pub fn extract_snippet_for_file(
    file: &Arc<DiffFile>,
    exp_state: Option<&FileExpandedState>,
    side: CommentSide,
    range: LineRangeSpec,
) -> String {
    let merged = merge_chunks_for_file(file, exp_state);
    extract_snippet_across_merged(&merged, side, range)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpandBarPosition {
    Top,
    Middle,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SplitAlign {
    Left,
    Right,
    Full,
}

/// Pre-indexed inline review threads for `O(1)` per-row placement lookup.
#[derive(Debug, Default)]
struct ThreadIndex {
    by_side_and_end_line: HashMap<(CommentSide, u32), Vec<JjCommentThread>>,
}

impl ThreadIndex {
    fn from_threads(threads: &[JjCommentThread]) -> Self {
        let mut by_side_and_end_line: HashMap<(CommentSide, u32), Vec<JjCommentThread>> =
            HashMap::new();
        for thread in threads {
            if let Some(range) = thread.line_range {
                by_side_and_end_line
                    .entry((thread.side, range.end))
                    .or_default()
                    .push(thread.clone());
            }
        }
        Self {
            by_side_and_end_line,
        }
    }

    fn is_empty(&self) -> bool {
        self.by_side_and_end_line.is_empty()
    }

    fn at(&self, side: CommentSide, end_line: u32) -> &[JjCommentThread] {
        self.by_side_and_end_line
            .get(&(side, end_line))
            .map_or(&[], Vec::as_slice)
    }

    fn unified_line_threads<'a>(&'a self, line: &DiffLine) -> Vec<&'a JjCommentThread> {
        if self.is_empty() {
            return Vec::new();
        }
        let mut matched = Vec::new();
        let mut seen_ids: HashSet<&'a str> = HashSet::new();

        let (primary_num, primary_side) = if line.kind == DiffLineKind::Delete {
            (line.old_line_number, CommentSide::Old)
        } else {
            (line.new_line_number, CommentSide::New)
        };

        if let Some(num) = primary_num {
            for thread in self.at(primary_side, num) {
                if seen_ids.insert(thread.id.as_str()) {
                    matched.push(thread);
                }
            }
        }

        if line.kind == DiffLineKind::Context
            && let Some(old_num) = line.old_line_number
        {
            for thread in self.at(CommentSide::Old, old_num) {
                if seen_ids.insert(thread.id.as_str()) {
                    matched.push(thread);
                }
            }
        }

        matched
    }
}

#[derive(Clone, Copy)]
struct ChunkRenderCtx {
    is_commit_msg: bool,
    composer_selector: StoredValue<Selector<Option<(CommentSide, u32)>>, LocalStorage>,
    active_composer: RwSignal<Option<ActiveComposer>>,
    on_create_thread: Callback<CreateThreadEvent>,
    on_reply: Callback<CommentReplyEvent>,
    on_toggle_resolve: Callback<(String, bool)>,
}

/// Navigation controls merged into [`TextDiffCard`]'s sticky header in single-file view.
#[derive(Clone, Copy)]
pub struct SingleFileHeaderProps {
    /// Target URL for returning to the change overview (`/c/<change_id>`).
    pub overview_url: Signal<String>,
    /// Human-readable file position counter such as `"3 / 330"`.
    pub position_label: Signal<String>,
    /// Path of the previous file in diff order, if any.
    pub prev_path: Signal<Option<String>>,
    /// Path of the next file in diff order, if any.
    pub next_path: Signal<Option<String>>,
    /// Navigates back to the change overview via SPA routing.
    pub on_back_to_overview: Callback<()>,
    /// Opens a sibling file path via SPA routing.
    pub on_open_file: Callback<String>,
    /// Marks the active file viewed and advances to the next unviewed file.
    pub on_viewed_and_next: Callback<String>,
}

/// Interactive diff card supporting Text, Markdown, Image views, expansion, drag selection, and threads.
#[component]
pub fn TextDiffCard(
    path: String,
    #[prop(into)] file: Signal<Arc<DiffFile>>,
    #[prop(into)] file_index: Signal<usize>,
    #[prop(into)] commits: Signal<(String, String)>,
    drag_state: RwSignal<Option<LineSelectionDrag>>,
    active_composer: RwSignal<Option<ActiveComposer>>,
    md_tab: RwSignal<MarkdownTab>,
    pending_md_scroll: RwSignal<Option<TabScrollAnchor>>,
    #[prop(into)] is_near_viewport: Signal<bool>,
    #[prop(into)] loading_chunks: Signal<bool>,
    #[prop(into)] load_error: Signal<Option<String>>,
    visible_line_limit: RwSignal<usize>,
    #[prop(into)] all_threads: Signal<Vec<JjCommentThread>>,
    #[prop(into)] reviewed_files: Signal<HashSet<String>>,
    #[prop(into)] collapsed_files: Signal<HashSet<String>>,
    #[prop(into)] diff_mode: Signal<DiffViewMode>,
    #[prop(into)] show_resolved_inline: Signal<bool>,
    #[prop(into)] expanded_lines: Signal<ExpandedLinesMap>,
    #[prop(into)] expand_loading: Signal<bool>,
    #[prop(default = true)] show_collapse_toggle: bool,
    #[prop(optional)] single_file_header: Option<SingleFileHeaderProps>,
    on_register_card: Callback<(String, web_sys::Element)>,
    on_unregister_card: Callback<web_sys::Element>,
    on_request_file_diff: Callback<String>,
    on_window_expanded: Callback<()>,
    on_toggle_viewed: Callback<String>,
    on_toggle_collapse: Callback<String>,
    on_open_in_editor: Callback<(String, usize)>,
    on_prefetch_line_count: Callback<Arc<DiffFile>>,
    on_expand_lines: Callback<ExpandLinesEvent>,
    on_create_thread: Callback<CreateThreadEvent>,
    on_reply: Callback<CommentReplyEvent>,
    on_toggle_resolve: Callback<(String, bool)>,
) -> impl IntoView {
    let is_commit_msg = path == COMMIT_MESSAGE_PATH;
    let is_md = !is_commit_msg && state::is_markdown_file(&path);
    let is_img = !is_commit_msg && state::is_image_file(&path);
    let path_for_id = state::file_element_id(&path);
    let path_attr = path.clone();
    let path_toggle_collapse = path.clone();
    let path_toggle_viewed = path.clone();
    let path_viewed_and_next = path.clone();
    let path_for_viewed = path.clone();
    let path_for_collapsed = path.clone();
    let path_for_threads = path.clone();
    let path_for_title = path.clone();
    let path_for_edit = path.clone();
    let path_for_exp = path.clone();
    let path_for_body = path.clone();
    let path_for_observe = path.clone();
    let path_for_lazy = path.clone();
    let path_for_retry = path.clone();

    let is_viewed_sig =
        Memo::new(move |_| reviewed_files.with(|set| set.contains(&path_for_viewed)));
    let is_collapsed_sig =
        Memo::new(move |_| collapsed_files.with(|set| set.contains(&path_for_collapsed)));

    let whole_file_composing = RwSignal::new(false);
    let md_full_ready = RwSignal::new(false);
    let card_ref = NodeRef::<leptos::html::Article>::new();
    let observed_el = StoredValue::new_local(Option::<web_sys::Element>::None);
    let marked_selection_els = StoredValue::new_local(Vec::<web_sys::Element>::new());
    let selection_sync_pending = StoredValue::new_local(false);
    let restore_raf_id = StoredValue::new_local(Option::<i32>::None);
    let restore_raf_cb = StoredValue::new_local(Option::<Closure<dyn FnMut()>>::None);
    let chunks_cv_overridden = StoredValue::new_local(false);
    let last_commits = StoredValue::new_local(Option::<(String, String)>::None);

    let cancel_pending_restore_raf = move || {
        let mut prev_id = None;
        let _ = restore_raf_id.try_update_value(|slot| {
            prev_id = slot.take();
        });
        if let Some(id) = prev_id
            && let Some(window) = web_sys::window()
        {
            let _ = window.cancel_animation_frame(id);
        }
        let _ = restore_raf_cb.try_set_value(None);
        if chunks_cv_overridden.try_get_value().unwrap_or(false) {
            let _ = chunks_cv_overridden.try_set_value(false);
            if let Some(article) = card_ref.try_get_untracked().flatten() {
                let el: &web_sys::Element = article.as_ref();
                if el.is_connected() {
                    clear_chunk_content_visibility_overrides(el);
                }
            }
        }
    };

    Effect::new(move |_| {
        if let Some(article) = card_ref.get()
            && observed_el.with_value(Option::is_none)
        {
            let el: web_sys::Element = article.into();
            observed_el.set_value(Some(el.clone()));
            on_register_card.run((path_for_observe.clone(), el));
        }
    });
    on_cleanup(move || {
        cancel_pending_restore_raf();
        let _ = pending_md_scroll.try_set(None);
        let mut prev = None;
        let _ = observed_el.try_update_value(|slot| {
            prev = slot.take();
        });
        if let Some(el) = prev {
            on_unregister_card.run(el);
        }
    });

    // Hydrate summary-only files lazily once the card is both uncollapsed and near the viewport.
    Effect::new(move |_| {
        if is_collapsed_sig.get() || !is_near_viewport.get() {
            return;
        }
        let needs_load = file.with(|f| !f.chunks_loaded);
        if needs_load && !loading_chunks.get() && load_error.with(Option::is_none) {
            on_request_file_diff.run(path_for_lazy.clone());
        }
    });

    let visible_threads = Memo::new(move |_| {
        let show_resolved = show_resolved_inline.get();
        all_threads.with(|list| {
            list.iter()
                .filter(|t| t.file_path == path_for_threads && (show_resolved || !t.resolved))
                .cloned()
                .collect::<Vec<_>>()
        })
    });
    let total_thread_count = Memo::new(move |_| visible_threads.with(Vec::len));

    let file_exp_memo =
        Memo::new(move |_| expanded_lines.with(|map| map.get(&path_for_exp).cloned()));
    let merged_chunks_memo = Memo::new(move |_| {
        Arc::new(file.with(|f| file_exp_memo.with(|exp| merge_chunks_for_file(f, exp.as_ref()))))
    });

    let composer_selector = StoredValue::new_local(Selector::new(move || {
        active_composer.with(|c| c.as_ref().map(|c| (c.side, c.range.end)))
    }));

    // Defer `/api/line-count` until the card is uncollapsed, near the viewport, and hydrated.
    Effect::new(move |_| {
        if is_collapsed_sig.get() || !is_near_viewport.get() {
            return;
        }
        let can_expand = file.with(|f| {
            !is_commit_msg
                && !is_img
                && f.chunks_loaded
                && f.status != FileStatus::Added
                && f.status != FileStatus::Deleted
                && !f.chunks.is_empty()
        });
        let has_line_counts = file_exp_memo.with(|exp| {
            exp.as_ref()
                .is_some_and(|e| e.old_total_lines.is_some() || e.new_total_lines.is_some())
        });
        if can_expand && !has_line_counts {
            on_prefetch_line_count.run(file.get_untracked());
        }
    });

    // Apply drag/composer highlight classes directly to matching rows/cells on this card,
    // avoiding per-row reactive closures across thousands of diff lines.
    Effect::new(move |_| {
        let drag = drag_state.get();
        let comp = active_composer.get();

        if drag.is_none() && comp.is_none() {
            marked_selection_els.update_value(|els| {
                for el in els.drain(..) {
                    let list = el.class_list();
                    let _ = list.remove_2("line-row--drag-selected", "line-row--comment-selected");
                }
            });
            return;
        }

        // Only subscribe to layout/table re-renders while a drag or composer is active on this card.
        if is_collapsed_sig.get() {
            marked_selection_els.update_value(|els| {
                for el in els.drain(..) {
                    let list = el.class_list();
                    let _ = list.remove_2("line-row--drag-selected", "line-row--comment-selected");
                }
            });
            return;
        }
        diff_mode.track();
        merged_chunks_memo.track();
        visible_line_limit.track();

        if selection_sync_pending.get_value() {
            return;
        }
        selection_sync_pending.set_value(true);
        spawn_local(async move {
            let _ = JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED)).await;
            let _ = selection_sync_pending.try_set_value(false);

            let _ = marked_selection_els.try_update_value(|els| {
                for el in els.drain(..) {
                    let list = el.class_list();
                    let _ = list.remove_2("line-row--drag-selected", "line-row--comment-selected");
                }
            });

            let drag_now = drag_state.try_get_untracked().flatten();
            let comp_now = active_composer.try_get_untracked().flatten();
            if drag_now.is_none() && comp_now.is_none() {
                return;
            }
            if is_collapsed_sig.try_get_untracked().unwrap_or(true) {
                return;
            }
            let Some(mode_now) = diff_mode.try_get_untracked() else {
                return;
            };
            let Some(article) = card_ref.get_untracked() else {
                return;
            };
            let article_el: &web_sys::Element = article.as_ref();
            let mut newly_marked = Vec::new();
            apply_selection_classes_to_card(
                article_el,
                mode_now,
                drag_now,
                comp_now.as_ref(),
                &mut newly_marked,
            );
            let _ = marked_selection_els.try_set_value(newly_marked);
        });
    });

    let chunk_ctx = ChunkRenderCtx {
        is_commit_msg,
        composer_selector,
        active_composer,
        on_create_thread,
        on_reply,
        on_toggle_resolve,
    };

    let show_code = Memo::new(move |_| !is_md || md_tab.get() == MarkdownTab::Code);

    // Clear stale Markdown tab scroll anchors whenever the compared commits change.
    Effect::new(move |_| {
        let cur_commits = commits.get();
        let changed = last_commits.with_value(|prev| {
            prev.as_ref()
                .is_some_and(|prev_pair| *prev_pair != cur_commits)
        });
        last_commits.set_value(Some(cur_commits));
        if changed {
            cancel_pending_restore_raf();
            let _ = pending_md_scroll.try_set(None);
        }
    });

    // Restore viewport scroll position once the destination Markdown tab mounts and lays out.
    Effect::new(move |_| {
        if !is_md {
            return;
        }
        let Some(anchor) = pending_md_scroll.get() else {
            return;
        };
        let current_tab = md_tab.get();
        if current_tab != anchor.target_tab || is_collapsed_sig.get() {
            cancel_pending_restore_raf();
            pending_md_scroll.set(None);
            return;
        }

        match current_tab {
            MarkdownTab::Code => {
                let chunks_loaded = file.with(|f| f.chunks_loaded);
                if !chunks_loaded {
                    if load_error.with(Option::is_some) {
                        pending_md_scroll.set(None);
                    }
                    return;
                }
                let merged_chunks = merged_chunks_memo.get();
                let mode = diff_mode.get();
                if let Some(target_line) = anchor.source_line {
                    let current_limit = visible_line_limit.get();
                    if let Some(next_limit) = required_window_limit_for_line(
                        &merged_chunks,
                        mode,
                        current_limit,
                        target_line,
                    ) {
                        visible_line_limit.set(next_limit);
                        on_window_expanded.run(());
                        return;
                    }
                }
            }
            MarkdownTab::Full => {
                if !is_near_viewport.get() || !md_full_ready.get() {
                    return;
                }
            }
            MarkdownTab::Preview => {
                if !is_near_viewport.get() {
                    return;
                }
                if !file.with(|f| f.chunks_loaded) {
                    if load_error.with(Option::is_some) {
                        pending_md_scroll.set(None);
                    }
                    return;
                }
            }
        }

        pending_md_scroll.set(None);
        cancel_pending_restore_raf();
        let Some(window) = web_sys::window() else {
            return;
        };
        let cb = Closure::<dyn FnMut()>::new(move || {
            let _ = restore_raf_id.try_set_value(None);
            let _current_cb = restore_raf_cb.try_update_value(Option::take);
            if let Some(article) = card_ref.try_get_untracked().flatten() {
                let el: &web_sys::Element = article.as_ref();
                if el.is_connected()
                    && let Some(follow_up) = restore_tab_scroll_anchor(el, anchor)
                {
                    let _ = chunks_cv_overridden.try_set_value(true);
                    if let Some(win) = web_sys::window() {
                        let finish_cb = Closure::<dyn FnMut()>::new(move || {
                            let _ = restore_raf_id.try_set_value(None);
                            let _finish_self = restore_raf_cb.try_update_value(Option::take);
                            let _ = chunks_cv_overridden.try_set_value(false);
                            if let Some(art) = card_ref.try_get_untracked().flatten() {
                                let card: &web_sys::Element = art.as_ref();
                                if card.is_connected() {
                                    finish_code_scroll_restore(card, &follow_up);
                                }
                            }
                        });
                        if let Ok(follow_id) =
                            win.request_animation_frame(finish_cb.as_ref().unchecked_ref())
                        {
                            let _ = restore_raf_id.try_set_value(Some(follow_id));
                            let _ = restore_raf_cb.try_set_value(Some(finish_cb));
                        } else {
                            let _ = chunks_cv_overridden.try_set_value(false);
                            clear_chunk_content_visibility_overrides(el);
                        }
                    } else {
                        let _ = chunks_cv_overridden.try_set_value(false);
                        clear_chunk_content_visibility_overrides(el);
                    }
                }
            }
        });
        if let Ok(id) = window.request_animation_frame(cb.as_ref().unchecked_ref()) {
            restore_raf_id.set_value(Some(id));
            restore_raf_cb.set_value(Some(cb));
        }
    });

    let on_select_md_tab = move |next_tab: MarkdownTab| {
        let article = card_ref.get_untracked();
        let card_el: Option<&web_sys::Element> = article.as_deref().map(AsRef::as_ref);
        switch_markdown_tab(card_el, md_tab, pending_md_scroll, next_tab);
    };
    let display_path = Memo::new(move |_| {
        file.with(|f| match f.old_path {
            Some(ref old) => alloc::format!("{old} -> {path_for_title}"),
            None => path_for_title.clone(),
        })
    });
    let is_single_file = single_file_header.is_some();

    view! {
        <article
            node_ref=card_ref
            id=path_for_id
            data-file=path_attr
            data-file-idx=move || file_index.get().to_string()
            data-card-columns=move || if is_md && !show_code.get() { "1" } else { "auto" }
            class=move || {
                match (is_single_file, is_viewed_sig.get()) {
                    (true, true) => "diff-card diff-card--single diff-card--viewed",
                    (true, false) => "diff-card diff-card--single",
                    (false, true) => "diff-card diff-card--viewed",
                    (false, false) => "diff-card",
                }
            }
        >
            <header class={if is_single_file {
                "diff-card__header diff-card__header--single"
            } else {
                "diff-card__header"
            }}>
                <div class="diff-card__left">
                    {if let Some(nav) = single_file_header {
                        view! {
                            <a
                                class="btn btn--ghost btn--xs overview-action-link single-file-bar__back"
                                href=move || nav.overview_url.get()
                                title="Return to change overview (u)"
                                on:click=move |ev: web_sys::MouseEvent| {
                                    if is_plain_primary_click(&ev) {
                                        ev.prevent_default();
                                        nav.on_back_to_overview.run(());
                                    }
                                }
                            >
                                "← Overview"
                            </a>
                        }
                        .into_any()
                    } else if show_collapse_toggle {
                        view! {
                            <button
                                type="button"
                                class="icon-btn"
                                title=move || if is_collapsed_sig.get() { "Expand file" } else { "Collapse file" }
                                aria-expanded=move || !is_collapsed_sig.get()
                                on:click=move |_| on_toggle_collapse.run(path_toggle_collapse.clone())
                            >
                                {move || if is_collapsed_sig.get() { "▸" } else { "▾" }}
                            </button>
                        }
                        .into_any()
                    } else {
                        ().into_any()
                    }}
                    {if is_commit_msg {
                        view! { <span class="file-status file-status--commit">"CM"</span> }.into_any()
                    } else {
                        view! {
                            <span class=move || file.with(|f| status_badge_class(f.status))>
                                {move || file.with(|f| f.status.short_code())}
                            </span>
                        }
                        .into_any()
                    }}
                    <span class="diff-card__path" title=move || display_path.get()>
                        {move || display_path.get()}
                    </span>
                    {is_single_file.then(|| view! {
                        <span class="diff-totals">
                            <span class="stat-add">{move || alloc::format!("+{}", file.with(|f| f.additions))}</span>
                            <span class="stat-del">{move || alloc::format!("-{}", file.with(|f| f.deletions))}</span>
                        </span>
                    })}
                    {move || file.with(|f| state::auto_collapse_reason(f)).map(|reason| {
                        let cls = if reason == AutoCollapseReason::Deleted {
                            "pill-tag pill-tag--del"
                        } else {
                            "pill-tag"
                        };
                        view! { <span class=cls>{reason.badge_label()}</span> }
                    })}
                    {move || {
                        let count = total_thread_count.get();
                        (count > 0).then(|| view! {
                            <span class="pill-tag pill-tag--warn">
                                {alloc::format!("{count} thread{}", if count == 1 { "" } else { "s" })}
                            </span>
                        })
                    }}
                    {single_file_header.map(|nav| view! {
                        <span class="single-file-bar__counter">
                            {move || nav.position_label.get()}
                        </span>
                    })}
                </div>

                <div class="diff-card__right">
                    {is_md.then(|| view! {
                        <div class="segmented" role="group" aria-label="Markdown view mode">
                            <button
                                type="button"
                                class=move || if md_tab.get() == MarkdownTab::Preview { "segmented__btn segmented__btn--active" } else { "segmented__btn" }
                                on:click=move |_| on_select_md_tab(MarkdownTab::Preview)
                            >
                                "Preview"
                            </button>
                            <button
                                type="button"
                                class=move || if md_tab.get() == MarkdownTab::Full { "segmented__btn segmented__btn--active" } else { "segmented__btn" }
                                on:click=move |_| on_select_md_tab(MarkdownTab::Full)
                            >
                                "Full"
                            </button>
                            <button
                                type="button"
                                class=move || if md_tab.get() == MarkdownTab::Code { "segmented__btn segmented__btn--active" } else { "segmented__btn" }
                                on:click=move |_| on_select_md_tab(MarkdownTab::Code)
                            >
                                "Code"
                            </button>
                        </div>
                    })}
                    <button
                        type="button"
                        class="btn btn--ghost btn--xs"
                        title={if is_commit_msg {
                            "Add a comment on commit message"
                        } else {
                            "Add a file-level comment"
                        }}
                        on:click=move |_| whole_file_composing.update(|v| *v = !*v)
                    >
                        "+ Comment"
                    </button>

                    {move || (!is_commit_msg && file.with(|f| f.status != FileStatus::Deleted)).then(|| {
                        let edit_path = path_for_edit.clone();
                        view! {
                            <button
                                type="button"
                                class="btn btn--ghost btn--xs"
                                title="Open file in external editor"
                                on:click=move |_| on_open_in_editor.run((edit_path.clone(), 1))
                            >
                                "Open ↗"
                            </button>
                        }
                    })}

                    {(!is_single_file).then(|| view! {
                        <span class="diff-totals">
                            <span class="stat-add">{move || alloc::format!("+{}", file.with(|f| f.additions))}</span>
                            <span class="stat-del">{move || alloc::format!("-{}", file.with(|f| f.deletions))}</span>
                        </span>
                    })}

                    {single_file_header.map(|nav| view! {
                        <>
                            <button
                                type="button"
                                class="btn btn--ghost btn--xs"
                                title="Previous file ([)"
                                disabled=move || nav.prev_path.with(Option::is_none)
                                on:click=move |_| {
                                    if let Some(prev) = nav.prev_path.get_untracked() {
                                        nav.on_open_file.run(prev);
                                    }
                                }
                            >
                                "← Prev"
                            </button>
                            <button
                                type="button"
                                class="btn btn--ghost btn--xs"
                                title="Next file (])"
                                disabled=move || nav.next_path.with(Option::is_none)
                                on:click=move |_| {
                                    if let Some(next) = nav.next_path.get_untracked() {
                                        nav.on_open_file.run(next);
                                    }
                                }
                            >
                                "Next →"
                            </button>
                        </>
                    })}

                    <label class="viewed-toggle" title="Toggle viewed (v)">
                        <input
                            type="checkbox"
                            class="file-checkbox"
                            prop:checked=move || is_viewed_sig.get()
                            on:change=move |_| on_toggle_viewed.run(path_toggle_viewed.clone())
                        />
                        <span>"Viewed"</span>
                    </label>

                    {single_file_header.map(|nav| {
                        let next_path = path_viewed_and_next.clone();
                        view! {
                            <button
                                type="button"
                                class="btn btn--primary btn--xs"
                                title="Mark viewed and go to next unviewed file (Shift+V)"
                                on:click=move |_| nav.on_viewed_and_next.run(next_path.clone())
                            >
                                "Viewed & Next"
                            </button>
                        }
                    })}
                </div>
            </header>

            {move || if is_collapsed_sig.get() {
                let (hunks_count, chunks_loaded, additions, deletions, reason) = file.with(|f| {
                    (
                        f.chunks.len(),
                        f.chunks_loaded,
                        f.additions,
                        f.deletions,
                        state::auto_collapse_reason(f),
                    )
                });
                let has_summary = hunks_count > 0 || !chunks_loaded || additions > 0 || deletions > 0;
                has_summary.then(|| {
                    let prefix = reason.map_or("Collapsed", AutoCollapseReason::collapsed_prefix);
                    let detail = if chunks_loaded && hunks_count > 0 {
                        alloc::format!("{prefix} ({hunks_count} hunks)")
                    } else {
                        alloc::format!("{prefix} (+{additions} -{deletions}). Click to expand.")
                    };
                    view! { <div class="diff-card__collapsed-note">{detail}</div> }
                }).into_any()
            } else {
                let path_for_whole_submit = path_for_body.clone();
                let path_for_line_submit = path_for_body.clone();
                let path_for_code_retry = path_for_retry.clone();

                view! {
                    <div class="diff-card__body diff-card__body--flush">
                        {move || whole_file_composing.get().then(|| {
                            let submit_path = path_for_whole_submit.clone();
                            let title = if submit_path == COMMIT_MESSAGE_PATH {
                                String::from("Comment on Commit Message")
                            } else {
                                alloc::format!("Comment on {submit_path}")
                            };
                            view! {
                                <div class="whole-file-composer">
                                    <CommentComposer
                                        title=title
                                        on_submit=Callback::new(move |evt: CommentSubmitEvent| {
                                            let on_done = evt.on_done;
                                            let wrapped = Callback::new(move |res: Result<(), String>| {
                                                if res.is_ok() {
                                                    let _ = whole_file_composing.try_set(false);
                                                }
                                                on_done.run(res);
                                            });
                                            on_create_thread.run(CreateThreadEvent {
                                                file_path: submit_path.clone(),
                                                side: CommentSide::New,
                                                line_range: None,
                                                body: evt.body,
                                                on_done: wrapped,
                                            });
                                        })
                                        on_cancel=Callback::new(move |()| whole_file_composing.set(false))
                                    />
                                </div>
                            }
                        })}

                        {if is_img {
                            view! {
                                <>
                                    {move || render_file_threads_section(visible_threads.get(), on_reply, on_toggle_resolve)}
                                    {move || {
                                        if !is_near_viewport.get() {
                                            return view! {
                                                <div class="diff-card__summary">"Loading image diff..."</div>
                                            }.into_any();
                                        }
                                        let file_arc = file.get();
                                        let (base_c, target_c) = commits.get();
                                        view! {
                                            <ImageDiffBody
                                                file=file_arc
                                                base_commit=base_c
                                                target_commit=target_c
                                            />
                                        }.into_any()
                                    }}
                                </>
                            }.into_any()
                        } else {
                            let md_meta = Memo::new(move |_| {
                                file.with(|f| (f.status, f.old_path.clone()))
                            });
                            let md_preview_panel = is_md.then(|| {
                                view! {
                                    <div style:display=move || show_code.get().then_some("none")>
                                        {move || (!show_code.get()).then(|| {
                                            render_file_threads_section(visible_threads.get(), on_reply, on_toggle_resolve)
                                        })}
                                        {move || {
                                            if !is_near_viewport.get() {
                                                return view! {
                                                    <div class="diff-card__summary">"Loading Markdown preview..."</div>
                                                }.into_any();
                                            }
                                            // Only re-mount when commit IDs or file identity/status change,
                                            // not when diff chunks finish hydrating (which merged_chunks_memo handles).
                                            md_meta.track();
                                            let (md_base, md_target) = commits.get();
                                            let md_file = file.get_untracked();
                                            view! {
                                                <MarkdownPreviewBody
                                                    file=md_file
                                                    merged_chunks=merged_chunks_memo
                                                    base_commit=md_base
                                                    target_commit=md_target
                                                    active_tab=md_tab
                                                    full_ready=md_full_ready
                                                />
                                            }.into_any()
                                        }}
                                    </div>
                                }
                            });

                            view! {
                                <>
                                    {md_preview_panel}
                                    {move || {
                                        if !show_code.get() {
                                            composer_selector.with_value(Selector::clear);
                                            return ().into_any();
                                        }

                                        let chunks_loaded = file.with(|f| f.chunks_loaded);
                                        if !chunks_loaded {
                                            composer_selector.with_value(Selector::clear);
                                            if let Some(err_msg) = load_error.get() {
                                                let retry_path = path_for_code_retry.clone();
                                                return view! {
                                                    <div class="diff-load-error">
                                                        <span>{alloc::format!("Failed to load diff: {err_msg}")}</span>
                                                        <button
                                                            type="button"
                                                            class="btn btn--ghost btn--xs"
                                                            on:click=move |_| on_request_file_diff.run(retry_path.clone())
                                                        >
                                                            "Retry"
                                                        </button>
                                                    </div>
                                                }.into_any();
                                            }
                                            return view! {
                                                <div class="diff-card__summary">"Loading diff..."</div>
                                            }.into_any();
                                        }

                                        let merged_chunks = merged_chunks_memo.get();
                                        let mode = diff_mode.get();
                                        let file_snapshot = file.get_untracked();
                                        // Evict stale subscriber entries from prior mode or chunk renders.
                                        composer_selector.with_value(Selector::clear);

                                        let total_merged_lines: usize =
                                            merged_chunks.iter().map(|mc| mc.lines.len()).sum();
                                        let limit = if total_merged_lines > LARGE_FILE_WINDOW_THRESHOLD {
                                            visible_line_limit.get()
                                        } else {
                                            usize::MAX
                                        };
                                        // In split mode, pair full Delete/Add runs before slicing
                                        // to the window budget so side-by-side alignment stays
                                        // intact when the user reveals additional rows.
                                        let split_chunks: Vec<Vec<SideBySideRow>> =
                                            if mode == DiffViewMode::Split {
                                                merged_chunks
                                                    .iter()
                                                    .map(pair_side_by_side_rows)
                                                    .collect()
                                            } else {
                                                Vec::new()
                                            };
                                        let chunk_counts: Vec<usize> =
                                            if mode == DiffViewMode::Split {
                                                split_chunks.iter().map(Vec::len).collect()
                                            } else {
                                                merged_chunks
                                                    .iter()
                                                    .map(|mc| mc.lines.len())
                                                    .collect()
                                            };
                                        let window_plan = compute_chunk_window(
                                            &chunk_counts,
                                            total_merged_lines,
                                            limit,
                                        );

                                        let (thread_index, unplaced) = visible_threads.with(|active_threads| {
                                            let idx = ThreadIndex::from_threads(active_threads);
                                            if active_threads.is_empty() {
                                                return (idx, Vec::new());
                                            }
                                            let mut placed_ids = HashSet::new();
                                            if !idx.is_empty() {
                                                for (merged_idx, (mc, &chunk_limit)) in merged_chunks
                                                    .iter()
                                                    .zip(window_plan.chunk_limits.iter())
                                                    .enumerate()
                                                {
                                                    if chunk_limit == 0 {
                                                        continue;
                                                    }
                                                    if mode == DiffViewMode::Split {
                                                        let visible_rows = &split_chunks[merged_idx]
                                                            [..chunk_limit.min(
                                                                split_chunks[merged_idx].len(),
                                                            )];
                                                        collect_placed_split_thread_ids(
                                                            mc,
                                                            visible_rows,
                                                            &idx,
                                                            &mut placed_ids,
                                                        );
                                                    } else {
                                                        collect_placed_unified_thread_ids(
                                                            mc,
                                                            chunk_limit,
                                                            &idx,
                                                            &mut placed_ids,
                                                        );
                                                    }
                                                }
                                            }
                                            let unplaced = active_threads
                                                .iter()
                                                .filter(|t| !placed_ids.contains(t.id.as_str()))
                                                .cloned()
                                                .collect::<Vec<_>>();
                                            (idx, unplaced)
                                        });

                                        let chunk_count = merged_chunks.len();
                                        let path_clone = file_snapshot.path.clone();
                                        let remaining_lines = window_plan.remaining_lines;
                                        let shown_lines = window_plan.shown_lines;
                                        let total_lines = window_plan.total_lines;

                                        view! {
                                            <>
                                                {render_file_threads_section(unplaced, on_reply, on_toggle_resolve)}

                                                {if merged_chunks.is_empty() {
                                                    view! {
                                                        <div class="diff-card__summary">
                                                            "No text hunk changes in this entry."
                                                        </div>
                                                    }.into_any()
                                                } else {
                                                    merged_chunks
                                                        .iter()
                                                        .enumerate()
                                                        .filter_map(|(merged_idx, mc)| {
                                                            let chunk_limit = window_plan
                                                                .chunk_limits
                                                                .get(merged_idx)
                                                                .copied()
                                                                .unwrap_or(0);
                                                            if chunk_limit == 0 {
                                                                return None;
                                                            }
                                                            let first_orig = *mc.original_indices.first().unwrap_or(&0);
                                                            let last_orig = *mc.original_indices.last().unwrap_or(&0);
                                                            let is_first = merged_idx == 0;
                                                            let is_last = merged_idx + 1 == chunk_count
                                                                && remaining_lines == 0;
                                                            let hidden_before = mc.hidden_lines_before;
                                                            let hidden_after = mc.hidden_lines_after.unwrap_or(0);
                                                            let file_top = Arc::clone(&file_snapshot);
                                                            let file_mid = Arc::clone(&file_snapshot);
                                                            let file_bot = Arc::clone(&file_snapshot);

                                                            Some(view! {
                                                                <div class="diff-chunk-wrapper">
                                                                    {(is_first && hidden_before > 0).then(|| view! {
                                                                        <ExpandBar
                                                                            position=ExpandBarPosition::Top
                                                                            hidden_lines=hidden_before
                                                                            is_loading=expand_loading
                                                                            on_expand_up=Callback::new(|()| {})
                                                                            on_expand_down=Callback::new({
                                                                                let f = Arc::clone(&file_top);
                                                                                move |()| on_expand_lines.run(ExpandLinesEvent {
                                                                                    file: Arc::clone(&f),
                                                                                    chunk_index: first_orig,
                                                                                    direction: Some(ExpandDirection::Up),
                                                                                    count: DEFAULT_EXPAND_COUNT,
                                                                                })
                                                                            })
                                                                            on_expand_all=Callback::new({
                                                                                let f = Arc::clone(&file_top);
                                                                                move |()| on_expand_lines.run(ExpandLinesEvent {
                                                                                    file: Arc::clone(&f),
                                                                                    chunk_index: first_orig,
                                                                                    direction: None,
                                                                                    count: hidden_before,
                                                                                })
                                                                            })
                                                                        />
                                                                    })}

                                                                    {(!is_first && hidden_before > 0).then(|| view! {
                                                                        <ExpandBar
                                                                            position=ExpandBarPosition::Middle
                                                                            hidden_lines=hidden_before
                                                                            is_loading=expand_loading
                                                                            on_expand_up=Callback::new({
                                                                                let f = Arc::clone(&file_mid);
                                                                                move |()| on_expand_lines.run(ExpandLinesEvent {
                                                                                    file: Arc::clone(&f),
                                                                                    chunk_index: first_orig.saturating_sub(1),
                                                                                    direction: Some(ExpandDirection::Down),
                                                                                    count: DEFAULT_EXPAND_COUNT,
                                                                                })
                                                                            })
                                                                            on_expand_down=Callback::new({
                                                                                let f = Arc::clone(&file_mid);
                                                                                move |()| on_expand_lines.run(ExpandLinesEvent {
                                                                                    file: Arc::clone(&f),
                                                                                    chunk_index: first_orig,
                                                                                    direction: Some(ExpandDirection::Up),
                                                                                    count: DEFAULT_EXPAND_COUNT,
                                                                                })
                                                                            })
                                                                            on_expand_all=Callback::new({
                                                                                let f = Arc::clone(&file_mid);
                                                                                move |()| on_expand_lines.run(ExpandLinesEvent {
                                                                                    file: Arc::clone(&f),
                                                                                    chunk_index: first_orig,
                                                                                    direction: None,
                                                                                    count: hidden_before,
                                                                                })
                                                                            })
                                                                        />
                                                                    })}

                                                                    <div class="diff-chunk-header">{mc.header.clone()}</div>

                                                                    {if mode == DiffViewMode::Split {
                                                                        let visible_rows = &split_chunks[merged_idx]
                                                                            [..chunk_limit.min(
                                                                                split_chunks[merged_idx].len(),
                                                                            )];
                                                                        render_split_chunk(
                                                                            mc,
                                                                            visible_rows,
                                                                            &path_clone,
                                                                            &thread_index,
                                                                            &path_for_line_submit,
                                                                            chunk_ctx,
                                                                        ).into_any()
                                                                    } else {
                                                                        render_unified_chunk(
                                                                            mc,
                                                                            chunk_limit,
                                                                            &path_clone,
                                                                            &thread_index,
                                                                            &path_for_line_submit,
                                                                            chunk_ctx,
                                                                        ).into_any()
                                                                    }}

                                                                    {(is_last && hidden_after > 0).then(|| view! {
                                                                        <ExpandBar
                                                                            position=ExpandBarPosition::Bottom
                                                                            hidden_lines=hidden_after
                                                                            is_loading=expand_loading
                                                                            on_expand_up=Callback::new({
                                                                                let f = Arc::clone(&file_bot);
                                                                                move |()| on_expand_lines.run(ExpandLinesEvent {
                                                                                    file: Arc::clone(&f),
                                                                                    chunk_index: last_orig,
                                                                                    direction: Some(ExpandDirection::Down),
                                                                                    count: DEFAULT_EXPAND_COUNT,
                                                                                })
                                                                            })
                                                                            on_expand_down=Callback::new(|()| {})
                                                                            on_expand_all=Callback::new({
                                                                                let f = Arc::clone(&file_bot);
                                                                                move |()| on_expand_lines.run(ExpandLinesEvent {
                                                                                    file: Arc::clone(&f),
                                                                                    chunk_index: last_orig,
                                                                                    direction: Some(ExpandDirection::Down),
                                                                                    count: hidden_after,
                                                                                })
                                                                            })
                                                                        />
                                                                    })}
                                                                </div>
                                                            })
                                                        })
                                                        .collect_view()
                                                        .into_any()
                                                }}

                                                {(remaining_lines > 0).then(|| {
                                                    let next_step = remaining_lines.min(CHUNK_WINDOW_STEP_LINES);
                                                    view! {
                                                        <div class="diff-window-bar">
                                                            <span class="diff-window-bar__label">
                                                                {alloc::format!("Showing {shown_lines} of {total_lines} lines")}
                                                            </span>
                                                            <div class="diff-window-bar__actions">
                                                                <button
                                                                    type="button"
                                                                    class="btn btn--ghost btn--xs"
                                                                    on:click=move |_| {
                                                                        visible_line_limit.update(|n| {
                                                                            *n = n.saturating_add(CHUNK_WINDOW_STEP_LINES);
                                                                        });
                                                                        on_window_expanded.run(());
                                                                    }
                                                                >
                                                                    {alloc::format!("Show next {next_step} lines")}
                                                                </button>
                                                                <button
                                                                    type="button"
                                                                    class="btn btn--ghost btn--xs"
                                                                    on:click=move |_| {
                                                                        visible_line_limit.set(usize::MAX);
                                                                        on_window_expanded.run(());
                                                                    }
                                                                >
                                                                    {alloc::format!("Show all ({remaining_lines} remaining)")}
                                                                </button>
                                                            </div>
                                                        </div>
                                                    }
                                                })}
                                            </>
                                        }.into_any()
                                    }}
                                </>
                            }.into_any()
                        }}
                    </div>
                }.into_any()
            }}
        </article>
    }
}

fn apply_selection_classes_to_card(
    article_el: &web_sys::Element,
    mode: DiffViewMode,
    drag: Option<LineSelectionDrag>,
    composer: Option<&ActiveComposer>,
    marked: &mut Vec<web_sys::Element>,
) {
    match mode {
        DiffViewMode::Unified => {
            let Ok(rows) = article_el.query_selector_all("tr[data-old-line], tr[data-new-line]")
            else {
                return;
            };
            for idx in 0..rows.length() {
                let Some(node) = rows.item(idx) else {
                    continue;
                };
                let Ok(el) = node.dyn_into::<web_sys::Element>() else {
                    continue;
                };
                let old_num = el
                    .get_attribute("data-old-line")
                    .and_then(|s| s.parse::<u32>().ok());
                let new_num = el
                    .get_attribute("data-new-line")
                    .and_then(|s| s.parse::<u32>().ok());

                let sel_new = selection_class(drag, composer, CommentSide::New, new_num);
                let sel = if sel_new.is_empty() {
                    selection_class(drag, composer, CommentSide::Old, old_num)
                } else {
                    sel_new
                };
                if !sel.is_empty() {
                    let _ = el.class_list().add_1(sel);
                    marked.push(el);
                }
            }
        }
        DiffViewMode::Split => {
            let Ok(cells) = article_el.query_selector_all("td[data-split-side][data-split-line]")
            else {
                return;
            };
            for idx in 0..cells.length() {
                let Some(node) = cells.item(idx) else {
                    continue;
                };
                let Ok(el) = node.dyn_into::<web_sys::Element>() else {
                    continue;
                };
                let Some(side) = el
                    .get_attribute("data-split-side")
                    .as_deref()
                    .and_then(parse_comment_side)
                else {
                    continue;
                };
                let line_num = el
                    .get_attribute("data-split-line")
                    .and_then(|s| s.parse::<u32>().ok());
                let sel = selection_class(drag, composer, side, line_num);
                if !sel.is_empty() {
                    let _ = el.class_list().add_1(sel);
                    marked.push(el);
                }
            }
        }
    }
}

fn render_file_threads_section(
    threads: Vec<JjCommentThread>,
    on_reply: Callback<CommentReplyEvent>,
    on_toggle_resolve: Callback<(String, bool)>,
) -> Option<AnyView> {
    if threads.is_empty() {
        return None;
    }
    Some(
        view! {
            <div class="file-threads-section">
                {threads.into_iter().map(|thread| {
                    view! {
                        <CommentThreadCard
                            thread=thread
                            on_reply=on_reply
                            on_toggle_resolve=on_toggle_resolve
                        />
                    }
                }).collect_view()}
            </div>
        }
        .into_any(),
    )
}

#[component]
fn ExpandBar(
    position: ExpandBarPosition,
    hidden_lines: u32,
    #[prop(into)] is_loading: Signal<bool>,
    on_expand_up: Callback<()>,
    on_expand_down: Callback<()>,
    on_expand_all: Callback<()>,
) -> impl IntoView {
    let label = if hidden_lines == 1 {
        String::from("1 hidden line")
    } else {
        alloc::format!("{hidden_lines} hidden lines")
    };

    view! {
        <div class="expand-bar">
            <span class="expand-bar__label">{label}</span>
            <div class="expand-bar__actions">
                {if hidden_lines <= DEFAULT_EXPAND_COUNT {
                    view! {
                        <button
                            type="button"
                            class="btn btn--ghost btn--xs"
                            prop:disabled=move || is_loading.get()
                            on:click=move |_| on_expand_all.run(())
                        >
                            {alloc::format!("↕ Expand all {hidden_lines}")}
                        </button>
                    }.into_any()
                } else {
                    match position {
                        ExpandBarPosition::Top => view! {
                            <>
                                <button
                                    type="button"
                                    class="btn btn--ghost btn--xs"
                                    prop:disabled=move || is_loading.get()
                                    on:click=move |_| on_expand_down.run(())
                                >
                                    "↓ Expand 20"
                                </button>
                                <button
                                    type="button"
                                    class="btn btn--ghost btn--xs"
                                    prop:disabled=move || is_loading.get()
                                    on:click=move |_| on_expand_all.run(())
                                >
                                    {alloc::format!("↕ Expand all ({hidden_lines})")}
                                </button>
                            </>
                        }.into_any(),
                        ExpandBarPosition::Middle => view! {
                            <>
                                <button
                                    type="button"
                                    class="btn btn--ghost btn--xs"
                                    prop:disabled=move || is_loading.get()
                                    on:click=move |_| on_expand_up.run(())
                                >
                                    "↑ 20 above"
                                </button>
                                <button
                                    type="button"
                                    class="btn btn--ghost btn--xs"
                                    prop:disabled=move || is_loading.get()
                                    on:click=move |_| on_expand_down.run(())
                                >
                                    "↓ 20 below"
                                </button>
                                <button
                                    type="button"
                                    class="btn btn--ghost btn--xs"
                                    prop:disabled=move || is_loading.get()
                                    on:click=move |_| on_expand_all.run(())
                                >
                                    {alloc::format!("↕ All ({hidden_lines})")}
                                </button>
                            </>
                        }.into_any(),
                        ExpandBarPosition::Bottom => view! {
                            <>
                                <button
                                    type="button"
                                    class="btn btn--ghost btn--xs"
                                    prop:disabled=move || is_loading.get()
                                    on:click=move |_| on_expand_up.run(())
                                >
                                    "↑ Expand 20"
                                </button>
                                <button
                                    type="button"
                                    class="btn btn--ghost btn--xs"
                                    prop:disabled=move || is_loading.get()
                                    on:click=move |_| on_expand_all.run(())
                                >
                                    {alloc::format!("↕ Expand all ({hidden_lines})")}
                                </button>
                            </>
                        }.into_any(),
                    }
                }}
            </div>
        </div>
    }
}

fn side_attr(side: CommentSide) -> &'static str {
    match side {
        CommentSide::Old => "old",
        CommentSide::New => "new",
    }
}

fn render_line_actions(
    side: CommentSide,
    line_num: u32,
    editor_line: Option<u32>,
) -> impl IntoView {
    let side_str = side_attr(side);
    let line_str = line_num.to_string();
    view! {
        <span class="diff-line-actions">
            {editor_line.map(|line_n| {
                let edit_line_str = line_n.to_string();
                view! {
                    <button
                        type="button"
                        class="line-action-btn"
                        title="Open line in external editor"
                        data-line-action="open-editor"
                        data-action-line=edit_line_str
                    >
                        "↗"
                    </button>
                }
            })}
            <button
                type="button"
                class="line-action-btn line-action-btn--comment"
                title="Comment on line (Shift+click or drag for range)"
                data-line-action="comment"
                data-gutter-side=side_str
                data-gutter-line=line_str
            >
                "+"
            </button>
        </span>
    }
}

fn render_unified_chunk(
    mc: &MergedChunk,
    line_limit: usize,
    file_path: &str,
    thread_index: &ThreadIndex,
    submit_file_path: &str,
    ctx: ChunkRenderCtx,
) -> impl IntoView {
    let limit = mc.lines.len().min(line_limit);
    view! {
        <table class="diff-table diff-table--unified">
            <colgroup>
                <col class="col-num" />
                <col class="col-num" />
                <col class="col-code" />
            </colgroup>
            <tbody>
                {(0..limit).filter_map(|idx| {
                    let line = mc.line(idx)?;
                    let is_expanded = mc.is_line_expanded(idx);
                    let row_class_base = unified_row_class(line.kind, is_expanded);
                    let default_side = if line.kind == DiffLineKind::Delete {
                        CommentSide::Old
                    } else {
                        CommentSide::New
                    };
                    let default_num = match default_side {
                        CommentSide::Old => line.old_line_number,
                        CommentSide::New => line.new_line_number.or(line.old_line_number),
                    };

                    let line_threads = thread_index.unified_line_threads(line);
                    let editor_line = (!ctx.is_commit_msg && line.kind != DiffLineKind::Delete)
                        .then(|| line.new_line_number.or(line.old_line_number))
                        .flatten();
                    let old_num = line.old_line_number;
                    let new_num = line.new_line_number;
                    let kind = line.kind;
                    let sigil = line_sigil(kind);
                    let sigil_class = line_sigil_class(kind);
                    let line_src = mc.line_source(idx);
                    let row_id = line_src.map(|(oc, ol)| {
                        state::diff_line_element_id(file_path, oc, ol, default_side)
                    });

                    let data_old_line = (kind != DiffLineKind::Add)
                        .then_some(old_num)
                        .flatten()
                        .map(|n| n.to_string());
                    let data_new_line = (kind != DiffLineKind::Delete)
                        .then_some(new_num)
                        .flatten()
                        .map(|n| n.to_string());
                    let old_gutter_line = old_num.map(|n| n.to_string());
                    let new_gutter_line = new_num.map(|n| n.to_string());

                    let old_gutter_text = old_gutter_line.clone().unwrap_or_default();
                    let new_gutter_text = new_gutter_line.clone().unwrap_or_default();

                    let is_composer_here = move || {
                        (kind != DiffLineKind::Delete
                            && new_num.is_some_and(|n| {
                                ctx.composer_selector
                                    .with_value(|sel| sel.selected(&Some((CommentSide::New, n))))
                            }))
                            || (kind != DiffLineKind::Add
                                && old_num.is_some_and(|n| {
                                    ctx.composer_selector.with_value(|sel| {
                                        sel.selected(&Some((CommentSide::Old, n)))
                                    })
                                }))
                    };
                    let submit_path = String::from(submit_file_path);

                    let thread_views = line_threads
                        .into_iter()
                        .map(|thread| {
                            let snippet = thread.line_range.map(|lr| {
                                extract_chunk_line_range_snippet(mc, thread.side, lr)
                            });
                            view! {
                                <tr class="diff-inline-slot">
                                    <td colspan=3 class="diff-inline-cell">
                                        <div class="diff-inline-inner">
                                            <CommentThreadCard
                                                thread=thread.clone()
                                                code_snippet=snippet
                                                on_reply=ctx.on_reply
                                                on_toggle_resolve=ctx.on_toggle_resolve
                                            />
                                        </div>
                                    </td>
                                </tr>
                            }
                        })
                        .collect_view();

                    Some(view! {
                        <>
                            <tr
                                id=row_id
                                class=row_class_base
                                data-old-line=data_old_line
                                data-new-line=data_new_line
                            >
                                <td
                                    class="diff-gutter diff-gutter--num"
                                    title={old_num.map(|_| "Click or drag to comment on old line")}
                                    data-gutter-side=old_num.map(|_| "old")
                                    data-gutter-line=old_gutter_line
                                >
                                    {old_gutter_text}
                                </td>
                                <td
                                    class="diff-gutter diff-gutter--num diff-gutter--actions"
                                    title={new_num.map(|_| "Click or drag to comment on new line")}
                                    data-gutter-side=new_num.map(|_| "new")
                                    data-gutter-line=new_gutter_line
                                >
                                    <span>{new_gutter_text}</span>
                                    {default_num.map(|ln| {
                                        render_line_actions(default_side, ln, editor_line)
                                    })}
                                </td>
                                <td class="diff-code-cell">
                                    <div class="diff-code-inner">
                                        <span class=sigil_class>{sigil}</span>
                                        {render_code_line(line)}
                                    </div>
                                </td>
                            </tr>

                            {thread_views}

                            {move || {
                                if !is_composer_here() {
                                    return None;
                                }
                                ctx.active_composer.get().map(|comp| {
                                    let sp = submit_path.clone();
                                    let title = format_composer_title(comp.range, comp.side);
                                    view! {
                                        <tr class="diff-inline-slot">
                                            <td colspan=3 class="diff-inline-cell">
                                                <div class="diff-inline-inner">
                                                    <CommentComposer
                                                        title=title
                                                        selected_code=Some(comp.code_snippet.clone())
                                                        on_submit=Callback::new(move |evt: CommentSubmitEvent| {
                                                            let on_done = evt.on_done;
                                                            let wrapped = Callback::new(move |res: Result<(), String>| {
                                                                if res.is_ok() {
                                                                    let _ = ctx.active_composer.try_set(None);
                                                                }
                                                                on_done.run(res);
                                                            });
                                                            ctx.on_create_thread.run(CreateThreadEvent {
                                                                file_path: sp.clone(),
                                                                side: comp.side,
                                                                line_range: Some(comp.range),
                                                                body: evt.body,
                                                                on_done: wrapped,
                                                            });
                                                        })
                                                        on_cancel=Callback::new(move |()| ctx.active_composer.set(None))
                                                    />
                                                </div>
                                            </td>
                                        </tr>
                                    }
                                })
                            }}
                        </>
                    })
                }).collect_view()}
            </tbody>
        </table>
    }
}

fn render_split_chunk(
    mc: &MergedChunk,
    rows: &[SideBySideRow],
    file_path: &str,
    thread_index: &ThreadIndex,
    submit_file_path: &str,
    ctx: ChunkRenderCtx,
) -> impl IntoView {
    view! {
        <table class="diff-table diff-table--split">
            <colgroup>
                <col class="col-num" />
                <col class="col-code" />
                <col class="col-num" />
                <col class="col-code" />
            </colgroup>
            <tbody>
                {rows.iter().copied().map(|row| {
                    let old_line = row.old_line_idx.and_then(|i| mc.line(i));
                    let new_line = row.new_line_idx.and_then(|i| mc.line(i));
                    let old_is_expanded = row.old_line_idx.is_some_and(|i| mc.is_line_expanded(i));
                    let new_is_expanded = row.new_line_idx.is_some_and(|i| mc.is_line_expanded(i));
                    let old_src = row.old_line_idx.and_then(|i| mc.line_source(i));
                    let new_src = row.new_line_idx.and_then(|i| mc.line_source(i));

                    let old_num = old_line.and_then(|l| l.old_line_number);
                    let new_num = new_line.and_then(|l| l.new_line_number);

                    let old_class_base = match old_line {
                        Some(l) => split_cell_class(l.kind, old_is_expanded),
                        None => "diff-cell--blank",
                    };
                    let new_class_base = match new_line {
                        Some(l) => split_cell_class(l.kind, new_is_expanded),
                        None => "diff-cell--blank",
                    };

                    let old_threads = old_num
                        .map_or(&[][..], |ln| thread_index.at(CommentSide::Old, ln));
                    let new_threads = new_num
                        .map_or(&[][..], |ln| thread_index.at(CommentSide::New, ln));

                    let is_context_pair = old_line.is_some_and(|l| l.kind == DiffLineKind::Context)
                        && new_line.is_some_and(|l| l.kind == DiffLineKind::Context);

                    let submit_old = String::from(submit_file_path);
                    let submit_new = String::from(submit_file_path);

                    let old_id = old_src.map(|(oc, ol)| {
                        state::diff_line_element_id(file_path, oc, ol, CommentSide::Old)
                    });
                    let new_id = new_src.map(|(oc, ol)| {
                        state::diff_line_element_id(file_path, oc, ol, CommentSide::New)
                    });

                    let old_line_str = old_num.map(|n| n.to_string());
                    let new_line_str = new_num.map(|n| n.to_string());
                    let old_line_text = old_line_str.clone().unwrap_or_default();
                    let new_line_text = new_line_str.clone().unwrap_or_default();

                    let old_composer_here = move || {
                        old_num.is_some_and(|n| {
                            ctx.composer_selector
                                .with_value(|sel| sel.selected(&Some((CommentSide::Old, n))))
                        })
                    };
                    let new_composer_here = move || {
                        new_num.is_some_and(|n| {
                            ctx.composer_selector
                                .with_value(|sel| sel.selected(&Some((CommentSide::New, n))))
                        })
                    };

                    let old_gutter_cls = alloc::format!("{old_class_base} diff-gutter diff-gutter--num diff-gutter--actions");
                    let old_code_cls = alloc::format!("{old_class_base} diff-code-cell diff-code-cell--split");
                    let new_gutter_cls = alloc::format!("{new_class_base} diff-gutter diff-gutter--num diff-gutter--actions");
                    let new_code_cls = alloc::format!("{new_class_base} diff-code-cell diff-code-cell--split");

                    let old_thread_views = old_threads
                        .iter()
                        .map(|thread| {
                            let align = if is_context_pair { SplitAlign::Full } else { SplitAlign::Left };
                            let snippet = thread.line_range.map(|lr| {
                                extract_chunk_line_range_snippet(mc, CommentSide::Old, lr)
                            });
                            render_split_thread_row(thread.clone(), snippet, align, ctx.on_reply, ctx.on_toggle_resolve)
                        })
                        .collect_view();

                    let new_thread_views = new_threads
                        .iter()
                        .map(|thread| {
                            let align = if is_context_pair { SplitAlign::Full } else { SplitAlign::Right };
                            let snippet = thread.line_range.map(|lr| {
                                extract_chunk_line_range_snippet(mc, CommentSide::New, lr)
                            });
                            render_split_thread_row(thread.clone(), snippet, align, ctx.on_reply, ctx.on_toggle_resolve)
                        })
                        .collect_view();

                    view! {
                        <>
                            <tr
                                class="diff-split-row"
                                data-old-line=old_line_str.clone()
                                data-new-line=new_line_str.clone()
                            >
                                <td
                                    id=old_id
                                    class=old_gutter_cls
                                    title={old_num.map(|_| "Click or drag to comment on old side")}
                                    data-gutter-side=old_num.map(|_| "old")
                                    data-gutter-line=old_line_str.clone()
                                    data-split-side=old_num.map(|_| "old")
                                    data-split-line=old_line_str.clone()
                                >
                                    <span>{old_line_text}</span>
                                    {old_num.map(|ln| {
                                        render_line_actions(CommentSide::Old, ln, None)
                                    })}
                                </td>
                                <td
                                    class=old_code_cls
                                    data-split-side=old_num.map(|_| "old")
                                    data-split-line=old_line_str.clone()
                                >
                                    {old_line.map(|l| view! {
                                        <div class="diff-code-inner">
                                            <span class=line_sigil_class(l.kind)>{line_sigil(l.kind)}</span>
                                            {render_code_line(l)}
                                        </div>
                                    })}
                                </td>

                                <td
                                    id=new_id
                                    class=new_gutter_cls
                                    title={new_num.map(|_| "Click or drag to comment on new side")}
                                    data-gutter-side=new_num.map(|_| "new")
                                    data-gutter-line=new_line_str.clone()
                                    data-split-side=new_num.map(|_| "new")
                                    data-split-line=new_line_str.clone()
                                >
                                    <span>{new_line_text}</span>
                                    {new_num.map(|ln| {
                                        render_line_actions(
                                            CommentSide::New,
                                            ln,
                                            (!ctx.is_commit_msg).then_some(ln),
                                        )
                                    })}
                                </td>
                                <td
                                    class=new_code_cls
                                    data-split-side=new_num.map(|_| "new")
                                    data-split-line=new_line_str.clone()
                                >
                                    {new_line.map(|l| view! {
                                        <div class="diff-code-inner">
                                            <span class=line_sigil_class(l.kind)>{line_sigil(l.kind)}</span>
                                            {render_code_line(l)}
                                        </div>
                                    })}
                                </td>
                            </tr>

                            {old_thread_views}
                            {new_thread_views}

                            {move || {
                                if !old_composer_here() {
                                    return None;
                                }
                                ctx.active_composer.get().map(|comp| {
                                    let align = if is_context_pair { SplitAlign::Full } else { SplitAlign::Left };
                                    render_split_composer_row(comp, submit_old.clone(), align, ctx.active_composer, ctx.on_create_thread)
                                })
                            }}

                            {move || {
                                if !new_composer_here() {
                                    return None;
                                }
                                ctx.active_composer.get().map(|comp| {
                                    let align = if is_context_pair { SplitAlign::Full } else { SplitAlign::Right };
                                    render_split_composer_row(comp, submit_new.clone(), align, ctx.active_composer, ctx.on_create_thread)
                                })
                            }}
                        </>
                    }
                }).collect_view()}
            </tbody>
        </table>
    }
}

fn render_split_thread_row(
    thread: JjCommentThread,
    snippet: Option<String>,
    align: SplitAlign,
    on_reply: Callback<CommentReplyEvent>,
    on_toggle_resolve: Callback<(String, bool)>,
) -> impl IntoView {
    let slot_class = split_slot_class(align);
    view! {
        <tr class="diff-inline-slot">
            <td colspan=4 class="diff-inline-cell">
                <div class=slot_class>
                    <CommentThreadCard
                        thread=thread
                        code_snippet=snippet
                        on_reply=on_reply
                        on_toggle_resolve=on_toggle_resolve
                    />
                </div>
            </td>
        </tr>
    }
}

fn render_split_composer_row(
    comp: ActiveComposer,
    submit_file_path: String,
    align: SplitAlign,
    active_composer: RwSignal<Option<ActiveComposer>>,
    on_create_thread: Callback<CreateThreadEvent>,
) -> impl IntoView {
    let sp = submit_file_path;
    let slot_class = split_slot_class(align);
    let title = format_composer_title(comp.range, comp.side);
    view! {
        <tr class="diff-inline-slot">
            <td colspan=4 class="diff-inline-cell">
                <div class=slot_class>
                    <CommentComposer
                        title=title
                        selected_code=Some(comp.code_snippet.clone())
                        on_submit=Callback::new(move |evt: CommentSubmitEvent| {
                            let on_done = evt.on_done;
                            let wrapped = Callback::new(move |res: Result<(), String>| {
                                if res.is_ok() {
                                    let _ = active_composer.try_set(None);
                                }
                                on_done.run(res);
                            });
                            on_create_thread.run(CreateThreadEvent {
                                file_path: sp.clone(),
                                side: comp.side,
                                line_range: Some(comp.range),
                                body: evt.body,
                                on_done: wrapped,
                            });
                        })
                        on_cancel=Callback::new(move |()| active_composer.set(None))
                    />
                </div>
            </td>
        </tr>
    }
}

fn collect_placed_unified_thread_ids<'a>(
    mc: &MergedChunk,
    line_limit: usize,
    thread_index: &'a ThreadIndex,
    placed: &mut HashSet<&'a str>,
) {
    let limit = mc.lines.len().min(line_limit);
    for idx in 0..limit {
        let Some(line) = mc.line(idx) else {
            continue;
        };
        for t in thread_index.unified_line_threads(line) {
            placed.insert(t.id.as_str());
        }
    }
}

fn collect_placed_split_thread_ids<'a>(
    mc: &MergedChunk,
    visible_rows: &[SideBySideRow],
    thread_index: &'a ThreadIndex,
    placed: &mut HashSet<&'a str>,
) {
    for row in visible_rows {
        if let Some(old_n) = row
            .old_line_idx
            .and_then(|i| mc.line(i))
            .and_then(|l| l.old_line_number)
        {
            for t in thread_index.at(CommentSide::Old, old_n) {
                placed.insert(t.id.as_str());
            }
        }
        if let Some(new_n) = row
            .new_line_idx
            .and_then(|i| mc.line(i))
            .and_then(|l| l.new_line_number)
        {
            for t in thread_index.at(CommentSide::New, new_n) {
                placed.insert(t.id.as_str());
            }
        }
    }
}

fn extract_chunk_line_range_snippet(
    mc: &MergedChunk,
    side: CommentSide,
    range: LineRangeSpec,
) -> String {
    mc.iter_lines()
        .filter_map(|l| {
            let ln = match side {
                CommentSide::Old => l.old_line_number,
                CommentSide::New => l.new_line_number,
            }?;
            (ln >= range.start && ln <= range.end).then_some(l.content.as_str())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_snippet_across_merged(
    chunks: &[MergedChunk],
    side: CommentSide,
    range: LineRangeSpec,
) -> String {
    let mut parts = Vec::new();
    for chunk in chunks {
        let s = extract_chunk_line_range_snippet(chunk, side, range);
        if !s.is_empty() {
            parts.push(s);
        }
    }
    parts.join("\n")
}

fn selection_class(
    drag: Option<LineSelectionDrag>,
    composer: Option<&ActiveComposer>,
    side: CommentSide,
    line_num: Option<u32>,
) -> &'static str {
    let Some(ln) = line_num else {
        return "";
    };
    if let Some(d) = drag
        && d.side == side
    {
        let min = d.anchor_line.min(d.current_line);
        let max = d.anchor_line.max(d.current_line);
        if ln >= min && ln <= max {
            return "line-row--drag-selected";
        }
    }
    if let Some(c) = composer
        && c.side == side
        && ln >= c.range.start
        && ln <= c.range.end
    {
        return "line-row--comment-selected";
    }
    ""
}

fn format_composer_title(range: LineRangeSpec, side: CommentSide) -> String {
    let side_tag = if side == CommentSide::Old {
        " (old)"
    } else {
        ""
    };
    if range.start == range.end {
        alloc::format!("Comment on line {}{side_tag}", range.start)
    } else {
        alloc::format!("Comment on lines {}-{}{side_tag}", range.start, range.end)
    }
}

fn unified_row_class(kind: DiffLineKind, is_expanded: bool) -> &'static str {
    if is_expanded {
        return "diff-row diff-row--expanded";
    }
    match kind {
        DiffLineKind::Add => "diff-row diff-row--add",
        DiffLineKind::Delete => "diff-row diff-row--del",
        DiffLineKind::Context => "diff-row diff-row--ctx",
    }
}

fn split_cell_class(kind: DiffLineKind, is_expanded: bool) -> &'static str {
    if is_expanded {
        return "diff-cell--expanded";
    }
    match kind {
        DiffLineKind::Add => "diff-cell--add",
        DiffLineKind::Delete => "diff-cell--del",
        DiffLineKind::Context => "diff-cell--ctx",
    }
}

fn split_slot_class(align: SplitAlign) -> &'static str {
    match align {
        SplitAlign::Left => "diff-inline-inner diff-inline-inner--left",
        SplitAlign::Right => "diff-inline-inner diff-inline-inner--right",
        SplitAlign::Full => "diff-inline-inner diff-inline-inner--full",
    }
}

fn line_sigil(kind: DiffLineKind) -> &'static str {
    match kind {
        DiffLineKind::Add => "+",
        DiffLineKind::Delete => "-",
        DiffLineKind::Context => " ",
    }
}

fn line_sigil_class(kind: DiffLineKind) -> &'static str {
    match kind {
        DiffLineKind::Add => "diff-sigil diff-sigil--add",
        DiffLineKind::Delete => "diff-sigil diff-sigil--del",
        DiffLineKind::Context => "diff-sigil diff-sigil--ctx",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tellit_types::DiffChunk;

    fn mk_line(kind: DiffLineKind, old_n: Option<u32>, new_n: Option<u32>, text: &str) -> DiffLine {
        DiffLine {
            kind,
            content: String::from(text),
            old_line_number: old_n,
            new_line_number: new_n,
            syntax_spans: Vec::new(),
            word_diff: Vec::new(),
        }
    }

    #[test]
    fn extracts_line_range_snippet_for_requested_side() {
        let file = Arc::new(DiffFile {
            path: String::from("src/lib.rs"),
            old_path: None,
            status: FileStatus::Modified,
            additions: 2,
            deletions: 1,
            is_generated: false,
            chunks: alloc::vec![DiffChunk {
                old_start: 1,
                old_lines: 2,
                new_start: 1,
                new_lines: 3,
                header: String::from("@@ -1,2 +1,3 @@"),
                lines: alloc::vec![
                    mk_line(DiffLineKind::Context, Some(1), Some(1), "ctx 1"),
                    mk_line(DiffLineKind::Delete, Some(2), None, "old 2"),
                    mk_line(DiffLineKind::Add, None, Some(2), "new 2"),
                    mk_line(DiffLineKind::Add, None, Some(3), "new 3"),
                ],
            }],
            chunks_loaded: true,
        });

        assert_eq!(
            extract_snippet_for_file(
                &file,
                None,
                CommentSide::New,
                LineRangeSpec { start: 1, end: 3 }
            ),
            "ctx 1\nnew 2\nnew 3"
        );
        assert_eq!(
            extract_snippet_for_file(
                &file,
                None,
                CommentSide::Old,
                LineRangeSpec { start: 2, end: 2 }
            ),
            "old 2"
        );
    }

    #[test]
    fn thread_index_resolves_unified_and_split_threads_in_constant_time() {
        let mk_thread = |id: &str, side: CommentSide, end: u32| JjCommentThread {
            id: String::from(id),
            file_path: String::from("src/lib.rs"),
            line_range: Some(LineRangeSpec { start: end, end }),
            side,
            resolved: false,
            commit_id: String::from("abc"),
            change_id: String::from("ch1"),
            comments: Vec::new(),
        };

        let threads = alloc::vec![
            mk_thread("t-new-2", CommentSide::New, 2),
            mk_thread("t-old-2", CommentSide::Old, 2),
        ];
        let idx = ThreadIndex::from_threads(&threads);

        let ctx_line = mk_line(DiffLineKind::Context, Some(2), Some(2), "ctx");
        let unified_ids: Vec<&str> = idx
            .unified_line_threads(&ctx_line)
            .into_iter()
            .map(|t| t.id.as_str())
            .collect();
        assert_eq!(unified_ids, alloc::vec!["t-new-2", "t-old-2"]);
        assert_eq!(idx.at(CommentSide::Old, 2).len(), 1);
        assert_eq!(idx.at(CommentSide::New, 2).len(), 1);
        assert!(idx.at(CommentSide::New, 99).is_empty());
    }

    #[test]
    fn chunk_window_renders_small_files_in_full_and_slices_large_files_progressively() {
        // Under the 500-line threshold, every chunk renders in full regardless of visible_limit.
        let small = compute_chunk_window(&[120, 200, 180], 500, CHUNK_WINDOW_STEP_LINES);
        assert_eq!(small.total_lines, 500);
        assert_eq!(small.shown_lines, 500);
        assert_eq!(small.remaining_lines, 0);
        assert_eq!(small.chunk_limits, alloc::vec![120, 200, 180]);

        // Over 500 lines, slices chunks sequentially up to visible_limit.
        let step1 = compute_chunk_window(&[100, 400, 300], 800, CHUNK_WINDOW_STEP_LINES);
        assert_eq!(step1.total_lines, 800);
        assert_eq!(step1.shown_lines, 150);
        assert_eq!(step1.remaining_lines, 650);
        assert_eq!(step1.chunk_limits, alloc::vec![100, 50, 0]);

        // Expanding the budget reveals more lines and eventually all chunks.
        let step2 = compute_chunk_window(&[100, 400, 300], 800, CHUNK_WINDOW_STEP_LINES * 4);
        assert_eq!(step2.shown_lines, 600);
        assert_eq!(step2.remaining_lines, 200);
        assert_eq!(step2.chunk_limits, alloc::vec![100, 400, 100]);

        let all = compute_chunk_window(&[100, 400, 300], 800, usize::MAX);
        assert_eq!(all.shown_lines, 800);
        assert_eq!(all.remaining_lines, 0);
        assert_eq!(all.chunk_limits, alloc::vec![100, 400, 300]);

        // Split mode: 300 paired rows from 600 merged lines (300 Delete + 300 Add) still windows
        // because progressive gating uses total_merged_lines (> 500), not paired row count.
        let split_window = compute_chunk_window(&[300], 600, CHUNK_WINDOW_STEP_LINES);
        assert_eq!(split_window.total_lines, 300);
        assert_eq!(split_window.shown_lines, 150);
        assert_eq!(split_window.remaining_lines, 150);
        assert_eq!(split_window.chunk_limits, alloc::vec![150]);
    }
}
