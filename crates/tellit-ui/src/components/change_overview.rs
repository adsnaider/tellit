//! Gerrit-style change overview: commit message panel and filterable file table with inline diff expansion.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use tellit_types::{DiffFile, DiffLineKind, JjChange, JjCommentThread};
use wasm_bindgen::JsCast;
use web_sys::{HtmlInputElement, MouseEvent};

use crate::components::file_list::status_badge_class;
use crate::components::is_plain_primary_click;
use crate::components::text_diff::TextDiffCard;
use crate::components::text_diff::expanded::ExpandedLinesMap;
use crate::route::{AppRoute, COMMIT_MESSAGE_PATH, RouteQuery};
use crate::state::{self, AutoCollapseReason, DiffViewMode};
use crate::{DiffCardCallbacks, FileCardSlot};

fn select_commit_message_body(
    cm_file: Option<&DiffFile>,
    commit_desc: Option<&str>,
    change_desc: Option<&str>,
) -> String {
    let trimmed_commit_desc = commit_desc.filter(|s| !s.trim().is_empty());
    if let Some(file) = cm_file {
        let is_partial_hunk =
            file.chunks.len() > 1 || file.chunks.first().is_some_and(|c| c.new_start > 1);
        if is_partial_hunk && let Some(full) = trimmed_commit_desc {
            return String::from(full.trim_end());
        }
        let mut lines = Vec::new();
        for chunk in &file.chunks {
            for line in &chunk.lines {
                if line.kind != DiffLineKind::Delete {
                    lines.push(line.content.as_str());
                }
            }
        }
        let extracted = lines.join("\n");
        if !extracted.trim().is_empty() {
            return extracted;
        }
    }
    trimmed_commit_desc
        .map(str::trim_end)
        .or_else(|| change_desc.map(str::trim).filter(|s| !s.is_empty()))
        .map_or_else(|| String::from("(no description set)"), String::from)
}

