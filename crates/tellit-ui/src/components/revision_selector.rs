//! Top-bar Jujutsu change picker, evolog From/To comparison dropdowns, rebase toggle, and approval control.

extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use std::collections::HashSet;

use leptos::ev;
use leptos::html;
use leptos::prelude::*;
use tellit_types::{JjChange, JjCommit, JjGraphRow};
use wasm_bindgen::JsCast;
use web_sys::{HtmlInputElement, Node, SubmitEvent};

use super::review_dot::ReviewDot;
use super::revision_tree::JjRevisionTree;

/// Returns `true` when two commit hashes match exactly or share a prefix of at least 6 characters.
#[must_use]
pub fn commit_ids_match(a: &str, b: &str) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    a.len().min(b.len()) >= 6 && (a.starts_with(b) || b.starts_with(a))
}

/// Returns `true` when `change` is empty, has no description, and has zero comments.
#[must_use]
pub const fn is_empty_undescribed_change(change: &JjChange) -> bool {
    change.is_empty && !change.has_description && change.comment_count == 0
}

fn walk_to_non_empty_ancestor<'a>(
    changes: &'a [JjChange],
    start: &'a JjChange,
) -> Option<&'a JjChange> {
    let mut visited: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<&'a JjChange> = VecDeque::new();
    queue.push_back(start);

    while let Some(current) = queue.pop_front() {
        if !visited.insert(current.commit_id.as_str()) {
            continue;
        }
        if !is_empty_undescribed_change(current) {
            return Some(current);
        }
        for parent_id in &current.parent_commit_ids {
            if let Some(parent) = changes
                .iter()
                .find(|c| commit_ids_match(&c.commit_id, parent_id))
                && !visited.contains(parent.commit_id.as_str())
            {
                queue.push_back(parent);
            }
        }
    }
    None
}

/// Selects the initial change to display, skipping empty undescribed working-copy commits.
#[must_use]
pub fn pick_initial_change<'a>(
    changes: &'a [JjChange],
    current_commit_id: Option<&str>,
) -> Option<&'a JjChange> {
    if changes.is_empty() {
        return None;
    }

    if let Some(cid) = current_commit_id
        && !cid.is_empty()
        && let Some(commit_match) = changes.iter().find(|c| commit_ids_match(&c.commit_id, cid))
    {
        if !is_empty_undescribed_change(commit_match) {
            return Some(commit_match);
        }
        return walk_to_non_empty_ancestor(changes, commit_match)
            .or_else(|| changes.iter().find(|c| !is_empty_undescribed_change(c)))
            .or(Some(commit_match));
    }

    if let Some(working_copy) = changes.iter().find(|c| c.is_working_copy) {
        return walk_to_non_empty_ancestor(changes, working_copy)
            .or_else(|| changes.iter().find(|c| !is_empty_undescribed_change(c)))
            .or(Some(working_copy));
    }

    changes
        .iter()
        .find(|c| !is_empty_undescribed_change(c))
        .or_else(|| changes.first())
}

/// Evolog commit paired with its 1-based causal revision number (`#1`, `#2`, ...).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedJjCommit {
    /// Underlying Jujutsu evolog commit entry.
    pub commit: JjCommit,
    /// 1-based index in causal evolution order.
    pub commit_number: usize,
}

fn is_interesting_commit(
    commit: &JjCommit,
    is_latest: bool,
    target_commit_id: Option<&str>,
    base_commit_id: Option<&str>,
) -> bool {
    if is_latest || commit.approved || commit.comment_count > 0 {
        return true;
    }
    if target_commit_id.is_some_and(|tid| commit_ids_match(&commit.commit_id, tid)) {
        return true;
    }
    base_commit_id
        .filter(|&b| !b.is_empty() && b != "parent")
        .is_some_and(|bid| commit_ids_match(&commit.commit_id, bid))
}

/// Filters evolog commits to reviewed, approved, selected, or latest revisions in reverse order.
#[must_use]
pub fn filter_interesting_commits(
    commits: &[JjCommit],
    target_commit_id: Option<&str>,
    base_commit_id: Option<&str>,
) -> Vec<IndexedJjCommit> {
    let len = commits.len();
    commits
        .iter()
        .enumerate()
        .filter(|(idx, commit)| {
            is_interesting_commit(commit, *idx + 1 == len, target_commit_id, base_commit_id)
        })
        .map(|(idx, commit)| IndexedJjCommit {
            commit: commit.clone(),
            commit_number: idx + 1,
        })
        .rev()
        .collect()
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    let mut out: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        out.push_str("...");
    }
    out
}

