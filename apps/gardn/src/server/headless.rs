//! Headless server mode — runs the Gardn event loop without a real terminal.
//!
//! The server:
//! - Does not enter raw mode or read stdin
//! - Creates and listens on both `gardn.sock` (existing JSON API) and
//!   `gardn-client.sock` (new binary protocol)
//! - Initializes AppState and all PTYs from session restore or fresh state
//! - Runs the main event loop (drain events, drain API requests, scheduled tasks)
//! - Renders to a virtual ratatui Buffer in memory
//! - Accepts client connections on the client socket
//! - Streams frames to connected clients after each render
//! - Routes client input events through the existing input pipeline
//! - Continues running after client disconnect
//! - Handles stale socket cleanup, explicit server stop, minimum terminal size,
//!   and pane spawn failure during restore

use crate::ipc::{bind_local_listener, LocalListener};
use interprocess::local_socket::{traits::Listener as _, ListenerNonblockingMode};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ratatui::layout::Rect;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use base64::Engine;
use bytes::Bytes;

use crate::api;
use crate::app;
use crate::config;
use crate::events::AppEvent;
use crate::ipc::{remove_socket_file_if_owned, socket_file_identity, SocketFileIdentity};
use crate::protocol::{self, FrameData, ServerMessage, MAX_FRAME_SIZE, MAX_GRAPHICS_FRAME_SIZE};
use crate::server::client_accept::{
    accept_pending_client_connections, reject_pending_client_connections,
};
use crate::server::client_transport::ServerEvent;
use crate::server::clients::{
    events_include_interaction, latest_app_client, render_targets, terminal_attach_client_ids,
    ClientConnection, ClientConnectionMode, StagedClipboardFile,
};
use crate::server::keybindings::{app_keybindings, apply_keybindings};
use crate::server::notifications::{
    should_forward_toast_to_clients, toast_message_from_state_change, toast_notify_kind,
};
use crate::server::socket_paths::{
    client_socket_path, prepare_socket_path, restrict_socket_permissions,
};
use crate::server::tab_control::{
    TabControlCoordinator, TabControlError, TabControlKey, TabControlStatus,
};
use crate::server::terminal_attach::paste_payload_for_runtime;

#[cfg(test)]
use crate::server::client_transport::ClientWriter;
#[cfg(test)]
use std::fs;

const LIVE_HANDOFF_RESPONSE_TIMEOUT: Duration = Duration::from_secs(5);

fn sanitize_notification_text(value: &str, max_chars: usize) -> Option<String> {
    let mut out = String::new();
    let mut pending_space = false;

    for ch in value.chars() {
        if ch.is_control() || ch.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space && out.chars().count() < max_chars {
            out.push(' ');
        }
        pending_space = false;
        if out.chars().count() >= max_chars {
            break;
        }
        out.push(ch);
    }

    (!out.is_empty()).then_some(out)
}

fn notification_message(title: &str, body: Option<&str>) -> String {
    match body {
        Some(body) if !body.is_empty() => format!("{title}: {body}"),
        _ => title.to_owned(),
    }
}

fn sound_notify_message(sound: crate::sound::Sound) -> &'static str {
    match sound {
        crate::sound::Sound::Done => "agent done",
        crate::sound::Sound::Request => "agent attention",
    }
}

fn non_empty_body(body: &str) -> Option<String> {
    (!body.is_empty()).then(|| body.to_owned())
}

fn notification_show_response_shown(response: &str) -> bool {
    let Ok(response) = serde_json::from_str::<api::schema::SuccessResponse>(response) else {
        return false;
    };
    matches!(
        response.result,
        api::schema::ResponseResult::NotificationShow {
            shown: true,
            reason: api::schema::NotificationShowReason::Shown,
        }
    )
}

// ---------------------------------------------------------------------------
// Loop event enum for the headless server event loop
// ---------------------------------------------------------------------------

/// Events that the headless server event loop can process.
enum LoopEvent {
    Timer,
    Internal(Box<AppEvent>),
    Api(Box<api::ApiRequestMessage>),
    ServerEvent(ServerEvent),
    RenderRequested,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default shared runtime size (columns, rows) when no clients are attached.
#[cfg(test)]
const MIN_COLS: u16 = 80;
#[cfg(test)]
const MIN_ROWS: u16 = 24;

/// Timeout for in-flight API requests during shutdown.
#[allow(dead_code)]
const SHUTDOWN_API_TIMEOUT: Duration = Duration::from_secs(5);

/// How often the idle headless loop wakes to poll the std UnixListener for new
/// client connections.
///
/// The listener is non-blocking and not integrated into `tokio::select!`, so
/// a low-frequency wake is required to notice new thin-client attaches while
/// otherwise idle. Keep this much slower than the old resize-poll cadence to
/// avoid reintroducing the idle CPU spin.
const CLIENT_ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(250);

struct PendingClipboardImageStage {
    client_id: u64,
    terminal_id: crate::terminal::TerminalId,
    location: crate::execution_host::ResourceLocation,
}

struct AltScreenReadSpec {
    terminal_id: crate::terminal::TerminalId,
    lines: usize,
    unwrap: bool,
    initial: crate::terminal::ScreenSnapshot,
    content_seq: u64,
}

enum AltScreenReadConflict {
    None,
    Frozen(crate::pane::TerminalReadSnapshot),
    Defer,
}

// ---------------------------------------------------------------------------
// Headless server
// ---------------------------------------------------------------------------

/// The headless server — runs the Gardn event loop without a real terminal.
pub struct HeadlessServer {
    app: app::App,
    api_tx: Option<api::ApiRequestSender>,
    api_server: Option<api::ServerHandle>,
    client_listener: LocalListener,
    client_socket_path: PathBuf,
    client_socket_identity: SocketFileIdentity,
    clients: HashMap<u64, ClientConnection>,
    next_client_id: u64,
    pending_clipboard_image_stages: HashMap<
        (
            crate::execution_host::ExecutionHostId,
            crate::execution_host::protocol::RequestId,
        ),
        PendingClipboardImageStage,
    >,
    /// Most recently interactive full app client, used for shared host context only.
    foreground_client_id: Option<u64>,
    /// Outer window title last pushed, paired with the client that received it.
    /// Keying on the client means a newly attached terminal is written to even
    /// when the title itself has not changed, without every code path that
    /// changes the foreground client having to remember to invalidate this.
    sent_window_title: Option<(u64, Option<String>)>,
    /// Window title set through `client.window_title.set`. While present it wins
    /// over the configured `ui.window_title` until the API clears it again.
    api_window_title: Option<String>,
    /// Server-owned keybindings, restored when foreground clients use server mode.
    server_keybindings: crate::config::LiveKeybindConfig,
    /// Full server config warning shown to clients that use server keybindings.
    server_config_diagnostic: Option<String>,
    /// Server config warning with keybinding diagnostics removed for local-keybinding clients.
    server_config_diagnostic_without_keybindings: Option<String>,
    /// Writable direct attach owner per terminal id string.
    terminal_attach_owners: HashMap<String, u64>,
    /// Deferred application-history reads currently driving alternate-screen viewports.
    pending_alt_screen_reads: Vec<crate::server::alt_screen_read::PendingAltScreenRead>,
    /// Reads waiting for an alternate-screen traversal of the same terminal to finish.
    deferred_alt_screen_reads: Vec<api::ApiRequestMessage>,
    /// Exclusive normal-client controller for each live tab.
    tab_controls: TabControlCoordinator,
    /// Last stable tab observed by each normal client, used to avoid implicit promotion in place.
    client_tab_keys: HashMap<u64, TabControlKey>,
    /// Monotonic activity counter used to pick the most recently active client.
    next_activity_stamp: u64,
    /// Legacy default canvas size for server-owned views with no attached controller.
    effective_size: (u16, u16),
    /// Flag set when shutdown is initiated.
    shutting_down: bool,
    /// Flag set while exporting live PTYs to a replacement server.
    handoff_in_progress: bool,
    /// Imported panes get one app-safe resize nudge after the first client attaches.
    pending_handoff_repaint_nudge: bool,
    /// Flag set by Ctrl+C or `server stop` signal.
    should_quit: Arc<AtomicBool>,
    /// Channel for receiving server events from client connection threads.
    server_event_rx: mpsc::Receiver<ServerEvent>,
    /// Sender for server events (cloned for each client thread).
    server_event_tx: mpsc::Sender<ServerEvent>,
}

impl HeadlessServer {
    /// Creates and starts the headless server.
    ///
    /// This:
    /// 1. Prepares the client socket path (cleans up stale sockets)
    /// 2. Binds the client socket listener
    /// 3. Returns the server ready to run
    pub fn new(
        app: app::App,
        config_diagnostics: &[String],
        api_tx: Option<api::ApiRequestSender>,
        api_server: Option<api::ServerHandle>,
        should_quit: Arc<AtomicBool>,
    ) -> io::Result<Self> {
        let client_path = client_socket_path();
        prepare_socket_path(&client_path)?;

        let listener = bind_local_listener(&client_path)?;
        restrict_socket_permissions(&client_path)?;
        let client_socket_identity = socket_file_identity(&client_path)?;
        info!(path = %client_path.display(), "client protocol socket listening");

        listener.set_nonblocking(ListenerNonblockingMode::Accept)?;

        // Channel for server events from client threads.
        let (server_event_tx, server_event_rx) = mpsc::channel(64);
        let server_keybindings = app_keybindings(&app);
        let (server_config_diagnostic, server_config_diagnostic_without_keybindings) =
            server_config_diagnostic_summaries(config_diagnostics);
        let effective_size = app.state.headless_size;

        Ok(Self {
            app,
            api_tx,
            api_server,
            client_listener: listener,
            client_socket_path: client_path,
            client_socket_identity,
            clients: HashMap::new(),
            next_client_id: 1,
            pending_clipboard_image_stages: HashMap::new(),
            foreground_client_id: None,
            sent_window_title: None,
            api_window_title: None,
            server_keybindings,
            server_config_diagnostic,
            server_config_diagnostic_without_keybindings,
            terminal_attach_owners: HashMap::new(),
            pending_alt_screen_reads: Vec::new(),
            deferred_alt_screen_reads: Vec::new(),
            tab_controls: TabControlCoordinator::new(),
            client_tab_keys: HashMap::new(),
            next_activity_stamp: 1,
            effective_size,

            shutting_down: false,
            handoff_in_progress: false,
            pending_handoff_repaint_nudge: false,
            should_quit,
            server_event_rx,
            server_event_tx,
        })
    }

    /// Runs the headless server event loop until shutdown.
    ///
    /// This is the server's main loop — analogous to `App::run()` but without
    /// a real terminal. It:
    /// - Drains internal events (pane death, state changes)
    /// - Drains API requests (from the JSON socket)
    /// - Accepts new client connections
    /// - Reads client messages and routes input
    /// - Handles scheduled tasks (resize poll, animation, session save, etc.)
    /// - Renders virtually and streams frames to clients
    pub async fn run(&mut self) -> io::Result<()> {
        crate::logging::startup("server");
        self.app.resume_pending_connection_retirement()?;

        // Register SIGINT handler for graceful shutdown.
        let should_quit = self.should_quit.clone();
        let quit_notify = self.server_event_tx.clone();
        ctrlc_handler(should_quit, quit_notify);

        // No input_rx needed — server doesn't read stdin.
        // We use None for input_rx so the event loop doesn't try to read from stdin.
        self.app.input_rx = None;

        let mut needs_render = true;

        loop {
            let loop_started = Instant::now();
            let drain_started = Instant::now();
            self.app.reap_finished_custom_commands();
            // If shutdown has been initiated, complete it and exit.
            if self.shutting_down {
                self.complete_shutdown()?;
                break;
            }

            // Check if we should start shutting down.
            if self.app.state.should_quit || self.should_quit.load(Ordering::Acquire) {
                self.initiate_shutdown();
                continue;
            }

            // 1. Check render_dirty flag from PTY reader tasks.
            if self.app.render_dirty.is_pending() {
                needs_render = true;
            }

            // 2. Drain internal events.
            if self.drain_internal_events_with_forwarding() {
                needs_render = true;
            }

            // 3. Drain API requests.
            if self.drain_api_requests_with_shutdown_check() {
                needs_render = true;
            }

            self.poll_pending_alt_screen_reads(Instant::now());
            if self.process_deferred_alt_screen_reads() {
                needs_render = true;
            }

            self.app.sync_focus_events();
            self.app.sync_session_save_schedule();

            // Outer window title. Every app-state input changes with the
            // drained events above; the focused pane's own terminal title
            // arrives through PTY parsing, so check its dirty flag too.
            if needs_render
                || (self.app.window_title_uses_terminal_title()
                    && self.app.take_focused_terminal_title_dirty())
            {
                self.sync_window_title();
            }

            // 4. Accept new client connections.
            self.accept_client_connections()?;

            if self.drain_server_events() {
                needs_render = true;
            }
            let drain = drain_started.elapsed();
            let schedule_started = Instant::now();

            // 6. Handle scheduled tasks.
            let now = Instant::now();
            if self.app.poll_execution_hosts(now) {
                needs_render = true;
            }
            // Deferred focus cleanup is client-scoped; apply exact markers to live views.
            let client_view_effects = self.app.take_client_view_effects();
            if !client_view_effects.is_empty() {
                for effect in &client_view_effects {
                    for client in self.clients.values_mut() {
                        if let Some(view) = client.view_state.as_mut() {
                            if view.apply_client_view_effect(effect) {
                                needs_render = true;
                            }
                        }
                    }
                }
            }
            if self.handle_scheduled_tasks_headless(now, needs_render) {
                needs_render = true;
            }

            // Handle deferred workspace requests through the same path as the
            // monolithic app loop so client/server mode cannot drift.
            if self.app.process_deferred_workspace_requests() {
                needs_render = true;
            }

            if self.app.state.request_reload_config {
                self.app.state.request_reload_config = false;
                self.reload_server_config(true);
                needs_render = true;
            }

            if self.handle_open_project_command_request() {
                needs_render = true;
            }
            needs_render |= self.app.poll_github();
            for client in self.clients.values_mut() {
                if let Some(view) = client.view_state.as_mut() {
                    if view.github.is_some()
                        || view.github_host.is_some()
                        || view.focused_tab_is_github(&self.app.state)
                    {
                        needs_render |= self.app.pump_github_for_view(view);
                    }
                }
            }

            self.drain_client_config_reload_request();
            if self.app.reconcile_terminal_themes() {
                needs_render = true;
            }
            self.stream_host_mouse_capture_mode();

            self.sync_animation_timer(now);
            let schedule = schedule_started.elapsed();

            // 7. Render virtually and stream frames.
            if needs_render && self.app.can_render_now(now) {
                let render_request = self.app.render_dirty.take();
                let pty_only = !render_request.generic && !render_request.pty_sources.is_empty();
                if pty_only
                    && !self.pty_sources_visible_to_any_render_target(&render_request.pty_sources)
                {
                    self.app.last_render_at = Some(now);
                    self.app.loop_stats.finish_frame(
                        drain,
                        schedule,
                        Duration::ZERO,
                        Duration::ZERO,
                        "skip",
                        loop_started.elapsed(),
                    );
                    needs_render = false;
                } else {
                    self.app.sync_pending_agent_resume_deadline(now);
                    let allow_pending_agent_resume_empty_theme =
                        self.app.pending_agent_resume_due(now);
                    let draw_started = Instant::now();
                    let pending_resume_started = self.render_and_stream_with_pending_agent_resume(
                        allow_pending_agent_resume_empty_theme,
                    );
                    let draw = draw_started.elapsed();
                    if pending_resume_started
                        || self
                            .app
                            .start_pending_agent_resumes(allow_pending_agent_resume_empty_theme)
                    {
                        self.app.render_dirty.request_generic();
                        self.app.render_notify.notify_one();
                    }
                    self.app.last_render_at = Some(now);
                    self.app.loop_stats.finish_frame(
                        drain,
                        schedule,
                        draw,
                        Duration::ZERO,
                        "draw",
                        loop_started.elapsed(),
                    );
                    needs_render = false;
                }
            }

            // 8. Wait for next event.
            let client_selection_deadline = self
                .clients
                .values()
                .filter_map(|client| {
                    client
                        .view_state
                        .as_ref()
                        .and_then(|view| view.selection_highlight_clear_deadline)
                })
                .min();
            let next_deadline = [
                self.app.next_headless_loop_deadline_with_git_refresh(
                    now,
                    needs_render,
                    self.has_app_client(),
                ),
                client_selection_deadline,
                self.app
                    .github_has_pending()
                    .then_some(now + Duration::from_millis(50)),
            ]
            .into_iter()
            .flatten()
            .min()
            .map(|deadline| deadline.min(now + CLIENT_ACCEPT_POLL_INTERVAL))
            .or(Some(now + CLIENT_ACCEPT_POLL_INTERVAL));
            let next_deadline = self
                .pending_alt_screen_reads
                .iter()
                .map(|pending| pending.next_deadline())
                .fold(next_deadline, |deadline, pending| {
                    Some(deadline.map_or(pending, |current| current.min(pending)))
                });
            let event = {
                tokio::select! {
                    maybe_api = self.app.api_rx.recv() => match maybe_api {
                        Some(msg) => LoopEvent::Api(Box::new(msg)),
                        None => LoopEvent::Timer,
                    },
                    maybe_ev = self.app.event_rx.recv() => match maybe_ev {
                        Some(ev) => LoopEvent::Internal(Box::new(ev)),
                        None => LoopEvent::Timer,
                    },
                    maybe_server_ev = self.server_event_rx.recv() => match maybe_server_ev {
                        Some(ev) => LoopEvent::ServerEvent(ev),
                        None => LoopEvent::Timer,
                    },
                    _ = sleep_until_or_pending(next_deadline) => LoopEvent::Timer,
                    _ = self.app.render_notify.notified() => LoopEvent::RenderRequested,
                }
            };
            let input_started = Instant::now();
            let event_name = match &event {
                LoopEvent::Timer => "timer",
                LoopEvent::Internal(_) => "event",
                LoopEvent::Api(_) => "api",
                LoopEvent::ServerEvent(_) => "client",
                LoopEvent::RenderRequested => "notify",
            };
            match event {
                LoopEvent::Timer => {}
                LoopEvent::Internal(ev) => {
                    if self.handle_internal_event_with_forwarding(*ev) {
                        needs_render = true;
                    }
                }
                LoopEvent::Api(msg) => {
                    if self.handle_api_request_with_shutdown_check(*msg) {
                        needs_render = true;
                    }
                }
                LoopEvent::ServerEvent(ev) => {
                    if self.handle_server_event(ev) {
                        needs_render = true;
                    }
                }
                LoopEvent::RenderRequested => {
                    if self.app.render_dirty.is_pending() {
                        needs_render = true;
                    }
                }
            }
            self.app.loop_stats.finish_frame(
                drain,
                schedule,
                Duration::ZERO,
                input_started.elapsed(),
                event_name,
                loop_started.elapsed(),
            );
        }

        // Save session on exit.
        if !self.app.no_session {
            self.app.save_session_now();
        }

        info!("headless server exiting");
        Ok(())
    }

    fn allocate_activity_stamp(&mut self) -> u64 {
        let stamp = self.next_activity_stamp;
        self.next_activity_stamp = self.next_activity_stamp.saturating_add(1);
        stamp
    }

    fn tab_control_key_for_view(
        &self,
        view: &crate::app::ClientViewState,
    ) -> Option<TabControlKey> {
        let workspace_index = view.active_workspace?;
        let workspace = self.app.state.workspaces.get(workspace_index)?;
        let tab_index = view.active_tab_index_for_workspace(&self.app.state, workspace_index)?;
        let tab = workspace.tabs.get(tab_index)?;
        Some(TabControlKey::new(&workspace.id, tab.number))
    }
    fn tab_control_key_exists(&self, key: &TabControlKey) -> bool {
        self.app.state.workspaces.iter().any(|workspace| {
            workspace.id == key.workspace_id
                && workspace
                    .tabs
                    .iter()
                    .any(|tab| tab.number == key.tab_number)
        })
    }

    fn release_client_tab_control(&mut self, client_id: u64) -> bool {
        match self.tab_controls.release_client(client_id) {
            Ok(tab) => tab.is_some(),
            Err(err) => {
                warn!(client_id, %err, "failed to release tab control");
                false
            }
        }
    }

    fn prune_deleted_tab_controls(&mut self) {
        let known_keys = self.tab_controls.tab_keys().cloned().collect::<Vec<_>>();
        let stale_keys = known_keys
            .into_iter()
            .filter(|key| !self.tab_control_key_exists(key))
            .collect::<Vec<_>>();
        for key in stale_keys {
            self.tab_controls.remove_tab(&key);
        }
        let live_view_keys = self
            .client_tab_keys
            .values()
            .filter(|key| self.tab_control_key_exists(key))
            .cloned()
            .collect::<std::collections::HashSet<_>>();
        self.client_tab_keys
            .retain(|_, key| live_view_keys.contains(key));
    }

    fn tab_control_projection(
        status: TabControlStatus,
        client_id: u64,
    ) -> crate::app::ClientTabControl {
        if status.is_controlled_by(client_id) {
            crate::app::ClientTabControl::Controlling {
                epoch: status.epoch,
            }
        } else if status.is_free() {
            crate::app::ClientTabControl::WatchingFree {
                epoch: status.epoch,
            }
        } else {
            crate::app::ClientTabControl::WatchingControlled {
                epoch: status.epoch,
            }
        }
    }

    fn sync_client_tab_control_projection(&mut self, client_id: u64) {
        let key = self.clients.get(&client_id).and_then(|client| {
            client
                .view_state
                .as_ref()
                .and_then(|view| self.tab_control_key_for_view(view))
        });
        let projection = key
            .as_ref()
            .map(|key| Self::tab_control_projection(self.tab_controls.status(key), client_id))
            .unwrap_or(crate::app::ClientTabControl::Unavailable);
        let canvas_size = key
            .as_ref()
            .and_then(|key| self.tab_controls.canvas_size(key));
        if let Some(view) = self
            .clients
            .get_mut(&client_id)
            .and_then(|client| client.view_state.as_mut())
        {
            view.set_tab_control(projection);
            view.tab_canvas_size = canvas_size;
        }
    }

    fn sync_all_tab_control_projections(&mut self) {
        let client_ids = self.clients.keys().copied().collect::<Vec<_>>();
        for client_id in client_ids {
            self.sync_client_tab_control_projection(client_id);
        }
    }

    fn resize_all_controlled_tabs(&mut self) {
        let controller_ids = self
            .clients
            .keys()
            .copied()
            .filter(|client_id| {
                self.tab_controls
                    .controlled_tab_for_client(*client_id)
                    .is_some()
            })
            .collect::<Vec<_>>();
        for client_id in controller_ids {
            self.resize_controlled_tab_for_client(client_id);
        }
    }

    fn resize_controlled_tab_for_client(&mut self, client_id: u64) -> bool {
        let Some(mut view) = self
            .clients
            .get_mut(&client_id)
            .and_then(|client| client.view_state.take())
        else {
            return false;
        };
        let Some(controlled_tab) = self.tab_controls.controlled_tab_for_client(client_id) else {
            if let Some(client) = self.clients.get_mut(&client_id) {
                client.view_state = Some(view);
            }
            return false;
        };
        if self.tab_control_key_for_view(&view).as_ref() != Some(&controlled_tab) {
            if let Some(client) = self.clients.get_mut(&client_id) {
                client.view_state = Some(view);
            }
            return false;
        }
        let Some(client) = self.clients.get(&client_id) else {
            return false;
        };
        let (cols, rows) = client.terminal_size;
        let cell_size = client.cell_size;
        crate::ui::compute_view_for_client_with_cell_size(
            &self.app.state,
            &mut view,
            &self.app.terminal_runtimes,
            Rect::new(0, 0, cols, rows),
            cell_size,
        );
        let canvas_size = (
            view.computed.terminal_area.width,
            view.computed.terminal_area.height,
        );
        let canvas_changed =
            match self
                .tab_controls
                .set_canvas_size(client_id, &controlled_tab, canvas_size)
            {
                Ok(changed) => changed,
                Err(err) => {
                    warn!(client_id, %err, "failed to publish controlled tab canvas size");
                    false
                }
            };
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.view_state = Some(view);
        }
        if canvas_changed {
            self.sync_all_tab_control_projections();
        }
        for client in self.clients.values_mut() {
            client.request_full_redraw();
        }
        true
    }

    fn reconcile_client_tab_control(&mut self, client_id: u64) -> bool {
        let current_key = self.clients.get(&client_id).and_then(|client| {
            client
                .view_state
                .as_ref()
                .and_then(|view| self.tab_control_key_for_view(view))
        });
        let previous_observed_key = match current_key.as_ref() {
            Some(key) => self.client_tab_keys.insert(client_id, key.clone()),
            None => self.client_tab_keys.remove(&client_id),
        };
        let previous_key = self.tab_controls.controlled_tab_for_client(client_id);
        let mut changed = false;
        if previous_key.as_ref() != current_key.as_ref() {
            changed |= self.release_client_tab_control(client_id);
        }

        let request = self
            .clients
            .get_mut(&client_id)
            .and_then(|client| client.view_state.as_mut())
            .and_then(crate::app::ClientViewState::take_tab_control_request);
        if let (Some(key), Some(observed_epoch)) = (current_key.as_ref(), request) {
            match self.tab_controls.takeover(client_id, key, observed_epoch) {
                Ok(_) => changed = true,
                Err(TabControlError::StaleEpoch { .. } | TabControlError::Occupied { .. }) => {}
                Err(err) => warn!(client_id, %err, "failed to take control of tab"),
            }
        } else if let Some(key) = current_key.as_ref() {
            let status = self.tab_controls.status(key);
            let entered_tab = previous_observed_key.as_ref() != Some(key);
            if entered_tab
                && status.is_free()
                && self
                    .tab_controls
                    .controlled_tab_for_client(client_id)
                    .is_none()
            {
                match self.tab_controls.acquire_free(client_id, key) {
                    Ok(_) => changed = true,
                    Err(TabControlError::Occupied { .. }) => {}
                    Err(err) => warn!(client_id, %err, "failed to acquire free tab"),
                }
            }
        }

        self.sync_all_tab_control_projections();
        if changed {
            self.resize_controlled_tab_for_client(client_id);
        }
        changed
    }

    fn sync_foreground_client_state(&mut self) {
        let Some(client_id) = self.foreground_client_id else {
            self.app.state.outer_terminal_focus = None;
            self.effective_size = self.app.state.headless_size;
            let server_keybindings = self.server_keybindings.clone();
            apply_keybindings(&mut self.app, &server_keybindings);
            self.sync_visible_server_config_diagnostic(false);
            return;
        };
        let Some(client) = self.clients.get(&client_id) else {
            self.foreground_client_id = None;
            self.app.state.outer_terminal_focus = None;
            self.effective_size = self.app.state.headless_size;
            let server_keybindings = self.server_keybindings.clone();
            apply_keybindings(&mut self.app, &server_keybindings);
            self.sync_visible_server_config_diagnostic(false);
            return;
        };

        let terminal_size = client.terminal_size;
        let outer_terminal_focus = client.outer_terminal_focus;
        let host_terminal_theme = client.host_terminal_theme;
        let uses_local_keybindings = client.keybindings.is_some();
        let keybindings = client
            .keybindings
            .as_deref()
            .unwrap_or(&self.server_keybindings)
            .clone();
        let view_state = client.view_state.clone();

        self.effective_size = terminal_size;
        self.app.state.outer_terminal_focus = outer_terminal_focus;
        apply_keybindings(&mut self.app, &keybindings);
        self.sync_visible_server_config_diagnostic(uses_local_keybindings);
        if let Some(view_state) = view_state {
            self.app.default_client_view = view_state.clone_reconciled(&self.app.state);
        }
        if outer_terminal_focus != Some(false) {
            let view = self
                .app
                .default_client_view
                .clone_reconciled(&self.app.state);
            self.app.state.mark_active_tab_seen_for_view(&view);
        }
        if !host_terminal_theme.is_empty() {
            self.app.set_host_terminal_theme(host_terminal_theme);
        }
    }

