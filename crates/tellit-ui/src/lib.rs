//! Leptos client-side WebAssembly frontend for `tellit`.
//!
//! Renders the Jujutsu DAG revision sidebar, top-bar change and evolog comparison pickers,
//! rebase-isolation toggle, approval button, changed-files tree, and live `/api/watch` updates.
//!
//! # Examples
//!
//! ```
//! use tellit_types::{HeartbeatEvent, JjStatusResponse};
//! use tellit_ui::{ConnectionState, RepoStatusState};
//!
//! let conn = ConnectionState::Connecting.on_heartbeat(HeartbeatEvent::Connected);
//! assert_eq!(conn, ConnectionState::Live);
//! assert_eq!(conn.label(), "Live");
//!
//! let repo = RepoStatusState::Loaded(JjStatusResponse::jj("/workspace/tellit"));
//! assert_eq!(repo.badge_label(), "jj workspace");
//! ```

extern crate alloc;

mod api;
mod components;
mod state;

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use std::collections::{HashMap, HashSet};

use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;
use tellit_types::{
    AppearanceSettings, CommentSide, CreateCommentRequest, DiffFile, FileStatus, JjChange,
    JjCommentThread, JjCommit, JjDiffResponse, JjGraphRow, ThemePreference, WatchEvent,
};
use web_sys::{KeyboardEvent, MouseEvent, ScrollLogicalPosition};

use components::comments::{CommentReplyEvent, CommentsDrawer, UnresolvedJumpMenu};
use components::file_list::FileList;
use components::help_modal::HelpModal;
use components::keyboard::{self, CursorPosition, NavigationDirection, NavigationTarget};
use components::review_dot::ReviewDot;
use components::revision_selector::{
    JjChangeSelector, JjCommitComparisonSelector, commit_ids_match, pick_initial_change,
};
use components::revision_tree::JjRevisionTreePanel;
pub use components::revision_tree::{
    GraphCellLayout, GraphNodeKind, GraphRowLayout, compute_graph_layout,
};
use components::settings_modal::SettingsModal;
use components::text_diff::expanded::{
    ExpandedLinesMap, record_expand_all_before, record_line_expansion, split_blob_lines,
};
use components::text_diff::{
    CreateThreadEvent, ExpandLinesEvent, KeyboardCommentTrigger, TextDiffCard,
};
pub use state::{ConnectionState, DiffViewMode, RepoStatusState, ResolvedTheme};

#[derive(Clone, Copy)]
struct AppCtx {
    repo_status: RwSignal<RepoStatusState>,
    connection: RwSignal<ConnectionState>,
    watch_connected: RwSignal<bool>,
    reloading: RwSignal<bool>,
    changes: RwSignal<Vec<JjChange>>,
    graph_rows: RwSignal<Vec<JjGraphRow>>,
    loading_changes: RwSignal<bool>,
    selected_change_id: RwSignal<String>,
    commits: RwSignal<Vec<JjCommit>>,
    loading_commits: RwSignal<bool>,
    target_commit_id: RwSignal<String>,
    base_commit_id: RwSignal<String>,
    ignore_rebase: RwSignal<bool>,
    diff_data: RwSignal<Option<JjDiffResponse>>,
    loading_diff: RwSignal<bool>,
    comments: RwSignal<Vec<JjCommentThread>>,
    error_banner: RwSignal<Option<String>>,
    lookup_error: RwSignal<Option<String>>,
    reviewed_files: RwSignal<HashSet<String>>,
    collapsed_files: RwSignal<HashSet<String>>,
    selected_file_path: RwSignal<Option<String>>,
    diff_mode: RwSignal<DiffViewMode>,
    show_resolved_inline: RwSignal<bool>,
    drawer_open: RwSignal<bool>,
    highlighted_word: RwSignal<Option<String>>,
    expanded_lines: RwSignal<ExpandedLinesMap>,
    expand_loading: RwSignal<bool>,
    appearance: RwSignal<AppearanceSettings>,
    resolved_theme: RwSignal<ResolvedTheme>,
    settings_open: RwSignal<bool>,
    help_open: RwSignal<bool>,
    active_cursor: RwSignal<Option<CursorPosition>>,
    hovered_file_index: RwSignal<Option<usize>>,
    keyboard_comment_trigger: RwSignal<Option<KeyboardCommentTrigger>>,
    keyboard_comment_seq: StoredValue<u64, LocalStorage>,
    unresolved_jump_cursor: StoredValue<usize, LocalStorage>,
    changes_req_id: StoredValue<u64, LocalStorage>,
    commits_req_id: StoredValue<u64, LocalStorage>,
    diff_req_id: StoredValue<u64, LocalStorage>,
    comments_req_id: StoredValue<u64, LocalStorage>,
    diff_generation: StoredValue<u64, LocalStorage>,
    prefetched_line_counts: StoredValue<HashSet<String>, LocalStorage>,
    pending_initial_target: StoredValue<Option<String>, LocalStorage>,
    pending_initial_base: StoredValue<Option<String>, LocalStorage>,
}

impl AppCtx {
    fn new() -> Self {
        let appearance_init = state::load_appearance_settings();
        let resolved_init = state::apply_appearance_to_document(&appearance_init);
        Self {
            repo_status: RwSignal::new(RepoStatusState::Loading),
            connection: RwSignal::new(ConnectionState::Connecting),
            watch_connected: RwSignal::new(false),
            reloading: RwSignal::new(false),
            changes: RwSignal::new(Vec::new()),
            graph_rows: RwSignal::new(Vec::new()),
            loading_changes: RwSignal::new(true),
            selected_change_id: RwSignal::new(String::new()),
            commits: RwSignal::new(Vec::new()),
            loading_commits: RwSignal::new(false),
            target_commit_id: RwSignal::new(String::new()),
            base_commit_id: RwSignal::new(String::from("parent")),
            ignore_rebase: RwSignal::new(true),
            diff_data: RwSignal::new(None),
            loading_diff: RwSignal::new(false),
            comments: RwSignal::new(Vec::new()),
            error_banner: RwSignal::new(None),
            lookup_error: RwSignal::new(None),
            reviewed_files: RwSignal::new(HashSet::new()),
            collapsed_files: RwSignal::new(HashSet::new()),
            selected_file_path: RwSignal::new(None),
            diff_mode: RwSignal::new(state::load_diff_view_mode()),
            show_resolved_inline: RwSignal::new(state::load_storage_bool(
                state::SHOW_RESOLVED_INLINE_STORAGE_KEY,
                true,
            )),
            drawer_open: RwSignal::new(false),
            highlighted_word: RwSignal::new(None),
            expanded_lines: RwSignal::new(HashMap::new()),
            expand_loading: RwSignal::new(false),
            appearance: RwSignal::new(appearance_init),
            resolved_theme: RwSignal::new(resolved_init),
            settings_open: RwSignal::new(false),
            help_open: RwSignal::new(false),
            active_cursor: RwSignal::new(None),
            hovered_file_index: RwSignal::new(None),
            keyboard_comment_trigger: RwSignal::new(None),
            keyboard_comment_seq: StoredValue::new_local(0),
            unresolved_jump_cursor: StoredValue::new_local(0),
            changes_req_id: StoredValue::new_local(0),
            commits_req_id: StoredValue::new_local(0),
            diff_req_id: StoredValue::new_local(0),
            comments_req_id: StoredValue::new_local(0),
            diff_generation: StoredValue::new_local(0),
            prefetched_line_counts: StoredValue::new_local(HashSet::new()),
            pending_initial_target: StoredValue::new_local(None),
            pending_initial_base: StoredValue::new_local(None),
        }
    }

    fn repo_root_string(&self) -> String {
        match self.repo_status.get_untracked() {
            RepoStatusState::Loaded(status) => status.repo_root.unwrap_or_default(),
            _ => String::new(),
        }
    }
}

fn next_req_id(slot: StoredValue<u64, LocalStorage>) -> u64 {
    let mut next = 0u64;
    slot.update_value(|id| {
        *id += 1;
        next = *id;
    });
    next
}