fn format_short_timestamp(ts: &str) -> &str {
    if ts.len() >= 16 { &ts[5..16] } else { ts }
}

fn setup_click_outside(
    container: NodeRef<html::Div>,
    is_open: RwSignal<bool>,
    on_close: Callback<()>,
) {
    let handle = window_event_listener(ev::mousedown, move |ev| {
        if !is_open.get_untracked() {
            return;
        }
        let Some(el) = container.get_untracked() else {
            return;
        };
        let node: &Node = el.as_ref();
        let clicked_inside = ev
            .target()
            .and_then(|t| t.dyn_into::<Node>().ok())
            .is_some_and(|target| node.contains(Some(&target)));
        if !clicked_inside {
            on_close.run(());
        }
    });
    on_cleanup(move || handle.remove());
}

/// Top-bar Jujutsu change dropdown with embedded revision graph and revset search input.
#[component]
pub fn JjChangeSelector(
    #[prop(into)] changes: Signal<Vec<JjChange>>,
    #[prop(into)] graph_rows: Signal<Vec<JjGraphRow>>,
    #[prop(into)] selected_change_id: Signal<String>,
    on_select_change: Callback<String>,
    on_lookup_query: Callback<String>,
    #[prop(into)] lookup_error: Signal<Option<String>>,
    #[prop(into)] loading: Signal<bool>,
) -> impl IntoView {
    let is_open = RwSignal::new(false);
    let is_custom_input = RwSignal::new(false);
    let custom_input = RwSignal::new(String::new());
    let dropdown_ref = NodeRef::<html::Div>::new();

    setup_click_outside(
        dropdown_ref,
        is_open,
        Callback::new(move |()| is_open.set(false)),
    );

    let selected_change = Signal::derive(move || {
        let sel = selected_change_id.get();
        changes.get().into_iter().find(|c| c.change_id == sel)
    });

    let on_submit = move |ev: SubmitEvent| {
        ev.prevent_default();
        let query = custom_input.get_untracked().trim().to_string();
        if !query.is_empty() {
            on_lookup_query.run(query);
            is_custom_input.set(false);
            custom_input.set(String::new());
        }
    };

    view! {
        <div class="dropdown-anchor" node_ref=dropdown_ref>
            <div class="selector-pill">
                <span class="selector-pill__label">"Change:"</span>
                {move || if is_custom_input.get() {
                    view! {
                        <form class="rev-lookup-form" on:submit=on_submit>
                            <input
                                type="text"
                                class="rev-lookup-input rev-lookup-input--inline"
                                placeholder="e.g. kkmpptxz or @-"
                                prop:value=move || custom_input.get()
                                on:input=move |ev| {
                                    if let Some(target) = ev.target()
                                        && let Ok(input) = target.dyn_into::<HtmlInputElement>()
                                    {
                                        custom_input.set(input.value());
                                    }
                                }
                            />
                            <button type="submit" class="btn btn--accent-sm">"Go"</button>
                            <button
                                type="button"
                                class="icon-btn"
                                title="Cancel"
                                on:click=move |_| is_custom_input.set(false)
                            >
                                "×"
                            </button>
                        </form>
                    }.into_any()
                } else {
                    view! {
                        <div class="selector-pill__controls">
                            <button
                                type="button"
                                class="selector-pill__button"
                                title="Select Jujutsu Change"
                                on:click=move |_| is_open.update(|v| *v = !*v)
                            >
                                <span class="selector-pill__value">
                                    {move || match selected_change.get() {
                                        Some(c) => {
                                            let prefix = if c.is_working_copy { "@ " } else { "" };
                                            let desc = truncate_chars(&c.description, 22);
                                            alloc::format!("{prefix}{} {desc}", c.short_change_id)
                                        }
                                        None if loading.get() => String::from("Loading..."),
                                        None => String::from("Select change..."),
                                    }}
                                </span>
                                {move || selected_change.get().map(|c| {
                                    let review = c.review_status();
                                    let status = Signal::derive(move || review);
                                    view! {
                                        <ReviewDot
                                            status=status
                                            comment_count=c.comment_count
                                            unresolved_count=c.unresolved_comment_count
                                        />
                                    }
                                })}
                                <span class="chevron">"▾"</span>
                            </button>
                            <button
                                type="button"
                                class="icon-btn"
                                title="Enter Change ID or revset"
                                aria-label="Search change or revset"
                                on:click=move |_| {
                                    is_open.set(false);
                                    is_custom_input.set(true);
                                }
                            >
                                "⌕"
                            </button>
                        </div>
                    }.into_any()
                }}
                {move || lookup_error.get().map(|err| view! {
                    <span class="inline-error">{err}</span>
                })}
            </div>

            {move || is_open.get().then(|| view! {
                <div class="dropdown-popover dropdown-popover--wide">
                    <div class="dropdown-popover__header">"Changes"</div>
                    <JjRevisionTree
                        graph_rows=graph_rows
                        selected_change_id=selected_change_id
                        on_select_change=Callback::new(move |cid| {
                            on_select_change.run(cid);
                            is_open.set(false);
                        })
                        loading=loading
                        list_class="jj-tree-list jj-tree-list--dropdown"
                    />
                    <div class="dropdown-popover__footer">
                        <button
                            type="button"
                            class="dropdown-footer-btn"
                            on:click=move |_| {
                                is_open.set(false);
                                is_custom_input.set(true);
                            }
                        >
                            "⌕ Enter Change ID / Rev..."
                        </button>
                    </div>
                </div>
            })}
        </div>
    }
}