    #[cfg(unix)]
    fn perform_live_handoff(
        &mut self,
        params: crate::api::schema::ServerLiveHandoffParams,
    ) -> io::Result<()> {
        info!("starting live handoff");
        let import_exe = params.import_exe.as_deref().map(std::path::PathBuf::from);
        let socket_path = crate::server::handoff::handoff_socket_path();
        let token = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let listener = match crate::server::handoff::bind_listener(&socket_path) {
            Ok(listener) => listener,
            Err(err) => {
                self.handoff_in_progress = false;
                return Err(err);
            }
        };

        let mut pane_by_terminal = HashMap::new();
        for ws in &self.app.state.workspaces {
            for tab in &ws.tabs {
                for (pane_id, pane) in &tab.panes {
                    pane_by_terminal.insert(pane.attached_terminal_id.clone(), pane_id.raw());
                }
            }
        }
        if pane_by_terminal.len() > crate::server::handoff::MAX_FDS_PER_HANDOFF {
            let _ = std::fs::remove_file(&socket_path);
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "live handoff supports at most {} panes in one update; close panes or restart gardn normally",
                    crate::server::handoff::MAX_FDS_PER_HANDOFF
                ),
            ));
        }

        self.handoff_in_progress = true;
        self.disconnect_all_clients_for_handoff();
        let _ = reject_pending_client_connections(&self.client_listener);

        let mut paused_terminal_ids = Vec::new();
        for terminal_id in pane_by_terminal.keys() {
            if let Some(runtime) = self.app.terminal_runtimes.get(terminal_id) {
                if let Err(err) = runtime.pause_handoff_reader(Duration::from_secs(2)) {
                    self.rollback_handoff_before_commit(&socket_path, &paused_terminal_ids);
                    return Err(err);
                }
                paused_terminal_ids.push(terminal_id.clone());
            }
        }

        let mut snapshot = crate::persist::capture_handoff(
            &self.app.state.groups,
            self.app.state.active_group,
            self.app.state.group_filter_enabled,
            &self.app.state.session_namespace_id,
            &self.app.state.remote_termination_tombstones,
            &self.app.state.workspaces,
            &self.app.state.terminals,
            &self.app.terminal_runtimes,
            self.app.state.active,
            self.app.state.selected,
            self.app.state.agent_panel_scope,
            self.app.state.sidebar_width,
            self.app.state.sidebar_collapsed,
            self.app.state.sidebar_section_split,
            self.app.state.right_sidebar_width,
            self.app.state.right_sidebar_collapsed,
            &self.app.state.agent_follow_up,
        );
        snapshot.ui = crate::persist::SessionUiSnapshot::from_app_state(&self.app.state);
        snapshot.default_view.ui = snapshot.ui.clone();
        snapshot.pane_id_aliases = self
            .app
            .state
            .pane_id_aliases
            .iter()
            .map(|(old_raw, current)| (*old_raw, current.raw()))
            .collect();

        let mut handoff_entries = Vec::new();
        for (terminal_id, runtime) in self.app.terminal_runtimes.iter() {
            let Some(pane_id) = pane_by_terminal.get(terminal_id).copied() else {
                continue;
            };
            let mut handoff_runtime = runtime.handoff_runtime_state(pane_id);
            let has_agent_session = self
                .app
                .state
                .terminals
                .get(terminal_id)
                .is_some_and(|terminal| terminal.persisted_agent_session.is_some());
            if !has_agent_session {
                handoff_runtime.initial_history_ansi = runtime.handoff_history_ansi();
            }
            handoff_entries.push((terminal_id.clone(), handoff_runtime));
        }

        let panes = handoff_entries
            .iter()
            .map(|(_, runtime)| runtime.clone())
            .collect();
        let manifest = crate::server::handoff::manifest_for(
            snapshot,
            panes,
            params.expected_protocol,
            params.expected_version,
            self.api_window_title.clone(),
        );
        let mut import_child = match crate::server::handoff::spawn_handoff_import(
            import_exe.as_deref(),
            &socket_path,
            &token,
        ) {
            Ok(child) => child,
            Err(err) => {
                self.rollback_handoff_before_commit(&socket_path, &paused_terminal_ids);
                return Err(err);
            }
        };
        let child_pid = import_child.id();
        info!(pid = child_pid, socket = %socket_path.display(), "spawned handoff import server");

        let mut fds = Vec::new();
        let duplicate_result = (|| {
            for (terminal_id, _) in &handoff_entries {
                let Some(runtime) = self.app.terminal_runtimes.get(terminal_id) else {
                    continue;
                };
                fds.push(runtime.duplicate_handoff_fd()?);
            }
            Ok::<(), io::Error>(())
        })();
        if let Err(err) = duplicate_result {
            for fd in fds {
                let _ = unsafe { libc::close(fd) };
            }
            crate::server::handoff::cleanup_failed_import_child(&mut import_child);
            self.rollback_handoff_before_commit(&socket_path, &paused_terminal_ids);
            return Err(err);
        }

        let mut stream = match crate::server::handoff::accept_and_validate_on(
            listener,
            &socket_path,
            &token,
            &manifest,
        ) {
            Ok(stream) => stream,
            Err(err) => {
                for fd in fds {
                    let _ = unsafe { libc::close(fd) };
                }
                crate::server::handoff::cleanup_failed_import_child(&mut import_child);
                self.rollback_handoff_before_commit(&socket_path, &paused_terminal_ids);
                return Err(err);
            }
        };

        let send_result = crate::server::handoff::send_fds_and_wait_restored(&mut stream, &fds);
        for fd in fds {
            let _ = unsafe { libc::close(fd) };
        }
        if let Err(err) = send_result {
            crate::server::handoff::cleanup_failed_import_child(&mut import_child);
            self.rollback_handoff_before_commit(&socket_path, &paused_terminal_ids);
            return Err(err);
        }

        if let Some(api_server) = &self.api_server {
            let _ = api_server.remove_socket_file_if_owned();
        } else {
            let _ = std::fs::remove_file(crate::api::socket_path());
        }
        let _ = remove_socket_file_if_owned(&self.client_socket_path, &self.client_socket_identity);
        if let Err(err) = crate::server::handoff::wait_ready(&mut stream) {
            crate::server::handoff::cleanup_failed_import_child(&mut import_child);
            match self.wait_then_restore_public_sockets_after_failed_handoff() {
                Ok(()) => {
                    self.rollback_handoff_before_commit(&socket_path, &paused_terminal_ids);
                }
                Err(restore_err) => {
                    self.rollback_handoff_before_commit(&socket_path, &paused_terminal_ids);
                    return Err(io::Error::other(format!(
                        "handoff replacement server did not become ready: {err}; old server could not restore public sockets: {restore_err}"
                    )));
                }
            }
            return Err(io::Error::other(format!(
                "handoff replacement server did not become ready: {err}"
            )));
        }
        if let Err(err) = crate::server::handoff::report_committed(&mut stream) {
            crate::server::handoff::cleanup_failed_import_child(&mut import_child);
            match self.wait_then_restore_public_sockets_after_failed_handoff() {
                Ok(()) => {
                    self.rollback_handoff_before_commit(&socket_path, &paused_terminal_ids);
                }
                Err(restore_err) => {
                    self.rollback_handoff_before_commit(&socket_path, &paused_terminal_ids);
                    return Err(io::Error::other(format!(
                        "handoff replacement server was ready, but commit failed: {err}; old server could not restore public sockets: {restore_err}"
                    )));
                }
            }
            return Err(err);
        }

        for (terminal_id, runtime) in self.app.terminal_runtimes.drain_for_handoff() {
            if !pane_by_terminal.contains_key(&terminal_id) {
                continue;
            }
            debug!(terminal = %terminal_id, "preserving pane runtime for handoff");
            runtime.preserve_for_handoff();
        }
        crate::server::handoff::wait_owned_ack(&mut stream);

        self.shutting_down = true;
        self.app.state.should_quit = true;
        self.app.no_session = true;
        info!("live handoff completed; old server exiting");
        Ok(())
    }

    #[cfg(not(unix))]
    fn perform_live_handoff(
        &mut self,
        _params: crate::api::schema::ServerLiveHandoffParams,
    ) -> io::Result<()> {
        Err(io::Error::other("live handoff is only supported on Unix"))
    }

    fn sync_visible_server_config_diagnostic(&mut self, uses_local_keybindings: bool) {
        let visible = if uses_local_keybindings {
            &self.server_config_diagnostic_without_keybindings
        } else {
            &self.server_config_diagnostic
        };
        let current = self.app.state.config_diagnostic.as_ref();
        if current == self.server_config_diagnostic.as_ref()
            || current == self.server_config_diagnostic_without_keybindings.as_ref()
        {
            self.app.state.config_diagnostic = visible.clone();
            self.app.state.config_issue = visible
                .clone()
                .map(crate::app::state::ConfigIssue::from_details);
        }
    }

    #[cfg(unix)]
    fn restore_public_sockets_after_failed_handoff(&mut self) -> io::Result<()> {
        let api_tx = self
            .api_tx
            .clone()
            .ok_or_else(|| io::Error::other("cannot restore api socket without api sender"))?;
        let api_server = api::start_server_with_stop_control(
            api_tx,
            self.app.event_hub.clone(),
            self.should_quit.clone(),
        )?;

        let client_path = client_socket_path();
        prepare_socket_path(&client_path)?;
        let listener = bind_local_listener(&client_path)?;
        restrict_socket_permissions(&client_path)?;
        let client_socket_identity = socket_file_identity(&client_path)?;
        listener.set_nonblocking(ListenerNonblockingMode::Accept)?;

        self.api_server = Some(api_server);
        self.client_listener = listener;
        self.client_socket_path = client_path;
        self.client_socket_identity = client_socket_identity;
        Ok(())
    }

    #[cfg(unix)]
    fn wait_then_restore_public_sockets_after_failed_handoff(&mut self) -> io::Result<()> {
        let timeout = crate::server::handoff::COMMIT_TIMEOUT + Duration::from_secs(2);
        wait_for_old_public_sockets_to_close(timeout)?;
        self.restore_public_sockets_after_failed_handoff()
    }

    #[cfg(unix)]
    fn rollback_handoff_before_commit(
        &mut self,
        socket_path: &Path,
        paused_terminal_ids: &[crate::terminal::TerminalId],
    ) {
        for terminal_id in paused_terminal_ids {
            if let Some(runtime) = self.app.terminal_runtimes.get(terminal_id) {
                runtime.set_handoff_reader_paused(false);
            }
        }
        self.handoff_in_progress = false;
        let _ = std::fs::remove_file(socket_path);
    }

    #[cfg(unix)]
    fn nudge_handoff_panes_on_first_client_attach(&mut self) {
        if !self.pending_handoff_repaint_nudge {
            return;
        }
        self.pending_handoff_repaint_nudge = false;
        self.app
            .terminal_runtimes
            .nudge_child_redraw_after_handoff();
    }

    #[cfg(not(unix))]
    fn nudge_handoff_panes_on_first_client_attach(&mut self) {}

    fn reload_server_config(&mut self, notify_success: bool) -> crate::config::ConfigReloadReport {
        let server_keybindings = self.server_keybindings.clone();
        apply_keybindings(&mut self.app, &server_keybindings);
        let report = self.app.apply_config_from_disk(notify_success);
        self.app.take_config_reloaded_from_disk();
        self.server_keybindings = app_keybindings(&self.app);
        let (server_config_diagnostic, server_config_diagnostic_without_keybindings) =
            server_config_diagnostic_summaries(&report.diagnostics);
        self.server_config_diagnostic = server_config_diagnostic;
        self.server_config_diagnostic_without_keybindings =
            server_config_diagnostic_without_keybindings;
        self.sync_foreground_client_state();
        report
    }

    fn foreground_client_outer_focus(&self) -> Option<bool> {
        let client_id = self.foreground_client_id?;
        self.clients.get(&client_id)?.outer_terminal_focus
    }

    fn active_tab_suppresses_notifications(&self, is_active_tab: bool) -> bool {
        crate::app::actions::active_tab_suppresses_notifications(
            is_active_tab,
            self.foreground_client_outer_focus(),
        )
    }

    fn client_view_contains_pane(
        &self,
        view: &crate::app::ClientViewState,
        pane_id: crate::layout::PaneId,
    ) -> bool {
        if self
            .app
            .state
            .popup_panes
            .values()
            .any(|popup| popup.pane_id == pane_id)
        {
            return true;
        }
        let Some(ws_idx) = view.active_workspace else {
            return false;
        };
        let Some(workspace) = self.app.state.workspaces.get(ws_idx) else {
            return false;
        };
        let Some(tab_idx) = view
            .active_tab_index_for_workspace(&self.app.state, ws_idx)
            .or_else(|| {
                workspace
                    .tabs
                    .get(workspace.active_tab_index())
                    .map(|_| workspace.active_tab_index())
            })
        else {
            return false;
        };
        let Some(tab) = workspace.tabs.get(tab_idx) else {
            return false;
        };
        if !tab.panes.contains_key(&pane_id) {
            return false;
        }
        if !tab.zoomed {
            return true;
        }
        view.focused_pane_for_tab(&workspace.id, tab.number)
            .unwrap_or_else(|| tab.layout.focused())
            == pane_id
    }

    fn any_app_client_view_contains_pane(&self, pane_id: crate::layout::PaneId) -> bool {
        self.clients.values().any(|client| {
            client.is_full_app_client()
                && client.writer.is_some()
                && client
                    .view_state
                    .as_ref()
                    .is_some_and(|view| self.client_view_contains_pane(view, pane_id))
        })
    }

    fn foreground_client_view_contains_pane(&self, pane_id: crate::layout::PaneId) -> bool {
        let Some(client_id) = self.foreground_client_id else {
            return false;
        };
        let Some(client) = self.clients.get(&client_id) else {
            return false;
        };
        client
            .view_state
            .as_ref()
            .is_some_and(|view| self.client_view_contains_pane(view, pane_id))
    }

    fn promote_client_to_foreground(&mut self, client_id: u64) -> bool {
        let stamp = self.allocate_activity_stamp();
        let Some(client) = self.clients.get_mut(&client_id) else {
            return false;
        };
        client.last_activity = stamp;

        let changed = self.foreground_client_id != Some(client_id);
        self.foreground_client_id = Some(client_id);
        self.sync_foreground_client_state();
        changed
    }

    fn promote_latest_remaining_client(&mut self) -> bool {
        let next_foreground = latest_app_client(&self.clients);
        let changed = next_foreground != self.foreground_client_id;
        self.foreground_client_id = next_foreground;
        self.sync_foreground_client_state();
        changed
    }

    fn app_client_count(&self) -> usize {
        self.clients
            .values()
            .filter(|client| client.is_full_app_client() && client.writer.is_some())
            .count()
    }

    fn has_app_client(&self) -> bool {
        self.app_client_count() > 0
    }

    fn remove_staged_clipboard_files(&mut self, files: Vec<StagedClipboardFile>) {
        let mut local_paths = Vec::new();
        for file in files {
            match file {
                StagedClipboardFile::Local(path) => local_paths.push(path),
                StagedClipboardFile::Remote(location) => {
                    if let Some(hosts) = self.app.execution_hosts.as_mut() {
                        if let Err(error) = hosts.remove_staged_file(location.clone()) {
                            debug!(path = %location.path, %error, "remote staged clipboard cleanup deferred to TTL");
                        }
                    }
                }
            }
        }
        crate::server::clipboard_image::remove_files(local_paths);
    }

    fn remove_client(&mut self, client_id: u64) -> bool {
        self.client_tab_keys.remove(&client_id);
        let was_foreground = self.foreground_client_id == Some(client_id);
        self.send_client_graphics_cleanup(client_id);
        let removed = self.clients.remove(&client_id);
        let mut removed_terminal_attach = false;
        self.pending_clipboard_image_stages
            .retain(|_, pending| pending.client_id != client_id);
        if let Some(mut removed) = removed {
            self.remove_staged_clipboard_files(removed.staged_clipboard_files);
            if let Some(view) = removed.view_state.as_mut() {
                self.app.release_github_for_view(view);
            }
            if let Some(view) = removed.view_state.as_ref() {
                let view_id = view.id();
                if let Some(hosts) = self.app.execution_hosts.as_ref() {
                    hosts.cancel_authentication_owner(
                        crate::execution_host::auth::AuthenticationOwner::new(view_id),
                    );
                }
                self.app
                    .state
                    .client_overlay_owners
                    .retain(|_, owner| *owner != view_id);
            }
            if let ClientConnectionMode::TerminalAttach { terminal_id } = removed.mode {
                removed_terminal_attach = true;
                self.terminal_attach_owners.remove(&terminal_id);
                if let Some(terminal_id) = self.terminal_id_by_string(&terminal_id) {
                    self.app
                        .state
                        .direct_attach_resize_locks
                        .remove(&terminal_id);
                }
            }
        }
        let released_control = self.release_client_tab_control(client_id);
        let foreground_changed = if was_foreground {
            self.promote_latest_remaining_client()
        } else {
            false
        };
        if released_control {
            self.sync_all_tab_control_projections();
        }
        if removed_terminal_attach {
            self.resize_all_controlled_tabs();
        }
        foreground_changed
    }

    fn send_client_graphics_cleanup(&mut self, client_id: u64) {
        let (writer, bytes) = match self.clients.get_mut(&client_id) {
            Some(client) => {
                let bytes = client.graphics_cache.clear_bytes();
                (client.writer.as_ref().cloned(), bytes)
            }
            None => return,
        };
        if bytes.is_empty() {
            return;
        }
        let Some(writer) = writer else {
            return;
        };
        let Ok(serialized) = Self::frame_server_message(&ServerMessage::Graphics { bytes }) else {
            return;
        };
        let _ = writer.control.send(serialized);
    }

    fn send_all_clients_graphics_cleanup(&mut self) {
        let client_ids = self.clients.keys().copied().collect::<Vec<_>>();
        for client_id in client_ids {
            self.send_client_graphics_cleanup(client_id);
        }
    }

    fn handle_open_project_command_request(&mut self) -> bool {
        let Some(kind) = self.app.state.request_open_project_command.take() else {
            return false;
        };

        let target_workspace = self.app.state.request_open_project_command_workspace.take();
        if kind == crate::app::state::ProjectCommandKind::Github {
            let target_client = self.foreground_client_id.and_then(|id| {
                self.clients
                    .get_mut(&id)
                    .and_then(|client| client.view_state.take())
                    .map(|view| (id, view))
            });
            if let Some((id, mut view)) = target_client {
                if target_workspace.is_some() {
                    view.active_workspace = target_workspace;
                }
                self.app.open_github_for_view(&mut view);
                if let Some(client) = self.clients.get_mut(&id) {
                    client.view_state = Some(view);
                }
            } else {
                self.app.open_default_github(target_workspace);
            }
            return true;
        }
        let result = if let Some(ws_idx) = target_workspace {
            self.app.state.open_project_command_for_workspace(
                &mut self.app.terminal_runtimes,
                ws_idx,
                kind,
            )
        } else {
            self.app
                .state
                .open_project_command(&mut self.app.terminal_runtimes, kind)
        };
        if let Err(err) = result {
            self.app.state.toast = Some(app::state::ToastNotification {
                kind: app::state::ToastKind::NeedsAttention,
                title: format!(
                    "{} Command Failed",
                    self.app.state.project_command_role(kind)
                ),
                context: err,
                position: None,
                target: None,
            });
            self.app.toast_deadline = Some(Instant::now() + Duration::from_secs(8));
        }

        true
    }

    fn update_client_host_theme_from_events(
        &mut self,
        client_id: u64,
        events: &[crate::raw_input::RawInputEvent],
    ) -> bool {
        let Some(client) = self.clients.get_mut(&client_id) else {
            return false;
        };

        if !client.update_host_theme_from_events(events) {
            return false;
        }

        if self.foreground_client_id == Some(client_id) {
            self.app.set_host_terminal_theme(client.host_terminal_theme)
        } else {
            false
        }
    }

    fn update_client_outer_focus_from_events(
        &mut self,
        client_id: u64,
        events: &[crate::raw_input::RawInputEvent],
    ) {
        let Some(client) = self.clients.get_mut(&client_id) else {
            return;
        };
        let Some(next_focus) = client.update_outer_focus_from_events(events) else {
            return;
        };
        if self.foreground_client_id == Some(client_id) {
            self.app.state.outer_terminal_focus = Some(next_focus);
        }
    }

    /// Accepts pending client connections from the non-blocking listener.
    fn accept_client_connections(&mut self) -> io::Result<()> {
        if self.handoff_in_progress {
            return reject_pending_client_connections(&self.client_listener);
        }
        accept_pending_client_connections(
            &self.client_listener,
            &mut self.next_client_id,
            &self.should_quit,
            &self.server_event_tx,
        )
    }

    /// Drains server events from the dedicated channel.
    ///
    /// Returns true if any input was processed (requiring a re-render).
    fn drain_server_events(&mut self) -> bool {
        let mut changed = false;
        while !self.should_quit.load(Ordering::Acquire) {
            let Ok(ev) = self.server_event_rx.try_recv() else {
                break;
            };
            changed |= self.handle_server_event(ev);
        }
        changed
    }

    fn terminal_id_by_string(&self, terminal_id: &str) -> Option<crate::terminal::TerminalId> {
        self.app
            .state
            .terminals
            .keys()
            .find(|id| id.to_string() == terminal_id)
            .cloned()
    }

    fn runtime_for_terminal_id_string(
        &self,
        terminal_id: &str,
    ) -> Option<&crate::terminal::TerminalRuntime> {
        let terminal_id = self.terminal_id_by_string(terminal_id)?;
        self.app.terminal_runtimes.get(&terminal_id)
    }

    fn client_clipboard_image_terminal(
        &self,
        client_id: u64,
    ) -> Option<crate::terminal::TerminalId> {
        let client = self.clients.get(&client_id)?;
        if let ClientConnectionMode::TerminalAttach { terminal_id } = &client.mode {
            return self.terminal_id_by_string(terminal_id);
        }
        let view = client.view_state.as_ref()?;
        let ws_idx = view.active_workspace?;
        let (_, pane_id) = view.focused_pane_for_workspace(&self.app.state, ws_idx)?;
        self.app
            .state
            .workspaces
            .get(ws_idx)?
            .pane_state(pane_id)
            .map(|pane| pane.attached_terminal_id.clone())
    }

    fn send_client_effect_error(
        &mut self,
        client_id: u64,
        code: crate::protocol::ClientEffectErrorCode,
        message: impl Into<String>,
    ) {
        self.send_to_client(
            client_id,
            ServerMessage::ClientEffectError {
                code,
                message: message.into(),
            },
        );
    }

    fn request_client_clipboard_image(
        &mut self,
        client_id: u64,
        extension: String,
        data: Vec<u8>,
    ) -> bool {
        let Some(terminal_id) = self.client_clipboard_image_terminal(client_id) else {
            self.send_client_effect_error(
                client_id,
                crate::protocol::ClientEffectErrorCode::Failed,
                "Clipboard image paste failed: no target terminal",
            );
            return false;
        };
        let Some(location) = self
            .app
            .state
            .terminals
            .get(&terminal_id)
            .map(|terminal| terminal.location.clone())
        else {
            self.send_client_effect_error(
                client_id,
                crate::protocol::ClientEffectErrorCode::Failed,
                "Clipboard image paste failed: target terminal is unavailable",
            );
            return false;
        };

        if location.is_local() {
            match crate::server::clipboard_image::stage(client_id, &extension, &data) {
                Ok(staged) => {
                    if let Some(client) = self.clients.get_mut(&client_id) {
                        client
                            .staged_clipboard_files
                            .push(StagedClipboardFile::Local(staged.path));
                        info!(client_id, bytes = data.len(), path = %staged.paste_text, "staged local clipboard image");
                        self.paste_client_clipboard_image_path(client_id, staged.paste_text);
                        return true;
                    }
                    crate::server::clipboard_image::remove_files(vec![staged.path]);
                    false
                }
                Err(error) => {
                    warn!(client_id, %error, "failed to stage local clipboard image");
                    self.send_client_effect_error(
                        client_id,
                        crate::protocol::ClientEffectErrorCode::Failed,
                        format!("Clipboard image paste failed: {error}"),
                    );
                    false
                }
            }
        } else {
            let Some(hosts) = self.app.execution_hosts.as_mut() else {
                self.send_client_effect_error(
                    client_id,
                    crate::protocol::ClientEffectErrorCode::Unsupported,
                    "Unsupported: target execution host cannot stage clipboard images",
                );
                return false;
            };
            let stage_location = location.clone();
            match hosts.request_stage_file(
                location,
                extension,
                data,
                crate::execution_host::staging::DEFAULT_STAGED_FILE_TTL.as_secs() as u32,
            ) {
                Ok(request_id) => {
                    self.pending_clipboard_image_stages.insert(
                        (stage_location.execution_host_id.clone(), request_id),
                        PendingClipboardImageStage {
                            client_id,
                            terminal_id,
                            location: stage_location,
                        },
                    );
                    true
                }
                Err(error) => {
                    let code = if matches!(
                        error,
                        crate::execution_host::HostOperationError::Unsupported { .. }
                    ) {
                        crate::protocol::ClientEffectErrorCode::Unsupported
                    } else {
                        crate::protocol::ClientEffectErrorCode::Failed
                    };
                    self.send_client_effect_error(
                        client_id,
                        code,
                        format!("Clipboard image paste failed: {error}"),
                    );
                    false
                }
            }
        }
    }

    fn complete_remote_clipboard_image_stage(
        &mut self,
        host_id: crate::execution_host::ExecutionHostId,
        request_id: crate::execution_host::protocol::RequestId,
        location: crate::execution_host::ResourceLocation,
        result: Result<
            crate::execution_host::HostPath,
            crate::execution_host::protocol::WorkerError,
        >,
    ) -> bool {
        let pending = self
            .pending_clipboard_image_stages
            .remove(&(host_id.clone(), request_id));
        if pending.as_ref().is_some_and(|pending| {
            pending.location != location || location.execution_host_id != host_id
        }) {
            if let Ok(path) = &result {
                let staged_location = crate::execution_host::ResourceLocation::new(
                    location.execution_host_id.clone(),
                    path.clone(),
                );
                self.remove_staged_clipboard_files(vec![StagedClipboardFile::Remote(
                    staged_location,
                )]);
            }
            return false;
        }
        let path = match result {
            Ok(path) => path,
            Err(error) => {
                if let Some(pending) = pending {
                    let code = if error.code
                        == crate::execution_host::protocol::WorkerErrorCode::UnsupportedCapability
                    {
                        crate::protocol::ClientEffectErrorCode::Unsupported
                    } else {
                        crate::protocol::ClientEffectErrorCode::Failed
                    };
                    self.send_client_effect_error(
                        pending.client_id,
                        code,
                        format!("Clipboard image paste failed: {}", error.message),
                    );
                }
                return false;
            }
        };
        let staged_location =
            crate::execution_host::ResourceLocation::new(location.execution_host_id, path.clone());
        let Some(pending) = pending else {
            self.remove_staged_clipboard_files(vec![StagedClipboardFile::Remote(staged_location)]);
            return false;
        };
        let target_matches = self.client_clipboard_image_terminal(pending.client_id)
            == Some(pending.terminal_id.clone())
            && self
                .app
                .state
                .terminals
                .get(&pending.terminal_id)
                .is_some_and(|terminal| {
                    terminal.location.execution_host_id == staged_location.execution_host_id
                });
        if !target_matches {
            self.remove_staged_clipboard_files(vec![StagedClipboardFile::Remote(staged_location)]);
            return false;
        }
        if let Some(client) = self.clients.get_mut(&pending.client_id) {
            client
                .staged_clipboard_files
                .push(StagedClipboardFile::Remote(staged_location));
        }
        info!(client_id = pending.client_id, path = %path, "staged remote clipboard image");
        self.paste_client_clipboard_image_path(pending.client_id, path.to_string())
    }

    fn paste_client_clipboard_image_path(&mut self, client_id: u64, path: String) -> bool {
        if let Some(ClientConnection {
            mode: ClientConnectionMode::TerminalAttach { terminal_id },
            ..
        }) = self.clients.get(&client_id)
        {
            if let Some(runtime) = self.runtime_for_terminal_id_string(terminal_id) {
                let payload = paste_payload_for_runtime(runtime, &path);
                if let Err(err) = runtime.try_send_bytes(Bytes::from(payload)) {
                    warn!(client_id, terminal_id = %terminal_id, err = %err, "terminal attach clipboard image paste failed");
                }
            }
            return true;
        }

        self.promote_client_to_foreground(client_id);
        if let Some(client) = self.clients.get_mut(&client_id) {
            client.request_semantic_redraw_after_input();
        }
        if let Some(mut view_state) = self
            .clients
            .get_mut(&client_id)
            .and_then(|client| client.view_state.take())
        {
            self.app.route_client_events_for_view(
                &mut view_state,
                vec![crate::raw_input::RawInputEvent::Paste(path)],
                true,
            );
            if let Some(client) = self.clients.get_mut(&client_id) {
                client.view_state = Some(view_state);
            }
            self.reconcile_client_tab_control(client_id);
        }
        true
    }

    fn pane_effective_state(&self, pane_id: crate::layout::PaneId) -> crate::detect::AgentState {
        self.app
            .state
            .workspaces
            .iter()
            .find_map(|ws| {
                ws.tabs.iter().find_map(|tab| {
                    let pane = tab.panes.get(&pane_id)?;
                    self.app
                        .state
                        .terminals
                        .get(&pane.attached_terminal_id)
                        .map(|terminal| terminal.state)
                })
            })
            .unwrap_or(crate::detect::AgentState::Unknown)
    }

    fn forward_agent_notification_delivery(
        &mut self,
        delivery: &crate::app::state::AgentNotificationDelivery,
    ) {
        if let Some(sound) = delivery.sound {
            self.send_notify_to_foreground_client(
                protocol::NotifyKind::Sound,
                sound_notify_message(sound),
                None,
            );
        }

        if should_forward_toast_to_clients(self.app.state.toast_config.delivery) {
            if let Some(toast) = &delivery.client_notification {
                self.send_notify_to_foreground_client(
                    toast_notify_kind(self.app.state.toast_config.delivery)
                        .expect("toast forwarding requires a client notification kind"),
                    &toast.title,
                    non_empty_body(&toast.context),
                );
            }
        }
    }

    fn send_notify_to_foreground_client(
        &mut self,
        kind: protocol::NotifyKind,
        message: impl Into<String>,
        body: Option<String>,
    ) -> bool {
        let message = message.into();
        let message = match body {
            Some(body) if !body.is_empty() => notification_message(&message, Some(body.as_str())),
            _ => message,
        };
        self.send_to_foreground_client(ServerMessage::Notify { kind, message })
    }

    fn handle_notification_show_api(
        &mut self,
        id: String,
        params: api::schema::NotificationShowParams,
    ) -> String {
        use api::schema::{NotificationShowReason, ResponseResult};

        let Some(title) = sanitize_notification_text(&params.title, 80) else {
            return serde_json::to_string(&api::schema::ErrorResponse {
                id,
                error: api::schema::ErrorBody {
                    code: "invalid_params".into(),
                    message: "notification title is empty".into(),
                },
            })
            .unwrap_or_else(|_| "{}".to_string());
        };

        match self.app.state.toast_config.delivery {
            config::ToastDelivery::Off => {
                return serde_json::to_string(&api::schema::SuccessResponse {
                    id,
                    result: ResponseResult::NotificationShow {
                        shown: false,
                        reason: NotificationShowReason::Disabled,
                    },
                })
                .unwrap_or_else(|_| "{}".to_string());
            }
            config::ToastDelivery::Gardn => {
                let sound = params.sound;
                let response = self.app.handle_api_request_after_internal_events_drained(
                    api::schema::Request {
                        id,
                        method: api::schema::Method::NotificationShow(params),
                    },
                );
                if notification_show_response_shown(&response) {
                    self.forward_api_notification_sound(sound);
                }
                return response;
            }
            config::ToastDelivery::Terminal | config::ToastDelivery::System => {}
        }

        let body = params
            .body
            .as_deref()
            .and_then(|body| sanitize_notification_text(body, 240));
        if self.app.api_notification_rate_limited(Instant::now()) {
            return serde_json::to_string(&api::schema::SuccessResponse {
                id,
                result: ResponseResult::NotificationShow {
                    shown: false,
                    reason: NotificationShowReason::RateLimited,
                },
            })
            .unwrap_or_else(|_| "{}".to_string());
        }
        let kind = toast_notify_kind(self.app.state.toast_config.delivery)
            .expect("terminal/system delivery has notify kind");
        let shown = self.send_notify_to_foreground_client(kind, title, body);
        if shown {
            self.app.mark_api_notification_shown(Instant::now());
            self.forward_api_notification_sound(params.sound);
        }
        let reason = if shown {
            NotificationShowReason::Shown
        } else {
            NotificationShowReason::NoForegroundClient
        };

        serde_json::to_string(&api::schema::SuccessResponse {
            id,
            result: ResponseResult::NotificationShow { shown, reason },
        })
        .unwrap_or_else(|_| "{}".to_string())
    }

    /// Renders `ui.window_title` against current session state. `None` means
    /// window titles are disabled or every token resolved empty, which leaves
    /// the client on Gardn's default title.
    fn configured_window_title(&self) -> Option<String> {
        self.app
            .window_title()
            .and_then(|title| crate::config::sanitize_window_title_text(&title))
    }

    /// Pushes the configured outer window title to the foreground client when
    /// it changed. Gardn consumes each pane's own `OSC 0`/`OSC 2`, so
    /// without this the host terminal title never follows the session — which
    /// is what window managers read for tab and group bar labels.
    fn sync_window_title(&mut self) {
        let title = match &self.api_window_title {
            Some(title) => Some(title.clone()),
            None if self.app.window_title_configured() => self.configured_window_title(),
            None => return,
        };
        if let (Some(client_id), Some((sent_client_id, sent_title))) =
            (self.foreground_client_id, self.sent_window_title.as_ref())
        {
            let foreground_attached = self
                .clients
                .get(&client_id)
                .is_some_and(|client| client.writer.is_some());
            if foreground_attached && *sent_client_id == client_id && *sent_title == title {
                return;
            }
        }
        self.send_window_title(title);
    }

    /// Sends a window title and remembers it only when a foreground client
    /// took it, so the next client to attach is written to rather than
    /// skipped.
    fn send_window_title(&mut self, title: Option<String>) -> bool {
        let Some(client_id) = self.foreground_client_id else {
            self.sent_window_title = None;
            return false;
        };
        // A detached client keeps its entry with no writer, and a targeted
        // send to one reports success without queuing anything. Caching the
        // title against that client would skip the send once it attaches
        // again.
        if self
            .clients
            .get(&client_id)
            .is_none_or(|client| client.writer.is_none())
        {
            self.sent_window_title = None;
            return false;
        }
        let sent = self.send_to_client(
            client_id,
            ServerMessage::WindowTitle {
                title: title.clone(),
            },
        );
        self.sent_window_title = sent.then_some((client_id, title));
        sent
    }

    fn handle_client_window_title_api(&mut self, id: String, title: Option<String>) -> String {
        use api::schema::{ClientWindowTitleReason, ResponseResult};

        let title = match title {
            Some(title) => match crate::config::sanitize_window_title_text(&title) {
                Some(title) => Some(title),
                None => {
                    return serde_json::to_string(&api::schema::ErrorResponse {
                        id,
                        error: api::schema::ErrorBody {
                            code: "invalid_params".into(),
                            message: "window title is empty".into(),
                        },
                    })
                    .unwrap_or_else(|_| "{}".to_string());
                }
            },
            None => None,
        };
        let set_title = title.is_some();
        // An explicit title suppresses `ui.window_title` until it is cleared,
        // and clearing restores the configured title rather than only "gardn".
        self.api_window_title = title.clone();
        let title = title.or_else(|| self.configured_window_title());
        let changed = self.send_window_title(title);
        let reason = match (changed, set_title) {
            (true, true) => ClientWindowTitleReason::Set,
            (true, false) => ClientWindowTitleReason::Cleared,
            (false, _) => ClientWindowTitleReason::NoForegroundClient,
        };
        serde_json::to_string(&api::schema::SuccessResponse {
            id,
            result: ResponseResult::ClientWindowTitle { changed, reason },
        })
        .unwrap_or_else(|_| "{}".to_string())
    }

    fn forward_api_notification_sound(&mut self, sound: api::schema::NotificationShowSound) {
        let Some(sound) = api::schema::notification_show_sound_to_sound(sound) else {
            return;
        };
        self.send_notify_to_foreground_client(
            protocol::NotifyKind::Sound,
            sound_notify_message(sound),
            None,
        );
    }
    /// Handles a single internal event with forwarding logic for clipboard,
    /// sound, and toast notifications to connected clients.
    ///
    /// ALL internal events MUST be routed through this method to ensure
    /// clipboard/notify forwarding is never bypassed. Do not call
    /// `self.app.handle_internal_event()` directly for any internal event
    /// in the headless server — use this method instead.
    ///
    /// Returns true if the event changed visual state (requiring a re-render).
    fn foreground_clipboard_controller(&self) -> Option<u64> {
        let client_id = self.foreground_client_id?;
        self.clients
            .get(&client_id)
            .is_some_and(ClientConnection::is_full_app_client)
            .then_some(client_id)
    }

    fn clipboard_controller_for_pane(&self, pane_id: crate::layout::PaneId) -> Option<u64> {
        for (terminal_id, owner) in &self.terminal_attach_owners {
            // Stale attach mappings must not abort ownership resolution for later
            // valid entries or tab/popup controllers.
            let Some(terminal_id) = self.terminal_id_by_string(terminal_id) else {
                continue;
            };
            let owns_workspace_pane = self.app.state.workspaces.iter().any(|workspace| {
                workspace.tabs.iter().any(|tab| {
                    tab.panes
                        .get(&pane_id)
                        .is_some_and(|pane| pane.attached_terminal_id == terminal_id)
                })
            });
            if owns_workspace_pane {
                return Some(*owner);
            }
            let owns_popup_pane = self
                .app
                .state
                .popup_panes
                .get(&pane_id)
                .is_some_and(|popup| popup.terminal_id == terminal_id);
            if owns_popup_pane {
                return Some(*owner);
            }
        }

        if let Some(popup) = self.app.state.popup_panes.get(&pane_id) {
            if let Some(owner_view_id) = popup.owner {
                if let Some((&client_id, _)) = self.clients.iter().find(|(_, client)| {
                    client
                        .view_state
                        .as_ref()
                        .is_some_and(|view| view.id() == owner_view_id)
                }) {
                    return Some(client_id);
                }
            }
            if let Some((&client_id, _)) = self.clients.iter().find(|(_, client)| {
                client
                    .view_state
                    .as_ref()
                    .is_some_and(|view| view.popup_pane == Some(pane_id))
            }) {
                return Some(client_id);
            }
        }

        self.app.state.workspaces.iter().find_map(|workspace| {
            workspace.tabs.iter().find_map(|tab| {
                tab.panes.contains_key(&pane_id).then(|| {
                    self.tab_controls
                        .status(&TabControlKey::new(&workspace.id, tab.number))
                        .controller
                })
            })
        })?
    }

    fn pane_execution_host_is_local(&self, pane_id: crate::layout::PaneId) -> bool {
        if let Some(popup) = self.app.state.popup_panes.get(&pane_id) {
            return self
                .app
                .state
                .terminals
                .get(&popup.terminal_id)
                .is_some_and(|terminal| terminal.location.is_local());
        }
        self.app.state.workspaces.iter().any(|workspace| {
            workspace.tabs.iter().any(|tab| {
                tab.panes.get(&pane_id).is_some_and(|pane| {
                    self.app
                        .state
                        .terminals
                        .get(&pane.attached_terminal_id)
                        .is_some_and(|terminal| terminal.location.is_local())
                })
            })
        })
    }

    fn handle_internal_event_with_forwarding(&mut self, ev: AppEvent) -> bool {
        match &ev {
            AppEvent::ClientClipboardWrite { view_id, content } => {
                let client_id = self.clients.iter().find_map(|(id, client)| {
                    client
                        .view_state
                        .as_ref()
                        .filter(|view| view.id() == *view_id)
                        .map(|_| *id)
                });
                if let Some(client_id) = client_id {
                    let data = base64::engine::general_purpose::STANDARD.encode(content);
                    self.send_to_client(client_id, ServerMessage::Clipboard { data });
                }
                true
            }
            AppEvent::ClientOpenUrl { view_id, url } => {
                let client_id = self.clients.iter().find_map(|(id, client)| {
                    client
                        .view_state
                        .as_ref()
                        .filter(|view| view.id() == *view_id)
                        .map(|_| *id)
                });
                if let Some(client_id) = client_id {
                    if crate::app::rendering_client_may_open_url(url, false) {
                        self.send_to_client(client_id, ServerMessage::OpenUrl { url: url.clone() });
                    }
                }
                true
            }
            AppEvent::ClipboardWrite { content } => {
                // UI clipboard writes belong to the invoking foreground controller.
                let data = base64::engine::general_purpose::STANDARD.encode(content.as_slice());
                if let Some(client_id) = self.foreground_clipboard_controller() {
                    if self.send_to_client(client_id, ServerMessage::Clipboard { data }) {
                        self.app.show_clipboard_feedback();
                    }
                }
                true
            }
            AppEvent::TerminalClipboardWrite { pane_id, content } => {
                let data = base64::engine::general_purpose::STANDARD.encode(content.as_slice());
                if let Some(client_id) = self.clipboard_controller_for_pane(*pane_id) {
                    if self.send_to_client(client_id, ServerMessage::Clipboard { data }) {
                        self.app.show_clipboard_feedback();
                    }
                }
                true
            }
            AppEvent::ExecutionFileStaged {
                host_id,
                request_id,
                location,
                result,
            } => self.complete_remote_clipboard_image_stage(
                host_id.clone(),
                *request_id,
                location.clone(),
                result.clone(),
            ),
            AppEvent::OpenUrl { pane_id, url } => {
                let Some(client_id) = self.clipboard_controller_for_pane(*pane_id) else {
                    return true;
                };
                if crate::app::rendering_client_may_open_url(
                    url,
                    self.pane_execution_host_is_local(*pane_id),
                ) {
                    self.send_to_client(client_id, ServerMessage::OpenUrl { url: url.clone() });
                } else {
                    self.send_client_effect_error(
                        client_id,
                        crate::protocol::ClientEffectErrorCode::Unsupported,
                        format!(
                            "Refused to open execution-host-local URL on rendering host: {url}"
                        ),
                    );
                }
                true
            }
            AppEvent::TerminalBell { pane_id, count } => {
                if !self.send_to_foreground_client(ServerMessage::TerminalBell { count: *count }) {
                    debug!(
                        pane = pane_id.raw(),
                        count, "dropped terminal bell without a foreground client"
                    );
                }
                true
            }
            AppEvent::PrefixInputSource { active } => {
                // Input-source switching is host-local; only the foreground
                // client can safely apply it.
                self.send_to_foreground_client(ServerMessage::PrefixInputSource {
                    active: *active,
                });
                true
            }
            AppEvent::StateChanged { pane_id, agent, .. } => {
                // Capture toast before handling.
                let toast_before = self.app.state.toast.clone();
                let pane_id_val = *pane_id;
                let agent_val = *agent;

                // Find the previous effective state of this pane before the event
                // is processed. Notifications must follow effective state changes,
                // not raw fallback reports that may be masked by hook authority.
                let prev_state = self.pane_effective_state(pane_id_val);

                // Handle the state change using the rendering client's view. Shared
                // workspace focus can be stale when clients navigate independently.
                let is_active_tab = self.foreground_client_view_contains_pane(pane_id_val);
                self.sync_foreground_client_state();
                self.app
                    .handle_internal_event_for_active_tab(ev, is_active_tab);

                let suppress_active_tab_notifications =
                    self.active_tab_suppresses_notifications(is_active_tab);

                let next_state = self.pane_effective_state(pane_id_val);

                if self.app.state.toast_config.delay_seconds == 0
                    && self.app.state.sound.allows(agent_val)
                {
                    if let Some(sound) = crate::app::actions::notification_sound_for_state_change(
                        suppress_active_tab_notifications,
                        prev_state,
                        next_state,
                    ) {
                        let msg = match sound {
                            crate::sound::Sound::Done => "agent done",
                            crate::sound::Sound::Request => "agent attention",
                        };
                        self.send_to_foreground_client(ServerMessage::Notify {
                            kind: protocol::NotifyKind::Sound,
                            message: msg.to_owned(),
                        });
                    }
                }

                let toast_msg = if self.app.state.toast_config.delay_seconds == 0
                    && should_forward_toast_to_clients(self.app.state.toast_config.delivery)
                {
                    if self.app.state.toast.is_some() && self.app.state.toast != toast_before {
                        self.app
                            .state
                            .toast
                            .as_ref()
                            .map(|toast| format!("{}: {}", toast.title, toast.context))
                    } else {
                        toast_message_from_state_change(
                            &self.app.state,
                            &self.app.terminal_runtimes,
                            pane_id_val,
                            suppress_active_tab_notifications,
                            prev_state,
                            next_state,
                        )
                    }
                } else {
                    None
                };

                if let Some(msg) = toast_msg {
                    self.send_to_foreground_client(ServerMessage::Notify {
                        kind: toast_notify_kind(self.app.state.toast_config.delivery)
                            .expect("toast forwarding requires a client notification kind"),
                        message: msg,
                    });
                }

                true
            }
            AppEvent::HookStateReported {
                pane_id,
                agent_label,
                ..
            } => {
                // Hook reports can be stale or no-op after sequence rejection.
                // Forward only effective state changes observed after handling.
                let toast_before = self.app.state.toast.clone();
                let pane_id_val = *pane_id;
                let agent_val = crate::detect::parse_agent_label(agent_label);

                // Capture the previous effective state for this pane. Hook reports
                // are already folded into pane.state; raw hook transitions must not
                // produce a second notification path.
                let prev_state = self.pane_effective_state(pane_id_val);

                self.sync_foreground_client_state();
                self.app.handle_internal_event(ev);

                // Forward sound notification based on the effective transition when
                // server-side sound policy allows it.
                let is_active_tab = self.foreground_client_view_contains_pane(pane_id_val);

                let suppress_active_tab_notifications =
                    self.active_tab_suppresses_notifications(is_active_tab);

                let next_state = self.pane_effective_state(pane_id_val);

                if self.app.state.toast_config.delay_seconds == 0
                    && self.app.state.sound.allows(agent_val)
                {
                    if let Some(sound) = crate::app::actions::notification_sound_for_state_change(
                        suppress_active_tab_notifications,
                        prev_state,
                        next_state,
                    ) {
                        let msg = match sound {
                            crate::sound::Sound::Done => "agent done",
                            crate::sound::Sound::Request => "agent attention",
                        };
                        self.send_to_foreground_client(ServerMessage::Notify {
                            kind: protocol::NotifyKind::Sound,
                            message: msg.to_owned(),
                        });
                    }
                }

                let toast_msg = if self.app.state.toast_config.delay_seconds == 0
                    && should_forward_toast_to_clients(self.app.state.toast_config.delivery)
                {
                    if self.app.state.toast.is_some() && self.app.state.toast != toast_before {
                        self.app
                            .state
                            .toast
                            .as_ref()
                            .map(|toast| format!("{}: {}", toast.title, toast.context))
                    } else {
                        toast_message_from_state_change(
                            &self.app.state,
                            &self.app.terminal_runtimes,
                            pane_id_val,
                            suppress_active_tab_notifications,
                            prev_state,
                            next_state,
                        )
                    }
                } else {
                    None
                };

                if let Some(msg) = toast_msg {
                    self.send_to_foreground_client(ServerMessage::Notify {
                        kind: toast_notify_kind(self.app.state.toast_config.delivery)
                            .expect("toast forwarding requires a client notification kind"),
                        message: msg,
                    });
                }

                true
            }
            AppEvent::UpdateReady { version, install } => {
                let toast_before = self.app.state.toast.clone();
                let version = version.clone();
                let install = *install;

                self.app.handle_internal_event(ev);

                let toast_msg = if self.app.state.toast_config.delay_seconds == 0
                    && should_forward_toast_to_clients(self.app.state.toast_config.delivery)
                {
                    if self.app.state.toast.is_some() && self.app.state.toast != toast_before {
                        self.app
                            .state
                            .toast
                            .as_ref()
                            .map(|toast| format!("{}: {}", toast.title, toast.context))
                    } else {
                        Some(install.availability_notification_body(&version))
                    }
                } else {
                    None
                };

                if let Some(msg) = toast_msg {
                    self.send_to_foreground_client(ServerMessage::Notify {
                        kind: toast_notify_kind(self.app.state.toast_config.delivery)
                            .expect("toast forwarding requires a client notification kind"),
                        message: msg,
                    });
                }

                true
            }
            AppEvent::PaneDied { pane_id, .. } => {
                let pane_id_val = *pane_id;
                let terminal_id = self.app.state.workspaces.iter().find_map(|ws| {
                    ws.tabs.iter().find_map(|tab| {
                        tab.panes
                            .get(pane_id)
                            .map(|pane| pane.attached_terminal_id.clone())
                    })
                });

                self.app.handle_internal_event(ev);

                if self.app.find_pane(pane_id_val).is_none() {
                    if let Some(terminal_id) = terminal_id {
                        let terminal_id = terminal_id.to_string();
                        self.shutdown_terminal_attach_clients(
                            &terminal_id,
                            format!("terminal {terminal_id} exited"),
                        );
                    }
                }

                true
            }
            AppEvent::ConnectionRetirementPreviewed {
                authentication_owner,
                profile_id,
                result,
            } => self.apply_connection_retirement_previewed_event(
                *authentication_owner,
                profile_id,
                result,
            ),
            AppEvent::ConnectionRetirementStarted {
                authentication_owner,
                profile_id,
                preview,
            } => self.apply_connection_retirement_started_event(
                *authentication_owner,
                profile_id,
                preview,
            ),
            AppEvent::ConnectionRetired {
                authentication_owner,
                profile_id,
                result,
                journal,
            } => {
                let final_result = self
                    .app
                    .finalize_connection_retirement(profile_id, result, journal);
                self.apply_connection_retired_event(
                    *authentication_owner,
                    profile_id,
                    &final_result,
                )
            }
            _ => {
                self.app.handle_internal_event(ev);
                true
            }
        }
    }

    fn apply_connection_retirement_previewed_event(
        &mut self,
        owner: crate::execution_host::auth::AuthenticationOwner,
        profile_id: &str,
        result: &Result<crate::app::state::ConnectionRetirementPreview, String>,
    ) -> bool {
        let mut changed = false;
        for client in self.clients.values_mut() {
            let Some(view) = client.view_state.as_mut() else {
                continue;
            };
            if crate::app::App::apply_connection_retirement_previewed_to_view(
                view, owner, profile_id, result,
            ) {
                client.render_pending = true;
                changed = true;
                break;
            }
        }
        changed |= self
            .app
            .apply_connection_retirement_previewed_for_owner(owner, profile_id, result);
        changed
    }

    fn apply_connection_retirement_started_event(
        &mut self,
        owner: crate::execution_host::auth::AuthenticationOwner,
        profile_id: &str,
        preview: &crate::app::state::ConnectionRetirementPreview,
    ) -> bool {
        let mut changed = false;
        for client in self.clients.values_mut() {
            let Some(view) = client.view_state.as_mut() else {
                continue;
            };
            if crate::app::App::apply_connection_retirement_started_to_view(
                view, owner, profile_id, preview,
            ) {
                client.render_pending = true;
                changed = true;
                break;
            }
        }
        changed |= self
            .app
            .apply_connection_retirement_started_for_owner(owner, profile_id, preview);
        changed
    }

    fn apply_connection_retired_event(
        &mut self,
        owner: crate::execution_host::auth::AuthenticationOwner,
        profile_id: &str,
        result: &Result<String, String>,
    ) -> bool {
        let mut changed = false;
        for client in self.clients.values_mut() {
            let Some(view) = client.view_state.as_mut() else {
                continue;
            };
            if crate::app::App::apply_connection_retired_to_view(view, owner, profile_id, result) {
                client.render_pending = true;
                changed = true;
                break;
            }
        }
        changed |= self
            .app
            .apply_connection_retired_for_owner(owner, profile_id, result);
        changed
    }

    /// Drains internal events, forwarding clipboard, sound, and toast
    /// notifications to connected clients instead of processing them locally.
    ///
    /// In the monolithic mode:
    /// - `ClipboardWrite` events are written to stdout via `write_osc52_bytes`.
    /// - Sound notifications are played locally via `sound::play`.
    /// - Toast notifications are set on AppState and rendered into the frame.
    ///
    /// In the headless server, there is no stdout terminal or audio subsystem,
    /// so we:
    /// - Forward `ClipboardWrite` as `ServerMessage::Clipboard` to the
    ///   foreground client only.
    /// - Detect when a sound would be played and forward as
    ///   `ServerMessage::Notify { kind: Sound }` to the foreground client.
    /// - Detect when a toast is set on AppState and forward as
    ///   `ServerMessage::Notify` to the foreground client for terminal/system delivery.
    fn drain_internal_events_with_forwarding(&mut self) -> bool {
        let mut changed = false;
        while let Ok(ev) = self.app.event_rx.try_recv() {
            changed |= self.handle_internal_event_with_forwarding(ev);
        }
        changed
    }

    fn drain_client_config_reload_request(&mut self) {
        if !self.app.state.request_client_config_reload {
            return;
        }
        self.app.state.request_client_config_reload = false;
        self.send_to_all_clients(ServerMessage::ReloadSoundConfig);
    }

    /// Encodes a server message into a length-prefixed frame.
    fn frame_server_message(msg: &ServerMessage) -> Result<Vec<u8>, protocol::FramingError> {
        Self::frame_server_message_with_max(msg, MAX_FRAME_SIZE)
    }

    /// Encodes a server message using an explicit payload cap.
    fn frame_server_message_with_max(
        msg: &ServerMessage,
        max_frame_size: usize,
    ) -> Result<Vec<u8>, protocol::FramingError> {
        let mut framed = Vec::new();
        protocol::write_message(&mut framed, msg)?;
        let payload_len = framed.len().saturating_sub(4);
        if payload_len > max_frame_size {
            return Err(protocol::FramingError::Oversized {
                claimed: payload_len,
                max: max_frame_size,
            });
        }
        Ok(framed)
    }

    /// Sends a message to all connected clients.
    /// Broken connections are tracked and cleaned up.
    fn send_to_all_clients(&mut self, msg: ServerMessage) {
        let serialized = match Self::frame_server_message(&msg) {
            Ok(framed) => framed,
            Err(err) => {
                warn!(err = %err, "failed to serialize message for clients");
                return;
            }
        };

        let mut broken_clients: Vec<u64> = Vec::new();
        for (&client_id, client) in &mut self.clients {
            if let Some(writer) = &client.writer {
                if writer.control.send(serialized.clone()).is_err() {
                    debug!(client_id, "client writer channel closed during broadcast");
                    broken_clients.push(client_id);
                }
            }
        }

        // Remove broken clients.
        for client_id in broken_clients {
            self.remove_client(client_id);
        }
    }

    /// Sends a client-local side effect to the foreground client only.
    fn send_to_foreground_client(&mut self, msg: ServerMessage) -> bool {
        let Some(client_id) = self.foreground_client_id else {
            return false;
        };
        self.send_to_client(client_id, msg)
    }

    /// Sends a message to a specific client. Returns false if the client
    /// was not found or the send failed (client removed).
    fn send_to_client(&mut self, client_id: u64, msg: ServerMessage) -> bool {
        let serialized = match Self::frame_server_message(&msg) {
            Ok(framed) => framed,
            Err(err) => {
                warn!(client_id, err = %err, "failed to serialize message for client");
                return false;
            }
        };

        if let Some(client) = self.clients.get(&client_id) {
            if let Some(writer) = &client.writer {
                if writer.control.send(serialized).is_err() {
                    debug!(
                        client_id,
                        "client writer channel closed during targeted send"
                    );
                    self.remove_client(client_id);
                    return false;
                }
            }
            true
        } else {
            false
        }
    }

    fn shutdown_terminal_attach_clients(&mut self, terminal_id: &str, reason: String) {
        let client_ids = terminal_attach_client_ids(&self.clients, terminal_id);

        for client_id in client_ids {
            self.send_to_client(
                client_id,
                ServerMessage::ServerShutdown {
                    reason: Some(reason.clone()),
                },
            );
            self.remove_client(client_id);
        }
    }

    fn disconnect_all_clients_for_handoff(&mut self) {
        let client_ids = self.clients.keys().copied().collect::<Vec<_>>();
        for client_id in client_ids {
            self.send_client_graphics_cleanup(client_id);
            self.send_to_client(
                client_id,
                ServerMessage::ServerShutdown {
                    reason: Some(
                        "live update in progress; reconnect after handoff completes".to_owned(),
                    ),
                },
            );
            if let Some(client) = self.clients.get_mut(&client_id) {
                client.writer = None;
            }
            let _ = self.remove_client(client_id);
        }
        self.foreground_client_id = None;
        self.sync_foreground_client_state();
    }

    fn attach_terminal_client(
        &mut self,
        client_id: u64,
        terminal_id: String,
        takeover: bool,
    ) -> bool {
        let Some(real_terminal_id) = self.terminal_id_by_string(&terminal_id) else {
            self.send_to_client(
                client_id,
                ServerMessage::ServerShutdown {
                    reason: Some(format!(
                        "terminal attach failed: terminal {terminal_id} not found"
                    )),
                },
            );
            self.remove_client(client_id);
            return false;
        };

        if self
            .pending_alt_screen_reads
            .iter()
            .any(|pending| pending.terminal_id == real_terminal_id)
        {
            self.send_to_client(
                client_id,
                ServerMessage::ServerShutdown {
                    reason: Some(format!(
                        "terminal attach failed: terminal {terminal_id} has a read in progress; retry"
                    )),
                },
            );
            self.remove_client(client_id);
            return false;
        }

        if let Some(existing_owner) = self.terminal_attach_owners.get(&terminal_id).copied() {
            if existing_owner != client_id && !takeover {
                self.send_to_client(
                    client_id,
                    ServerMessage::ServerShutdown {
                        reason: Some(format!(
                            "terminal attach failed: terminal {terminal_id} already has an attached client; retry with --takeover"
                        )),
                    },
                );
                self.remove_client(client_id);
                return false;
            }
            if existing_owner != client_id {
                self.send_to_client(
                    existing_owner,
                    ServerMessage::ServerShutdown {
                        reason: Some("terminal attach taken over".to_owned()),
                    },
                );
                self.remove_client(existing_owner);
            }
        }

        let stamp = self.allocate_activity_stamp();
        let Some(client) = self.clients.get_mut(&client_id) else {
            return false;
        };
        let (cols, rows) = client.terminal_size;
        let cell_size = client.cell_size;
        client.mode = ClientConnectionMode::TerminalAttach {
            terminal_id: terminal_id.clone(),
        };
        client.pending_terminal_attach = false;
        client.render_state.reset_baseline();
        client.last_activity = stamp;
        let was_foreground = self.foreground_client_id == Some(client_id);
        if was_foreground {
            self.promote_latest_remaining_client();
        }
        self.client_tab_keys.remove(&client_id);
        if self.release_client_tab_control(client_id) {
            self.sync_all_tab_control_projections();
        }

        info!(client_id, cols, rows, terminal_id = %terminal_id, "terminal attach client connected");
        self.terminal_attach_owners
            .insert(terminal_id.clone(), client_id);
        self.app
            .state
            .direct_attach_resize_locks
            .insert(real_terminal_id.clone());
        if let Some(runtime) = self.app.terminal_runtimes.get(&real_terminal_id) {
            runtime.resize(rows, cols, cell_size.width_px, cell_size.height_px);
        }
        true
    }

    /// Handles a server event. Returns true if the event requires a re-render.
    fn handle_server_event(&mut self, ev: ServerEvent) -> bool {
        if self.handoff_in_progress && Self::ignore_client_event_during_handoff(&ev) {
            return false;
        }

        match ev {
            ServerEvent::ClientConnected {
                client_id,
                cols,
                rows,
                cell_width_px,
                cell_height_px,
                keybindings,
                writer,
                render_encoding,
                direct_attach_requested,
                direct_graphics,
            } => {
                if self.handoff_in_progress {
                    if let Ok(message) =
                        Self::frame_server_message(&ServerMessage::ServerShutdown {
                            reason: Some(
                                "live update in progress; reconnect after handoff completes"
                                    .to_owned(),
                            ),
                        })
                    {
                        let _ = writer.control.send(message);
                    }
                    return false;
                }
                let first_app_client = !direct_attach_requested && self.app_client_count() == 0;
                info!(
                    client_id,
                    cols,
                    rows,
                    cell_width_px,
                    cell_height_px,
                    ?render_encoding,
                    "client connected"
                );
                let last_activity = self.allocate_activity_stamp();
                let mut client = ClientConnection::new_with_mode(
                    ClientConnectionMode::App,
                    keybindings,
                    (cols, rows),
                    crate::kitty_graphics::HostCellSize {
                        width_px: cell_width_px,
                        height_px: cell_height_px,
                    },
                    crate::terminal_theme::TerminalTheme::default(),
                    None,
                    last_activity,
                    render_encoding,
                    direct_attach_requested,
                    Some(writer),
                );
                if !direct_attach_requested {
                    let mut view_state =
                        crate::app::ClientViewState::for_new_client(&self.app.state);
                    if !self.app.state.workspaces.is_empty() {
                        view_state.mode = crate::app::Mode::Terminal;
                    }
                    client.view_state = Some(view_state);
                }
                self.clients.insert(client_id, client);
                if !direct_attach_requested {
                    self.foreground_client_id = Some(client_id);
                }
                if first_app_client {
                    self.app.mark_git_status_refresh_due(Instant::now());
                }
                if !direct_attach_requested {
                    self.app.direct_graphics_available = self.app.direct_graphics_available
                        || (direct_graphics && cell_width_px > 0 && cell_height_px > 0);
                }
                self.sync_foreground_client_state();
                if !direct_attach_requested {
                    self.reconcile_client_tab_control(client_id);
                }
                self.nudge_handoff_panes_on_first_client_attach();
                true
            }
            ServerEvent::ClientAttachTerminal {
                client_id,
                terminal_id,
                takeover,
            } => self.attach_terminal_client(client_id, terminal_id, takeover),
            ServerEvent::ClientInput { client_id, data } => {
                if self.handoff_in_progress {
                    debug!(
                        client_id,
                        len = data.len(),
                        "ignored client input during handoff"
                    );
                    return false;
                }
                debug!(client_id, len = data.len(), "client input received");
                let source_was_foreground = self.foreground_client_id == Some(client_id);
                let source_is_full_app = self
                    .clients
                    .get(&client_id)
                    .is_some_and(ClientConnection::is_full_app_client);
                if let Some(ClientConnection {
                    mode: ClientConnectionMode::TerminalAttach { terminal_id },
                    ..
                }) = self.clients.get(&client_id)
                {
                    if let Some(runtime) = self.runtime_for_terminal_id_string(terminal_id) {
                        if let Err(err) = runtime.try_send_bytes(Bytes::from(data)) {
                            warn!(client_id, terminal_id = %terminal_id, err = %err, "terminal attach input failed");
                        }
                    }
                    return true;
                }
                let events = crate::raw_input::parse_raw_input_bytes_sync(&data);
                let host_surface_redraw = crate::raw_input::events_require_host_surface_redraw(
                    &events,
                    self.app.state.redraw_on_focus_gained,
                );
                if let Some(client) = self.clients.get_mut(&client_id) {
                    if host_surface_redraw {
                        client.request_repaint();
                        client.render_pending = true;
                    } else {
                        // Ensure semantic clients receive one post-input frame even if the
                        // semantic buffer compares equal. Terminal-ANSI clients must keep their
                        // server-side blit baseline; resetting it here forces a full redraw on
                        // every keypress and makes remote sessions feel extremely slow.
                        client.request_semantic_redraw_after_input();
                    }
                }
                if source_is_full_app {
                    self.update_client_outer_focus_from_events(client_id, &events);
                }
                let events =
                    events_for_app_routing(events, source_was_foreground, source_is_full_app);
                let interaction = events_include_interaction(&events);
                let motion_mode = self
                    .clients
                    .get(&client_id)
                    .and_then(|client| client.view_state.as_ref())
                    .map(|view| view.mode)
                    .unwrap_or(self.app.state.mode);
                let render_neutral_mouse_motion =
                    events_are_render_neutral_mouse_motion(&events, motion_mode);
                let foreground_changed = if interaction {
                    self.promote_client_to_foreground(client_id)
                } else {
                    false
                };
                let theme_changed = self.update_client_host_theme_from_events(client_id, &events);
                let apply_host_terminal_theme = self.foreground_client_id == Some(client_id);
                self.apply_client_pointer_context(client_id);
                if self
                    .clients
                    .get(&client_id)
                    .is_some_and(|client| client.view_state.is_some())
                {
                    if let Some(client) = self.clients.get_mut(&client_id) {
                        if let Some(view_state) = client.view_state.as_mut() {
                            self.app.route_client_events_for_view(
                                view_state,
                                events,
                                apply_host_terminal_theme,
                            );
                        }
                    }
                } else {
                    self.app
                        .route_client_events(events, apply_host_terminal_theme);
                }
                let control_changed = self.reconcile_client_tab_control(client_id);
                if self.app.take_config_reloaded_from_disk() {
                    self.reload_server_config(false);
                } else {
                    self.sync_foreground_client_state();
                }

                // Check if the detach keybind was triggered during input processing.
                if self.app.state.detach_requested {
                    self.app.state.detach_requested = false;
                    info!(client_id, "client detach requested via keybind");

                    // Clear client-local host graphics before sending ServerShutdown
                    // so the outer terminal does not retain stale images.
                    self.send_client_graphics_cleanup(client_id);

                    // Send a ServerShutdown with "detached" reason to this client
                    // so it exits cleanly (not with a connection-lost error).
                    // The client will close its connection after receiving this,
                    // which triggers a ClientDisconnected event that removes it.
                    self.send_to_client(
                        client_id,
                        ServerMessage::ServerShutdown {
                            reason: Some("detached".to_owned()),
                        },
                    );

                    // Don't remove the client here — let the client disconnect
                    // naturally after receiving the ServerShutdown. The client's
                    // read loop will see EOF and the server will get a
                    // ClientDisconnected event which handles cleanup.
                    //
                    // However, we do need to stop sending frames to this client
                    // since it's detaching. Drop the writer channel so no more
                    // frames are queued for this client.
                    if let Some(client) = self.clients.get_mut(&client_id) {
                        client.writer = None;
                    }

                    // No re-render needed for remaining clients.
                    false
                } else {
                    foreground_changed
                        || theme_changed
                        || (interaction && !render_neutral_mouse_motion)
                        || control_changed
                }
            }
            ServerEvent::ClientInputEvents { client_id, events } => {
                if self.handoff_in_progress {
                    debug!(
                        client_id,
                        len = events.len(),
                        "ignored client input events during handoff"
                    );
                    return false;
                }
                debug!(
                    client_id,
                    len = events.len(),
                    "client input events received"
                );
                let source_was_foreground = self.foreground_client_id == Some(client_id);
                let source_is_full_app = self
                    .clients
                    .get(&client_id)
                    .is_some_and(ClientConnection::is_full_app_client);
                if let Some(ClientConnection {
                    mode: ClientConnectionMode::TerminalAttach { terminal_id },
                    ..
                }) = self.clients.get(&client_id)
                {
                    if let Some(runtime) = self.runtime_for_terminal_id_string(terminal_id) {
                        for event in events {
                            let bytes = match event {
                                crate::raw_input::RawInputEvent::Key(key) => {
                                    runtime.encode_terminal_key(key)
                                }
                                crate::raw_input::RawInputEvent::TextCommit(commit) => {
                                    commit.into_string().into_bytes()
                                }
                                crate::raw_input::RawInputEvent::Paste(text) => text.into_bytes(),
                                _ => Vec::new(),
                            };
                            if !bytes.is_empty() {
                                if let Err(err) = runtime.try_send_bytes(Bytes::from(bytes)) {
                                    warn!(client_id, terminal_id = %terminal_id, err = %err, "terminal attach input event failed");
                                }
                            }
                        }
                    }
                    return true;
                }
                let host_surface_redraw = crate::raw_input::events_require_host_surface_redraw(
                    &events,
                    self.app.state.redraw_on_focus_gained,
                );
                if let Some(client) = self.clients.get_mut(&client_id) {
                    if host_surface_redraw {
                        client.request_repaint();
                        client.render_pending = true;
                    } else {
                        client.request_semantic_redraw_after_input();
                    }
                }
                if source_is_full_app {
                    self.update_client_outer_focus_from_events(client_id, &events);
                }
                let events =
                    events_for_app_routing(events, source_was_foreground, source_is_full_app);
                let interaction = events_include_interaction(&events);
                let motion_mode = self
                    .clients
                    .get(&client_id)
                    .and_then(|client| client.view_state.as_ref())
                    .map(|view| view.mode)
                    .unwrap_or(self.app.state.mode);
                let render_neutral_mouse_motion =
                    events_are_render_neutral_mouse_motion(&events, motion_mode);
                let foreground_changed = if interaction {
                    self.promote_client_to_foreground(client_id)
                } else {
                    false
                };
                let theme_changed = self.update_client_host_theme_from_events(client_id, &events);
                let apply_host_terminal_theme = self.foreground_client_id == Some(client_id);
                self.apply_client_pointer_context(client_id);
                if self
                    .clients
                    .get(&client_id)
                    .is_some_and(|client| client.view_state.is_some())
                {
                    if let Some(client) = self.clients.get_mut(&client_id) {
                        if let Some(view_state) = client.view_state.as_mut() {
                            self.app.route_client_events_for_view(
                                view_state,
                                events,
                                apply_host_terminal_theme,
                            );
                        }
                    }
                } else {
                    self.app
                        .route_client_events(events, apply_host_terminal_theme);
                }
                let control_changed = self.reconcile_client_tab_control(client_id);
                if self.app.take_config_reloaded_from_disk() {
                    self.reload_server_config(false);
                } else {
                    self.sync_foreground_client_state();
                }
                foreground_changed
                    || theme_changed
                    || (interaction && !render_neutral_mouse_motion)
                    || control_changed
            }
            ServerEvent::ClientPasteRejected {
                client_id,
                size,
                max,
            } => {
                self.send_to_client(
                    client_id,
                    ServerMessage::Notify {
                        kind: crate::protocol::NotifyKind::Toast,
                        message: format!(
                            "Paste rejected: input message is {size} bytes; \
                             Gardn's limit is {max} bytes"
                        ),
                    },
                );
                false
            }
            ServerEvent::ClientClipboardImage {
                client_id,
                extension,
                data,
            } => {
                debug!(
                    client_id,
                    len = data.len(),
                    extension = %extension,
                    "client clipboard image received"
                );
                self.request_client_clipboard_image(client_id, extension, data)
            }
            ServerEvent::ClientResize {
                client_id,
                cols,
                rows,
                cell_width_px,
                cell_height_px,
            } => {
                info!(
                    client_id,
                    cols, rows, cell_width_px, cell_height_px, "client resize"
                );
                let direct_terminal_id = if let Some(ClientConnection {
                    mode: ClientConnectionMode::TerminalAttach { terminal_id },
                    terminal_size,
                    cell_size,
                    render_state,
                    ..
                }) = self.clients.get_mut(&client_id)
                {
                    *terminal_size = (cols, rows);
                    *cell_size = crate::kitty_graphics::HostCellSize {
                        width_px: cell_width_px,
                        height_px: cell_height_px,
                    };
                    render_state.reset_baseline();
                    Some(terminal_id.clone())
                } else {
                    None
                };
                if let Some(terminal_id) = direct_terminal_id {
                    if let Some(runtime) = self.runtime_for_terminal_id_string(&terminal_id) {
                        runtime.resize(rows, cols, cell_width_px, cell_height_px);
                    }
                    return true;
                }
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.terminal_size = (cols, rows);
                    client.cell_size = crate::kitty_graphics::HostCellSize {
                        width_px: cell_width_px,
                        height_px: cell_height_px,
                    };
                }
                self.promote_client_to_foreground(client_id);
                self.sync_client_tab_control_projection(client_id);
                self.resize_controlled_tab_for_client(client_id);
                true
            }
            ServerEvent::ClientDetach { client_id } => {
                info!(client_id, "client detached");
                self.remove_client(client_id);
                true
            }
            ServerEvent::ClientDisconnected { client_id } => {
                info!(client_id, "client disconnected");
                self.remove_client(client_id);
                true
            }
            ServerEvent::ClientWriterDrained { client_id } => {
                let Some(client) = self.clients.get_mut(&client_id) else {
                    return false;
                };
                if client.render_pending {
                    client.render_pending = false;
                    true
                } else {
                    false
                }
            }
            ServerEvent::QuitSignal => {
                // The quit check at the top of the loop handles this.
                // No render needed — the next iteration will initiate shutdown.
                false
            }
        }
    }

    fn ignore_client_event_during_handoff(ev: &ServerEvent) -> bool {
        !matches!(
            ev,
            ServerEvent::ClientConnected { .. }
                | ServerEvent::ClientDisconnected { .. }
                | ServerEvent::ClientWriterDrained { .. }
                | ServerEvent::QuitSignal
        )
    }
    fn agent_read_not_idle_error(
        &self,
        request: &api::schema::Request,
    ) -> Option<api::schema::ErrorBody> {
        use api::schema::{Method, ReadFormat, ReadSource};

        let Method::AgentRead(params) = &request.method else {
            return None;
        };
        let requested = params.lines?;
        if params.format != ReadFormat::Text
            || !matches!(
                params.source,
                ReadSource::Recent | ReadSource::RecentUnwrapped
            )
        {
            return None;
        }
        let target = self.app.resolve_agent_target(&params.target).ok()?;
        let terminal = self
            .app
            .state
            .terminals
            .values()
            .find(|terminal| terminal.id.as_str() == target.terminal_id)?;
        if terminal.effective_known_agent().is_none()
            || terminal.state == crate::detect::AgentState::Idle
        {
            return None;
        }
        let runtime = self.app.terminal_runtimes.get(&terminal.id)?;
        let (screen, snapshot) = runtime.screen_text_snapshot()?;
        if screen != crate::ghostty::ActiveScreen::Alternate
            || snapshot.rows.len() >= requested.min(1000) as usize
        {
            return None;
        }
        let status = crate::detect::manifest::agent_state_label(terminal.state);
        Some(api::schema::ErrorBody {
            code: "agent_not_idle".into(),
            message: format!(
                "cannot read {requested} lines while {} is {status}: its alternate-screen history can only be captured by scrolling while idle. Wait and retry, or use --source visible",
                params.target
            ),
        })
    }

    fn alt_screen_read_spec(&self, request: &api::schema::Request) -> Option<AltScreenReadSpec> {
        use api::schema::{Method, ReadFormat, ReadIntent, ReadSource};

        let (target, source, lines, format) = match &request.method {
            Method::AgentRead(params) => (
                self.app.resolve_agent_target(&params.target).ok()?,
                params.source,
                params.lines,
                params.format,
            ),
            Method::PaneRead(params) if params.intent == ReadIntent::Interactive => (
                self.app.resolve_terminal_target(&params.pane_id).ok()?,
                params.source,
                params.lines,
                params.format,
            ),
            _ => return None,
        };
        if format != ReadFormat::Text
            || !matches!(source, ReadSource::Recent | ReadSource::RecentUnwrapped)
        {
            return None;
        }
        let lines = lines.unwrap_or(80).min(1000) as usize;
        if lines == 0
            || self
                .terminal_attach_owners
                .contains_key(target.terminal_id.as_str())
            || self
                .pending_alt_screen_reads
                .iter()
                .any(|pending| pending.terminal_id.as_str() == target.terminal_id)
        {
            return None;
        }
        let terminal = self
            .app
            .state
            .terminals
            .values()
            .find(|terminal| terminal.id.as_str() == target.terminal_id)?;
        if terminal.effective_known_agent().is_none()
            || terminal.state != crate::detect::AgentState::Idle
        {
            return None;
        }
        let runtime = self.app.terminal_runtimes.get(&terminal.id)?;
        if runtime.wheel_routing() != Some(crate::pane::WheelRouting::MouseReport) {
            return None;
        }
        let (screen, initial, content_seq) = runtime.screen_text_snapshot_with_seq()?;
        if screen != crate::ghostty::ActiveScreen::Alternate || initial.rows.len() >= lines {
            return None;
        }
        Some(AltScreenReadSpec {
            terminal_id: terminal.id.clone(),
            lines,
            unwrap: source == ReadSource::RecentUnwrapped,
            initial,
            content_seq,
        })
    }

    fn poll_pending_alt_screen_reads(&mut self, now: Instant) {
        let pending = std::mem::take(&mut self.pending_alt_screen_reads);
        for read in pending {
            let runtime = self.app.terminal_runtimes.get(&read.terminal_id);
            let remains_idle = self
                .app
                .state
                .terminals
                .get(&read.terminal_id)
                .is_some_and(|terminal| terminal.state == crate::detect::AgentState::Idle);
            let attached = self
                .terminal_attach_owners
                .contains_key(read.terminal_id.as_str());
            let outcome = if remains_idle && !attached {
                read.poll(runtime, now)
            } else {
                read.abort(runtime, now)
            };
            if let Some(read) = outcome {
                self.pending_alt_screen_reads.push(read);
            }
        }
    }

    fn alt_screen_read_conflict(&self, request: &api::schema::Request) -> AltScreenReadConflict {
        let (target, source, lines, format) = match &request.method {
            api::schema::Method::AgentRead(params) => (
                self.app.resolve_agent_target(&params.target).ok(),
                params.source,
                params.lines,
                params.format,
            ),
            api::schema::Method::PaneRead(params) => (
                self.app.resolve_terminal_target(&params.pane_id).ok(),
                params.source,
                params.lines,
                params.format,
            ),
            _ => return AltScreenReadConflict::None,
        };
        let Some(target) = target else {
            return AltScreenReadConflict::None;
        };
        let Some(pending) = self
            .pending_alt_screen_reads
            .iter()
            .find(|pending| pending.terminal_id.as_str() == target.terminal_id)
        else {
            return AltScreenReadConflict::None;
        };
        if format == api::schema::ReadFormat::Text {
            AltScreenReadConflict::Frozen(pending.frozen_snapshot(source, lines))
        } else {
            AltScreenReadConflict::Defer
        }
    }

    fn process_deferred_alt_screen_reads(&mut self) -> bool {
        let deferred = std::mem::take(&mut self.deferred_alt_screen_reads);
        let mut changed = false;
        for msg in deferred {
            match self.alt_screen_read_conflict(&msg.request) {
                AltScreenReadConflict::None => {
                    changed |= self.handle_api_request_with_shutdown_check(msg);
                }
                AltScreenReadConflict::Frozen(_) | AltScreenReadConflict::Defer => {
                    self.deferred_alt_screen_reads.push(msg);
                }
            }
        }
        changed
    }

    /// Drains API requests with shutdown awareness.
    ///
    /// During shutdown, remaining requests get a `server_unavailable` error.
    fn drain_api_requests_with_shutdown_check(&mut self) -> bool {
        let mut changed = false;
        while !self.should_quit.load(Ordering::Acquire) {
            let Ok(msg) = self.app.api_rx.try_recv() else {
                break;
            };
            changed |= self.handle_api_request_with_shutdown_check(msg);
        }
        changed
    }

    fn reject_queued_api_requests_for_shutdown(&mut self) {
        for _ in 0..self.app.api_rx.len() {
            let Ok(msg) = self.app.api_rx.try_recv() else {
                break;
            };
            self.handle_api_request_with_shutdown_check(msg);
        }
    }

    /// Handles a single API request with shutdown awareness.
    ///
    /// Also forwards any toast/sound notifications that result from the API
    /// request to connected clients. API methods like `pane.report_agent`
    /// trigger internal events that may set toast state or would normally
    /// play sounds — in headless mode we forward these to clients instead.
    fn handle_api_request_with_shutdown_check(&mut self, msg: api::ApiRequestMessage) -> bool {
        if self.shutting_down {
            // During shutdown, respond with server_unavailable.
            let response = serde_json::to_string(&api::schema::ErrorResponse {
                id: msg.request.id,
                error: api::schema::ErrorBody {
                    code: "server_unavailable".into(),
                    message: "server is shutting down".into(),
                },
            })
            .unwrap_or_else(|_| {
                r#"{"id":"","error":{"code":"server_unavailable","message":"server is shutting down"}}"#
                    .to_string()
            });
            let _ = msg.respond_to.send(response);
            return false;
        }

        if let api::schema::Method::ServerLiveHandoff(params) = &msg.request.method {
            let handoff_result = self.perform_live_handoff(params.clone());
            let handoff_succeeded = handoff_result.is_ok();
            let response = match handoff_result {
                Ok(()) => serde_json::to_string(&api::schema::SuccessResponse {
                    id: msg.request.id,
                    result: api::schema::ResponseResult::Ok {},
                }),
                Err(err) => serde_json::to_string(&api::schema::ErrorResponse {
                    id: msg.request.id,
                    error: api::schema::ErrorBody {
                        code: "handoff_failed".into(),
                        message: err.to_string(),
                    },
                }),
            }
            .unwrap_or_else(|_| "{}".to_string());
            let response_sent = msg.respond_to.send(response).is_ok();
            if handoff_succeeded
                && response_sent
                && msg.response_written.is_some_and(|response_written| {
                    response_written
                        .recv_timeout(LIVE_HANDOFF_RESPONSE_TIMEOUT)
                        .is_err()
                })
            {
                warn!("live handoff response was not written before the old server exited");
            }
            return true;
        }

        if let api::schema::Method::NotificationShow(params) = &msg.request.method {
            let response =
                self.handle_notification_show_api(msg.request.id.clone(), params.clone());
            let _ = msg.respond_to.send(response);
            return true;
        }

        match &msg.request.method {
            api::schema::Method::ClientWindowTitleSet(params) => {
                let response = self.handle_client_window_title_api(
                    msg.request.id.clone(),
                    Some(params.title.clone()),
                );
                let _ = msg.respond_to.send(response);
                return true;
            }
            api::schema::Method::ClientWindowTitleClear(_) => {
                let response = self.handle_client_window_title_api(msg.request.id.clone(), None);
                let _ = msg.respond_to.send(response);
                return true;
            }
            _ => {}
        }

        let frozen_alt_screen_read = match self.alt_screen_read_conflict(&msg.request) {
            AltScreenReadConflict::None => None,
            AltScreenReadConflict::Frozen(snapshot) => Some(snapshot),
            AltScreenReadConflict::Defer => {
                self.deferred_alt_screen_reads.push(msg);
                return false;
            }
        };

        let mut changed = api::request_changes_ui(&msg.request);
        changed |= self.drain_internal_events_with_forwarding();
        // Capture toast and effective pane states before the API call so we can
        // forward resulting client-local notifications. API requests like
        // pane.report_agent trigger handle_internal_event internally, which
        // bypasses drain_internal_events_with_forwarding. Headless mode disables
        // local sound playback, so sound notifications need to be forwarded here.
        let toast_before = self.app.state.toast.clone();
        let pane_states_before: Vec<(usize, crate::layout::PaneId, crate::detect::AgentState)> = {
            let terminals = &self.app.state.terminals;
            self.app
                .state
                .workspaces
                .iter()
                .enumerate()
                .flat_map(|(ws_idx, ws)| {
                    ws.tabs.iter().flat_map(move |tab| {
                        tab.panes.iter().filter_map(move |(&pane_id, pane)| {
                            terminals
                                .get(&pane.attached_terminal_id)
                                .map(|terminal| (ws_idx, pane_id, terminal.state))
                        })
                    })
                })
                .collect()
        };
        self.sync_foreground_client_state();
        if let Some(error) = self.agent_read_not_idle_error(&msg.request) {
            let response = serde_json::to_string(&api::schema::ErrorResponse {
                id: msg.request.id.clone(),
                error,
            })
            .unwrap_or_else(|_| "{}".to_owned());
            let _ = msg.respond_to.send(response);
            return changed;
        }
        let alt_screen_read_spec = self.alt_screen_read_spec(&msg.request);
        let disposition = if matches!(
            &msg.request.method,
            api::schema::Method::ServerReloadConfig(_)
        ) {
            let report = self.reload_server_config(true);
            api::ApiRequestDisposition::Respond(
                serde_json::to_string(&api::schema::SuccessResponse {
                    id: msg.request.id.clone(),
                    result: api::schema::ResponseResult::ConfigReload {
                        status: api::schema::config_reload_status_from_config(report.status),
                        diagnostics: report.diagnostics,
                    },
                })
                .unwrap_or_else(|err| {
                    serde_json::to_string(&api::schema::ErrorResponse {
                        id: String::new(),
                        error: api::schema::ErrorBody {
                            code: "serialization_error".into(),
                            message: err.to_string(),
                        },
                    })
                    .unwrap_or_else(|_| "{}".to_string())
                }),
            )
        } else if let Some(client_id) = self.foreground_client_id {
            if let Some(mut view_state) = self
                .clients
                .get_mut(&client_id)
                .and_then(|client| client.view_state.take())
            {
                let disposition = self
                    .app
                    .handle_api_request_disposition_for_view(&mut view_state, msg.request);
                if let Some(client) = self.clients.get_mut(&client_id) {
                    client.view_state = Some(view_state);
                }
                disposition
            } else {
                let mut view_state = self
                    .app
                    .default_client_view
                    .clone_reconciled(&self.app.state);
                let disposition = self
                    .app
                    .handle_api_request_disposition_for_view(&mut view_state, msg.request);
                self.app.default_client_view = view_state;
                disposition
            }
        } else {
            let mut view_state = self
                .app
                .default_client_view
                .clone_reconciled(&self.app.state);
            let disposition = self
                .app
                .handle_api_request_disposition_for_view(&mut view_state, msg.request);
            self.app.default_client_view = view_state;
            disposition
        };
        match disposition {
            api::ApiRequestDisposition::Respond(mut response) => {
                if let Some(snapshot) = frozen_alt_screen_read {
                    if let Ok(mut success) =
                        serde_json::from_str::<api::schema::SuccessResponse>(&response)
                    {
                        if let api::schema::ResponseResult::PaneRead { read } = &mut success.result
                        {
                            read.text = snapshot.text;
                            read.truncated = snapshot.truncated;
                            if let Ok(serialized) = serde_json::to_string(&success) {
                                response = serialized;
                            }
                        }
                    }
                }
                if let Some(spec) = alt_screen_read_spec {
                    if let Ok(success) =
                        serde_json::from_str::<api::schema::SuccessResponse>(&response)
                    {
                        if let api::schema::ResponseResult::PaneRead { read } = success.result {
                            let pending =
                                crate::server::alt_screen_read::PendingAltScreenRead::start(
                                    spec.terminal_id,
                                    success.id,
                                    msg.respond_to,
                                    response,
                                    read,
                                    spec.lines,
                                    spec.unwrap,
                                    spec.initial,
                                    spec.content_seq,
                                    Instant::now(),
                                );
                            self.pending_alt_screen_reads.push(pending);
                            return changed;
                        }
                    }
                }
                let _ = msg.respond_to.send(response);
            }
            api::ApiRequestDisposition::Deferred(deferred) => {
                let (terminal_id, pending) =
                    crate::app::PendingRemoteApiResponse::from_deferred(deferred, msg.respond_to);
                self.app
                    .store_pending_remote_api_response(terminal_id, pending);
            }
        }

        // Forward new toast state only when a client-local delivery mode is selected.
        // Gardn delivery renders the toast in-frame and must not ask clients to
        // show a terminal or system notification.
        let toast_after = self.app.state.toast.clone();
        let forwarded_toast_from_state = if self.app.state.toast_config.delay_seconds == 0
            && should_forward_toast_to_clients(self.app.state.toast_config.delivery)
            && toast_after.is_some()
            && toast_after != toast_before
        {
            if let Some(toast) = &toast_after {
                let msg_text = format!("{}: {}", toast.title, toast.context);
                debug!(msg = %msg_text, "forwarding toast notification from API request");
                self.send_to_foreground_client(ServerMessage::Notify {
                    kind: toast_notify_kind(self.app.state.toast_config.delivery)
                        .expect("toast forwarding requires a client notification kind"),
                    message: msg_text,
                });
                true
            } else {
                false
            }
        } else {
            false
        };

        // Forward notifications for effective pane state changes that occurred
        // during the API request. Hook authority is already folded into
        // pane.state, so raw hook transitions must not produce separate sounds.
        for (ws_idx, pane_id, prev_state) in &pane_states_before {
            let pane_after = self
                .app
                .state
                .workspaces
                .get(*ws_idx)
                .and_then(|ws| ws.tabs.iter().find_map(|tab| tab.panes.get(pane_id)));

            let Some(pane_after) = pane_after else {
                continue;
            };

            let Some(terminal_after) = self
                .app
                .state
                .terminals
                .get(&pane_after.attached_terminal_id)
            else {
                continue;
            };

            let new_state = terminal_after.state;
            if new_state == *prev_state {
                continue;
            }
            let is_active_tab = self.foreground_client_view_contains_pane(*pane_id);
            let suppress_active_tab_notifications =
                self.active_tab_suppresses_notifications(is_active_tab);

            let agent = terminal_after.effective_known_agent();

            debug!(
                ws_idx,
                pane_id = pane_id.raw(),
                prev_state = ?prev_state,
                new_state = ?new_state,
                agent = ?agent,
                "pane effective state changed during API request, checking notification"
            );

            if !forwarded_toast_from_state
                && self.app.state.toast_config.delay_seconds == 0
                && should_forward_toast_to_clients(self.app.state.toast_config.delivery)
            {
                if let Some(kind) = crate::app::actions::notification_toast_for_state_change(
                    suppress_active_tab_notifications,
                    *prev_state,
                    new_state,
                ) {
                    if let Some(agent_label) = self
                        .app
                        .state
                        .terminals
                        .get(&pane_after.attached_terminal_id)
                        .and_then(|terminal| terminal.effective_agent_label())
                    {
                        let event_text = match kind {
                            crate::app::state::ToastKind::NeedsAttention => "needs attention",
                            crate::app::state::ToastKind::Finished => "finished",
                            crate::app::state::ToastKind::UpdateInstalled => "updated",
                        };
                        let workspace_label = self.app.state.workspaces[*ws_idx].display_name_from(
                            &self.app.state.terminals,
                            &self.app.terminal_runtimes,
                        );
                        let msg_text = format!(
                            "{} {}: {}",
                            agent_label,
                            event_text,
                            crate::app::actions::notification_context(
                                &self.app.state.workspaces[*ws_idx],
                                &workspace_label,
                                *ws_idx,
                                *pane_id,
                            )
                        );
                        self.send_to_foreground_client(ServerMessage::Notify {
                            kind: toast_notify_kind(self.app.state.toast_config.delivery)
                                .expect("toast forwarding requires a client notification kind"),
                            message: msg_text,
                        });
                    }
                }
            }

            // Forward sound notification when server-side sound policy allows it.
            // Clients still decide locally whether they can execute the side effect.
            if self.app.state.toast_config.delay_seconds == 0 && self.app.state.sound.allows(agent)
            {
                if let Some(sound) = crate::app::actions::notification_sound_for_state_change(
                    suppress_active_tab_notifications,
                    *prev_state,
                    new_state,
                ) {
                    let msg_text = match sound {
                        crate::sound::Sound::Done => "agent done",
                        crate::sound::Sound::Request => "agent attention",
                    };
                    debug!(sound = ?sound, "forwarding sound notification from API request");
                    self.send_to_foreground_client(ServerMessage::Notify {
                        kind: protocol::NotifyKind::Sound,
                        message: msg_text.to_owned(),
                    });
                }
            }
        }

        if changed {
            if let Some(client_id) = self.foreground_client_id {
                if self
                    .clients
                    .get(&client_id)
                    .is_some_and(|client| client.view_state.is_some())
                {
                    changed |= self.reconcile_client_tab_control(client_id);
                }
            }
        }
        changed
    }
    fn apply_client_pointer_context(&mut self, client_id: u64) {
        let Some(client) = self.clients.get(&client_id) else {
            return;
        };
        self.app.state.host_sgr_pixels = client.host_sgr_pixels_active == Some(true);
        self.app.state.host_cell_size = client.cell_size;
    }

    fn stream_host_mouse_capture_mode(&mut self) {
        let default_view = self
            .app
            .default_client_view
            .clone_reconciled(&self.app.state);
        let mut updates: Vec<(u64, bool, bool)> = Vec::new();
        for (&client_id, client) in &self.clients {
            if !client.is_full_app_client() {
                continue;
            }
            let view = client
                .view_state
                .as_ref()
                .map(|view| view.clone_reconciled(&self.app.state))
                .unwrap_or_else(|| default_view.clone());
            let enabled = self
                .app
                .state
                .should_capture_host_mouse_from_view(&self.app.terminal_runtimes, &view);
            let sgr_pixels = enabled
                && self
                    .app
                    .state
                    .focused_pane_requests_sgr_pixels_from_view(&self.app.terminal_runtimes, &view);
            if client.host_mouse_capture_active == Some(enabled)
                && client.host_sgr_pixels_active == Some(sgr_pixels)
            {
                continue;
            }
            updates.push((client_id, enabled, sgr_pixels));
        }

        let mut broken_clients: Vec<u64> = Vec::new();
        for (client_id, enabled, sgr_pixels) in updates {
            let serialized = match Self::frame_server_message(&ServerMessage::MouseCapture {
                enabled,
                sgr_pixels,
            }) {
                Ok(framed) => framed,
                Err(err) => {
                    warn!(err = %err, "failed to serialize mouse capture mode for clients");
                    continue;
                }
            };
            let Some(client) = self.clients.get_mut(&client_id) else {
                continue;
            };
            let Some(writer) = &client.writer else {
                continue;
            };
            if writer.control.send(serialized).is_err() {
                debug!(
                    client_id,
                    "client writer channel closed during mouse capture update"
                );
                broken_clients.push(client_id);
                continue;
            }
            client.host_mouse_capture_active = Some(enabled);
            client.host_sgr_pixels_active = Some(sgr_pixels);
        }

        for client_id in broken_clients {
            self.remove_client(client_id);
        }
    }

    fn pty_sources_visible_to_any_render_target(
        &self,
        sources: &HashSet<crate::layout::PaneId>,
    ) -> bool {
        let mut has_app_target = false;
        let mut direct_terminal_targets = HashSet::new();
        for client in self
            .clients
            .values()
            .filter(|client| client.writer.is_some())
        {
            match &client.mode {
                ClientConnectionMode::App => {
                    if client.is_full_app_client() {
                        has_app_target = true;
                    }
                }
                ClientConnectionMode::TerminalAttach { terminal_id } => {
                    direct_terminal_targets.insert(terminal_id.as_str());
                }
            }
        }
        if !has_app_target && direct_terminal_targets.is_empty() {
            return false;
        }

        sources.iter().any(|&pane_id| {
            let terminal_id = self.terminal_id_for_pane(pane_id);
            (has_app_target && (terminal_id.is_none() || self.app_surface_contains_pane(pane_id)))
                || terminal_id
                    .is_none_or(|source| direct_terminal_targets.contains(source.as_str()))
        })
    }

    fn terminal_id_for_pane(
        &self,
        pane_id: crate::layout::PaneId,
    ) -> Option<&crate::terminal::TerminalId> {
        if let Some(popup) = self
            .app
            .state
            .popup_panes
            .values()
            .find(|popup| popup.pane_id == pane_id)
        {
            return Some(&popup.terminal_id);
        }
        self.app
            .find_pane(pane_id)
            .map(|(_, pane)| &pane.attached_terminal_id)
    }

    fn app_surface_contains_pane(&self, pane_id: crate::layout::PaneId) -> bool {
        self.any_app_client_view_contains_pane(pane_id)
    }

    /// Renders the current state to client-sized virtual buffers and streams
    /// frames to all connected clients.
    #[cfg(test)]
    fn render_and_stream(&mut self) {
        let _ = self.render_and_stream_with_pending_agent_resume(false);
    }

    fn render_and_stream_with_pending_agent_resume(
        &mut self,
        allow_empty_pending_agent_theme: bool,
    ) -> bool {
        self.prune_deleted_tab_controls();
        self.sync_all_tab_control_projections();
        let mut pending_resume_started = false;
        let render_targets = render_targets(&self.clients, self.foreground_client_id);

        if render_targets.is_empty() {
            let (cols, rows) = self.effective_size;
            let area = Rect::new(0, 0, cols, rows);
            let resize_panes = false;
            let _ = crate::server::render_stream::render_virtual_with_runtime_registry(
                &mut self.app.state,
                &self.app.terminal_runtimes,
                area,
                resize_panes,
                crate::kitty_graphics::HostCellSize::default(),
            );
            debug!(
                cols,
                rows, resize_panes, "rendered virtual frame with no attached clients"
            );
            return false;
        }

        let mut broken_clients: Vec<u64> = Vec::new();
        for (client_id, (cols, rows), cell_size, _is_foreground, mode) in render_targets {
            let area = Rect::new(0, 0, cols, rows);
            let is_app_client = matches!(mode, ClientConnectionMode::App);
            let mut frame = match mode {
                ClientConnectionMode::App => {
                    let render_cell_size =
                        if self.app.state.kitty_graphics_enabled && cell_size.is_known() {
                            cell_size
                        } else {
                            crate::kitty_graphics::HostCellSize::default()
                        };
                    let Some(client) = self.clients.get_mut(&client_id) else {
                        continue;
                    };
                    let Some(view_state) = client.view_state.as_mut() else {
                        continue;
                    };
                    let resize_controlled_tab = view_state.tab_control.can_mutate_tab();
                    let (buffer, cursor, hyperlinks) =
                        crate::server::render_stream::render_virtual_for_client_view(
                            &mut self.app.state,
                            view_state,
                            &self.app.terminal_runtimes,
                            area,
                            resize_controlled_tab,
                            render_cell_size,
                        );
                    pending_resume_started |= self.app.start_pending_agent_resumes_for_client_view(
                        view_state,
                        allow_empty_pending_agent_theme,
                    );
                    FrameData::from_ratatui_buffer_with_hyperlinks(&buffer, cursor, &hyperlinks)
                }
                ClientConnectionMode::TerminalAttach { terminal_id } => {
                    let Some(runtime) = self.runtime_for_terminal_id_string(&terminal_id) else {
                        self.send_to_client(
                            client_id,
                            ServerMessage::ServerShutdown {
                                reason: Some(format!(
                                    "terminal attach ended: terminal {terminal_id} not found"
                                )),
                            },
                        );
                        broken_clients.push(client_id);
                        continue;
                    };
                    let (buffer, cursor) =
                        crate::server::render_stream::render_terminal_virtual(runtime, area);
                    let hyperlinks = runtime.visible_hyperlinks(area);
                    FrameData::from_ratatui_buffer_with_hyperlinks(&buffer, cursor, &hyperlinks)
                }
            };

            let Some(client) = self.clients.get_mut(&client_id) else {
                continue;
            };
            let mut next_graphics_cache = client.graphics_cache.clone();
            let graphics_surface_reset_pending = client.graphics_surface_reset_pending;
            if is_app_client && self.app.state.kitty_graphics_enabled && cell_size.is_known() {
                if graphics_surface_reset_pending {
                    frame.graphics = next_graphics_cache.clear_bytes();
                }
                if let Some(view_state) = client.view_state.as_ref() {
                    frame.graphics.extend(
                        crate::kitty_graphics::encode_local_pane_graphics_for_view(
                            &self.app.state,
                            view_state,
                            &mut self.app.pane_graphics,
                            &self.app.terminal_runtimes,
                            cell_size,
                            &mut next_graphics_cache,
                        ),
                    );
                }
            } else {
                frame.graphics = next_graphics_cache.clear_bytes();
            }

            let Some(writer) = client.writer.as_ref().cloned() else {
                continue;
            };

            let mut commit_graphics_cache = true;
            if frame.graphics.len() > MAX_GRAPHICS_FRAME_SIZE {
                warn!(
                    client_id,
                    graphics_bytes = frame.graphics.len(),
                    max = MAX_GRAPHICS_FRAME_SIZE,
                    "dropping oversized graphics payload for client frame"
                );
                frame.graphics.clear();
                commit_graphics_cache = false;
            }

            let max_frame_size = if frame.graphics.is_empty() {
                MAX_FRAME_SIZE
            } else {
                MAX_GRAPHICS_FRAME_SIZE
            };
            let has_graphics = !frame.graphics.is_empty();
            let Some(mut prepared) = client.render_state.prepare_frame(frame) else {
                client.render_pending = false;
                continue;
            };
            let serialized = match Self::frame_server_message_with_max(
                prepared.message(),
                max_frame_size,
            ) {
                Ok(framed) => framed,
                Err(protocol::FramingError::Oversized { claimed, max }) if has_graphics => {
                    warn!(
                        client_id,
                        claimed, max, "dropping graphics from oversized frame for client"
                    );
                    let Some(mut text_only_frame) = prepared.into_frame() else {
                        continue;
                    };
                    text_only_frame.graphics.clear();
                    let Some(text_only_prepared) =
                        client.render_state.prepare_frame(text_only_frame)
                    else {
                        client.render_pending = false;
                        continue;
                    };
                    let framed = match Self::frame_server_message(text_only_prepared.message()) {
                        Ok(framed) => framed,
                        Err(err) => {
                            warn!(client_id, err = %err, "failed to serialize text-only frame for client");
                            broken_clients.push(client_id);
                            continue;
                        }
                    };
                    prepared = text_only_prepared;
                    commit_graphics_cache = false;
                    framed
                }
                Err(protocol::FramingError::Oversized { claimed, max }) => {
                    warn!(
                        client_id,
                        claimed, max, "skipping oversized frame for client"
                    );
                    continue;
                }
                Err(err) => {
                    warn!(client_id, err = %err, "failed to serialize frame for client");
                    broken_clients.push(client_id);
                    continue;
                }
            };

            match writer.render.try_send(serialized) {
                Ok(()) => {
                    client.render_pending = false;
                    if commit_graphics_cache {
                        client.graphics_cache = next_graphics_cache;
                        client.graphics_surface_reset_pending = false;
                    }
                    client.render_state.commit_sent_frame(prepared);
                }
                Err(std::sync::mpsc::TrySendError::Full(_)) => {
                    client.render_pending = true;
                    debug!(client_id, "render queue full, deferring latest frame");
                    continue;
                }
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    debug!(client_id, "client writer channel closed, marking as broken");
                    broken_clients.push(client_id);
                    continue;
                }
            }
        }

        for client_id in broken_clients {
            self.remove_client(client_id);
        }

        let (cols, rows) = self.effective_size;
        debug!(cols, rows, foreground_client_id = ?self.foreground_client_id, "rendered virtual frame(s)");
        pending_resume_started
    }

    /// Handle scheduled tasks for the headless server.
    ///
    /// Similar to `App::handle_scheduled_tasks` but without resize polling
    /// (the server doesn't have a terminal to resize).
    fn handle_scheduled_tasks_headless(&mut self, now: Instant, geometry_dirty: bool) -> bool {
        let mut changed = false;

        self.sync_animation_timer(now);
        self.app.flush_due_pane_mouse_motion(now);

        // No resize polling needed — server has no terminal.
        // Client resize messages drive size changes instead.

        if now >= self.app.next_port_scan {
            // ENG-187: ports are unused in the UI. Keep the timer so a later
            // surface can observe in the background instead of this loop.
            self.app.next_port_scan = now + app::PORT_SCAN_INTERVAL;
        }

        if now >= self.app.next_command_scan {
            changed |= self.app.state.refresh_command_catalog_with_hosts(
                &self.app.terminal_runtimes,
                self.app.execution_hosts.as_mut(),
            );
            changed |= self
                .app
                .state
                .refresh_command_run_statuses(&self.app.terminal_runtimes);
            self.app.next_command_scan = now + app::COMMAND_SCAN_INTERVAL;
        }

        if self
            .app
            .config_diagnostic_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            self.app.config_diagnostic_deadline = None;
            self.app.state.config_diagnostic = None;
            changed = true;
        }

        if self
            .app
            .toast_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            self.app.toast_deadline = None;
            self.app.state.toast = None;
            changed = true;
        }

        if self
            .app
            .copy_feedback_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            self.app.copy_feedback_deadline = None;
            self.app.state.copy_feedback = None;
            changed = true;
        }

        if self
            .app
            .state
            .next_pending_agent_notification_deadline()
            .is_some_and(|deadline| now >= deadline)
        {
            let deliveries = self.app.state.drain_due_agent_notifications(now);
            for delivery in &deliveries {
                self.forward_agent_notification_delivery(delivery);
            }
            if !deliveries.is_empty() {
                self.app.sync_toast_deadline(None);
                changed = true;
            }
        }

        if self
            .app
            .next_animation_tick
            .is_some_and(|deadline| now >= deadline)
        {
            self.app.state.spinner_tick = self
                .app
                .state
                .spinner_tick
                .wrapping_add(app::ANIMATION_TICK_STEP);
            self.app.next_animation_tick = Some(now + app::ANIMATION_INTERVAL);
            changed = true;
        }

        if self
            .app
            .selection_autoscroll_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            self.app.tick_selection_autoscroll(now);
            changed = true;
        }

        changed |= self.app.clear_due_selection_highlight(now);
        for client in self.clients.values_mut() {
            if let Some(view) = client.view_state.as_mut() {
                changed |= view.clear_due_selection_highlight(now);
            }
        }

        if self.has_app_client() {
            self.app.start_git_status_refresh_if_due(now);
        }

        if self
            .app
            .next_auto_update_check
            .is_some_and(|deadline| now >= deadline)
        {
            self.app.run_auto_update_check();
        }

        if self
            .app
            .next_agent_manifest_update_check
            .is_some_and(|deadline| now >= deadline)
        {
            self.app.run_agent_manifest_update_check();
        }

        if self
            .app
            .session_save_deadline
            .is_some_and(|deadline| now >= deadline)
        {
            self.app.start_background_session_save();
        }

        if let Some(deadline) = self
            .app
            .agent_metadata_deadline
            .filter(|deadline| now >= *deadline)
        {
            let previous_toast = self.app.state.toast.clone();
            for update in self.app.state.expire_agent_metadata_at(deadline, now) {
                self.app
                    .refresh_new_gardn_toast_context_for_update(&update, &previous_toast);
                self.app.emit_pane_state_update(&update);
            }
            self.app.sync_agent_metadata_deadline();
            changed = true;
        }

        if geometry_dirty {
            self.app.pending_agent_resume_deadline = None;
        } else {
            self.app.sync_pending_agent_resume_deadline(now);
            changed |= self
                .app
                .start_pending_agent_resumes(self.app.pending_agent_resume_due(now));
        }

        self.sync_animation_timer(now);
        changed
    }

    fn sync_animation_timer(&mut self, now: Instant) {
        let has_app_client = self.has_app_client();
        let client_view_has_animation = has_app_client
            && self.clients.values().any(|client| {
                client
                    .view_state
                    .as_ref()
                    .and_then(|view| view.settings.connection_editor.as_ref())
                    .is_some_and(crate::app::state::ConnectionEditorState::retirement_in_progress)
            });
        self.app
            .sync_headless_animation_timer(now, has_app_client, client_view_has_animation);
    }

    /// Initiates graceful shutdown.
    fn initiate_shutdown(&mut self) {
        if self.shutting_down {
            return;
        }
        info!("server shutdown initiated");
        self.shutting_down = true;

        // Clear client-local host graphics, then send ServerShutdown to all connected clients.
        self.send_all_clients_graphics_cleanup();
        let shutdown_msg = ServerMessage::ServerShutdown {
            reason: Some("server is shutting down".to_owned()),
        };
        self.send_to_all_clients(shutdown_msg);

        // Give client writer threads a moment to flush the shutdown message.
        // A short sleep ensures the message is written to the socket before
        // we close the connections.
        std::thread::sleep(Duration::from_millis(50));

        // Signal the main loop to exit.
        self.should_quit.store(true, Ordering::Release);
        self.app.state.should_quit = true;
    }

    /// Completes the shutdown sequence: send ServerShutdown to clients,
    /// close client connections, remove socket files, and clean up.
    fn complete_shutdown(&mut self) -> io::Result<()> {
        info!("completing server shutdown");

        // Send ServerShutdown to all remaining clients.
        if !self.clients.is_empty() {
            self.send_all_clients_graphics_cleanup();
            let shutdown_msg = ServerMessage::ServerShutdown {
                reason: Some("server is shutting down".to_owned()),
            };
            self.send_to_all_clients(shutdown_msg);

            // Give writer threads a moment to flush before closing.
            std::thread::sleep(Duration::from_millis(50));
        }

        // Reject only the requests already queued when shutdown reached cleanup.
        self.reject_queued_api_requests_for_shutdown();

        // Close all client connections.
        let staged_files = self
            .clients
            .drain()
            .flat_map(|(_, client)| client.staged_clipboard_files)
            .collect::<Vec<_>>();
        self.remove_staged_clipboard_files(staged_files);

        // Remove socket files.
        self.cleanup_sockets()?;

        Ok(())
    }

    /// Removes socket files created by the server.
    fn cleanup_sockets(&self) -> io::Result<()> {
        if let Err(err) =
            remove_socket_file_if_owned(&self.client_socket_path, &self.client_socket_identity)
        {
            if err.kind() != io::ErrorKind::NotFound {
                warn!(
                    path = %self.client_socket_path.display(),
                    err = %err,
                    "failed to remove client socket on shutdown"
                );
            }
        }
        Ok(())
    }
}

