//! Debounced filesystem and Jujutsu operation watcher streaming `GET /api/watch` SSE events.

extern crate alloc;

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::convert::Infallible;
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use core::time::Duration;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, RwLock};

use axum::response::sse::Event;
use notify::event::ModifyKind;
use notify::{Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tellit_types::{WatchEvent, WatchSource};
use tokio::sync::{mpsc, watch};
use tokio_stream::Stream;

use super::jujutsu::{find_jj_workspace_root, list_working_copy_diff_files, resolve_jj_repo_paths};

const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(250);
const CLIENT_QUEUE_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchSignal {
    WorkingTree,
    JjOp,
    CommentsChanged,
}

enum WatchCommand {
    SetWorkingCopyFiles(Vec<PathBuf>),
    EnsureStoreWatch,
}

struct RefreshResult {
    generation: u64,
    rel_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
struct WatchTargets {
    workspace_root: PathBuf,
    op_heads_dir: Option<PathBuf>,
    store_dir: PathBuf,
}

impl WatchTargets {
    fn resolve(repo_path: &Path, store_dir: &Path) -> Self {
        let canonical_repo = canonicalize_best_effort(repo_path);
        let workspace_root =
            find_jj_workspace_root(&canonical_repo).unwrap_or_else(|| canonical_repo.clone());
        let jj_paths = resolve_jj_repo_paths(&canonical_repo);
        let op_heads_dir = jj_paths
            .as_ref()
            .map(|p| canonicalize_best_effort(&p.jj_repo_dir.join("op_heads")));
        let resolved_store = canonicalize_future_path(store_dir);

        Self {
            workspace_root,
            op_heads_dir,
            store_dir: resolved_store,
        }
    }
}

#[derive(Default)]
struct ClientRegistry {
    clients: Mutex<Vec<mpsc::Sender<WatchEvent>>>,
}

impl ClientRegistry {
    fn subscribe(&self) -> mpsc::Receiver<WatchEvent> {
        let (tx, rx) = mpsc::channel(CLIENT_QUEUE_CAPACITY);
        if let Ok(mut guard) = self.clients.lock() {
            guard.push(tx);
        }
        rx
    }

    fn broadcast(&self, event: WatchEvent) {
        if let Ok(mut guard) = self.clients.lock() {
            guard.retain(|tx| match tx.try_send(event) {
                Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => true,
                Err(mpsc::error::TrySendError::Closed(_)) => false,
            });
        }
    }
}

/// Manages targeted `notify` filesystem watches and broadcasts [`WatchEvent`] updates to
/// `/api/watch` clients.
pub struct WatchHub {
    registry: Arc<ClientRegistry>,
    shutdown_tx: watch::Sender<bool>,
    cmd_tx: mpsc::UnboundedSender<WatchCommand>,
}

impl WatchHub {
    /// Spawns background watcher initialization for `.tellit/`, `.jj/repo/op_heads`, and the
    /// working copy's diffed files without blocking server startup.
    ///
    /// If initializing the OS watcher fails (for example due to inotify limits), the hub still
    /// delivers API-initiated [`WatchEvent::CommentsChanged`] notifications.
    #[must_use]
    pub fn start(
        repo_path: PathBuf,
        store_dir: PathBuf,
        shutdown_tx: watch::Sender<bool>,
        quiet: bool,
    ) -> Arc<Self> {
        Self::start_with_debounce(repo_path, store_dir, shutdown_tx, DEFAULT_DEBOUNCE, quiet)
    }

    #[must_use]
    fn start_with_debounce(
        repo_path: PathBuf,
        store_dir: PathBuf,
        shutdown_tx: watch::Sender<bool>,
        debounce: Duration,
        quiet: bool,
    ) -> Arc<Self> {
        let registry = Arc::new(ClientRegistry::default());
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        let loop_registry = Arc::clone(&registry);
        let task_shutdown = shutdown_tx.subscribe();
        tokio::spawn(async move {
            run_watch_task(
                repo_path,
                store_dir,
                loop_registry,
                cmd_rx,
                task_shutdown,
                debounce,
                quiet,
            )
            .await;
        });

        Arc::new(Self {
            registry,
            shutdown_tx,
            cmd_tx,
        })
    }

    /// Broadcasts a [`WatchEvent::CommentsChanged`] event immediately to all connected clients.
    pub fn broadcast_comments_changed(&self) {
        self.registry.broadcast(WatchEvent::CommentsChanged);
        let _ = self.cmd_tx.send(WatchCommand::EnsureStoreWatch);
    }

    /// Updates the set of repository-relative working-copy diffed files watched for edits.
    pub fn update_working_copy_files(&self, rel_paths: Vec<PathBuf>) {
        let _ = self
            .cmd_tx
            .send(WatchCommand::SetWorkingCopyFiles(rel_paths));
    }

    /// Opens a new SSE stream starting with [`WatchEvent::Connected`].
    #[must_use]
    pub fn subscribe(&self) -> WatchStream {
        WatchStream::new(self.registry.subscribe(), self.shutdown_tx.subscribe())
    }
}

/// Owns the OS watcher and keeps non-recursive parent directory watches synchronized with the
/// tracked diffed-file set and pending `.tellit/` store parent.
struct WatcherState {
    watcher: RecommendedWatcher,
    targets: WatchTargets,
    tracked_files: Arc<RwLock<HashSet<PathBuf>>>,
    store_watch_attached: bool,
    store_parent_watch: Option<PathBuf>,
    watched_file_dirs: HashSet<PathBuf>,
    active_non_recursive_dirs: HashSet<PathBuf>,
}

impl WatcherState {
    fn new(
        targets: WatchTargets,
        sig_tx: mpsc::UnboundedSender<WatchSignal>,
        quiet: bool,
    ) -> Option<Self> {
        let tracked_files = Arc::new(RwLock::new(HashSet::new()));
        let classify_targets = targets.clone();
        let classify_tracked = Arc::clone(&tracked_files);

        let mut watcher =
            match notify::recommended_watcher(move |res: notify::Result<NotifyEvent>| {
                let Ok(event) = res else {
                    return;
                };
                if !is_relevant_event_kind(&event.kind) {
                    return;
                }
                let Ok(tracked) = classify_tracked.read() else {
                    return;
                };
                for path in &event.paths {
                    if let Some(signal) = classify_watch_path(path, &classify_targets, &tracked) {
                        let _ = sig_tx.send(signal);
                    }
                }
            }) {
                Ok(w) => w,
                Err(err) => {
                    if !quiet {
                        eprintln!("Warning: file watcher disabled: {err}");
                    }
                    return None;
                }
            };

        if let Some(ref op_heads) = targets.op_heads_dir
            && op_heads.exists()
            && let Err(err) = watcher.watch(op_heads, RecursiveMode::Recursive)
            && !quiet
        {
            eprintln!(
                "Warning: failed to watch jj op_heads {}: {err}",
                op_heads.display()
            );
        }

        let mut store_watch_attached = false;
        let mut store_parent_watch = None;
        if targets.store_dir.exists() {
            match watcher.watch(&targets.store_dir, RecursiveMode::Recursive) {
                Ok(()) => store_watch_attached = true,
                Err(err) if !quiet => {
                    eprintln!(
                        "Warning: failed to watch review store {}: {err}",
                        targets.store_dir.display()
                    );
                }
                Err(_) => {}
            }
        } else if let Some(parent) = targets.store_dir.parent()
            && parent.is_dir()
        {
            store_parent_watch = Some(canonicalize_best_effort(parent));
        }

        let mut state = Self {
            watcher,
            targets,
            tracked_files,
            store_watch_attached,
            store_parent_watch,
            watched_file_dirs: HashSet::new(),
            active_non_recursive_dirs: HashSet::new(),
        };
        state.sync_non_recursive_dirs();
        Some(state)
    }

    fn try_attach_store_watch(&mut self) {
        if self.store_watch_attached || !self.targets.store_dir.exists() {
            return;
        }
        if self
            .watcher
            .watch(&self.targets.store_dir, RecursiveMode::Recursive)
            .is_ok()
        {
            self.store_watch_attached = true;
            self.store_parent_watch = None;
            self.sync_non_recursive_dirs();
        }
    }

    fn set_working_copy_files(&mut self, rel_paths: &[PathBuf]) {
        let mut next_files = HashSet::with_capacity(rel_paths.len());
        let mut next_dirs = HashSet::new();

        for rel in rel_paths {
            let Some(full_path) = resolve_tracked_file_path(&self.targets.workspace_root, rel)
            else {
                continue;
            };
            if full_path.starts_with(&self.targets.store_dir)
                || self
                    .targets
                    .op_heads_dir
                    .as_ref()
                    .is_some_and(|dir| full_path.starts_with(dir))
            {
                continue;
            }
            if let Some(parent) = full_path.parent()
                && parent.is_dir()
            {
                next_dirs.insert(parent.to_path_buf());
            }
            next_files.insert(full_path);
        }

        if let Ok(mut guard) = self.tracked_files.write() {
            *guard = next_files;
        }
        self.watched_file_dirs = next_dirs;
        self.sync_non_recursive_dirs();
    }

    fn is_covered_by_recursive_watch(&self, dir: &Path) -> bool {
        (self.store_watch_attached && dir.starts_with(&self.targets.store_dir))
            || self
                .targets
                .op_heads_dir
                .as_ref()
                .is_some_and(|op_heads| dir.starts_with(op_heads))
    }

    fn sync_non_recursive_dirs(&mut self) {
        let desired: HashSet<PathBuf> = self
            .watched_file_dirs
            .iter()
            .chain(self.store_parent_watch.iter())
            .filter(|dir| dir.is_dir() && !self.is_covered_by_recursive_watch(dir))
            .cloned()
            .collect();

        let to_remove: Vec<PathBuf> = self
            .active_non_recursive_dirs
            .difference(&desired)
            .cloned()
            .collect();
        for dir in to_remove {
            let _ = self.watcher.unwatch(&dir);
            self.active_non_recursive_dirs.remove(&dir);
        }

        let to_add: Vec<PathBuf> = desired
            .difference(&self.active_non_recursive_dirs)
            .cloned()
            .collect();
        for dir in to_add {
            if self
                .watcher
                .watch(&dir, RecursiveMode::NonRecursive)
                .is_ok()
            {
                self.active_non_recursive_dirs.insert(dir);
            }
        }
    }
}

async fn run_watch_task(
    repo_path: PathBuf,
    store_dir: PathBuf,
    registry: Arc<ClientRegistry>,
    mut cmd_rx: mpsc::UnboundedReceiver<WatchCommand>,
    mut shutdown_rx: watch::Receiver<bool>,
    debounce: Duration,
    quiet: bool,
) {
    let (sig_tx, mut sig_rx) = mpsc::unbounded_channel();
    let init = tokio::task::spawn_blocking(move || {
        let targets = WatchTargets::resolve(&repo_path, &store_dir);
        WatcherState::new(targets, sig_tx, quiet)
    })
    .await;

    let Ok(Some(mut state)) = init else {
        return;
    };
    if *shutdown_rx.borrow_and_update() {
        return;
    }

    let (refresh_tx, mut refresh_rx) = mpsc::unbounded_channel::<RefreshResult>();
    let mut generation: u64 = 0;

    if state.targets.op_heads_dir.is_some() {
        generation = generation.wrapping_add(1);
        spawn_working_copy_refresh(
            state.targets.workspace_root.clone(),
            generation,
            refresh_tx.clone(),
        );
    }

    let mut cmd_open = true;

    loop {
        let first = tokio::select! {
            () = wait_for_shutdown(&mut shutdown_rx) => return,
            maybe_cmd = cmd_rx.recv(), if cmd_open => {
                match maybe_cmd {
                    Some(cmd) => apply_command(cmd, &mut state, &mut generation),
                    None => cmd_open = false,
                }
                continue;
            }
            Some(result) = refresh_rx.recv() => {
                if result.generation == generation {
                    state.set_working_copy_files(&result.rel_paths);
                }
                continue;
            }
            maybe_sig = sig_rx.recv() => match maybe_sig {
                Some(sig) => sig,
                None => return,
            }
        };

        if first == WatchSignal::CommentsChanged {
            state.try_attach_store_watch();
        }
        let mut pending = PendingSignals::default();
        pending.record(first);

        let timer = tokio::time::sleep(debounce);
        tokio::pin!(timer);

        loop {
            tokio::select! {
                () = wait_for_shutdown(&mut shutdown_rx) => return,
                () = &mut timer => {
                    let had_jj_op = pending.jj_op;
                    pending.flush(&registry);
                    if had_jj_op && state.targets.op_heads_dir.is_some() {
                        generation = generation.wrapping_add(1);
                        spawn_working_copy_refresh(
                            state.targets.workspace_root.clone(),
                            generation,
                            refresh_tx.clone(),
                        );
                    }
                    break;
                }
                maybe_cmd = cmd_rx.recv(), if cmd_open => {
                    match maybe_cmd {
                        Some(cmd) => apply_command(cmd, &mut state, &mut generation),
                        None => cmd_open = false,
                    }
                }
                Some(result) = refresh_rx.recv() => {
                    if result.generation == generation {
                        state.set_working_copy_files(&result.rel_paths);
                    }
                }
                maybe_sig = sig_rx.recv() => match maybe_sig {
                    Some(sig) => {
                        if sig == WatchSignal::CommentsChanged {
                            state.try_attach_store_watch();
                        }
                        pending.record(sig);
                    }
                    None => {
                        pending.flush(&registry);
                        return;
                    }
                }
            }
        }
    }
}

fn apply_command(cmd: WatchCommand, state: &mut WatcherState, generation: &mut u64) {
    match cmd {
        WatchCommand::SetWorkingCopyFiles(rel_paths) => {
            *generation = generation.wrapping_add(1);
            state.set_working_copy_files(&rel_paths);
        }
        WatchCommand::EnsureStoreWatch => {
            state.try_attach_store_watch();
        }
    }
}

fn spawn_working_copy_refresh(
    workspace_root: PathBuf,
    generation: u64,
    refresh_tx: mpsc::UnboundedSender<RefreshResult>,
) {
    tokio::spawn(async move {
        if let Some(rel_paths) = list_working_copy_diff_files(&workspace_root).await {
            let _ = refresh_tx.send(RefreshResult {
                generation,
                rel_paths,
            });
        }
    });
}

#[derive(Default)]
struct PendingSignals {
    working_tree: bool,
    jj_op: bool,
    comments_changed: bool,
}

impl PendingSignals {
    fn record(&mut self, sig: WatchSignal) {
        match sig {
            WatchSignal::WorkingTree => self.working_tree = true,
            WatchSignal::JjOp => self.jj_op = true,
            WatchSignal::CommentsChanged => self.comments_changed = true,
        }
    }

    fn flush(&mut self, registry: &ClientRegistry) {
        if self.comments_changed {
            registry.broadcast(WatchEvent::CommentsChanged);
        }
        if self.jj_op {
            registry.broadcast(WatchEvent::Reload {
                source: WatchSource::JjOp,
            });
        } else if self.working_tree {
            registry.broadcast(WatchEvent::Reload {
                source: WatchSource::WorkingTree,
            });
        }
        *self = Self::default();
    }
}

async fn wait_for_shutdown(shutdown_rx: &mut watch::Receiver<bool>) {
    while !*shutdown_rx.borrow_and_update() {
        if shutdown_rx.changed().await.is_err() {
            break;
        }
    }
}

fn is_relevant_event_kind(kind: &EventKind) -> bool {
    !matches!(
        kind,
        EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(_))
    )
}

fn classify_watch_path(
    path: &Path,
    targets: &WatchTargets,
    tracked_files: &HashSet<PathBuf>,
) -> Option<WatchSignal> {
    if path.starts_with(&targets.store_dir) {
        return Some(WatchSignal::CommentsChanged);
    }
    if targets
        .op_heads_dir
        .as_ref()
        .is_some_and(|dir| path.starts_with(dir))
    {
        return Some(WatchSignal::JjOp);
    }
    if tracked_files.contains(path) {
        return Some(WatchSignal::WorkingTree);
    }
    None
}

fn resolve_tracked_file_path(workspace_root: &Path, rel_path: &Path) -> Option<PathBuf> {
    if rel_path.as_os_str().is_empty() || rel_path.is_absolute() {
        return None;
    }

    let mut clean = PathBuf::new();
    for comp in rel_path.components() {
        match comp {
            Component::Normal(seg) => {
                if matches!(seg.to_str(), Some(".git" | ".jj")) {
                    return None;
                }
                clean.push(seg);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if clean.as_os_str().is_empty() {
        return None;
    }

    let joined = workspace_root.join(&clean);
    match (joined.parent(), joined.file_name()) {
        (Some(parent), Some(name)) if parent.is_dir() => {
            Some(canonicalize_best_effort(parent).join(name))
        }
        _ => Some(joined),
    }
}

fn canonicalize_best_effort(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn canonicalize_future_path(path: &Path) -> PathBuf {
    if path.exists() {
        return canonicalize_best_effort(path);
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) if parent.exists() => {
            canonicalize_best_effort(parent).join(name)
        }
        _ => path.to_path_buf(),
    }
}

/// SSE stream emitting [`WatchEvent::Connected`] followed by live repository and comment updates.
pub struct WatchStream {
    rx: mpsc::Receiver<WatchEvent>,
    sent_initial: bool,
    closed: bool,
    shutdown_fut: Pin<Box<dyn Future<Output = ()> + Send>>,
}

impl WatchStream {
    fn new(rx: mpsc::Receiver<WatchEvent>, mut shutdown_rx: watch::Receiver<bool>) -> Self {
        let shutdown_fut = Box::pin(async move {
            wait_for_shutdown(&mut shutdown_rx).await;
        });
        Self {
            rx,
            sent_initial: false,
            closed: false,
            shutdown_fut,
        }
    }
}

impl Stream for WatchStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.closed {
            return Poll::Ready(None);
        }

        if self.shutdown_fut.as_mut().poll(cx).is_ready() {
            self.closed = true;
            return Poll::Ready(None);
        }

        if !self.sent_initial {
            self.sent_initial = true;
            if let Ok(json) = serde_json::to_string(&WatchEvent::Connected) {
                return Poll::Ready(Some(Ok(Event::default().data(json))));
            }
        }

        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(event)) => match serde_json::to_string(&event) {
                Ok(json) => Poll::Ready(Some(Ok(Event::default().data(json)))),
                Err(_) => Poll::Pending,
            },
            Poll::Ready(None) => {
                self.closed = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_tracked_files_op_heads_store_and_ignores_unrelated_paths() {
        let targets = WatchTargets {
            workspace_root: PathBuf::from("/workspace/repo"),
            op_heads_dir: Some(PathBuf::from("/workspace/repo/.jj/repo/op_heads")),
            store_dir: PathBuf::from("/workspace/repo/.tellit"),
        };
        let tracked = HashSet::from([PathBuf::from("/workspace/repo/src/lib.rs")]);

        assert_eq!(
            classify_watch_path(Path::new("/workspace/repo/src/lib.rs"), &targets, &tracked),
            Some(WatchSignal::WorkingTree)
        );
        // Unrelated siblings in the same watched directory must not trigger reloads.
        assert_eq!(
            classify_watch_path(
                Path::new("/workspace/repo/src/unrelated.rs"),
                &targets,
                &tracked
            ),
            None
        );
        assert_eq!(
            classify_watch_path(
                Path::new("/workspace/repo/.jj/repo/op_heads/heads/abc"),
                &targets,
                &tracked
            ),
            Some(WatchSignal::JjOp)
        );
        assert_eq!(
            classify_watch_path(
                Path::new("/workspace/repo/.tellit/chg/commit/thread.json"),
                &targets,
                &tracked
            ),
            Some(WatchSignal::CommentsChanged)
        );
        assert_eq!(
            classify_watch_path(Path::new("/workspace/repo/.git/index"), &targets, &tracked),
            None
        );
        assert_eq!(
            classify_watch_path(
                Path::new("/workspace/repo/.jj/working_copy/tree_state"),
                &targets,
                &tracked
            ),
            None
        );
        assert_eq!(
            classify_watch_path(
                Path::new("/workspace/repo/target/debug/tellit"),
                &targets,
                &tracked
            ),
            None
        );
    }

    #[test]
    fn watcher_state_tracks_only_diffed_parents_and_preserves_overlapping_store_parent() {
        let dir = tempfile::tempdir().unwrap();
        let root = canonicalize_best_effort(dir.path());
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("crates/sub")).unwrap();
        std::fs::create_dir_all(root.join("target/debug")).unwrap();
        std::fs::write(root.join("Cargo.toml"), "[package]\n").unwrap();
        std::fs::write(root.join("src/lib.rs"), "pub fn a() {}\n").unwrap();
        std::fs::write(root.join("crates/sub/mod.rs"), "pub fn b() {}\n").unwrap();

        let store_dir = root.join(".tellit");
        let targets = WatchTargets {
            workspace_root: root.clone(),
            op_heads_dir: None,
            store_dir: store_dir.clone(),
        };
        let (sig_tx, _sig_rx) = mpsc::unbounded_channel();
        let mut state = WatcherState::new(targets, sig_tx, true).expect("watcher must initialize");

        // Before `.tellit` exists, only its parent (`root`) is watched non-recursively.
        assert!(!state.store_watch_attached);
        assert_eq!(
            state.active_non_recursive_dirs,
            HashSet::from([root.clone()])
        );

        // Watching diffed files adds only their immediate parent dirs (never `target/debug`).
        state.set_working_copy_files(&[
            PathBuf::from("Cargo.toml"),
            PathBuf::from("src/lib.rs"),
            PathBuf::from("crates/sub/mod.rs"),
        ]);
        assert_eq!(
            state.active_non_recursive_dirs,
            HashSet::from([root.clone(), root.join("src"), root.join("crates/sub")])
        );

        // Creating `.tellit` attaches the recursive store watch without unwatching `root`,
        // because `Cargo.toml` is still tracked inside `root`.
        std::fs::create_dir_all(&store_dir).unwrap();
        state.try_attach_store_watch();
        assert!(state.store_watch_attached);
        assert!(state.store_parent_watch.is_none());
        assert!(state.active_non_recursive_dirs.contains(&root));

        // Shrinking the diffed set unwatches `root` and `crates/sub` cleanly.
        state.set_working_copy_files(&[PathBuf::from("src/lib.rs")]);
        assert_eq!(
            state.active_non_recursive_dirs,
            HashSet::from([root.join("src")])
        );
    }

    #[tokio::test]
    async fn pending_signals_merge_comments_and_jj_op_over_working_tree() {
        let registry = ClientRegistry::default();
        let mut rx = registry.subscribe();

        let mut pending = PendingSignals::default();
        pending.record(WatchSignal::WorkingTree);
        pending.record(WatchSignal::JjOp);
        pending.record(WatchSignal::CommentsChanged);
        pending.flush(&registry);

        assert_eq!(rx.recv().await, Some(WatchEvent::CommentsChanged));
        assert_eq!(
            rx.recv().await,
            Some(WatchEvent::Reload {
                source: WatchSource::JjOp
            })
        );
        assert!(rx.try_recv().is_err());
    }
}
