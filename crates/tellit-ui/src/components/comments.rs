//! Inline review comment composers, thread cards, unresolved jump menu, and drawer sidebar.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use leptos::ev;
use leptos::prelude::*;
use tellit_types::JjCommentThread;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{HtmlInputElement, HtmlTextAreaElement, KeyboardEvent, Node};

use crate::api;
use crate::state::{self, BodySegment};

/// Payload emitted when a user submits a comment composer form.
#[derive(Clone)]
pub struct CommentSubmitEvent {
    /// Trimmed Markdown comment body.
    pub body: String,
    /// Completion callback invoked with `Ok(())` on success or an error string on failure.
    pub on_done: Callback<Result<(), String>>,
}

/// Payload emitted when a user submits a reply inside an existing [`CommentThreadCard`].
#[derive(Clone)]
pub struct CommentReplyEvent {
    /// Target review thread UUID.
    pub thread_id: String,
    /// Trimmed reply body.
    pub body: String,
    /// Completion callback invoked with `Ok(())` on success or an error string on failure.
    pub on_done: Callback<Result<(), String>>,
}

#[component]
fn CommentBodyView(
    body: String,
    #[prop(default = None)] original_code: Option<String>,
) -> impl IntoView {
    let segments = state::parse_body_segments(&body);
    let orig_lines: Vec<String> = original_code
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|code| code.lines().map(str::to_owned).collect())
        .unwrap_or_default();

    view! {
        <div class="comment-body">
            {segments.into_iter().map(|seg| match seg {
                BodySegment::Text(text) => view! {
                    <p class="comment-body__text">{text}</p>
                }.into_any(),
                BodySegment::Suggestion(suggested) => {
                    let orig = orig_lines.clone();
                    let sugg_lines: Vec<String> = suggested.lines().map(str::to_owned).collect();
                    view! {
                        <div class="suggestion-block">
                            <div class="suggestion-block__header">"Suggested change"</div>
                            <div class="suggestion-block__code">
                                {orig.into_iter().map(|line| view! {
                                    <div class="suggestion-line suggestion-line--del">
                                        <span class="suggestion-line__sigil">"-"</span>
                                        <code>{line}</code>
                                    </div>
                                }).collect_view()}
                                {sugg_lines.into_iter().map(|line| view! {
                                    <div class="suggestion-line suggestion-line--add">
                                        <span class="suggestion-line__sigil">"+"</span>
                                        <code>{line}</code>
                                    </div>
                                }).collect_view()}
                            </div>
                        </div>
                    }.into_any()
                }
            }).collect_view()}
        </div>
    }
}