fn events_are_render_neutral_mouse_motion(
    events: &[crate::raw_input::RawInputEvent],
    mode: crate::app::Mode,
) -> bool {
    !events.is_empty()
        && events.iter().all(|event| {
            matches!(
                event,
                crate::raw_input::RawInputEvent::Mouse(mouse)
                    if matches!(mouse.kind, crossterm::event::MouseEventKind::Moved)
            )
        })
        && !mode.mouse_motion_changes_view()
}

fn events_for_app_routing(
    events: Vec<crate::raw_input::RawInputEvent>,
    mut source_is_foreground: bool,
    source_is_full_app: bool,
) -> Vec<crate::raw_input::RawInputEvent> {
    events
        .into_iter()
        .filter_map(|event| match event {
            crate::raw_input::RawInputEvent::OuterFocusGained
            | crate::raw_input::RawInputEvent::OuterFocusLost
                if !source_is_full_app =>
            {
                None
            }
            crate::raw_input::RawInputEvent::OuterFocusGained => {
                source_is_foreground = true;
                Some(event)
            }
            crate::raw_input::RawInputEvent::OuterFocusLost if !source_is_foreground => None,
            crate::raw_input::RawInputEvent::Key(_)
            | crate::raw_input::RawInputEvent::Mouse(_)
            | crate::raw_input::RawInputEvent::Paste(_)
            | crate::raw_input::RawInputEvent::TextCommit(_) => {
                source_is_foreground = true;
                Some(event)
            }
            _ => Some(event),
        })
        .collect()
}

