//! Unified and split text diff viewer with context expansion, word highlighting, and inline review threads.

extern crate alloc;

pub(crate) mod expanded;
pub(crate) mod split;
pub(crate) mod word_highlight;

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use std::collections::HashSet;

use leptos::ev;
use leptos::prelude::*;
use tellit_types::{
    CommentSide, DiffFile, DiffLine, DiffLineKind, FileStatus, JjCommentThread, LineRangeSpec,
};

use crate::components::comments::{
    CommentComposer, CommentReplyEvent, CommentSubmitEvent, CommentThreadCard,
};
use crate::components::file_list::status_badge_class;
use crate::components::image_diff::ImageDiffBody;
use crate::components::keyboard::CursorPosition;
use crate::components::markdown_diff::{MarkdownPreviewBody, MarkdownTab};
use crate::state::{self, DiffViewMode};

use expanded::{
    DEFAULT_EXPAND_COUNT, ExpandDirection, ExpandedLinesMap, MergedChunk, merge_chunks_for_file,
};
use split::pair_side_by_side_rows;
use word_highlight::CodeLineContent;

const COMMIT_MESSAGE_PATH: &str = "Commit Message";

/// Trigger signal emitted when the user presses `c` on a keyboard-focused diff line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardCommentTrigger {
    /// Monotonic counter so repeated triggers on the same line still open the composer.
    pub seq: u64,
    /// Target file path of the focused row.
    pub file_path: String,
    /// Diff side (`Old` or `New`) of the focused row.
    pub side: CommentSide,
    /// 1-based line number of the focused row.
    pub line_num: u32,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineSelectionDrag {
    side: CommentSide,
    anchor_line: u32,
    current_line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveComposer {
    side: CommentSide,
    range: LineRangeSpec,
    code_snippet: String,
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

#[derive(Clone, Copy)]
struct ChunkRenderCtx {
    is_commit_msg: bool,
    file_index: usize,
    active_cursor: Signal<Option<CursorPosition>>,
    highlighted_word: Signal<Option<String>>,
    on_toggle_word: Callback<String>,
    drag_state: RwSignal<Option<LineSelectionDrag>>,
    selection_anchor: RwSignal<Option<(CommentSide, u32)>>,
    active_composer: RwSignal<Option<ActiveComposer>>,
    on_open_in_editor: Callback<(String, usize)>,
    on_create_thread: Callback<CreateThreadEvent>,
    on_reply: Callback<CommentReplyEvent>,
    on_toggle_resolve: Callback<(String, bool)>,
}

/// Interactive diff card supporting Text, Markdown, Image views, expansion, drag selection, and threads.
#[component]
pub fn TextDiffCard(
    file: DiffFile,
    file_index: usize,
    base_commit: String,
    target_commit: String,
    #[prop(into)] all_threads: Signal<Vec<JjCommentThread>>,
    #[prop(into)] reviewed_files: Signal<HashSet<String>>,
    #[prop(into)] collapsed_files: Signal<HashSet<String>>,
    #[prop(into)] diff_mode: Signal<DiffViewMode>,
    #[prop(into)] show_resolved_inline: Signal<bool>,
    #[prop(into)] highlighted_word: Signal<Option<String>>,
    #[prop(into)] expanded_lines: Signal<ExpandedLinesMap>,
    #[prop(into)] expand_loading: Signal<bool>,
    #[prop(into)] active_cursor: Signal<Option<CursorPosition>>,
    #[prop(into)] keyboard_comment_trigger: Signal<Option<KeyboardCommentTrigger>>,
    on_hover_file: Callback<Option<usize>>,
    on_toggle_word: Callback<String>,
    on_toggle_viewed: Callback<String>,
    on_toggle_collapse: Callback<String>,
    on_open_in_editor: Callback<(String, usize)>,
    on_prefetch_line_count: Callback<Arc<DiffFile>>,
    on_expand_lines: Callback<ExpandLinesEvent>,
    on_create_thread: Callback<CreateThreadEvent>,
    on_reply: Callback<CommentReplyEvent>,
    on_toggle_resolve: Callback<(String, bool)>,
) -> impl IntoView {
    let file = Arc::new(file);
    let path = file.path.clone();
    let is_commit_msg = path == COMMIT_MESSAGE_PATH;
    let is_md = !is_commit_msg && state::is_markdown_file(&path);
    let is_img = !is_commit_msg && state::is_image_file(&path);
    let md_tab = RwSignal::new(MarkdownTab::Preview);
    let is_expandable = !is_commit_msg
        && !is_img
        && file.status != FileStatus::Added
        && file.status != FileStatus::Deleted;
    let has_chunks = !file.chunks.is_empty();
    let hunks_count = file.chunks.len();
    let is_generated = file.is_generated;
    let path_for_id = state::file_element_id(&path);
    let path_toggle_collapse = path.clone();
    let path_toggle_viewed = path.clone();
    let path_for_viewed = path.clone();
    let path_for_collapsed = path.clone();
    let path_for_threads = path.clone();
    let path_for_kbd = path.clone();

    let is_viewed_sig = Signal::derive(move || reviewed_files.get().contains(&path_for_viewed));
    let is_collapsed_sig =
        Signal::derive(move || collapsed_files.get().contains(&path_for_collapsed));

    let header_title = match file.old_path {
        Some(ref old) => alloc::format!("{old} -> {path}"),
        None => path.clone(),
    };

    let whole_file_composing = RwSignal::new(false);
    let drag_state = RwSignal::new(Option::<LineSelectionDrag>::None);
    let selection_anchor = RwSignal::new(Option::<(CommentSide, u32)>::None);
    let active_composer = RwSignal::new(Option::<ActiveComposer>::None);

    let file_for_mouseup = StoredValue::new_local(Arc::clone(&file));
    let file_for_kbd = StoredValue::new_local(Arc::clone(&file));
    let file_for_prefetch = StoredValue::new_local(Arc::clone(&file));

    Effect::new(move |_| {
        let Some(trig) = keyboard_comment_trigger.get() else {
            return;
        };
        if trig.file_path != path_for_kbd {
            return;
        }
        if is_md {
            md_tab.set(MarkdownTab::Code);
        }
        let range = LineRangeSpec {
            start: trig.line_num,
            end: trig.line_num,
        };
        let snippet = file_for_kbd.with_value(|f| {
            let exp_map = expanded_lines.get_untracked();
            let merged = merge_chunks_for_file(f, exp_map.get(&f.path));
            extract_snippet_across_merged(&merged, trig.side, range)
        });
        active_composer.set(Some(ActiveComposer {
            side: trig.side,
            range,
            code_snippet: snippet,
        }));
    });

    let mouseup_handle = window_event_listener(ev::mouseup, move |_| {
        let Some(drag) = drag_state.get_untracked() else {
            return;
        };
        drag_state.set(None);
        let min = drag.anchor_line.min(drag.current_line);
        let max = drag.anchor_line.max(drag.current_line);
        let range = LineRangeSpec {
            start: min,
            end: max,
        };
        let snippet = file_for_mouseup.with_value(|f| {
            let exp_map = expanded_lines.get_untracked();
            let merged = merge_chunks_for_file(f, exp_map.get(&f.path));
            extract_snippet_across_merged(&merged, drag.side, range)
        });
        active_composer.set(Some(ActiveComposer {
            side: drag.side,
            range,
            code_snippet: snippet,
        }));
    });
    on_cleanup(move || mouseup_handle.remove());

    Effect::new(move |_| {
        if is_expandable && has_chunks && !is_collapsed_sig.get() {
            file_for_prefetch.with_value(|f| on_prefetch_line_count.run(Arc::clone(f)));
        }
    });

    let chunk_ctx = ChunkRenderCtx {
        is_commit_msg,
        file_index,
        active_cursor,
        highlighted_word,
        on_toggle_word,
        drag_state,
        selection_anchor,
        active_composer,
        on_open_in_editor,
        on_create_thread,
        on_reply,
        on_toggle_resolve,
    };

    let visible_threads = Signal::derive(move || {
        let show_resolved = show_resolved_inline.get();
        all_threads
            .get()
            .into_iter()
            .filter(|t| t.file_path == path_for_threads && (show_resolved || !t.resolved))
            .collect::<Vec<_>>()
    });

    let total_thread_count = Signal::derive(move || visible_threads.get().len());
    let file_for_body = Arc::clone(&file);

    view! {
        <article
            id=path_for_id
            class=move || if is_viewed_sig.get() {
                "diff-card diff-card--viewed"
            } else {
                "diff-card"
            }
            on:mouseenter=move |_| on_hover_file.run(Some(file_index))
            on:mouseleave=move |_| on_hover_file.run(None)
        >
            <header class="diff-card__header">
                <div class="diff-card__left">
                    <button
                        type="button"
                        class="icon-btn"
                        title=move || if is_collapsed_sig.get() { "Expand file" } else { "Collapse file" }
                        aria-expanded=move || !is_collapsed_sig.get()
                        on:click=move |_| on_toggle_collapse.run(path_toggle_collapse.clone())
                    >
                        {move || if is_collapsed_sig.get() { "▸" } else { "▾" }}
                    </button>
                    <span class=status_badge_class(file.status)>{file.status.short_code()}</span>
                    <span class="diff-card__path">{header_title}</span>
                    {file.is_generated.then(|| view! {
                        <span class="pill-tag">"generated"</span>
                    })}
                    {move || {
                        let count = total_thread_count.get();
                        (count > 0).then(|| view! {
                            <span class="pill-tag pill-tag--warn">
                                {alloc::format!("{count} thread{}", if count == 1 { "" } else { "s" })}
                            </span>
                        })
                    }}
                </div>

                <div class="diff-card__right">
                    {is_md.then(|| view! {
                        <div class="segmented" role="group" aria-label="Markdown view mode">
                            <button
                                type="button"
                                class=move || if md_tab.get() == MarkdownTab::Preview { "segmented__btn segmented__btn--active" } else { "segmented__btn" }
                                on:click=move |_| md_tab.set(MarkdownTab::Preview)
                            >
                                "Preview"
                            </button>
                            <button
                                type="button"
                                class=move || if md_tab.get() == MarkdownTab::Full { "segmented__btn segmented__btn--active" } else { "segmented__btn" }
                                on:click=move |_| md_tab.set(MarkdownTab::Full)
                            >
                                "Full"
                            </button>
                            <button
                                type="button"
                                class=move || if md_tab.get() == MarkdownTab::Code { "segmented__btn segmented__btn--active" } else { "segmented__btn" }
                                on:click=move |_| md_tab.set(MarkdownTab::Code)
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

                    {(!is_commit_msg && file.status != FileStatus::Deleted).then(|| {
                        let edit_path = path.clone();
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

                    <span class="diff-totals">
                        <span class="stat-add">{alloc::format!("+{}", file.additions)}</span>
                        <span class="stat-del">{alloc::format!("-{}", file.deletions)}</span>
                    </span>

                    <label class="viewed-toggle">
                        <input
                            type="checkbox"
                            class="file-checkbox"
                            prop:checked=move || is_viewed_sig.get()
                            on:change=move |_| on_toggle_viewed.run(path_toggle_viewed.clone())
                        />
                        <span>"Viewed"</span>
                    </label>
                </div>
            </header>

            {move || if is_collapsed_sig.get() {
                (hunks_count > 0).then(|| view! {
                    <div class="diff-card__collapsed-note">
                        {if is_generated {
                            alloc::format!("Generated file collapsed ({hunks_count} hunks)")
                        } else {
                            alloc::format!("Collapsed ({hunks_count} hunks)")
                        }}
                    </div>
                }).into_any()
            } else {
                let file_snapshot = Arc::clone(&file_for_body);
                let base_for_body = base_commit.clone();
                let target_for_body = target_commit.clone();
                let path_for_whole_submit = file_snapshot.path.clone();
                let path_for_line_submit = file_snapshot.path.clone();

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
                                    <ImageDiffBody
                                        file=(*file_snapshot).clone()
                                        base_commit=base_for_body
                                        target_commit=target_for_body
                                    />
                                </>
                            }.into_any()
                        } else {
                            let path_for_exp = file_snapshot.path.clone();
                            let file_for_chunks = StoredValue::new_local(Arc::clone(&file_snapshot));
                            let file_exp_memo = Memo::new(move |_| {
                                expanded_lines.with(|map| map.get(&path_for_exp).cloned())
                            });
                            let merged_chunks_memo = Memo::new(move |_| {
                                Arc::new(file_for_chunks.with_value(|f| {
                                    file_exp_memo.with(|exp| merge_chunks_for_file(f, exp.as_ref()))
                                }))
                            });
                            let show_code = Memo::new(move |_| !is_md || md_tab.get() == MarkdownTab::Code);

                            let md_preview_panel = is_md.then(|| {
                                let md_file = Arc::clone(&file_snapshot);
                                let md_base = base_for_body.clone();
                                let md_target = target_for_body.clone();
                                view! {
                                    <div style:display=move || show_code.get().then_some("none")>
                                        {move || (!show_code.get()).then(|| {
                                            render_file_threads_section(visible_threads.get(), on_reply, on_toggle_resolve)
                                        })}
                                        <MarkdownPreviewBody
                                            file=md_file
                                            merged_chunks=merged_chunks_memo
                                            base_commit=md_base
                                            target_commit=md_target
                                            active_tab=md_tab
                                        />
                                    </div>
                                }
                            });

                            view! {
                                <>
                                    {md_preview_panel}
                                    {move || {
                                        if !show_code.get() {
                                            return ().into_any();
                                        }

                                        let active_threads = visible_threads.get();
                                        let merged_chunks = merged_chunks_memo.get();
                                        let mode = diff_mode.get();

                                        let mut placed_ids = HashSet::new();
                                        for mc in merged_chunks.iter() {
                                            collect_placed_thread_ids(mc, &active_threads, mode, &mut placed_ids);
                                        }

                                        let unplaced: Vec<JjCommentThread> = active_threads
                                            .iter()
                                            .filter(|t| !placed_ids.contains(&t.id))
                                            .cloned()
                                            .collect();

                                        let chunk_count = merged_chunks.len();
                                        let path_clone = file_snapshot.path.clone();

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
                                                        .map(|(merged_idx, mc)| {
                                                            let first_orig = *mc.original_indices.first().unwrap_or(&0);
                                                            let last_orig = *mc.original_indices.last().unwrap_or(&0);
                                                            let is_first = merged_idx == 0;
                                                            let is_last = merged_idx + 1 == chunk_count;
                                                            let hidden_before = mc.hidden_lines_before;
                                                            let hidden_after = mc.hidden_lines_after.unwrap_or(0);
                                                            let file_top = Arc::clone(&file_snapshot);
                                                            let file_mid = Arc::clone(&file_snapshot);
                                                            let file_bot = Arc::clone(&file_snapshot);
                                                            let fpath = path_clone.clone();
                                                            let lsubmit = path_for_line_submit.clone();
                                                            let t_list = active_threads.clone();

                                                            view! {
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
                                                                        render_split_chunk(
                                                                            mc,
                                                                            &fpath,
                                                                            &t_list,
                                                                            &lsubmit,
                                                                            chunk_ctx,
                                                                        ).into_any()
                                                                    } else {
                                                                        render_unified_chunk(
                                                                            mc,
                                                                            &fpath,
                                                                            &t_list,
                                                                            &lsubmit,
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
                                                            }
                                                        })
                                                        .collect_view()
                                                        .into_any()
                                                }}
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

fn render_line_actions(
    side: CommentSide,
    line_num: u32,
    editor_line: Option<u32>,
    edit_path: String,
    lines_for_drag: Arc<[DiffLine]>,
    ctx: ChunkRenderCtx,
) -> impl IntoView {
    view! {
        <span class="diff-line-actions">
            {editor_line.map(|line_n| {
                view! {
                    <button
                        type="button"
                        class="line-action-btn"
                        title="Open line in external editor"
                        on:mousedown=move |ev| ev.stop_propagation()
                        on:click=move |ev| {
                            ev.stop_propagation();
                            if let Ok(line_usize) = usize::try_from(line_n) {
                                ctx.on_open_in_editor.run((edit_path.clone(), line_usize));
                            }
                        }
                    >
                        "↗"
                    </button>
                }
            })}
            <button
                type="button"
                class="line-action-btn line-action-btn--comment"
                title="Comment on line (Shift+click or drag for range)"
                on:mousedown=move |ev: ev::MouseEvent| {
                    handle_gutter_mousedown(
                        &ev,
                        side,
                        line_num,
                        &lines_for_drag,
                        ctx.drag_state,
                        ctx.selection_anchor,
                        ctx.active_composer,
                    );
                }
            >
                "+"
            </button>
        </span>
    }
}

fn render_unified_chunk(
    mc: &MergedChunk,
    file_path: &str,
    threads: &[JjCommentThread],
    submit_file_path: &str,
    ctx: ChunkRenderCtx,
) -> impl IntoView {
    let all_lines: Arc<[DiffLine]> = Arc::from(mc.lines.as_slice());

    view! {
        <table class="diff-table diff-table--unified">
            <tbody>
                {mc.lines.iter().enumerate().map(|(idx, line)| {
                    let is_expanded = mc.expanded_line_flags.get(idx).copied().unwrap_or(false);
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

                    let line_threads = unified_line_threads(line, threads);
                    let lines_for_old = Arc::clone(&all_lines);
                    let lines_for_new = Arc::clone(&all_lines);
                    let lines_for_plus = Arc::clone(&all_lines);
                    let lines_for_snippet = Arc::clone(&all_lines);
                    let edit_path = String::from(file_path);
                    let editor_line = (!ctx.is_commit_msg && line.kind != DiffLineKind::Delete)
                        .then(|| line.new_line_number.or(line.old_line_number))
                        .flatten();
                    let old_num = line.old_line_number;
                    let new_num = line.new_line_number;
                    let kind = line.kind;
                    let sigil = line_sigil(kind);
                    let sigil_class = line_sigil_class(kind);
                    let line_src = mc.line_sources.get(idx).copied().flatten();
                    let row_id = line_src.map(|(oc, ol)| {
                        state::diff_line_element_id(file_path, oc, ol, default_side)
                    });

                    let row_class = move || {
                        let drag = ctx.drag_state.get();
                        let comp = ctx.active_composer.get();
                        let sel_new = if kind != DiffLineKind::Delete {
                            selection_class(drag, comp.as_ref(), CommentSide::New, new_num)
                        } else {
                            ""
                        };
                        let sel = if sel_new.is_empty() && kind != DiffLineKind::Add {
                            selection_class(drag, comp.as_ref(), CommentSide::Old, old_num)
                        } else {
                            sel_new
                        };
                        let is_kbd = line_src.is_some_and(|(oc, ol)| {
                            ctx.active_cursor.get().is_some_and(|cur| {
                                cur.file_index == ctx.file_index
                                    && cur.chunk_index == oc
                                    && cur.line_index == ol
                            })
                        });
                        let kbd_cls = if is_kbd { "diff-row--kbd-active" } else { "" };
                        combine_classes(row_class_base, sel, kbd_cls)
                    };

                    let composer_here = move || {
                        ctx.active_composer.get().filter(|comp| match comp.side {
                            CommentSide::Old => {
                                kind != DiffLineKind::Add && old_num == Some(comp.range.end)
                            }
                            CommentSide::New => {
                                kind != DiffLineKind::Delete && new_num == Some(comp.range.end)
                            }
                        })
                    };
                    let submit_path = String::from(submit_file_path);

                    view! {
                        <>
                            <tr
                                id=row_id
                                class=row_class
                                on:mouseenter=move |_| {
                                    if let Some(drag) = ctx.drag_state.get_untracked()
                                        && let Some(ln) = match drag.side {
                                            CommentSide::Old => old_num,
                                            CommentSide::New => new_num,
                                        }
                                    {
                                        ctx.drag_state.set(Some(LineSelectionDrag {
                                            current_line: ln,
                                            ..drag
                                        }));
                                    }
                                }
                            >
                                <td
                                    class="diff-gutter diff-gutter--num"
                                    title={old_num.map(|_| "Click or drag to comment on old line")}
                                    on:mousedown=move |ev: ev::MouseEvent| {
                                        if let Some(ln) = old_num {
                                            handle_gutter_mousedown(
                                                &ev,
                                                CommentSide::Old,
                                                ln,
                                                &lines_for_old,
                                                ctx.drag_state,
                                                ctx.selection_anchor,
                                                ctx.active_composer,
                                            );
                                        }
                                    }
                                >
                                    {old_num.map_or_else(String::new, |n| n.to_string())}
                                </td>
                                <td
                                    class="diff-gutter diff-gutter--num diff-gutter--actions"
                                    title={new_num.map(|_| "Click or drag to comment on new line")}
                                    on:mousedown=move |ev: ev::MouseEvent| {
                                        if let Some(ln) = new_num {
                                            handle_gutter_mousedown(
                                                &ev,
                                                CommentSide::New,
                                                ln,
                                                &lines_for_new,
                                                ctx.drag_state,
                                                ctx.selection_anchor,
                                                ctx.active_composer,
                                            );
                                        }
                                    }
                                >
                                    <span>{new_num.map_or_else(String::new, |n| n.to_string())}</span>
                                    {default_num.map(|ln| {
                                        render_line_actions(
                                            default_side,
                                            ln,
                                            editor_line,
                                            edit_path.clone(),
                                            lines_for_plus.clone(),
                                            ctx,
                                        )
                                    })}
                                </td>
                                <td class="diff-code-cell">
                                    <div class="diff-code-inner">
                                        <span class=sigil_class>{sigil}</span>
                                        <CodeLineContent
                                            line_kind=line.kind
                                            content=line.content.clone()
                                            syntax_spans=line.syntax_spans.clone()
                                            word_segments=line.word_diff.clone()
                                            highlighted_word=ctx.highlighted_word
                                            on_toggle_word=ctx.on_toggle_word
                                        />
                                    </div>
                                </td>
                            </tr>

                            {line_threads.into_iter().map(|thread| {
                                let snippet = thread.line_range.map(|lr| {
                                    extract_line_range_snippet(&lines_for_snippet, thread.side, lr)
                                });
                                view! {
                                    <tr class="diff-inline-slot">
                                        <td colspan=3 class="diff-inline-cell">
                                            <div class="diff-inline-inner">
                                                <CommentThreadCard
                                                    thread=thread
                                                    code_snippet=snippet
                                                    on_reply=ctx.on_reply
                                                    on_toggle_resolve=ctx.on_toggle_resolve
                                                />
                                            </div>
                                        </td>
                                    </tr>
                                }
                            }).collect_view()}

                            {move || composer_here().map(|comp| {
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
                            })}
                        </>
                    }
                }).collect_view()}
            </tbody>
        </table>
    }
}

fn render_split_chunk(
    mc: &MergedChunk,
    file_path: &str,
    threads: &[JjCommentThread],
    submit_file_path: &str,
    ctx: ChunkRenderCtx,
) -> impl IntoView {
    let rows = pair_side_by_side_rows(&mc.lines, &mc.expanded_line_flags, &mc.line_sources);
    let all_lines: Arc<[DiffLine]> = Arc::from(mc.lines.as_slice());

    view! {
        <table class="diff-table diff-table--split">
            <colgroup>
                <col class="col-num" />
                <col class="col-code" />
                <col class="col-num" />
                <col class="col-code" />
            </colgroup>
            <tbody>
                {rows.into_iter().map(|row| {
                    let old_num = row.old_line.as_ref().and_then(|l| l.old_line_number);
                    let new_num = row.new_line.as_ref().and_then(|l| l.new_line_number);

                    let old_class_base = match row.old_line {
                        Some(ref l) => split_cell_class(l.kind, row.old_is_expanded),
                        None => "diff-cell--blank",
                    };
                    let new_class_base = match row.new_line {
                        Some(ref l) => split_cell_class(l.kind, row.new_is_expanded),
                        None => "diff-cell--blank",
                    };

                    let old_threads = old_num.map_or_else(Vec::new, |ln| {
                        split_side_threads(threads, CommentSide::Old, ln)
                    });
                    let new_threads = new_num.map_or_else(Vec::new, |ln| {
                        split_side_threads(threads, CommentSide::New, ln)
                    });

                    let is_context_pair = row.old_line.as_ref().is_some_and(|l| l.kind == DiffLineKind::Context)
                        && row.new_line.as_ref().is_some_and(|l| l.kind == DiffLineKind::Context);

                    let old_lines_drag = Arc::clone(&all_lines);
                    let new_lines_drag = Arc::clone(&all_lines);
                    let snippet_lines = Arc::clone(&all_lines);
                    let old_action_lines = Arc::clone(&all_lines);
                    let new_action_lines = Arc::clone(&all_lines);
                    let edit_path = String::from(file_path);
                    let submit_old = String::from(submit_file_path);
                    let submit_new = String::from(submit_file_path);

                    let old_src = row.old_source;
                    let new_src = row.new_source;
                    let old_id = old_src.map(|(oc, ol)| {
                        state::diff_line_element_id(file_path, oc, ol, CommentSide::Old)
                    });
                    let new_id = new_src.map(|(oc, ol)| {
                        state::diff_line_element_id(file_path, oc, ol, CommentSide::New)
                    });
                    let old_sel_class = move || {
                        let sel = selection_class(
                            ctx.drag_state.get(),
                            ctx.active_composer.get().as_ref(),
                            CommentSide::Old,
                            old_num,
                        );
                        let kbd = if old_src.is_some_and(|(oc, ol)| {
                            ctx.active_cursor.get().is_some_and(|cur| {
                                cur.file_index == ctx.file_index
                                    && cur.chunk_index == oc
                                    && cur.line_index == ol
                                    && cur.side == CommentSide::Old
                            })
                        }) {
                            " diff-row--kbd-active"
                        } else {
                            ""
                        };
                        if kbd.is_empty() {
                            String::from(sel)
                        } else {
                            alloc::format!("{sel}{kbd}")
                        }
                    };
                    let new_sel_class = move || {
                        let sel = selection_class(
                            ctx.drag_state.get(),
                            ctx.active_composer.get().as_ref(),
                            CommentSide::New,
                            new_num,
                        );
                        let kbd = if new_src.is_some_and(|(oc, ol)| {
                            ctx.active_cursor.get().is_some_and(|cur| {
                                cur.file_index == ctx.file_index
                                    && cur.chunk_index == oc
                                    && cur.line_index == ol
                                    && cur.side == CommentSide::New
                            })
                        }) {
                            " diff-row--kbd-active"
                        } else {
                            ""
                        };
                        if kbd.is_empty() {
                            String::from(sel)
                        } else {
                            alloc::format!("{sel}{kbd}")
                        }
                    };

                    let old_composer = move || {
                        ctx.active_composer.get().filter(|comp| {
                            comp.side == CommentSide::Old && old_num == Some(comp.range.end)
                        })
                    };
                    let new_composer = move || {
                        ctx.active_composer.get().filter(|comp| {
                            comp.side == CommentSide::New && new_num == Some(comp.range.end)
                        })
                    };

                    view! {
                        <>
                            <tr
                                class="diff-split-row"
                                on:mouseenter=move |_| {
                                    if let Some(drag) = ctx.drag_state.get_untracked()
                                        && let Some(ln) = match drag.side {
                                            CommentSide::Old => old_num,
                                            CommentSide::New => new_num,
                                        }
                                    {
                                        ctx.drag_state.set(Some(LineSelectionDrag {
                                            current_line: ln,
                                            ..drag
                                        }));
                                    }
                                }
                            >
                                <td
                                    id=old_id
                                    class=move || combine_classes(old_class_base, "diff-gutter diff-gutter--num diff-gutter--actions", &old_sel_class())
                                    title={old_num.map(|_| "Click or drag to comment on old side")}
                                    on:mousedown=move |ev: ev::MouseEvent| {
                                        if let Some(ln) = old_num {
                                            handle_gutter_mousedown(
                                                &ev,
                                                CommentSide::Old,
                                                ln,
                                                &old_lines_drag,
                                                ctx.drag_state,
                                                ctx.selection_anchor,
                                                ctx.active_composer,
                                            );
                                        }
                                    }
                                >
                                    <span>{old_num.map_or_else(String::new, |n| n.to_string())}</span>
                                    {old_num.map(|ln| {
                                        render_line_actions(
                                            CommentSide::Old,
                                            ln,
                                            None,
                                            edit_path.clone(),
                                            old_action_lines,
                                            ctx,
                                        )
                                    })}
                                </td>
                                <td class=move || combine_classes(old_class_base, "diff-code-cell diff-code-cell--split", &old_sel_class())>
                                    {row.old_line.map(|l| view! {
                                        <div class="diff-code-inner">
                                            <span class=line_sigil_class(l.kind)>{line_sigil(l.kind)}</span>
                                            <CodeLineContent
                                                line_kind=l.kind
                                                content=l.content
                                                syntax_spans=l.syntax_spans
                                                word_segments=l.word_diff
                                                highlighted_word=ctx.highlighted_word
                                                on_toggle_word=ctx.on_toggle_word
                                            />
                                        </div>
                                    })}
                                </td>

                                <td
                                    id=new_id
                                    class=move || combine_classes(new_class_base, "diff-gutter diff-gutter--num diff-gutter--actions", &new_sel_class())
                                    title={new_num.map(|_| "Click or drag to comment on new side")}
                                    on:mousedown=move |ev: ev::MouseEvent| {
                                        if let Some(ln) = new_num {
                                            handle_gutter_mousedown(
                                                &ev,
                                                CommentSide::New,
                                                ln,
                                                &new_lines_drag,
                                                ctx.drag_state,
                                                ctx.selection_anchor,
                                                ctx.active_composer,
                                            );
                                        }
                                    }
                                >
                                    <span>{new_num.map_or_else(String::new, |n| n.to_string())}</span>
                                    {new_num.map(|ln| {
                                        render_line_actions(
                                            CommentSide::New,
                                            ln,
                                            (!ctx.is_commit_msg).then_some(ln),
                                            edit_path.clone(),
                                            new_action_lines,
                                            ctx,
                                        )
                                    })}
                                </td>
                                <td class=move || combine_classes(new_class_base, "diff-code-cell diff-code-cell--split", &new_sel_class())>
                                    {row.new_line.map(|l| view! {
                                        <div class="diff-code-inner">
                                            <span class=line_sigil_class(l.kind)>{line_sigil(l.kind)}</span>
                                            <CodeLineContent
                                                line_kind=l.kind
                                                content=l.content
                                                syntax_spans=l.syntax_spans
                                                word_segments=l.word_diff
                                                highlighted_word=ctx.highlighted_word
                                                on_toggle_word=ctx.on_toggle_word
                                            />
                                        </div>
                                    })}
                                </td>
                            </tr>

                            {old_threads.into_iter().map(|thread| {
                                let align = if is_context_pair { SplitAlign::Full } else { SplitAlign::Left };
                                let snippet = thread.line_range.map(|lr| {
                                    extract_line_range_snippet(&snippet_lines, CommentSide::Old, lr)
                                });
                                render_split_thread_row(thread, snippet, align, ctx.on_reply, ctx.on_toggle_resolve)
                            }).collect_view()}

                            {new_threads.into_iter().map(|thread| {
                                let align = if is_context_pair { SplitAlign::Full } else { SplitAlign::Right };
                                let snippet = thread.line_range.map(|lr| {
                                    extract_line_range_snippet(&snippet_lines, CommentSide::New, lr)
                                });
                                render_split_thread_row(thread, snippet, align, ctx.on_reply, ctx.on_toggle_resolve)
                            }).collect_view()}

                            {move || old_composer().map(|comp| {
                                let align = if is_context_pair { SplitAlign::Full } else { SplitAlign::Left };
                                render_split_composer_row(comp, submit_old.clone(), align, ctx.active_composer, ctx.on_create_thread)
                            })}

                            {move || new_composer().map(|comp| {
                                let align = if is_context_pair { SplitAlign::Full } else { SplitAlign::Right };
                                render_split_composer_row(comp, submit_new.clone(), align, ctx.active_composer, ctx.on_create_thread)
                            })}
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

fn handle_gutter_mousedown(
    ev: &ev::MouseEvent,
    side: CommentSide,
    line_num: u32,
    chunk_lines: &[DiffLine],
    drag_state: RwSignal<Option<LineSelectionDrag>>,
    selection_anchor: RwSignal<Option<(CommentSide, u32)>>,
    active_composer: RwSignal<Option<ActiveComposer>>,
) {
    if ev.button() != 0 {
        return;
    }
    ev.prevent_default();
    ev.stop_propagation();

    if ev.shift_key() {
        let (start, end) = match selection_anchor.get_untracked() {
            Some((prev_side, prev_line)) if prev_side == side => {
                (prev_line.min(line_num), prev_line.max(line_num))
            }
            _ => (line_num, line_num),
        };
        selection_anchor.set(Some((side, line_num)));
        let range = LineRangeSpec { start, end };
        let snippet = extract_line_range_snippet(chunk_lines, side, range);
        active_composer.set(Some(ActiveComposer {
            side,
            range,
            code_snippet: snippet,
        }));
        return;
    }

    selection_anchor.set(Some((side, line_num)));
    drag_state.set(Some(LineSelectionDrag {
        side,
        anchor_line: line_num,
        current_line: line_num,
    }));
}

fn unified_line_threads(line: &DiffLine, threads: &[JjCommentThread]) -> Vec<JjCommentThread> {
    let mut matched = Vec::new();
    let mut seen_ids = HashSet::new();

    let (primary_num, primary_side) = if line.kind == DiffLineKind::Delete {
        (line.old_line_number, CommentSide::Old)
    } else {
        (line.new_line_number, CommentSide::New)
    };

    if let Some(num) = primary_num {
        for thread in threads {
            if thread.side == primary_side
                && thread.line_range.is_some_and(|r| r.end == num)
                && seen_ids.insert(thread.id.clone())
            {
                matched.push(thread.clone());
            }
        }
    }

    if line.kind == DiffLineKind::Context
        && let Some(old_num) = line.old_line_number
    {
        for thread in threads {
            if thread.side == CommentSide::Old
                && thread.line_range.is_some_and(|r| r.end == old_num)
                && seen_ids.insert(thread.id.clone())
            {
                matched.push(thread.clone());
            }
        }
    }

    matched
}

fn split_side_threads(
    threads: &[JjCommentThread],
    side: CommentSide,
    line_num: u32,
) -> Vec<JjCommentThread> {
    threads
        .iter()
        .filter(|t| t.side == side && t.line_range.is_some_and(|r| r.end == line_num))
        .cloned()
        .collect()
}

fn collect_placed_thread_ids(
    mc: &MergedChunk,
    threads: &[JjCommentThread],
    mode: DiffViewMode,
    placed: &mut HashSet<String>,
) {
    for line in &mc.lines {
        if mode == DiffViewMode::Split {
            if let Some(old_n) = line.old_line_number {
                for t in split_side_threads(threads, CommentSide::Old, old_n) {
                    placed.insert(t.id);
                }
            }
            if let Some(new_n) = line.new_line_number {
                for t in split_side_threads(threads, CommentSide::New, new_n) {
                    placed.insert(t.id);
                }
            }
        } else {
            for t in unified_line_threads(line, threads) {
                placed.insert(t.id);
            }
        }
    }
}

fn extract_line_range_snippet(
    lines: &[DiffLine],
    side: CommentSide,
    range: LineRangeSpec,
) -> String {
    lines
        .iter()
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
        let s = extract_line_range_snippet(&chunk.lines, side, range);
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

fn combine_classes(a: &str, b: &str, c: &str) -> String {
    if c.is_empty() {
        alloc::format!("{a} {b}")
    } else {
        alloc::format!("{a} {b} {c}")
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
        let lines = alloc::vec![
            mk_line(DiffLineKind::Context, Some(1), Some(1), "ctx 1"),
            mk_line(DiffLineKind::Delete, Some(2), None, "old 2"),
            mk_line(DiffLineKind::Add, None, Some(2), "new 2"),
            mk_line(DiffLineKind::Add, None, Some(3), "new 3"),
        ];
        assert_eq!(
            extract_line_range_snippet(
                &lines,
                CommentSide::New,
                LineRangeSpec { start: 1, end: 3 }
            ),
            "ctx 1\nnew 2\nnew 3"
        );
        assert_eq!(
            extract_line_range_snippet(
                &lines,
                CommentSide::Old,
                LineRangeSpec { start: 2, end: 2 }
            ),
            "old 2"
        );
    }
}
