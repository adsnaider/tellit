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
mod route;
mod state;

use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use std::collections::{HashMap, HashSet};

use leptos::ev;
use leptos::prelude::*;
use leptos::task::spawn_local;
use tellit_types::{
    AppearanceSettings, CommentSide, CreateCommentRequest, DiffFile, FileStatus, JjChange,
    JjCommentThread, JjCommit, JjDiffResponse, JjGraphRow, LineRangeSpec, ThemePreference,
    WatchEvent,
};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{KeyboardEvent, MouseEvent, ScrollLogicalPosition};

use components::change_overview::ChangeOverview;
use components::comments::{CommentReplyEvent, CommentsDrawer, UnresolvedJumpMenu};
use components::file_list::{FileList, FileTreeEntry};
use components::help_modal::HelpModal;
use components::keyboard::{self, CursorPosition, NavigationDirection, NavigationTarget};
use components::markdown_diff::MarkdownTab;
use components::review_dot::ReviewDot;
use components::revision_selector::{
    JjChangeSelector, JjCommitComparisonSelector, commit_ids_match, pick_initial_change,
};
use components::revision_tree::JjRevisionTreePanel;
pub use components::revision_tree::{
    GraphCellLayout, GraphNodeKind, GraphRowLayout, compute_graph_layout,
};
use components::settings_modal::SettingsModal;
use components::single_file_view::SingleFileView;
use components::text_diff::expanded::{
    ExpandedLinesMap, record_expand_all_before, record_line_expansion, split_blob_lines,
};
use components::text_diff::word_highlight;
use components::text_diff::{
    ActiveComposer, CHUNK_WINDOW_STEP_LINES, CreateThreadEvent, ExpandLinesEvent,
    LineSelectionDrag, extract_snippet_for_file, parse_comment_side,
};
use route::{AppRoute, RouteQuery};
pub use state::{ConnectionState, DiffViewMode, RepoStatusState, ResolvedTheme};

const MAX_CONCURRENT_FILE_FETCHES: usize = 4;
const VIEWPORT_OBSERVER_ROOT_MARGIN: &str = "1500px 0px";

#[derive(Debug, Clone, PartialEq, Eq)]
struct LoadedDiff {
    commit: String,
    base_commit: String,
    is_empty: bool,
    is_rebase_diff: bool,
    rebase_ignored: bool,
    rebase_conflict: bool,
    files: Vec<Arc<DiffFile>>,
}