impl Drop for HeadlessServer {
    fn drop(&mut self) {
        let staged_files = self
            .clients
            .drain()
            .flat_map(|(_, client)| client.staged_clipboard_files)
            .collect::<Vec<_>>();
        self.remove_staged_clipboard_files(staged_files);
        let _ = self.cleanup_sockets();
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Installs a Ctrl+C handler that sets the should_quit flag and wakes up
/// the event loop by sending a QuitSignal on the server event channel.
fn ctrlc_handler(should_quit: Arc<AtomicBool>, server_event_tx: mpsc::Sender<ServerEvent>) {
    let _ = ctrlc::set_handler(move || {
        should_quit.store(true, Ordering::Release);
        // Wake up the event loop so the quit flag is checked promptly.
        let _ = server_event_tx.try_send(ServerEvent::QuitSignal);
    });
}

/// Sleep until a deadline, or return pending if none.
async fn sleep_until_or_pending(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)).await,
        None => std::future::pending().await,
    }
}

fn server_config_diagnostic_summaries(diagnostics: &[String]) -> (Option<String>, Option<String>) {
    let without_keybindings = diagnostics
        .iter()
        .filter(|diagnostic| !is_keybinding_config_diagnostic(diagnostic))
        .cloned()
        .collect::<Vec<_>>();
    (
        config::config_diagnostic_summary(diagnostics),
        config::config_diagnostic_summary(&without_keybindings),
    )
}