#[component]
fn CommitOptionRow(
    commit: JjCommit,
    commit_number: usize,
    is_selected: bool,
    #[prop(optional)] is_target: bool,
    on_select: Callback<String>,
) -> impl IntoView {
    let cid = commit.commit_id.clone();
    let short_id = commit.short_commit_id.clone();
    let ts = format_short_timestamp(&commit.timestamp).to_string();
    let desc = commit.description.clone();
    let review = commit.review_status();
    let status = Signal::derive(move || review);
    let comment_count = commit.comment_count;
    let unresolved_count = commit.unresolved_comment_count;
    let clean_source = commit.clean_rebase_source_commit_number;

    view! {
        <button
            type="button"
            class={if is_selected {
                "commit-option commit-option--selected"
            } else {
                "commit-option"
            }}
            on:click=move |_| on_select.run(cid.clone())
        >
            <div class="commit-option__left">
                <span class="commit-option__num">{alloc::format!("#{commit_number}")}</span>
                <span class="commit-option__id">{short_id}</span>
                {(!ts.is_empty()).then(|| view! {
                    <span class="commit-option__ts">{alloc::format!("[{ts}]")}</span>
                })}
                <span class="commit-option__desc">{desc}</span>
                {is_target.then(|| view! {
                    <span class="commit-option__head">"(head)"</span>
                })}
            </div>
            <ReviewDot
                status=status
                comment_count=comment_count
                unresolved_count=unresolved_count
                clean_rebase_source_number=clean_source
            />
        </button>
    }
}