fn load_diff_and_comments(ctx: AppCtx, target: String, base: Option<String>, ignore_rebase: bool) {
    let diff_id = next_req_id(ctx.diff_req_id);
    let comments_id = next_req_id(ctx.comments_req_id);
    ctx.loading_diff.set(true);

    let target_for_diff = target.clone();
    let base_for_diff = base.clone();
    spawn_local(async move {
        match api::fetch_jj_diff(&target_for_diff, base_for_diff.as_deref(), ignore_rebase).await {
            Ok(diff) => {
                if ctx.diff_req_id.get_value() != diff_id {
                    return;
                }
                let _ = next_req_id(ctx.diff_generation);
                ctx.expanded_lines.set(HashMap::new());
                ctx.prefetched_line_counts.update_value(HashSet::clear);
                ctx.highlighted_word.set(None);
                ctx.expand_loading.set(false);
                ctx.active_cursor.set(None);

                let repo_root = ctx.repo_root_string();
                let mut viewed = state::load_viewed_files(&repo_root, &diff.commit);
                let auto_patterns = ctx.appearance.get_untracked().auto_viewed_patterns;
                if !auto_patterns.is_empty() {
                    for file in &diff.files {
                        if file.path != "Commit Message"
                            && state::matches_auto_viewed_patterns(&file.path, &auto_patterns)
                        {
                            viewed.insert(file.path.clone());
                        }
                    }
                }
                let mut collapsed: HashSet<String> = viewed.clone();
                for file in &diff.files {
                    if file.is_generated {
                        collapsed.insert(file.path.clone());
                    }
                }
                ctx.reviewed_files.set(viewed);
                ctx.collapsed_files.set(collapsed);
                ctx.diff_data.set(Some(diff));
                ctx.loading_diff.set(false);
                ctx.error_banner.set(None);
            }
            Err(err) => {
                if ctx.diff_req_id.get_value() != diff_id {
                    return;
                }
                ctx.loading_diff.set(false);
                ctx.error_banner.set(Some(err.to_string()));
            }
        }
    });

    spawn_local(async move {
        if let Ok(resp) = api::fetch_jj_comments(&target, base.as_deref()).await
            && ctx.comments_req_id.get_value() == comments_id
        {
            ctx.comments.set(resp.threads);
        }
    });
}

fn refresh_comments_only(ctx: AppCtx) {
    let target = ctx.target_commit_id.get_untracked();
    if target.is_empty() {
        return;
    }
    let base = ctx.base_commit_id.get_untracked();
    let base_opt = (base != "parent" && !base.is_empty()).then_some(base);
    let comments_id = next_req_id(ctx.comments_req_id);
    spawn_local(async move {
        if let Ok(resp) = api::fetch_jj_comments(&target, base_opt.as_deref()).await
            && ctx.comments_req_id.get_value() == comments_id
        {
            ctx.comments.set(resp.threads);
        }
    });
}

fn select_change(ctx: AppCtx, change_id: String) {
    if change_id.is_empty() {
        return;
    }
    ctx.selected_change_id.set(change_id.clone());
    ctx.lookup_error.set(None);

    let immediate_target = ctx
        .changes
        .get_untracked()
        .into_iter()
        .find(|c| c.change_id == change_id)
        .map(|c| c.commit_id);

    let initial_base = ctx.pending_initial_base.get_value();
    let initial_target = ctx.pending_initial_target.get_value();
    ctx.pending_initial_base.set_value(None);
    ctx.pending_initial_target.set_value(None);

    let base_for_fetch = initial_base.clone();
    ctx.base_commit_id.set(
        base_for_fetch
            .clone()
            .unwrap_or_else(|| String::from("parent")),
    );

    if let Some(ref tid) = immediate_target {
        let initial_pick = initial_target.clone().unwrap_or_else(|| tid.clone());
        ctx.target_commit_id.set(initial_pick.clone());
        load_diff_and_comments(
            ctx,
            initial_pick,
            base_for_fetch.clone(),
            ctx.ignore_rebase.get_untracked(),
        );
    }

    let req_id = next_req_id(ctx.commits_req_id);
    ctx.loading_commits.set(true);
    spawn_local(async move {
        match api::fetch_jj_commits(&change_id).await {
            Ok(resp) => {
                if ctx.commits_req_id.get_value() != req_id
                    || ctx.selected_change_id.get_untracked() != change_id
                {
                    return;
                }
                let fetched = resp.commits;
                ctx.loading_commits.set(false);

                let desired_target = initial_target
                    .as_deref()
                    .and_then(|init| {
                        fetched
                            .iter()
                            .find(|c| commit_ids_match(&c.commit_id, init))
                            .map(|c| c.commit_id.clone())
                    })
                    .or_else(|| {
                        immediate_target.as_deref().and_then(|head| {
                            fetched
                                .iter()
                                .find(|c| commit_ids_match(&c.commit_id, head))
                                .map(|c| c.commit_id.clone())
                        })
                    })
                    .or_else(|| fetched.last().map(|c| c.commit_id.clone()));

                ctx.commits.set(fetched);

                if let Some(selected_target) = desired_target {
                    let current = ctx.target_commit_id.get_untracked();
                    if current.is_empty() || !commit_ids_match(&current, &selected_target) {
                        ctx.target_commit_id.set(selected_target.clone());
                        load_diff_and_comments(
                            ctx,
                            selected_target,
                            base_for_fetch,
                            ctx.ignore_rebase.get_untracked(),
                        );
                    }
                }
            }
            Err(err) => {
                if ctx.commits_req_id.get_value() == req_id {
                    ctx.loading_commits.set(false);
                    ctx.error_banner.set(Some(err.to_string()));
                }
            }
        }
    });
}

async fn resolve_initial_change_into_response(
    resp: &mut tellit_types::JjChangesResponse,
    initial_target: Option<&str>,
) -> Option<String> {
    let mut picked_id =
        pick_initial_change(&resp.changes, initial_target).map(|c| c.change_id.clone());

    // If `tellit launch <rev>` pointed at a revision outside the default `jj log` graph,
    // look it up explicitly so it appears in the tree and becomes active.
    if let Some(init_tid) = initial_target
        && !resp
            .changes
            .iter()
            .any(|c| commit_ids_match(&c.commit_id, init_tid))
        && let Ok(extra) = api::fetch_jj_changes(Some(init_tid)).await
        && let Some(found) = extra.changes.into_iter().next()
    {
        let cid = found.change_id.clone();
        if !resp.changes.iter().any(|c| c.change_id == cid) {
            resp.graph_rows.insert(0, JjGraphRow::standalone(&found));
            resp.changes.insert(0, found);
        }
        picked_id = Some(cid);
    }

    picked_id
}

fn load_initial_jj_data(ctx: AppCtx) {
    let changes_id = next_req_id(ctx.changes_req_id);
    ctx.loading_changes.set(true);
    spawn_local(async move {
        let initial_target = ctx.pending_initial_target.get_value();
        match api::fetch_jj_changes(None).await {
            Ok(mut resp) => {
                if ctx.changes_req_id.get_value() != changes_id {
                    return;
                }
                let picked_id =
                    resolve_initial_change_into_response(&mut resp, initial_target.as_deref())
                        .await;
                if ctx.changes_req_id.get_value() != changes_id {
                    return;
                }

                ctx.changes.set(resp.changes);
                ctx.graph_rows.set(resp.graph_rows);
                ctx.loading_changes.set(false);

                if let Some(cid) = picked_id
                    && ctx.selected_change_id.get_untracked().is_empty()
                {
                    select_change(ctx, cid);
                }
            }
            Err(err) => {
                if ctx.changes_req_id.get_value() != changes_id {
                    return;
                }
                ctx.loading_changes.set(false);
                ctx.error_banner.set(Some(err.to_string()));
            }
        }
    });
}