impl From<JjDiffResponse> for LoadedDiff {
    fn from(diff: JjDiffResponse) -> Self {
        Self {
            commit: diff.commit,
            base_commit: diff.base_commit,
            is_empty: diff.is_empty,
            is_rebase_diff: diff.is_rebase_diff,
            rebase_ignored: diff.rebase_ignored,
            rebase_conflict: diff.rebase_conflict,
            files: diff.files.into_iter().map(Arc::new).collect(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct FileCardSlot {
    pub(crate) path: String,
    pub(crate) file: RwSignal<Arc<DiffFile>>,
    pub(crate) file_index: RwSignal<usize>,
    pub(crate) commits: RwSignal<(String, String)>,
    pub(crate) drag_state: RwSignal<Option<LineSelectionDrag>>,
    pub(crate) active_composer: RwSignal<Option<ActiveComposer>>,
    pub(crate) md_tab: RwSignal<MarkdownTab>,
    pub(crate) is_near_viewport: RwSignal<bool>,
    pub(crate) loading_chunks: RwSignal<bool>,
    pub(crate) load_error: RwSignal<Option<String>>,
    pub(crate) visible_line_limit: RwSignal<usize>,
}

/// Shared callback bundle passed from `App` down to inline and single-file `TextDiffCard` mounts.
#[derive(Clone, Copy)]
pub(crate) struct DiffCardCallbacks {
    pub(crate) on_register_card: Callback<(String, web_sys::Element)>,
    pub(crate) on_unregister_card: Callback<web_sys::Element>,
    pub(crate) on_request_file_diff: Callback<String>,
    pub(crate) on_window_expanded: Callback<()>,
    pub(crate) on_toggle_viewed: Callback<String>,
    pub(crate) on_open_in_editor: Callback<(String, usize)>,
    pub(crate) on_prefetch_line_count: Callback<Arc<DiffFile>>,
    pub(crate) on_expand_lines: Callback<ExpandLinesEvent>,
    pub(crate) on_create_thread: Callback<CreateThreadEvent>,
    pub(crate) on_reply: Callback<CommentReplyEvent>,
    pub(crate) on_toggle_resolve: Callback<(String, bool)>,
}

impl FileCardSlot {
    fn dispose(&self) {
        self.file.dispose();
        self.file_index.dispose();
        self.commits.dispose();
        self.drag_state.dispose();
        self.active_composer.dispose();
        self.md_tab.dispose();
        self.is_near_viewport.dispose();
        self.loading_chunks.dispose();
        self.load_error.dispose();
        self.visible_line_limit.dispose();
    }
}

#[derive(Default)]
struct FileFetchQueue {
    active_generation: u64,
    in_flight: HashSet<String>,
    pending: VecDeque<String>,
    pending_set: HashSet<String>,
}

impl FileFetchQueue {
    fn reset(&mut self, generation: u64) {
        self.active_generation = generation;
        self.in_flight.clear();
        self.pending.clear();
        self.pending_set.clear();
    }
}

struct ViewportObserverHandle {
    observer: web_sys::IntersectionObserver,
    _callback: Closure<dyn FnMut(js_sys::Array)>,
}

impl Drop for ViewportObserverHandle {
    fn drop(&mut self) {
        self.observer.disconnect();
    }
}

fn create_viewport_observer(ctx: AppCtx) -> Option<ViewportObserverHandle> {
    let cb = Closure::<dyn FnMut(js_sys::Array)>::new(move |entries: js_sys::Array| {
        for entry_val in entries.iter() {
            let Ok(entry) = entry_val.dyn_into::<web_sys::IntersectionObserverEntry>() else {
                continue;
            };
            if !entry.is_intersecting() {
                continue;
            }
            if let Some(path) = entry.target().get_attribute("data-file") {
                mark_file_near_viewport(ctx, &path);
            }
        }
    });
    let init = web_sys::IntersectionObserverInit::new();
    init.set_root_margin(VIEWPORT_OBSERVER_ROOT_MARGIN);
    let observer =
        web_sys::IntersectionObserver::new_with_options(cb.as_ref().unchecked_ref(), &init).ok()?;
    Some(ViewportObserverHandle {
        observer,
        _callback: cb,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DiffPaneState {
    Loading,
    Idle,
    Ready,
}

#[derive(Clone, Copy)]
struct AppCtx {
    repo_status: RwSignal<RepoStatusState>,
    connection: RwSignal<ConnectionState>,
    watch_connected: RwSignal<bool>,
    reloading: RwSignal<bool>,
    route: RwSignal<AppRoute>,
    changes: RwSignal<Vec<JjChange>>,
    graph_rows: RwSignal<Vec<JjGraphRow>>,
    loading_changes: RwSignal<bool>,
    selected_change_id: RwSignal<String>,
    commits: RwSignal<Vec<JjCommit>>,
    loading_commits: RwSignal<bool>,
    target_commit_id: RwSignal<String>,
    base_commit_id: RwSignal<String>,
    ignore_rebase: RwSignal<bool>,
    diff_data: RwSignal<Option<Arc<LoadedDiff>>>,
    ordered_file_slots: RwSignal<Vec<FileCardSlot>>,
    hydrated_files_rev: RwSignal<u64>,
    file_slot_map: StoredValue<HashMap<String, FileCardSlot>, LocalStorage>,
    loading_diff: RwSignal<bool>,
    comments: RwSignal<Vec<JjCommentThread>>,
    error_banner: RwSignal<Option<String>>,
    lookup_error: RwSignal<Option<String>>,
    reviewed_files: RwSignal<HashSet<String>>,
    overview_expanded_files: RwSignal<HashSet<String>>,
    selected_file_path: RwSignal<Option<String>>,
    diff_mode: RwSignal<DiffViewMode>,
    diff_width_chars: RwSignal<Option<u16>>,
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
    active_drag_file: StoredValue<Option<String>, LocalStorage>,
    selection_anchor: StoredValue<Option<(String, CommentSide, u32)>, LocalStorage>,
    unresolved_jump_cursor: StoredValue<usize, LocalStorage>,
    changes_req_id: StoredValue<u64, LocalStorage>,
    commits_req_id: StoredValue<u64, LocalStorage>,
    diff_req_id: StoredValue<u64, LocalStorage>,
    comments_req_id: StoredValue<u64, LocalStorage>,
    diff_generation: StoredValue<u64, LocalStorage>,
    file_fetch_queue: StoredValue<FileFetchQueue, LocalStorage>,
    pending_thread_scroll: StoredValue<Option<(u64, String, String)>, LocalStorage>,
    pending_cursor_scroll_block: StoredValue<Option<ScrollLogicalPosition>, LocalStorage>,
    prefetched_line_counts: StoredValue<HashSet<String>, LocalStorage>,
    pending_initial_target: StoredValue<Option<String>, LocalStorage>,
    pending_initial_base: StoredValue<Option<String>, LocalStorage>,
}

impl AppCtx {
    fn new() -> Self {
        let appearance_init = state::load_appearance_settings();
        let resolved_init = state::apply_appearance_to_document(&appearance_init);
        let initial_route = route::current_browser_route();
        let initial_ignore_rebase = initial_route
            .query()
            .and_then(|q| q.ignore_rebase)
            .unwrap_or(true);
        let initial_selected_file = initial_route.file_path().map(String::from);
        Self {
            repo_status: RwSignal::new(RepoStatusState::Loading),
            connection: RwSignal::new(ConnectionState::Connecting),
            watch_connected: RwSignal::new(false),
            reloading: RwSignal::new(false),
            route: RwSignal::new(initial_route),
            changes: RwSignal::new(Vec::new()),
            graph_rows: RwSignal::new(Vec::new()),
            loading_changes: RwSignal::new(true),
            selected_change_id: RwSignal::new(String::new()),
            commits: RwSignal::new(Vec::new()),
            loading_commits: RwSignal::new(false),
            target_commit_id: RwSignal::new(String::new()),
            base_commit_id: RwSignal::new(String::from("parent")),
            ignore_rebase: RwSignal::new(initial_ignore_rebase),
            diff_data: RwSignal::new(None),
            ordered_file_slots: RwSignal::new(Vec::new()),
            hydrated_files_rev: RwSignal::new(0),
            file_slot_map: StoredValue::new_local(HashMap::new()),
            loading_diff: RwSignal::new(false),
            comments: RwSignal::new(Vec::new()),
            error_banner: RwSignal::new(None),
            lookup_error: RwSignal::new(None),
            reviewed_files: RwSignal::new(HashSet::new()),
            overview_expanded_files: RwSignal::new(HashSet::new()),
            selected_file_path: RwSignal::new(initial_selected_file),
            diff_mode: RwSignal::new(state::load_diff_view_mode()),
            diff_width_chars: RwSignal::new(state::load_diff_width_chars()),
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
            active_drag_file: StoredValue::new_local(None),
            selection_anchor: StoredValue::new_local(None),
            unresolved_jump_cursor: StoredValue::new_local(0),
            changes_req_id: StoredValue::new_local(0),
            commits_req_id: StoredValue::new_local(0),
            diff_req_id: StoredValue::new_local(0),
            comments_req_id: StoredValue::new_local(0),
            diff_generation: StoredValue::new_local(0),
            file_fetch_queue: StoredValue::new_local(FileFetchQueue::default()),
            pending_thread_scroll: StoredValue::new_local(None),
            pending_cursor_scroll_block: StoredValue::new_local(None),
            prefetched_line_counts: StoredValue::new_local(HashSet::new()),
            pending_initial_target: StoredValue::new_local(None),
            pending_initial_base: StoredValue::new_local(None),
        }
    }

    fn repo_root_string(&self) -> String {
        self.repo_status.with_untracked(|s| match s {
            RepoStatusState::Loaded(status) => status.repo_root.clone().unwrap_or_default(),
            _ => String::new(),
        })
    }

    fn current_diff(&self) -> Option<Arc<LoadedDiff>> {
        self.diff_data
            .with_untracked(|d| d.as_ref().map(Arc::clone))
    }
}

/// Unmounts cards for files absent from `loaded` before `collapsed_files` updates so an outgoing
/// collapsed file never flashes open prior to unmount.
fn prune_removed_file_slots(ctx: AppCtx, loaded: &LoadedDiff) {
    let keep_paths: HashSet<&str> = loaded.files.iter().map(|f| f.path.as_str()).collect();
    let has_removed = ctx
        .ordered_file_slots
        .with_untracked(|cur| cur.iter().any(|s| !keep_paths.contains(s.path.as_str())));
    if has_removed {
        ctx.ordered_file_slots.update(|cur| {
            cur.retain(|s| keep_paths.contains(s.path.as_str()));
        });
    }
}

fn mark_file_near_viewport(ctx: AppCtx, path: &str) {
    let slot = ctx
        .file_slot_map
        .try_with_value(|map| map.get(path).cloned())
        .flatten();
    if let Some(slot) = slot
        && !slot.is_near_viewport.try_get_untracked().unwrap_or(true)
    {
        let _ = slot.is_near_viewport.try_set(true);
    }
}

fn is_strict_40_hex_oid(raw: &str) -> bool {
    raw.len() == 40 && raw.bytes().all(|b| b.is_ascii_hexdigit())
}

fn enqueue_file_diff_fetch(ctx: AppCtx, path: String) {
    enqueue_file_diff_fetch_with_priority(ctx, path, false);
}

fn enqueue_file_diff_fetch_priority(ctx: AppCtx, path: String) {
    enqueue_file_diff_fetch_with_priority(ctx, path, true);
}

fn enqueue_file_diff_fetch_with_priority(ctx: AppCtx, path: String, prioritize: bool) {
    let slot = ctx
        .file_slot_map
        .try_with_value(|map| map.get(&path).cloned())
        .flatten();
    let Some(slot) = slot else {
        return;
    };
    if slot
        .file
        .try_with_untracked(|f| f.chunks_loaded)
        .unwrap_or(true)
    {
        return;
    }

    let mut queued = false;
    let _ = ctx.file_fetch_queue.try_update_value(|q| {
        if q.in_flight.contains(&path) {
            return;
        }
        if q.pending_set.contains(&path) {
            if prioritize && let Some(pos) = q.pending.iter().position(|p| p == &path) {
                q.pending.remove(pos);
                q.pending.push_front(path.clone());
            }
            return;
        }
        q.pending_set.insert(path.clone());
        if prioritize {
            q.pending.push_front(path.clone());
        } else {
            q.pending.push_back(path.clone());
        }
        queued = true;
    });
    if !queued {
        return;
    }

    let _ = slot.load_error.try_set(None);
    let _ = slot.loading_chunks.try_set(true);
    pump_file_fetch_queue(ctx);
}

fn pump_file_fetch_queue(ctx: AppCtx) {
    let Some(diff) = ctx.current_diff() else {
        return;
    };
    let generation = ctx.diff_generation.get_value();
    let target = diff.commit.clone();
    let base_raw = ctx.base_commit_id.get_untracked();
    let base_opt = (base_raw != "parent" && !base_raw.is_empty()).then_some(base_raw);
    let ignore_rebase = ctx.ignore_rebase.get_untracked();

    let mut batch = Vec::new();
    let _ = ctx.file_fetch_queue.try_update_value(|q| {
        if q.active_generation != generation {
            return;
        }
        while q.in_flight.len() < MAX_CONCURRENT_FILE_FETCHES {
            let Some(next_path) = q.pending.pop_front() else {
                break;
            };
            q.pending_set.remove(&next_path);
            q.in_flight.insert(next_path.clone());
            batch.push(next_path);
        }
    });

    for path in batch {
        let target_c = target.clone();
        let base_c = base_opt.clone();
        spawn_local(async move {
            let res =
                api::fetch_jj_diff_file(&target_c, base_c.as_deref(), ignore_rebase, &path).await;
            let mut is_current = false;
            let _ = ctx.file_fetch_queue.try_update_value(|q| {
                if q.active_generation == generation {
                    q.in_flight.remove(&path);
                    is_current = true;
                }
            });
            if is_current && ctx.diff_generation.try_get_value() == Some(generation) {
                let slot = ctx
                    .file_slot_map
                    .try_with_value(|map| map.get(&path).cloned())
                    .flatten();
                if let Some(slot) = slot {
                    match res {
                        Ok(loaded_file) => {
                            let _ = slot.file.try_set(Arc::new(loaded_file));
                            let _ = slot.loading_chunks.try_set(false);
                            let _ = slot.load_error.try_set(None);

                            // Re-normalize cursor side and scroll to the row if the keyboard
                            // cursor landed on this file while it was still summary-only.
                            let hydrated_idx = slot.file_index.try_get_untracked();
                            if let Some(cur) = ctx.active_cursor.try_get_untracked().flatten()
                                && hydrated_idx == Some(cur.file_index)
                            {
                                let files_now: Vec<Arc<DiffFile>> = ctx
                                    .ordered_file_slots
                                    .try_with_untracked(|slots| {
                                        slots
                                            .iter()
                                            .filter_map(|s| s.file.try_get_untracked())
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                let normalized = keyboard::normalize_cursor_side(cur, &files_now);
                                if normalized != cur {
                                    let _ = ctx.active_cursor.try_set(Some(normalized));
                                }
                                let mode = ctx
                                    .diff_mode
                                    .try_get_untracked()
                                    .unwrap_or(DiffViewMode::Unified);
                                if let Some(row_id) =
                                    keyboard::cursor_scroll_id(normalized, &files_now, mode)
                                {
                                    let block = ctx
                                        .pending_cursor_scroll_block
                                        .try_update_value(Option::take)
                                        .flatten()
                                        .unwrap_or(ScrollLogicalPosition::Center);
                                    spawn_local(async move {
                                        let _ = JsFuture::from(js_sys::Promise::resolve(
                                            &JsValue::UNDEFINED,
                                        ))
                                        .await;
                                        if ctx.diff_generation.try_get_value() == Some(generation)
                                            && ctx.active_cursor.try_get_untracked().flatten()
                                                == Some(normalized)
                                        {
                                            let _ = keyboard::scroll_to_element_id(&row_id, block);
                                        }
                                    });
                                }
                            }

                            // Complete any pending `jump_to_thread` scroll targeting this file.
                            let pending_thread_dom = ctx
                                .pending_thread_scroll
                                .try_update_value(|pending| {
                                    if let Some((gen_id, ref pending_path, _)) = *pending
                                        && gen_id == generation
                                        && *pending_path == path
                                    {
                                        pending.take().map(|(_, _, dom_id)| dom_id)
                                    } else {
                                        None
                                    }
                                })
                                .flatten();
                            if let Some(thread_dom) = pending_thread_dom {
                                spawn_local(async move {
                                    let _ = JsFuture::from(js_sys::Promise::resolve(
                                        &JsValue::UNDEFINED,
                                    ))
                                    .await;
                                    if ctx.diff_generation.try_get_value() == Some(generation) {
                                        let _ = keyboard::scroll_to_element_id(
                                            &thread_dom,
                                            ScrollLogicalPosition::Center,
                                        );
                                    }
                                });
                            }

                            let _ = ctx.hydrated_files_rev.try_update(|rev| *rev += 1);
                        }
                        Err(err) => {
                            let _ = slot.loading_chunks.try_set(false);
                            let _ = slot.load_error.try_set(Some(err.to_string()));
                            let _ = ctx.pending_thread_scroll.try_update_value(|pending| {
                                if let Some((gen_id, ref pending_path, _)) = *pending
                                    && gen_id == generation
                                    && *pending_path == path
                                {
                                    *pending = None;
                                }
                            });
                        }
                    }
                }
                pump_file_fetch_queue(ctx);
            }
        });
    }
}

fn sync_file_slots(
    ctx: AppCtx,
    loaded: &LoadedDiff,
    prev_rebase_ignored: Option<bool>,
) -> Vec<FileCardSlot> {
    let mut ordered = Vec::with_capacity(loaded.files.len());
    let mut keep_paths: HashSet<&str> = HashSet::with_capacity(loaded.files.len());
    let mut changed_paths: HashSet<&str> = HashSet::new();
    let mut evicted = Vec::new();
    let commits_are_immutable_oids =
        is_strict_40_hex_oid(&loaded.commit) && is_strict_40_hex_oid(&loaded.base_commit);
    let same_rebase_mode = prev_rebase_ignored == Some(loaded.rebase_ignored);

    ctx.file_slot_map.update_value(|map| {
        for (idx, file_arc) in loaded.files.iter().enumerate() {
            keep_paths.insert(file_arc.path.as_str());
            if let Some(slot) = map.get(&file_arc.path).cloned() {
                let same_commits = slot
                    .commits
                    .with_untracked(|c| c.0 == loaded.base_commit && c.1 == loaded.commit);
                let keep_existing_hydrated = commits_are_immutable_oids
                    && same_rebase_mode
                    && same_commits
                    && slot.file.with_untracked(|old| {
                        old.chunks_loaded
                            && !file_arc.chunks_loaded
                            && old.path == file_arc.path
                            && old.old_path == file_arc.old_path
                            && old.status == file_arc.status
                            && old.additions == file_arc.additions
                            && old.deletions == file_arc.deletions
                            && old.is_generated == file_arc.is_generated
                    });
                let file_changed = !keep_existing_hydrated
                    && slot
                        .file
                        .with_untracked(|old| old.as_ref() != file_arc.as_ref());
                if file_changed {
                    changed_paths.insert(file_arc.path.as_str());
                    slot.file.set(Arc::clone(file_arc));
                    slot.drag_state.set(None);
                    slot.active_composer.set(None);
                    slot.loading_chunks.set(false);
                    slot.load_error.set(None);
                    slot.visible_line_limit.set(CHUNK_WINDOW_STEP_LINES);
                }
                if slot.file_index.get_untracked() != idx {
                    slot.file_index.set(idx);
                }
                if !same_commits {
                    slot.commits
                        .set((loaded.base_commit.clone(), loaded.commit.clone()));
                }
                ordered.push(slot);
            } else {
                let slot = FileCardSlot {
                    path: file_arc.path.clone(),
                    file: RwSignal::new(Arc::clone(file_arc)),
                    file_index: RwSignal::new(idx),
                    commits: RwSignal::new((loaded.base_commit.clone(), loaded.commit.clone())),
                    drag_state: RwSignal::new(None),
                    active_composer: RwSignal::new(None),
                    md_tab: RwSignal::new(MarkdownTab::Preview),
                    is_near_viewport: RwSignal::new(false),
                    loading_chunks: RwSignal::new(false),
                    load_error: RwSignal::new(None),
                    visible_line_limit: RwSignal::new(CHUNK_WINDOW_STEP_LINES),
                };
                map.insert(file_arc.path.clone(), slot.clone());
                ordered.push(slot);
            }
        }
        let stale_keys: Vec<String> = map
            .keys()
            .filter(|path| !keep_paths.contains(path.as_str()))
            .cloned()
            .collect();
        for key in stale_keys {
            if let Some(slot) = map.remove(&key) {
                evicted.push(slot);
            }
        }
    });

    ctx.active_drag_file.update_value(|active| {
        if let Some(ref p) = *active
            && (!keep_paths.contains(p.as_str()) || changed_paths.contains(p.as_str()))
        {
            *active = None;
        }
    });

    let order_unchanged = ctx.ordered_file_slots.with_untracked(|cur| {
        cur.len() == ordered.len()
            && cur
                .iter()
                .zip(ordered.iter())
                .all(|(a, b)| a.path == b.path)
    });
    if !order_unchanged {
        ctx.ordered_file_slots.set(ordered);
    }

    evicted
}

fn clear_all_slot_interactions(ctx: AppCtx) {
    ctx.active_drag_file.set_value(None);
    ctx.selection_anchor.set_value(None);
    ctx.file_slot_map.with_value(|map| {
        for slot in map.values() {
            if slot.drag_state.with_untracked(Option::is_some) {
                slot.drag_state.set(None);
            }
            if slot.active_composer.with_untracked(Option::is_some) {
                slot.active_composer.set(None);
            }
        }
    });
}

fn next_req_id(slot: StoredValue<u64, LocalStorage>) -> u64 {
    let mut next = 0u64;
    slot.update_value(|id| {
        *id += 1;
        next = *id;
    });
    next
}

fn current_route_query(ctx: AppCtx) -> RouteQuery {
    let cid = ctx.selected_change_id.get_untracked();
    let head_commit = ctx.changes.with_untracked(|list| {
        list.iter()
            .find(|c| c.change_id == cid)
            .map(|c| c.commit_id.clone())
    });
    let target = ctx.target_commit_id.get_untracked();
    let base = ctx.base_commit_id.get_untracked();
    let ignore_rebase = ctx.ignore_rebase.get_untracked();
    RouteQuery::from_ui_state(
        (!target.is_empty()).then_some(target.as_str()),
        head_commit.as_deref(),
        &base,
        ignore_rebase,
    )
}

fn scroll_diff_main_to_top() {
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|doc| doc.query_selector(".diff-main").ok().flatten())
    {
        el.set_scroll_top(0);
    }
}

fn navigate_to_route(ctx: AppCtx, next: AppRoute, replace: bool, scroll_top: bool) {
    let next_file = next.file_path().map(String::from);
    ctx.route.set(next.clone());
    if let Some(ref path) = next_file {
        ctx.selected_file_path.set(Some(path.clone()));
    }
    ctx.active_cursor.set(None);
    if replace {
        route::replace_browser_route(&next);
    } else {
        route::push_browser_route(&next);
    }
    if let Some(ref path) = next_file {
        mark_file_near_viewport(ctx, path);
        enqueue_file_diff_fetch_priority(ctx, path.clone());
    }
    if scroll_top {
        scroll_diff_main_to_top();
    }
}

fn navigate_to_file(ctx: AppCtx, path: String, scroll_top: bool) {
    let cid = ctx.selected_change_id.get_untracked();
    if cid.is_empty() {
        return;
    }
    let next = AppRoute::FileDiff {
        change_id: cid,
        file_path: path,
        query: current_route_query(ctx),
    };
    navigate_to_route(ctx, next, false, scroll_top);
}

fn navigate_to_overview(ctx: AppCtx) {
    let cid = ctx.selected_change_id.get_untracked();
    if cid.is_empty() {
        return;
    }
    let next = AppRoute::ChangeOverview {
        change_id: cid,
        query: current_route_query(ctx),
    };
    navigate_to_route(ctx, next, false, true);
}

fn sync_browser_query(ctx: AppCtx) {
    let cid = ctx.selected_change_id.get_untracked();
    if cid.is_empty() {
        return;
    }
    let query = current_route_query(ctx);
    let next = match ctx.route.get_untracked() {
        AppRoute::FileDiff { file_path, .. } => AppRoute::FileDiff {
            change_id: cid,
            file_path,
            query,
        },
        AppRoute::ChangeOverview { .. } | AppRoute::Root => AppRoute::ChangeOverview {
            change_id: cid,
            query,
        },
    };
    ctx.route.set(next.clone());
    route::replace_browser_route(&next);
}

fn find_matching_change_id(changes: &[JjChange], requested: &str) -> Option<String> {
    changes
        .iter()
        .find(|c| c.change_id == requested || c.short_change_id == requested)
        .or_else(|| {
            changes
                .iter()
                .find(|c| c.change_id.starts_with(requested) || requested.starts_with(&c.change_id))
        })
        .map(|c| c.change_id.clone())
}

fn handle_popstate(ctx: AppCtx) {
    let prev_route = ctx.route.get_untracked();
    let next = route::current_browser_route();
    ctx.active_cursor.set(None);

    match next.clone() {
        AppRoute::Root => {
            let current_cid = ctx.selected_change_id.get_untracked();
            if !current_cid.is_empty() {
                let fallback = AppRoute::ChangeOverview {
                    change_id: current_cid,
                    query: RouteQuery::default(),
                };
                ctx.route.set(fallback.clone());
                route::replace_browser_route(&fallback);
            } else {
                ctx.route.set(AppRoute::Root);
            }
        }
        AppRoute::ChangeOverview { change_id, query }
        | AppRoute::FileDiff {
            change_id, query, ..
        } => {
            let resolved_cid = ctx
                .changes
                .with_untracked(|list| find_matching_change_id(list, &change_id));
            let current_cid = ctx.selected_change_id.get_untracked();
            let desired_ignore_rebase = query.ignore_rebase.unwrap_or(true);
            let desired_base = query.base.clone().unwrap_or_else(|| String::from("parent"));

            if resolved_cid.as_deref() == Some(current_cid.as_str()) || change_id == current_cid {
                ctx.route.set(next.clone());
                if let Some(path) = next.file_path() {
                    ctx.selected_file_path.set(Some(String::from(path)));
                }
                let head_commit = ctx.changes.with_untracked(|list| {
                    list.iter()
                        .find(|c| c.change_id == current_cid)
                        .map(|c| c.commit_id.clone())
                });
                let desired_target = query.target.clone().or(head_commit);
                let cur_target = ctx.target_commit_id.get_untracked();
                let cur_base = ctx.base_commit_id.get_untracked();
                let cur_ignore = ctx.ignore_rebase.get_untracked();

                let target_changed = desired_target
                    .as_deref()
                    .is_some_and(|t| !commit_ids_match(t, &cur_target));
                if target_changed || desired_base != cur_base || desired_ignore_rebase != cur_ignore
                {
                    ctx.ignore_rebase.set(desired_ignore_rebase);
                    ctx.base_commit_id.set(desired_base.clone());
                    if let Some(t) = desired_target {
                        ctx.target_commit_id.set(t.clone());
                        let base_opt = (desired_base != "parent" && !desired_base.is_empty())
                            .then_some(desired_base);
                        load_diff_and_comments(ctx, t, base_opt, desired_ignore_rebase);
                    }
                }
            } else if let Some(found_cid) = resolved_cid {
                ctx.route.set(next.clone());
                ctx.selected_file_path
                    .set(next.file_path().map(String::from));
                ctx.ignore_rebase.set(desired_ignore_rebase);
                ctx.pending_initial_target.set_value(query.target);
                ctx.pending_initial_base.set_value(query.base);
                select_change_inner(ctx, found_cid, false);
            } else {
                let next_file = next.file_path().map(String::from);
                spawn_local(async move {
                    if let Ok(resp) = api::fetch_jj_changes(Some(&change_id)).await
                        && let Some(found) = resp.changes.into_iter().next()
                    {
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
                        let canonical = match next_file {
                            Some(file_path) => AppRoute::FileDiff {
                                change_id: cid.clone(),
                                file_path,
                                query: query.clone(),
                            },
                            None => AppRoute::ChangeOverview {
                                change_id: cid.clone(),
                                query: query.clone(),
                            },
                        };
                        ctx.route.set(canonical.clone());
                        ctx.selected_file_path
                            .set(canonical.file_path().map(String::from));
                        route::replace_browser_route(&canonical);
                        ctx.ignore_rebase.set(desired_ignore_rebase);
                        ctx.pending_initial_target.set_value(query.target);
                        ctx.pending_initial_base.set_value(query.base);
                        ctx.error_banner.set(None);
                        select_change_inner(ctx, cid, false);
                    } else {
                        ctx.error_banner
                            .set(Some(alloc::format!("Change '{change_id}' was not found.")));
                        ctx.route.set(prev_route.clone());
                        route::replace_browser_route(&prev_route);
                    }
                });
            }
        }
    }
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
                let loaded = Arc::new(LoadedDiff::from(diff));
                let generation = next_req_id(ctx.diff_generation);
                ctx.file_fetch_queue.update_value(|q| q.reset(generation));
                ctx.pending_thread_scroll.set_value(None);
                ctx.pending_cursor_scroll_block.set_value(None);
                ctx.expanded_lines.set(HashMap::new());
                ctx.prefetched_line_counts.update_value(HashSet::clear);
                ctx.highlighted_word.set(None);
                ctx.expand_loading.set(false);
                ctx.active_cursor.set(None);

                let repo_root = ctx.repo_root_string();
                let mut viewed = state::load_viewed_files(&repo_root, &loaded.commit);
                let auto_patterns = ctx
                    .appearance
                    .with_untracked(|a| a.auto_viewed_patterns.clone());
                if !auto_patterns.is_empty() {
                    for file in &loaded.files {
                        if file.path != "Commit Message"
                            && state::matches_auto_viewed_patterns(&file.path, &auto_patterns)
                        {
                            viewed.insert(file.path.clone());
                        }
                    }
                }
                let prev_meta = ctx.diff_data.with_untracked(|d| {
                    d.as_ref().map(|prev| {
                        (
                            prev.base_commit.clone(),
                            prev.commit.clone(),
                            prev.rebase_ignored,
                        )
                    })
                });
                let prev_rebase_ignored = prev_meta.as_ref().map(|(_, _, r)| *r);
                let same_immutable_diff = prev_meta.as_ref().is_some_and(|(pb, pc, pr)| {
                    *pb == loaded.base_commit
                        && *pc == loaded.commit
                        && *pr == loaded.rebase_ignored
                        && is_strict_40_hex_oid(&loaded.commit)
                        && is_strict_40_hex_oid(&loaded.base_commit)
                });

                prune_removed_file_slots(ctx, &loaded);
                ctx.reviewed_files.set(viewed);
                if same_immutable_diff {
                    let keep_paths: HashSet<&str> =
                        loaded.files.iter().map(|f| f.path.as_str()).collect();
                    ctx.overview_expanded_files.update(|set| {
                        set.retain(|p| keep_paths.contains(p.as_str()));
                    });
                } else {
                    ctx.overview_expanded_files.set(HashSet::new());
                }
                let evicted_slots = sync_file_slots(ctx, &loaded, prev_rebase_ignored);
                ctx.diff_data.set(Some(loaded));
                for slot in evicted_slots {
                    slot.dispose();
                }
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
    select_change_inner(ctx, change_id, true);
}

fn select_change_inner(ctx: AppCtx, change_id: String, push_overview_route: bool) {
    if change_id.is_empty() {
        return;
    }
    clear_all_slot_interactions(ctx);
    ctx.selected_change_id.set(change_id.clone());
    ctx.lookup_error.set(None);
    ctx.overview_expanded_files.set(HashSet::new());

    if push_overview_route {
        ctx.ignore_rebase.set(true);
        ctx.selected_file_path.set(None);
        let next = AppRoute::ChangeOverview {
            change_id: change_id.clone(),
            query: RouteQuery::default(),
        };
        navigate_to_route(ctx, next, false, true);
    }

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

struct InitialChangePick {
    change_id: String,
    url_change_matched: bool,
}

async fn resolve_initial_change_into_response(
    resp: &mut tellit_types::JjChangesResponse,
    url_change_id: Option<&str>,
    initial_target: Option<&str>,
) -> Option<InitialChangePick> {
    if let Some(req_cid) = url_change_id {
        if let Some(found_cid) = find_matching_change_id(&resp.changes, req_cid) {
            return Some(InitialChangePick {
                change_id: found_cid,
                url_change_matched: true,
            });
        }
        if let Ok(extra) = api::fetch_jj_changes(Some(req_cid)).await
            && let Some(found) = extra.changes.into_iter().next()
        {
            let cid = found.change_id.clone();
            if !resp.changes.iter().any(|c| c.change_id == cid) {
                resp.graph_rows.insert(0, JjGraphRow::standalone(&found));
                resp.changes.insert(0, found);
            }
            return Some(InitialChangePick {
                change_id: cid,
                url_change_matched: true,
            });
        }
    }

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

    picked_id.map(|change_id| InitialChangePick {
        change_id,
        url_change_matched: url_change_id.is_none(),
    })
}

fn finalize_initial_change_selection(ctx: AppCtx, pick: InitialChangePick) {
    let cid = pick.change_id;
    let cur_route = ctx.route.get_untracked();
    let canonical_route = if !pick.url_change_matched {
        let missing = cur_route.change_id().unwrap_or_default();
        ctx.error_banner.set(Some(alloc::format!(
            "Change '{missing}' was not found; showing active revision instead."
        )));
        ctx.pending_initial_target.set_value(None);
        ctx.pending_initial_base.set_value(None);
        ctx.ignore_rebase.set(true);
        AppRoute::ChangeOverview {
            change_id: cid.clone(),
            query: RouteQuery::default(),
        }
    } else {
        match cur_route {
            AppRoute::FileDiff {
                file_path, query, ..
            } => AppRoute::FileDiff {
                change_id: cid.clone(),
                file_path,
                query,
            },
            AppRoute::ChangeOverview { query, .. } => AppRoute::ChangeOverview {
                change_id: cid.clone(),
                query,
            },
            AppRoute::Root => {
                let head_commit = ctx.changes.with_untracked(|list| {
                    list.iter()
                        .find(|c| c.change_id == cid)
                        .map(|c| c.commit_id.clone())
                });
                let init_target = ctx.pending_initial_target.get_value();
                let init_base = ctx
                    .pending_initial_base
                    .get_value()
                    .unwrap_or_else(|| String::from("parent"));
                let query = RouteQuery::from_ui_state(
                    init_target.as_deref(),
                    head_commit.as_deref(),
                    &init_base,
                    ctx.ignore_rebase.get_untracked(),
                );
                AppRoute::ChangeOverview {
                    change_id: cid.clone(),
                    query,
                }
            }
        }
    };
    ctx.route.set(canonical_route.clone());
    ctx.selected_file_path
        .set(canonical_route.file_path().map(String::from));
    route::replace_browser_route(&canonical_route);
    select_change_inner(ctx, cid, false);
}

fn load_initial_jj_data(ctx: AppCtx) {
    let changes_id = next_req_id(ctx.changes_req_id);
    ctx.loading_changes.set(true);
    spawn_local(async move {
        let url_change_id = ctx
            .route
            .with_untracked(|r| r.change_id().map(String::from));
        let initial_target = ctx.pending_initial_target.get_value();
        match api::fetch_jj_changes(None).await {
            Ok(mut resp) => {
                if ctx.changes_req_id.get_value() != changes_id {
                    return;
                }
                let picked = resolve_initial_change_into_response(
                    &mut resp,
                    url_change_id.as_deref(),
                    initial_target.as_deref(),
                )
                .await;
                if ctx.changes_req_id.get_value() != changes_id {
                    return;
                }

                ctx.changes.set(resp.changes);
                ctx.graph_rows.set(resp.graph_rows);
                ctx.loading_changes.set(false);

                if let Some(pick) = picked
                    && ctx.selected_change_id.get_untracked().is_empty()
                {
                    finalize_initial_change_selection(ctx, pick);
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
            let url_change_id = ctx
                .route
                .with_untracked(|r| r.change_id().map(String::from));
            let initial_target = ctx.pending_initial_target.get_value();
            let picked = resolve_initial_change_into_response(
                &mut resp,
                url_change_id.as_deref(),
                initial_target.as_deref(),
            )
            .await;
            if ctx.changes_req_id.get_value() != changes_id {
                return;
            }
            ctx.changes.set(resp.changes);
            ctx.graph_rows.set(resp.graph_rows);
            ctx.loading_changes.set(false);
            ctx.reloading.set(false);
            if let Some(pick) = picked
                && ctx.selected_change_id.get_untracked().is_empty()
            {
                finalize_initial_change_selection(ctx, pick);
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
                sync_browser_query(ctx);
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
                let cur_route = ctx.route.get_untracked();
                match cur_route.query() {
                    Some(q) => {
                        ctx.pending_initial_target.set_value(q.target.clone());
                        ctx.pending_initial_base.set_value(q.base.clone());
                    }
                    None => {
                        ctx.pending_initial_target
                            .set_value(status.initial_target.clone());
                        ctx.pending_initial_base
                            .set_value(status.initial_base.clone());
                    }
                }
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

fn jump_to_thread(ctx: AppCtx, thread: JjCommentThread) {
    let in_overview_expanded = matches!(ctx.route.get_untracked(), AppRoute::ChangeOverview { .. })
        && ctx
            .overview_expanded_files
            .with_untracked(|set| set.contains(&thread.file_path));

    if in_overview_expanded {
        ctx.selected_file_path.set(Some(thread.file_path.clone()));
    } else {
        let already_on_file = ctx
            .route
            .with_untracked(|r| r.file_path() == Some(thread.file_path.as_str()));
        if !already_on_file {
            navigate_to_file(ctx, thread.file_path.clone(), false);
        }
    }

    mark_file_near_viewport(ctx, &thread.file_path);
    let mut needs_hydration = false;
    if let Some(slot) = ctx
        .file_slot_map
        .try_with_value(|map| map.get(&thread.file_path).cloned())
        .flatten()
    {
        if slot
            .visible_line_limit
            .try_get_untracked()
            .unwrap_or(usize::MAX)
            < usize::MAX
        {
            let _ = slot.visible_line_limit.try_set(usize::MAX);
            let _ = ctx.hydrated_files_rev.try_update(|rev| *rev += 1);
        }
        needs_hydration = slot
            .file
            .try_with_untracked(|f| !f.chunks_loaded)
            .unwrap_or(false);
    }

    let thread_dom = state::thread_element_id(&thread.id);
    let file_dom = state::file_element_id(&thread.file_path);

    if needs_hydration {
        let generation = ctx.diff_generation.get_value();
        ctx.pending_thread_scroll.set_value(Some((
            generation,
            thread.file_path.clone(),
            thread_dom,
        )));
        enqueue_file_diff_fetch_priority(ctx, thread.file_path);
        if in_overview_expanded {
            let _ = keyboard::scroll_to_element_id(&file_dom, ScrollLogicalPosition::Start);
        }
        return;
    }

    ctx.pending_thread_scroll.set_value(None);
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
    let Some(diff) = ctx.current_diff() else {
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
    let Some(diff) = ctx.current_diff() else {
        return;
    };
    let generation = ctx.diff_generation.get_value();
    ctx.expand_loading.set(true);

    spawn_local(async move {
        let path = req.file.path.clone();
        let old_path = req.file.old_path.as_deref().unwrap_or(&path).to_owned();

        let (need_old, need_new) = ctx.expanded_lines.with_untracked(|map| {
            let existing = map.get(&path);
            (
                req.file.status != FileStatus::Added
                    && existing.is_none_or(|e| e.old_content.is_none()),
                req.file.status != FileStatus::Deleted
                    && existing.is_none_or(|e| e.new_content.is_none()),
            )
        });

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
    let viewport_observer = StoredValue::new_local(create_viewport_observer(ctx));

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
    let pending_sidebar_width = StoredValue::new_local(Option::<u32>::None);
    let resize_raf_id = StoredValue::new_local(Option::<i32>::None);
    let resize_raf_cb = StoredValue::new_local(Option::<Closure<dyn FnMut()>>::None);
    let highlight_sync_pending = StoredValue::new_local(false);
    let marked_kbd_els = StoredValue::new_local(Vec::<web_sys::Element>::new());
    let kbd_sync_pending = StoredValue::new_local(false);

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

    // Apply CSS Custom Highlight API matches on a microtask after Leptos flushes DOM updates,
    // avoiding per-token DOM spans and ensuring mode/route/expansion renders are already mounted.
    Effect::new(move |_| {
        if ctx.highlighted_word.with(Option::is_none) {
            word_highlight::clear_css_word_highlight();
            return;
        }
        // Track layout/content mutations only while an active highlight exists.
        ctx.route.track();
        ctx.diff_data.track();
        ctx.ordered_file_slots.track();
        ctx.hydrated_files_rev.track();
        ctx.diff_mode.track();
        ctx.overview_expanded_files.track();
        ctx.expanded_lines.track();

        if highlight_sync_pending.get_value() {
            return;
        }
        highlight_sync_pending.set_value(true);
        spawn_local(async move {
            let _ = JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED)).await;
            let _ = highlight_sync_pending.try_set_value(false);
            match ctx.highlighted_word.try_get_untracked().flatten() {
                Some(word) => word_highlight::apply_css_word_highlight(&word),
                None => word_highlight::clear_css_word_highlight(),
            }
        });
    });

    // Toggle `diff-row--kbd-active` on the single focused row (or split gutter + code pair)
    // without creating per-row reactive effects across tens of thousands of diff lines.
    Effect::new(move |_| {
        if ctx.active_cursor.with(Option::is_none) {
            marked_kbd_els.update_value(|els| {
                for el in els.drain(..) {
                    let _ = el.class_list().remove_1("diff-row--kbd-active");
                }
            });
            return;
        }
        ctx.route.track();
        ctx.diff_mode.track();
        ctx.diff_data.track();
        ctx.ordered_file_slots.track();
        ctx.hydrated_files_rev.track();
        ctx.overview_expanded_files.track();
        ctx.expanded_lines.track();

        if kbd_sync_pending.get_value() {
            return;
        }
        kbd_sync_pending.set_value(true);
        spawn_local(async move {
            let _ = JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED)).await;
            let _ = kbd_sync_pending.try_set_value(false);

            let _ = marked_kbd_els.try_update_value(|els| {
                for el in els.drain(..) {
                    let _ = el.class_list().remove_1("diff-row--kbd-active");
                }
            });

            let Some(cur) = ctx.active_cursor.try_get_untracked().flatten() else {
                return;
            };
            let Some(mode) = ctx.diff_mode.try_get_untracked() else {
                return;
            };
            let Some(files) = ctx.ordered_file_slots.try_with_untracked(|slots| {
                slots
                    .iter()
                    .filter_map(|s| s.file.try_get_untracked())
                    .collect::<Vec<_>>()
            }) else {
                return;
            };
            let Some(element_id) = keyboard::cursor_scroll_id(cur, &files, mode) else {
                return;
            };
            let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
                return;
            };
            let Some(el) = doc.get_element_by_id(&element_id) else {
                return;
            };

            let mut newly_marked = Vec::with_capacity(2);
            let _ = el.class_list().add_1("diff-row--kbd-active");
            if mode == DiffViewMode::Split
                && let Some(code_cell) = el.next_element_sibling()
            {
                let _ = code_cell.class_list().add_1("diff-row--kbd-active");
                newly_marked.push(code_cell);
            }
            newly_marked.push(el);
            let _ = marked_kbd_els.try_set_value(newly_marked);
        });
    });

    // In Single-File View, prioritize hydrating the active file and proactively prefetch its
    // immediate neighbors (i + 1, then i - 1) at normal priority so `[` / `]` stepping is instant.
    Effect::new(move |_| {
        let active_path = ctx.route.with(|r| r.file_path().map(String::from));
        let Some(active_path) = active_path else {
            return;
        };
        let slots = ctx.ordered_file_slots.get();
        let Some(idx) = slots.iter().position(|s| s.path == active_path) else {
            return;
        };
        let active_slot = &slots[idx];
        if !active_slot.is_near_viewport.get_untracked() {
            active_slot.is_near_viewport.set(true);
        }
        if active_slot.file.with_untracked(|f| !f.chunks_loaded) {
            enqueue_file_diff_fetch_priority(ctx, active_path);
        }
        if let Some(next_slot) = slots.get(idx + 1)
            && next_slot.file.with_untracked(|f| !f.chunks_loaded)
        {
            enqueue_file_diff_fetch(ctx, next_slot.path.clone());
        }
        if let Some(prev_idx) = idx.checked_sub(1)
            && let Some(prev_slot) = slots.get(prev_idx)
            && prev_slot.file.with_untracked(|f| !f.chunks_loaded)
        {
            enqueue_file_diff_fetch(ctx, prev_slot.path.clone());
        }
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

    let apply_diff_width_update = Callback::new(move |next: Option<u16>| {
        let clamped = next.map(state::clamp_diff_width_chars);
        ctx.diff_width_chars.set(clamped);
        state::save_diff_width_chars(clamped);
    });

    let mousemove_handle = window_event_listener(ev::mousemove, move |ev: MouseEvent| {
        if !is_resizing.get_untracked() {
            return;
        }
        let (start_x, start_w) = drag_origin.get_untracked();
        let delta = ev.client_x() - start_x;
        let next_w = i64::from(start_w).saturating_add(i64::from(delta)).max(0);
        let clamped = state::clamp_sidebar_width(u32::try_from(next_w).unwrap_or(u32::MAX));
        pending_sidebar_width.set_value(Some(clamped));

        if resize_raf_id.get_value().is_none() {
            if let Some(window) = web_sys::window() {
                let cb = Closure::<dyn FnMut()>::new(move || {
                    resize_raf_id.set_value(None);
                    let mut next = None;
                    pending_sidebar_width.update_value(|slot| {
                        next = slot.take();
                    });
                    if let Some(w) = next {
                        sidebar_width.set(w);
                    }
                });
                if let Ok(id) = window.request_animation_frame(cb.as_ref().unchecked_ref()) {
                    resize_raf_id.set_value(Some(id));
                    resize_raf_cb.set_value(Some(cb));
                } else {
                    sidebar_width.set(clamped);
                }
            } else {
                sidebar_width.set(clamped);
            }
        }
    });

    let mouseup_handle = window_event_listener(ev::mouseup, move |_ev: MouseEvent| {
        if is_resizing.get_untracked() {
            if let Some(id) = resize_raf_id.get_value() {
                resize_raf_id.set_value(None);
                if let Some(window) = web_sys::window() {
                    let _ = window.cancel_animation_frame(id);
                }
            }
            let mut final_w = None;
            pending_sidebar_width.update_value(|slot| {
                final_w = slot.take();
            });
            if let Some(w) = final_w {
                sidebar_width.set(w);
            }
            is_resizing.set(false);
            state::save_storage_u32(
                state::SIDEBAR_WIDTH_STORAGE_KEY,
                sidebar_width.get_untracked(),
            );
        }
        finish_active_line_drag(ctx);
    });

    let keydown_handle = window_event_listener(ev::keydown, move |ev: KeyboardEvent| {
        handle_global_keydown(ctx, &ev);
    });

    let popstate_handle = window_event_listener(ev::popstate, move |_| {
        handle_popstate(ctx);
    });

    on_cleanup(move || {
        if let Some(id) = resize_raf_id.get_value() {
            resize_raf_id.set_value(None);
            if let Some(window) = web_sys::window() {
                let _ = window.cancel_animation_frame(id);
            }
        }
        resize_raf_cb.set_value(None);
        marked_kbd_els.update_value(|els| {
            for el in els.drain(..) {
                let _ = el.class_list().remove_1("diff-row--kbd-active");
            }
        });
        ctx.file_slot_map.update_value(|map| {
            for (_, slot) in map.drain() {
                slot.dispose();
            }
        });
        word_highlight::clear_css_word_highlight();
        viewport_observer.set_value(None);
        heartbeat_handle.set_value(None);
        watch_handle.set_value(None);
        mousemove_handle.remove();
        mouseup_handle.remove();
        keydown_handle.remove();
        popstate_handle.remove();
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
        sync_browser_query(ctx);
        let base = ctx.base_commit_id.get_untracked();
        let base_opt = (base != "parent" && !base.is_empty()).then_some(base);
        load_diff_and_comments(ctx, new_target, base_opt, ctx.ignore_rebase.get_untracked());
    });

    let on_select_base = Callback::new(move |new_base: String| {
        ctx.base_commit_id.set(new_base.clone());
        sync_browser_query(ctx);
        let target = ctx.target_commit_id.get_untracked();
        if !target.is_empty() {
            let base_opt = (new_base != "parent" && !new_base.is_empty()).then_some(new_base);
            load_diff_and_comments(ctx, target, base_opt, ctx.ignore_rebase.get_untracked());
        }
    });

    let on_toggle_ignore_rebase = Callback::new(move |()| {
        let next = !ctx.ignore_rebase.get_untracked();
        ctx.ignore_rebase.set(next);
        sync_browser_query(ctx);
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
        let Some(diff) = ctx.current_diff() else {
            return;
        };
        let in_single_file = ctx
            .route
            .with_untracked(|r| matches!(r, AppRoute::FileDiff { .. }));
        toggle_file_viewed(ctx, &diff.commit, &path, !in_single_file);
    });

    let on_toggle_folder_reviewed =
        Callback::new(move |(folder_path, mark_reviewed): (String, bool)| {
            let Some(diff) = ctx.current_diff() else {
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
            if mark_reviewed {
                ctx.overview_expanded_files.update(|set| {
                    for p in &affected {
                        set.remove(p);
                    }
                });
            }
        });

    let on_toggle_inline_expand = Callback::new(move |path: String| {
        let mut now_expanded = false;
        ctx.overview_expanded_files.update(|set| {
            if !set.remove(&path) {
                set.insert(path.clone());
                now_expanded = true;
            }
        });
        if now_expanded {
            mark_file_near_viewport(ctx, &path);
        }
    });

    let on_expand_all_inline = Callback::new(move |paths: Vec<String>| {
        ctx.overview_expanded_files.update(|set| {
            for path in paths {
                set.insert(path);
            }
        });
    });

    let on_collapse_all_inline = Callback::new(move |()| {
        ctx.overview_expanded_files.set(HashSet::new());
    });

    let on_select_file = Callback::new(move |path: String| {
        navigate_to_file(ctx, path, true);
    });

    let on_back_to_overview = Callback::new(move |()| {
        navigate_to_overview(ctx);
    });

    let on_viewed_and_next = Callback::new(move |path: String| {
        let Some(diff) = ctx.current_diff() else {
            return;
        };
        let live_files: Vec<Arc<DiffFile>> = ctx
            .ordered_file_slots
            .with_untracked(|slots| slots.iter().map(|s| s.file.get_untracked()).collect());
        let files: &[Arc<DiffFile>] = if live_files.is_empty() {
            &diff.files
        } else {
            &live_files
        };
        mark_viewed_and_advance_single_file(ctx, &diff.commit, &path, files);
    });

    let on_register_card = Callback::new(move |(path, el): (String, web_sys::Element)| {
        let observed = viewport_observer.with_value(|opt| {
            if let Some(handle) = opt {
                handle.observer.observe(&el);
                true
            } else {
                false
            }
        });
        if !observed {
            mark_file_near_viewport(ctx, &path);
        }
    });

    let on_unregister_card = Callback::new(move |el: web_sys::Element| {
        let _ = viewport_observer.try_with_value(|opt| {
            if let Some(handle) = opt {
                handle.observer.unobserve(&el);
            }
        });
    });

    let on_request_file_diff = Callback::new(move |path: String| {
        enqueue_file_diff_fetch(ctx, path);
    });

    let on_window_expanded = Callback::new(move |()| {
        let _ = ctx.hydrated_files_rev.try_update(|rev| *rev += 1);
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
        let Some(diff) = ctx.current_diff() else {
            evt.on_done.run(Err(String::from("No active diff loaded")));
            return;
        };
        let base_state = ctx.base_commit_id.get_untracked();
        let is_interdiff = base_state != "parent" && !base_state.is_empty();
        let commit_id = if evt.side == CommentSide::Old && is_interdiff {
            diff.base_commit.clone()
        } else {
            diff.commit.clone()
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

    let diff_card_callbacks = DiffCardCallbacks {
        on_register_card,
        on_unregister_card,
        on_request_file_diff,
        on_window_expanded,
        on_toggle_viewed: on_toggle_reviewed,
        on_open_in_editor,
        on_prefetch_line_count,
        on_expand_lines,
        on_create_thread,
        on_reply,
        on_toggle_resolve,
    };

    let is_single_file_view =
        Memo::new(move |_| ctx.route.with(|r| matches!(r, AppRoute::FileDiff { .. })));
    let active_route_file_path = Memo::new(move |_| {
        ctx.route
            .with(|r| r.file_path().map_or_else(String::new, String::from))
    });
    let active_route_query =
        Memo::new(move |_| ctx.route.with(|r| r.query().cloned().unwrap_or_default()));

    let comment_counts_by_file = Memo::new(move |_| {
        ctx.comments.with(|list| {
            let mut counts: HashMap<String, usize> = HashMap::new();
            for thread in list {
                *counts.entry(thread.file_path.clone()).or_insert(0) += 1;
            }
            counts
        })
    });

    let diff_files = Memo::new(move |_| {
        ctx.diff_data.with(|d| {
            d.as_ref()
                .map(|d| {
                    d.files
                        .iter()
                        .map(|f| FileTreeEntry::from(f.as_ref()))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
    });

    let is_rebase_diff = Memo::new(move |_| {
        ctx.diff_data
            .with(|d| d.as_ref().is_some_and(|d| d.is_rebase_diff))
    });
    let rebase_ignored = Memo::new(move |_| {
        ctx.diff_data
            .with(|d| d.as_ref().is_some_and(|d| d.rebase_ignored))
    });
    let rebase_conflict = Memo::new(move |_| {
        ctx.diff_data
            .with(|d| d.as_ref().is_some_and(|d| d.rebase_conflict))
    });

    let active_change = Memo::new(move |_| {
        let sel = ctx.selected_change_id.get();
        ctx.changes
            .with(|list| list.iter().find(|c| c.change_id == sel).cloned())
    });

    let active_commit = Memo::new(move |_| {
        let target = ctx.target_commit_id.get();
        ctx.commits.with(|list| {
            list.iter()
                .enumerate()
                .find(|(_, c)| commit_ids_match(&c.commit_id, &target))
                .map(|(i, c)| (i, c.clone()))
        })
    });

    let active_commit_description = Memo::new(move |_| {
        active_commit.with(|opt| opt.as_ref().map(|(_, c)| c.description.clone()))
    });

    let is_empty_diff = Memo::new(move |_| {
        ctx.diff_data
            .with(|opt| opt.as_ref().is_some_and(|d| d.is_empty))
    });

    let is_jj_workspace = Memo::new(move |_| {
        ctx.repo_status
            .with(|s| matches!(s, RepoStatusState::Loaded(status) if status.is_jj))
    });

    let diff_pane_state = Memo::new(move |_| {
        ctx.diff_data.with(|opt| match opt.as_ref() {
            None if ctx.loading_diff.get() => DiffPaneState::Loading,
            None => DiffPaneState::Idle,
            Some(_) => DiffPaneState::Ready,
        })
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
                                title="Toggle comments panel (Shift+L)"
                                aria-label="Toggle comments panel"
                                aria-pressed=move || ctx.drawer_open.get()
                                on:click=move |_| ctx.drawer_open.update(|v| *v = !*v)
                            >
                                "Comments"
                            </button>
                        </>
                    })}
                    {move || {
                        let (total, viewed) = ctx.diff_data.with(|d| {
                            ctx.reviewed_files.with(|rev| {
                                d.as_ref().map_or((0, 0), |d| {
                                    let done =
                                        d.files.iter().filter(|f| rev.contains(&f.path)).count();
                                    (d.files.len(), done)
                                })
                            })
                        });
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

                <main
                    class="diff-main"
                    data-diff-mode=move || ctx.diff_mode.get().as_str()
                    data-diff-width=move || if ctx.diff_width_chars.get().is_some() {
                        "fixed"
                    } else {
                        "full"
                    }
                    style=move || state::format_diff_width_css_var(ctx.diff_width_chars.get())
                    on:mousedown=move |ev: MouseEvent| handle_diff_main_mousedown(ctx, &ev)
                    on:mouseover=move |ev: MouseEvent| handle_diff_main_mouseover(ctx, &ev)
                    on:mouseleave=move |_| {
                        if ctx.hovered_file_index.get_untracked().is_some() {
                            ctx.hovered_file_index.set(None);
                        }
                    }
                    on:click=move |ev: MouseEvent| {
                        handle_diff_main_click(&ev, on_open_in_editor, on_toggle_word);
                    }
                >
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

                                    {move || match diff_pane_state.get() {
                                        DiffPaneState::Loading => view! {
                                            <div class="center-message">"Loading diff..."</div>
                                        }.into_any(),
                                        DiffPaneState::Idle => view! {
                                            <div class="center-message">"Select a revision to inspect its diff."</div>
                                        }.into_any(),
                                        DiffPaneState::Ready => view! {
                                            {move || if is_single_file_view.get() {
                                                view! {
                                                    <SingleFileView
                                                        change_id=ctx.selected_change_id
                                                        route_query=active_route_query
                                                        file_path=active_route_file_path
                                                        ordered_file_slots=ctx.ordered_file_slots
                                                        reviewed_files=ctx.reviewed_files
                                                        all_threads=ctx.comments
                                                        diff_mode=ctx.diff_mode
                                                        show_resolved_inline=ctx.show_resolved_inline
                                                        expanded_lines=ctx.expanded_lines
                                                        expand_loading=ctx.expand_loading
                                                        callbacks=diff_card_callbacks
                                                        on_back_to_overview=on_back_to_overview
                                                        on_open_file=on_select_file
                                                        on_viewed_and_next=on_viewed_and_next
                                                    />
                                                }.into_any()
                                            } else {
                                                view! {
                                                    <ChangeOverview
                                                        change_id=ctx.selected_change_id
                                                        route_query=active_route_query
                                                        active_change=active_change
                                                        active_commit_description=active_commit_description
                                                        selected_file_path=ctx.selected_file_path
                                                        is_empty_diff=is_empty_diff
                                                        ordered_file_slots=ctx.ordered_file_slots
                                                        overview_expanded_files=ctx.overview_expanded_files
                                                        reviewed_files=ctx.reviewed_files
                                                        all_threads=ctx.comments
                                                        diff_mode=ctx.diff_mode
                                                        show_resolved_inline=ctx.show_resolved_inline
                                                        expanded_lines=ctx.expanded_lines
                                                        expand_loading=ctx.expand_loading
                                                        callbacks=diff_card_callbacks
                                                        on_open_file=on_select_file
                                                        on_toggle_inline_expand=on_toggle_inline_expand
                                                        on_expand_all_inline=on_expand_all_inline
                                                        on_collapse_all_inline=on_collapse_all_inline
                                                    />
                                                }.into_any()
                                            }}
                                        }.into_any(),
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
                diff_width_chars=ctx.diff_width_chars
                on_change=apply_settings_update
                on_change_diff_width=apply_diff_width_update
                on_close=Callback::new(move |()| ctx.settings_open.set(false))
            />
            <HelpModal
                is_open=ctx.help_open
                on_close=Callback::new(move |()| ctx.help_open.set(false))
            />
        </div>
    }
}

fn event_target_element(ev: &MouseEvent) -> Option<web_sys::Element> {
    let target = ev.target()?;
    if let Ok(el) = target.clone().dyn_into::<web_sys::Element>() {
        return Some(el);
    }
    target
        .dyn_into::<web_sys::Node>()
        .ok()
        .and_then(|n| n.parent_element())
}

fn handle_diff_main_mousedown(ctx: AppCtx, ev: &MouseEvent) {
    if ev.button() != 0 {
        return;
    }
    let Some(target) = event_target_element(ev) else {
        return;
    };
    // Do not start a gutter selection drag when clicking the open-in-editor action button.
    if target
        .closest("[data-line-action='open-editor']")
        .ok()
        .flatten()
        .is_some()
    {
        return;
    }
    let Some(gutter_el) = target
        .closest("[data-gutter-side][data-gutter-line]")
        .ok()
        .flatten()
    else {
        return;
    };
    let Some(card_el) = gutter_el.closest("[data-file]").ok().flatten() else {
        return;
    };
    let Some(file_path) = card_el.get_attribute("data-file") else {
        return;
    };
    let Some(side) = gutter_el
        .get_attribute("data-gutter-side")
        .as_deref()
        .and_then(parse_comment_side)
    else {
        return;
    };
    let Some(line_num) = gutter_el
        .get_attribute("data-gutter-line")
        .and_then(|s| s.parse::<u32>().ok())
    else {
        return;
    };

    let Some(slot) = ctx
        .file_slot_map
        .with_value(|map| map.get(&file_path).cloned())
    else {
        return;
    };

    ev.prevent_default();

    if ev.shift_key() {
        let (start, end) = ctx.selection_anchor.with_value(|anchor| match anchor {
            Some((prev_file, prev_side, prev_line))
                if *prev_file == file_path && *prev_side == side =>
            {
                ((*prev_line).min(line_num), (*prev_line).max(line_num))
            }
            _ => (line_num, line_num),
        });
        ctx.selection_anchor
            .set_value(Some((file_path.clone(), side, line_num)));
        let range = LineRangeSpec { start, end };
        let file_arc = slot.file.get_untracked();
        let snippet = ctx.expanded_lines.with_untracked(|map| {
            extract_snippet_for_file(&file_arc, map.get(&file_path), side, range)
        });
        slot.active_composer.set(Some(ActiveComposer {
            side,
            range,
            code_snippet: snippet,
        }));
        return;
    }

    ctx.selection_anchor
        .set_value(Some((file_path.clone(), side, line_num)));
    ctx.active_drag_file.set_value(Some(file_path));
    slot.drag_state.set(Some(LineSelectionDrag {
        side,
        anchor_line: line_num,
        current_line: line_num,
    }));
}

fn handle_diff_main_mouseover(ctx: AppCtx, ev: &MouseEvent) {
    let Some(target) = event_target_element(ev) else {
        return;
    };

    let hovered_idx = target
        .closest("[data-file-idx]")
        .ok()
        .flatten()
        .and_then(|el| el.get_attribute("data-file-idx"))
        .and_then(|s| s.parse::<usize>().ok());
    if ctx.hovered_file_index.get_untracked() != hovered_idx {
        ctx.hovered_file_index.set(hovered_idx);
    }

    let Some(drag_file) = ctx.active_drag_file.with_value(Clone::clone) else {
        return;
    };
    let Some(row_el) = target
        .closest("tr[data-old-line], tr[data-new-line]")
        .ok()
        .flatten()
    else {
        return;
    };
    let Some(card_el) = row_el.closest("[data-file]").ok().flatten() else {
        return;
    };
    if card_el.get_attribute("data-file").as_deref() != Some(drag_file.as_str()) {
        return;
    }
    let Some(slot) = ctx
        .file_slot_map
        .with_value(|map| map.get(&drag_file).cloned())
    else {
        return;
    };
    let Some(drag) = slot.drag_state.get_untracked() else {
        return;
    };
    let attr = match drag.side {
        CommentSide::Old => "data-old-line",
        CommentSide::New => "data-new-line",
    };
    if let Some(ln) = row_el
        .get_attribute(attr)
        .and_then(|s| s.parse::<u32>().ok())
        && ln != drag.current_line
    {
        slot.drag_state.set(Some(LineSelectionDrag {
            current_line: ln,
            ..drag
        }));
    }
}

fn finish_active_line_drag(ctx: AppCtx) {
    let mut drag_file = None;
    ctx.active_drag_file.update_value(|slot| {
        drag_file = slot.take();
    });
    let Some(file_path) = drag_file else {
        return;
    };
    let Some(slot) = ctx
        .file_slot_map
        .with_value(|map| map.get(&file_path).cloned())
    else {
        return;
    };
    let Some(drag) = slot.drag_state.get_untracked() else {
        return;
    };
    slot.drag_state.set(None);

    let range = LineRangeSpec {
        start: drag.anchor_line.min(drag.current_line),
        end: drag.anchor_line.max(drag.current_line),
    };
    let file_arc = slot.file.get_untracked();
    let snippet = ctx.expanded_lines.with_untracked(|map| {
        extract_snippet_for_file(&file_arc, map.get(&file_path), drag.side, range)
    });
    slot.active_composer.set(Some(ActiveComposer {
        side: drag.side,
        range,
        code_snippet: snippet,
    }));
}

fn handle_diff_main_click(
    ev: &MouseEvent,
    on_open_in_editor: Callback<(String, usize)>,
    on_toggle_word: Callback<String>,
) {
    let Some(target) = event_target_element(ev) else {
        return;
    };
    if let Some(btn_el) = target
        .closest("[data-line-action='open-editor']")
        .ok()
        .flatten()
    {
        if let Some(card_el) = btn_el.closest("[data-file]").ok().flatten()
            && let Some(file_path) = card_el.get_attribute("data-file")
            && let Some(line_n) = btn_el
                .get_attribute("data-action-line")
                .and_then(|s| s.parse::<usize>().ok())
        {
            ev.stop_propagation();
            on_open_in_editor.run((file_path, line_n));
        }
        return;
    }

    if let Some(code_el) = target.closest(".diff-code-text").ok().flatten()
        && let Some(word) =
            word_highlight::extract_word_at_click_point(&code_el, ev.client_x(), ev.client_y())
    {
        on_toggle_word.run(word);
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

    let Some(diff) = ctx.current_diff() else {
        return;
    };
    let live_files: Vec<Arc<DiffFile>> = ctx
        .ordered_file_slots
        .with_untracked(|slots| slots.iter().map(|s| s.file.get_untracked()).collect());
    let files: &[Arc<DiffFile>] = if live_files.is_empty() {
        &diff.files
    } else {
        &live_files
    };
    let mode = ctx.diff_mode.get_untracked();

    match key.as_str() {
        "j" | "ArrowDown" => {
            ev.prevent_default();
            step_keyboard_cursor(
                ctx,
                NavigationDirection::Next,
                NavigationTarget::Line,
                files,
                mode,
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
            );
        }
        "]" if !files.is_empty() => {
            ev.prevent_default();
            if let Some(active_path) = ctx
                .route
                .with_untracked(|r| r.file_path().map(String::from))
            {
                if let Some(cur_idx) = files.iter().position(|f| f.path == active_path)
                    && let Some(next_idx) = keyboard::adjacent_file_index(
                        cur_idx,
                        files.len(),
                        NavigationDirection::Next,
                    )
                {
                    navigate_to_file(ctx, files[next_idx].path.clone(), true);
                }
            } else {
                let next_idx = ctx
                    .selected_file_path
                    .with_untracked(|sel| {
                        sel.as_deref()
                            .and_then(|p| files.iter().position(|f| f.path == p))
                            .map(|i| (i + 1) % files.len())
                    })
                    .unwrap_or(0);
                navigate_to_file(ctx, files[next_idx].path.clone(), true);
            }
        }
        "[" if !files.is_empty() => {
            ev.prevent_default();
            if let Some(active_path) = ctx
                .route
                .with_untracked(|r| r.file_path().map(String::from))
            {
                if let Some(cur_idx) = files.iter().position(|f| f.path == active_path)
                    && let Some(prev_idx) = keyboard::adjacent_file_index(
                        cur_idx,
                        files.len(),
                        NavigationDirection::Prev,
                    )
                {
                    navigate_to_file(ctx, files[prev_idx].path.clone(), true);
                }
            } else {
                let prev_idx = ctx
                    .selected_file_path
                    .with_untracked(|sel| {
                        sel.as_deref()
                            .and_then(|p| files.iter().position(|f| f.path == p))
                            .map(|i| (i + files.len() - 1) % files.len())
                    })
                    .unwrap_or(files.len() - 1);
                navigate_to_file(ctx, files[prev_idx].path.clone(), true);
            }
        }
        "{" if !files.is_empty() => {
            ev.prevent_default();
            navigate_to_file(ctx, files[0].path.clone(), true);
        }
        "}" if !files.is_empty() => {
            ev.prevent_default();
            navigate_to_file(ctx, files[files.len() - 1].path.clone(), true);
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
                .with_untracked(|list| list.iter().filter(|t| !t.resolved).cloned().collect());
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
            if ctx
                .route
                .with_untracked(|r| matches!(r, AppRoute::FileDiff { .. }))
            {
                ev.prevent_default();
                navigate_to_overview(ctx);
            }
        }
        "U" => {
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
            if let Some(active_path) = ctx
                .route
                .with_untracked(|r| r.file_path().map(String::from))
            {
                ev.prevent_default();
                toggle_file_viewed(ctx, &diff.commit, &active_path, false);
            } else {
                let file_idx = ctx
                    .active_cursor
                    .get_untracked()
                    .map(|c| c.file_index)
                    .or_else(|| ctx.hovered_file_index.get_untracked());
                if let Some(idx) = file_idx
                    && let Some(file) = files.get(idx)
                {
                    ev.prevent_default();
                    toggle_file_viewed(ctx, &diff.commit, &file.path, true);
                }
            }
        }
        "V" => {
            if let Some(active_path) = ctx
                .route
                .with_untracked(|r| r.file_path().map(String::from))
            {
                ev.prevent_default();
                mark_viewed_and_advance_single_file(ctx, &diff.commit, &active_path, files);
            } else {
                let file_idx = ctx
                    .active_cursor
                    .get_untracked()
                    .map(|c| c.file_index)
                    .or_else(|| ctx.hovered_file_index.get_untracked());
                if let Some(idx) = file_idx
                    && let Some(file) = files.get(idx)
                {
                    ev.prevent_default();
                    let already_reviewed = ctx
                        .reviewed_files
                        .with_untracked(|set| set.contains(&file.path));
                    if !already_reviewed {
                        toggle_file_viewed(ctx, &diff.commit, &file.path, true);
                    }
                    let next_idx = ctx.reviewed_files.with_untracked(|updated_reviewed| {
                        keyboard::next_unviewed_file_index(idx, files, updated_reviewed)
                    });
                    if let Some(next_idx) = next_idx {
                        navigate_to_file(ctx, files[next_idx].path.clone(), true);
                    }
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
                if let Some(ln) = line_num
                    && let Some(slot) = ctx
                        .file_slot_map
                        .with_value(|map| map.get(&file.path).cloned())
                {
                    ev.prevent_default();
                    if matches!(ctx.route.get_untracked(), AppRoute::ChangeOverview { .. }) {
                        ctx.overview_expanded_files.update(|set| {
                            set.insert(file.path.clone());
                        });
                    }
                    if file.path != "Commit Message" && state::is_markdown_file(&file.path) {
                        slot.md_tab.set(MarkdownTab::Code);
                    }
                    let range = LineRangeSpec { start: ln, end: ln };
                    let snippet = ctx.expanded_lines.with_untracked(|map| {
                        extract_snippet_for_file(file, map.get(&file.path), cur.side, range)
                    });
                    slot.active_composer.set(Some(ActiveComposer {
                        side: cur.side,
                        range,
                        code_snippet: snippet,
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
            let prompt = ctx.comments.with_untracked(|comments| {
                state::format_all_comments_prompt(
                    comments,
                    &ctx.target_commit_id.get_untracked(),
                    &ctx.base_commit_id.get_untracked(),
                )
            });
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
    files: &[Arc<DiffFile>],
    mode: DiffViewMode,
) {
    let active_single_file = ctx
        .route
        .with_untracked(|r| r.file_path().map(String::from));
    let next = if let Some(ref active_path) = active_single_file {
        let collapsed: HashSet<String> = files
            .iter()
            .filter(|f| f.path != *active_path)
            .map(|f| f.path.clone())
            .collect();
        let empty_reviewed = HashSet::new();
        keyboard::find_next_position(
            ctx.active_cursor.get_untracked(),
            dir,
            target,
            files,
            mode,
            &empty_reviewed,
            &collapsed,
        )
    } else {
        let collapsed: HashSet<String> = ctx.overview_expanded_files.with_untracked(|expanded| {
            files
                .iter()
                .filter(|f| !expanded.contains(&f.path))
                .map(|f| f.path.clone())
                .collect()
        });
        // Any file explicitly expanded inline in Overview should remain keyboard-navigable
        // even if it is already marked Viewed.
        let empty_reviewed = HashSet::new();
        keyboard::find_next_position(
            ctx.active_cursor.get_untracked(),
            dir,
            target,
            files,
            mode,
            &empty_reviewed,
            &collapsed,
        )
    };
    if let Some(next) = next {
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
    files: &[Arc<DiffFile>],
    block: ScrollLogicalPosition,
) {
    if let Some(file) = files.get(pos.file_index) {
        mark_file_near_viewport(ctx, &file.path);
        if !file.chunks_loaded {
            ctx.pending_cursor_scroll_block.set_value(Some(block));
            enqueue_file_diff_fetch_priority(ctx, file.path.clone());
            let _ = keyboard::scroll_to_element_id(&state::file_element_id(&file.path), block);
            return;
        }
    }
    ctx.pending_cursor_scroll_block.set_value(None);
    let mode = ctx.diff_mode.get_untracked();
    if let Some(id) = keyboard::cursor_scroll_id(pos, files, mode) {
        if keyboard::scroll_to_element_id(&id, block) {
            return;
        }
        // If the target row lies past a large file's progressive render window, expand the window.
        if let Some(file) = files.get(pos.file_index)
            && file.chunks_loaded
            && let Some(slot) = ctx
                .file_slot_map
                .try_with_value(|map| map.get(&file.path).cloned())
                .flatten()
            && slot
                .visible_line_limit
                .try_get_untracked()
                .unwrap_or(usize::MAX)
                < usize::MAX
        {
            let _ = slot.visible_line_limit.try_set(usize::MAX);
            let _ = ctx.hydrated_files_rev.try_update(|rev| *rev += 1);
            spawn_local(async move {
                let _ = JsFuture::from(js_sys::Promise::resolve(&JsValue::UNDEFINED)).await;
                let _ = keyboard::scroll_to_element_id(&id, block);
            });
            return;
        }
    }
    if let Some(file) = files.get(pos.file_index) {
        let _ = keyboard::scroll_to_element_id(&state::file_element_id(&file.path), block);
    }
}

fn toggle_file_viewed(ctx: AppCtx, commit: &str, path: &str, collapse_in_overview: bool) {
    let mut now_viewed = false;
    ctx.reviewed_files.update(|set| {
        if !set.remove(path) {
            set.insert(String::from(path));
            now_viewed = true;
        }
        state::save_viewed_files(&ctx.repo_root_string(), commit, set);
    });
    if collapse_in_overview && now_viewed {
        ctx.overview_expanded_files.update(|set| {
            set.remove(path);
        });
    }
}

fn mark_viewed_and_advance_single_file(
    ctx: AppCtx,
    commit: &str,
    path: &str,
    files: &[Arc<DiffFile>],
) {
    let already_reviewed = ctx.reviewed_files.with_untracked(|set| set.contains(path));
    if !already_reviewed {
        toggle_file_viewed(ctx, commit, path, false);
    }
    let cur_idx = files.iter().position(|f| f.path == path).unwrap_or(0);
    let next_idx = ctx
        .reviewed_files
        .with_untracked(|updated| keyboard::next_unviewed_file_index(cur_idx, files, updated));
    if let Some(next_idx) = next_idx {
        navigate_to_file(ctx, files[next_idx].path.clone(), true);
    } else {
        navigate_to_overview(ctx);
    }
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