/// Inline comment or reply composer supporting suggestion insertion and `Cmd/Ctrl+Enter` submit.
#[component]
pub fn CommentComposer(
    #[prop(into)] title: String,
    #[prop(default = "Submit")] submit_label: &'static str,
    #[prop(default = "Leave a comment...")] placeholder: &'static str,
    #[prop(default = None)] selected_code: Option<String>,
    #[prop(default = false)] embedded: bool,
    #[prop(default = true)] autofocus: bool,
    on_submit: Callback<CommentSubmitEvent>,
    on_cancel: Callback<()>,
) -> impl IntoView {
    let draft = RwSignal::new(String::new());
    let submitting = RwSignal::new(false);
    let submit_error = RwSignal::new(Option::<String>::None);
    let selected_code_for_suggest = selected_code.clone();
    let textarea_ref = NodeRef::<leptos::html::Textarea>::new();
    let focused_once = StoredValue::new_local(false);

    // Focus once on mount after the triggering click/keydown finishes so typing starts immediately
    // without stealing focus on later reactive updates.
    Effect::new(move |_| {
        if !autofocus || focused_once.get_value() {
            return;
        }
        if let Some(el) = textarea_ref.get() {
            focused_once.set_value(true);
            leptos::task::spawn_local(async move {
                let _ = JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED)).await;
                if el.is_connected() {
                    let _ = el.focus();
                }
            });
        }
    });

    let trigger_submit = move || {
        if submitting.get_untracked() {
            return;
        }
        let body = draft.get_untracked().trim().to_string();
        if body.is_empty() {
            return;
        }
        submit_error.set(None);
        submitting.set(true);
        on_submit.run(CommentSubmitEvent {
            body,
            on_done: Callback::new(move |res: Result<(), String>| {
                submitting.try_set(false);
                match res {
                    Ok(()) => {
                        let _ = draft.try_set(String::new());
                    }
                    Err(err) => {
                        let _ = submit_error.try_set(Some(err));
                    }
                }
            }),
        });
    };

    let form_class = if embedded {
        "comment-form comment-form--embedded"
    } else {
        "comment-form"
    };

    view! {
        <form
            class=form_class
            on:submit=move |ev: ev::SubmitEvent| {
                ev.prevent_default();
                ev.stop_propagation();
                trigger_submit();
            }
            on:click=move |ev| ev.stop_propagation()
        >
            <div class="comment-form__header">
                <span class="comment-form__title">{title}</span>
                {selected_code_for_suggest.filter(|s| !s.is_empty()).map(|code| {
                    view! {
                        <button
                            type="button"
                            class="btn btn--ghost btn--xs"
                            title="Insert code suggestion block"
                            on:click=move |_| {
                                let snippet = alloc::format!("```suggestion\n{code}\n```");
                                draft.update(|cur| {
                                    if !cur.trim().is_empty() {
                                        cur.push_str("\n\n");
                                    }
                                    cur.push_str(&snippet);
                                });
                                if let Some(el) = textarea_ref.get_untracked() {
                                    let _ = el.focus();
                                }
                            }
                        >
                            "+ Suggestion"
                        </button>
                    }
                })}
            </div>

            <textarea
                node_ref=textarea_ref
                class="comment-form__textarea"
                rows=3
                placeholder=placeholder
                prop:value=move || draft.get()
                prop:disabled=move || submitting.get()
                on:input=move |ev| {
                    if let Some(target) = ev.target()
                        && let Ok(textarea) = target.dyn_into::<HtmlTextAreaElement>()
                    {
                        draft.set(textarea.value());
                        if submit_error.get_untracked().is_some() {
                            submit_error.set(None);
                        }
                    }
                }
                on:keydown=move |ev: KeyboardEvent| {
                    if ev.key() == "Enter" && (ev.meta_key() || ev.ctrl_key()) {
                        ev.prevent_default();
                        trigger_submit();
                    } else if ev.key() == "Escape" {
                        ev.prevent_default();
                        ev.stop_propagation();
                        on_cancel.run(());
                    }
                }
            ></textarea>

            {move || submit_error.get().map(|err| view! {
                <div class="inline-error">{err}</div>
            })}

            <div class="comment-form__actions">
                <span class="muted-label">"Ctrl/Cmd+Enter to submit"</span>
                <div class="comment-form__buttons">
                    <button
                        type="button"
                        class="btn btn--ghost"
                        prop:disabled=move || submitting.get()
                        on:click=move |_| on_cancel.run(())
                    >
                        "Cancel"
                    </button>
                    <button
                        type="submit"
                        class="btn btn--accent-sm"
                        prop:disabled=move || submitting.get() || draft.get().trim().is_empty()
                    >
                        {move || if submitting.get() { "Saving..." } else { submit_label }}
                    </button>
                </div>
            </div>
        </form>
    }
}

