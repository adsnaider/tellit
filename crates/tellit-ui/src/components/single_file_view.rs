//! Dedicated Gerrit-style single-file diff view with prev/next stepping and viewed-and-next flow.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use std::collections::HashSet;

use leptos::prelude::*;
use tellit_types::JjCommentThread;
use web_sys::MouseEvent;

use crate::components::is_plain_primary_click;
use crate::components::text_diff::expanded::ExpandedLinesMap;
use crate::components::text_diff::{SingleFileHeaderProps, TextDiffCard};
use crate::route::{AppRoute, RouteQuery};
use crate::state::DiffViewMode;
use crate::{DiffCardCallbacks, FileCardSlot};

/// Position and adjacent file targets for the active single-file diff view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SingleFileNavState {
    /// Zero-based index of the active file in the ordered diff list, if present.
    pub position: Option<usize>,
    /// Total number of files in the active diff.
    pub total: usize,
    /// Path of the previous file in diff order, if any.
    pub prev_path: Option<String>,
    /// Path of the next file in diff order, if any.
    pub next_path: Option<String>,
}

impl SingleFileNavState {
    /// Formats the 1-based position counter shown in the single-file header (e.g. `"3 / 330"`).
    #[must_use]
    pub fn position_label(&self) -> String {
        match self.position {
            Some(i) => alloc::format!("{} / {}", i + 1, self.total),
            None => alloc::format!("— / {}", self.total),
        }
    }
}

/// Computes position and adjacent file paths for `target_path` inside an ordered list of paths.
#[must_use]
pub fn compute_single_file_nav<S: AsRef<str>>(
    paths: &[S],
    target_path: &str,
) -> SingleFileNavState {
    let total = paths.len();
    let position = paths.iter().position(|p| p.as_ref() == target_path);
    let prev_path = position
        .and_then(|i| i.checked_sub(1))
        .map(|i| String::from(paths[i].as_ref()));
    let next_path = position
        .and_then(|i| (i + 1 < total).then_some(i + 1))
        .map(|i| String::from(paths[i].as_ref()));
    SingleFileNavState {
        position,
        total,
        prev_path,
        next_path,
    }
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

    let nav_state = Memo::new(move |_| {
        let target_path = file_path.get();
        ordered_file_slots.with(|slots| {
            let paths: Vec<&str> = slots.iter().map(|s| s.path.as_str()).collect();
            compute_single_file_nav(&paths, &target_path)
        })
    });

    let position_label = Signal::derive(move || nav_state.with(SingleFileNavState::position_label));
    let prev_path = Signal::derive(move || nav_state.with(|n| n.prev_path.clone()));
    let next_path = Signal::derive(move || nav_state.with(|n| n.next_path.clone()));

    let header_props = SingleFileHeaderProps {
        overview_url: overview_url.into(),
        position_label,
        prev_path,
        next_path,
        on_back_to_overview,
        on_open_file,
        on_viewed_and_next,
    };

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

    view! {
        <div class="single-file-view">
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
                            single_file_header=header_props
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
                    <>
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
                                <span class="single-file-bar__counter">
                                    {move || position_label.get()}
                                </span>
                            </div>
                        </header>
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
                    </>
                }
            })}
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_nav_state_for_first_middle_last_and_single_files() {
        let files = ["Commit Message", "src/a.rs", "src/b.rs"];

        let first = compute_single_file_nav(&files, "Commit Message");
        assert_eq!(first.position, Some(0));
        assert_eq!(first.total, 3);
        assert_eq!(first.prev_path, None);
        assert_eq!(first.next_path.as_deref(), Some("src/a.rs"));
        assert_eq!(first.position_label(), "1 / 3");

        let middle = compute_single_file_nav(&files, "src/a.rs");
        assert_eq!(middle.position, Some(1));
        assert_eq!(middle.prev_path.as_deref(), Some("Commit Message"));
        assert_eq!(middle.next_path.as_deref(), Some("src/b.rs"));
        assert_eq!(middle.position_label(), "2 / 3");

        let last = compute_single_file_nav(&files, "src/b.rs");
        assert_eq!(last.position, Some(2));
        assert_eq!(last.prev_path.as_deref(), Some("src/a.rs"));
        assert_eq!(last.next_path, None);
        assert_eq!(last.position_label(), "3 / 3");

        let single = compute_single_file_nav(&["only.rs"], "only.rs");
        assert_eq!(single.position, Some(0));
        assert_eq!(single.prev_path, None);
        assert_eq!(single.next_path, None);
        assert_eq!(single.position_label(), "1 / 1");
    }

    #[test]
    fn computes_nav_state_for_missing_and_empty_lists() {
        let files = ["src/a.rs", "src/b.rs"];
        let missing = compute_single_file_nav(&files, "src/missing.rs");
        assert_eq!(missing.position, None);
        assert_eq!(missing.total, 2);
        assert_eq!(missing.prev_path, None);
        assert_eq!(missing.next_path, None);
        assert_eq!(missing.position_label(), "— / 2");

        let empty: [&str; 0] = [];
        let none = compute_single_file_nav(&empty, "src/a.rs");
        assert_eq!(none.position, None);
        assert_eq!(none.total, 0);
        assert_eq!(none.prev_path, None);
        assert_eq!(none.next_path, None);
        assert_eq!(none.position_label(), "— / 0");
    }
}