fn extract_commit_message_body(
    slots: &[FileCardSlot],
    commit_desc: Option<&str>,
    change_desc: Option<&str>,
) -> String {
    if let Some(cm_slot) = slots.iter().find(|s| s.path == COMMIT_MESSAGE_PATH) {
        return cm_slot.file.with(|file| {
            select_commit_message_body(Some(file.as_ref()), commit_desc, change_desc)
        });
    }
    select_commit_message_body(None, commit_desc, change_desc)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct FileThreadSummary {
    total: usize,
    unresolved: usize,
}

/// Gerrit-style Change Overview displaying the commit message followed by the changed files table.
#[component]
pub fn ChangeOverview(
    #[prop(into)] change_id: Signal<String>,
    #[prop(into)] route_query: Signal<RouteQuery>,
    #[prop(into)] active_change: Signal<Option<JjChange>>,
    #[prop(into)] active_commit_description: Signal<Option<String>>,
    #[prop(into)] selected_file_path: Signal<Option<String>>,
    #[prop(into)] is_empty_diff: Signal<bool>,
    #[prop(into)] ordered_file_slots: Signal<Vec<FileCardSlot>>,
    overview_expanded_files: RwSignal<HashSet<String>>,
    #[prop(into)] reviewed_files: Signal<HashSet<String>>,
    #[prop(into)] all_threads: Signal<Vec<JjCommentThread>>,
    #[prop(into)] diff_mode: Signal<DiffViewMode>,
    #[prop(into)] show_resolved_inline: Signal<bool>,
    #[prop(into)] expanded_lines: Signal<ExpandedLinesMap>,
    #[prop(into)] expand_loading: Signal<bool>,
    callbacks: DiffCardCallbacks,
    on_open_file: Callback<String>,
    on_toggle_inline_expand: Callback<String>,
    on_expand_all_inline: Callback<Vec<String>>,
    on_collapse_all_inline: Callback<()>,
) -> impl IntoView {
    let filter_text = RwSignal::new(String::new());
    let empty_collapsed = Signal::derive(HashSet::<String>::new);

    let thread_summaries = Memo::new(move |_| {
        all_threads.with(|list| {
            let mut map: HashMap<String, FileThreadSummary> = HashMap::new();
            for t in list {
                let entry = map.entry(t.file_path.clone()).or_default();
                entry.total += 1;
                if !t.resolved {
                    entry.unresolved += 1;
                }
            }
            map
        })
    });

    let commit_message_body = Memo::new(move |_| {
        let commit_desc = active_commit_description.get();
        let change_desc = active_change.with(|c| c.as_ref().map(|ch| ch.description.clone()));
        ordered_file_slots.with(|slots| {
            extract_commit_message_body(slots, commit_desc.as_deref(), change_desc.as_deref())
        })
    });

    let filtered_slots = Memo::new(move |_| {
        let query = filter_text.with(|s| s.trim().to_ascii_lowercase());
        ordered_file_slots.with(|slots| {
            if query.is_empty() {
                return slots.clone();
            }
            slots
                .iter()
                .filter(|slot| {
                    slot.path.to_ascii_lowercase().contains(&query)
                        || slot.file.with(|f| {
                            f.old_path
                                .as_deref()
                                .is_some_and(|old| old.to_ascii_lowercase().contains(&query))
                        })
                })
                .cloned()
                .collect::<Vec<_>>()
        })
    });

    let totals = Memo::new(move |_| {
        ordered_file_slots.with(|slots| {
            let mut add = 0u64;
            let mut del = 0u64;
            for slot in slots {
                slot.file.with(|f| {
                    add += u64::from(f.additions);
                    del += u64::from(f.deletions);
                });
            }
            (slots.len(), add, del)
        })
    });

    let commit_msg_url = Memo::new(move |_| {
        AppRoute::FileDiff {
            change_id: change_id.get(),
            file_path: String::from(COMMIT_MESSAGE_PATH),
            query: route_query.get(),
        }
        .to_url()
    });

    let commit_msg_threads = Memo::new(move |_| {
        thread_summaries.with(|map| map.get(COMMIT_MESSAGE_PATH).copied().unwrap_or_default())
    });

    let commit_msg_inline_expanded =
        Memo::new(move |_| overview_expanded_files.with(|set| set.contains(COMMIT_MESSAGE_PATH)));

    view! {
        <div class="change-overview">
            <section class="commit-message-panel">
                <header class="commit-message-panel__header">
                    <div class="commit-message-panel__title-group">
                        <span class="file-status file-status--commit">"CM"</span>
                        <h2 class="commit-message-panel__title">"Commit Message"</h2>
                        {move || {
                            let summary = commit_msg_threads.get();
                            (summary.total > 0).then(|| {
                                let cls = if summary.unresolved > 0 {
                                    "pill-tag pill-tag--warn"
                                } else {
                                    "pill-tag pill-tag--ok"
                                };
                                let label = if summary.unresolved > 0 {
                                    alloc::format!("{} unresolved / {}", summary.unresolved, summary.total)
                                } else {
                                    alloc::format!("{} resolved", summary.total)
                                };
                                view! { <span class=cls>{label}</span> }
                            })
                        }}
                    </div>
                    <div class="commit-message-panel__actions">
                        <button
                            type="button"
                            class="btn btn--ghost btn--xs"
                            on:click=move |_| on_toggle_inline_expand.run(String::from(COMMIT_MESSAGE_PATH))
                        >
                            {move || if commit_msg_inline_expanded.get() { "Collapse inline" } else { "Expand inline" }}
                        </button>
                        <a
                            class="btn btn--ghost btn--xs overview-action-link"
                            href=move || commit_msg_url.get()
                            on:click=move |ev: MouseEvent| {
                                if is_plain_primary_click(&ev) {
                                    ev.prevent_default();
                                    on_open_file.run(String::from(COMMIT_MESSAGE_PATH));
                                }
                            }
                        >
                            "Open diff →"
                        </a>
                    </div>
                </header>
                <pre class="commit-message-panel__body">{move || commit_message_body.get()}</pre>
            </section>

            <section class="overview-files-card">
                <div class="overview-toolbar">
                    <div class="overview-toolbar__left">
                        <input
                            type="search"
                            class="overview-filter-input"
                            placeholder="Filter files..."
                            aria-label="Filter changed files"
                            prop:value=move || filter_text.get()
                            on:input=move |ev| {
                                if let Some(input) = ev
                                    .target()
                                    .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
                                {
                                    filter_text.set(input.value());
                                }
                            }
                        />
                        {move || (!filter_text.with(String::is_empty)).then(|| view! {
                            <button
                                type="button"
                                class="btn btn--ghost btn--xs"
                                on:click=move |_| filter_text.set(String::new())
                            >
                                "Clear"
                            </button>
                        })}
                        <span class="overview-toolbar__stats">
                            {move || {
                                let (total_files, total_add, total_del) = totals.get();
                                let viewed = ordered_file_slots.with(|slots| {
                                    reviewed_files.with(|rev| {
                                        slots.iter().filter(|s| rev.contains(&s.path)).count()
                                    })
                                });
                                let shown = filtered_slots.with(Vec::len);
                                let count_label = if shown == total_files {
                                    alloc::format!("{viewed} / {total_files} viewed")
                                } else {
                                    alloc::format!("{shown} of {total_files} files ({viewed} viewed)")
                                };
                                view! {
                                    <>
                                        <span>{count_label}</span>
                                        <span class="diff-totals">
                                            <span class="stat-add">{alloc::format!("+{total_add}")}</span>
                                            <span class="stat-del">{alloc::format!("-{total_del}")}</span>
                                        </span>
                                    </>
                                }
                            }}
                        </span>
                    </div>
                    <div class="overview-toolbar__actions">
                        <button
                            type="button"
                            class="btn btn--ghost btn--xs"
                            on:click=move |_| {
                                let paths: Vec<String> = filtered_slots
                                    .with_untracked(|slots| slots.iter().map(|s| s.path.clone()).collect());
                                on_expand_all_inline.run(paths);
                            }
                        >
                            "Expand All"
                        </button>
                        <button
                            type="button"
                            class="btn btn--ghost btn--xs"
                            on:click=move |_| on_collapse_all_inline.run(())
                        >
                            "Collapse All"
                        </button>
                    </div>
                </div>

                <div class="overview-table-header" role="row">
                    <span class="overview-col-viewed" title="Viewed">"✓"</span>
                    <span class="overview-col-expand"></span>
                    <span class="overview-col-status">"St"</span>
                    <span class="overview-col-path">"File"</span>
                    <span class="overview-col-comments">"Comments"</span>
                    <span class="overview-col-stats">"Size"</span>
                </div>

                <div class="overview-table-body">
                    <For
                        each=move || filtered_slots.get()
                        key=|slot| slot.path.clone()
                        children=move |slot| {
                            let path_for_viewed = slot.path.clone();
                            let path_for_toggle_viewed = slot.path.clone();
                            let path_for_expanded = slot.path.clone();
                            let path_for_selected = slot.path.clone();
                            let path_for_toggle_expand = slot.path.clone();
                            let path_for_url = slot.path.clone();
                            let path_for_open = slot.path.clone();
                            let path_for_title = slot.path.clone();
                            let path_for_threads = slot.path.clone();
                            let is_commit_msg = slot.path == COMMIT_MESSAGE_PATH;

                            let is_viewed = Memo::new(move |_| {
                                reviewed_files.with(|set| set.contains(&path_for_viewed))
                            });
                            let is_expanded = Memo::new(move |_| {
                                overview_expanded_files.with(|set| set.contains(&path_for_expanded))
                            });
                            let is_selected = Memo::new(move |_| {
                                selected_file_path
                                    .with(|sel| sel.as_deref() == Some(path_for_selected.as_str()))
                            });
                            let file_url = Memo::new(move |_| {
                                AppRoute::FileDiff {
                                    change_id: change_id.get(),
                                    file_path: path_for_url.clone(),
                                    query: route_query.get(),
                                }
                                .to_url()
                            });
                            let file_threads = Memo::new(move |_| {
                                thread_summaries
                                    .with(|map| map.get(&path_for_threads).copied().unwrap_or_default())
                            });

                            let slot_for_card = slot.clone();
                            let row_id = state::file_element_id(&slot.path);

                            view! {
                                <div class="overview-file-group">
                                    <div
                                        id=move || (!is_expanded.get()).then(|| row_id.clone())
                                        data-file-idx=move || slot.file_index.get().to_string()
                                        class=move || {
                                            let mut cls = String::from("overview-file-row");
                                            if is_viewed.get() {
                                                cls.push_str(" overview-file-row--viewed");
                                            }
                                            if is_expanded.get() {
                                                cls.push_str(" overview-file-row--expanded");
                                            }
                                            if is_selected.get() {
                                                cls.push_str(" overview-file-row--selected");
                                            }
                                            cls
                                        }
                                    >
                                        <span class="overview-col-viewed">
                                            <input
                                                type="checkbox"
                                                class="file-checkbox"
                                                aria-label="Mark file as viewed"
                                                prop:checked=move || is_viewed.get()
                                                on:change=move |_| callbacks.on_toggle_viewed.run(path_for_toggle_viewed.clone())
                                            />
                                        </span>

                                        <span class="overview-col-expand">
                                            <button
                                                type="button"
                                                class="icon-btn overview-expand-btn"
                                                title=move || if is_expanded.get() { "Collapse inline diff" } else { "Expand inline diff" }
                                                aria-expanded=move || is_expanded.get()
                                                on:click=move |_| on_toggle_inline_expand.run(path_for_toggle_expand.clone())
                                            >
                                                {move || if is_expanded.get() { "▾" } else { "▸" }}
                                            </button>
                                        </span>

                                        <span class="overview-col-status">
                                            {if is_commit_msg {
                                                view! { <span class="file-status file-status--commit">"CM"</span> }.into_any()
                                            } else {
                                                view! {
                                                    <span class=move || slot.file.with(|f| status_badge_class(f.status))>
                                                        {move || slot.file.with(|f| f.status.short_code())}
                                                    </span>
                                                }.into_any()
                                            }}
                                        </span>

                                        <span class="overview-col-path">
                                            <a
                                                class="overview-file-link"
                                                href=move || file_url.get()
                                                on:click=move |ev: MouseEvent| {
                                                    if is_plain_primary_click(&ev) {
                                                        ev.prevent_default();
                                                        on_open_file.run(path_for_open.clone());
                                                    }
                                                }
                                            >
                                                {move || slot.file.with(|f| match f.old_path {
                                                    Some(ref old) => alloc::format!("{old} -> {path_for_title}"),
                                                    None => path_for_title.clone(),
                                                })}
                                            </a>
                                            {move || slot.file.with(|f| state::auto_collapse_reason(f)).map(|reason| {
                                                let cls = if reason == AutoCollapseReason::Deleted {
                                                    "pill-tag pill-tag--del"
                                                } else {
                                                    "pill-tag"
                                                };
                                                view! { <span class=cls>{reason.badge_label()}</span> }
                                            })}
                                        </span>

                                        <span class="overview-col-comments">
                                            {move || {
                                                let summary = file_threads.get();
                                                (summary.total > 0).then(|| {
                                                    let cls = if summary.unresolved > 0 {
                                                        "pill-tag pill-tag--warn"
                                                    } else {
                                                        "pill-tag pill-tag--ok"
                                                    };
                                                    let label = if summary.unresolved > 0 {
                                                        alloc::format!("{}/{}", summary.unresolved, summary.total)
                                                    } else {
                                                        alloc::format!("{} ✓", summary.total)
                                                    };
                                                    view! { <span class=cls>{label}</span> }
                                                })
                                            }}
                                        </span>

                                        <span class="overview-col-stats diff-totals">
                                            <span class="stat-add">{move || alloc::format!("+{}", slot.file.with(|f| f.additions))}</span>
                                            <span class="stat-del">{move || alloc::format!("-{}", slot.file.with(|f| f.deletions))}</span>
                                        </span>
                                    </div>

                                    {move || {
                                        if !is_expanded.get() {
                                            return ().into_any();
                                        }
                                        let card_slot = slot_for_card.clone();
                                        view! {
                                            <div class="overview-inline-diff">
                                                <TextDiffCard
                                                    path=card_slot.path
                                                    file=card_slot.file
                                                    file_index=card_slot.file_index
                                                    commits=card_slot.commits
                                                    drag_state=card_slot.drag_state
                                                    active_composer=card_slot.active_composer
                                                    md_tab=card_slot.md_tab
                                                    pending_md_scroll=card_slot.pending_md_scroll
                                                    is_near_viewport=card_slot.is_near_viewport
                                                    loading_chunks=card_slot.loading_chunks
                                                    load_error=card_slot.load_error
                                                    visible_line_limit=card_slot.visible_line_limit
                                                    all_threads=all_threads
                                                    reviewed_files=reviewed_files
                                                    collapsed_files=empty_collapsed
                                                    diff_mode=diff_mode
                                                    show_resolved_inline=show_resolved_inline
                                                    expanded_lines=expanded_lines
                                                    expand_loading=expand_loading
                                                    show_collapse_toggle=true
                                                    on_register_card=callbacks.on_register_card
                                                    on_unregister_card=callbacks.on_unregister_card
                                                    on_request_file_diff=callbacks.on_request_file_diff
                                                    on_window_expanded=callbacks.on_window_expanded
                                                    on_toggle_viewed=callbacks.on_toggle_viewed
                                                    on_toggle_collapse=on_toggle_inline_expand
                                                    on_open_in_editor=callbacks.on_open_in_editor
                                                    on_prefetch_line_count=callbacks.on_prefetch_line_count
                                                    on_expand_lines=callbacks.on_expand_lines
                                                    on_create_thread=callbacks.on_create_thread
                                                    on_reply=callbacks.on_reply
                                                    on_toggle_resolve=callbacks.on_toggle_resolve
                                                />
                                            </div>
                                        }
                                        .into_any()
                                    }}
                                </div>
                            }
                        }
                    />
                    {move || filtered_slots.with(Vec::is_empty).then(|| view! {
                        <div class="overview-empty-filter">"No files match the current filter."</div>
                    })}
                    {move || (is_empty_diff.get() && ordered_file_slots.with(|s| s.len() <= 1)).then(|| view! {
                        <div class="overview-empty-filter">"No file modifications in this revision."</div>
                    })}
                </div>
            </section>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tellit_types::{DiffChunk, DiffLine, FileStatus};

    fn mk_line(kind: DiffLineKind, content: &str, new_n: Option<u32>) -> DiffLine {
        DiffLine {
            kind,
            content: String::from(content),
            old_line_number: new_n,
            new_line_number: new_n,
            syntax_spans: Vec::new(),
            word_diff: Vec::new(),
        }
    }

    #[test]
    fn commit_message_panel_prefers_full_description_over_partial_interdiff_hunk() {
        let partial_cm = DiffFile {
            path: String::from(COMMIT_MESSAGE_PATH),
            old_path: None,
            status: FileStatus::Modified,
            additions: 1,
            deletions: 1,
            is_generated: false,
            chunks: alloc::vec![DiffChunk {
                old_start: 8,
                old_lines: 2,
                new_start: 8,
                new_lines: 2,
                header: String::from("@@ -8,2 +8,2 @@"),
                lines: alloc::vec![
                    mk_line(DiffLineKind::Context, "tail context", Some(8)),
                    mk_line(DiffLineKind::Add, "updated footer", Some(9)),
                ],
            }],
            chunks_loaded: true,
        };
        let full_desc = "feat: full title\n\nBody paragraph\n\ntail context\nupdated footer\n";
        assert_eq!(
            select_commit_message_body(
                Some(&partial_cm),
                Some(full_desc),
                Some("feat: full title")
            ),
            "feat: full title\n\nBody paragraph\n\ntail context\nupdated footer"
        );

        // Full single-commit diff starting at line 1 uses the chunk text directly.
        let full_cm = DiffFile {
            chunks: alloc::vec![DiffChunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 2,
                header: String::from("@@ -0,0 +1,2 @@"),
                lines: alloc::vec![
                    mk_line(DiffLineKind::Add, "title line", Some(1)),
                    mk_line(DiffLineKind::Add, "second line", Some(2)),
                ],
            }],
            ..partial_cm
        };
        assert_eq!(
            select_commit_message_body(Some(&full_cm), None, Some("title line")),
            "title line\nsecond line"
        );
    }
}
