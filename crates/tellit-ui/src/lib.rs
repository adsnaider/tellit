//! Leptos client-side WebAssembly frontend for `tellit`.
//!
//! Renders the browser review shell, queries `/api/jj/status` to detect the active Jujutsu
//! workspace, and maintains a live `/api/heartbeat` Server-Sent Events connection so the local
//! server knows while a review tab remains open.
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

use alloc::string::{String, ToString};

use leptos::prelude::*;
use leptos::task::spawn_local;
use tellit_types::{HeartbeatEvent, JjStatusResponse};

/// Browser-to-server `/api/heartbeat` SSE connection state.
///
/// # Examples
///
/// ```
/// use tellit_types::HeartbeatEvent;
/// use tellit_ui::ConnectionState;
///
/// let state = ConnectionState::Connecting.on_heartbeat(HeartbeatEvent::Ping);
/// assert_eq!(state, ConnectionState::Live);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Initial state while opening `/api/heartbeat`.
    Connecting,
    /// Connected to `/api/heartbeat` and receiving server events.
    Live,
    /// Heartbeat stream closed or encountered a transport error.
    Offline,
}

impl ConnectionState {
    /// Transitions connection state upon receiving a [`HeartbeatEvent`].
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_types::HeartbeatEvent;
    /// use tellit_ui::ConnectionState;
    ///
    /// assert_eq!(
    ///     ConnectionState::Connecting.on_heartbeat(HeartbeatEvent::Connected),
    ///     ConnectionState::Live
    /// );
    /// ```
    #[must_use]
    pub const fn on_heartbeat(self, _event: HeartbeatEvent) -> Self {
        Self::Live
    }

    /// Short human-readable status label for the top bar indicator.
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_ui::ConnectionState;
    ///
    /// assert_eq!(ConnectionState::Connecting.label(), "Connecting");
    /// assert_eq!(ConnectionState::Live.label(), "Live");
    /// assert_eq!(ConnectionState::Offline.label(), "Offline");
    /// ```
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Connecting => "Connecting",
            Self::Live => "Live",
            Self::Offline => "Offline",
        }
    }

    /// CSS class modifier applied to the connection pill in the header.
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_ui::ConnectionState;
    ///
    /// assert_eq!(ConnectionState::Live.badge_class(), "badge badge--live");
    /// ```
    #[must_use]
    pub const fn badge_class(self) -> &'static str {
        match self {
            Self::Connecting => "badge badge--connecting",
            Self::Live => "badge badge--live",
            Self::Offline => "badge badge--offline",
        }
    }
}

/// View state for the initial `GET /api/jj/status` probe.
///
/// # Examples
///
/// ```
/// use tellit_types::JjStatusResponse;
/// use tellit_ui::RepoStatusState;
///
/// let state = RepoStatusState::Loaded(JjStatusResponse::not_jj());
/// assert_eq!(state.badge_label(), "non-jj directory");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoStatusState {
    /// Request to `/api/jj/status` is in flight.
    Loading,
    /// Server returned a [`JjStatusResponse`].
    Loaded(JjStatusResponse),
    /// Request to `/api/jj/status` failed with an error message.
    Error(String),
}

impl RepoStatusState {
    /// Summary label shown in the top bar repository badge.
    ///
    /// # Examples
    ///
    /// ```
    /// use tellit_types::JjStatusResponse;
    /// use tellit_ui::RepoStatusState;
    ///
    /// assert_eq!(RepoStatusState::Loading.badge_label(), "Checking workspace...");
    /// assert_eq!(
    ///     RepoStatusState::Loaded(JjStatusResponse::jj("/repo")).badge_label(),
    ///     "jj workspace"
    /// );
    /// ```
    #[must_use]
    pub fn badge_label(&self) -> &'static str {
        match self {
            Self::Loading => "Checking workspace...",
            Self::Loaded(status) if status.is_jj => "jj workspace",
            Self::Loaded(_) => "non-jj directory",
            Self::Error(_) => "Status unavailable",
        }
    }
}

fn load_jj_status(repo_status: RwSignal<RepoStatusState>) {
    repo_status.set(RepoStatusState::Loading);
    spawn_local(async move {
        let next = match api::fetch_jj_status().await {
            Ok(status) => RepoStatusState::Loaded(status),
            Err(err) => RepoStatusState::Error(err.to_string()),
        };
        repo_status.set(next);
    });
}

fn connect_heartbeat(
    connection: RwSignal<ConnectionState>,
    heartbeat_handle: StoredValue<Option<api::HeartbeatSubscription>, LocalStorage>,
) {
    heartbeat_handle.set_value(None);
    connection.set(ConnectionState::Connecting);
    match api::subscribe_heartbeat(
        move |event| connection.update(|state| *state = state.on_heartbeat(event)),
        move || connection.set(ConnectionState::Offline),
    ) {
        Ok(subscription) => heartbeat_handle.set_value(Some(subscription)),
        Err(_) => connection.set(ConnectionState::Offline),
    }
}

/// Root Leptos component for the `tellit` code review application shell.
#[component]
pub fn App() -> impl IntoView {
    let repo_status = RwSignal::new(RepoStatusState::Loading);
    let connection = RwSignal::new(ConnectionState::Connecting);
    let heartbeat_handle = StoredValue::new_local(None::<api::HeartbeatSubscription>);

    Effect::new(move |_| {
        load_jj_status(repo_status);
        connect_heartbeat(connection, heartbeat_handle);
    });

    on_cleanup(move || {
        heartbeat_handle.set_value(None);
    });

    view! {
        <div class="app-shell">
            <header class="topbar">
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

                <div class="topbar-meta">
                    <span class="badge">
                        {move || repo_status.get().badge_label()}
                    </span>
                    <span class=move || connection.get().badge_class()>
                        <span class="badge-dot" aria-hidden="true"></span>
                        <span>{move || connection.get().label()}</span>
                    </span>
                </div>
            </header>

            <main class="workspace-main">
                {move || match repo_status.get() {
                    RepoStatusState::Loading => view! {
                        <section class="status-card" aria-busy="true">
                            <h1 class="status-card__title">"Connecting to workspace"</h1>
                            <p class="status-card__body">
                                "Checking Jujutsu repository status..."
                            </p>
                        </section>
                    }.into_any(),
                    RepoStatusState::Loaded(JjStatusResponse { is_jj: true, repo_root }) => view! {
                        <section class="status-card">
                            <h1 class="status-card__title">"Jujutsu workspace ready"</h1>
                            <p class="status-card__body">
                                "Connected to the local tellit review server."
                            </p>
                            {repo_root.map(|root| view! {
                                <div class="repo-root" data-testid="repo-root">{root}</div>
                            })}
                        </section>
                    }.into_any(),
                    RepoStatusState::Loaded(JjStatusResponse { is_jj: false, .. }) => view! {
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
                                    load_jj_status(repo_status);
                                    if connection.get_untracked() == ConnectionState::Offline {
                                        connect_heartbeat(connection, heartbeat_handle);
                                    }
                                }
                            >
                                "Retry"
                            </button>
                        </section>
                    }.into_any(),
                }}
            </main>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