fn reload_changes_and_commits(ctx: AppCtx, reload_diff: bool) {
    let changes_id = next_req_id(ctx.changes_req_id);
    let initial_selected = ctx.selected_change_id.get_untracked();
    let old_head_commit = ctx
        .changes
        .get_untracked()
        .into_iter()
        .find(|c| c.change_id == initial_selected)
        .map(|c| c.commit_id);

    ctx.reloading.set(true);
    spawn_local(async move {
        let Ok(mut resp) = api::fetch_jj_changes(None).await else {
            if ctx.changes_req_id.get_value() == changes_id {
                ctx.reloading.set(false);
                ctx.loading_changes.set(false);
            }
            return;
        };

        if ctx.changes_req_id.get_value() != changes_id {
            return;
        }

        // If a watch reload preempted `load_initial_jj_data` before any change was selected,
        // finish initial selection here so the UI never stalls on "Loading Jujutsu revisions...".
        let current_selected = ctx.selected_change_id.get_untracked();
        if current_selected.is_empty() {
            let initial_target = ctx.pending_initial_target.get_value();
            let picked_id =
                resolve_initial_change_into_response(&mut resp, initial_target.as_deref()).await;
            if ctx.changes_req_id.get_value() != changes_id {
                return;
            }
            ctx.changes.set(resp.changes);
            ctx.graph_rows.set(resp.graph_rows);
            ctx.loading_changes.set(false);
            ctx.reloading.set(false);
            if let Some(cid) = picked_id
                && ctx.selected_change_id.get_untracked().is_empty()
            {
                select_change(ctx, cid);
            }
            return;
        }

        let new_head_commit = resp
            .changes
            .iter()
            .find(|c| c.change_id == initial_selected)
            .map(|c| c.commit_id.clone());

        ctx.changes.set(resp.changes);
        ctx.graph_rows.set(resp.graph_rows);
        ctx.loading_changes.set(false);
        ctx.reloading.set(false);

        // If the user switched to a different change while `fetch_jj_changes` was in flight,
        // `select_change` already kicked off the newer commit and diff fetches.
        if current_selected != initial_selected {
            return;
        }

        if !current_selected.is_empty() {
            let req_id = next_req_id(ctx.commits_req_id);
            if let Ok(commits_resp) = api::fetch_jj_commits(&current_selected).await
                && ctx.commits_req_id.get_value() == req_id
                && ctx.selected_change_id.get_untracked() == current_selected
            {
                ctx.commits.set(commits_resp.commits);
            }
        }

        if ctx.changes_req_id.get_value() != changes_id
            || ctx.selected_change_id.get_untracked() != initial_selected
        {
            return;
        }

        if reload_diff {
            let mut target = ctx.target_commit_id.get_untracked();
            let base = ctx.base_commit_id.get_untracked();
            // If the user was inspecting the active head of this change and a jj op rewrote it,
            // follow the new head commit automatically.
            if let (Some(old_head), Some(new_head)) =
                (old_head_commit.as_deref(), new_head_commit.as_deref())
                && commit_ids_match(&target, old_head)
                && !commit_ids_match(old_head, new_head)
            {
                target = String::from(new_head);
                ctx.target_commit_id.set(target.clone());
            }
            if !target.is_empty() {
                let base_opt = (base != "parent" && !base.is_empty()).then_some(base);
                load_diff_and_comments(ctx, target, base_opt, ctx.ignore_rebase.get_untracked());
            }
        } else {
            refresh_comments_only(ctx);
        }
    });
}

fn lookup_and_select_query(ctx: AppCtx, query: String) {
    ctx.lookup_error.set(None);
    spawn_local(async move {
        match api::fetch_jj_changes(Some(&query)).await {
            Ok(resp) => {
                let Some(found) = resp.changes.into_iter().next() else {
                    ctx.lookup_error.set(Some(String::from("Not found")));
                    return;
                };
                let cid = found.change_id.clone();
                ctx.changes.update(|list| {
                    if !list.iter().any(|c| c.change_id == cid) {
                        list.insert(0, found.clone());
                    }
                });
                ctx.graph_rows.update(|rows| {
                    let exists = rows.iter().any(|r| {
                        matches!(r, JjGraphRow::Commit { change, .. } if change.change_id == cid)
                    });
                    if !exists {
                        rows.insert(0, JjGraphRow::standalone(&found));
                    }
                });
                select_change(ctx, cid);
            }
            Err(_) => ctx.lookup_error.set(Some(String::from("Query failed"))),
        }
    });
}

fn load_jj_status(ctx: AppCtx) {
    ctx.repo_status.set(RepoStatusState::Loading);
    spawn_local(async move {
        match api::fetch_jj_status().await {
            Ok(status) => {
                let is_jj = status.is_jj;
                ctx.pending_initial_target
                    .set_value(status.initial_target.clone());
                ctx.pending_initial_base
                    .set_value(status.initial_base.clone());
                ctx.repo_status.set(RepoStatusState::Loaded(status));
                if is_jj {
                    load_initial_jj_data(ctx);
                } else {
                    ctx.loading_changes.set(false);
                }
            }
            Err(err) => {
                ctx.loading_changes.set(false);
                ctx.repo_status.set(RepoStatusState::Error(err.to_string()));
            }
        }
    });
}

fn connect_heartbeat(
    connection: RwSignal<ConnectionState>,
    heartbeat_handle: StoredValue<Option<api::SseSubscription>, LocalStorage>,
) {
    heartbeat_handle.set_value(None);
    connection.set(ConnectionState::Connecting);
    match api::subscribe_heartbeat(
        move |event| connection.update(|state| *state = state.on_heartbeat(event)),
        move || connection.set(ConnectionState::Offline),
    ) {
        Ok(sub) => heartbeat_handle.set_value(Some(sub)),
        Err(_) => connection.set(ConnectionState::Offline),
    }
}

fn connect_watch(
    ctx: AppCtx,
    watch_handle: StoredValue<Option<api::SseSubscription>, LocalStorage>,
) {
    watch_handle.set_value(None);
    match api::subscribe_watch(
        move |event| match event {
            WatchEvent::Connected => ctx.watch_connected.set(true),
            WatchEvent::Reload { .. } => reload_changes_and_commits(ctx, true),
            WatchEvent::CommentsChanged => reload_changes_and_commits(ctx, false),
        },
        move || ctx.watch_connected.set(false),
    ) {
        Ok(sub) => watch_handle.set_value(Some(sub)),
        Err(_) => ctx.watch_connected.set(false),
    }
}

fn scroll_to_file_element(path: &str) {
    let _ =
        keyboard::scroll_to_element_id(&state::file_element_id(path), ScrollLogicalPosition::Start);
}

fn jump_to_thread(ctx: AppCtx, thread: JjCommentThread) {
    ctx.selected_file_path.set(Some(thread.file_path.clone()));
    ctx.collapsed_files.update(|set| {
        set.remove(&thread.file_path);
    });

    let thread_dom = state::thread_element_id(&thread.id);
    let file_dom = state::file_element_id(&thread.file_path);
    let do_scroll = move || {
        if !keyboard::scroll_to_element_id(&thread_dom, ScrollLogicalPosition::Center) {
            let _ = keyboard::scroll_to_element_id(&file_dom, ScrollLogicalPosition::Start);
        }
    };
    if let Some(window) = web_sys::window() {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 0);
        });
        spawn_local(async move {
            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
            do_scroll();
        });
    } else {
        do_scroll();
    }
}

fn prefetch_line_count_for_file(ctx: AppCtx, file: Arc<DiffFile>) {
    if ctx
        .prefetched_line_counts
        .with_value(|s| s.contains(&file.path))
    {
        return;
    }
    let Some(diff) = ctx.diff_data.get_untracked() else {
        return;
    };
    let generation = ctx.diff_generation.get_value();
    let path = file.path.clone();
    ctx.prefetched_line_counts.update_value(|s| {
        s.insert(path.clone());
    });

    let old_ref = (file.status != FileStatus::Added).then(|| diff.base_commit.clone());
    let new_ref = (file.status != FileStatus::Deleted).then(|| diff.commit.clone());
    let old_path = file.old_path.clone();

    spawn_local(async move {
        let Ok(counts) = api::fetch_line_count(
            &path,
            old_ref.as_deref(),
            new_ref.as_deref(),
            old_path.as_deref(),
        )
        .await
        else {
            return;
        };
        if ctx.diff_generation.get_value() != generation {
            return;
        }
        ctx.expanded_lines.update(|map| {
            let entry = map.entry(path).or_default();
            if entry.old_total_lines.is_none() {
                entry.old_total_lines = counts.old_line_count.and_then(|n| u32::try_from(n).ok());
            }
            if entry.new_total_lines.is_none() {
                entry.new_total_lines = counts.new_line_count.and_then(|n| u32::try_from(n).ok());
            }
        });
    });
}

fn expand_lines_for_file(ctx: AppCtx, req: ExpandLinesEvent) {
    let Some(diff) = ctx.diff_data.get_untracked() else {
        return;
    };
    let generation = ctx.diff_generation.get_value();
    ctx.expand_loading.set(true);

    spawn_local(async move {
        let path = req.file.path.clone();
        let old_path = req.file.old_path.as_deref().unwrap_or(&path).to_owned();

        let existing = ctx.expanded_lines.get_untracked().get(&path).cloned();
        let need_old = req.file.status != FileStatus::Added
            && existing.as_ref().is_none_or(|e| e.old_content.is_none());
        let need_new = req.file.status != FileStatus::Deleted
            && existing.as_ref().is_none_or(|e| e.new_content.is_none());

        let old_lines = if need_old {
            match api::fetch_blob_text(&old_path, &diff.base_commit).await {
                Ok(text) => Some(split_blob_lines(&text)),
                Err(_) => Some(Vec::new()),
            }
        } else {
            None
        };

        let new_lines = if need_new {
            match api::fetch_blob_text(&path, &diff.commit).await {
                Ok(text) => Some(split_blob_lines(&text)),
                Err(_) => Some(Vec::new()),
            }
        } else {
            None
        };

        if ctx.diff_generation.get_value() != generation {
            ctx.expand_loading.set(false);
            return;
        }

        ctx.expanded_lines.update(|map| {
            let entry = map.entry(path.clone()).or_default();
            if let Some(lines) = old_lines {
                entry.old_total_lines = Some(u32::try_from(lines.len()).unwrap_or(u32::MAX));
                entry.old_content = Some(lines);
            }
            if let Some(lines) = new_lines {
                entry.new_total_lines = Some(u32::try_from(lines.len()).unwrap_or(u32::MAX));
                entry.new_content = Some(lines);
            }
            match req.direction {
                Some(dir) => record_line_expansion(map, &path, req.chunk_index, dir, req.count),
                None => record_expand_all_before(map, &path, req.chunk_index, req.count),
            }
        });
        ctx.expand_loading.set(false);
    });
}

