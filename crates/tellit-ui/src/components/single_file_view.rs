//! Dedicated Gerrit-style single-file diff view with file switcher, prev/next stepping, and viewed-and-next flow.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use tellit_types::JjCommentThread;
use wasm_bindgen::JsCast;
use web_sys::{HtmlSelectElement, MouseEvent};

use crate::components::text_diff::TextDiffCard;
use crate::components::text_diff::expanded::ExpandedLinesMap;
use crate::route::{AppRoute, RouteQuery};
use crate::state::DiffViewMode;
use crate::{DiffCardCallbacks, FileCardSlot};

fn is_plain_primary_click(ev: &MouseEvent) -> bool {
    ev.button() == 0 && !ev.ctrl_key() && !ev.meta_key() && !ev.shift_key() && !ev.alt_key()
}

/// Focused single-file diff page (`/c/<change_id>/file/<encoded_path>`).
#[component]
pub fn SingleFileView(
    #[prop(into)] change_id: Signal<String>,
    #[prop(into)] route_query: Signal<RouteQuery>,
    #[prop(into)] file_path: Signal<String>,
    #[prop(into)] ordered_file_slots: Signal<Vec<FileCardSlot>>,
    #[prop(into)] reviewed_files: Signal<HashSet<String>>,
    #[prop(into)] all_threads: Signal<Vec<JjCommentThread>>,
    #[prop(into)] diff_mode: Signal<DiffViewMode>,
    #[prop(into)] show_resolved_inline: Signal<bool>,
    #[prop(into)] expanded_lines: Signal<ExpandedLinesMap>,
    #[prop(into)] expand_loading: Signal<bool>,
    callbacks: DiffCardCallbacks,
    on_back_to_overview: Callback<()>,
    on_open_file: Callback<String>,
    on_viewed_and_next: Callback<String>,
) -> impl IntoView {
    let empty_collapsed = Signal::derive(HashSet::<String>::new);
    let noop_collapse = Callback::new(|_: String| {});

    let overview_url = Memo::new(move |_| {
        AppRoute::ChangeOverview {
            change_id: change_id.get(),
            query: route_query.get(),
        }
        .to_url()
    });

    let thread_counts = Memo::new(move |_| {
        all_threads.with(|list| {
            let mut counts: HashMap<String, (usize, usize)> = HashMap::new();
            for t in list {
                let entry = counts.entry(t.file_path.clone()).or_insert((0, 0));
                entry.0 += 1;
                if !t.resolved {
                    entry.1 += 1;
                }
            }
            counts
        })
    });

    let active_match = Memo::new(move |_| {
        let target_path = file_path.get();
        ordered_file_slots.with(|slots| {
            let total = slots.len();
            let pos = slots.iter().position(|s| s.path == target_path);
            let prev_path = pos
                .and_then(|i| i.checked_sub(1))
                .map(|i| slots[i].path.clone());
            let next_path = pos
                .and_then(|i| (i + 1 < total).then_some(i + 1))
                .map(|i| slots[i].path.clone());
            (pos, total, prev_path, next_path)
        })
    });

    let active_slot_list = Memo::new(move |_| {
        let target_path = file_path.get();
        ordered_file_slots.with(|slots| {
            slots
                .iter()
                .find(|s| s.path == target_path)
                .cloned()
                .into_iter()
                .collect::<Vec<_>>()
        })
    });

    let is_active_viewed = Memo::new(move |_| {
        let p = file_path.get();
        reviewed_files.with(|set| set.contains(&p))
    });

    let switcher_options = Memo::new(move |_| {
        ordered_file_slots.with(|slots| {
            reviewed_files.with(|reviewed| {
                thread_counts.with(|counts| {
                    slots
                        .iter()
                        .map(|s| {
                            let is_done = reviewed.contains(&s.path);
                            let (total, unresolved) =
                                counts.get(&s.path).copied().unwrap_or((0, 0));
                            let prefix = if is_done { "✓ " } else { "  " };
                            let suffix = if unresolved > 0 {
                                alloc::format!(" ({unresolved} unresolved / {total})")
                            } else if total > 0 {
                                alloc::format!(" ({total} resolved)")
                            } else {
                                String::new()
                            };
                            (s.path.clone(), alloc::format!("{prefix}{}{suffix}", s.path))
                        })
                        .collect::<Vec<_>>()
                })
            })
        })
    });

    view! {
        <div class="single-file-view">
            <header class="single-file-bar">
                <div class="single-file-bar__left">
                    <a
                        class="btn btn--ghost btn--xs overview-action-link"
                        href=move || overview_url.get()
                        title="Return to change overview (u)"
                        on:click=move |ev: MouseEvent| {
                            if is_plain_primary_click(&ev) {
                                ev.prevent_default();
                                on_back_to_overview.run(());
                            }
                        }
                    >
                        "← Overview"
                    </a>

                    <select
                        class="single-file-bar__select"
                        aria-label="Switch file"
                        prop:value=move || file_path.get()
                        on:change=move |ev| {
                            if let Some(sel) = ev
                                .target()
                                .and_then(|t| t.dyn_into::<HtmlSelectElement>().ok())
                            {
                                let next = sel.value();
                                if !next.is_empty() {
                                    on_open_file.run(next);
                                }
                            }
                        }
                    >
                        {move || {
                            let current = file_path.get();
                            switcher_options
                                .get()
                                .into_iter()
                                .map(|(path, label)| {
                                    let is_selected = path == current;
                                    view! {
                                        <option value=path selected=is_selected>{label}</option>
                                    }
                                })
                                .collect_view()
                        }}
                    </select>

                    <span class="single-file-bar__counter">
                        {move || {
                            let (pos, total, _, _) = active_match.get();
                            match pos {
                                Some(i) => alloc::format!("{} / {total}", i + 1),
                                None => alloc::format!("— / {total}"),
                            }
                        }}
                    </span>
                </div>

                <div class="single-file-bar__right">
                    <button
                        type="button"
                        class="btn btn--ghost btn--xs"
                        title="Previous file ([)"
                        disabled=move || active_match.with(|m| m.2.is_none())
                        on:click=move |_| {
                            if let Some(prev) = active_match.with_untracked(|m| m.2.clone()) {
                                on_open_file.run(prev);
                            }
                        }
                    >
                        "← Prev"
                    </button>
                    <button
                        type="button"
                        class="btn btn--ghost btn--xs"
                        title="Next file (])"
                        disabled=move || active_match.with(|m| m.3.is_none())
                        on:click=move |_| {
                            if let Some(next) = active_match.with_untracked(|m| m.3.clone()) {
                                on_open_file.run(next);
                            }
                        }
                    >
                        "Next →"
                    </button>

                    {move || active_match.with(|m| m.0.is_some()).then(|| view! {
                        <>
                            <label class="viewed-toggle" title="Toggle viewed (v)">
                                <input
                                    type="checkbox"
                                    class="file-checkbox"
                                    prop:checked=move || is_active_viewed.get()
                                    on:change=move |_| callbacks.on_toggle_viewed.run(file_path.get_untracked())
                                />
                                <span>"Viewed"</span>
                            </label>
                            <button
                                type="button"
                                class="btn btn--primary btn--xs"
                                title="Mark viewed and go to next unviewed file (Shift+V)"
                                on:click=move |_| on_viewed_and_next.run(file_path.get_untracked())
                            >
                                "Viewed & Next"
                            </button>
                        </>
                    })}
                </div>
            </header>

            <For
                each=move || active_slot_list.get()
                key=|slot| slot.path.clone()
                children=move |slot| {
                    view! {
                        <TextDiffCard
                            path=slot.path
                            file=slot.file
                            file_index=slot.file_index
                            commits=slot.commits
                            drag_state=slot.drag_state
                            active_composer=slot.active_composer
                            md_tab=slot.md_tab
                            is_near_viewport=slot.is_near_viewport
                            loading_chunks=slot.loading_chunks
                            load_error=slot.load_error
                            visible_line_limit=slot.visible_line_limit
                            all_threads=all_threads
                            reviewed_files=reviewed_files
                            collapsed_files=empty_collapsed
                            diff_mode=diff_mode
                            show_resolved_inline=show_resolved_inline
                            expanded_lines=expanded_lines
                            expand_loading=expand_loading
                            show_collapse_toggle=false
                            on_register_card=callbacks.on_register_card
                            on_unregister_card=callbacks.on_unregister_card
                            on_request_file_diff=callbacks.on_request_file_diff
                            on_window_expanded=callbacks.on_window_expanded
                            on_toggle_viewed=callbacks.on_toggle_viewed
                            on_toggle_collapse=noop_collapse
                            on_open_in_editor=callbacks.on_open_in_editor
                            on_prefetch_line_count=callbacks.on_prefetch_line_count
                            on_expand_lines=callbacks.on_expand_lines
                            on_create_thread=callbacks.on_create_thread
                            on_reply=callbacks.on_reply
                            on_toggle_resolve=callbacks.on_toggle_resolve
                        />
                    }
                }
            />

            {move || active_slot_list.with(Vec::is_empty).then(|| {
                let missing = file_path.get();
                view! {
                    <div class="empty-diff-card">
                        <h2>"File not found in this revision"</h2>
                        <p>{alloc::format!("The path \"{missing}\" is not part of the loaded diff.")}</p>
                        <div class="empty-diff-card__actions">
                            <button
                                type="button"
                                class="btn btn--primary"
                                on:click=move |_| on_back_to_overview.run(())
                            >
                                "← Back to Overview"
                            </button>
                        </div>
                    </div>
                }
            })}
        </div>
    }
}