/// Interactive review thread card displaying root message, replies, resolve toggle, and prompt copy.
#[component]
pub fn CommentThreadCard(
    thread: JjCommentThread,
    #[prop(default = None)] code_snippet: Option<String>,
    on_reply: Callback<CommentReplyEvent>,
    on_toggle_resolve: Callback<(String, bool)>,
    #[prop(default = None)] on_jump: Option<Callback<JjCommentThread>>,
) -> impl IntoView {
    let collapsed = RwSignal::new(false);
    let replying = RwSignal::new(false);
    let copied = RwSignal::new(false);

    let thread_id = thread.id.clone();
    let dom_id = state::thread_element_id(&thread_id);
    let location = state::format_thread_location(&thread);
    let is_resolved = thread.resolved;
    let comments = thread.comments.clone();
    let msg_count = comments.len();
    let preview_line = comments
        .first()
        .and_then(|c| c.lines().next())
        .unwrap_or("")
        .to_owned();

    let thread_for_copy = thread.clone();
    let snippet_for_copy = code_snippet.clone();
    let thread_for_jump = thread.clone();

    let card_class = if is_resolved {
        "thread-card thread-card--resolved"
    } else {
        "thread-card thread-card--unresolved"
    };

    let tid_resolve = thread_id.clone();
    let tid_reply = thread_id;

    view! {
        <div
            id=dom_id
            class={if on_jump.is_some() {
                alloc::format!("{card_class} thread-card--clickable")
            } else {
                String::from(card_class)
            }}
            on:click=move |_| {
                if let Some(ref jump) = on_jump {
                    jump.run(thread_for_jump.clone());
                }
            }
        >
            <div class="thread-card__header">
                <div class="thread-card__meta">
                    <button
                        type="button"
                        class="icon-btn"
                        title=move || if collapsed.get() { "Expand thread" } else { "Collapse thread" }
                        on:click=move |ev| {
                            ev.stop_propagation();
                            collapsed.update(|v| *v = !*v);
                        }
                    >
                        {move || if collapsed.get() { "▸" } else { "▾" }}
                    </button>
                    <span class={if is_resolved {
                        "thread-loc thread-loc--resolved"
                    } else {
                        "thread-loc thread-loc--unresolved"
                    }}>
                        {location}
                    </span>
                    {is_resolved.then(|| view! {
                        <span class="pill-tag pill-tag--ok">"Resolved"</span>
                    })}
                    {move || collapsed.get().then(|| {
                        let preview = preview_line.clone();
                        view! {
                            <span class="thread-card__collapsed-preview">
                                {alloc::format!("{preview} ({msg_count})")}
                            </span>
                        }
                    })}
                </div>

                <div class="thread-card__controls" on:click=move |ev| ev.stop_propagation()>
                    <button
                        type="button"
                        class="btn btn--ghost btn--xs"
                        title="Copy thread prompt for AI agent"
                        on:click=move |_| {
                            let prompt = state::format_thread_prompt(
                                &thread_for_copy,
                                snippet_for_copy.as_deref(),
                            );
                            leptos::task::spawn_local(async move {
                                if api::copy_to_clipboard(&prompt).await.is_ok() {
                                    let _ = copied.try_set(true);
                                    schedule_flag_reset(copied, 1500);
                                }
                            });
                        }
                    >
                        {move || if copied.get() { "Copied!" } else { "Copy Prompt" }}
                    </button>

                    <button
                        type="button"
                        class={if is_resolved {
                            "btn btn--ghost btn--xs"
                        } else {
                            "btn btn--xs"
                        }}
                        title={if is_resolved { "Reopen thread" } else { "Resolve thread" }}
                        on:click=move |ev| {
                            ev.stop_propagation();
                            on_toggle_resolve.run((tid_resolve.clone(), is_resolved));
                        }
                    >
                        {if is_resolved { "Reopen" } else { "Resolve" }}
                    </button>
                </div>
            </div>

            {move || (!collapsed.get()).then(|| {
                let items = comments.clone();
                let root_snippet = code_snippet.clone();
                let tid = tid_reply.clone();
                view! {
                    <div class="thread-card__messages">
                        {items.into_iter().enumerate().map(|(idx, body)| {
                            let is_root = idx == 0;
                            let snippet = is_root.then(|| root_snippet.clone()).flatten();
                            view! {
                                <div class={if is_root {
                                    "thread-msg thread-msg--root"
                                } else {
                                    "thread-msg thread-msg--reply"
                                }}>
                                    {(!is_root).then(|| view! {
                                        <div class="thread-msg__label">{alloc::format!("Reply {idx}")}</div>
                                    })}
                                    <CommentBodyView body=body original_code=snippet />
                                </div>
                            }
                        }).collect_view()}

                        <div class="thread-card__reply-box" on:click=move |ev| ev.stop_propagation()>
                            {move || if replying.get() {
                                let tid_submit = tid.clone();
                                view! {
                                    <CommentComposer
                                        title="Reply to thread"
                                        submit_label="Reply"
                                        placeholder="Write a reply..."
                                        embedded=true
                                        on_submit=Callback::new(move |evt: CommentSubmitEvent| {
                                            let on_done = evt.on_done;
                                            let wrapped_done = Callback::new(move |res: Result<(), String>| {
                                                if res.is_ok() {
                                                    let _ = replying.try_set(false);
                                                }
                                                on_done.run(res);
                                            });
                                            on_reply.run(CommentReplyEvent {
                                                thread_id: tid_submit.clone(),
                                                body: evt.body,
                                                on_done: wrapped_done,
                                            });
                                        })
                                        on_cancel=Callback::new(move |()| replying.set(false))
                                    />
                                }.into_any()
                            } else {
                                view! {
                                    <button
                                        type="button"
                                        class="reply-trigger-btn"
                                        on:click=move |_| replying.set(true)
                                    >
                                        "Write a reply..."
                                    </button>
                                }.into_any()
                            }}
                        </div>
                    </div>
                }
            })}
        </div>
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrawerFilter {
    All,
    Open,
    Resolved,
}

/// Collapsible right-hand drawer listing all review threads with filtering and prompt copy.
#[component]
pub fn CommentsDrawer(
    #[prop(into)] is_open: Signal<bool>,
    #[prop(into)] threads: Signal<Vec<JjCommentThread>>,
    #[prop(into)] show_resolved_inline: Signal<bool>,
    #[prop(into)] target_commit_id: Signal<String>,
    #[prop(into)] base_commit_id: Signal<String>,
    on_close: Callback<()>,
    on_toggle_show_resolved_inline: Callback<bool>,
    on_jump_to_thread: Callback<JjCommentThread>,
    on_reply: Callback<CommentReplyEvent>,
    on_toggle_resolve: Callback<(String, bool)>,
) -> impl IntoView {
    let filter = RwSignal::new(DrawerFilter::All);
    let copied_all = RwSignal::new(false);

    let sorted_threads = Memo::new(move |_| sort_threads_for_drawer(&threads.get()));

    move || {
        is_open.get().then(|| {
            let all = sorted_threads.get();
            let total_count = all.len();
            let open_count = all.iter().filter(|t| !t.resolved).count();
            let resolved_count = total_count.saturating_sub(open_count);
            let active_filter = filter.get();

            let visible: Vec<JjCommentThread> = all
                .iter()
                .filter(|t| match active_filter {
                    DrawerFilter::All => true,
                    DrawerFilter::Open => !t.resolved,
                    DrawerFilter::Resolved => t.resolved,
                })
                .cloned()
                .collect();

            view! {
                <aside class="comments-drawer" aria-label="Comments panel">
                    <div class="comments-drawer__header">
                        <div class="comments-drawer__title">
                            <span>"Comments"</span>
                            <span class="muted-label">{alloc::format!("({total_count})")}</span>
                            {(open_count > 0).then(|| view! {
                                <span class="pill-tag pill-tag--warn">
                                    {alloc::format!("{open_count} open")}
                                </span>
                            })}
                        </div>
                        <div class="comments-drawer__actions">
                            {(total_count > 0).then(|| view! {
                                <button
                                    type="button"
                                    class="btn btn--ghost btn--xs"
                                    title="Copy all comments prompt for AI agent"
                                    on:click=move |_| {
                                        let prompt = state::format_all_comments_prompt(
                                            &threads.get_untracked(),
                                            &target_commit_id.get_untracked(),
                                            &base_commit_id.get_untracked(),
                                        );
                                        leptos::task::spawn_local(async move {
                                            if api::copy_to_clipboard(&prompt).await.is_ok() {
                                                let _ = copied_all.try_set(true);
                                                schedule_flag_reset(copied_all, 1500);
                                            }
                                        });
                                    }
                                >
                                    {move || if copied_all.get() { "Copied!" } else { "Copy All" }}
                                </button>
                            })}
                            <button
                                type="button"
                                class="icon-btn"
                                title="Close comments panel"
                                aria-label="Close comments panel"
                                on:click=move |_| on_close.run(())
                            >
                                "×"
                            </button>
                        </div>
                    </div>

                    <div class="comments-drawer__filters">
                        <div class="segmented">
                            <button
                                type="button"
                                class={if active_filter == DrawerFilter::All {
                                    "segmented__btn segmented__btn--active"
                                } else {
                                    "segmented__btn"
                                }}
                                on:click=move |_| filter.set(DrawerFilter::All)
                            >
                                {alloc::format!("All ({total_count})")}
                            </button>
                            <button
                                type="button"
                                class={if active_filter == DrawerFilter::Open {
                                    "segmented__btn segmented__btn--active"
                                } else {
                                    "segmented__btn"
                                }}
                                on:click=move |_| filter.set(DrawerFilter::Open)
                            >
                                {alloc::format!("Open ({open_count})")}
                            </button>
                            <button
                                type="button"
                                class={if active_filter == DrawerFilter::Resolved {
                                    "segmented__btn segmented__btn--active"
                                } else {
                                    "segmented__btn"
                                }}
                                on:click=move |_| filter.set(DrawerFilter::Resolved)
                            >
                                {alloc::format!("Resolved ({resolved_count})")}
                            </button>
                        </div>
                        <label class="viewed-toggle">
                            <input
                                type="checkbox"
                                prop:checked=move || show_resolved_inline.get()
                                on:change=move |ev| {
                                    if let Some(target) = ev.target()
                                        && let Ok(input) = target.dyn_into::<HtmlInputElement>()
                                    {
                                        on_toggle_show_resolved_inline.run(input.checked());
                                    }
                                }
                            />
                            <span>"Show resolved in diff"</span>
                        </label>
                    </div>

                    <div class="comments-drawer__list">
                        {if visible.is_empty() {
                            view! {
                                <div class="empty-inline-note">
                                    {if total_count == 0 {
                                        "No comments on this revision yet."
                                    } else {
                                        "No comments match the active filter."
                                    }}
                                </div>
                            }.into_any()
                        } else {
                            visible.into_iter().map(|thread| {
                                view! {
                                    <CommentThreadCard
                                        thread=thread
                                        on_reply=on_reply
                                        on_toggle_resolve=on_toggle_resolve
                                        on_jump=Some(on_jump_to_thread)
                                    />
                                }
                            }).collect_view().into_any()
                        }}
                    </div>
                </aside>
            }
        })
    }
}

/// Top-bar quick jump menu listing unresolved review threads for rapid navigation.
#[component]
pub fn UnresolvedJumpMenu(
    #[prop(into)] threads: Signal<Vec<JjCommentThread>>,
    on_jump_to_thread: Callback<JjCommentThread>,
    on_open_drawer: Callback<()>,
) -> impl IntoView {
    let is_open = RwSignal::new(false);
    let anchor_ref = NodeRef::<leptos::html::Div>::new();

    let listener = window_event_listener(ev::mousedown, move |ev| {
        if !is_open.get_untracked() {
            return;
        }
        let Some(el) = anchor_ref.get_untracked() else {
            return;
        };
        let node: &Node = el.as_ref();
        let inside = ev
            .target()
            .and_then(|t| t.dyn_into::<Node>().ok())
            .is_some_and(|target| node.contains(Some(&target)));
        if !inside {
            is_open.set(false);
        }
    });
    on_cleanup(move || listener.remove());

    view! {
        <div class="dropdown-anchor" node_ref=anchor_ref>
            <button
                type="button"
                class=move || {
                    let unresolved = threads.get().iter().filter(|t| !t.resolved).count();
                    if unresolved > 0 {
                        "btn btn--warn"
                    } else {
                        "btn btn--ghost"
                    }
                }
                title="Jump to unresolved review threads"
                on:click=move |_| is_open.update(|v| *v = !*v)
            >
                {move || {
                    let all = threads.get();
                    let unresolved = all.iter().filter(|t| !t.resolved).count();
                    let total = all.len();
                    if unresolved > 0 {
                        alloc::format!("{unresolved} unresolved")
                    } else if total > 0 {
                        alloc::format!("{total} resolved")
                    } else {
                        String::from("0 threads")
                    }
                }}
                <span class="chevron">"▾"</span>
            </button>

            {move || is_open.get().then(|| {
                let unresolved: Vec<JjCommentThread> = sort_threads_for_drawer(&threads.get())
                    .into_iter()
                    .filter(|t| !t.resolved)
                    .collect();

                view! {
                    <div class="dropdown-popover dropdown-popover--right">
                        <div class="dropdown-popover__header">
                            <span>"Unresolved Threads"</span>
                            <button
                                type="button"
                                class="btn btn--ghost btn--xs"
                                on:click=move |_| {
                                    is_open.set(false);
                                    on_open_drawer.run(());
                                }
                            >
                                "Open Comments"
                            </button>
                        </div>
                        <div class="dropdown-popover__list">
                            {if unresolved.is_empty() {
                                view! {
                                    <div class="empty-inline-note">"No unresolved threads."</div>
                                }.into_any()
                            } else {
                                unresolved.into_iter().map(|thread| {
                                    let loc = state::format_thread_location(&thread);
                                    let preview = thread
                                        .comments
                                        .first()
                                        .and_then(|c| c.lines().next())
                                        .unwrap_or("")
                                        .to_owned();
                                    let jump_thread = thread;
                                    view! {
                                        <button
                                            type="button"
                                            class="jump-thread-item"
                                            on:click=move |_| {
                                                is_open.set(false);
                                                on_jump_to_thread.run(jump_thread.clone());
                                            }
                                        >
                                            <span class="jump-thread-item__loc">{loc}</span>
                                            <span class="jump-thread-item__preview">{preview}</span>
                                        </button>
                                    }
                                }).collect_view().into_any()
                            }}
                        </div>
                    </div>
                }
            })}
        </div>
    }
}

/// Sorts threads with unresolved items first, then by file path, line start, and thread ID.
#[must_use]
pub fn sort_threads_for_drawer(threads: &[JjCommentThread]) -> Vec<JjCommentThread> {
    let mut sorted = threads.to_vec();
    sorted.sort_by(|a, b| {
        a.resolved
            .cmp(&b.resolved)
            .then_with(|| a.file_path.cmp(&b.file_path))
            .then_with(|| {
                let a_line = a.line_range.map_or(0, |r| r.start);
                let b_line = b.line_range.map_or(0, |r| r.start);
                a_line.cmp(&b_line)
            })
            .then_with(|| a.id.cmp(&b.id))
    });
    sorted
}

fn schedule_flag_reset(flag: RwSignal<bool>, delay_ms: i32) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, delay_ms);
    });
    leptos::task::spawn_local(async move {
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
        let _ = flag.try_set(false);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tellit_types::{CommentSide, LineRangeSpec};

    #[test]
    fn parses_prose_and_suggestion_blocks() {
        let parsed = state::parse_body_segments("Before\n```suggestion\nlet a = 2;\n```\nAfter");
        assert_eq!(
            parsed,
            vec![
                BodySegment::Text(String::from("Before")),
                BodySegment::Suggestion(String::from("let a = 2;")),
                BodySegment::Text(String::from("After")),
            ]
        );
    }

    #[test]
    fn sorts_unresolved_threads_before_resolved() {
        let resolved = JjCommentThread {
            id: String::from("1"),
            change_id: String::from("c"),
            commit_id: String::from("k1"),
            file_path: String::from("a.rs"),
            side: CommentSide::New,
            line_range: Some(LineRangeSpec { start: 1, end: 1 }),
            comments: alloc::vec![String::from("ok")],
            resolved: true,
        };
        let open = JjCommentThread {
            id: String::from("2"),
            change_id: String::from("c"),
            commit_id: String::from("k1"),
            file_path: String::from("z.rs"),
            side: CommentSide::New,
            line_range: Some(LineRangeSpec { start: 5, end: 5 }),
            comments: alloc::vec![String::from("fix")],
            resolved: false,
        };

        let sorted = sort_threads_for_drawer(&[resolved, open]);
        assert_eq!(sorted[0].id, "2");
        assert_eq!(sorted[1].id, "1");
    }
}