/// Top-bar `From -> To` evolog comparison selector, rebase-isolation toggle, and approval button.
#[component]
pub fn JjCommitComparisonSelector(
    #[prop(into)] commits: Signal<Vec<JjCommit>>,
    #[prop(into)] target_commit_id: Signal<String>,
    #[prop(into)] base_commit_id: Signal<String>,
    on_select_target: Callback<String>,
    on_select_base: Callback<String>,
    on_toggle_approve: Callback<(String, bool)>,
    #[prop(into)] is_rebase_diff: Signal<bool>,
    #[prop(into)] rebase_ignored: Signal<bool>,
    #[prop(into)] rebase_conflict: Signal<bool>,
    on_toggle_ignore_rebase: Callback<()>,
    #[prop(into)] loading: Signal<bool>,
) -> impl IntoView {
    let is_from_open = RwSignal::new(false);
    let is_to_open = RwSignal::new(false);
    let show_all_from = RwSignal::new(false);
    let show_all_to = RwSignal::new(false);
    let from_ref = NodeRef::<html::Div>::new();
    let to_ref = NodeRef::<html::Div>::new();

    setup_click_outside(
        from_ref,
        is_from_open,
        Callback::new(move |()| {
            is_from_open.set(false);
            show_all_from.set(false);
        }),
    );
    setup_click_outside(
        to_ref,
        is_to_open,
        Callback::new(move |()| {
            is_to_open.set(false);
            show_all_to.set(false);
        }),
    );

    let all_reversed = Memo::new(move |_| {
        commits
            .get()
            .into_iter()
            .enumerate()
            .map(|(idx, commit)| IndexedJjCommit {
                commit,
                commit_number: idx + 1,
            })
            .rev()
            .collect::<Vec<_>>()
    });

    let interesting_reversed = Memo::new(move |_| {
        let list = commits.get();
        let target = target_commit_id.get();
        let base = base_commit_id.get();
        filter_interesting_commits(&list, Some(&target), Some(&base))
    });

    let active_target = Signal::derive(move || {
        let target = target_commit_id.get();
        commits
            .get()
            .into_iter()
            .enumerate()
            .find(|(_, c)| commit_ids_match(&c.commit_id, &target))
    });

    let active_base = Signal::derive(move || {
        let base = base_commit_id.get();
        if base == "parent" || base.is_empty() {
            return None;
        }
        commits
            .get()
            .into_iter()
            .enumerate()
            .find(|(_, c)| commit_ids_match(&c.commit_id, &base))
    });

    view! {
        {move || {
            let list = commits.get();
            if loading.get() && list.is_empty() {
                return view! {
                    <span class="muted-label">"Loading commits..."</span>
                }.into_any();
            }
            if list.is_empty() {
                return ().into_any();
            }

            let is_base_parent = base_commit_id.get() == "parent" || base_commit_id.get().is_empty();
            let total_count = list.len();
            let interesting_count = interesting_reversed.get().len();
            let hidden_count = total_count.saturating_sub(interesting_count);

            view! {
                <div class="comparison-controls">
                    <div class="dropdown-anchor" node_ref=from_ref>
                        <button
                            type="button"
                            class="selector-btn"
                            title="Select diff base revision (From)"
                            on:click=move |_| {
                                if is_from_open.get_untracked() {
                                    is_from_open.set(false);
                                    show_all_from.set(false);
                                } else {
                                    is_to_open.set(false);
                                    is_from_open.set(true);
                                }
                            }
                        >
                            <span class="selector-btn__text">
                                {move || match active_base.get() {
                                    Some((idx, c)) => alloc::format!("#{}: {}", idx + 1, c.short_commit_id),
                                    None => String::from("Parent"),
                                }}
                            </span>
                            {move || active_base.get().map(|(_, c)| {
                                let review = c.review_status();
                                let status = Signal::derive(move || review);
                                view! {
                                    <ReviewDot
                                        status=status
                                        comment_count=c.comment_count
                                        unresolved_count=c.unresolved_comment_count
                                        clean_rebase_source_number=c.clean_rebase_source_commit_number
                                    />
                                }
                            })}
                            <span class="chevron">"▾"</span>
                        </button>

                        {move || is_from_open.get().then(|| {
                            let visible = if show_all_from.get() || hidden_count == 0 {
                                all_reversed.get()
                            } else {
                                interesting_reversed.get()
                            };
                            let cur_base = base_commit_id.get();
                            let cur_target = target_commit_id.get();
                            let base_is_parent = cur_base == "parent" || cur_base.is_empty();

                            view! {
                                <div class="dropdown-popover dropdown-popover--wide">
                                    <div class="dropdown-popover__header">"Diff Base (From)"</div>
                                    <div class="dropdown-popover__list">
                                        <button
                                            type="button"
                                            class={if base_is_parent {
                                                "commit-option commit-option--selected"
                                            } else {
                                                "commit-option"
                                            }}
                                            on:click=move |_| {
                                                on_select_base.run(String::from("parent"));
                                                is_from_open.set(false);
                                                show_all_from.set(false);
                                            }
                                        >
                                            <div class="commit-option__left">
                                                <span class="commit-option__id">"Parent"</span>
                                                <span class="commit-option__desc">"(Entire Change)"</span>
                                            </div>
                                        </button>

                                        {visible.into_iter().map(|entry| {
                                            let selected = !base_is_parent
                                                && commit_ids_match(&entry.commit.commit_id, &cur_base);
                                            let is_target = commit_ids_match(&entry.commit.commit_id, &cur_target);
                                            view! {
                                                <CommitOptionRow
                                                    commit=entry.commit
                                                    commit_number=entry.commit_number
                                                    is_selected=selected
                                                    is_target=is_target
                                                    on_select=Callback::new(move |cid| {
                                                        on_select_base.run(cid);
                                                        is_from_open.set(false);
                                                        show_all_from.set(false);
                                                    })
                                                />
                                            }
                                        }).collect_view()}
                                    </div>
                                    {(hidden_count > 0).then(|| view! {
                                        <div class="dropdown-popover__footer">
                                            <button
                                                type="button"
                                                class="dropdown-footer-btn"
                                                on:click=move |_| show_all_from.update(|v| *v = !*v)
                                            >
                                                {move || if show_all_from.get() {
                                                    alloc::format!("Hide {hidden_count} unreviewed revisions")
                                                } else {
                                                    alloc::format!("Show all {total_count} revisions ({hidden_count} hidden)")
                                                }}
                                            </button>
                                        </div>
                                    })}
                                </div>
                            }
                        })}
                    </div>

                    <span class="comparison-arrow" aria-hidden="true">"→"</span>

                    <div class="dropdown-anchor" node_ref=to_ref>
                        <button
                            type="button"
                            class="selector-btn"
                            title="Select diff head revision (To)"
                            on:click=move |_| {
                                if is_to_open.get_untracked() {
                                    is_to_open.set(false);
                                    show_all_to.set(false);
                                } else {
                                    is_from_open.set(false);
                                    is_to_open.set(true);
                                }
                            }
                        >
                            <span class="selector-btn__text">
                                {move || match active_target.get() {
                                    Some((idx, c)) => {
                                        let short_desc = truncate_chars(&c.description, 18);
                                        alloc::format!("#{}: {} - {short_desc}", idx + 1, c.short_commit_id)
                                    }
                                    None => String::from("Select commit..."),
                                }}
                            </span>
                            {move || active_target.get().map(|(_, c)| {
                                let review = c.review_status();
                                let status = Signal::derive(move || review);
                                view! {
                                    <ReviewDot
                                        status=status
                                        comment_count=c.comment_count
                                        unresolved_count=c.unresolved_comment_count
                                        clean_rebase_source_number=c.clean_rebase_source_commit_number
                                    />
                                }
                            })}
                            <span class="chevron">"▾"</span>
                        </button>

                        {move || is_to_open.get().then(|| {
                            let visible = if show_all_to.get() || hidden_count == 0 {
                                all_reversed.get()
                            } else {
                                interesting_reversed.get()
                            };
                            let cur_target = target_commit_id.get();
                            let header_label = if show_all_to.get() || hidden_count == 0 {
                                alloc::format!("Diff Head / Commits ({total_count})")
                            } else {
                                alloc::format!("Diff Head / Commits ({interesting_count} of {total_count})")
                            };

                            view! {
                                <div class="dropdown-popover dropdown-popover--wide dropdown-popover--right">
                                    <div class="dropdown-popover__header">{header_label}</div>
                                    <div class="dropdown-popover__list">
                                        {visible.into_iter().map(|entry| {
                                            let selected = commit_ids_match(&entry.commit.commit_id, &cur_target);
                                            view! {
                                                <CommitOptionRow
                                                    commit=entry.commit
                                                    commit_number=entry.commit_number
                                                    is_selected=selected
                                                    on_select=Callback::new(move |cid| {
                                                        on_select_target.run(cid);
                                                        is_to_open.set(false);
                                                        show_all_to.set(false);
                                                    })
                                                />
                                            }
                                        }).collect_view()}
                                    </div>
                                    {(hidden_count > 0).then(|| view! {
                                        <div class="dropdown-popover__footer">
                                            <button
                                                type="button"
                                                class="dropdown-footer-btn"
                                                on:click=move |_| show_all_to.update(|v| *v = !*v)
                                            >
                                                {move || if show_all_to.get() {
                                                    alloc::format!("Hide {hidden_count} unreviewed revisions")
                                                } else {
                                                    alloc::format!("Show all {total_count} revisions ({hidden_count} hidden)")
                                                }}
                                            </button>
                                        </div>
                                    })}
                                </div>
                            }
                        })}
                    </div>

                    {if is_base_parent {
                        view! {
                            <span
                                class="pill-disabled"
                                title="Upstream rebase filtering is available when comparing two revisions"
                            >
                                "Rebase filter (interdiff only)"
                            </span>
                        }.into_any()
                    } else if is_rebase_diff.get() {
                        let (btn_class, label, title_text) = if rebase_conflict.get() {
                            (
                                "btn btn--rebase-conflict",
                                "Rebase conflict",
                                "Rebase conflict detected: showing full diff including upstream changes.",
                            )
                        } else if rebase_ignored.get() {
                            (
                                "btn btn--rebase-hidden",
                                "Rebase hidden",
                                "Upstream rebase changes are hidden. Click to show full diff.",
                            )
                        } else {
                            (
                                "btn btn--rebase-shown",
                                "Show rebase",
                                "Showing full diff including upstream changes. Click to hide rebase changes.",
                            )
                        };
                        view! {
                            <button
                                type="button"
                                class=btn_class
                                title=title_text
                                on:click=move |_| on_toggle_ignore_rebase.run(())
                            >
                                {label}
                            </button>
                        }.into_any()
                    } else {
                        view! {
                            <span
                                class="pill-disabled"
                                title="Compared revisions share the same parent commit"
                            >
                                "No rebase (same base)"
                            </span>
                        }.into_any()
                    }}

                    {move || active_target.get().map(|(_, c)| {
                        let cid = c.commit_id.clone();
                        let approved = c.approved;
                        let clean_rebase = c.clean_rebase_approved;
                        let (btn_class, label, title_text) = if approved {
                            (
                                "btn btn--approved",
                                String::from("Approved"),
                                String::from("Approved: click to revoke approval"),
                            )
                        } else if clean_rebase {
                            (
                                "btn btn--clean-rebase",
                                String::from("Approved (Clean Rebase)"),
                                match c.clean_rebase_source_commit_number {
                                    Some(n) => alloc::format!(
                                        "Approved (Clean Rebase from #{n}): click to upgrade to direct approval"
                                    ),
                                    None => String::from(
                                        "Approved (Clean Rebase): click to upgrade to direct approval"
                                    ),
                                },
                            )
                        } else {
                            (
                                "btn btn--approve",
                                String::from("Approve"),
                                String::from("Approve this revision"),
                            )
                        };

                        let disapprove = approved;
                        view! {
                            <button
                                type="button"
                                class=btn_class
                                title=title_text
                                on:click=move |_| on_toggle_approve.run((cid.clone(), disapprove))
                            >
                                {label}
                            </button>
                        }
                    })}
                </div>
            }.into_any()
        }}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn make_change(id: &str, commit: &str, wc: bool, empty: bool, desc: &str) -> JjChange {
        JjChange {
            change_id: String::from(id),
            short_change_id: id.chars().take(8).collect(),
            shortest_change_id: id.chars().take(1).collect(),
            commit_id: String::from(commit),
            short_commit_id: commit.chars().take(8).collect(),
            is_working_copy: wc,
            is_empty: empty,
            has_description: !desc.is_empty(),
            is_immutable: false,
            is_conflict: false,
            bookmarks: Vec::new(),
            parent_commit_ids: Vec::new(),
            author: String::from("dev"),
            timestamp: String::from("2026-11-17 10:00:00"),
            description: if desc.is_empty() {
                String::from("(no description set)")
            } else {
                String::from(desc)
            },
            comment_count: 0,
            unresolved_comment_count: 0,
            approved: false,
            clean_rebase_approved: false,
        }
    }

    #[test]
    fn pick_initial_change_skips_empty_undescribed_working_copy() {
        let mut wc = make_change("wc111111", "commit_wc_11", true, true, "");
        wc.parent_commit_ids = vec![String::from("commit_par_22")];
        let parent = make_change("par22222", "commit_par_22", false, false, "feat: real work");

        let changes = vec![wc, parent];
        let picked = pick_initial_change(&changes, Some("commit_wc_11")).unwrap();
        assert_eq!(picked.change_id, "par22222");
    }

    #[test]
    fn filter_interesting_commits_keeps_latest_approved_commented_and_selected() {
        let make_commit = |id: &str, approved: bool, comments: usize| JjCommit {
            commit_id: String::from(id),
            short_commit_id: id.chars().take(8).collect(),
            parent_commit_ids: Vec::new(),
            timestamp: String::from("2026-11-17 10:00:00"),
            description: String::from("msg"),
            is_working_copy: false,
            comment_count: comments,
            unresolved_comment_count: 0,
            approved,
            clean_rebase_approved: false,
            clean_rebase_source_commit_id: None,
            clean_rebase_source_commit_number: None,
        };

        let commits = vec![
            make_commit("111111aa", false, 0),
            make_commit("222222bb", true, 0),
            make_commit("333333cc", false, 0),
            make_commit("444444dd", false, 2),
            make_commit("555555ee", false, 0),
        ];

        let filtered = filter_interesting_commits(&commits, Some("555555ee"), Some("111111aa"));
        let numbers: Vec<usize> = filtered.iter().map(|c| c.commit_number).collect();
        assert_eq!(numbers, vec![5, 4, 2, 1]);
    }
}