/// Root Leptos component for the `tellit` Jujutsu review application.
#[component]
pub fn App() -> impl IntoView {
    let ctx = AppCtx::new();
    let heartbeat_handle = StoredValue::new_local(None::<api::SseSubscription>);
    let watch_handle = StoredValue::new_local(None::<api::SseSubscription>);

    let sidebar_open = RwSignal::new(state::load_storage_bool(
        state::SIDEBAR_OPEN_STORAGE_KEY,
        true,
    ));
    let sidebar_width = RwSignal::new(state::clamp_sidebar_width(state::load_storage_u32(
        state::SIDEBAR_WIDTH_STORAGE_KEY,
        state::SIDEBAR_DEFAULT_WIDTH,
    )));
    let is_resizing = RwSignal::new(false);
    let drag_origin = RwSignal::new((0i32, 300u32));

    Effect::new(move |_| {
        load_jj_status(ctx);
        connect_heartbeat(ctx.connection, heartbeat_handle);
        connect_watch(ctx, watch_handle);
        spawn_local(async move {
            if let Ok(Some(remote)) = api::fetch_user_settings().await {
                let normalized = remote.normalized();
                ctx.appearance.set(normalized.clone());
                ctx.resolved_theme
                    .set(state::apply_appearance_to_document(&normalized));
                state::save_appearance_settings(&normalized);
            }
        });
    });

    let apply_settings_update = Callback::new(move |next: AppearanceSettings| {
        let normalized = next.normalized();
        ctx.appearance.set(normalized.clone());
        ctx.resolved_theme
            .set(state::apply_appearance_to_document(&normalized));
        state::save_appearance_settings(&normalized);
        spawn_local(async move {
            let _ = api::save_user_settings(&normalized).await;
        });
    });

    let mousemove_handle = window_event_listener(ev::mousemove, move |ev: MouseEvent| {
        if !is_resizing.get_untracked() {
            return;
        }
        let (start_x, start_w) = drag_origin.get_untracked();
        let delta = ev.client_x() - start_x;
        let next_w = i64::from(start_w).saturating_add(i64::from(delta)).max(0);
        let clamped = state::clamp_sidebar_width(u32::try_from(next_w).unwrap_or(u32::MAX));
        sidebar_width.set(clamped);
    });

    let mouseup_handle = window_event_listener(ev::mouseup, move |_ev: MouseEvent| {
        if is_resizing.get_untracked() {
            is_resizing.set(false);
            state::save_storage_u32(
                state::SIDEBAR_WIDTH_STORAGE_KEY,
                sidebar_width.get_untracked(),
            );
        }
    });

    let keydown_handle = window_event_listener(ev::keydown, move |ev: KeyboardEvent| {
        handle_global_keydown(ctx, &ev);
    });

    on_cleanup(move || {
        heartbeat_handle.set_value(None);
        watch_handle.set_value(None);
        mousemove_handle.remove();
        mouseup_handle.remove();
        keydown_handle.remove();
    });

    let on_toggle_sidebar = move |_| {
        let next = !sidebar_open.get_untracked();
        sidebar_open.set(next);
        state::save_storage_bool(state::SIDEBAR_OPEN_STORAGE_KEY, next);
    };

    let on_select_change = Callback::new(move |cid: String| select_change(ctx, cid));
    let on_lookup_query = Callback::new(move |query: String| lookup_and_select_query(ctx, query));

    let on_select_target = Callback::new(move |new_target: String| {
        ctx.target_commit_id.set(new_target.clone());
        let base = ctx.base_commit_id.get_untracked();
        let base_opt = (base != "parent" && !base.is_empty()).then_some(base);
        load_diff_and_comments(ctx, new_target, base_opt, ctx.ignore_rebase.get_untracked());
    });

    let on_select_base = Callback::new(move |new_base: String| {
        ctx.base_commit_id.set(new_base.clone());
        let target = ctx.target_commit_id.get_untracked();
        if !target.is_empty() {
            let base_opt = (new_base != "parent" && !new_base.is_empty()).then_some(new_base);
            load_diff_and_comments(ctx, target, base_opt, ctx.ignore_rebase.get_untracked());
        }
    });

    let on_toggle_ignore_rebase = Callback::new(move |()| {
        let next = !ctx.ignore_rebase.get_untracked();
        ctx.ignore_rebase.set(next);
        let target = ctx.target_commit_id.get_untracked();
        if !target.is_empty() {
            let base = ctx.base_commit_id.get_untracked();
            let base_opt = (base != "parent" && !base.is_empty()).then_some(base);
            load_diff_and_comments(ctx, target, base_opt, next);
        }
    });

    let on_toggle_approve = Callback::new(move |(commit_id, disapprove): (String, bool)| {
        spawn_local(async move {
            match api::post_jj_approve(&commit_id, disapprove).await {
                Ok(_) => reload_changes_and_commits(ctx, false),
                Err(err) => ctx.error_banner.set(Some(err.to_string())),
            }
        });
    });

    let on_toggle_reviewed = Callback::new(move |path: String| {
        let Some(diff) = ctx.diff_data.get_untracked() else {
            return;
        };
        toggle_file_viewed_and_collapse(ctx, &diff.commit, &path);
    });

    let on_toggle_folder_reviewed =
        Callback::new(move |(folder_path, mark_reviewed): (String, bool)| {
            let Some(diff) = ctx.diff_data.get_untracked() else {
                return;
            };
            let prefix = alloc::format!("{folder_path}/");
            let affected: Vec<String> = diff
                .files
                .iter()
                .filter(|f| f.path.starts_with(&prefix))
                .map(|f| f.path.clone())
                .collect();
            if affected.is_empty() {
                return;
            }
            ctx.reviewed_files.update(|set| {
                for p in &affected {
                    if mark_reviewed {
                        set.insert(p.clone());
                    } else {
                        set.remove(p);
                    }
                }
                state::save_viewed_files(&ctx.repo_root_string(), &diff.commit, set);
            });
            ctx.collapsed_files.update(|set| {
                for p in &affected {
                    if mark_reviewed {
                        set.insert(p.clone());
                    } else {
                        set.remove(p);
                    }
                }
            });
        });

    let on_toggle_collapse = Callback::new(move |path: String| {
        ctx.collapsed_files.update(|set| {
            if !set.remove(&path) {
                set.insert(path);
            }
        });
    });

    let on_select_file = Callback::new(move |path: String| {
        ctx.selected_file_path.set(Some(path.clone()));
        scroll_to_file_element(&path);
    });

    let on_toggle_word = Callback::new(move |word: String| {
        ctx.highlighted_word.update(|cur| {
            if cur.as_deref() == Some(word.as_str()) {
                *cur = None;
            } else {
                *cur = Some(word);
            }
        });
    });

    let on_open_in_editor = Callback::new(move |(file_path, line): (String, usize)| {
        let editor = ctx.appearance.get_untracked().editor;
        spawn_local(async move {
            if let Err(err) = api::post_open_in_editor(&file_path, Some(line), Some(editor)).await {
                ctx.error_banner.set(Some(err.to_string()));
            }
        });
    });

    let on_prefetch_line_count = Callback::new(move |file: Arc<DiffFile>| {
        prefetch_line_count_for_file(ctx, file);
    });

    let on_expand_lines = Callback::new(move |req: ExpandLinesEvent| {
        expand_lines_for_file(ctx, req);
    });

    let on_create_thread = Callback::new(move |evt: CreateThreadEvent| {
        let Some(diff) = ctx.diff_data.get_untracked() else {
            evt.on_done.run(Err(String::from("No active diff loaded")));
            return;
        };
        let base_state = ctx.base_commit_id.get_untracked();
        let is_interdiff = base_state != "parent" && !base_state.is_empty();
        let commit_id = if evt.side == CommentSide::Old && is_interdiff {
            diff.base_commit
        } else {
            diff.commit
        };

        spawn_local(async move {
            let req = CreateCommentRequest {
                commit_id,
                file_path: evt.file_path,
                line_range: evt.line_range,
                content: evt.body,
            };
            match api::post_jj_comment(&req).await {
                Ok(_) => {
                    evt.on_done.run(Ok(()));
                    refresh_comments_only(ctx);
                    reload_changes_and_commits(ctx, false);
                }
                Err(err) => evt.on_done.run(Err(err.to_string())),
            }
        });
    });

    let on_reply = Callback::new(move |evt: CommentReplyEvent| {
        spawn_local(async move {
            match api::post_jj_comment_reply(&evt.thread_id, &evt.body).await {
                Ok(_) => {
                    evt.on_done.run(Ok(()));
                    refresh_comments_only(ctx);
                    reload_changes_and_commits(ctx, false);
                }
                Err(err) => evt.on_done.run(Err(err.to_string())),
            }
        });
    });

    let on_toggle_resolve = Callback::new(move |(thread_id, current_resolved): (String, bool)| {
        spawn_local(async move {
            match api::post_jj_comment_resolve(&thread_id, current_resolved).await {
                Ok(_) => {
                    refresh_comments_only(ctx);
                    reload_changes_and_commits(ctx, false);
                }
                Err(err) => ctx.error_banner.set(Some(err.to_string())),
            }
        });
    });

    let on_jump_to_thread = Callback::new(move |thread: JjCommentThread| {
        jump_to_thread(ctx, thread);
    });

    let on_toggle_show_resolved_inline = Callback::new(move |show: bool| {
        ctx.show_resolved_inline.set(show);
        state::save_storage_bool(state::SHOW_RESOLVED_INLINE_STORAGE_KEY, show);
    });

    let comment_counts_by_file = Signal::derive(move || {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for thread in ctx.comments.get() {
            *counts.entry(thread.file_path).or_insert(0) += 1;
        }
        counts
    });

    let diff_files = Signal::derive(move || {
        ctx.diff_data
            .with(|d| d.as_ref().map(|d| d.files.clone()).unwrap_or_default())
    });

    let is_rebase_diff = Signal::derive(move || {
        ctx.diff_data
            .with(|d| d.as_ref().is_some_and(|d| d.is_rebase_diff))
    });
    let rebase_ignored = Signal::derive(move || {
        ctx.diff_data
            .with(|d| d.as_ref().is_some_and(|d| d.rebase_ignored))
    });
    let rebase_conflict = Signal::derive(move || {
        ctx.diff_data
            .with(|d| d.as_ref().is_some_and(|d| d.rebase_conflict))
    });

    let active_change = Signal::derive(move || {
        let sel = ctx.selected_change_id.get();
        ctx.changes.get().into_iter().find(|c| c.change_id == sel)
    });

    let active_commit = Signal::derive(move || {
        let target = ctx.target_commit_id.get();
        ctx.commits
            .get()
            .into_iter()
            .enumerate()
            .find(|(_, c)| commit_ids_match(&c.commit_id, &target))
    });

    let is_jj_workspace = Signal::derive(move || {
        matches!(
            ctx.repo_status.get(),
            RepoStatusState::Loaded(ref status) if status.is_jj
        )
    });

    view! {
        <div class="app-shell">
            <header class="topbar">
                <div class="topbar-left">
                    <button
                        type="button"
                        class="icon-btn icon-btn--lg"
                        title=move || if sidebar_open.get() { "Collapse sidebar" } else { "Expand sidebar" }
                        aria-expanded=move || sidebar_open.get()
                        aria-label="Toggle navigation sidebar"
                        on:click=on_toggle_sidebar
                    >
                        "☰"
                    </button>
                    <div class="brand">
                        <svg
                            class="brand-mark"
                            viewBox="0 0 219 190"
                            fill="none"
                            xmlns="http://www.w3.org/2000/svg"
                            aria-hidden="true"
                        >
                            <path
                                fill-rule="evenodd"
                                clip-rule="evenodd"
                                d="M9.42299 0.744751C4.61699 2.15275 2.03999 5.66375 0.956994 12.2768C-0.369006 20.3728 -0.294006 170.791 1.03899 176.572C2.29699 182.031 4.81699 185.308 9.65399 187.774C13.265 189.615 16.865 189.684 108.63 189.684C163.129 189.684 204.095 189.304 204.41 188.795C206 188.5 206 188.795 207.5 188C211.167 187.079 216.779 180.138 218.005 175.584C218.682 173.068 218.939 143.952 218.756 90.4718C218.487 11.7478 218.417 9.10275 216.535 6.62675C215.466 5.22075 213.441 3.19575 212.035 2.12675C209.546 0.23575 206.839 0.17975 110.978 0.0167504C55.373 -0.0782496 11.148 0.239751 9.42299 0.744751ZM203.778 14.8838C205.558 16.6638 205.509 172.577 203.728 175.005C202.577 176.574 198.865 176.688 156.978 176.446L111.478 176.184L111.226 138.684L110.974 101.184H84.059C69.255 101.184 56.808 101.52 56.397 101.931C55.986 102.343 60.674 107.82 66.814 114.104C72.954 120.388 77.978 126.202 77.978 127.026C77.978 129.577 74.769 134.267 71.896 135.916C69.609 137.229 68.75 137.278 66.704 136.217C63.533 134.572 28.705 99.1448 27.757 96.5998C27.36 95.5348 27.491 93.4308 28.049 91.9238C28.955 89.4768 68.498 49.6838 70.024 49.6838C70.366 49.6838 72.356 51.1627 74.447 52.9697C77.589 55.6867 78.118 56.6658 77.498 58.6198C77.086 59.9198 72.075 65.8308 66.363 71.7568C60.651 77.6818 55.978 83.0148 55.978 83.6068C55.978 84.3488 64.068 84.6838 81.952 84.6838H107.927H110.666C111.144 82.5 110.978 73.4537 110.978 46.2337V13.6838H156.778C191.067 13.6838 202.88 13.9858 203.778 14.8838ZM148.803 52.7217C146.51 54.0597 144.845 59.5997 145.74 62.9107C146.146 64.4107 150.657 69.9308 155.764 75.1788C161.411 80.9788 164.547 84.8988 163.764 85.1788C163.057 85.4318 151.549 85.7608 138.19 85.9108C117.896 86.1388 113.747 86.4308 112.956 87.6838C112.435 88.5088 112.002 91.7228 111.993 94.8268C111.982 99.0598 112.373 100.622 113.56 101.077C114.43 101.411 126.13 101.684 139.56 101.684C156.121 101.684 163.978 102.026 163.978 102.746C163.978 103.33 160.187 107.607 155.554 112.251C146.244 121.582 144.276 125.371 146.069 130.514C147.321 134.107 149.576 135.684 153.463 135.684C155.522 135.684 159.843 131.902 173.778 117.904C191.699 99.9008 194.978 96.0678 194.978 93.1208C194.978 91.1338 157.422 53.8238 154.163 52.5728C151.322 51.4818 150.908 51.4937 148.803 52.7217Z"
                                fill="currentColor"
                            />
                        </svg>
                        <span class="brand-name">"tellit"</span>
                    </div>
                </div>

                {move || is_jj_workspace.get().then(|| view! {
                    <div class="topbar-center">
                        <JjChangeSelector
                            changes=ctx.changes
                            graph_rows=ctx.graph_rows
                            selected_change_id=ctx.selected_change_id
                            on_select_change=on_select_change
                            on_lookup_query=on_lookup_query
                            lookup_error=ctx.lookup_error
                            loading=ctx.loading_changes
                        />
                        <JjCommitComparisonSelector
                            commits=ctx.commits
                            target_commit_id=ctx.target_commit_id
                            base_commit_id=ctx.base_commit_id
                            on_select_target=on_select_target
                            on_select_base=on_select_base
                            on_toggle_approve=on_toggle_approve
                            is_rebase_diff=is_rebase_diff
                            rebase_ignored=rebase_ignored
                            rebase_conflict=rebase_conflict
                            on_toggle_ignore_rebase=on_toggle_ignore_rebase
                            loading=ctx.loading_commits
                        />
                    </div>
                })}

                <div class="topbar-meta">
                    {move || is_jj_workspace.get().then(|| view! {
                        <>
                            <div class="segmented" role="group" aria-label="Diff view mode">
                                <button
                                    type="button"
                                    class=move || if ctx.diff_mode.get() == DiffViewMode::Unified {
                                        "segmented__btn segmented__btn--active"
                                    } else {
                                        "segmented__btn"
                                    }
                                    on:click=move |_| {
                                        ctx.diff_mode.set(DiffViewMode::Unified);
                                        state::save_diff_view_mode(DiffViewMode::Unified);
                                    }
                                >
                                    "Unified"
                                </button>
                                <button
                                    type="button"
                                    class=move || if ctx.diff_mode.get() == DiffViewMode::Split {
                                        "segmented__btn segmented__btn--active"
                                    } else {
                                        "segmented__btn"
                                    }
                                    on:click=move |_| {
                                        ctx.diff_mode.set(DiffViewMode::Split);
                                        state::save_diff_view_mode(DiffViewMode::Split);
                                    }
                                >
                                    "Split"
                                </button>
                            </div>
                            <UnresolvedJumpMenu
                                threads=ctx.comments
                                on_jump_to_thread=on_jump_to_thread
                                on_open_drawer=Callback::new(move |()| ctx.drawer_open.set(true))
                            />
                            <button
                                type="button"
                                class=move || if ctx.drawer_open.get() {
                                    "btn btn--active"
                                } else {
                                    "btn btn--ghost"
                                }
                                title="Toggle review comments drawer"
                                on:click=move |_| ctx.drawer_open.update(|v| *v = !*v)
                            >
                                "Drawer"
                            </button>
                        </>
                    })}
                    {move || {
                        let total = ctx.diff_data.with(|d| d.as_ref().map_or(0, |d| d.files.len()));
                        let viewed = ctx.reviewed_files.get().len().min(total);
                        (total > 0).then(|| {
                            let pct = (viewed * 100) / total;
                            view! {
                                <div class="viewed-progress" title="Files marked viewed">
                                    <span class="viewed-progress__label">
                                        {alloc::format!("{viewed} / {total} viewed")}
                                    </span>
                                    <div class="viewed-progress__bar">
                                        <div
                                            class="viewed-progress__fill"
                                            style=alloc::format!("width: {pct}%")
                                        ></div>
                                    </div>
                                </div>
                            }
                        })
                    }}
                    {move || is_jj_workspace.get().then(|| view! {
                        <button
                            type="button"
                            class="btn btn--ghost"
                            title=move || if ctx.watch_connected.get() {
                                "Watch live: click to refresh revisions and diff"
                            } else {
                                "Refresh revisions and diff"
                            }
                            on:click=move |_| reload_changes_and_commits(ctx, true)
                        >
                            {move || if ctx.reloading.get() { "Refreshing..." } else { "↻ Refresh" }}
                        </button>
                    })}
                    <button
                        type="button"
                        class="icon-btn icon-btn--lg"
                        title=move || match ctx.resolved_theme.get() {
                            ResolvedTheme::Dark => "Switch to light theme",
                            ResolvedTheme::Light => "Switch to dark theme",
                        }
                        aria-label="Toggle dark or light theme"
                        on:click=move |_| {
                            let mut next = ctx.appearance.get_untracked();
                            next.theme = match ctx.resolved_theme.get_untracked() {
                                ResolvedTheme::Dark => ThemePreference::Light,
                                ResolvedTheme::Light => ThemePreference::Dark,
                            };
                            apply_settings_update.run(next);
                        }
                    >
                        {move || match ctx.resolved_theme.get() {
                            ResolvedTheme::Dark => "☀",
                            ResolvedTheme::Light => "🌙",
                        }}
                    </button>
                    <button
                        type="button"
                        class="icon-btn icon-btn--lg"
                        title="Settings"
                        aria-label="Open settings modal"
                        on:click=move |_| ctx.settings_open.set(true)
                    >
                        "⚙"
                    </button>
                    <button
                        type="button"
                        class="icon-btn icon-btn--lg"
                        title="Keyboard shortcuts (?)"
                        aria-label="Open keyboard shortcuts help"
                        on:click=move |_| ctx.help_open.update(|v| *v = !*v)
                    >
                        "?"
                    </button>
                    <span class="badge">
                        {move || ctx.repo_status.get().badge_label()}
                    </span>
                    <span class=move || ctx.connection.get().badge_class()>
                        <span class="badge-dot" aria-hidden="true"></span>
                        <span>{move || ctx.connection.get().label()}</span>
                    </span>
                </div>
            </header>

            {move || ctx.error_banner.get().map(|msg| view! {
                <div class="banner banner--error" role="alert">
                    <span>{msg}</span>
                    <button
                        type="button"
                        class="icon-btn"
                        aria-label="Dismiss error"
                        on:click=move |_| ctx.error_banner.set(None)
                    >
                        "×"
                    </button>
                </div>
            })}

            {move || rebase_conflict.get().then(|| view! {
                <div class="banner banner--warning">
                    "Rebase conflict detected between patchsets: displaying full diff including upstream changes."
                </div>
            })}

            {move || active_commit.get().and_then(|(idx, c)| {
                (c.clean_rebase_approved && !c.approved).then(|| {
                    let src_label = c
                        .clean_rebase_source_commit_number
                        .map_or_else(|| String::from("earlier"), |n| alloc::format!("#{n}"));
                    view! {
                        <div class="banner banner--emerald">
                            {alloc::format!(
                                "Revision #{} matches approved revision {src_label} (clean rebase onto upstream).",
                                idx + 1
                            )}
                        </div>
                    }
                })
            })}

            <div class="workspace-body">
                {move || is_jj_workspace.get().then(|| view! {
                    <>
                        <aside
                            class="sidebar"
                            style=move || if sidebar_open.get() {
                                alloc::format!("width: {}px", sidebar_width.get())
                            } else {
                                String::from("width: 0px; border-right: none;")
                            }
                        >
                            {move || sidebar_open.get().then(|| view! {
                                <div class="sidebar-inner">
                                    <JjRevisionTreePanel
                                        changes=ctx.changes
                                        graph_rows=ctx.graph_rows
                                        selected_change_id=ctx.selected_change_id
                                        on_select_change=on_select_change
                                        on_lookup_query=on_lookup_query
                                        lookup_error=ctx.lookup_error
                                        loading=ctx.loading_changes
                                    />
                                    <FileList
                                        files=diff_files
                                        comment_counts=comment_counts_by_file
                                        reviewed_files=ctx.reviewed_files
                                        selected_file_path=ctx.selected_file_path
                                        on_select_file=on_select_file
                                        on_toggle_reviewed=on_toggle_reviewed
                                        on_toggle_folder_reviewed=on_toggle_folder_reviewed
                                    />
                                </div>
                            })}
                        </aside>
                        {move || sidebar_open.get().then(|| view! {
                            <div
                                class="sidebar-resizer"
                                title="Drag to resize sidebar"
                                on:mousedown=move |ev: MouseEvent| {
                                    ev.prevent_default();
                                    drag_origin.set((ev.client_x(), sidebar_width.get_untracked()));
                                    is_resizing.set(true);
                                }
                            ></div>
                        })}
                    </>
                })}

                <main class="diff-main">
                    {move || match ctx.repo_status.get() {
                        RepoStatusState::Loading => view! {
                            <section class="status-card" aria-busy="true">
                                <h1 class="status-card__title">"Connecting to workspace"</h1>
                                <p class="status-card__body">"Checking Jujutsu repository status..."</p>
                            </section>
                        }.into_any(),
                        RepoStatusState::Loaded(status) if !status.is_jj => view! {
                            <section class="status-card">
                                <h1 class="status-card__title">"No Jujutsu workspace detected"</h1>
                                <p class="status-card__body">
                                    "Run tellit launch inside a Jujutsu (jj) workspace to review changes."
                                </p>
                            </section>
                        }.into_any(),
                        RepoStatusState::Error(message) => view! {
                            <section class="status-card status-card--error" role="alert">
                                <h1 class="status-card__title">"Failed to query repository status"</h1>
                                <p class="status-card__body">{message}</p>
                                <button
                                    type="button"
                                    class="retry-button"
                                    on:click=move |_| {
                                        load_jj_status(ctx);
                                        if ctx.connection.get_untracked() == ConnectionState::Offline {
                                            connect_heartbeat(ctx.connection, heartbeat_handle);
                                        }
                                    }
                                >
                                    "Retry"
                                </button>
                            </section>
                        }.into_any(),
                        RepoStatusState::Loaded(_) => {
                            if ctx.loading_changes.get()
                                && ctx.diff_data.with(|d| d.is_none())
                            {
                                return view! {
                                    <div class="center-message">"Loading Jujutsu revisions..."</div>
                                }.into_any();
                            }

                            view! {
                                <div class="diff-workspace">
                                    {move || {
                                        let change_opt = active_change.get();
                                        let total_threads = ctx.comments.with(Vec::len);
                                        let unresolved_threads = ctx
                                            .comments
                                            .with(|list| list.iter().filter(|t| !t.resolved).count());
                                        change_opt.map(|change| {
                                            let review = change.review_status();
                                            let status = Signal::derive(move || review);
                                            view! {
                                                <section class="revision-summary">
                                                    <div class="revision-summary__title-row">
                                                        <span class="jj-change-id jj-change-id--lg">
                                                            {change.short_change_id}
                                                        </span>
                                                        <span class="muted-mono">{change.short_commit_id}</span>
                                                        <ReviewDot
                                                            status=status
                                                            comment_count=change.comment_count
                                                            unresolved_count=change.unresolved_comment_count
                                                        />
                                                        <h1 class="revision-summary__desc">{change.description}</h1>
                                                    </div>
                                                    <div class="revision-summary__meta">
                                                        <span>{change.author}</span>
                                                        <span>{change.timestamp}</span>
                                                        {(total_threads > 0).then(|| view! {
                                                            <span class={if unresolved_threads > 0 {
                                                                "pill-tag pill-tag--warn"
                                                            } else {
                                                                "pill-tag pill-tag--ok"
                                                            }}>
                                                                {if unresolved_threads > 0 {
                                                                    alloc::format!("{unresolved_threads} unresolved / {total_threads} threads")
                                                                } else {
                                                                    alloc::format!("{total_threads} resolved")
                                                                }}
                                                            </span>
                                                        })}
                                                        {move || ctx.highlighted_word.get().map(|w| view! {
                                                            <button
                                                                type="button"
                                                                class="pill-tag pill-tag--info"
                                                                title="Click to clear highlighted identifier"
                                                                on:click=move |_| ctx.highlighted_word.set(None)
                                                            >
                                                                {alloc::format!("token: {w} ×")}
                                                            </button>
                                                        })}
                                                    </div>
                                                </section>
                                            }
                                        })
                                    }}

                                    {move || {
                                        let diff_meta = ctx.diff_data.with(|opt| {
                                            opt.as_ref().map(|d| {
                                                (
                                                    d.is_empty && d.files.len() <= 1,
                                                    d.base_commit.clone(),
                                                    d.commit.clone(),
                                                )
                                            })
                                        });
                                        match diff_meta {
                                            None if ctx.loading_diff.get() => view! {
                                                <div class="center-message">"Loading diff..."</div>
                                            }.into_any(),
                                            None => view! {
                                                <div class="center-message">"Select a revision to inspect its diff."</div>
                                            }.into_any(),
                                            Some((true, base_c, target_c)) => view! {
                                                <div class="empty-diff-card">
                                                    <h2>"No changes in this revision"</h2>
                                                    <p>
                                                        {alloc::format!(
                                                            "Commit {} has no modified files against {}.",
                                                            target_c.chars().take(8).collect::<String>(),
                                                            base_c.chars().take(8).collect::<String>(),
                                                        )}
                                                    </p>
                                                </div>
                                            }.into_any(),
                                            Some((false, diff_base, diff_target)) => {
                                                let generation = ctx.diff_generation.get_value();
                                                view! {
                                                    <div class="diff-file-list">
                                                        <For
                                                            each=move || {
                                                                ctx.diff_data
                                                                    .with(|opt| {
                                                                        opt.as_ref()
                                                                            .map(|d| d.files.iter().cloned().enumerate().collect::<Vec<_>>())
                                                                            .unwrap_or_default()
                                                                    })
                                                            }
                                                            key=move |(idx, file)| alloc::format!("{generation}:{idx}:{}", file.path)
                                                            children=move |(idx, file)| {
                                                                view! {
                                                                    <TextDiffCard
                                                                        file=file
                                                                        file_index=idx
                                                                        base_commit=diff_base.clone()
                                                                        target_commit=diff_target.clone()
                                                                        all_threads=ctx.comments
                                                                        reviewed_files=ctx.reviewed_files
                                                                        collapsed_files=ctx.collapsed_files
                                                                        diff_mode=ctx.diff_mode
                                                                        show_resolved_inline=ctx.show_resolved_inline
                                                                        highlighted_word=ctx.highlighted_word
                                                                        expanded_lines=ctx.expanded_lines
                                                                        expand_loading=ctx.expand_loading
                                                                        active_cursor=ctx.active_cursor
                                                                        keyboard_comment_trigger=ctx.keyboard_comment_trigger
                                                                        on_hover_file=Callback::new(move |hovered| ctx.hovered_file_index.set(hovered))
                                                                        on_toggle_word=on_toggle_word
                                                                        on_toggle_viewed=on_toggle_reviewed
                                                                        on_toggle_collapse=on_toggle_collapse
                                                                        on_open_in_editor=on_open_in_editor
                                                                        on_prefetch_line_count=on_prefetch_line_count
                                                                        on_expand_lines=on_expand_lines
                                                                        on_create_thread=on_create_thread
                                                                        on_reply=on_reply
                                                                        on_toggle_resolve=on_toggle_resolve
                                                                    />
                                                                }
                                                            }
                                                        />
                                                    </div>
                                                }.into_any()
                                            }
                                        }
                                    }}
                                </div>
                            }.into_any()
                        }
                    }}
                </main>

                {move || is_jj_workspace.get().then(|| view! {
                    <CommentsDrawer
                        is_open=ctx.drawer_open
                        threads=ctx.comments
                        show_resolved_inline=ctx.show_resolved_inline
                        target_commit_id=ctx.target_commit_id
                        base_commit_id=ctx.base_commit_id
                        on_close=Callback::new(move |()| ctx.drawer_open.set(false))
                        on_toggle_show_resolved_inline=on_toggle_show_resolved_inline
                        on_jump_to_thread=on_jump_to_thread
                        on_reply=on_reply
                        on_toggle_resolve=on_toggle_resolve
                    />
                })}
            </div>

            <SettingsModal
                is_open=ctx.settings_open
                settings=ctx.appearance
                on_change=apply_settings_update
                on_close=Callback::new(move |()| ctx.settings_open.set(false))
            />
            <HelpModal
                is_open=ctx.help_open
                on_close=Callback::new(move |()| ctx.help_open.set(false))
            />
        </div>
    }
}

