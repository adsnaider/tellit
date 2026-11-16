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
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use axum::response::sse::Event;
use notify::event::ModifyKind;
use notify::{Event as NotifyEvent, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tellit_types::{WatchEvent, WatchSource};
use tokio::sync::{mpsc, watch};
use tokio_stream::Stream;

use super::jujutsu::resolve_jj_repo_paths;

const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(250);
const CLIENT_QUEUE_CAPACITY: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchSignal {
    WorkingTree,
    JjOp,
    CommentsChanged,
}

#[derive(Debug, Clone)]
struct WatchTargets {
    repo_root: PathBuf,
    op_heads_dir: Option<PathBuf>,
    store_dir: PathBuf,
}

impl WatchTargets {
    fn resolve(repo_path: &Path, store_dir: &Path) -> Self {
        let repo_root = canonicalize_best_effort(repo_path);
        let jj_paths = resolve_jj_repo_paths(&repo_root);
        let op_heads_dir = jj_paths
            .as_ref()
            .map(|p| canonicalize_best_effort(&p.jj_repo_dir.join("op_heads")));
        let resolved_store = canonicalize_future_path(store_dir);

        Self {
            repo_root,
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

/// Manages `notify` filesystem watches and broadcasts [`WatchEvent`] updates to `/api/watch` clients.
pub struct WatchHub {
    registry: Arc<ClientRegistry>,
    shutdown_tx: watch::Sender<bool>,
    _watcher: Option<Arc<Mutex<RecommendedWatcher>>>,
}

impl WatchHub {
    /// Starts the debounced `notify` watcher for `repo_path`, `.jj/repo/op_heads`, and `store_dir`.
    ///
    /// If initializing the OS watcher fails (for example due to inotify limits), returns a hub
    /// that still delivers API-initiated [`WatchEvent::CommentsChanged`] notifications.
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
        let targets = WatchTargets::resolve(&repo_path, &store_dir);
        let (sig_tx, sig_rx) = mpsc::unbounded_channel();

        let watcher = create_watcher(&targets, sig_tx, quiet);

        let loop_registry = Arc::clone(&registry);
        let loop_watcher = watcher.clone();
        let loop_store_dir = targets.store_dir.clone();
        let mut store_watch_attached =
            targets.store_dir.starts_with(&targets.repo_root) || targets.store_dir.exists();
        let mut task_shutdown = shutdown_tx.subscribe();

        tokio::spawn(async move {
            run_debounce_loop(sig_rx, &loop_registry, &mut task_shutdown, debounce, || {
                if !store_watch_attached
                    && loop_store_dir.exists()
                    && let Some(ref w) = loop_watcher
                    && let Ok(mut guard) = w.lock()
                    && guard
                        .watch(&loop_store_dir, RecursiveMode::Recursive)
                        .is_ok()
                {
                    store_watch_attached = true;
                }
            })
            .await;
        });

        Arc::new(Self {
            registry,
            shutdown_tx,
            _watcher: watcher,
        })
    }

    /// Broadcasts a [`WatchEvent::CommentsChanged`] event immediately to all connected clients.
    pub fn broadcast_comments_changed(&self) {
        self.registry.broadcast(WatchEvent::CommentsChanged);
    }

    /// Opens a new SSE stream starting with [`WatchEvent::Connected`].
    #[must_use]
    pub fn subscribe(&self) -> WatchStream {
        WatchStream::new(self.registry.subscribe(), self.shutdown_tx.subscribe())
    }
}

fn create_watcher(
    targets: &WatchTargets,
    sig_tx: mpsc::UnboundedSender<WatchSignal>,
    quiet: bool,
) -> Option<Arc<Mutex<RecommendedWatcher>>> {
    let classify_targets = targets.clone();
    let mut watcher = match notify::recommended_watcher(move |res: notify::Result<NotifyEvent>| {
        let Ok(event) = res else {
            return;
        };
        if !is_relevant_event_kind(&event.kind) {
            return;
        }
        for path in &event.paths {
            if let Some(signal) = classify_watch_path(path, &classify_targets) {
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

    if targets.repo_root.exists()
        && let Err(err) = watcher.watch(&targets.repo_root, RecursiveMode::Recursive)
        && !quiet
    {
        eprintln!(
            "Warning: failed to watch workspace {}: {err}",
            targets.repo_root.display()
        );
    }

    if let Some(ref op_heads) = targets.op_heads_dir
        && !op_heads.starts_with(&targets.repo_root)
        && op_heads.exists()
        && let Err(err) = watcher.watch(op_heads, RecursiveMode::Recursive)
        && !quiet
    {
        eprintln!(
            "Warning: failed to watch jj op_heads {}: {err}",
            op_heads.display()
        );
    }

    if !targets.store_dir.starts_with(&targets.repo_root) {
        if targets.store_dir.exists() {
            let _ = watcher.watch(&targets.store_dir, RecursiveMode::Recursive);
        } else if let Some(parent) = targets.store_dir.parent()
            && parent.exists()
        {
            let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
        }
    }

    Some(Arc::new(Mutex::new(watcher)))
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

async fn run_debounce_loop(
    mut sig_rx: mpsc::UnboundedReceiver<WatchSignal>,
    registry: &ClientRegistry,
    shutdown_rx: &mut watch::Receiver<bool>,
    debounce: Duration,
    mut on_signal: impl FnMut(),
) {
    loop {
        let first = tokio::select! {
            () = wait_for_shutdown(shutdown_rx) => return,
            maybe_sig = sig_rx.recv() => match maybe_sig {
                Some(sig) => sig,
                None => return,
            }
        };

        on_signal();
        let mut pending = PendingSignals::default();
        pending.record(first);

        let timer = tokio::time::sleep(debounce);
        tokio::pin!(timer);

        loop {
            tokio::select! {
                () = wait_for_shutdown(shutdown_rx) => return,
                () = &mut timer => {
                    pending.flush(registry);
                    break;
                }
                maybe_sig = sig_rx.recv() => match maybe_sig {
                    Some(sig) => {
                        on_signal();
                        pending.record(sig);
                    }
                    None => {
                        pending.flush(registry);
                        return;
                    }
                }
            }
        }
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

fn classify_watch_path(path: &Path, targets: &WatchTargets) -> Option<WatchSignal> {
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

    if let Ok(rel) = path.strip_prefix(&targets.repo_root)
        && rel.components().any(|comp| {
            matches!(
                comp.as_os_str().to_str(),
                Some(".git" | ".jj" | "target" | "node_modules")
            )
        })
    {
        return None;
    }

    if path.starts_with(&targets.repo_root) {
        return Some(WatchSignal::WorkingTree);
    }

    None
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
    fn classifies_working_tree_op_heads_store_and_ignored_paths() {
        let targets = WatchTargets {
            repo_root: PathBuf::from("/workspace/repo"),
            op_heads_dir: Some(PathBuf::from("/workspace/repo/.jj/repo/op_heads")),
            store_dir: PathBuf::from("/workspace/repo/.tellit"),
        };

        assert_eq!(
            classify_watch_path(Path::new("/workspace/repo/src/lib.rs"), &targets),
            Some(WatchSignal::WorkingTree)
        );
        assert_eq!(
            classify_watch_path(
                Path::new("/workspace/repo/.jj/repo/op_heads/heads/abc"),
                &targets
            ),
            Some(WatchSignal::JjOp)
        );
        assert_eq!(
            classify_watch_path(
                Path::new("/workspace/repo/.tellit/chg/commit/thread.json"),
                &targets
            ),
            Some(WatchSignal::CommentsChanged)
        );
        assert_eq!(
            classify_watch_path(Path::new("/workspace/repo/.git/index"), &targets),
            None
        );
        assert_eq!(
            classify_watch_path(
                Path::new("/workspace/repo/.jj/working_copy/tree_state"),
                &targets
            ),
            None
        );
        assert_eq!(
            classify_watch_path(Path::new("/workspace/repo/target/debug/tellit"), &targets),
            None
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