fn is_keybinding_config_diagnostic(diagnostic: &str) -> bool {
    diagnostic.contains("keybinding") || diagnostic.contains("keys.")
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Run the headless server. This is the entry point called from main.rs.
pub fn run_server() -> io::Result<()> {
    init_logging();
    crate::platform::raise_server_nofile_limit();

    let args: Vec<String> = std::env::args().collect();
    if args.get(2).map(String::as_str) == Some("--handoff-import") {
        let socket_path = args
            .get(3)
            .map(PathBuf::from)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing handoff socket"))?;
        let token = args
            .get(4)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing handoff token"))?;
        return run_handoff_import_server(&socket_path, token);
    }

    let loaded_config = config::Config::load();
    let (api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
    let event_hub = api::EventHub::default();
    let should_quit = Arc::new(AtomicBool::new(false));

    // Start the JSON API socket server.
    let _api_server = match api::start_server_with_stop_control(
        api_tx.clone(),
        event_hub.clone(),
        should_quit.clone(),
    ) {
        Ok(server) => server,
        Err(err) if err.kind() == io::ErrorKind::AddrInUse => {
            eprintln!("error: Gardn server is already running");
            eprintln!("api socket: {}", api::socket_path().display());
            std::process::exit(1);
        }
        Err(err) => return Err(err),
    };

    let no_session = false; // Server always does session persistence.

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(io::Error::other)?;

    let result = rt.block_on(async {
        let mut app = app::App::new(
            &loaded_config.config,
            no_session,
            (!loaded_config.diagnostics.is_empty()).then(|| loaded_config.diagnostics.clone()),
            api_rx,
            event_hub,
        );

        app.state.local_sound_playback = false;
        app.local_terminal_notifications = false;
        app.local_input_source_switch = false;

        let mut server = match HeadlessServer::new(
            app,
            &loaded_config.diagnostics,
            Some(api_tx.clone()),
            Some(_api_server),
            should_quit,
        ) {
            Ok(server) => server,
            Err(err) if err.kind() == io::ErrorKind::AddrInUse => {
                eprintln!("error: Gardn server is already running");
                eprintln!("client socket: {}", client_socket_path().display());
                std::process::exit(1);
            }
            Err(err) => return Err(err),
        };

        info!(
            api_socket = %api::socket_path().display(),
            client_socket = %client_socket_path().display(),
            "Gardn server started"
        );
        print_ready_message(&api::socket_path(), &client_socket_path());
        server.app.run_plugin_startup_hooks();

        server.run().await
    });

    rt.shutdown_timeout(Duration::from_millis(100));
    crate::logging::shutdown("server");
    result
}

#[cfg(unix)]
fn run_handoff_import_server(socket_path: &Path, token: &str) -> io::Result<()> {
    let loaded_config = config::Config::load();
    let mut received = crate::server::handoff::receive(socket_path, token)?;
    crate::server::handoff::log_import_result(received.manifest.panes.len());

    let (api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
    let event_hub = api::EventHub::default();
    let should_quit = Arc::new(AtomicBool::new(false));

    let mut imports = HashMap::new();
    for (pane, fd) in received.manifest.panes.into_iter().zip(received.fds) {
        let pane_id = pane.pane_id;
        imports.insert(
            pane_id,
            crate::handoff_runtime::ImportedHandoffRuntime {
                master_fd: fd,
                state: pane,
            },
        );
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(io::Error::other)?;

    let result = rt.block_on(async {
        let mut app = app::App::new_from_handoff(
            &loaded_config.config,
            (!loaded_config.diagnostics.is_empty()).then(|| loaded_config.diagnostics.clone()),
            api_rx,
            event_hub.clone(),
            &received.manifest.snapshot,
            &mut imports,
        )?;
        app.state.local_sound_playback = false;
        app.local_terminal_notifications = false;
        app.local_input_source_switch = false;
        crate::server::handoff::report_restored(&mut received.stream)?;
        if std::env::var("GARDN_TEST_HANDOFF_IMPORT_FAIL").as_deref() == Ok("after_restored") {
            return Err(io::Error::other(
                "test handoff import failure after restored",
            ));
        }
        wait_for_old_public_sockets_to_close(Duration::from_secs(5))?;

        let api_server = api::start_server_with_stop_control(
            api_tx.clone(),
            event_hub.clone(),
            should_quit.clone(),
        )?;
        let mut server = HeadlessServer::new(
            app,
            &loaded_config.diagnostics,
            Some(api_tx.clone()),
            Some(api_server),
            should_quit,
        )?;

        // Carried across before any client attaches, so the first title sent
        // is the override rather than the configured one it replaced.
        server.api_window_title = received.manifest.api_window_title.take();
        crate::server::handoff::report_ready(&mut received.stream)?;
        crate::server::handoff::wait_committed(&mut received.stream)?;
        server.app.assume_handoff_ownership();
        server.app.unpause_handoff_readers();
        server.pending_handoff_repaint_nudge = true;
        if let Err(err) = crate::server::handoff::report_owned(&mut received.stream) {
            warn!(err = %err, "failed to report handoff ownership; continuing as owner");
        }
        info!("handoff import server started");
        print_ready_message(&api::socket_path(), &client_socket_path());
        server.app.run_plugin_startup_hooks();
        server.run().await
    });

    rt.shutdown_timeout(Duration::from_millis(100));
    crate::logging::shutdown("server");
    result
}

#[cfg(unix)]
fn wait_for_old_public_sockets_to_close(timeout: Duration) -> io::Result<()> {
    let deadline = Instant::now() + timeout;
    let api_socket = api::socket_path();
    let client_socket = client_socket_path();
    while Instant::now() < deadline {
        let api_open = api_socket.exists() && crate::ipc::connect_local_stream(&api_socket).is_ok();
        let client_open =
            client_socket.exists() && crate::ipc::connect_local_stream(&client_socket).is_ok();
        if !api_open && !client_open {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "old server sockets did not close before handoff import bind",
    ))
}

#[cfg(not(unix))]
fn run_handoff_import_server(_socket_path: &Path, _token: &str) -> io::Result<()> {
    Err(io::Error::other("live handoff is only supported on Unix"))
}

fn print_ready_message(api_socket: &Path, client_socket: &Path) {
    eprintln!("Gardn server running; you can use any `gardn` CLI command in another terminal.");
    eprintln!("api socket: {}", api_socket.display());
    eprintln!("client socket: {}", client_socket.display());
    eprintln!(
        "logs: {}",
        crate::session::data_dir()
            .join("gardn-server.log")
            .display()
    );
    eprintln!("did you mean to open the Gardn TUI? run `gardn`; you do not need `gardn server`.");
}

/// Initialize logging for the server process.
fn init_logging() {
    crate::logging::init_file_logging("gardn-server.log");
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{FrameData, RenderEncoding, ServerMessage};
    use crate::server::clients::{ClientConnection, ClientConnectionMode};
    use std::time::Duration;

    use crate::app::AppState;
    use crate::protocol::CursorState;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SOCKET_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_headless_server() -> HeadlessServer {
        let config = crate::config::Config::default();
        let (api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = crate::app::App::new(&config, true, None, api_rx, api::EventHub::default());

        app.state.local_sound_playback = false;
        app.state.toast_config.delay_seconds = 0;
        app.local_terminal_notifications = false;
        app.local_input_source_switch = false;

        let dir = std::env::temp_dir().join(format!(
            "gardn-headless-{}-{}-{}",
            std::process::id(),
            TEST_SOCKET_COUNTER.fetch_add(1, Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::create_dir_all(&dir);
        let socket_path = dir.join("client.sock");
        let _ = fs::remove_file(&socket_path);
        let listener = bind_local_listener(&socket_path).expect("bind test listener");
        let client_socket_identity =
            socket_file_identity(&socket_path).expect("test listener socket identity");
        listener
            .set_nonblocking(ListenerNonblockingMode::Accept)
            .expect("set listener nonblocking");
        let (server_event_tx, server_event_rx) = mpsc::channel(64);
        let server_keybindings = app_keybindings(&app);

        HeadlessServer {
            app,
            api_tx: Some(api_tx),

            api_server: None,
            client_listener: listener,
            client_socket_path: socket_path,
            client_tab_keys: HashMap::new(),
            client_socket_identity,
            clients: HashMap::new(),
            next_client_id: 1,
            pending_clipboard_image_stages: HashMap::new(),
            foreground_client_id: None,
            sent_window_title: None,
            api_window_title: None,
            server_keybindings,
            server_config_diagnostic: None,
            server_config_diagnostic_without_keybindings: None,
            terminal_attach_owners: HashMap::new(),
            pending_alt_screen_reads: Vec::new(),
            deferred_alt_screen_reads: Vec::new(),
            tab_controls: TabControlCoordinator::new(),
            next_activity_stamp: 1,
            effective_size: (MIN_COLS, MIN_ROWS),
            shutting_down: false,
            handoff_in_progress: false,
            pending_handoff_repaint_nudge: false,
            should_quit: Arc::new(AtomicBool::new(false)),
            server_event_rx,
            server_event_tx,
        }
    }

    #[tokio::test]
    async fn headless_loop_applies_managed_terminal_theme_before_render() {
        let mut server = test_headless_server();
        server.app.state.workspaces = vec![crate::workspace::Workspace::test_new("test")];
        server.app.state.ensure_test_terminals();
        let pane_id = server.app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = server.app.state.workspaces[0]
            .terminal_id(pane_id)
            .expect("workspace terminal")
            .clone();
        server
            .app
            .state
            .terminals
            .get_mut(&terminal_id)
            .expect("terminal state")
            .terminal_theme_binding =
            Some(crate::terminal_theme::TerminalThemeBinding::workspace_palette());
        let render_background = |runtime: &crate::terminal::TerminalRuntime| {
            let backend = ratatui::backend::TestBackend::new(80, 24);
            let mut terminal = ratatui::Terminal::new(backend).expect("test terminal");
            terminal
                .draw(|frame| {
                    runtime.render_with_theme_background(
                        frame,
                        Rect::new(0, 0, 80, 24),
                        false,
                        None,
                    );
                })
                .expect("render managed terminal");
            terminal.backend().buffer()[(0, 0)].style().bg
        };
        let (runtime, _input_rx) =
            crate::terminal::TerminalRuntime::test_with_channel_and_screen_bytes(80, 24, b"X");
        server
            .app
            .terminal_runtimes
            .insert(terminal_id.clone(), runtime);
        server.app.state.palette = crate::app::state::Palette::dracula();
        server.app.state.global_palette = server.app.state.palette.clone();
        server.app.state.theme_name = "dracula".to_string();
        server.app.state.global_theme_name = "dracula".to_string();
        server.app.state.global_theme_mode = crate::config::ThemeMode::Dark;
        server.app.state.effective_theme_appearance = crate::terminal_theme::ThemeAppearance::Dark;
        assert!(server.app.reconcile_terminal_themes());
        let previous_background = render_background(
            server
                .app
                .terminal_runtimes
                .get(&terminal_id)
                .expect("terminal runtime"),
        );

        server.app.state.palette = crate::app::state::Palette::gruvbox_light();
        server.app.state.global_palette = server.app.state.palette.clone();
        server.app.state.theme_name = "gruvbox-light".to_string();
        server.app.state.global_theme_name = "gruvbox-light".to_string();
        server.app.state.global_theme_mode = crate::config::ThemeMode::Light;
        server.app.state.effective_theme_appearance = crate::terminal_theme::ThemeAppearance::Light;
        let expected = server
            .app
            .state
            .managed_terminal_theme_targets()
            .into_iter()
            .find(|target| target.terminal_id == terminal_id)
            .and_then(|target| target.resolved_override)
            .expect("managed terminal theme");
        let expected_background = Some(ratatui::style::Color::Rgb(
            expected.background.r,
            expected.background.g,
            expected.background.b,
        ));
        assert_ne!(previous_background, expected_background);

        server.app.render_notify.notified().await;
        let render_notify = server.app.render_notify.clone();
        let should_quit = server.should_quit.clone();
        let server_event_tx = server.server_event_tx.clone();
        let shutdown = tokio::spawn(async move {
            render_notify.notified().await;
            should_quit.store(true, Ordering::Release);
            server_event_tx
                .send(ServerEvent::QuitSignal)
                .await
                .expect("wake server for shutdown");
        });
        tokio::task::yield_now().await;
        tokio::time::timeout(Duration::from_secs(1), server.run())
            .await
            .expect("headless server reconciled before timeout")
            .expect("run headless server");
        shutdown.await.expect("join shutdown task");

        let runtime = server
            .app
            .terminal_runtimes
            .get(&terminal_id)
            .expect("terminal runtime");
        assert_eq!(render_background(runtime), expected_background);
    }

    #[test]
    fn drain_server_events_stops_after_quit() {
        let mut server = test_headless_server();
        server.should_quit.store(true, Ordering::Release);
        server
            .server_event_tx
            .try_send(ServerEvent::QuitSignal)
            .expect("queue quit event");

        assert!(!server.drain_server_events());
        assert!(
            server.server_event_rx.try_recv().is_ok(),
            "queued events must remain undrained after quit"
        );
    }

    #[test]
    fn drain_api_requests_stops_after_quit() {
        let mut server = test_headless_server();
        let (respond_to, response_rx) = std::sync::mpsc::channel();
        server
            .api_tx
            .as_ref()
            .expect("test server keeps api sender")
            .send(api::ApiRequestMessage {
                request: api::schema::Request {
                    id: "after-quit".into(),
                    method: api::schema::Method::WorkspaceList(api::schema::EmptyParams::default()),
                },
                respond_to,
                response_written: None,
                stream_active: None,
            })
            .expect("queue api request");
        server.should_quit.store(true, Ordering::Release);

        assert!(!server.drain_api_requests_with_shutdown_check());
        assert!(
            response_rx.try_recv().is_err(),
            "new api work must not run after quit"
        );
    }

    #[test]
    fn reject_queued_api_requests_for_shutdown_only_drains_existing() {
        let mut server = test_headless_server();
        let (queued_tx, queued_rx) = std::sync::mpsc::channel();
        server
            .api_tx
            .as_ref()
            .expect("test server keeps api sender")
            .send(api::ApiRequestMessage {
                request: api::schema::Request {
                    id: "queued".into(),
                    method: api::schema::Method::WorkspaceList(api::schema::EmptyParams::default()),
                },
                respond_to: queued_tx,
                response_written: None,
                stream_active: None,
            })
            .expect("queue api request");
        server.shutting_down = true;
        server.should_quit.store(true, Ordering::Release);

        server.reject_queued_api_requests_for_shutdown();

        let response = queued_rx
            .recv_timeout(Duration::from_millis(100))
            .expect("queued request should be rejected");
        let parsed: api::schema::ErrorResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(parsed.id, "queued");
        assert_eq!(parsed.error.code, "server_unavailable");
        assert_eq!(parsed.error.message, "server is shutting down");
        assert_eq!(server.app.api_rx.len(), 0);
    }

    fn hidden_pty_visibility_test_server(
        client_sizes: &[(u16, u16)],
    ) -> (HeadlessServer, crate::layout::PaneId) {
        let mut server = test_headless_server();
        let mut workspace = crate::workspace::Workspace::test_new("test");
        let background_tab = workspace.test_add_tab(Some("background"));
        let background_pane = workspace.tabs[background_tab].root_pane;
        server.app.state.workspaces = vec![workspace];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.mode = crate::app::Mode::Terminal;

        for (index, &terminal_size) in client_sizes.iter().enumerate() {
            let client_id = index as u64 + 1;
            let (client_tx, _client_control_rx, _client_rx) = test_client_writer();
            let mut client = ClientConnection::new(
                terminal_size,
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                client_id,
                RenderEncoding::SemanticFrame,
                Some(client_tx),
            );
            client.view_state = Some(crate::app::ClientViewState::from_default_client_state(
                &server.app.state,
            ));
            server.clients.insert(client_id, client);
        }

        (server, background_pane)
    }

    fn frame_text(frame: &FrameData) -> String {
        frame
            .cells
            .iter()
            .map(|cell| cell.symbol.as_str())
            .collect()
    }

    #[test]
    fn inactive_tab_pty_source_is_hidden_until_tab_focus() {
        let (server, background_pane) = hidden_pty_visibility_test_server(&[]);
        let sources = HashSet::from([background_pane]);
        assert!(!server.pty_sources_visible_to_any_render_target(&sources));

        let (mut server, background_pane) =
            hidden_pty_visibility_test_server(&[(120, 40), (44, 20)]);
        let sources = HashSet::from([background_pane]);
        assert!(!server.pty_sources_visible_to_any_render_target(&sources));

        let workspace_id = server.app.state.workspaces[0].id.clone();
        for client in server.clients.values_mut() {
            if let Some(view) = client.view_state.as_mut() {
                view.active_tabs.insert(workspace_id.clone(), 1);
            }
        }
        assert!(server.pty_sources_visible_to_any_render_target(&sources));
    }

    #[tokio::test]
    async fn hidden_pty_output_appears_after_switching_to_its_tab() {
        let mut server = test_headless_server();
        let mut workspace = crate::workspace::Workspace::test_new("test");
        let workspace_id = workspace.id.clone();
        let background_tab = workspace.test_add_tab(Some("background"));
        let background_pane = workspace.tabs[background_tab].root_pane;
        workspace.tabs[background_tab].runtimes.insert(
            background_pane,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(80, 24, b"before"),
        );
        server.app.state.workspaces = vec![workspace];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.mode = crate::app::Mode::Terminal;

        let (client_tx, _client_control_rx, client_rx) = test_client_writer();
        let mut client = ClientConnection::new(
            (80, 24),
            crate::kitty_graphics::HostCellSize::default(),
            crate::terminal_theme::TerminalTheme::default(),
            None,
            1,
            RenderEncoding::SemanticFrame,
            Some(client_tx),
        );
        client.view_state = Some(crate::app::ClientViewState::from_default_client_state(
            &server.app.state,
        ));
        server.clients.insert(1, client);
        server.foreground_client_id = Some(1);
        server.sync_foreground_client_state();
        server.render_and_stream();
        let _initial_frame = client_rx
            .recv_timeout(Duration::from_millis(100))
            .expect("initial frame");

        let runtime = server
            .app
            .state
            .runtime_for_pane_in_workspace(&server.app.terminal_runtimes, 0, background_pane)
            .expect("background runtime");
        runtime.test_process_pty_bytes(background_pane, b"\rhidden-update");
        assert!(server.app.render_dirty.request_pty(background_pane));
        let request = server.app.render_dirty.take();
        assert!(!server.pty_sources_visible_to_any_render_target(&request.pty_sources));
        assert!(client_rx.recv_timeout(Duration::from_millis(50)).is_err());

        server.app.state.workspaces[0].switch_tab(background_tab);
        if let Some(view) = server
            .clients
            .get_mut(&1)
            .and_then(|client| client.view_state.as_mut())
        {
            view.active_tabs.insert(workspace_id, background_tab);
        }
        server.render_and_stream();
        let visible_frame = match read_server_message(
            client_rx
                .recv_timeout(Duration::from_millis(100))
                .expect("frame after tab switch"),
        ) {
            ServerMessage::Frame(frame) => frame,
            other => panic!("expected semantic frame, got {other:?}"),
        };
        assert!(frame_text(&visible_frame).contains("hidden-update"));
    }

    #[test]
    fn direct_terminal_attach_keeps_hidden_pty_source_renderable() {
        let (mut server, background_pane) = hidden_pty_visibility_test_server(&[]);
        let terminal_id = server.app.state.workspaces[0]
            .terminal_id(background_pane)
            .expect("background terminal id")
            .to_string();
        let (client_tx, _client_control_rx, _client_rx) = test_client_writer();
        server.clients.insert(
            1,
            ClientConnection::new_with_mode(
                ClientConnectionMode::TerminalAttach { terminal_id },
                None,
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                false,
                Some(client_tx),
            ),
        );

        assert!(server.pty_sources_visible_to_any_render_target(&HashSet::from([background_pane])));
    }

    #[test]
    fn render_and_stream_sends_large_terminal_frame_for_terminal_ansi_client() {
        let mut server = test_headless_server();
        server.app.state.workspaces = vec![crate::workspace::Workspace::test_new("test")];
        server.app.state.ensure_test_terminals();
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.mode = crate::app::Mode::Terminal;
        let (client_tx, _client_control_rx, client_rx) = test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (278, 85),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::TerminalAnsi,
                Some(client_tx),
            ),
        );
        // Compact full ANSI redraws stay under the transport limit even after
        // a large geometry change. Semantic clients exercise the same encoder
        // through the client blit path; this server test keeps the large
        // follow-up frame deliverable.
        server.foreground_client_id = Some(1);
        server.sync_foreground_client_state();
        server.render_and_stream();
        match read_server_message(
            client_rx
                .recv_timeout(Duration::from_millis(100))
                .expect("initial terminal frame"),
        ) {
            ServerMessage::Terminal(frame) => {
                assert_eq!(frame.seq, 1);
                assert_eq!((frame.width, frame.height), (278, 85));
                assert!(frame.full);
            }
            other => panic!("expected terminal frame, got {other:?}"),
        }

        assert!(server.handle_server_event(ServerEvent::ClientResize {
            client_id: 1,
            cols: 710,
            rows: 202,
            cell_width_px: 0,
            cell_height_px: 0,
        }));
        server.render_and_stream();
        match read_server_message(
            client_rx
                .recv_timeout(Duration::from_millis(100))
                .expect("large terminal frame"),
        ) {
            ServerMessage::Terminal(frame) => {
                assert_eq!(frame.seq, 2);
                assert_eq!((frame.width, frame.height), (710, 202));
                assert!(frame.full);
                assert!(!frame.bytes.is_empty());
                assert!(!frame.bytes.windows(4).any(|bytes| bytes == b"\x1b[2J"));
            }
            other => panic!("expected terminal frame, got {other:?}"),
        }
    }

    fn install_focused_test_runtime(
        server: &mut HeadlessServer,
        terminal_bytes: &[u8],
    ) -> tokio::sync::mpsc::Receiver<Bytes> {
        let mut workspace = crate::workspace::Workspace::test_new("focus-reporting");
        let pane_id = workspace.tabs[0].root_pane;
        let (runtime, input_rx) =
            crate::terminal::TerminalRuntime::test_with_channel_and_scrollback_bytes(
                80,
                24,
                0,
                terminal_bytes,
                4,
            );
        workspace.insert_test_runtime(pane_id, runtime);
        server.app.state.workspaces = vec![workspace];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.mode = crate::app::Mode::Terminal;
        input_rx
    }

    fn test_app_client(outer_terminal_focus: Option<bool>, last_activity: u64) -> ClientConnection {
        ClientConnection::new(
            (80, 24),
            crate::kitty_graphics::HostCellSize::default(),
            crate::terminal_theme::TerminalTheme::default(),
            outer_terminal_focus,
            last_activity,
            RenderEncoding::SemanticFrame,
            None,
        )
    }

    #[tokio::test]
    async fn removing_client_preserves_github_tab_for_reattach() {
        let mut server = test_headless_server();
        server.app.state.workspaces = vec![crate::workspace::Workspace::test_new("workspace")];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.mode = crate::app::Mode::Terminal;
        let mut view = crate::app::ClientViewState::from_default_client_state(&server.app.state);
        server.app.open_github_for_view(&mut view);
        let workspace_id = server.app.state.workspaces[0].id.clone();
        let github_tab = server.app.state.workspaces[0]
            .tabs
            .iter()
            .position(|tab| tab.role == crate::workspace::TabRole::Github)
            .expect("GitHub tab");
        let mut client = test_app_client(None, 1);
        client.view_state = Some(view);
        server.clients.insert(1, client);

        server.remove_client(1);

        assert_eq!(server.app.state.workspaces[0].tabs.len(), 2);
        assert_eq!(
            server.app.state.workspaces[0].tabs[github_tab].role,
            crate::workspace::TabRole::Github
        );
        let mut reattached =
            crate::app::ClientViewState::from_default_client_state(&server.app.state);
        reattached.active_tabs.insert(workspace_id, github_tab);
        assert!(server.app.pump_github_for_view(&mut reattached));
        assert!(reattached.github.is_some());
    }

    #[tokio::test]
    async fn headless_server_processes_command_palette_agent_profile_request() {
        let mut server = test_headless_server();
        server.app.state.workspaces = vec![crate::workspace::Workspace::test_new("test")];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.mode = app::state::Mode::CommandPalette;
        server.app.state.agent_profiles = crate::agent_profiles::AgentProfileCatalog::from_config(
            &crate::agent_profiles::AgentProfilesConfig {
                order: vec!["user:shell-builtin".to_string()],
                custom: vec![crate::agent_profiles::UserAgentProfileConfig {
                    id: "shell-builtin".to_string(),
                    name: "shell builtin".to_string(),
                    kind: crate::agent_profiles::AgentKind::Omp,
                    command: "cd .".to_string(),
                    env: std::collections::BTreeMap::new(),
                    enabled: true,
                }],
            },
        );
        server.app.state.integration_recommendations =
            vec![crate::integration::IntegrationRecommendation {
                target: crate::api::schema::IntegrationTarget::Omp,
                label: "omp",
                command: "omp",
                available: true,
                path: std::path::PathBuf::from("/tmp/gardn-test-omp"),
                state: crate::integration::IntegrationStatusKind::Current,
            }];
        server.app.state.command_palette.query = "new agent".to_string();

        server.app.route_client_events(
            vec![crate::raw_input::RawInputEvent::Key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Enter,
                    crossterm::event::KeyModifiers::empty(),
                )
                .into(),
            )],
            true,
        );
        assert_eq!(server.app.state.mode, app::state::Mode::AgentProfilePicker);
        assert_eq!(server.app.state.agent_profile_picker.ws_idx, 0);

        server.app.route_client_events(
            vec![crate::raw_input::RawInputEvent::Key(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Enter,
                    crossterm::event::KeyModifiers::empty(),
                )
                .into(),
            )],
            true,
        );

        assert_eq!(server.app.state.workspaces[0].tabs.len(), 1);
        assert!(server.app.process_deferred_workspace_requests());
        assert_eq!(server.app.state.workspaces[0].tabs.len(), 2);
        assert!(server.app.state.request_agent_profile_tab.is_none());
    }

    fn read_server_message(bytes: Vec<u8>) -> ServerMessage {
        let mut cursor = std::io::Cursor::new(bytes);
        protocol::read_message(&mut cursor, MAX_FRAME_SIZE).expect("decode server message")
    }

    fn read_server_frame(bytes: Vec<u8>) -> FrameData {
        match read_server_message(bytes) {
            ServerMessage::Frame(frame) => frame,
            other => panic!("expected frame, got {other:?}"),
        }
    }

    #[test]
    fn headless_scheduled_tasks_defer_port_refresh() {
        let mut server = test_headless_server();
        let now = Instant::now();
        server.app.next_port_scan = now;

        server.handle_scheduled_tasks_headless(now, false);

        assert!(
            server.app.next_port_scan > now,
            "due port scan should schedule a future refresh"
        );
        assert!(
            server.app.next_port_scan <= now + app::PORT_SCAN_INTERVAL,
            "due port scan should not skip the next refresh interval"
        );
    }

    #[test]
    fn headless_handles_project_command_open_request() {
        let mut server = test_headless_server();
        server.app.state.request_open_project_command =
            Some(crate::app::state::ProjectCommandKind::Review);

        assert!(server.handle_open_project_command_request());

        assert_eq!(server.app.state.request_open_project_command, None);
        assert_eq!(
            server
                .app
                .state
                .toast
                .as_ref()
                .map(|toast| toast.title.as_str()),
            Some("Review Command Failed")
        );
        assert!(server.app.toast_deadline.is_some());
    }

    fn temp_project(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "gardn-headless-commands-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn headless_scheduled_tasks_refresh_commands() {
        let mut server = test_headless_server();
        let project = temp_project("scan");
        fs::write(
            project.join("package.json"),
            r#"{"scripts":{"dev":"astro dev"}}"#,
        )
        .unwrap();
        server.app.state.workspaces = vec![crate::workspace::Workspace::test_new("web")];
        server.app.state.ensure_test_terminals();
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        let root_pane = server.app.state.workspaces[0].tabs[0].root_pane;
        let terminal_id = server.app.state.terminal_id_for_pane(0, root_pane).unwrap();
        server
            .app
            .state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .cwd = project;
        let now = Instant::now();
        server.app.next_command_scan = now;

        assert!(server.handle_scheduled_tasks_headless(now, false));

        assert_eq!(server.app.state.command_catalog.len(), 1);
        assert_eq!(server.app.state.command_catalog[0].name, "dev");
        assert_eq!(
            server.app.next_command_scan,
            now + app::COMMAND_SCAN_INTERVAL
        );
    }

    fn read_server_shutdown_reason(bytes: Vec<u8>) -> Option<String> {
        match read_server_message(bytes) {
            ServerMessage::ServerShutdown { reason } => reason,
            other => panic!("expected shutdown, got {other:?}"),
        }
    }

    fn test_client_writer() -> (
        ClientWriter,
        std::sync::mpsc::Receiver<Vec<u8>>,
        std::sync::mpsc::Receiver<Vec<u8>>,
    ) {
        let (control_tx, control_rx) = std::sync::mpsc::channel();
        let (render_tx, render_rx) = std::sync::mpsc::sync_channel(1);
        (
            ClientWriter::test_channel(control_tx, render_tx),
            control_rx,
            render_rx,
        )
    }

    #[test]
    fn headless_working_animation_requires_an_app_client() {
        let mut server = test_headless_server();
        let workspace = crate::workspace::Workspace::test_new("working");
        let pane_id = workspace.tabs[0].root_pane;
        server.app.state.workspaces = vec![workspace];
        server.app.state.ensure_test_terminals();
        let terminal_id = server.app.state.workspaces[0].tabs[0].panes[&pane_id]
            .attached_terminal_id
            .clone();
        let terminal = server
            .app
            .state
            .terminals
            .get_mut(&terminal_id)
            .expect("pane terminal");
        terminal.detected_agent = Some(crate::detect::Agent::Codex);
        terminal.state = crate::detect::AgentState::Working;
        server.app.state.status_indicators = crate::config::StatusIndicatorStyle::Symbols;
        let now = Instant::now();

        server.sync_animation_timer(now);
        assert_eq!(server.app.next_animation_tick, None);

        let (writer, _control_rx, _render_rx) = test_client_writer();
        server.clients.insert(
            1,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                Some(writer),
            ),
        );
        server.sync_animation_timer(now);
        assert_eq!(
            server.app.next_animation_tick,
            Some(now + app::ANIMATION_INTERVAL)
        );
    }

    #[test]
    fn oversized_paste_rejection_notifies_only_the_sending_client() {
        let mut server = test_headless_server();
        let (sender_writer, sender_control_rx, _sender_render_rx) = test_client_writer();
        let (foreground_writer, foreground_control_rx, _foreground_render_rx) =
            test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (120, 40),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                Some(sender_writer),
            ),
        );
        server.clients.insert(
            2,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                2,
                RenderEncoding::SemanticFrame,
                Some(foreground_writer),
            ),
        );
        server.foreground_client_id = Some(2);
        server.sync_foreground_client_state();

        assert!(
            !server.handle_server_event(ServerEvent::ClientPasteRejected {
                client_id: 1,
                size: 5_000_012,
                max: 1_048_576,
            })
        );

        match read_server_message(
            sender_control_rx
                .recv_timeout(Duration::from_millis(100))
                .expect("sending client rejection notification"),
        ) {
            ServerMessage::Notify { kind, message } => {
                assert_eq!(kind, crate::protocol::NotifyKind::Toast);
                assert_eq!(
                    message,
                    "Paste rejected: input message is 5000012 bytes; \
                     Gardn's limit is 1048576 bytes"
                );
            }
            other => panic!("expected paste rejection notification, got {other:?}"),
        }
        assert!(
            foreground_control_rx
                .recv_timeout(Duration::from_millis(50))
                .is_err(),
            "foreground client must not receive another client's rejection"
        );
        assert_eq!(server.foreground_client_id, Some(2));
        assert_eq!(server.clients.len(), 2);
        assert!(server.app.state.toast.is_none());
    }

    #[test]
    fn each_new_client_starts_with_the_configured_sidebar_overview() {
        let mut server = test_headless_server();
        server.app.state.group_filter_enabled = true;
        server.app.state.sidebar_collapsed = true;
        server.app.state.right_sidebar_collapsed = true;
        server.app.state.agent_panel_scope = crate::app::state::AgentPanelScope::CurrentWorkspace;
        let (writer_a, _control_a, _render_a) = test_client_writer();
        let (writer_b, _control_b, _render_b) = test_client_writer();

        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 1,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::SemanticFrame,
            keybindings: None,
            direct_attach_requested: false,
            direct_graphics: false,
            writer: writer_a,
        }));
        let first = server.clients[&1].view_state.as_ref().unwrap();
        assert!(!first.sidebar_collapsed);
        assert!(!first.right_sidebar_collapsed);
        assert!(!first.group_filter_enabled);
        assert_eq!(
            first.agent_panel_scope,
            crate::app::state::AgentPanelScope::AllWorkspaces
        );

        let first = server
            .clients
            .get_mut(&1)
            .unwrap()
            .view_state
            .as_mut()
            .unwrap();
        first.sidebar_collapsed = true;
        first.group_filter_enabled = true;
        first.agent_panel_scope = crate::app::state::AgentPanelScope::CurrentWorkspace;

        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 2,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::SemanticFrame,
            keybindings: None,
            direct_attach_requested: false,
            direct_graphics: false,
            writer: writer_b,
        }));
        let second = server.clients[&2].view_state.as_ref().unwrap();
        assert!(!second.sidebar_collapsed);
        assert!(!second.right_sidebar_collapsed);
        assert!(!second.group_filter_enabled);
        assert_eq!(
            second.agent_panel_scope,
            crate::app::state::AgentPanelScope::AllWorkspaces
        );
        assert!(
            server.clients[&1]
                .view_state
                .as_ref()
                .unwrap()
                .sidebar_collapsed
        );
    }

    #[test]
    fn foreground_client_applies_client_keybindings() {
        let mut server = test_headless_server();
        let local_config: crate::config::Config = toml::from_str(
            r#"
[keys]
prefix = "ctrl+a"
new_tab = "prefix+t"
"#,
        )
        .unwrap();
        let local_keybindings = local_config.live_keybinds().unwrap();
        let (writer_a, _control_a, _render_a) = test_client_writer();
        let (writer_b, _control_b, _render_b) = test_client_writer();

        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 1,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::SemanticFrame,
            keybindings: Some(Box::new(local_keybindings)),
            direct_attach_requested: false,
            direct_graphics: false,
            writer: writer_a,
        }));
        assert_eq!(
            server.app.state.prefix_code,
            crossterm::event::KeyCode::Char('a')
        );
        assert!(server
            .app
            .state
            .keybinds
            .new_tab
            .bindings
            .iter()
            .any(|binding| binding.label == "prefix+t"));

        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 2,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::SemanticFrame,
            keybindings: None,
            direct_attach_requested: false,
            direct_graphics: false,
            writer: writer_b,
        }));
        assert_eq!(
            server.app.state.prefix_code,
            crossterm::event::KeyCode::Char('b')
        );
        assert!(server
            .app
            .state
            .keybinds
            .new_tab
            .bindings
            .iter()
            .any(|binding| binding.label == "prefix+c"));
    }

    #[test]
    fn local_keybinding_client_hides_server_keybinding_warnings() {
        let mut server = test_headless_server();
        let diagnostics = vec![
            "unsafe direct keybinding: keys.close_pane = \"x\" would intercept typing".to_owned(),
            "theme warning".to_owned(),
        ];
        let (full, without_keybindings) = server_config_diagnostic_summaries(&diagnostics);
        server.server_config_diagnostic = full.clone();
        server.server_config_diagnostic_without_keybindings = without_keybindings.clone();
        server.app.state.config_diagnostic = full;
        let local_keybindings = crate::config::Config::default().live_keybinds().unwrap();
        let (writer_a, _control_a, _render_a) = test_client_writer();
        let (writer_b, _control_b, _render_b) = test_client_writer();

        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 1,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::SemanticFrame,
            keybindings: Some(Box::new(local_keybindings)),
            direct_attach_requested: false,
            direct_graphics: false,
            writer: writer_a,
        }));
        assert_eq!(server.app.state.config_diagnostic, without_keybindings);

        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 2,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::SemanticFrame,
            keybindings: None,
            direct_attach_requested: false,
            direct_graphics: false,
            writer: writer_b,
        }));
        assert_eq!(
            server.app.state.config_diagnostic,
            server.server_config_diagnostic
        );
    }

    #[test]
    fn local_keybinding_client_keeps_local_keybindings_after_settings_save() {
        let path = std::env::temp_dir().join(format!(
            "gardn-headless-settings-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&path, "onboarding = false\n").unwrap();
        let _guard = crate::config::test_config_env_lock().lock().unwrap();
        let _config_path_env =
            crate::config::TestEnvVar::set(crate::config::CONFIG_PATH_ENV_VAR, &path);

        let mut server = test_headless_server();
        let local_config: crate::config::Config = toml::from_str(
            r#"
[keys]
prefix = "ctrl+a"
new_workspace = "prefix+n"
next_tab = ""
"#,
        )
        .unwrap();
        let local_keybindings = local_config.live_keybinds().unwrap();
        let (writer, _control, _render) = test_client_writer();
        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 1,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::SemanticFrame,
            keybindings: Some(Box::new(local_keybindings)),
            direct_attach_requested: false,
            direct_graphics: false,
            writer,
        }));
        server.app.state.mode = crate::app::Mode::Settings;
        server.app.state.settings.section = crate::app::state::SettingsSection::Toast;
        server.app.state.settings.list.selected = 0;
        server.app.state.settings.list.show();
        if let Some(client) = server.clients.get_mut(&1) {
            client.view_state = Some(crate::app::ClientViewState::from_default_client_state(
                &server.app.state,
            ));
        }

        assert!(server.handle_server_event(ServerEvent::ClientInput {
            client_id: 1,
            data: b" ".to_vec(),
        }));
        assert!(server.handle_server_event(ServerEvent::ClientInput {
            client_id: 1,
            data: b"\r".to_vec(),
        }));

        assert_eq!(
            server.app.state.prefix_code,
            crossterm::event::KeyCode::Char('a')
        );
        assert!(server
            .app
            .state
            .keybinds
            .new_workspace
            .bindings
            .iter()
            .any(|binding| binding.label == "prefix+n"));
        assert!(server.app.state.toast.is_none());
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("delivery = \"gardn\""));

        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn config_reload_updates_server_owned_headless_size() {
        let path = std::env::temp_dir().join(format!(
            "gardn-headless-size-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(
            &path,
            "onboarding = false\n[server]\nheadless_cols = 160\nheadless_rows = 50\n",
        )
        .unwrap();
        let _guard = crate::config::test_config_env_lock().lock().unwrap();
        let _config_path_env =
            crate::config::TestEnvVar::set(crate::config::CONFIG_PATH_ENV_VAR, &path);
        let mut server = test_headless_server();
        let (writer, _control, _render) = test_client_writer();
        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 1,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::SemanticFrame,
            keybindings: None,
            direct_attach_requested: false,
            direct_graphics: false,
            writer,
        }));

        server.reload_server_config(false);
        assert_eq!(server.app.state.headless_size, (160, 50));
        assert_eq!(server.effective_size, (80, 24));

        assert!(server.handle_server_event(ServerEvent::ClientDisconnected { client_id: 1 }));
        assert_eq!(server.effective_size, (160, 50));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn invalid_server_keybindings_do_not_cache_local_keybindings_after_settings_save() {
        let path = std::env::temp_dir().join(format!(
            "gardn-headless-invalid-settings-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(
            &path,
            "onboarding = false\n[keys]\nnew_workspace = \"x\"\n[ui.toast]\ndelivery = \"off\"\n",
        )
        .unwrap();
        let _guard = crate::config::test_config_env_lock().lock().unwrap();
        let _config_path_env =
            crate::config::TestEnvVar::set(crate::config::CONFIG_PATH_ENV_VAR, &path);

        let mut server = test_headless_server();
        let previous_server_config: crate::config::Config =
            toml::from_str("[keys]\nprefix = \"ctrl+c\"\nnew_workspace = \"prefix+m\"\n").unwrap();
        server.server_keybindings = previous_server_config.live_keybinds().unwrap();
        let local_config: crate::config::Config = toml::from_str(
            r#"
[keys]
prefix = "ctrl+a"
new_workspace = "prefix+n"
next_tab = ""
"#,
        )
        .unwrap();
        let (writer_a, _control_a, _render_a) = test_client_writer();
        let (writer_b, _control_b, _render_b) = test_client_writer();

        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 1,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::SemanticFrame,
            keybindings: Some(Box::new(local_config.live_keybinds().unwrap())),
            direct_attach_requested: false,
            direct_graphics: false,
            writer: writer_a,
        }));
        server.app.state.mode = crate::app::Mode::Settings;
        server.app.state.settings.section = crate::app::state::SettingsSection::Toast;
        server.app.state.settings.list.selected = 1;
        if let Some(client) = server.clients.get_mut(&1) {
            client.view_state = Some(crate::app::ClientViewState::from_default_client_state(
                &server.app.state,
            ));
        }

        assert!(server.handle_server_event(ServerEvent::ClientInput {
            client_id: 1,
            data: b"\r".to_vec(),
        }));

        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 2,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::SemanticFrame,
            keybindings: None,
            direct_attach_requested: false,
            direct_graphics: false,
            writer: writer_b,
        }));
        assert_eq!(
            server.app.state.prefix_code,
            crossterm::event::KeyCode::Char('c')
        );
        assert!(server
            .app
            .state
            .keybinds
            .new_workspace
            .bindings
            .iter()
            .any(|binding| binding.label == "prefix+m"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn terminal_attach_rejects_missing_terminal_and_removes_client() {
        let mut server = test_headless_server();
        let (writer, control_rx, _render_rx) = test_client_writer();

        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 7,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::TerminalAnsi,
            keybindings: None,
            direct_attach_requested: true,
            direct_graphics: false,
            writer,
        }));
        assert!(server.clients.contains_key(&7));
        assert!(server.clients.get(&7).unwrap().view_state.is_none());

        assert!(
            !server.handle_server_event(ServerEvent::ClientAttachTerminal {
                client_id: 7,
                terminal_id: "term_missing".to_owned(),
                takeover: false,
            })
        );
        assert!(!server.clients.contains_key(&7));
        let reason = read_server_shutdown_reason(control_rx.recv().expect("shutdown message"));
        assert_eq!(
            reason,
            Some("terminal attach failed: terminal term_missing not found".to_owned())
        );
    }

    #[tokio::test]
    async fn direct_semantic_terminal_attach_does_not_forward_key_releases() {
        let mut server = test_headless_server();
        let workspace = crate::workspace::Workspace::test_new("attached");
        let pane_id = workspace.tabs[0].root_pane;
        let terminal_id = workspace
            .pane_state(pane_id)
            .expect("root pane")
            .attached_terminal_id
            .clone();
        let terminal_id_string = terminal_id.to_string();
        server.app.state.workspaces = vec![workspace];
        server.app.state.ensure_test_terminals();
        let (runtime, mut input_rx) = crate::terminal::TerminalRuntime::test_with_channel(80, 24);
        server.app.terminal_runtimes.insert(terminal_id, runtime);
        let (writer, _control_rx, _render_rx) = test_client_writer();

        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 7,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::SemanticFrame,
            keybindings: None,
            direct_attach_requested: true,
            direct_graphics: false,
            writer,
        }));
        assert!(
            server.handle_server_event(ServerEvent::ClientAttachTerminal {
                client_id: 7,
                terminal_id: terminal_id_string,
                takeover: false,
            })
        );

        let events = [
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyCode::Backspace,
            crossterm::event::KeyCode::Char('a'),
        ]
        .into_iter()
        .flat_map(|code| {
            [
                crate::input::TerminalKey::new(code, crossterm::event::KeyModifiers::empty())
                    .with_kind(crossterm::event::KeyEventKind::Press),
                crate::input::TerminalKey::new(code, crossterm::event::KeyModifiers::empty())
                    .with_kind(crossterm::event::KeyEventKind::Release),
            ]
        })
        .map(crate::raw_input::RawInputEvent::Key)
        .collect();
        assert!(server.handle_server_event(ServerEvent::ClientInputEvents {
            client_id: 7,
            events,
        }));

        let chunks: Vec<Vec<u8>> = std::iter::from_fn(|| input_rx.try_recv().ok())
            .map(|bytes| bytes.to_vec())
            .collect();
        assert_eq!(
            chunks,
            vec![b"\r".to_vec(), b"\x7f".to_vec(), b"a".to_vec()]
        );
    }

    fn app_client_marks_git_refresh_due_on_first_attach(render_encoding: RenderEncoding) {
        let mut server = test_headless_server();
        server
            .app
            .state
            .workspaces
            .push(crate::workspace::Workspace::test_new("test"));
        let future = Instant::now() + Duration::from_secs(60);
        server.app.last_git_remote_status_refresh = future;
        let (writer, _control_rx, _render_rx) = test_client_writer();

        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 7,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding,
            keybindings: None,
            direct_attach_requested: false,
            direct_graphics: false,
            writer,
        }));

        assert!(server.has_app_client());
        assert!(server
            .app
            .git_refresh_deadline()
            .is_some_and(|deadline| deadline <= Instant::now()));
    }

    #[test]
    fn terminal_ansi_app_client_enables_headless_git_refresh() {
        app_client_marks_git_refresh_due_on_first_attach(RenderEncoding::TerminalAnsi);
    }

    #[test]
    fn pending_terminal_attach_client_does_not_enable_headless_git_refresh() {
        let mut server = test_headless_server();
        server
            .app
            .state
            .workspaces
            .push(crate::workspace::Workspace::test_new("test"));
        let now = Instant::now();
        server.app.next_command_scan = now + Duration::from_secs(30);
        let (writer, _control_rx, _render_rx) = test_client_writer();

        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 7,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::TerminalAnsi,
            keybindings: None,
            direct_attach_requested: true,
            direct_graphics: false,
            writer,
        }));

        assert!(!server.has_app_client());
        assert_eq!(
            server.app.next_headless_loop_deadline_with_git_refresh(
                now,
                false,
                server.has_app_client()
            ),
            Some(server.app.next_command_scan)
        );
    }

    #[test]
    fn writerless_app_client_does_not_enable_headless_git_refresh() {
        let mut server = test_headless_server();
        server
            .app
            .state
            .workspaces
            .push(crate::workspace::Workspace::test_new("test"));
        let now = Instant::now();
        server.app.next_command_scan = now + Duration::from_secs(30);
        let (writer, _control_rx, _render_rx) = test_client_writer();

        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 7,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::SemanticFrame,
            keybindings: None,
            direct_attach_requested: false,
            direct_graphics: false,
            writer,
        }));
        assert!(server.has_app_client());

        server.clients.get_mut(&7).expect("client").writer = None;

        assert!(!server.has_app_client());
        assert_eq!(
            server.app.next_headless_loop_deadline_with_git_refresh(
                now,
                false,
                server.has_app_client()
            ),
            Some(server.app.next_command_scan)
        );
    }

    #[test]
    fn semantic_app_client_marks_git_refresh_due_on_first_attach() {
        app_client_marks_git_refresh_due_on_first_attach(RenderEncoding::SemanticFrame);
    }

    #[test]
    fn terminal_attach_client_exits_when_attached_pane_dies() {
        let mut server = test_headless_server();
        let workspace = crate::workspace::Workspace::test_new("attached");
        let pane_id = workspace.tabs[0].root_pane;
        server.app.state.workspaces = vec![workspace];
        server.app.state.ensure_test_terminals();
        let terminal_id = server.app.state.workspaces[0]
            .pane_state(pane_id)
            .expect("pane")
            .attached_terminal_id
            .to_string();
        let (writer, control_rx, _render_rx) = test_client_writer();

        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 7,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::TerminalAnsi,
            keybindings: None,
            direct_attach_requested: true,
            direct_graphics: false,
            writer,
        }));
        assert!(
            server.handle_server_event(ServerEvent::ClientAttachTerminal {
                client_id: 7,
                terminal_id: terminal_id.clone(),
                takeover: false,
            })
        );
        assert_eq!(server.terminal_attach_owners.get(&terminal_id), Some(&7));

        assert!(
            server.handle_internal_event_with_forwarding(AppEvent::PaneDied {
                pane_id,
                child_pid: 0,
                exit_success: true,
                exit_code: None,
                exit_signal: None,
            })
        );

        assert!(!server.clients.contains_key(&7));
        assert!(!server.terminal_attach_owners.contains_key(&terminal_id));
        let reason = read_server_shutdown_reason(control_rx.recv().expect("shutdown message"));
        assert_eq!(reason, Some(format!("terminal {terminal_id} exited")));
    }

    #[test]
    fn terminal_control_rejects_attach_during_alt_screen_read() {
        let mut server = test_headless_server();
        let workspace = crate::workspace::Workspace::test_new("attached");
        let pane_id = workspace.tabs[0].root_pane;
        server.app.state.workspaces = vec![workspace];
        server.app.state.ensure_test_terminals();
        let terminal_id = server.app.state.workspaces[0]
            .pane_state(pane_id)
            .expect("pane")
            .attached_terminal_id
            .clone();
        let terminal_id_string = terminal_id.to_string();
        let (respond_to, _response_rx) = std::sync::mpsc::channel();
        server.pending_alt_screen_reads.push(
            crate::server::alt_screen_read::PendingAltScreenRead::start(
                terminal_id,
                "read".into(),
                respond_to,
                "fallback".into(),
                api::schema::PaneReadResult {
                    pane_id: "w1:p1".into(),
                    workspace_id: "w1".into(),
                    tab_id: "w1:t1".into(),
                    source: api::schema::ReadSource::Recent,
                    format: api::schema::ReadFormat::Text,
                    text: String::new(),
                    revision: 0,
                    truncated: false,
                },
                120,
                false,
                crate::terminal::ScreenSnapshot {
                    cols: 80,
                    rows: Vec::new(),
                },
                0,
                Instant::now(),
            ),
        );
        let (writer, control_rx, _render_rx) = test_client_writer();
        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 7,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::TerminalAnsi,
            keybindings: None,
            direct_attach_requested: true,
            direct_graphics: false,
            writer,
        }));

        assert!(
            !server.handle_server_event(ServerEvent::ClientAttachTerminal {
                client_id: 7,
                terminal_id: terminal_id_string.clone(),
                takeover: false,
            })
        );
        assert!(!server.clients.contains_key(&7));
        assert!(!server
            .terminal_attach_owners
            .contains_key(&terminal_id_string));
        let reason = read_server_shutdown_reason(control_rx.recv().expect("shutdown message"));
        assert_eq!(
            reason,
            Some(format!(
                "terminal attach failed: terminal {terminal_id_string} has a read in progress; retry"
            ))
        );
    }

    #[tokio::test]
    async fn explicit_agent_history_read_requires_idle_on_alternate_screen() {
        let mut server = test_headless_server();
        let workspace = crate::workspace::Workspace::test_new("agent");
        let pane_id = workspace.tabs[0].root_pane;
        server.app.state.workspaces = vec![workspace];
        server.app.state.ensure_test_terminals();
        let terminal_id = server.app.state.workspaces[0]
            .pane_state(pane_id)
            .expect("pane")
            .attached_terminal_id
            .clone();
        let public_pane_id = crate::workspace::public_pane_id_for_number(
            &server.app.state.workspaces[0].id,
            server.app.state.workspaces[0]
                .public_pane_number(pane_id)
                .expect("pane number"),
        );
        let terminal = server
            .app
            .state
            .terminals
            .get_mut(&terminal_id)
            .expect("terminal");
        terminal.detected_agent = Some(crate::detect::Agent::Claude);
        terminal.state = crate::detect::AgentState::Working;
        server.app.terminal_runtimes.insert(
            terminal_id,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(80, 24, b"\x1b[?1049hworking"),
        );
        let request = api::schema::Request {
            id: "read".into(),
            method: api::schema::Method::AgentRead(api::schema::AgentReadParams {
                target: public_pane_id.clone(),
                source: api::schema::ReadSource::Recent,
                lines: Some(200),
                format: api::schema::ReadFormat::Text,
                strip_ansi: true,
            }),
        };

        assert_eq!(
            server.agent_read_not_idle_error(&request),
            Some(api::schema::ErrorBody {
                code: "agent_not_idle".into(),
                message: format!(
                    "cannot read 200 lines while {public_pane_id} is working: its alternate-screen history can only be captured by scrolling while idle. Wait and retry, or use --source visible"
                ),
            })
        );

        let mut default_request = request.clone();
        let api::schema::Method::AgentRead(params) = &mut default_request.method else {
            unreachable!();
        };
        params.lines = None;
        assert_eq!(server.agent_read_not_idle_error(&default_request), None);

        let mut visible_request = request;
        let api::schema::Method::AgentRead(params) = &mut visible_request.method else {
            unreachable!();
        };
        params.source = api::schema::ReadSource::Visible;
        assert_eq!(server.agent_read_not_idle_error(&visible_request), None);
    }

    #[test]
    fn headless_scheduled_tasks_expire_agent_metadata() {
        let mut server = test_headless_server();
        let workspace = crate::workspace::Workspace::test_new("metadata");
        let pane_id = workspace.tabs[0].root_pane;
        server.app.state.workspaces = vec![workspace];
        server.app.state.ensure_test_terminals();

        assert!(
            server.handle_internal_event_with_forwarding(AppEvent::HookStateReported {
                pane_id,
                source: "gardn:pi".into(),
                agent_label: "pi".into(),
                state: crate::detect::AgentState::Working,
                message: None,
                custom_status: None,
                seq: None,
                session_ref: None,
                launch_env: Vec::new(),
            })
        );
        assert!(
            server.handle_internal_event_with_forwarding(AppEvent::HookMetadataReported {
                pane_id,
                source: "user:pi-display".into(),
                agent_label: Some("pi".into()),
                applies_to_source: Some("gardn:pi".into()),
                title: None,
                display_agent: None,
                custom_status: Some("short lived".into()),
                state_labels: HashMap::new(),
                tokens: HashMap::new(),
                clear_title: false,
                clear_display_agent: false,
                clear_custom_status: false,
                clear_state_labels: false,
                seq: None,
                ttl: Some(Duration::from_millis(1)),
            })
        );

        let deadline = server
            .app
            .agent_metadata_deadline
            .expect("metadata deadline");
        let terminal_id = server.app.state.workspaces[0]
            .pane_state(pane_id)
            .expect("pane")
            .attached_terminal_id
            .clone();
        assert_eq!(
            server
                .app
                .state
                .terminals
                .get(&terminal_id)
                .expect("terminal")
                .effective_custom_status()
                .as_deref(),
            Some("short lived")
        );

        assert!(server.handle_scheduled_tasks_headless(deadline + Duration::from_millis(1), false));

        assert_eq!(server.app.agent_metadata_deadline, None);
        assert_eq!(
            server
                .app
                .state
                .terminals
                .get(&terminal_id)
                .expect("terminal")
                .effective_custom_status(),
            None
        );
        assert!(server
            .app
            .event_hub
            .events_after(0)
            .iter()
            .any(|(_, event)| {
                event.event == crate::api::schema::EventKind::PaneAgentStatusChanged
                    && matches!(
                        &event.data,
                        crate::api::schema::EventData::PaneAgentStatusChanged {
                            custom_status,
                            ..
                        } if custom_status.is_none()
                    )
            }));
    }

    #[test]
    fn virtual_render_produces_nonempty_buffer() {
        let mut state = AppState::test_new();
        let area = Rect::new(0, 0, 80, 24);
        let (buffer, _cursor) =
            crate::server::render_stream::render_virtual(&mut state, area, true);
        assert_eq!(buffer.area.width, 80);
        assert_eq!(buffer.area.height, 24);
    }

    #[test]
    fn virtual_render_without_frame_cursor_keeps_cursor_hidden() {
        let mut state = AppState::test_new();
        let area = Rect::new(0, 0, 80, 24);
        let (_buffer, cursor) =
            crate::server::render_stream::render_virtual(&mut state, area, true);

        assert_eq!(cursor, None);
    }

    #[tokio::test]
    async fn virtual_render_preserves_explicit_frame_cursor_position() {
        let mut state = AppState::test_new();
        let mut ws = crate::workspace::Workspace::test_new("test");
        let pane_id = ws.tabs[0].root_pane;
        ws.insert_test_runtime(
            pane_id,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(20, 5, b"left"),
        );

        state.workspaces = vec![ws];
        state.active = Some(0);
        state.selected = 0;
        state.mode = crate::app::Mode::Terminal;

        let area = Rect::new(0, 0, 80, 24);
        let (_buffer, cursor) =
            crate::server::render_stream::render_virtual(&mut state, area, true);
        let pane = state
            .view
            .pane_infos
            .iter()
            .find(|info| info.id == pane_id)
            .expect("focused pane info");

        assert_eq!(
            cursor,
            Some(CursorState {
                x: pane.inner_rect.x + 4,
                y: pane.inner_rect.y,
                visible: true,
                shape: cursor.as_ref().map(|c| c.shape).unwrap_or(0),
            })
        );
    }

    #[tokio::test]
    async fn virtual_render_preserves_hidden_focused_pane_cursor_position() {
        let mut state = AppState::test_new();
        let mut ws = crate::workspace::Workspace::test_new("test");
        let pane_id = ws.tabs[0].root_pane;
        ws.insert_test_runtime(
            pane_id,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(20, 5, b"left\x1b[?25l"),
        );

        state.workspaces = vec![ws];
        state.active = Some(0);
        state.selected = 0;
        state.mode = crate::app::Mode::Terminal;

        let area = Rect::new(0, 0, 80, 24);
        let (_buffer, cursor) =
            crate::server::render_stream::render_virtual(&mut state, area, true);
        let pane = state
            .view
            .pane_infos
            .iter()
            .find(|info| info.id == pane_id)
            .expect("focused pane info");

        assert_eq!(
            cursor,
            Some(CursorState {
                x: pane.inner_rect.x + 4,
                y: pane.inner_rect.y,
                visible: false,
                shape: cursor.as_ref().map(|c| c.shape).unwrap_or(0),
            })
        );
    }

    #[tokio::test]
    async fn virtual_render_exposes_hidden_pane_cursor_when_reveal_hidden_for_cjk_ime() {
        let mut state = AppState::test_new();
        state.reveal_hidden_cursor_for_cjk_ime = true;
        let mut ws = crate::workspace::Workspace::test_new("test");
        let pane_id = ws.tabs[0].root_pane;
        ws.insert_test_runtime(
            pane_id,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(20, 5, b"left\x1b[?25l"),
        );

        state.workspaces = vec![ws];
        state.active = Some(0);
        state.selected = 0;
        state.mode = crate::app::Mode::Terminal;

        let area = Rect::new(0, 0, 80, 24);
        let (_buffer, cursor) =
            crate::server::render_stream::render_virtual(&mut state, area, true);
        let pane = state
            .view
            .pane_infos
            .iter()
            .find(|info| info.id == pane_id)
            .expect("focused pane info");

        assert_eq!(
            cursor,
            Some(CursorState {
                x: pane.inner_rect.x + 4,
                y: pane.inner_rect.y,
                visible: true,
                shape: state.cjk_ime_cursor_shape,
            })
        );
    }

    #[tokio::test]
    async fn virtual_render_keeps_cursor_hidden_when_scrolled_back_even_with_reveal_hidden_for_cjk_ime(
    ) {
        let mut state = AppState::test_new();
        state.reveal_hidden_cursor_for_cjk_ime = true;
        let mut ws = crate::workspace::Workspace::test_new("test");
        let pane_id = ws.tabs[0].root_pane;
        let mut bytes = Vec::new();
        for line in 0..80 {
            bytes.extend_from_slice(format!("line {line:02}\r\n").as_bytes());
        }
        let runtime =
            crate::terminal::TerminalRuntime::test_with_scrollback_bytes(20, 5, 4096, &bytes);
        ws.insert_test_runtime(pane_id, runtime);

        state.workspaces = vec![ws];
        state.active = Some(0);
        state.selected = 0;
        state.mode = crate::app::Mode::Terminal;

        let area = Rect::new(0, 0, 80, 24);
        let _ = crate::server::render_stream::render_virtual(&mut state, area, true);
        let terminal_runtimes = crate::terminal::TerminalRuntimeRegistry::new();
        let runtime = state
            .runtime_for_pane(&terminal_runtimes, pane_id)
            .expect("pane runtime after initial render");
        runtime.scroll_up(6);
        assert!(crate::ui::pane_is_scrolled_back(runtime));

        let (_buffer, cursor) =
            crate::server::render_stream::render_virtual(&mut state, area, true);

        assert!(
            cursor.as_ref().is_none_or(|cursor| !cursor.visible),
            "scrolled-back focused pane should keep the cursor hidden even when reveal_hidden_cursor_for_cjk_ime is true; got {cursor:?}",
        );
    }

    #[tokio::test]
    async fn virtual_render_fallback_cursor_when_viewport_none_and_reveal_hidden_for_cjk_ime() {
        let mut state = AppState::test_new();
        state.reveal_hidden_cursor_for_cjk_ime = true;
        let mut ws = crate::workspace::Workspace::test_new("test");
        let pane_id = ws.tabs[0].root_pane;
        // Feed only ?25l with no prior cursor movement — exercises the fallback
        // path for TUIs whose viewport has no cursor position.
        ws.insert_test_runtime(
            pane_id,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(20, 5, b"\x1b[?25l"),
        );

        state.workspaces = vec![ws];
        state.active = Some(0);
        state.selected = 0;
        state.mode = crate::app::Mode::Terminal;

        let area = Rect::new(0, 0, 80, 24);
        let (_buffer, cursor) =
            crate::server::render_stream::render_virtual(&mut state, area, true);
        let pane = state
            .view
            .pane_infos
            .iter()
            .find(|info| info.id == pane_id)
            .expect("focused pane info");

        assert_eq!(
            cursor,
            Some(CursorState {
                x: pane.inner_rect.x,
                y: pane.inner_rect.y,
                visible: true,
                shape: state.cjk_ime_cursor_shape,
            }),
            "fallback should anchor at pane top-left with the configured shape",
        );
    }

    #[tokio::test]
    async fn virtual_render_skips_reveal_when_focused_pane_has_no_detected_agent() {
        let mut state = AppState::test_new();
        state.reveal_hidden_cursor_for_cjk_ime = true;
        // Filter only Claude, but the test pane has no detected agent, so the
        // reveal must not apply.
        state.cjk_ime_agent_filter_configured = true;
        state.cjk_ime_agents = vec![crate::detect::Agent::Claude];
        let mut ws = crate::workspace::Workspace::test_new("test");
        let pane_id = ws.tabs[0].root_pane;
        ws.insert_test_runtime(
            pane_id,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(20, 5, b"left\x1b[?25l"),
        );

        state.workspaces = vec![ws];
        state.active = Some(0);
        state.selected = 0;
        state.mode = crate::app::Mode::Terminal;

        let area = Rect::new(0, 0, 80, 24);
        let (_buffer, cursor) =
            crate::server::render_stream::render_virtual(&mut state, area, true);

        assert!(
            cursor.as_ref().is_none_or(|cursor| !cursor.visible),
            "agent filter should suppress reveal when the focused pane's detected agent is not on the list; got {cursor:?}",
        );
    }

    #[tokio::test]
    async fn virtual_render_skips_reveal_when_agent_filter_has_no_valid_entries() {
        let mut state = AppState::test_new();
        state.reveal_hidden_cursor_for_cjk_ime = true;
        state.cjk_ime_agent_filter_configured = true;
        state.cjk_ime_agents = Vec::new();
        let mut ws = crate::workspace::Workspace::test_new("test");
        let pane_id = ws.tabs[0].root_pane;
        ws.insert_test_runtime(
            pane_id,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(20, 5, b"left\x1b[?25l"),
        );

        state.workspaces = vec![ws];
        state.active = Some(0);
        state.selected = 0;
        state.mode = crate::app::Mode::Terminal;

        let area = Rect::new(0, 0, 80, 24);
        let (_buffer, cursor) =
            crate::server::render_stream::render_virtual(&mut state, area, true);

        assert!(
            cursor.as_ref().is_none_or(|cursor| !cursor.visible),
            "agent filter with no valid entries should suppress reveal; got {cursor:?}",
        );
    }

    #[tokio::test]
    async fn virtual_render_omits_focused_pane_cursor_while_mobile_switcher_open() {
        let mut state = AppState::test_new();
        let mut ws = crate::workspace::Workspace::test_new("test");
        let pane_id = ws.tabs[0].root_pane;
        ws.insert_test_runtime(
            pane_id,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(20, 5, b"left"),
        );

        state.workspaces = vec![ws];
        state.active = Some(0);
        state.selected = 0;
        state.mode = crate::app::Mode::Navigate;

        let area = Rect::new(0, 0, 44, 24);
        let (_buffer, cursor) =
            crate::server::render_stream::render_virtual(&mut state, area, true);

        assert_eq!(cursor, None);
    }

    #[tokio::test]
    async fn virtual_render_hides_focused_pane_cursor_while_scrolled_back() {
        let mut state = AppState::test_new();
        let mut ws = crate::workspace::Workspace::test_new("test");
        let pane_id = ws.tabs[0].root_pane;
        let mut bytes = Vec::new();
        for line in 0..80 {
            bytes.extend_from_slice(format!("line {line:02}\r\n").as_bytes());
        }
        let runtime =
            crate::terminal::TerminalRuntime::test_with_scrollback_bytes(20, 5, 4096, &bytes);
        ws.insert_test_runtime(pane_id, runtime);

        state.workspaces = vec![ws];
        state.active = Some(0);
        state.selected = 0;
        state.mode = crate::app::Mode::Terminal;

        let area = Rect::new(0, 0, 80, 24);
        let _ = crate::server::render_stream::render_virtual(&mut state, area, true);
        let terminal_runtimes = crate::terminal::TerminalRuntimeRegistry::new();
        let runtime = state
            .runtime_for_pane(&terminal_runtimes, pane_id)
            .expect("pane runtime after initial render");
        runtime.scroll_up(6);
        assert!(crate::ui::pane_is_scrolled_back(runtime));

        let (_buffer, cursor) =
            crate::server::render_stream::render_virtual(&mut state, area, true);

        assert!(
            cursor.as_ref().is_none_or(|cursor| !cursor.visible),
            "cursor: {cursor:?}"
        );
    }

    #[test]
    fn latest_active_client_drives_shared_size_theme_and_fallback() {
        let mut server = test_headless_server();

        server.clients.insert(
            1,
            ClientConnection::new(
                (160, 45),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme {
                    foreground: Some(crate::terminal_theme::RgbColor {
                        r: 0xaa,
                        g: 0xbb,
                        b: 0xcc,
                    }),
                    background: Some(crate::terminal_theme::RgbColor {
                        r: 0x11,
                        g: 0x22,
                        b: 0x33,
                    }),
                    ..Default::default()
                },
                None,
                1,
                RenderEncoding::SemanticFrame,
                None,
            ),
        );
        server.clients.insert(
            2,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme {
                    foreground: Some(crate::terminal_theme::RgbColor {
                        r: 0x10,
                        g: 0x20,
                        b: 0x30,
                    }),
                    background: Some(crate::terminal_theme::RgbColor {
                        r: 0xdd,
                        g: 0xee,
                        b: 0xff,
                    }),
                    ..Default::default()
                },
                None,
                2,
                RenderEncoding::SemanticFrame,
                None,
            ),
        );

        assert!(server.promote_client_to_foreground(1));
        assert_eq!(server.foreground_client_id, Some(1));
        assert_eq!(server.effective_size, (160, 45));
        assert_eq!(
            server.app.state.host_terminal_theme,
            server.clients[&1].host_terminal_theme
        );

        assert!(server.promote_client_to_foreground(2));
        assert_eq!(server.foreground_client_id, Some(2));
        assert_eq!(server.effective_size, (80, 24));
        assert_eq!(
            server.app.state.host_terminal_theme,
            server.clients[&2].host_terminal_theme
        );

        assert!(server.remove_client(2));
        assert_eq!(server.foreground_client_id, Some(1));
        assert_eq!(server.effective_size, (160, 45));
        assert_eq!(
            server.app.state.host_terminal_theme,
            server.clients[&1].host_terminal_theme
        );
    }

    #[test]
    fn focus_lost_updates_client_without_promoting_foreground() {
        let mut server = test_headless_server();

        server.clients.insert(
            1,
            ClientConnection::new(
                (120, 40),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                None,
            ),
        );
        server.clients.insert(
            2,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                Some(true),
                2,
                RenderEncoding::SemanticFrame,
                None,
            ),
        );
        server.foreground_client_id = Some(2);
        server.sync_foreground_client_state();

        let changed = server.handle_server_event(ServerEvent::ClientInput {
            client_id: 1,
            data: b"\x1b[O".to_vec(),
        });

        assert!(!changed);
        assert_eq!(server.foreground_client_id, Some(2));
        assert_eq!(server.clients[&1].outer_terminal_focus, Some(false));
        assert_eq!(server.app.state.outer_terminal_focus, Some(true));
    }

    #[test]
    fn focus_gained_promotes_client_to_foreground() {
        let mut server = test_headless_server();

        server.clients.insert(
            1,
            ClientConnection::new(
                (120, 40),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                None,
            ),
        );
        server.clients.insert(
            2,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                Some(true),
                2,
                RenderEncoding::SemanticFrame,
                None,
            ),
        );
        server.foreground_client_id = Some(2);
        server.sync_foreground_client_state();

        let changed = server.handle_server_event(ServerEvent::ClientInput {
            client_id: 1,
            data: b"\x1b[I".to_vec(),
        });

        assert!(changed);
        assert_eq!(server.foreground_client_id, Some(1));
        assert_eq!(server.clients[&1].outer_terminal_focus, Some(true));
        assert_eq!(server.app.state.outer_terminal_focus, Some(true));
    }

    #[test]
    fn foreground_client_focus_event_updates_app_focus_state() {
        let mut server = test_headless_server();

        server.clients.insert(
            1,
            ClientConnection::new(
                (120, 40),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                Some(true),
                1,
                RenderEncoding::SemanticFrame,
                None,
            ),
        );
        server.foreground_client_id = Some(1);
        server.sync_foreground_client_state();

        let changed = server.handle_server_event(ServerEvent::ClientInput {
            client_id: 1,
            data: b"\x1b[O".to_vec(),
        });

        assert!(!changed);
        assert_eq!(server.clients[&1].outer_terminal_focus, Some(false));
        assert_eq!(server.app.state.outer_terminal_focus, Some(false));
    }

    #[test]
    fn render_and_stream_uses_each_client_terminal_size() {
        let mut server = test_headless_server();
        server.app.state.workspaces = vec![crate::workspace::Workspace::test_new("test")];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.mode = crate::app::Mode::Terminal;

        let (desktop_tx, _desktop_control_rx, desktop_rx) = test_client_writer();
        let (phone_tx, _phone_control_rx, phone_rx) = test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (120, 40),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                Some(desktop_tx),
            ),
        );
        server.clients.insert(
            2,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                2,
                RenderEncoding::SemanticFrame,
                Some(phone_tx),
            ),
        );
        server.clients.get_mut(&1).unwrap().view_state = Some(
            crate::app::ClientViewState::from_default_client_state(&server.app.state),
        );
        server.clients.get_mut(&2).unwrap().view_state = Some(
            crate::app::ClientViewState::from_default_client_state(&server.app.state),
        );
        server.foreground_client_id = Some(1);
        server.sync_foreground_client_state();
        assert!(server.reconcile_client_tab_control(1));

        server.render_and_stream();

        let desktop_frame = read_server_frame(desktop_rx.recv().expect("desktop frame"));
        let phone_frame = read_server_frame(phone_rx.recv().expect("phone frame"));

        assert_eq!((desktop_frame.width, desktop_frame.height), (120, 40));
        assert_eq!((phone_frame.width, phone_frame.height), (80, 24));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn render_and_stream_starts_pending_agent_resume_for_client_visible_tab() {
        let mut server = test_headless_server();
        let mut workspace = crate::workspace::Workspace::test_new("restore");
        let workspace_id = workspace.id.clone();
        let restored_tab = workspace.test_add_tab(Some("restored agent"));
        let restored_pane = workspace.tabs[restored_tab].root_pane;
        let restored_terminal = workspace.tabs[restored_tab]
            .terminal_id(restored_pane)
            .cloned()
            .expect("restored pane should have terminal");
        workspace.active_tab = 0;
        server.app.state.workspaces = vec![workspace];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.mode = crate::app::Mode::Terminal;
        server.app.state.ensure_test_terminals();
        server.app.state.host_terminal_theme = crate::terminal_theme::TerminalTheme {
            foreground: Some(crate::terminal_theme::RgbColor {
                r: 220,
                g: 220,
                b: 220,
            }),
            background: Some(crate::terminal_theme::RgbColor {
                r: 20,
                g: 20,
                b: 20,
            }),
            ..crate::terminal_theme::TerminalTheme::default()
        };
        {
            let terminal = server
                .app
                .state
                .terminals
                .get_mut(&restored_terminal)
                .expect("restored terminal should exist");
            terminal.set_agent_name("codex".into());
            terminal.pending_agent_resume_plan = Some(crate::agent_resume::AgentResumePlan {
                agent: "codex".into(),
                argv: vec!["/bin/sh".into(), "-c".into(), "sleep 5".into()],
                command_resolution: crate::agent_resume::AgentResumeCommandResolution::External,
                preserved_launch_argv: None,
                env: Vec::new(),
                dedupe_key: "gardn:codex\0codex\0Id\0client-visible-session".into(),
            });
        }

        let (client_tx, _client_control_rx, client_rx) = test_client_writer();
        let mut client = ClientConnection::new(
            (100, 30),
            crate::kitty_graphics::HostCellSize::default(),
            crate::terminal_theme::TerminalTheme::default(),
            Some(true),
            1,
            RenderEncoding::SemanticFrame,
            Some(client_tx),
        );
        let mut client_view =
            crate::app::ClientViewState::from_default_client_state(&server.app.state);
        client_view.active_tabs.insert(workspace_id, restored_tab);
        client.view_state = Some(client_view);
        server.clients.insert(1, client);
        server.foreground_client_id = Some(1);

        server.render_and_stream();
        let _ = client_rx
            .recv_timeout(Duration::from_millis(100))
            .expect("client-visible restored tab should render a frame");

        assert!(
            server
                .app
                .terminal_runtimes
                .get(&restored_terminal)
                .is_some(),
            "rendering the client-visible restored tab should start its pending native-agent runtime"
        );
        assert!(
            server
                .app
                .state
                .terminals
                .get(&restored_terminal)
                .expect("restored terminal should survive launch")
                .pending_agent_resume_plan
                .is_none(),
            "started native-agent resume should clear the pending plan"
        );
        assert_eq!(
            server.app.state.workspaces[0].active_tab, 0,
            "client-visible resume should not steal the shared app tab focus"
        );

        for (_, runtime) in server.app.terminal_runtimes.drain() {
            runtime.shutdown();
        }
    }

    #[test]
    fn accepted_app_client_gets_view_state_and_first_frame() {
        let mut server = test_headless_server();
        server.app.state.workspaces = vec![crate::workspace::Workspace::test_new("test")];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.mode = crate::app::Mode::Terminal;

        let (writer, _control_rx, render_rx) = test_client_writer();

        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 1,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::SemanticFrame,
            keybindings: None,
            direct_attach_requested: false,
            direct_graphics: false,
            writer,
        }));
        assert!(server.clients[&1].view_state.is_some());

        server.render_and_stream();

        let frame = read_server_frame(render_rx.recv().expect("first frame"));
        assert_eq!((frame.width, frame.height), (80, 24));
        assert!(
            frame
                .cells
                .iter()
                .any(|cell| !cell.symbol.trim().is_empty()),
            "first frame should contain visible UI text"
        );
    }

    #[tokio::test]
    async fn mouse_hover_in_workspace_group_does_not_project_into_empty_group_client() {
        let mut server = test_headless_server();
        let mut workspace_group = crate::app::state::Group::default_group();
        workspace_group.id = "with-space".to_string();
        workspace_group.name = "with space".to_string();
        let mut empty_group = crate::app::state::Group::default_group();
        empty_group.id = "empty".to_string();
        empty_group.name = "empty".to_string();

        let mut workspace = crate::workspace::Workspace::test_new("A_ONLY");
        workspace.group_id = workspace_group.id.clone();
        let pane_id = workspace.tabs[0].root_pane;
        workspace.insert_test_runtime(
            pane_id,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(80, 24, b"A_ONLY_MARKER"),
        );

        server.app.state.groups = vec![workspace_group, empty_group];
        server.app.state.workspaces = vec![workspace];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.active_group = 0;
        server.app.state.group_filter_enabled = true;
        server.app.state.mode = crate::app::Mode::Terminal;

        let (a_tx, _a_control_rx, a_render_rx) = test_client_writer();
        let (b_tx, _b_control_rx, b_render_rx) = test_client_writer();

        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 1,
            cols: 100,
            rows: 30,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::SemanticFrame,
            keybindings: None,
            direct_attach_requested: false,
            direct_graphics: false,
            writer: a_tx,
        }));
        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 2,
            cols: 100,
            rows: 30,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::SemanticFrame,
            keybindings: None,
            direct_attach_requested: false,
            direct_graphics: false,
            writer: b_tx,
        }));

        let mut b_view = crate::app::ClientViewState::from_default_client_state(&server.app.state);
        b_view.active_group = 1;
        b_view.group_filter_enabled = true;
        b_view.active_workspace = None;
        b_view.selected_workspace = 0;
        server.clients.get_mut(&2).unwrap().view_state = Some(b_view);
        server.foreground_client_id = Some(2);
        server.sync_foreground_client_state();

        server.render_and_stream();
        let _ = a_render_rx
            .recv_timeout(Duration::from_millis(100))
            .unwrap();
        let initial_b = read_server_frame(
            b_render_rx
                .recv_timeout(Duration::from_millis(100))
                .unwrap(),
        );
        assert!(
            !frame_text(&initial_b).contains("A_ONLY_MARKER"),
            "empty-group client should not initially show workspace group's pane"
        );

        assert!(server.handle_server_event(ServerEvent::ClientInput {
            client_id: 1,
            data: b"\x1b[<35;10;10M".to_vec(),
        }));
        server.render_and_stream();

        assert!(
            b_render_rx
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "client-local hover must not trigger a frame for an unchanged client"
        );
    }

    #[tokio::test]
    async fn controlled_tab_resize_leaves_background_tab_size_unchanged() {
        let mut server = test_headless_server();
        let mut workspace = crate::workspace::Workspace::test_new("test");
        let background_tab = workspace.test_add_tab(Some("background"));
        let active_pane = workspace.tabs[0].root_pane;
        let background_pane = workspace.tabs[background_tab].root_pane;
        workspace.tabs[0].runtimes.insert(
            active_pane,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(80, 24, b""),
        );
        workspace.tabs[background_tab].runtimes.insert(
            background_pane,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(80, 24, b""),
        );
        server.app.state.workspaces = vec![workspace];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.mode = crate::app::Mode::Terminal;

        server.clients.insert(
            1,
            ClientConnection::new(
                (120, 40),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                None,
            ),
        );
        server.clients.get_mut(&1).unwrap().view_state = Some(
            crate::app::ClientViewState::from_default_client_state(&server.app.state),
        );
        server.foreground_client_id = Some(1);
        server.sync_foreground_client_state();
        assert!(server.reconcile_client_tab_control(1));

        let terminal_area = server.clients[&1]
            .view_state
            .as_ref()
            .unwrap()
            .computed
            .terminal_area;
        let expected = (terminal_area.height, terminal_area.width.saturating_sub(1));
        assert_eq!(
            server
                .app
                .state
                .runtime_for_pane(&server.app.terminal_runtimes, active_pane)
                .unwrap()
                .current_size(),
            expected
        );
        assert_eq!(
            server
                .app
                .state
                .runtime_for_pane(&server.app.terminal_runtimes, background_pane)
                .unwrap()
                .current_size(),
            (24, 80)
        );
    }
    #[tokio::test]
    async fn tab_control_is_explicit_and_exclusive_across_clients() {
        let mut server = test_headless_server();
        let mut workspace = crate::workspace::Workspace::test_new("control");
        let pane_id = workspace.tabs[0].root_pane;
        let (runtime, mut input_rx) = crate::terminal::TerminalRuntime::test_with_channel(80, 24);
        workspace.tabs[0].runtimes.insert(pane_id, runtime);
        server.app.state.workspaces = vec![workspace];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.mode = crate::app::Mode::Terminal;

        let mut first = test_app_client(None, 1);
        first.terminal_size = (120, 40);
        first.view_state = Some(crate::app::ClientViewState::from_default_client_state(
            &server.app.state,
        ));
        let mut second = test_app_client(None, 2);
        second.terminal_size = (80, 24);
        second.view_state = Some(crate::app::ClientViewState::from_default_client_state(
            &server.app.state,
        ));
        server.clients.insert(1, first);
        server.clients.insert(2, second);

        assert!(server.reconcile_client_tab_control(1));
        assert!(matches!(
            server.clients[&1].view_state.as_ref().unwrap().tab_control,
            crate::app::ClientTabControl::Controlling { .. }
        ));
        assert!(matches!(
            server.clients[&2].view_state.as_ref().unwrap().tab_control,
            crate::app::ClientTabControl::WatchingControlled { .. }
        ));
        let first_canvas = server.clients[&1]
            .view_state
            .as_ref()
            .unwrap()
            .tab_canvas_size;
        assert_eq!(
            server.clients[&2]
                .view_state
                .as_ref()
                .unwrap()
                .tab_canvas_size,
            first_canvas
        );
        let first_size = server
            .app
            .state
            .runtime_for_pane(&server.app.terminal_runtimes, pane_id)
            .unwrap()
            .current_size();

        assert!(server.handle_server_event(ServerEvent::ClientResize {
            client_id: 2,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
        }));
        assert_eq!(
            server
                .app
                .state
                .runtime_for_pane(&server.app.terminal_runtimes, pane_id)
                .unwrap()
                .current_size(),
            first_size,
            "a watcher resize must not resize the shared PTY"
        );
        assert!(server.handle_server_event(ServerEvent::ClientInput {
            client_id: 2,
            data: b"x".to_vec(),
        }));
        assert!(
            input_rx.try_recv().is_err(),
            "a watcher must not write input to the shared PTY"
        );

        server
            .clients
            .get_mut(&2)
            .unwrap()
            .view_state
            .as_mut()
            .unwrap()
            .request_tab_control();
        assert!(server.reconcile_client_tab_control(2));
        assert!(matches!(
            server.clients[&1].view_state.as_ref().unwrap().tab_control,
            crate::app::ClientTabControl::WatchingControlled { .. }
        ));
        assert!(matches!(
            server.clients[&2].view_state.as_ref().unwrap().tab_control,
            crate::app::ClientTabControl::Controlling { .. }
        ));
        let second_canvas = server.clients[&2]
            .view_state
            .as_ref()
            .unwrap()
            .tab_canvas_size;
        assert_ne!(second_canvas, first_canvas);
        assert_eq!(
            server.clients[&1]
                .view_state
                .as_ref()
                .unwrap()
                .tab_canvas_size,
            second_canvas
        );
        let second_size = server
            .app
            .state
            .runtime_for_pane(&server.app.terminal_runtimes, pane_id)
            .unwrap()
            .current_size();
        assert_ne!(second_size, first_size);

        assert!(server.handle_server_event(ServerEvent::ClientInput {
            client_id: 2,
            data: b"x".to_vec(),
        }));
        assert_eq!(input_rx.recv().await.unwrap(), Bytes::from_static(b"x"));

        server.remove_client(2);
        assert!(matches!(
            server.clients[&1].view_state.as_ref().unwrap().tab_control,
            crate::app::ClientTabControl::WatchingFree { .. }
        ));
        assert!(
            !server.reconcile_client_tab_control(1),
            "a watcher must not be promoted merely because the tab became free"
        );
        assert!(matches!(
            server.clients[&1].view_state.as_ref().unwrap().tab_control,
            crate::app::ClientTabControl::WatchingFree { .. }
        ));
        server
            .clients
            .get_mut(&1)
            .unwrap()
            .view_state
            .as_mut()
            .unwrap()
            .request_tab_control();
        assert!(server.reconcile_client_tab_control(1));
        assert!(matches!(
            server.clients[&1].view_state.as_ref().unwrap().tab_control,
            crate::app::ClientTabControl::Controlling { .. }
        ));
        assert_eq!(
            server
                .app
                .state
                .runtime_for_pane(&server.app.terminal_runtimes, pane_id)
                .unwrap()
                .current_size(),
            first_size
        );
    }

    #[test]
    fn api_tab_focus_does_not_paint_take_control_on_the_foreground_client() {
        let mut server = test_headless_server();
        let mut workspace = crate::workspace::Workspace::test_new("control");
        workspace.test_add_tab(Some("logs"));
        server.app.state.workspaces = vec![workspace];
        server.app.state.ensure_test_terminals();
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.mode = crate::app::Mode::Terminal;

        let mut client = test_app_client(Some(true), 1);
        client.view_state = Some(crate::app::ClientViewState::from_default_client_state(
            &server.app.state,
        ));
        server.clients.insert(1, client);
        server.foreground_client_id = Some(1);
        assert!(server.reconcile_client_tab_control(1));
        assert!(matches!(
            server.clients[&1].view_state.as_ref().unwrap().tab_control,
            crate::app::ClientTabControl::Controlling { .. }
        ));

        let workspace_id = server.app.state.workspaces[0].id.clone();
        let tab_number = server.app.state.workspaces[0].tabs[1].number;
        let tab_id = crate::workspace::public_tab_id_for_number(&workspace_id, tab_number);
        let (respond_to, response_rx) = std::sync::mpsc::channel();
        assert!(
            server.handle_api_request_with_shutdown_check(api::ApiRequestMessage {
                request: api::schema::Request {
                    id: "focus".into(),
                    method: api::schema::Method::TabFocus(api::schema::TabTarget { tab_id }),
                },
                respond_to,
                response_written: None,
                stream_active: None,
            })
        );
        let _ = response_rx.recv_timeout(std::time::Duration::from_millis(100));

        server.render_and_stream();
        assert!(
            matches!(
                server.clients[&1].view_state.as_ref().unwrap().tab_control,
                crate::app::ClientTabControl::Controlling { .. }
            ),
            "API focus must reclaim the destination tab before the next paint, not flash Take Control"
        );
    }
    #[test]
    fn terminal_attach_disconnect_restores_app_pane_size() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        let _runtime_guard = rt.enter();
        let mut server = test_headless_server();
        let workspace = crate::workspace::Workspace::test_new("test");
        let pane_id = workspace.tabs[0].root_pane;
        let terminal_id = workspace.terminal_id(pane_id).expect("terminal id").clone();
        let terminal_id_string = terminal_id.to_string();
        server.app.state.workspaces = vec![workspace];
        server.app.state.ensure_test_terminals();
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.mode = crate::app::Mode::Terminal;
        server.app.terminal_runtimes.insert(
            terminal_id.clone(),
            crate::terminal::TerminalRuntime::test_with_screen_bytes(80, 24, b""),
        );
        let mut app_client = ClientConnection::new(
            (120, 40),
            crate::kitty_graphics::HostCellSize::default(),
            crate::terminal_theme::TerminalTheme::default(),
            None,
            1,
            RenderEncoding::SemanticFrame,
            None,
        );
        app_client.view_state = Some(crate::app::ClientViewState::from_default_client_state(
            &server.app.state,
        ));
        server.clients.insert(1, app_client);
        assert!(server.reconcile_client_tab_control(1));
        server.foreground_client_id = Some(1);
        server.sync_foreground_client_state();
        assert!(server.tab_controls.controlled_tab_for_client(1).is_some());
        let expected_app_size = server
            .app
            .terminal_runtimes
            .get(&terminal_id)
            .expect("runtime")
            .current_size();
        assert_ne!(expected_app_size, (24, 80));

        let (writer, _control_rx, _render_rx) = test_client_writer();
        assert!(server.handle_server_event(ServerEvent::ClientConnected {
            client_id: 2,
            cols: 80,
            rows: 24,
            cell_width_px: 0,
            cell_height_px: 0,
            render_encoding: RenderEncoding::TerminalAnsi,
            keybindings: None,
            direct_attach_requested: true,
            direct_graphics: false,
            writer,
        }));
        assert!(
            server.handle_server_event(ServerEvent::ClientAttachTerminal {
                client_id: 2,
                terminal_id: terminal_id_string.clone(),
                takeover: false,
            })
        );
        assert!(server.clients.get(&2).unwrap().view_state.is_none());
        assert_eq!(server.foreground_client_id, Some(1));
        assert!(server
            .app
            .state
            .direct_attach_resize_locks
            .contains(&terminal_id));
        assert_eq!(
            server
                .app
                .terminal_runtimes
                .get(&terminal_id)
                .expect("runtime")
                .current_size(),
            (24, 80)
        );

        assert!(server.handle_server_event(ServerEvent::ClientDisconnected { client_id: 2 }));

        assert!(!server
            .app
            .state
            .direct_attach_resize_locks
            .contains(&terminal_id));
        assert_eq!(
            server
                .app
                .terminal_runtimes
                .get(&terminal_id)
                .expect("runtime")
                .current_size(),
            expected_app_size
        );

        assert!(server.tab_controls.controlled_tab_for_client(1).is_some());
        assert!(
            server.handle_server_event(ServerEvent::ClientAttachTerminal {
                client_id: 1,
                terminal_id: terminal_id_string,
                takeover: false,
            })
        );
        assert_eq!(server.tab_controls.controlled_tab_for_client(1), None);
        drop(server);
        drop(_runtime_guard);
        rt.shutdown_timeout(Duration::from_millis(100));
    }

    #[test]
    fn render_and_stream_sends_terminal_frame_for_terminal_ansi_client() {
        let mut server = test_headless_server();
        let (client_tx, _client_control_rx, client_rx) = test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::TerminalAnsi,
                Some(client_tx),
            ),
        );
        server.foreground_client_id = Some(1);

        server.render_and_stream();

        match read_server_message(
            client_rx
                .recv_timeout(Duration::from_millis(100))
                .expect("terminal frame"),
        ) {
            ServerMessage::Terminal(frame) => {
                assert_eq!(frame.seq, 1);
                assert_eq!((frame.width, frame.height), (80, 24));
                assert!(frame.full);
                assert!(!frame.bytes.is_empty());
            }
            other => panic!("expected terminal frame, got {other:?}"),
        }
        assert_eq!(
            server
                .clients
                .get(&1)
                .unwrap()
                .render_state
                .terminal_seq()
                .unwrap(),
            1
        );
    }

    #[test]
    fn terminal_ansi_input_does_not_reset_blit_baseline() {
        let mut server = test_headless_server();
        let (client_tx, _client_control_rx, client_rx) = test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::TerminalAnsi,
                Some(client_tx),
            ),
        );
        server.foreground_client_id = Some(1);

        server.render_and_stream();
        let _ = client_rx
            .recv_timeout(Duration::from_millis(100))
            .expect("initial terminal frame");
        assert_eq!(
            server
                .clients
                .get(&1)
                .unwrap()
                .render_state
                .terminal_seq()
                .unwrap(),
            1
        );

        assert!(!server.handle_server_event(ServerEvent::ClientInput {
            client_id: 1,
            data: Vec::new(),
        }));
        server.render_and_stream();

        assert_eq!(
            server
                .clients
                .get(&1)
                .unwrap()
                .render_state
                .terminal_seq()
                .unwrap(),
            1
        );
        assert!(client_rx.recv_timeout(Duration::from_millis(50)).is_err());
    }
    #[tokio::test]
    async fn foreground_focus_events_reach_reporting_pane() {
        let mut server = test_headless_server();
        let mut input_rx = install_focused_test_runtime(&mut server, b"\x1b[?1004h");
        server.clients.insert(1, test_app_client(Some(false), 1));
        server.clients.get_mut(&1).unwrap().view_state = Some(
            crate::app::ClientViewState::from_default_client_state(&server.app.state),
        );
        server.foreground_client_id = Some(1);
        server.sync_foreground_client_state();
        assert!(server.reconcile_client_tab_control(1));

        assert!(server.handle_server_event(ServerEvent::ClientInput {
            client_id: 1,
            data: b"\x1b[I".to_vec(),
        }));
        assert_eq!(
            input_rx
                .recv()
                .await
                .expect("forwarded focus gained report"),
            Bytes::from_static(b"\x1b[I")
        );

        assert!(!server.handle_server_event(ServerEvent::ClientInput {
            client_id: 1,
            data: b"\x1b[O".to_vec(),
        }));
        assert_eq!(
            input_rx.recv().await.expect("forwarded focus lost report"),
            Bytes::from_static(b"\x1b[O")
        );
    }

    #[tokio::test]
    async fn structured_focus_targets_the_clients_focused_pane() {
        let mut server = test_headless_server();
        let mut workspace = crate::workspace::Workspace::test_new("focus-view");
        let first_pane = workspace.tabs[0].root_pane;
        let second_pane = workspace.test_split(ratatui::layout::Direction::Horizontal);
        let (first_runtime, mut first_rx) =
            crate::terminal::TerminalRuntime::test_with_channel_and_scrollback_bytes(
                80,
                24,
                0,
                b"\x1b[?1004h",
                4,
            );
        let (second_runtime, mut second_rx) =
            crate::terminal::TerminalRuntime::test_with_channel_and_scrollback_bytes(
                80,
                24,
                0,
                b"\x1b[?1004h",
                4,
            );
        workspace.insert_test_runtime(first_pane, first_runtime);
        workspace.insert_test_runtime(second_pane, second_runtime);
        server.app.state.workspaces = vec![workspace];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.mode = crate::app::Mode::Terminal;

        let mut client = test_app_client(Some(false), 1);
        let mut client_view =
            crate::app::ClientViewState::from_default_client_state(&server.app.state);
        let _ = client_view.focus_pane_in_workspace(&server.app.state, 0, 0, second_pane);
        client.view_state = Some(client_view);
        server.clients.insert(1, client);
        server.foreground_client_id = Some(1);
        server.sync_foreground_client_state();

        assert!(server.handle_server_event(ServerEvent::ClientInputEvents {
            client_id: 1,
            events: vec![crate::raw_input::RawInputEvent::OuterFocusGained],
        }));
        assert_eq!(
            second_rx.recv().await.expect("client focused pane report"),
            Bytes::from_static(b"\x1b[I")
        );
        assert!(first_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn background_focus_batch_forwards_only_after_promotion() {
        let mut server = test_headless_server();
        let mut input_rx = install_focused_test_runtime(&mut server, b"\x1b[?1004h");
        server.clients.insert(1, test_app_client(Some(true), 1));
        server.clients.insert(2, test_app_client(Some(false), 2));
        server.clients.get_mut(&1).unwrap().view_state = Some(
            crate::app::ClientViewState::from_default_client_state(&server.app.state),
        );
        server.clients.get_mut(&2).unwrap().view_state = Some(
            crate::app::ClientViewState::from_default_client_state(&server.app.state),
        );
        server.foreground_client_id = Some(1);
        server.sync_foreground_client_state();
        assert!(server.reconcile_client_tab_control(1));
        server
            .clients
            .get_mut(&2)
            .unwrap()
            .view_state
            .as_mut()
            .unwrap()
            .request_tab_control();
        assert!(server.reconcile_client_tab_control(2));

        assert!(server.handle_server_event(ServerEvent::ClientInputEvents {
            client_id: 2,
            events: vec![
                crate::raw_input::RawInputEvent::OuterFocusLost,
                crate::raw_input::RawInputEvent::OuterFocusGained,
            ],
        }));
        assert_eq!(server.foreground_client_id, Some(2));
        assert_eq!(server.app.state.outer_terminal_focus, Some(true));
        assert_eq!(
            input_rx.recv().await.expect("focus gained after promotion"),
            Bytes::from_static(b"\x1b[I")
        );
        assert!(input_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn non_app_focus_events_are_ignored_without_suppressing_keys() {
        let mut server = test_headless_server();
        let mut input_rx = install_focused_test_runtime(&mut server, b"\x1b[?1004h");
        server.clients.insert(1, test_app_client(Some(true), 1));

        let mut attached = test_app_client(Some(false), 2);
        attached.mode = ClientConnectionMode::TerminalAttach {
            terminal_id: "attached".to_owned(),
        };
        server.clients.insert(2, attached);

        let mut pending = test_app_client(Some(false), 3);
        pending.pending_terminal_attach = true;
        server.clients.insert(3, pending);
        server.foreground_client_id = Some(1);
        server.sync_foreground_client_state();

        for client_id in [2, 3] {
            let _ = server.handle_server_event(ServerEvent::ClientInputEvents {
                client_id,
                events: vec![crate::raw_input::RawInputEvent::OuterFocusGained],
            });
            assert_eq!(server.foreground_client_id, Some(1));
            assert_eq!(server.app.state.outer_terminal_focus, Some(true));
            assert_eq!(server.clients[&client_id].outer_terminal_focus, Some(false));
        }
        assert!(input_rx.try_recv().is_err());

        assert!(server.handle_server_event(ServerEvent::ClientInputEvents {
            client_id: 3,
            events: vec![crate::raw_input::RawInputEvent::Key(
                crate::input::TerminalKey::new(
                    crossterm::event::KeyCode::Char('x'),
                    crossterm::event::KeyModifiers::empty(),
                )
                .with_kind(crossterm::event::KeyEventKind::Press),
            )],
        }));
        assert_eq!(server.foreground_client_id, Some(3));
    }

    #[test]
    fn outer_focus_gained_forces_terminal_ansi_full_redraw() {
        let mut server = test_headless_server();
        let (client_tx, _client_control_rx, client_rx) = test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::TerminalAnsi,
                Some(client_tx),
            ),
        );
        server.foreground_client_id = Some(1);

        server.render_and_stream();
        let _ = client_rx
            .recv_timeout(Duration::from_millis(100))
            .expect("initial terminal frame");

        assert!(server.handle_server_event(ServerEvent::ClientInput {
            client_id: 1,
            data: b"\x1b[I".to_vec(),
        }));
        server.render_and_stream();

        match read_server_message(client_rx.recv_timeout(Duration::from_millis(100)).unwrap()) {
            ServerMessage::Terminal(frame) => {
                assert_eq!(frame.seq, 2);
                assert!(frame.full);
            }
            other => panic!("expected terminal frame, got {other:?}"),
        }
    }

    #[test]
    fn outer_focus_gained_client_render_pending_survives_semantic_render_queue_full() {
        let mut server = test_headless_server();
        let (client_tx, _client_control_rx, client_rx) = test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                Some(client_tx),
            ),
        );
        server.foreground_client_id = Some(1);

        server.render_and_stream();
        let _ = client_rx
            .recv_timeout(Duration::from_millis(100))
            .expect("initial semantic frame");

        let queued = HeadlessServer::frame_server_message(&ServerMessage::ReloadSoundConfig)
            .expect("serialize dummy message");
        server
            .clients
            .get(&1)
            .unwrap()
            .writer
            .as_ref()
            .unwrap()
            .render
            .try_send(queued)
            .expect("pre-fill render queue");

        assert!(server.handle_server_event(ServerEvent::ClientInput {
            client_id: 1,
            data: b"\x1b[I".to_vec(),
        }));
        assert!(server.clients.get(&1).unwrap().render_pending);

        server.render_and_stream();

        assert!(server.clients.get(&1).unwrap().render_pending);
        assert!(matches!(
            read_server_message(client_rx.recv_timeout(Duration::from_millis(100)).unwrap()),
            ServerMessage::ReloadSoundConfig
        ));

        assert!(server.handle_server_event(ServerEvent::ClientWriterDrained { client_id: 1 }));
        server.render_and_stream();

        assert!(!server.clients.get(&1).unwrap().render_pending);
        assert!(matches!(
            read_server_message(client_rx.recv_timeout(Duration::from_millis(100)).unwrap()),
            ServerMessage::Frame(_)
        ));
    }

    #[test]
    fn outer_focus_gained_does_not_force_terminal_ansi_full_redraw_when_disabled() {
        let mut server = test_headless_server();
        server.app.state.redraw_on_focus_gained = false;
        let (client_tx, _client_control_rx, client_rx) = test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::TerminalAnsi,
                Some(client_tx),
            ),
        );
        server.foreground_client_id = Some(1);

        server.render_and_stream();
        let _ = client_rx
            .recv_timeout(Duration::from_millis(100))
            .expect("initial terminal frame");

        server.handle_server_event(ServerEvent::ClientInput {
            client_id: 1,
            data: b"\x1b[I".to_vec(),
        });
        server.render_and_stream();

        assert!(client_rx.recv_timeout(Duration::from_millis(50)).is_err());
        assert_eq!(server.clients[&1].outer_terminal_focus, Some(true));
        assert_eq!(server.app.state.outer_terminal_focus, Some(true));
        assert_eq!(
            server
                .clients
                .get(&1)
                .unwrap()
                .render_state
                .terminal_seq()
                .unwrap(),
            1
        );
    }

    #[test]
    fn outer_focus_gained_does_not_mark_semantic_render_pending_when_disabled() {
        let mut server = test_headless_server();
        server.app.state.redraw_on_focus_gained = false;
        let (client_tx, _client_control_rx, _client_rx) = test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                Some(client_tx),
            ),
        );
        server.foreground_client_id = Some(1);

        assert!(server.handle_server_event(ServerEvent::ClientInput {
            client_id: 1,
            data: b"\x1b[I".to_vec(),
        }));

        assert!(!server.clients.get(&1).unwrap().render_pending);
        assert!(!server.app.full_redraw_pending);
        assert_eq!(server.clients[&1].outer_terminal_focus, Some(true));
        assert_eq!(server.app.state.outer_terminal_focus, Some(true));
    }

    #[test]
    fn full_render_queue_does_not_advance_terminal_ansi_baseline() {
        let mut server = test_headless_server();
        let (client_tx, _client_control_rx, client_rx) = test_client_writer();
        let queued = HeadlessServer::frame_server_message(&ServerMessage::ReloadSoundConfig)
            .expect("serialize dummy message");
        client_tx
            .render
            .try_send(queued)
            .expect("pre-fill render queue");

        server.clients.insert(
            1,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::TerminalAnsi,
                Some(client_tx),
            ),
        );
        server.foreground_client_id = Some(1);

        server.render_and_stream();

        assert_eq!(
            server
                .clients
                .get(&1)
                .unwrap()
                .render_state
                .terminal_seq()
                .unwrap(),
            0
        );
        assert!(matches!(
            read_server_message(client_rx.recv_timeout(Duration::from_millis(100)).unwrap()),
            ServerMessage::ReloadSoundConfig
        ));
        assert!(client_rx.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn writer_drained_retries_pending_terminal_ansi_render() {
        let mut server = test_headless_server();
        let (client_tx, _client_control_rx, client_rx) = test_client_writer();
        let queued = HeadlessServer::frame_server_message(&ServerMessage::ReloadSoundConfig)
            .expect("serialize dummy message");
        client_tx
            .render
            .try_send(queued)
            .expect("pre-fill render queue");

        server.clients.insert(
            1,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::TerminalAnsi,
                Some(client_tx),
            ),
        );
        server.foreground_client_id = Some(1);

        server.render_and_stream();
        assert!(server.clients.get(&1).unwrap().render_pending);
        assert!(matches!(
            read_server_message(client_rx.recv_timeout(Duration::from_millis(100)).unwrap()),
            ServerMessage::ReloadSoundConfig
        ));

        assert!(server.handle_server_event(ServerEvent::ClientWriterDrained { client_id: 1 }));
        server.render_and_stream();

        match read_server_message(client_rx.recv_timeout(Duration::from_millis(100)).unwrap()) {
            ServerMessage::Terminal(frame) => assert_eq!(frame.seq, 1),
            other => panic!("expected terminal frame, got {other:?}"),
        }
        assert_eq!(
            server
                .clients
                .get(&1)
                .unwrap()
                .render_state
                .terminal_seq()
                .unwrap(),
            1
        );
        assert!(!server.clients.get(&1).unwrap().render_pending);
    }

    #[test]
    fn render_and_stream_skips_identical_frame_sends() {
        let mut server = test_headless_server();
        server.app.state.workspaces = vec![crate::workspace::Workspace::test_new("test")];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.mode = crate::app::Mode::Terminal;

        let (client_tx, _client_control_rx, client_rx) = test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                Some(client_tx),
            ),
        );
        server.clients.get_mut(&1).unwrap().view_state = Some(
            crate::app::ClientViewState::from_default_client_state(&server.app.state),
        );
        server.foreground_client_id = Some(1);
        server.sync_foreground_client_state();
        assert!(server.reconcile_client_tab_control(1));

        server.render_and_stream();
        let first = client_rx.recv_timeout(Duration::from_millis(100));
        assert!(first.is_ok(), "expected first frame to be sent");

        server.render_and_stream();
        assert!(
            client_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "identical frame should not be sent twice"
        );
    }

    #[test]
    fn notification_show_api_forwards_system_to_foreground_client() {
        let mut server = test_headless_server();
        let (client_tx, client_control_rx, _client_rx) = test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                Some(client_tx),
            ),
        );
        server.foreground_client_id = Some(1);
        server.app.state.toast_config.delivery = crate::config::ToastDelivery::System;

        let (respond_to, response_rx) = std::sync::mpsc::channel();
        assert!(
            server.handle_api_request_with_shutdown_check(api::ApiRequestMessage {
                request: api::schema::Request {
                    id: "notify".into(),
                    method: api::schema::Method::NotificationShow(
                        api::schema::NotificationShowParams {
                            title: "build\nfailed".into(),
                            body: Some("api workspace".into()),
                            position: None,
                            sound: api::schema::NotificationShowSound::None,
                        },
                    ),
                },
                respond_to,
                response_written: None,
                stream_active: None,
            })
        );

        let response = response_rx
            .recv_timeout(Duration::from_millis(100))
            .unwrap();
        let parsed: api::schema::SuccessResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(
            parsed.result,
            api::schema::ResponseResult::NotificationShow {
                shown: true,
                reason: api::schema::NotificationShowReason::Shown,
            }
        );
        match read_server_message(
            client_control_rx
                .recv_timeout(Duration::from_millis(100))
                .expect("api notification message"),
        ) {
            ServerMessage::Notify { kind, message } => {
                assert_eq!(kind, protocol::NotifyKind::SystemToast);
                assert_eq!(message, "build failed: api workspace");
            }
            other => panic!("expected api notification, got {other:?}"),
        }
    }
    #[test]
    fn client_config_reload_request_refreshes_attached_clients() {
        let mut server = test_headless_server();
        let (client_tx, client_control_rx, _client_rx) = test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                Some(client_tx),
            ),
        );
        server.app.state.request_client_config_reload = true;

        server.drain_client_config_reload_request();

        match read_server_message(
            client_control_rx
                .recv_timeout(Duration::from_millis(100))
                .expect("client config reload message"),
        ) {
            ServerMessage::ReloadSoundConfig => {}
            other => panic!("expected ReloadSoundConfig, got {other:?}"),
        }
        assert!(!server.app.state.request_client_config_reload);
    }

    #[test]
    fn client_host_actions_follow_the_invoking_view_not_foreground_focus() {
        let mut server = test_headless_server();
        let (first_writer, first_control, _first_render) = test_client_writer();
        let (second_writer, second_control, _second_render) = test_client_writer();
        for (id, writer) in [(1, first_writer), (2, second_writer)] {
            let mut client = ClientConnection::new(
                (120, 40),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                id,
                RenderEncoding::SemanticFrame,
                Some(writer),
            );
            client.view_state = Some(crate::app::ClientViewState::from_default_client_state(
                &server.app.state,
            ));
            server.clients.insert(id, client);
        }
        server.foreground_client_id = Some(2);
        let view_id = server.clients[&1]
            .view_state
            .as_ref()
            .expect("invoking view")
            .id();
        server.handle_internal_event_with_forwarding(AppEvent::ClientOpenUrl {
            view_id,
            url: "https://github.com/acme/project/pull/17".into(),
        });
        server.handle_internal_event_with_forwarding(AppEvent::ClientClipboardWrite {
            view_id,
            content: b"acme/project#17".to_vec(),
        });
        assert!(matches!(
            read_server_message(first_control.recv_timeout(Duration::from_secs(1)).expect("browser action")),
            ServerMessage::OpenUrl { url } if url == "https://github.com/acme/project/pull/17"
        ));
        assert!(matches!(
            read_server_message(first_control.recv_timeout(Duration::from_secs(1)).expect("clipboard action")),
            ServerMessage::Clipboard { data } if data == "YWNtZS9wcm9qZWN0IzE3"
        ));
        assert!(matches!(
            second_control.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        server.remove_client(1);
        server.handle_internal_event_with_forwarding(AppEvent::ClientOpenUrl {
            view_id,
            url: "https://github.com/acme/project/pull/17".into(),
        });
        assert!(matches!(
            second_control.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn clipboard_write_targets_foreground_client_only() {
        let mut server = test_headless_server();
        let (background_tx, background_control_rx, _background_rx) = test_client_writer();
        let (foreground_tx, foreground_control_rx, _foreground_rx) = test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (120, 40),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                Some(background_tx),
            ),
        );
        server.clients.insert(
            2,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                2,
                RenderEncoding::SemanticFrame,
                Some(foreground_tx),
            ),
        );
        server.foreground_client_id = Some(2);
        server.sync_foreground_client_state();

        let changed = server.handle_internal_event_with_forwarding(AppEvent::ClipboardWrite {
            content: b"test".to_vec(),
        });

        assert!(changed);
        match read_server_message(
            foreground_control_rx
                .recv_timeout(Duration::from_millis(100))
                .expect("foreground clipboard message"),
        ) {
            ServerMessage::Clipboard { data } => assert_eq!(data, "dGVzdA=="),
            other => panic!("expected clipboard message, got {other:?}"),
        }
        assert!(
            background_control_rx
                .recv_timeout(Duration::from_millis(50))
                .is_err(),
            "background client should not receive clipboard writes"
        );
    }

    #[test]
    fn terminal_clipboard_write_targets_controller_not_watcher() {
        let mut server = test_headless_server();
        let workspace = crate::workspace::Workspace::test_new("osc-controller");
        let pane_id = workspace.tabs[0].root_pane;
        server.app.state.workspaces = vec![workspace];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        let (controller_writer, controller_rx, _controller_render_rx) = test_client_writer();
        let (watcher_writer, watcher_rx, _watcher_render_rx) = test_client_writer();

        let mut controller = test_app_client(Some(true), 2);
        controller.writer = Some(controller_writer);
        controller.view_state = Some(crate::app::ClientViewState::from_default_client_state(
            &server.app.state,
        ));
        let mut watcher = test_app_client(Some(false), 1);
        watcher.writer = Some(watcher_writer);
        watcher.view_state = Some(crate::app::ClientViewState::from_default_client_state(
            &server.app.state,
        ));
        server.clients.insert(1, controller);
        server.clients.insert(2, watcher);
        server.foreground_client_id = Some(1);
        server.sync_foreground_client_state();
        assert!(server.reconcile_client_tab_control(1));

        assert!(
            server.handle_internal_event_with_forwarding(AppEvent::TerminalClipboardWrite {
                pane_id,
                content: b"osc".to_vec(),
            },)
        );
        match read_server_message(
            controller_rx
                .recv_timeout(Duration::from_millis(100))
                .expect("controller clipboard message"),
        ) {
            ServerMessage::Clipboard { data } => assert_eq!(data, "b3Nj"),
            other => panic!("expected controller clipboard message, got {other:?}"),
        }
        assert!(
            watcher_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "watcher must not receive terminal OSC clipboard writes"
        );
    }

    fn install_test_popup_pane(
        server: &mut HeadlessServer,
        owner_view: &mut crate::app::ClientViewState,
        location: crate::execution_host::ResourceLocation,
    ) -> crate::layout::PaneId {
        let pane_id = crate::layout::PaneId::alloc();
        let terminal_id = crate::terminal::TerminalId::alloc();
        server.app.state.terminals.insert(
            terminal_id.clone(),
            crate::terminal::TerminalState::new_at(terminal_id.clone(), location),
        );
        server.app.state.popup_panes.insert(
            pane_id,
            crate::app::state::PopupPaneState {
                pane_id,
                terminal_id,
                width: None,
                height: None,
                owner: Some(owner_view.id()),
            },
        );
        owner_view.popup_pane = Some(pane_id);
        owner_view.mode = crate::app::Mode::Terminal;
        pane_id
    }

    #[test]
    fn popup_terminal_clipboard_write_targets_owning_client() {
        let mut server = test_headless_server();
        server.app.state.workspaces = vec![crate::workspace::Workspace::test_new("popup-osc")];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;

        let (owner_writer, owner_rx, _owner_render_rx) = test_client_writer();
        let (other_writer, other_rx, _other_render_rx) = test_client_writer();

        let mut owner = test_app_client(Some(true), 2);
        owner.writer = Some(owner_writer);
        let mut owner_view =
            crate::app::ClientViewState::from_default_client_state(&server.app.state);
        let mut other = test_app_client(Some(false), 1);
        other.writer = Some(other_writer);
        other.view_state = Some(crate::app::ClientViewState::from_default_client_state(
            &server.app.state,
        ));

        let pane_id = install_test_popup_pane(
            &mut server,
            &mut owner_view,
            crate::execution_host::ResourceLocation::local("/popup").expect("local popup location"),
        );
        owner.view_state = Some(owner_view);

        server.clients.insert(1, owner);
        server.clients.insert(2, other);
        server.foreground_client_id = Some(2);
        server.sync_foreground_client_state();

        assert!(
            server.handle_internal_event_with_forwarding(AppEvent::TerminalClipboardWrite {
                pane_id,
                content: b"popup-osc".to_vec(),
            })
        );
        match read_server_message(
            owner_rx
                .recv_timeout(Duration::from_millis(100))
                .expect("owner popup clipboard message"),
        ) {
            ServerMessage::Clipboard { data } => assert_eq!(data, "cG9wdXAtb3Nj"),
            other => panic!("expected owner clipboard message, got {other:?}"),
        }
        assert!(
            other_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "non-owner must not receive popup OSC clipboard writes"
        );
    }

    #[test]
    fn popup_open_url_targets_owning_client() {
        let mut server = test_headless_server();
        server.app.state.workspaces = vec![crate::workspace::Workspace::test_new("popup-url")];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;

        let (owner_writer, owner_rx, _owner_render_rx) = test_client_writer();
        let (other_writer, other_rx, _other_render_rx) = test_client_writer();

        let mut owner = test_app_client(Some(true), 2);
        owner.writer = Some(owner_writer);
        let mut owner_view =
            crate::app::ClientViewState::from_default_client_state(&server.app.state);
        let mut other = test_app_client(Some(false), 1);
        other.writer = Some(other_writer);
        other.view_state = Some(crate::app::ClientViewState::from_default_client_state(
            &server.app.state,
        ));

        // Remote popup execution host: public URLs still open on the rendering client.
        let remote_host = crate::execution_host::ExecutionHostId::new("ssh:remote-popup")
            .expect("remote host id");
        let remote_path =
            crate::execution_host::HostPath::new("/remote/popup").expect("remote popup path");
        let pane_id = install_test_popup_pane(
            &mut server,
            &mut owner_view,
            crate::execution_host::ResourceLocation::new(remote_host, remote_path),
        );
        owner.view_state = Some(owner_view);

        server.clients.insert(1, owner);
        server.clients.insert(2, other);
        server.foreground_client_id = Some(2);
        server.sync_foreground_client_state();

        assert!(
            server.handle_internal_event_with_forwarding(AppEvent::OpenUrl {
                pane_id,
                url: "https://example.com/popup".to_owned(),
            })
        );
        match read_server_message(
            owner_rx
                .recv_timeout(Duration::from_millis(100))
                .expect("owner popup open-url message"),
        ) {
            ServerMessage::OpenUrl { url } => assert_eq!(url, "https://example.com/popup"),
            other => panic!("expected owner OpenUrl message, got {other:?}"),
        }
        assert!(
            other_rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "non-owner must not receive popup OpenUrl"
        );
    }

    #[test]
    fn stale_terminal_attach_owner_does_not_mask_valid_ownership() {
        let mut server = test_headless_server();
        let workspace = crate::workspace::Workspace::test_new("stale-owner");
        let pane_id = workspace.tabs[0].root_pane;
        server.app.state.workspaces = vec![workspace];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.state.ensure_test_terminals();
        let terminal_id = server.app.state.workspaces[0]
            .pane_state(pane_id)
            .expect("root pane")
            .attached_terminal_id
            .clone();

        let (controller_writer, controller_rx, _controller_render_rx) = test_client_writer();
        let mut controller = test_app_client(Some(true), 2);
        controller.writer = Some(controller_writer);
        controller.view_state = Some(crate::app::ClientViewState::from_default_client_state(
            &server.app.state,
        ));
        server.clients.insert(1, controller);
        server.foreground_client_id = Some(1);
        server.sync_foreground_client_state();
        assert!(server.reconcile_client_tab_control(1));

        // Insert a stale mapping before the valid one so HashMap iteration can hit it first.
        server
            .terminal_attach_owners
            .insert("stale-missing-terminal".to_owned(), 99);
        server
            .terminal_attach_owners
            .insert(terminal_id.to_string(), 1);

        assert_eq!(server.clipboard_controller_for_pane(pane_id), Some(1));
        assert!(
            server.handle_internal_event_with_forwarding(AppEvent::TerminalClipboardWrite {
                pane_id,
                content: b"stale-ok".to_vec(),
            })
        );
        match read_server_message(
            controller_rx
                .recv_timeout(Duration::from_millis(100))
                .expect("controller clipboard after stale owner"),
        ) {
            ServerMessage::Clipboard { data } => assert_eq!(data, "c3RhbGUtb2s="),
            other => panic!("expected controller clipboard message, got {other:?}"),
        }
    }

    #[test]
    fn clipboard_write_without_foreground_client_does_not_show_feedback() {
        let mut server = test_headless_server();
        server.foreground_client_id = None;

        let changed = server.handle_internal_event_with_forwarding(AppEvent::ClipboardWrite {
            content: b"test".to_vec(),
        });

        assert!(changed);
        assert!(
            server.app.state.copy_feedback.is_none(),
            "clipboard feedback should only show when a foreground client can receive the write"
        );
    }

    #[test]
    fn clipboard_write_failed_foreground_send_does_not_show_feedback() {
        let mut server = test_headless_server();
        let (foreground_tx, foreground_control_rx, _foreground_rx) = test_client_writer();
        drop(foreground_control_rx);

        server.clients.insert(
            1,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                Some(foreground_tx),
            ),
        );
        server.foreground_client_id = Some(1);

        let changed = server.handle_internal_event_with_forwarding(AppEvent::ClipboardWrite {
            content: b"test".to_vec(),
        });

        assert!(changed);
        assert!(
            server.app.state.copy_feedback.is_none(),
            "clipboard feedback should only show after the foreground client receives the write"
        );
        assert!(
            !server.clients.contains_key(&1),
            "failed targeted send should remove the broken foreground client"
        );
    }

    #[test]
    fn client_local_notifications_target_foreground_client_only() {
        let mut server = test_headless_server();
        let (background_tx, background_control_rx, _background_rx) = test_client_writer();
        let (foreground_tx, foreground_control_rx, _foreground_rx) = test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (120, 40),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                Some(background_tx),
            ),
        );
        server.clients.insert(
            2,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                2,
                RenderEncoding::SemanticFrame,
                Some(foreground_tx),
            ),
        );
        server.foreground_client_id = Some(2);
        server.sync_foreground_client_state();

        assert!(server.send_to_foreground_client(ServerMessage::Notify {
            kind: protocol::NotifyKind::Toast,
            message: "pi finished: workspace 1".to_string(),
        }));

        match read_server_message(
            foreground_control_rx
                .recv_timeout(Duration::from_millis(100))
                .expect("foreground toast message"),
        ) {
            ServerMessage::Notify { kind, message } => {
                assert_eq!(kind, protocol::NotifyKind::Toast);
                assert_eq!(message, "pi finished: workspace 1");
            }
            other => panic!("expected toast notify, got {other:?}"),
        }
        assert!(
            background_control_rx
                .recv_timeout(Duration::from_millis(50))
                .is_err(),
            "background client should not receive client-local notifications"
        );
    }

    #[test]
    fn terminal_bell_targets_foreground_client_only() {
        let mut server = test_headless_server();
        let (background_tx, background_control_rx, _background_rx) = test_client_writer();
        let (foreground_tx, foreground_control_rx, _foreground_rx) = test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (120, 40),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                Some(background_tx),
            ),
        );
        server.clients.insert(
            2,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                2,
                RenderEncoding::SemanticFrame,
                Some(foreground_tx),
            ),
        );
        server.foreground_client_id = Some(2);
        server.sync_foreground_client_state();

        let changed = server.handle_internal_event_with_forwarding(AppEvent::TerminalBell {
            pane_id: crate::layout::PaneId::from_raw(1),
            count: 3,
        });

        assert!(changed);
        match read_server_message(
            foreground_control_rx
                .recv_timeout(Duration::from_millis(100))
                .expect("foreground terminal bell"),
        ) {
            ServerMessage::TerminalBell { count } => assert_eq!(count, 3),
            other => panic!("expected terminal bell, got {other:?}"),
        }
        assert!(
            background_control_rx
                .recv_timeout(Duration::from_millis(50))
                .is_err(),
            "background client should not receive terminal bells"
        );
    }

    #[test]
    fn foreground_client_view_controls_active_tab_notification_suppression() {
        let mut server = test_headless_server();
        server.app.state.workspaces = vec![
            crate::workspace::Workspace::test_new("foreground"),
            crate::workspace::Workspace::test_new("target"),
        ];
        server.app.state.ensure_test_terminals();
        server.app.state.active = Some(0);
        server.app.state.mode = crate::app::Mode::Terminal;
        server.app.state.toast_config.delivery = crate::config::ToastDelivery::Gardn;

        let target_pane = server.app.state.workspaces[1].tabs[0].root_pane;
        let target_terminal = server.app.state.workspaces[1]
            .panes
            .get(&target_pane)
            .expect("target pane")
            .attached_terminal_id
            .clone();
        server
            .app
            .state
            .terminals
            .get_mut(&target_terminal)
            .expect("target terminal")
            .state = crate::detect::AgentState::Working;

        let (background_tx, background_control_rx, _background_rx) = test_client_writer();
        let (foreground_tx, foreground_control_rx, _foreground_rx) = test_client_writer();
        let mut background_client = ClientConnection::new(
            (120, 40),
            crate::kitty_graphics::HostCellSize::default(),
            crate::terminal_theme::TerminalTheme::default(),
            Some(true),
            1,
            RenderEncoding::SemanticFrame,
            Some(background_tx),
        );
        let mut foreground_client = ClientConnection::new(
            (80, 24),
            crate::kitty_graphics::HostCellSize::default(),
            crate::terminal_theme::TerminalTheme::default(),
            Some(true),
            2,
            RenderEncoding::SemanticFrame,
            Some(foreground_tx),
        );
        let mut background_view =
            crate::app::ClientViewState::from_default_client_state(&server.app.state);
        background_view.active_workspace = Some(1);
        background_view.selected_workspace = 1;
        let foreground_view =
            crate::app::ClientViewState::from_default_client_state(&server.app.state);
        background_client.view_state = Some(background_view);
        foreground_client.view_state = Some(foreground_view);
        server.clients.insert(1, background_client);
        server.clients.insert(2, foreground_client);
        server.foreground_client_id = Some(2);

        assert!(
            server.handle_internal_event_with_forwarding(AppEvent::StateChanged {
                pane_id: target_pane,
                agent: Some(crate::detect::Agent::Pi),
                state: crate::detect::AgentState::Idle,
                visible_blocker: false,
                visible_idle: false,
                visible_working: false,
                process_exited: false,
                observed_at: Instant::now(),
            })
        );

        match read_server_message(
            foreground_control_rx
                .recv_timeout(Duration::from_millis(100))
                .expect("foreground sound notification"),
        ) {
            ServerMessage::Notify { kind, message } => {
                assert_eq!(kind, protocol::NotifyKind::Sound);
                assert_eq!(message, "agent done");
            }
            other => panic!("expected foreground sound notification, got {other:?}"),
        }
        assert!(
            background_control_rx
                .recv_timeout(Duration::from_millis(50))
                .is_err(),
            "background client watching the pane must not receive the foreground notification"
        );
    }

    #[test]
    fn foreground_client_view_suppresses_active_tab_notifications() {
        let mut server = test_headless_server();
        server.app.state.workspaces = vec![
            crate::workspace::Workspace::test_new("background"),
            crate::workspace::Workspace::test_new("foreground"),
        ];
        server.app.state.ensure_test_terminals();
        server.app.state.active = Some(0);
        server.app.state.mode = crate::app::Mode::Terminal;
        server.app.state.toast_config.delivery = crate::config::ToastDelivery::Gardn;

        let target_pane = server.app.state.workspaces[1].tabs[0].root_pane;
        let target_terminal = server.app.state.workspaces[1]
            .panes
            .get(&target_pane)
            .expect("target pane")
            .attached_terminal_id
            .clone();
        server
            .app
            .state
            .terminals
            .get_mut(&target_terminal)
            .expect("target terminal")
            .state = crate::detect::AgentState::Working;

        let (background_tx, background_control_rx, _background_rx) = test_client_writer();
        let (foreground_tx, foreground_control_rx, _foreground_rx) = test_client_writer();
        let mut background_client = ClientConnection::new(
            (120, 40),
            crate::kitty_graphics::HostCellSize::default(),
            crate::terminal_theme::TerminalTheme::default(),
            Some(true),
            1,
            RenderEncoding::SemanticFrame,
            Some(background_tx),
        );
        let mut foreground_client = ClientConnection::new(
            (80, 24),
            crate::kitty_graphics::HostCellSize::default(),
            crate::terminal_theme::TerminalTheme::default(),
            Some(true),
            2,
            RenderEncoding::SemanticFrame,
            Some(foreground_tx),
        );
        let background_view =
            crate::app::ClientViewState::from_default_client_state(&server.app.state);
        let mut foreground_view =
            crate::app::ClientViewState::from_default_client_state(&server.app.state);
        foreground_view.active_workspace = Some(1);
        foreground_view.selected_workspace = 1;
        background_client.view_state = Some(background_view);
        foreground_client.view_state = Some(foreground_view);
        server.clients.insert(1, background_client);
        server.clients.insert(2, foreground_client);
        server.foreground_client_id = Some(2);

        assert!(
            server.handle_internal_event_with_forwarding(AppEvent::StateChanged {
                pane_id: target_pane,
                agent: Some(crate::detect::Agent::Pi),
                state: crate::detect::AgentState::Idle,
                visible_blocker: false,
                visible_idle: false,
                visible_working: false,
                process_exited: false,
                observed_at: Instant::now(),
            })
        );

        assert!(
            foreground_control_rx
                .recv_timeout(Duration::from_millis(50))
                .is_err(),
            "foreground client watching the pane should suppress completion notification"
        );
        assert!(
            background_control_rx
                .recv_timeout(Duration::from_millis(50))
                .is_err(),
            "background client should not receive client-local notifications"
        );
    }

    #[test]
    fn gardn_toast_delivery_keeps_update_ready_in_frame_without_client_notify() {
        let mut server = test_headless_server();
        let (client_tx, client_control_rx, _client_rx) = test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                Some(client_tx),
            ),
        );
        server.foreground_client_id = Some(1);
        server.app.state.toast_config.delivery = crate::config::ToastDelivery::Gardn;

        let changed = server.handle_internal_event_with_forwarding(AppEvent::UpdateReady {
            version: "9.9.9".to_string(),
            install: crate::install::UpdateInstallAction::Direct,
        });

        assert!(changed);
        assert_eq!(server.app.state.update_available.as_deref(), Some("9.9.9"));
        assert_eq!(server.app.state.toast, None);
        assert!(
            client_control_rx
                .recv_timeout(Duration::from_millis(50))
                .is_err(),
            "gardn delivery should keep update-ready in the sidebar instead of forwarding a client-local notification"
        );
    }

    #[test]
    fn system_toast_delivery_forwards_system_notify_kind() {
        let mut server = test_headless_server();
        let (client_tx, client_control_rx, _client_rx) = test_client_writer();

        server.clients.insert(
            1,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                Some(client_tx),
            ),
        );
        server.foreground_client_id = Some(1);
        server.app.state.toast_config.delivery = crate::config::ToastDelivery::System;

        let changed = server.handle_internal_event_with_forwarding(AppEvent::UpdateReady {
            version: "9.9.9".to_string(),
            install: crate::install::UpdateInstallAction::Direct,
        });

        assert!(changed);
        match read_server_message(
            client_control_rx
                .recv_timeout(Duration::from_millis(100))
                .expect("system toast message"),
        ) {
            ServerMessage::Notify { kind, message } => {
                assert_eq!(kind, protocol::NotifyKind::SystemToast);
                assert_eq!(message, "v9.9.9 Available: Detach, then run `gardn update`");
            }
            other => panic!("expected system toast notify, got {other:?}"),
        }
    }

    #[test]
    fn stale_api_agent_report_does_not_forward_done_sound() {
        let mut server = test_headless_server();
        let background = crate::workspace::Workspace::test_new("background");
        let pane_id = background.tabs[0].root_pane;
        let public_pane_id = format!("{}-1", background.id);
        let foreground = crate::workspace::Workspace::test_new("foreground");
        server.app.state.workspaces = vec![background, foreground];
        server.app.state.ensure_test_terminals();
        let terminal_id = server.app.state.workspaces[0]
            .pane_state(pane_id)
            .unwrap()
            .attached_terminal_id
            .clone();
        server
            .app
            .state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .set_hook_authority(
                "gardn:pi".into(),
                "pi".into(),
                crate::detect::AgentState::Working,
                None,
                Some(20),
            );
        server.app.state.active = Some(1);
        server.app.state.selected = 1;
        server.app.state.mode = crate::app::Mode::Terminal;

        let (client_tx, client_control_rx, _client_rx) = test_client_writer();
        server.clients.insert(
            1,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                Some(client_tx),
            ),
        );
        server.foreground_client_id = Some(1);
        server.sync_foreground_client_state();

        let (respond_to, response_rx) = std::sync::mpsc::channel();
        let changed = server.handle_api_request_with_shutdown_check(api::ApiRequestMessage {
            request: api::schema::Request {
                id: "stale".into(),
                method: api::schema::Method::PaneReportAgent(api::schema::PaneReportAgentParams {
                    pane_id: public_pane_id,
                    source: "gardn:pi".into(),
                    agent: "pi".into(),
                    state: api::schema::PaneAgentState::Idle,
                    message: None,
                    custom_status: None,
                    seq: Some(19),
                    agent_session_id: None,
                    agent_session_path: None,
                    activity_unix_secs: None,
                    launch_env: std::collections::BTreeMap::new(),
                }),
            },
            respond_to,
            response_written: None,
            stream_active: None,
        });

        assert!(changed);
        assert!(response_rx.recv_timeout(Duration::from_millis(100)).is_ok());
        assert_eq!(
            server.app.state.terminals.get(&terminal_id).unwrap().state,
            crate::detect::AgentState::Working
        );
        assert!(
            client_control_rx
                .recv_timeout(Duration::from_millis(50))
                .is_err(),
            "stale idle report must not forward a done sound"
        );
    }

    /// Verify that no direct calls to `self.app.handle_internal_event`
    /// exist outside of `handle_internal_event_with_forwarding` in this
    /// module. This ensures the forwarding bypass cannot be reintroduced.
    ///
    /// The search pattern looks for `handle_internal_event` calls that
    /// are NOT inside the `handle_internal_event_with_forwarding` method.
    #[test]
    fn no_handle_internal_event_bypass_in_module() {
        let source = include_str!("headless.rs");

        // Find all lines containing handle_internal_event
        let mut bypass_lines: Vec<String> = Vec::new();
        let mut inside_forwarding_method = false;
        let mut forwarding_method_brace_depth = 0u32;

        for (i, line) in source.lines().enumerate() {
            let line_num = i + 1;

            // Track when we're inside handle_internal_event_with_forwarding
            if line.contains("fn handle_internal_event_with_forwarding") {
                inside_forwarding_method = true;
                forwarding_method_brace_depth = 0;
            }

            if inside_forwarding_method {
                // Count braces to track when we exit the method
                for ch in line.chars() {
                    match ch {
                        '{' => forwarding_method_brace_depth += 1,
                        '}' => {
                            forwarding_method_brace_depth =
                                forwarding_method_brace_depth.saturating_sub(1);
                            if forwarding_method_brace_depth == 0 {
                                inside_forwarding_method = false;
                            }
                        }
                        _ => {}
                    }
                }
            } else if line.contains("self.app.handle_internal_event(")
                && !line.trim().starts_with("///")
                && !line.contains("contains(")
            {
                // Direct call to handle_internal_event outside the forwarding method
                bypass_lines.push(format!("line {}: {}", line_num, line.trim()));
            }
        }

        assert!(
            bypass_lines.is_empty(),
            "Found direct calls to self.app.handle_internal_event outside \
             handle_internal_event_with_forwarding (bypass risk):\n  {}",
            bypass_lines.join("\n  ")
        );
    }
    // -------------------------------------------------------------------
    // Outer window title sync
    // -------------------------------------------------------------------

    fn attach_window_title_client(
        server: &mut HeadlessServer,
        client_id: u64,
    ) -> (
        std::sync::mpsc::Receiver<Vec<u8>>,
        std::sync::mpsc::Receiver<Vec<u8>>,
    ) {
        let (writer, control_rx, render_rx) = test_client_writer();
        server.clients.insert(
            client_id,
            ClientConnection::new(
                (80, 24),
                crate::kitty_graphics::HostCellSize::default(),
                crate::terminal_theme::TerminalTheme::default(),
                None,
                1,
                RenderEncoding::SemanticFrame,
                Some(writer),
            ),
        );
        server.foreground_client_id = Some(client_id);
        (control_rx, render_rx)
    }

    fn next_window_title(control_rx: &std::sync::mpsc::Receiver<Vec<u8>>) -> Option<String> {
        match read_server_message(
            control_rx
                .recv_timeout(Duration::from_millis(500))
                .expect("window title message"),
        ) {
            ServerMessage::WindowTitle { title } => title,
            other => panic!("expected window title, got {other:?}"),
        }
    }

    fn no_window_title(control_rx: &std::sync::mpsc::Receiver<Vec<u8>>) {
        assert!(
            control_rx.recv_timeout(Duration::from_millis(100)).is_err(),
            "unexpected window title message"
        );
    }

    fn window_title_test_server(template: &str) -> HeadlessServer {
        let mut server = test_headless_server();
        server.app.state.workspaces = vec![crate::workspace::Workspace::test_new("herd")];
        server.app.state.active = Some(0);
        server.app.state.selected = 0;
        server.app.configure_window_title(template);
        server
    }

    #[test]
    fn window_title_tracks_workspace_and_tab_names() {
        let mut server = window_title_test_server("{workspace}/{tab}");
        let (control_rx, _render_rx) = attach_window_title_client(&mut server, 1);

        server.sync_window_title();
        assert_eq!(next_window_title(&control_rx).as_deref(), Some("herd/1"));

        server.app.state.workspaces[0].tabs[0].custom_name = Some("build".into());
        server.sync_window_title();
        assert_eq!(
            next_window_title(&control_rx).as_deref(),
            Some("herd/build")
        );

        server.app.state.workspaces[0].custom_name = Some("api".into());
        server.sync_window_title();
        assert_eq!(next_window_title(&control_rx).as_deref(), Some("api/build"));
    }

    #[tokio::test]
    async fn window_title_uses_the_focused_pane_terminal_title() {
        let mut server = window_title_test_server("{terminal_title}");
        let pane_id = server.app.state.workspaces[0].tabs[0].root_pane;
        let runtime = crate::terminal::TerminalRuntime::test_with_screen_bytes(80, 24, b"");
        server.app.state.workspaces[0].insert_test_runtime(pane_id, runtime);
        let (control_rx, _render_rx) = attach_window_title_client(&mut server, 1);

        server.sync_window_title();
        assert_eq!(next_window_title(&control_rx), None);

        server.app.state.workspaces[0]
            .test_runtimes
            .get(&pane_id)
            .expect("runtime")
            .test_process_pty_bytes(pane_id, "\x1b]0;\u{280b} building\x07".as_bytes());
        assert!(server.app.take_focused_terminal_title_dirty());
        server.sync_window_title();
        assert_eq!(next_window_title(&control_rx).as_deref(), Some("building"));

        for (_, runtime) in server.app.state.workspaces[0].test_runtimes.drain() {
            runtime.shutdown();
        }
    }

    #[test]
    fn window_title_osc_bytes_are_stripped_and_titles_stay_bounded() {
        assert_eq!(
            crate::config::sanitize_window_title_text("gardn\x1b]0;evil\u{7}\u{9c}\n").as_deref(),
            Some("gardn]0;evil")
        );
        let bounded = crate::config::sanitize_window_title_text(&"x".repeat(500)).expect("title");
        assert_eq!(bounded.chars().count(), 200);
    }

    #[test]
    fn window_title_api_override_beats_the_config_until_cleared() {
        let mut server = window_title_test_server("{workspace}");
        let (control_rx, _render_rx) = attach_window_title_client(&mut server, 1);

        let response = server.handle_client_window_title_api("1".into(), Some("deploy".into()));
        assert!(response.contains(r#""reason":"set""#), "{response}");
        assert_eq!(next_window_title(&control_rx).as_deref(), Some("deploy"));

        // Config-driven changes stay suppressed while the override is set.
        server.app.state.workspaces[0].custom_name = Some("api".into());
        server.sync_window_title();
        no_window_title(&control_rx);

        let response = server.handle_client_window_title_api("2".into(), None);
        assert!(response.contains(r#""reason":"cleared""#), "{response}");
        assert_eq!(next_window_title(&control_rx).as_deref(), Some("api"));
    }

    #[test]
    fn window_title_api_rejects_unusable_titles() {
        let mut server = window_title_test_server("{workspace}");

        let response = server.handle_client_window_title_api("1".into(), Some("\u{7}\n".into()));
        assert!(response.contains("window title is empty"), "{response}");
        assert!(server.api_window_title.is_none());
    }

    #[test]
    fn window_title_is_resent_to_a_newly_attached_client() {
        let mut server = window_title_test_server("{workspace}");
        let (first_rx, _first_render) = attach_window_title_client(&mut server, 1);
        server.sync_window_title();
        assert_eq!(next_window_title(&first_rx).as_deref(), Some("herd"));

        // A second client taking foreground gets the title even though the
        // title itself has not changed.
        let (second_rx, _second_render) = attach_window_title_client(&mut server, 2);
        server.sync_window_title();
        assert_eq!(next_window_title(&second_rx).as_deref(), Some("herd"));
    }

    #[test]
    fn window_title_is_not_resent_unchanged() {
        let mut server = window_title_test_server("{workspace}");
        let (control_rx, _render_rx) = attach_window_title_client(&mut server, 1);

        server.sync_window_title();
        assert_eq!(next_window_title(&control_rx).as_deref(), Some("herd"));

        server.sync_window_title();
        server.sync_window_title();
        no_window_title(&control_rx);
    }

    #[test]
    fn window_title_is_resent_after_a_client_detaches() {
        let mut server = window_title_test_server("{workspace}");
        let (control_rx, _render_rx) = attach_window_title_client(&mut server, 1);
        server.sync_window_title();
        assert_eq!(next_window_title(&control_rx).as_deref(), Some("herd"));

        // Detach: the client entry stays but loses its writer.
        server.clients.get_mut(&1).expect("client").writer = None;
        server.sync_window_title();
        no_window_title(&control_rx);

        // Reattach with a writer: the cached title must not skip the send.
        let (writer, control_rx, _render_rx) = test_client_writer();
        server.clients.get_mut(&1).expect("client").writer = Some(writer);
        server.sync_window_title();
        assert_eq!(next_window_title(&control_rx).as_deref(), Some("herd"));
    }
}