fn handle_global_keydown(ctx: AppCtx, ev: &KeyboardEvent) {
    let key = ev.key();
    if key == "Escape" {
        if ctx.help_open.get_untracked() {
            ev.prevent_default();
            ctx.help_open.set(false);
            return;
        }
        if ctx.settings_open.get_untracked() {
            ev.prevent_default();
            ctx.settings_open.set(false);
            return;
        }
        if ctx.drawer_open.get_untracked() {
            ev.prevent_default();
            ctx.drawer_open.set(false);
            return;
        }
        if ctx.active_cursor.get_untracked().is_some() {
            ctx.active_cursor.set(None);
        }
        return;
    }

    if keyboard::should_ignore_shortcut(ev) {
        return;
    }

    if key == "?" {
        ev.prevent_default();
        ctx.settings_open.set(false);
        ctx.help_open.update(|v| *v = !*v);
        return;
    }

    if ctx.help_open.get_untracked() || ctx.settings_open.get_untracked() {
        return;
    }

    let Some(diff) = ctx.diff_data.get_untracked() else {
        return;
    };
    let files = &diff.files;
    let mode = ctx.diff_mode.get_untracked();
    let reviewed = ctx.reviewed_files.get_untracked();

    match key.as_str() {
        "j" | "ArrowDown" => {
            ev.prevent_default();
            step_keyboard_cursor(
                ctx,
                NavigationDirection::Next,
                NavigationTarget::Line,
                files,
                mode,
                &reviewed,
            );
        }
        "k" | "ArrowUp" => {
            ev.prevent_default();
            step_keyboard_cursor(
                ctx,
                NavigationDirection::Prev,
                NavigationTarget::Line,
                files,
                mode,
                &reviewed,
            );
        }
        "n" => {
            ev.prevent_default();
            step_keyboard_cursor(
                ctx,
                NavigationDirection::Next,
                NavigationTarget::Chunk,
                files,
                mode,
                &reviewed,
            );
        }
        "p" => {
            ev.prevent_default();
            step_keyboard_cursor(
                ctx,
                NavigationDirection::Prev,
                NavigationTarget::Chunk,
                files,
                mode,
                &reviewed,
            );
        }
        "]" => {
            ev.prevent_default();
            step_keyboard_cursor(
                ctx,
                NavigationDirection::Next,
                NavigationTarget::File,
                files,
                mode,
                &reviewed,
            );
        }
        "[" => {
            ev.prevent_default();
            step_keyboard_cursor(
                ctx,
                NavigationDirection::Prev,
                NavigationTarget::File,
                files,
                mode,
                &reviewed,
            );
        }
        "{" if !files.is_empty() => {
            ev.prevent_default();
            let pos = keyboard::normalize_cursor_side(
                CursorPosition {
                    file_index: 0,
                    chunk_index: 0,
                    line_index: 0,
                    side: CommentSide::New,
                },
                files,
            );
            ctx.active_cursor.set(Some(pos));
            scroll_cursor(ctx, pos, files, ScrollLogicalPosition::Start);
        }
        "}" if !files.is_empty() => {
            ev.prevent_default();
            let pos = keyboard::normalize_cursor_side(
                CursorPosition {
                    file_index: files.len() - 1,
                    chunk_index: 0,
                    line_index: 0,
                    side: CommentSide::New,
                },
                files,
            );
            ctx.active_cursor.set(Some(pos));
            scroll_cursor(ctx, pos, files, ScrollLogicalPosition::Start);
        }
        "h" | "ArrowLeft" if mode == DiffViewMode::Split => {
            if let Some(cur) = ctx.active_cursor.get_untracked() {
                ev.prevent_default();
                let next = keyboard::switch_split_side(cur, CommentSide::Old, files);
                ctx.active_cursor.set(Some(next));
                scroll_cursor(ctx, next, files, ScrollLogicalPosition::Center);
            }
        }
        "l" | "ArrowRight" if mode == DiffViewMode::Split => {
            if let Some(cur) = ctx.active_cursor.get_untracked() {
                ev.prevent_default();
                let next = keyboard::switch_split_side(cur, CommentSide::New, files);
                ctx.active_cursor.set(Some(next));
                scroll_cursor(ctx, next, files, ScrollLogicalPosition::Center);
            }
        }
        "N" | "P" => {
            ev.prevent_default();
            let unresolved: Vec<JjCommentThread> = ctx
                .comments
                .get_untracked()
                .into_iter()
                .filter(|t| !t.resolved)
                .collect();
            if unresolved.is_empty() {
                return;
            }
            let len = unresolved.len();
            let mut target_idx = 0usize;
            ctx.unresolved_jump_cursor.update_value(|idx| {
                if key == "N" {
                    *idx %= len;
                    target_idx = *idx;
                    *idx = (*idx + 1) % len;
                } else {
                    *idx = (*idx + len - 1) % len;
                    target_idx = *idx;
                }
            });
            jump_to_thread(ctx, unresolved[target_idx].clone());
        }
        "u" => {
            ev.prevent_default();
            ctx.diff_mode.set(DiffViewMode::Unified);
            state::save_diff_view_mode(DiffViewMode::Unified);
        }
        "s" => {
            ev.prevent_default();
            ctx.diff_mode.set(DiffViewMode::Split);
            state::save_diff_view_mode(DiffViewMode::Split);
        }
        "v" => {
            let file_idx = ctx
                .active_cursor
                .get_untracked()
                .map(|c| c.file_index)
                .or_else(|| ctx.hovered_file_index.get_untracked());
            if let Some(idx) = file_idx
                && let Some(file) = files.get(idx)
            {
                ev.prevent_default();
                toggle_file_viewed_and_collapse(ctx, &diff.commit, &file.path);
            }
        }
        "V" => {
            let file_idx = ctx
                .active_cursor
                .get_untracked()
                .map(|c| c.file_index)
                .or_else(|| ctx.hovered_file_index.get_untracked());
            if let Some(idx) = file_idx
                && let Some(file) = files.get(idx)
            {
                ev.prevent_default();
                if !reviewed.contains(&file.path) {
                    toggle_file_viewed_and_collapse(ctx, &diff.commit, &file.path);
                }
                let updated_reviewed = ctx.reviewed_files.get_untracked();
                if let Some(next_idx) =
                    keyboard::next_unviewed_file_index(idx, files, &updated_reviewed)
                {
                    let pos = keyboard::normalize_cursor_side(
                        CursorPosition {
                            file_index: next_idx,
                            chunk_index: 0,
                            line_index: 0,
                            side: CommentSide::New,
                        },
                        files,
                    );
                    ctx.active_cursor.set(Some(pos));
                    scroll_cursor(ctx, pos, files, ScrollLogicalPosition::Start);
                }
            }
        }
        "c" => {
            if let Some(cur) = ctx.active_cursor.get_untracked()
                && let Some(file) = files.get(cur.file_index)
                && let Some(line) = file
                    .chunks
                    .get(cur.chunk_index)
                    .and_then(|c| c.lines.get(cur.line_index))
            {
                let line_num = match cur.side {
                    CommentSide::Old => line.old_line_number.or(line.new_line_number),
                    CommentSide::New => line.new_line_number.or(line.old_line_number),
                };
                if let Some(ln) = line_num {
                    ev.prevent_default();
                    let seq = next_req_id(ctx.keyboard_comment_seq);
                    ctx.collapsed_files.update(|set| {
                        set.remove(&file.path);
                    });
                    ctx.keyboard_comment_trigger
                        .set(Some(KeyboardCommentTrigger {
                            seq,
                            file_path: file.path.clone(),
                            side: cur.side,
                            line_num: ln,
                        }));
                }
            }
        }
        "L" => {
            ev.prevent_default();
            ctx.drawer_open.update(|v| *v = !*v);
        }
        "C" => {
            ev.prevent_default();
            let prompt = state::format_all_comments_prompt(
                &ctx.comments.get_untracked(),
                &ctx.target_commit_id.get_untracked(),
                &ctx.base_commit_id.get_untracked(),
            );
            if !prompt.is_empty() {
                spawn_local(async move {
                    let _ = api::copy_to_clipboard(&prompt).await;
                });
            }
        }
        "R" => {
            ev.prevent_default();
            reload_changes_and_commits(ctx, true);
        }
        _ => {}
    }
}

fn step_keyboard_cursor(
    ctx: AppCtx,
    dir: NavigationDirection,
    target: NavigationTarget,
    files: &[DiffFile],
    mode: DiffViewMode,
    reviewed: &HashSet<String>,
) {
    if let Some(next) = keyboard::find_next_position(
        ctx.active_cursor.get_untracked(),
        dir,
        target,
        files,
        mode,
        reviewed,
    ) {
        ctx.active_cursor.set(Some(next));
        let block = if target == NavigationTarget::File {
            ScrollLogicalPosition::Start
        } else {
            ScrollLogicalPosition::Center
        };
        scroll_cursor(ctx, next, files, block);
    }
}

fn scroll_cursor(
    ctx: AppCtx,
    pos: CursorPosition,
    files: &[DiffFile],
    block: ScrollLogicalPosition,
) {
    let mode = ctx.diff_mode.get_untracked();
    if let Some(id) = keyboard::cursor_scroll_id(pos, files, mode)
        && keyboard::scroll_to_element_id(&id, block)
    {
        return;
    }
    if let Some(file) = files.get(pos.file_index) {
        let _ = keyboard::scroll_to_element_id(&state::file_element_id(&file.path), block);
    }
}

fn toggle_file_viewed_and_collapse(ctx: AppCtx, commit: &str, path: &str) {
    let mut now_viewed = false;
    ctx.reviewed_files.update(|set| {
        if !set.remove(path) {
            set.insert(String::from(path));
            now_viewed = true;
        }
        state::save_viewed_files(&ctx.repo_root_string(), commit, set);
    });
    ctx.collapsed_files.update(|set| {
        if now_viewed {
            set.insert(String::from(path));
        } else {
            set.remove(path);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tellit_types::{HeartbeatEvent, JjStatusResponse};

    #[test]
    fn connection_state_transitions_and_labels() {
        let initial = ConnectionState::Connecting;
        assert_eq!(initial.label(), "Connecting");
        assert_eq!(initial.badge_class(), "badge badge--connecting");

        let live = initial.on_heartbeat(HeartbeatEvent::Connected);
        assert_eq!(live, ConnectionState::Live);
        assert_eq!(live.label(), "Live");
        assert_eq!(live.badge_class(), "badge badge--live");

        let still_live = live.on_heartbeat(HeartbeatEvent::Ping);
        assert_eq!(still_live, ConnectionState::Live);

        assert_eq!(ConnectionState::Offline.label(), "Offline");
        assert_eq!(
            ConnectionState::Offline.badge_class(),
            "badge badge--offline"
        );
    }

    #[test]
    fn repo_status_badge_labels_cover_all_states() {
        assert_eq!(
            RepoStatusState::Loading.badge_label(),
            "Checking workspace..."
        );
        assert_eq!(
            RepoStatusState::Loaded(JjStatusResponse::jj("/tmp/ws")).badge_label(),
            "jj workspace"
        );
        assert_eq!(
            RepoStatusState::Loaded(JjStatusResponse::not_jj()).badge_label(),
            "non-jj directory"
        );
        assert_eq!(
            RepoStatusState::Error(String::from("network timeout")).badge_label(),
            "Status unavailable"
        );
    }
}
