use std::cell::Cell;
use std::io;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, Ordering},
    Arc, Mutex,
};

use bytes::Bytes;
use portable_pty::CommandBuilder;
#[cfg(test)]
use portable_pty::{native_pty_system, PtySize};
use ratatui::{layout::Rect, style::Color, Frame};
use tokio::sync::{mpsc, watch, Notify};
use tracing::{debug, error, info, warn};

use crate::detect::{Agent, AgentState};
use crate::events::AppEvent;
use crate::layout::PaneId;
use crate::pty::actor::{PtyIoActor, PtyIoActorConfig, PtyIoActorHandle, PtyReadResult};

mod agent_detection;
mod cursor;
mod input;
mod kitty_keyboard;
mod osc;
mod state;
mod terminal;

use self::agent_detection::{
    decide_detection_screen_read, decide_screen_detection_publish, mark_detection_content_changed,
    observe_detection_content_change, DetectionPublishDecision, DetectionScreenReadDecision,
    DetectionScreenReadInput, PendingIdleConfirmation, ScreenDetectionPublishInput,
    AGENT_PENDING_IDLE_RECHECK, AGENT_STARTUP_GRACE_WINDOW,
};
pub(crate) use self::terminal::TerminalReadSnapshot;
pub(crate) use self::terminal::TerminalViewport;
use self::terminal::{GhosttyPaneTerminal, PaneTerminal};
pub use self::{
    state::PaneState,
    terminal::{
        InputState, ScrollMetrics, TerminalCursorState, TerminalTextMatch, TerminalTextPoint,
        TerminalWordMotion,
    },
};

const RELEASE_REACQUIRE_SUPPRESSION: std::time::Duration = std::time::Duration::from_secs(1);
const PANE_TERM: &str = "xterm-256color";
const PANE_COLORTERM: &str = "truecolor";

pub(crate) type PaneOutputObserver = Arc<dyn Fn(&[u8]) + Send + Sync>;

#[cfg(test)]
thread_local! {
    static AGGREGATE_INPUT_STATE_READS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn take_aggregate_input_state_reads() -> usize {
    AGGREGATE_INPUT_STATE_READS.replace(0)
}

fn apply_pane_terminal_env(cmd: &mut CommandBuilder) {
    // Each pane is rendered by Gardn's own terminal layer, not the outer terminal
    // that launched the app. Advertising the inherited TERM leaks the host terminal
    // identity into shells and across SSH, which breaks redraw and cursor movement
    // when the remote side lacks matching terminfo entries.
    cmd.env("TERM", PANE_TERM);
    cmd.env("COLORTERM", PANE_COLORTERM);
}

#[derive(Clone)]
pub(crate) struct PaneLaunchEnv {
    extra: Vec<(String, String)>,
    identity: Option<PaneLaunchIdentity>,
    include_pane_identity: bool,
    /// When set, replaces the coordinator Local API socket path in the child env.
    /// Execution workers pass an empty string to strip `GARDN_SOCKET_PATH`.
    socket_path_override: Option<String>,
    /// Optional observer invoked for every drained PTY chunk after terminal parse.
    output_observer: Option<PaneOutputObserver>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaneLaunchIdentity {
    workspace_id: String,
    tab_id: String,
    pane_id: String,
}

impl Default for PaneLaunchEnv {
    fn default() -> Self {
        Self::from_extra(Vec::new())
    }
}

impl PaneLaunchEnv {
    pub(crate) fn from_extra(extra: Vec<(String, String)>) -> Self {
        Self {
            extra,
            identity: None,
            include_pane_identity: true,
            socket_path_override: None,
            output_observer: None,
        }
    }

    pub(crate) fn without_pane_identity(mut self) -> Self {
        self.include_pane_identity = false;
        self
    }

    pub(crate) fn with_worker_hook_endpoint(
        mut self,
        socket_path: &Path,
        runtime_token: String,
    ) -> Self {
        const RESERVED_GARDN_IDENTITY_KEYS: [&str; 4] = [
            crate::api::SOCKET_PATH_ENV_VAR,
            "GARDN_WORKSPACE_ID",
            "GARDN_TAB_ID",
            "GARDN_PANE_ID",
        ];
        self.extra.retain(|(key, _)| {
            !crate::product_env::is_alias_of(&RESERVED_GARDN_IDENTITY_KEYS, key)
        });
        crate::product_env::push(
            &mut self.extra,
            crate::integration::GARDN_PANE_ID_ENV_VAR,
            runtime_token,
        );

        self.identity = None;
        self.include_pane_identity = false;
        self.socket_path_override = Some(socket_path.to_string_lossy().into_owned());
        self
    }

    pub(crate) fn with_output_observer(mut self, observer: PaneOutputObserver) -> Self {
        self.output_observer = Some(observer);
        self
    }

    fn output_observer(&self) -> Option<PaneOutputObserver> {
        self.output_observer.clone()
    }

    pub(crate) fn with_identity(
        mut self,
        workspace_id: String,
        tab_id: String,
        pane_id: String,
    ) -> Self {
        self.identity = Some(PaneLaunchIdentity {
            workspace_id,
            tab_id,
            pane_id,
        });
        self
    }

    pub(crate) fn extra(&self) -> &[(String, String)] {
        &self.extra
    }
}

fn apply_pane_launch_env(cmd: &mut CommandBuilder, launch_env: &PaneLaunchEnv, pane_id: PaneId) {
    for (key, value) in &launch_env.extra {
        cmd.env(key, value);
    }
    crate::product_env::apply(cmd, crate::GARDN_ENV_VAR, crate::GARDN_ENV_VALUE);
    match launch_env.socket_path_override.as_deref() {
        Some("") => {
            // Worker-owned launches must not inherit a coordinator Local API socket.
            cmd.env_remove(crate::api::SOCKET_PATH_ENV_VAR);
            if let Some(alias) = crate::product_env::herdr_alias(crate::api::SOCKET_PATH_ENV_VAR) {
                cmd.env_remove(alias);
            }
        }
        Some(path) => {
            crate::product_env::apply(cmd, crate::api::SOCKET_PATH_ENV_VAR, path);
        }
        None => {
            crate::product_env::apply(
                cmd,
                crate::api::SOCKET_PATH_ENV_VAR,
                crate::api::socket_path(),
            );
        }
    }
    if let Some(identity) = &launch_env.identity {
        crate::product_env::apply(cmd, "GARDN_WORKSPACE_ID", &identity.workspace_id);
        crate::product_env::apply(cmd, "GARDN_TAB_ID", &identity.tab_id);
        crate::product_env::apply(cmd, "GARDN_PANE_ID", &identity.pane_id);
    } else if launch_env.include_pane_identity {
        crate::integration::apply_pane_env(cmd, pane_id);
    }
    crate::platform::apply_pane_runtime_marker(cmd);
    cmd.env_remove("CODEX_THREAD_ID");
}

#[derive(Debug, Clone, Copy)]
struct PendingAgentRelease {
    agent: Agent,
    until: std::time::Instant,
}

#[derive(Clone, Default)]
struct SpawnInitialState<'a> {
    detected_agent: Option<Agent>,
    history_ansi: Option<&'a str>,
    windows_powershell_prompt_cwd_reporting: bool,
    resolved_terminal_theme_override: Option<crate::terminal_theme::ResolvedTerminalTheme>,
    output_observer: Option<PaneOutputObserver>,
}

fn active_pending_release(
    pending_release: &Mutex<Option<PendingAgentRelease>>,
    now: std::time::Instant,
) -> Option<Agent> {
    let mut pending_release = pending_release.lock().ok()?;
    match *pending_release {
        Some(pending) if now < pending.until => Some(pending.agent),
        Some(_) => {
            *pending_release = None;
            None
        }
        None => None,
    }
}

fn full_lifecycle_authority_should_skip_screen_detection(
    active: bool,
    process_exited: bool,
    suppressed_agent: Option<Agent>,
) -> bool {
    active && !process_exited && suppressed_agent.is_none()
}

async fn publish_state_changed_event(
    state_events: mpsc::Sender<AppEvent>,
    pane_id: PaneId,
    agent: Option<Agent>,
    state: AgentState,
    visible_blocker: bool,
    visible_idle: bool,
    visible_working: bool,
    process_exited: bool,
    observed_at: std::time::Instant,
) {
    // This runs on the async detector task, not the PTY reader thread.
    // Waiting for queue space here preserves correctness-critical state transitions
    // without blocking pane I/O.
    if let Err(e) = state_events
        .send(AppEvent::StateChanged {
            pane_id,
            agent,
            state,
            visible_blocker,
            visible_idle,
            visible_working,
            process_exited,
            observed_at,
        })
        .await
    {
        warn!(
            pane = pane_id.raw(),
            err = %e,
            "failed to deliver StateChanged event"
        );
    }
}

async fn publish_agent_process_detected_event(
    state_events: mpsc::Sender<AppEvent>,
    pane_id: PaneId,
    agent: Agent,
    observed_at: std::time::Instant,
) {
    if let Err(e) = state_events
        .send(AppEvent::AgentProcessDetected {
            pane_id,
            agent,
            observed_at,
        })
        .await
    {
        warn!(
            pane = pane_id.raw(),
            err = %e,
            "failed to deliver AgentProcessDetected event"
        );
    }
}

fn publish_terminal_bells(pane_id: PaneId, count: u16, events: &mpsc::Sender<AppEvent>) {
    if count == 0 {
        return;
    }
    if let Err(err) = events.try_send(AppEvent::TerminalBell { pane_id, count }) {
        warn!(
            pane = pane_id.raw(),
            count,
            err = %err,
            "failed to queue terminal bell"
        );
    }
}

const AGENT_MISS_CONFIRMATION_ATTEMPTS: u8 = 6;
const STABLE_VISIBLE_SIGNAL_REFRESH: std::time::Duration = std::time::Duration::from_millis(800);

#[derive(Debug, Clone, Copy)]
struct AgentDetectionPresence {
    current_agent: Option<Agent>,
    consecutive_misses: u8,
}

#[cfg(test)]
fn should_clear_agent_for_foreground_shell(
    previous_agent: Option<Agent>,
    new_agent: Option<Agent>,
    foreground_is_pane_shell: bool,
) -> bool {
    previous_agent.is_some() && new_agent.is_none() && foreground_is_pane_shell
}
fn absolute_process_cwd(pid: u32) -> Option<std::path::PathBuf> {
    crate::platform::process_cwd(pid).filter(|cwd| cwd.is_absolute())
}

fn foreground_member_cwd_different_from_shell(
    shell_pid: u32,
    shell_cwd: Option<&std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    let job = crate::detect::foreground_job(shell_pid)?;
    for process in job.processes {
        if process.pid == shell_pid {
            continue;
        }
        let Some(cwd) = absolute_process_cwd(process.pid) else {
            continue;
        };
        if shell_cwd != Some(&cwd) {
            return Some(cwd);
        }
    }
    None
}

#[cfg(any(windows, test))]
fn should_observe_foreground_process_group(
    has_current_agent: bool,
    suppressed_agent: bool,
    pending_foreground_shell_clear: bool,
    pending_restore_probe: bool,
    content_changed: bool,
    elapsed: std::time::Duration,
    recheck: std::time::Duration,
) -> bool {
    !has_current_agent
        || suppressed_agent
        || pending_foreground_shell_clear
        || pending_restore_probe
        || content_changed
        || elapsed >= recheck
}

fn identify_foreground_job_with_hint(
    job: &crate::platform::ForegroundJob,
) -> Option<(Agent, String)> {
    if let Some(agent) = crate::platform::process_agent_hint(job.process_group_id) {
        return Some((agent, crate::detect::agent_label(agent).to_string()));
    }

    if let Some(leader) = job
        .processes
        .iter()
        .find(|process| process.pid == job.process_group_id)
    {
        let leader_job = crate::platform::ForegroundJob {
            process_group_id: job.process_group_id,
            processes: vec![leader.clone()],
        };
        if let Some(identified) = crate::detect::identify_agent_in_job(&leader_job) {
            return Some(identified);
        }
    }

    if let Some(agent) = job
        .processes
        .iter()
        .filter(|process| process.pid != job.process_group_id)
        .find_map(|process| crate::platform::process_agent_hint(process.pid))
    {
        return Some((agent, crate::detect::agent_label(agent).to_string()));
    }

    crate::detect::identify_agent_in_job(job)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ForegroundShellAgentAction {
    ObserveProbe,
    ReportProcessExit,
    ReportReplacementProcess,
    ClearAgent,
}

fn foreground_shell_agent_action(
    previous_agent: Option<Agent>,
    new_agent: Option<Agent>,
    foreground_is_pane_shell: bool,
    process_exit_reported: bool,
) -> ForegroundShellAgentAction {
    let Some(previous_agent) = previous_agent else {
        return ForegroundShellAgentAction::ObserveProbe;
    };
    if process_exit_reported {
        return if new_agent == Some(previous_agent) {
            ForegroundShellAgentAction::ReportReplacementProcess
        } else if new_agent.is_none() {
            ForegroundShellAgentAction::ClearAgent
        } else {
            ForegroundShellAgentAction::ObserveProbe
        };
    }
    if new_agent.is_some() {
        return ForegroundShellAgentAction::ObserveProbe;
    }

    if foreground_is_pane_shell {
        // Do not clear identity immediately. First publish an idle process-exit
        // transition for the previous agent so notifications observe completion.
        return ForegroundShellAgentAction::ReportProcessExit;
    }

    ForegroundShellAgentAction::ObserveProbe
}

fn apply_foreground_shell_agent_action(
    agent_presence: &mut AgentDetectionPresence,
    action: ForegroundShellAgentAction,
    previous_agent: Option<Agent>,
    new_agent: Option<Agent>,
    pending_foreground_shell_clear: &mut bool,
    foreground_shell_exit_reported: &mut bool,
) -> bool {
    match action {
        ForegroundShellAgentAction::ReportReplacementProcess => {
            *pending_foreground_shell_clear = false;
            *foreground_shell_exit_reported = false;
            agent_presence.observe_process_probe(previous_agent);
            true
        }
        ForegroundShellAgentAction::ReportProcessExit => {
            *pending_foreground_shell_clear = true;
            false
        }
        ForegroundShellAgentAction::ClearAgent => {
            *pending_foreground_shell_clear = false;
            *foreground_shell_exit_reported = false;
            agent_presence.clear_current_agent()
        }
        ForegroundShellAgentAction::ObserveProbe => {
            *pending_foreground_shell_clear = false;
            *foreground_shell_exit_reported = false;
            agent_presence.observe_process_probe(new_agent)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DetectionPublishState {
    state: AgentState,
    visible_blocker: bool,
    visible_idle: bool,
    visible_working: bool,
}

fn should_publish_detection_update(
    previous: DetectionPublishState,
    next: DetectionPublishState,
    agent_changed: bool,
    process_exited: bool,
    stable_visible_signal_refresh_due: bool,
) -> bool {
    next.state != previous.state
        || next.visible_blocker != previous.visible_blocker
        || next.visible_idle != previous.visible_idle
        || next.visible_working != previous.visible_working
        || agent_changed
        || process_exited
        || (stable_visible_signal_refresh_due
            && ((next.visible_blocker && previous.visible_blocker)
                || (next.visible_idle && previous.visible_idle)
                || (next.visible_working && previous.visible_working)))
}

fn stable_visible_signal_refresh_due(
    previous: DetectionPublishState,
    next: DetectionPublishState,
    last_refresh: Option<std::time::Instant>,
    now: std::time::Instant,
) -> bool {
    let stable_visible_signal = (next.visible_blocker && previous.visible_blocker)
        || (next.visible_idle && previous.visible_idle)
        || (next.visible_working && previous.visible_working);

    stable_visible_signal
        && last_refresh.is_none_or(|last_refresh| {
            now.duration_since(last_refresh) >= STABLE_VISIBLE_SIGNAL_REFRESH
        })
}

fn spawn_basic_detection_task(
    pane_id: PaneId,
    child_pid: Arc<AtomicU32>,
    terminal: Arc<PaneTerminal>,
    _detection_content_seq: Arc<AtomicU64>,
    full_lifecycle_authority_active: Arc<AtomicBool>,
    state_events: mpsc::Sender<AppEvent>,
) -> (
    tokio::task::AbortHandle,
    Arc<Notify>,
    Arc<Mutex<Option<PendingAgentRelease>>>,
) {
    let detect_reset_notify = Arc::new(Notify::new());
    let detect_reset = detect_reset_notify.clone();
    let pending_release = Arc::new(Mutex::new(None));
    let pending_release_for_task = pending_release.clone();

    let handle = tokio::spawn(async move {
        let mut agent_presence = AgentDetectionPresence::from_agent(None);
        let mut state = AgentState::Unknown;
        let mut last_visible_blocker = false;
        let mut last_visible_idle = false;
        let mut last_visible_working = false;
        let mut last_visible_signal_refresh = None;

        loop {
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(300)) => {}
                _ = detect_reset.notified() => {
                    agent_presence = AgentDetectionPresence::from_agent(None);
                    state = AgentState::Unknown;
                    last_visible_blocker = false;
                    last_visible_idle = false;
                    last_visible_working = false;
                    last_visible_signal_refresh = None;
                }
            }

            let now = std::time::Instant::now();
            let suppressed_agent = active_pending_release(&pending_release_for_task, now);
            if full_lifecycle_authority_active.load(Ordering::Acquire) {
                continue;
            }
            let pid = child_pid.load(Ordering::Acquire);
            let mut agent_changed = false;
            let mut agent = agent_presence.current_agent();

            if pid > 0 {
                let mut new_agent = crate::detect::foreground_job(pid).and_then(|job| {
                    crate::detect::identify_agent_in_job(&job).map(|(agent, _)| agent)
                });
                if let Some(suppressed_agent) = suppressed_agent {
                    if new_agent == Some(suppressed_agent) {
                        new_agent = None;
                    } else if let Ok(mut pending_release) = pending_release_for_task.lock() {
                        *pending_release = None;
                    }
                }
                let previous_agent = agent_presence.current_agent();
                if agent_presence.observe_process_probe(new_agent) {
                    agent = agent_presence.current_agent();
                    agent_changed = previous_agent != agent;
                }
            }

            if agent_changed {
                if let Some(detected) = agent {
                    state = AgentState::Unknown;
                    last_visible_blocker = false;
                    last_visible_idle = false;
                    last_visible_working = false;
                    last_visible_signal_refresh = None;
                    publish_agent_process_detected_event(
                        state_events.clone(),
                        pane_id,
                        detected,
                        now,
                    )
                    .await;
                    continue;
                }
            }
            let content = terminal.detection_text();
            let detection = crate::detect::detect_agent(agent, &content);
            let new_state = detection.state;
            let visible_blocker = detection.visible_blocker && new_state == AgentState::Blocked;
            let visible_idle = detection.visible_idle && new_state == AgentState::Idle;
            let visible_working = detection.visible_working && new_state == AgentState::Working;

            let previous_publish = DetectionPublishState {
                state,
                visible_blocker: last_visible_blocker,
                visible_idle: last_visible_idle,
                visible_working: last_visible_working,
            };
            let next_publish = DetectionPublishState {
                state: new_state,
                visible_blocker,
                visible_idle,
                visible_working,
            };
            let stable_refresh_due = stable_visible_signal_refresh_due(
                previous_publish,
                next_publish,
                last_visible_signal_refresh,
                now,
            );

            if should_publish_detection_update(
                previous_publish,
                next_publish,
                agent_changed,
                false,
                stable_refresh_due,
            ) {
                state = new_state;
                last_visible_blocker = visible_blocker;
                last_visible_idle = visible_idle;
                last_visible_working = visible_working;
                if visible_blocker || visible_idle || visible_working {
                    last_visible_signal_refresh = Some(now);
                } else {
                    last_visible_signal_refresh = None;
                }
                publish_state_changed_event(
                    state_events.clone(),
                    pane_id,
                    agent,
                    new_state,
                    visible_blocker,
                    visible_idle,
                    visible_working,
                    false,
                    now,
                )
                .await;
            }
        }
    });

    (handle.abort_handle(), detect_reset_notify, pending_release)
}

impl AgentDetectionPresence {
    fn from_agent(current_agent: Option<Agent>) -> Self {
        Self {
            current_agent,
            consecutive_misses: 0,
        }
    }

    fn current_agent(&self) -> Option<Agent> {
        self.current_agent
    }

    fn clear_current_agent(&mut self) -> bool {
        if self.current_agent.is_none() {
            self.consecutive_misses = 0;
            return false;
        }
        self.current_agent = None;
        self.consecutive_misses = 0;
        true
    }

    fn observe_process_probe(&mut self, identified_agent: Option<Agent>) -> bool {
        match identified_agent {
            Some(agent) => {
                self.consecutive_misses = 0;
                if Some(agent) == self.current_agent {
                    return false;
                }
                self.current_agent = Some(agent);
                true
            }
            None => {
                if self.current_agent.is_none() {
                    self.consecutive_misses = 0;
                    return false;
                }
                self.consecutive_misses = self.consecutive_misses.saturating_add(1);
                if self.consecutive_misses < AGENT_MISS_CONFIRMATION_ATTEMPTS {
                    return false;
                }
                self.current_agent = None;
                self.consecutive_misses = 0;
                true
            }
        }
    }
}

// ---------------------------------------------------------------------------
// PaneRuntime — PTY, parser, channels, background tasks
// ---------------------------------------------------------------------------

/// PTY runtime for a pane. Owns the terminal, I/O channels, and background tasks.
/// Dropping this shuts down all background tasks and closes the PTY.
pub struct PaneRuntime {
    pane_id: PaneId,
    terminal: Arc<PaneTerminal>,
    io: PaneRuntimeIo,
    current_size: Cell<(u16, u16, u32, u32)>,
    child_pid: Arc<AtomicU32>,
    child_wait_completed: Option<Arc<AtomicBool>>,
    kitty_keyboard_flags: Arc<AtomicU16>,
    content_seq: Arc<AtomicU64>,
    detection_content_seq: Arc<AtomicU64>,
    full_lifecycle_authority_active: Arc<AtomicBool>,
    detect_reset_notify: Arc<Notify>,
    pending_release: Arc<Mutex<Option<PendingAgentRelease>>>,
    preserve_processes_on_drop: bool,
    // Task handles for deterministic shutdown
    detect_handle: Option<tokio::task::AbortHandle>,
}

pub(crate) struct RemotePaneControl {
    input_rx: mpsc::Receiver<Bytes>,
    resize_rx: watch::Receiver<(u16, u16, u32, u32)>,
}

impl RemotePaneControl {
    pub(crate) fn try_recv_input(&mut self) -> Result<Bytes, mpsc::error::TryRecvError> {
        self.input_rx.try_recv()
    }

    pub(crate) fn take_resize(&mut self) -> Option<(u16, u16, u32, u32)> {
        self.resize_rx
            .has_changed()
            .ok()
            .filter(|changed| *changed)
            .map(|_| *self.resize_rx.borrow_and_update())
    }
}

enum PaneRuntimeIo {
    Actor(PtyIoActorHandle),
    Remote {
        sender: mpsc::Sender<Bytes>,
        resize_tx: watch::Sender<(u16, u16, u32, u32)>,
    },
    #[cfg(test)]
    TestChannel {
        sender: mpsc::Sender<Bytes>,
        resize_tx: watch::Sender<(u16, u16, u32, u32)>,
    },
}

impl PaneRuntimeIo {
    fn shutdown(&self) {
        match self {
            PaneRuntimeIo::Actor(actor) => actor.shutdown(),
            PaneRuntimeIo::Remote { .. } => {}
            #[cfg(test)]
            PaneRuntimeIo::TestChannel { .. } => {}
        }
    }

    #[cfg(unix)]
    fn duplicate_handoff_fd(&self) -> std::io::Result<std::os::fd::RawFd> {
        match self {
            PaneRuntimeIo::Actor(actor) => actor.duplicate_for_handoff(),
            PaneRuntimeIo::Remote { .. } => Err(std::io::Error::other(
                "remote runtime has no local PTY master fd",
            )),
            #[cfg(test)]
            PaneRuntimeIo::TestChannel { .. } => {
                Err(std::io::Error::other("test runtime has no PTY master fd"))
            }
        }
    }

    #[cfg(unix)]
    fn foreground_process_group_id(&self) -> Option<u32> {
        match self {
            PaneRuntimeIo::Actor(actor) => actor.foreground_process_group_id(),
            PaneRuntimeIo::Remote { .. } => None,
            #[cfg(test)]
            PaneRuntimeIo::TestChannel { .. } => None,
        }
    }

    #[cfg(unix)]
    fn begin_handoff(&self, timeout: std::time::Duration) -> std::io::Result<()> {
        match self {
            PaneRuntimeIo::Actor(actor) => actor.begin_handoff(timeout),
            PaneRuntimeIo::Remote { .. } => Ok(()),
            #[cfg(test)]
            PaneRuntimeIo::TestChannel { .. } => Ok(()),
        }
    }

    #[cfg(unix)]
    fn set_handoff_paused(&self, paused: bool) -> std::io::Result<()> {
        match self {
            PaneRuntimeIo::Actor(actor) => {
                if paused {
                    actor.begin_handoff(std::time::Duration::from_secs(1))
                } else {
                    actor.rollback_handoff()
                }
            }
            PaneRuntimeIo::Remote { .. } => {
                if paused {
                    Err(std::io::Error::other(
                        "remote runtimes use worker adoption instead of PTY handoff",
                    ))
                } else {
                    Ok(())
                }
            }
            #[cfg(test)]
            PaneRuntimeIo::TestChannel { .. } => Ok(()),
        }
    }

    #[cfg(unix)]
    fn release_after_commit(&self) -> std::io::Result<()> {
        match self {
            PaneRuntimeIo::Actor(actor) => actor.release_after_commit(),
            PaneRuntimeIo::Remote { .. } => Ok(()),
            #[cfg(test)]
            PaneRuntimeIo::TestChannel { .. } => Ok(()),
        }
    }

    fn resize(
        &self,
        rows: u16,
        cols: u16,
        cell_width_px: u32,
        cell_height_px: u32,
        terminal_responses: Vec<Bytes>,
    ) {
        match self {
            PaneRuntimeIo::Actor(actor) => {
                actor.resize(
                    rows,
                    cols,
                    cell_width_px,
                    cell_height_px,
                    terminal_responses,
                );
            }
            PaneRuntimeIo::Remote { resize_tx, .. } => {
                let _ = resize_tx.send((rows, cols, cell_width_px, cell_height_px));
            }
            #[cfg(test)]
            PaneRuntimeIo::TestChannel { resize_tx, .. } => {
                let _ = resize_tx.send((rows, cols, cell_width_px, cell_height_px));
            }
        }
    }

    fn nudge_child_redraw_after_handoff(
        &self,
        rows: u16,
        cols: u16,
        cell_width_px: u32,
        cell_height_px: u32,
    ) {
        match self {
            PaneRuntimeIo::Actor(actor) => {
                actor.nudge_child_redraw_after_handoff(rows, cols, cell_width_px, cell_height_px);
            }
            PaneRuntimeIo::Remote { .. } => {}
            #[cfg(test)]
            PaneRuntimeIo::TestChannel { .. } => {}
        }
    }

    async fn send_bytes(&self, bytes: Bytes) -> Result<(), mpsc::error::SendError<Bytes>> {
        match self {
            PaneRuntimeIo::Actor(actor) => actor.write_user_input(bytes).await,
            PaneRuntimeIo::Remote { sender, .. } => sender.send(bytes).await,
            #[cfg(test)]
            PaneRuntimeIo::TestChannel { sender, .. } => sender.send(bytes).await,
        }
    }

    fn try_send_bytes(&self, bytes: Bytes) -> Result<(), mpsc::error::TrySendError<Bytes>> {
        match self {
            PaneRuntimeIo::Actor(actor) => actor.try_write_user_input(bytes),
            PaneRuntimeIo::Remote { sender, .. } => sender.try_send(bytes),
            #[cfg(test)]
            PaneRuntimeIo::TestChannel { sender, .. } => sender.try_send(bytes),
        }
    }

    fn send_bytes_after(&self, bytes: Bytes, delay: std::time::Duration) {
        match self {
            PaneRuntimeIo::Actor(actor) => {
                let actor = actor.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    if let Err(err) = actor.write_user_input(bytes).await {
                        warn!(error = %err, "failed to send delayed PTY input");
                    }
                });
            }
            PaneRuntimeIo::Remote { sender, .. } => {
                let sender = sender.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    if let Err(err) = sender.send(bytes).await {
                        warn!(error = %err, "failed to send delayed remote PTY input");
                    }
                });
            }
            #[cfg(test)]
            PaneRuntimeIo::TestChannel { sender, .. } => {
                let sender = sender.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(delay).await;
                    let _ = sender.send(bytes).await;
                });
            }
        }
    }
    fn write_terminal_response(&self, response: impl FnOnce() -> Option<Bytes>) {
        match self {
            PaneRuntimeIo::Actor(actor) => actor.write_terminal_response(response),
            PaneRuntimeIo::Remote { sender, .. } => {
                if let Some(bytes) = response() {
                    let _ = sender.try_send(bytes);
                }
            }
            #[cfg(test)]
            PaneRuntimeIo::TestChannel { sender, .. } => {
                if let Some(bytes) = response() {
                    let _ = sender.try_send(bytes);
                }
            }
        }
    }

    fn response_sender(&self) -> Option<mpsc::Sender<Bytes>> {
        match self {
            PaneRuntimeIo::Remote { sender, .. } => Some(sender.clone()),
            PaneRuntimeIo::Actor(_) => None,
            #[cfg(test)]
            PaneRuntimeIo::TestChannel { sender, .. } => Some(sender.clone()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelRouting {
    HostScroll,
    MouseReport,
    AlternateScroll,
}

impl Drop for PaneRuntime {
    fn drop(&mut self) {
        // Abort detection task immediately and terminate the owned session.
        // The PTY actor shuts down before the process/session policy runs.
        if let Some(handle) = &self.detect_handle {
            handle.abort();
        }
        self.io.shutdown();
        if !self.preserve_processes_on_drop {
            shutdown_pane_processes(
                self.pane_id,
                self.child_pid.load(Ordering::Acquire),
                self.child_wait_completed.as_deref(),
            );
        }
    }
}

fn process_alive_for_shutdown(
    pid: u32,
    child_pid: u32,
    child_wait_completed: bool,
    process_exists: impl FnOnce(u32) -> bool,
) -> bool {
    if pid == child_pid && child_wait_completed {
        return false;
    }
    process_exists(pid)
}

fn wait_for_processes_to_exit(
    pids: &[u32],
    child_pid: u32,
    child_wait_completed: Option<&AtomicBool>,
    timeout: std::time::Duration,
) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let child_wait_completed =
            child_wait_completed.is_some_and(|flag| flag.load(Ordering::Acquire));
        if pids.iter().all(|pid| {
            !process_alive_for_shutdown(
                *pid,
                child_pid,
                child_wait_completed,
                crate::platform::process_exists,
            )
        }) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn shutdown_pane_processes(
    pane_id: PaneId,
    child_pid: u32,
    child_wait_completed: Option<&AtomicBool>,
) {
    if child_pid == 0 {
        return;
    }

    let mut pids = crate::platform::session_processes(child_pid);
    if pids.is_empty() {
        pids.push(child_pid);
    }
    pids.sort_unstable();
    pids.dedup();

    for (signal, grace) in [
        (
            crate::platform::Signal::Hangup,
            std::time::Duration::from_millis(250),
        ),
        (
            crate::platform::Signal::Terminate,
            std::time::Duration::from_millis(250),
        ),
        (
            crate::platform::Signal::Kill,
            std::time::Duration::from_millis(250),
        ),
    ] {
        crate::platform::signal_processes(&pids, signal);
        if wait_for_processes_to_exit(&pids, child_pid, child_wait_completed, grace) {
            info!(
                pane = pane_id.raw(),
                pid = child_pid,
                ?signal,
                "pane session terminated"
            );
            return;
        }
    }

    warn!(
        pane = pane_id.raw(),
        pid = child_pid,
        pids = ?pids,
        "pane session still alive after forced shutdown"
    );
}

#[cfg(unix)]
fn truncate_handoff_history(history: String, max_bytes: usize) -> String {
    if history.len() <= max_bytes {
        return history;
    }
    let mut start = history.len().saturating_sub(max_bytes);
    while !history.is_char_boundary(start) {
        start += 1;
    }
    let Some(newline_offset) = history[start..].find('\n') else {
        return String::new();
    };
    start += newline_offset + 1;
    history[start..].to_owned()
}

fn pane_shell_for_target(configured_shell: &str, target_is_windows: bool) -> String {
    pane_shell_from_parts(
        configured_shell,
        std::env::var("SHELL").ok(),
        std::env::var("COMSPEC").ok(),
        target_is_windows,
    )
}

#[cfg(test)]
fn pane_shell_from(configured_shell: &str, env_shell: Option<String>) -> String {
    pane_shell_from_parts(configured_shell, env_shell, None, false)
}

fn pane_shell_from_parts(
    configured_shell: &str,
    env_shell: Option<String>,
    env_comspec: Option<String>,
    target_is_windows: bool,
) -> String {
    let configured_shell = configured_shell.trim();
    if !configured_shell.is_empty() {
        return configured_shell.to_string();
    }

    let env_default = if target_is_windows {
        env_comspec.or(env_shell)
    } else {
        env_shell
    };

    env_default
        .map(|shell| shell.trim().to_string())
        .filter(|shell| !shell.is_empty())
        .unwrap_or_else(|| default_shell_for_target(target_is_windows).into())
}

fn default_shell_for_target(target_is_windows: bool) -> &'static str {
    if target_is_windows {
        "cmd.exe"
    } else {
        "/bin/sh"
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PaneShellConfig<'a> {
    pub(crate) default_shell: &'a str,
    pub(crate) mode: crate::config::ShellModeConfig,
}

impl<'a> PaneShellConfig<'a> {
    pub(crate) fn new(default_shell: &'a str, mode: crate::config::ShellModeConfig) -> Self {
        Self {
            default_shell,
            mode,
        }
    }
}

fn shell_mode_uses_login_shell(
    mode: crate::config::ShellModeConfig,
    target_is_macos: bool,
    target_is_windows: bool,
) -> bool {
    if target_is_windows {
        return false;
    }
    match mode {
        crate::config::ShellModeConfig::Auto => target_is_macos,
        crate::config::ShellModeConfig::Login => true,
        crate::config::ShellModeConfig::NonLogin => false,
    }
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn resolve_shell_for_login_mode(shell: &str) -> io::Result<String> {
    if shell.contains(std::path::MAIN_SEPARATOR) {
        let path = Path::new(shell);
        return is_executable_file(path)
            .then(|| shell.to_string())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("login shell {shell:?} is not executable"),
                )
            });
    }

    std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(shell))
                .find(|candidate| is_executable_file(candidate))
        })
        .and_then(|path| path.into_os_string().into_string().ok())
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("login shell {shell:?} was not found on PATH"),
            )
        })
}

/// PowerShell prompt wrapper used on native Windows panes to report the
/// current filesystem location via OSC 9;9.
pub(crate) const WINDOWS_POWERSHELL_SHELL_INTEGRATION_COMMAND: &str = r"if ($null -eq $global:__GardnOriginalPrompt) { $global:__GardnOriginalPrompt = $function:prompt; function global:prompt { $out = @(& $global:__GardnOriginalPrompt) -join ' '; $loc = $ExecutionContext.SessionState.Path.CurrentLocation; if ($loc.Provider.Name -eq 'FileSystem') { $esc = [string][char]27; $out += $esc + ']9;9;' + $loc.ProviderPath + $esc + '\' }; $out } }";

pub(crate) fn uses_windows_powershell_pane_shell(shell_config: PaneShellConfig<'_>) -> bool {
    uses_windows_powershell_pane_shell_for_target(shell_config, cfg!(windows))
}

fn uses_windows_powershell_pane_shell_for_target(
    shell_config: PaneShellConfig<'_>,
    target_is_windows: bool,
) -> bool {
    target_is_windows
        && !matches!(shell_config.mode, crate::config::ShellModeConfig::Login)
        && is_powershell_shell(&pane_shell_for_target(
            shell_config.default_shell,
            target_is_windows,
        ))
}

fn is_powershell_shell(shell: &str) -> bool {
    let name = shell.rsplit(['/', '\\']).next().unwrap_or(shell);
    matches!(
        name.to_ascii_lowercase().as_str(),
        "powershell" | "powershell.exe" | "pwsh" | "pwsh.exe"
    )
}

fn pane_shell_command_builder_for_target(
    shell_config: PaneShellConfig<'_>,
    target_is_macos: bool,
    target_is_windows: bool,
) -> io::Result<CommandBuilder> {
    let shell = pane_shell_for_target(shell_config.default_shell, target_is_windows);
    if shell_mode_uses_login_shell(shell_config.mode, target_is_macos, target_is_windows) {
        let mut cmd = CommandBuilder::new_default_prog();
        cmd.env("SHELL", resolve_shell_for_login_mode(&shell)?);
        Ok(cmd)
    } else {
        let mut cmd = CommandBuilder::new(&shell);
        if uses_windows_powershell_pane_shell_for_target(shell_config, target_is_windows) {
            cmd.args([
                "-NoExit",
                "-Command",
                WINDOWS_POWERSHELL_SHELL_INTEGRATION_COMMAND,
            ]);
        }
        Ok(cmd)
    }
}

fn pane_shell_command_builder(shell_config: PaneShellConfig<'_>) -> io::Result<CommandBuilder> {
    pane_shell_command_builder_for_target(shell_config, cfg!(target_os = "macos"), cfg!(windows))
}

fn shell_command_builder_for_target(
    shell_config: Option<PaneShellConfig<'_>>,
    command: &str,
    target_is_windows: bool,
) -> io::Result<CommandBuilder> {
    if target_is_windows {
        let shell = pane_shell_for_target(
            shell_config
                .map(|config| config.default_shell)
                .unwrap_or(""),
            target_is_windows,
        );
        let mut cmd = CommandBuilder::new(&shell);
        append_windows_shell_command_args(&mut cmd, &shell, command);
        return Ok(cmd);
    }

    match shell_config {
        Some(shell_config) => {
            let shell = pane_shell_for_target(shell_config.default_shell, target_is_windows);
            let mut cmd = CommandBuilder::new(resolve_shell_for_login_mode(&shell)?);
            cmd.arg("-lic");
            cmd.arg(command);
            Ok(cmd)
        }
        None => {
            let mut cmd = CommandBuilder::new("/bin/sh");
            cmd.arg("-c");
            cmd.arg(command);
            Ok(cmd)
        }
    }
}

fn append_windows_shell_command_args(cmd: &mut CommandBuilder, shell: &str, command: &str) {
    let shell_name = Path::new(shell)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(shell)
        .to_ascii_lowercase();
    if matches!(
        shell_name.as_str(),
        "powershell.exe" | "powershell" | "pwsh.exe" | "pwsh"
    ) {
        cmd.arg("-NoLogo");
        cmd.arg("-NoProfile");
        cmd.arg("-Command");
        cmd.arg(command);
    } else {
        cmd.arg("/C");
        cmd.arg(command);
    }
}

impl PaneRuntime {
    pub fn shutdown(mut self) {
        if let Some(handle) = &self.detect_handle {
            handle.abort();
        }
        self.io.shutdown();
        shutdown_pane_processes(
            self.pane_id,
            self.child_pid.load(Ordering::Acquire),
            self.child_wait_completed.as_deref(),
        );
        self.preserve_processes_on_drop = true;
    }

    #[cfg(unix)]
    pub fn duplicate_handoff_fd(&self) -> std::io::Result<std::os::fd::RawFd> {
        self.io.duplicate_handoff_fd()
    }

    #[cfg(unix)]
    pub fn preserve_for_handoff(mut self) {
        if let Err(err) = self.io.release_after_commit() {
            warn!(
                pane = self.pane_id.raw(),
                err = %err,
                "failed to release PTY actor after handoff commit; dropping runtime will still close the actor handle"
            );
        }
        if let Some(handle) = &self.detect_handle {
            handle.abort();
        }
        self.preserve_processes_on_drop = true;
    }

    #[cfg(unix)]
    pub fn assume_handoff_ownership(&mut self) {
        self.preserve_processes_on_drop = false;
    }

    #[cfg(unix)]
    pub fn set_handoff_reader_paused(&self, paused: bool) {
        if let Err(err) = self.io.set_handoff_paused(paused) {
            warn!(
                pane = self.pane_id.raw(),
                err = %err,
                paused,
                "failed to update PTY actor handoff pause state"
            );
        }
    }

    #[cfg(unix)]
    pub fn pause_handoff_reader(&self, timeout: std::time::Duration) -> std::io::Result<()> {
        self.io.begin_handoff(timeout)
    }

    #[cfg(unix)]
    pub fn handoff_runtime_state(
        &self,
        pane_id: u32,
    ) -> crate::handoff_runtime::HandoffRuntimeState {
        let child_pid = self.child_pid.load(Ordering::Acquire);
        let (rows, cols, cell_width_px, cell_height_px) = self.current_size.get();
        crate::handoff_runtime::HandoffRuntimeState {
            pane_id,
            child_pid,
            rows,
            cols,
            cell_width_px,
            cell_height_px,
            keyboard_protocol_flags: match self.keyboard_protocol() {
                crate::input::KeyboardProtocol::Legacy => 0,
                crate::input::KeyboardProtocol::Kitty { flags } => flags,
            },
            keyboard_protocol_ansi: self.terminal.kitty_keyboard_state_ansi(),
            input_state: self.input_state(),
            initial_history_ansi: None,
        }
    }

    #[cfg(unix)]
    pub fn handoff_history_ansi(&self) -> Option<String> {
        if self
            .terminal
            .input_state()
            .is_some_and(|input_state| input_state.alternate_screen)
        {
            return None;
        }
        self.snapshot_history().map(|history| {
            truncate_handoff_history(history, crate::server::handoff::MAX_REPLAY_BYTES_PER_PANE)
        })
    }

    pub fn apply_host_terminal_theme(&self, theme: crate::terminal_theme::TerminalTheme) {
        self.terminal.apply_host_terminal_theme(theme);
    }
    pub fn set_resolved_terminal_theme_override(
        &self,
        theme: Option<crate::terminal_theme::ResolvedTerminalTheme>,
    ) {
        self.terminal.set_resolved_terminal_theme_override(theme);
    }
    pub fn apply_host_terminal_appearance(
        &self,
        appearance: Option<crate::terminal_theme::ThemeAppearance>,
    ) {
        self.io
            .write_terminal_response(|| self.terminal.apply_host_terminal_appearance(appearance));
    }

    pub fn child_pid(&self) -> u32 {
        self.child_pid.load(Ordering::Acquire)
    }

    pub(crate) fn instance_key(&self) -> usize {
        Arc::as_ptr(&self.terminal) as usize
    }

    pub(crate) fn remote(
        pane_id: PaneId,
        rows: u16,
        cols: u16,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        events: mpsc::Sender<AppEvent>,
    ) -> std::io::Result<(Self, RemotePaneControl)> {
        let (input_tx, input_rx) = mpsc::channel::<Bytes>(256);
        let (resize_tx, resize_rx) = watch::channel((rows, cols, 0, 0));
        let mut terminal = crate::ghostty::Terminal::new(cols, rows, scrollback_limit_bytes)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        if crate::kitty_graphics::is_enabled() {
            terminal
                .enable_kitty_graphics(false)
                .map_err(|error| std::io::Error::other(error.to_string()))?;
        }
        let pane_terminal = GhosttyPaneTerminal::new(terminal, input_tx.clone())?;
        pane_terminal.apply_host_terminal_theme(host_terminal_theme);
        pane_terminal.apply_host_terminal_appearance(host_terminal_theme.appearance());

        let terminal = Arc::new(PaneTerminal::new(pane_terminal));
        let child_pid = Arc::new(AtomicU32::new(0));
        let content_seq = Arc::new(AtomicU64::new(0));
        let detection_content_seq = Arc::new(AtomicU64::new(0));
        let full_lifecycle_authority_active = Arc::new(AtomicBool::new(false));
        let (detect_handle, detect_reset_notify, pending_release) = spawn_basic_detection_task(
            pane_id,
            child_pid.clone(),
            terminal.clone(),
            detection_content_seq.clone(),
            full_lifecycle_authority_active.clone(),
            events,
        );
        Ok((
            Self {
                pane_id,
                terminal,
                io: PaneRuntimeIo::Remote {
                    sender: input_tx,
                    resize_tx,
                },
                current_size: Cell::new((rows, cols, 0, 0)),
                child_pid,
                child_wait_completed: None,
                kitty_keyboard_flags: Arc::new(AtomicU16::new(0)),
                content_seq,
                detection_content_seq,
                full_lifecycle_authority_active,
                detect_reset_notify,
                pending_release,
                preserve_processes_on_drop: true,
                detect_handle: Some(detect_handle),
            },
            RemotePaneControl {
                input_rx,
                resize_rx,
            },
        ))
    }

    pub(crate) fn process_remote_output(&self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let Some(response_writer) = self.io.response_sender() else {
            return Vec::new();
        };
        observe_detection_content_change(bytes, &self.detection_content_seq);
        self.content_seq.fetch_add(1, Ordering::AcqRel);
        let writes = self
            .terminal
            .process_pty_bytes(self.pane_id, 0, bytes, &response_writer)
            .clipboard_writes;
        self.content_seq.fetch_add(1, Ordering::Release);
        writes
    }

    pub fn spawn(
        pane_id: PaneId,
        rows: u16,
        cols: u16,
        cwd: std::path::PathBuf,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        shell_config: PaneShellConfig<'_>,
        launch_env: &PaneLaunchEnv,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
    ) -> std::io::Result<Self> {
        Self::spawn_with_initial_history(
            pane_id,
            rows,
            cols,
            cwd,
            scrollback_limit_bytes,
            host_terminal_theme,
            shell_config,
            launch_env,
            None,
            events,
            render_notify,
            render_dirty,
        )
    }

    // Initial-history spawn shares the base spawn contract plus restore bytes.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spawn_with_initial_history(
        pane_id: PaneId,
        rows: u16,
        cols: u16,
        cwd: std::path::PathBuf,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        shell_config: PaneShellConfig<'_>,
        launch_env: &PaneLaunchEnv,
        initial_history_ansi: Option<&str>,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
    ) -> std::io::Result<Self> {
        let mut cmd = pane_shell_command_builder(shell_config)?;
        cmd.cwd(cwd);
        apply_pane_terminal_env(&mut cmd);
        apply_pane_launch_env(&mut cmd, launch_env, pane_id);
        Self::spawn_command_builder(
            pane_id,
            rows,
            cols,
            scrollback_limit_bytes,
            host_terminal_theme,
            events,
            render_notify,
            render_dirty,
            cmd,
            "failed to spawn shell",
            SpawnInitialState {
                detected_agent: None,
                history_ansi: initial_history_ansi,
                windows_powershell_prompt_cwd_reporting: uses_windows_powershell_pane_shell(
                    shell_config,
                ),
                resolved_terminal_theme_override: None,
                output_observer: launch_env.output_observer(),
            },
        )
    }

    // Profile commands need the same spawn context shape as normal panes plus
    // an explicit command string while preserving the pane launch identity/env.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_profile_command(
        pane_id: PaneId,
        rows: u16,
        cols: u16,
        cwd: std::path::PathBuf,
        shell_config: PaneShellConfig<'_>,
        command: &str,
        launch_env: &PaneLaunchEnv,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
    ) -> std::io::Result<Self> {
        let mut cmd = shell_command_builder_for_target(Some(shell_config), command, cfg!(windows))?;
        cmd.cwd(cwd);
        apply_pane_terminal_env(&mut cmd);
        apply_pane_launch_env(&mut cmd, launch_env, pane_id);
        Self::spawn_command_builder(
            pane_id,
            rows,
            cols,
            scrollback_limit_bytes,
            host_terminal_theme,
            events,
            render_notify,
            render_dirty,
            cmd,
            "failed to spawn agent profile command pane",
            SpawnInitialState {
                output_observer: launch_env.output_observer(),
                ..SpawnInitialState::default()
            },
        )
    }

    pub fn spawn_shell_command(
        pane_id: PaneId,
        rows: u16,
        cols: u16,
        cwd: std::path::PathBuf,
        command: &str,
        launch_env: &PaneLaunchEnv,
        scrollback_limit_bytes: usize,
        terminal_theme: crate::terminal_theme::PaneTerminalTheme,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
    ) -> std::io::Result<Self> {
        let mut cmd = shell_command_builder_for_target(None, command, cfg!(windows))?;
        cmd.cwd(cwd);
        apply_pane_terminal_env(&mut cmd);
        apply_pane_launch_env(&mut cmd, launch_env, pane_id);
        Self::spawn_command_builder(
            pane_id,
            rows,
            cols,
            scrollback_limit_bytes,
            terminal_theme.host,
            events,
            render_notify,
            render_dirty,
            cmd,
            "failed to spawn command pane",
            SpawnInitialState {
                resolved_terminal_theme_override: terminal_theme.resolved_override,
                output_observer: launch_env.output_observer(),
                ..SpawnInitialState::default()
            },
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn spawn_custom_command(
        pane_id: PaneId,
        rows: u16,
        cols: u16,
        cwd: std::path::PathBuf,
        command: &str,
        launch_env: &PaneLaunchEnv,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
    ) -> std::io::Result<Self> {
        let mut cmd = crate::platform::pane_custom_command_pty_builder(command);
        cmd.cwd(cwd);
        apply_pane_terminal_env(&mut cmd);
        apply_pane_launch_env(&mut cmd, launch_env, pane_id);
        Self::spawn_command_builder(
            pane_id,
            rows,
            cols,
            scrollback_limit_bytes,
            host_terminal_theme,
            events,
            render_notify,
            render_dirty,
            cmd,
            "failed to spawn custom command pane",
            SpawnInitialState {
                output_observer: launch_env.output_observer(),
                ..SpawnInitialState::default()
            },
        )
    }

    pub fn spawn_argv_command(
        pane_id: PaneId,
        rows: u16,
        cols: u16,
        cwd: std::path::PathBuf,
        argv: &[String],
        launch_env: &PaneLaunchEnv,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
    ) -> std::io::Result<Self> {
        let Some((program, args)) = argv.split_first() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "argv must not be empty",
            ));
        };
        let mut cmd = CommandBuilder::new(program);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.cwd(cwd);
        apply_pane_terminal_env(&mut cmd);
        apply_pane_launch_env(&mut cmd, launch_env, pane_id);
        Self::spawn_command_builder(
            pane_id,
            rows,
            cols,
            scrollback_limit_bytes,
            host_terminal_theme,
            events,
            render_notify,
            render_dirty,
            cmd,
            "failed to spawn argv command pane",
            SpawnInitialState {
                output_observer: launch_env.output_observer(),
                ..SpawnInitialState::default()
            },
        )
    }

    #[cfg(unix)]
    pub fn from_handoff_fd(
        import: crate::handoff_runtime::ImportedHandoffRuntime,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
    ) -> std::io::Result<Self> {
        let crate::handoff_runtime::ImportedHandoffRuntime { master_fd, state } = import;
        let crate::handoff_runtime::HandoffRuntimeState {
            pane_id,
            child_pid,
            rows,
            cols,
            cell_width_px,
            cell_height_px,
            keyboard_protocol_flags,
            keyboard_protocol_ansi,
            input_state,
            initial_history_ansi,
        } = state;
        let pane_id = PaneId::from_raw(pane_id);
        use std::os::fd::FromRawFd;

        let master_fd = unsafe { std::os::fd::OwnedFd::from_raw_fd(master_fd) };

        let (response_tx, _response_rx) = mpsc::channel::<Bytes>(1);
        let mut terminal = crate::ghostty::Terminal::new(cols, rows, scrollback_limit_bytes)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        if crate::kitty_graphics::is_enabled() {
            terminal
                .enable_kitty_graphics(true)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
        }
        let pane_terminal = GhosttyPaneTerminal::new(terminal, response_tx.clone())?;
        pane_terminal.apply_host_terminal_theme(host_terminal_theme);
        pane_terminal.apply_host_terminal_appearance(host_terminal_theme.appearance());

        let content_seq = Arc::new(AtomicU64::new(0));
        let detection_content_seq = Arc::new(AtomicU64::new(0));
        if let Some(input_state) = input_state {
            pane_terminal.seed_handoff_input_state(input_state);
        }
        if let Some(ansi) = keyboard_protocol_ansi.as_deref() {
            pane_terminal.seed_keyboard_protocol_ansi(ansi);
        } else {
            pane_terminal.seed_keyboard_protocol_flags(keyboard_protocol_flags);
        }
        if let Some(ansi) = initial_history_ansi.as_deref() {
            pane_terminal.seed_history_ansi(ansi);
        }
        let terminal = Arc::new(PaneTerminal::new(pane_terminal));
        let child_pid = Arc::new(AtomicU32::new(child_pid));
        let kitty_keyboard_flags = Arc::new(AtomicU16::new(keyboard_protocol_flags));

        let io = {
            let terminal = terminal.clone();
            let response_writer = response_tx.clone();
            let render_notify = render_notify.clone();
            let render_dirty = render_dirty.clone();
            let child_pid = child_pid.clone();
            let exit_child_pid = child_pid.clone();
            let read_events = events.clone();
            let content_seq = content_seq.clone();
            let detection_content_seq = detection_content_seq.clone();
            let rt = tokio::runtime::Handle::current();
            let delay_rt = rt.clone();
            let on_read = Box::new(move |bytes: &[u8]| {
                observe_detection_content_change(bytes, &detection_content_seq);
                content_seq.fetch_add(1, Ordering::AcqRel);
                let shell_pid = child_pid.load(Ordering::Acquire);
                let result =
                    terminal.process_pty_bytes(pane_id, shell_pid, bytes, &response_writer);
                content_seq.fetch_add(1, Ordering::Release);
                if result.request_render && render_dirty.request_pty(pane_id) {
                    render_notify.notify_one();
                }
                if let Some(delay) = result.render_delay {
                    let render_notify = render_notify.clone();
                    let render_dirty = render_dirty.clone();
                    delay_rt.spawn(async move {
                        tokio::time::sleep(delay).await;
                        if render_dirty.request_pty(pane_id) {
                            render_notify.notify_one();
                        }
                    });
                }
                for content in result.clipboard_writes {
                    if let Err(err) =
                        read_events.try_send(AppEvent::TerminalClipboardWrite { pane_id, content })
                    {
                        warn!(
                            pane = pane_id.raw(),
                            err = %err,
                            "failed to queue OSC 52 clipboard write"
                        );
                    }
                }
                publish_terminal_bells(pane_id, result.terminal_bells, &read_events);
                PtyReadResult {
                    terminal_responses: result.terminal_responses,
                }
            });
            let exit_events = events.clone();
            let on_reader_exit = Box::new(move || {
                let _ = rt.block_on(exit_events.send(AppEvent::PaneDied {
                    pane_id,
                    child_pid: exit_child_pid.load(Ordering::Acquire),
                    exit_success: false,
                    exit_code: Some(1),
                    exit_signal: None,
                }));
                debug!(pane = pane_id.raw(), "handoff PTY actor exiting");
            });
            PaneRuntimeIo::Actor(PtyIoActor::spawn(PtyIoActorConfig {
                pane_id: pane_id.raw(),
                master_fd,
                initially_quiesced: true,
                on_read,
                on_reader_exit: Some(on_reader_exit),
            })?)
        };

        let full_lifecycle_authority_active = Arc::new(AtomicBool::new(false));
        let (detect_handle, detect_reset_notify, pending_release) = spawn_basic_detection_task(
            pane_id,
            child_pid.clone(),
            terminal.clone(),
            detection_content_seq.clone(),
            full_lifecycle_authority_active.clone(),
            events,
        );

        Ok(Self {
            pane_id,
            terminal,
            io,
            current_size: Cell::new((rows, cols, cell_width_px, cell_height_px)),
            child_pid,
            child_wait_completed: None,
            kitty_keyboard_flags,
            content_seq,
            detection_content_seq,
            full_lifecycle_authority_active,
            detect_reset_notify,
            pending_release,
            preserve_processes_on_drop: true,
            detect_handle: Some(detect_handle),
        })
    }

    fn spawn_command_builder(
        pane_id: PaneId,
        rows: u16,
        cols: u16,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
        cmd: CommandBuilder,
        spawn_error_message: &'static str,
        initial_state: SpawnInitialState<'_>,
    ) -> std::io::Result<Self> {
        crate::logging::pane_spawn_started(pane_id.raw(), rows, cols, scrollback_limit_bytes);

        let (response_tx, _response_rx) = mpsc::channel::<Bytes>(1);
        let mut terminal = crate::ghostty::Terminal::new(cols, rows, scrollback_limit_bytes)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        if crate::kitty_graphics::is_enabled() {
            terminal
                .enable_kitty_graphics(true)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
        }
        let content_seq = Arc::new(AtomicU64::new(0));
        let detection_content_seq = Arc::new(AtomicU64::new(0));
        let pane_terminal = GhosttyPaneTerminal::new(terminal, response_tx.clone())?;
        pane_terminal.apply_host_terminal_theme(host_terminal_theme);
        pane_terminal.apply_host_terminal_appearance(host_terminal_theme.appearance());

        if let Some(theme) = initial_state.resolved_terminal_theme_override {
            pane_terminal.set_resolved_terminal_theme_override(Some(theme));
        }
        pane_terminal.set_windows_powershell_prompt_cwd_reporting(
            initial_state.windows_powershell_prompt_cwd_reporting,
        );
        if let Some(ansi) = initial_state.history_ansi {
            pane_terminal.seed_history_ansi(ansi);
        }
        let terminal = Arc::new(PaneTerminal::new(pane_terminal));
        let kitty_keyboard_flags = Arc::new(AtomicU16::new(0));
        let output_observer = initial_state.output_observer.clone();

        let spawned = crate::pty::backend::spawn_with_portable_pty(rows, cols, cmd)
            .inspect_err(|err| error!(pane = pane_id.raw(), err = %err, "{spawn_error_message}"))?;

        // --- Child watcher task ---
        let child_pid = Arc::new(AtomicU32::new(0));
        let child_wait_completed = Arc::new(AtomicBool::new(false));
        {
            let child_pid = child_pid.clone();
            let child_wait_completed = child_wait_completed.clone();
            let events = events.clone();
            let rt = tokio::runtime::Handle::current();
            let mut child = spawned.child;
            if let Some(pid) = child.process_id() {
                child_pid.store(pid, Ordering::Release);
                crate::logging::pane_spawned(pane_id.raw(), pid);
            }
            tokio::task::spawn_blocking(move || {
                let (exit_success, exit_code, exit_signal) = match child.wait() {
                    Ok(status) => {
                        let status_parts = exit_status_parts(&status);
                        let status_text = format!("{status:?}");
                        crate::logging::pane_exited(pane_id.raw(), &status_text);
                        (status.success(), status_parts.0, status_parts.1)
                    }
                    Err(e) => {
                        crate::logging::pane_exit_failed(pane_id.raw(), &e.to_string());
                        (false, Some(1), None)
                    }
                };
                child_wait_completed.store(true, Ordering::Release);
                // Use blocking send — PaneDied is critical, must not be dropped
                if let Err(e) = rt.block_on(events.send(AppEvent::PaneDied {
                    pane_id,
                    child_pid: child_pid.load(Ordering::Acquire),
                    exit_success,
                    exit_code,
                    exit_signal,
                })) {
                    error!(pane = pane_id.raw(), err = %e, "failed to send PaneDied event");
                }
            });
        }

        let io = {
            let terminal = terminal.clone();
            let response_writer = response_tx.clone();
            let render_notify = render_notify.clone();
            let render_dirty = render_dirty.clone();
            let child_pid = child_pid.clone();
            let events = events.clone();
            let rt = tokio::runtime::Handle::current();
            let content_seq = content_seq.clone();
            let detection_content_seq = detection_content_seq.clone();
            let output_observer = output_observer.clone();
            let on_read = Box::new(move |bytes: &[u8]| {
                observe_detection_content_change(bytes, &detection_content_seq);
                content_seq.fetch_add(1, Ordering::AcqRel);
                let shell_pid = child_pid.load(Ordering::Acquire);
                let result =
                    terminal.process_pty_bytes(pane_id, shell_pid, bytes, &response_writer);
                content_seq.fetch_add(1, Ordering::Release);
                if let Some(observer) = output_observer.as_ref() {
                    observer(bytes);
                }
                if result.request_render && render_dirty.request_pty(pane_id) {
                    render_notify.notify_one();
                }
                if let Some(delay) = result.render_delay {
                    let render_notify = render_notify.clone();
                    let render_dirty = render_dirty.clone();
                    rt.spawn(async move {
                        tokio::time::sleep(delay).await;
                        if render_dirty.request_pty(pane_id) {
                            render_notify.notify_one();
                        }
                    });
                }
                for content in result.clipboard_writes {
                    if let Err(err) =
                        events.try_send(AppEvent::TerminalClipboardWrite { pane_id, content })
                    {
                        warn!(
                            pane = pane_id.raw(),
                            err = %err,
                            "failed to send OSC 52 clipboard write"
                        );
                    }
                }
                publish_terminal_bells(pane_id, result.terminal_bells, &events);
                PtyReadResult {
                    terminal_responses: result.terminal_responses,
                }
            });
            #[cfg(unix)]
            {
                PaneRuntimeIo::Actor(PtyIoActor::spawn(PtyIoActorConfig {
                    pane_id: pane_id.raw(),
                    master_fd: spawned.master_fd,
                    initially_quiesced: false,
                    on_read,
                    on_reader_exit: None,
                })?)
            }
            #[cfg(windows)]
            {
                PaneRuntimeIo::Actor(PtyIoActor::spawn(PtyIoActorConfig {
                    pane_id: pane_id.raw(),
                    master: spawned.master,
                    initially_quiesced: false,
                    on_read,
                    on_reader_exit: None,
                })?)
            }
        };

        // --- Detection task ---
        let full_lifecycle_authority_active = Arc::new(AtomicBool::new(false));
        let (detect_handle, detect_reset_notify, pending_release) = {
            use crate::detect;
            use std::time::{Duration, Instant};

            const TICK_UNIDENTIFIED: Duration = Duration::from_millis(500);
            const TICK_IDENTIFIED: Duration = Duration::from_millis(300);
            const TICK_PENDING_RELEASE: Duration = Duration::from_millis(50);
            const PROCESS_RECHECK: Duration = Duration::from_secs(5);

            let child_pid = child_pid.clone();
            let terminal = terminal.clone();
            let state_events = events.clone();
            let render_notify = render_notify.clone();
            let render_dirty = render_dirty.clone();
            let detection_content_seq_for_task = detection_content_seq.clone();
            let full_lifecycle_authority_active_for_task = full_lifecycle_authority_active.clone();
            let detect_reset_notify = Arc::new(Notify::new());
            let detect_reset = detect_reset_notify.clone();
            let pending_release = Arc::new(Mutex::new(None));
            let pending_release_for_task = pending_release.clone();

            let handle = tokio::spawn(async move {
                let mut agent_presence =
                    AgentDetectionPresence::from_agent(initial_state.detected_agent);
                let mut state = if initial_state.detected_agent.is_some() {
                    AgentState::Idle
                } else {
                    AgentState::Unknown
                };
                let mut last_process_check = Instant::now();
                let mut last_foreground_pgid = None;
                let mut pending_foreground_shell_clear = false;
                let mut foreground_shell_exit_reported = false;
                let mut pending_restore_probe = initial_state.detected_agent.is_some();
                let mut last_claude_working_at = None;
                let mut last_visible_blocker = false;
                let mut last_visible_idle = false;
                let mut last_visible_working = false;
                let mut last_visible_signal_refresh = None;
                let mut pending_idle = PendingIdleConfirmation::default();
                let mut last_screen_scan_detection_content_seq = None;
                let full_lifecycle_authority_active = full_lifecycle_authority_active_for_task;
                let mut agent_startup_grace_until = initial_state
                    .detected_agent
                    .map(|_| Instant::now() + AGENT_STARTUP_GRACE_WINDOW);

                tokio::time::sleep(Duration::from_millis(50)).await;

                loop {
                    let tick = if active_pending_release(&pending_release_for_task, Instant::now())
                        .is_some()
                        || terminal.has_transient_default_color_override()
                    {
                        TICK_PENDING_RELEASE
                    } else if pending_idle.active() {
                        AGENT_PENDING_IDLE_RECHECK
                    } else if agent_presence.current_agent().is_none() {
                        TICK_UNIDENTIFIED
                    } else {
                        TICK_IDENTIFIED
                    };
                    tokio::select! {
                        _ = tokio::time::sleep(tick) => {}
                        _ = detect_reset.notified() => {
                            agent_presence = AgentDetectionPresence::from_agent(None);
                            state = AgentState::Unknown;
                            last_foreground_pgid = None;
                            pending_foreground_shell_clear = false;
                            foreground_shell_exit_reported = false;
                            pending_restore_probe = false;
                            last_claude_working_at = None;
                            last_visible_blocker = false;
                            last_visible_idle = false;
                            last_visible_working = false;
                            last_visible_signal_refresh = None;
                            pending_idle.clear();
                            last_screen_scan_detection_content_seq = None;
                            agent_startup_grace_until = None;
                        }
                    }

                    let now = Instant::now();
                    let suppressed_agent = active_pending_release(&pending_release_for_task, now);
                    let pid = child_pid.load(Ordering::Acquire);
                    #[cfg(windows)]
                    let foreground_observation_due = should_observe_foreground_process_group(
                        agent_presence.current_agent().is_some(),
                        suppressed_agent.is_some(),
                        pending_foreground_shell_clear,
                        pending_restore_probe,
                        false,
                        now.duration_since(last_process_check),
                        PROCESS_RECHECK,
                    );
                    #[cfg(not(windows))]
                    let foreground_observation_due = true;
                    let foreground_pgid = (pid > 0
                        && agent_presence.current_agent().is_some()
                        && foreground_observation_due)
                        .then(|| detect::foreground_process_group_id(pid))
                        .flatten();
                    let foreground_group_changed = foreground_pgid.is_some()
                        && last_foreground_pgid.is_some()
                        && foreground_pgid != last_foreground_pgid;
                    let should_check_process = suppressed_agent.is_some()
                        || agent_presence.current_agent().is_none()
                        || foreground_group_changed
                        || pending_foreground_shell_clear
                        || pending_restore_probe
                        || now.duration_since(last_process_check) >= PROCESS_RECHECK;

                    let mut agent_changed = false;
                    let mut agent = agent_presence.current_agent();
                    if should_check_process {
                        last_process_check = now;
                        if pid > 0 {
                            let mut process_name = None;
                            let mut process_group_id = None;
                            let mut foreground_is_pane_shell = false;
                            let mut new_agent = None;

                            if let Some(job) = detect::foreground_job(pid) {
                                process_group_id = Some(job.process_group_id);
                                last_foreground_pgid = Some(job.process_group_id);
                                foreground_is_pane_shell =
                                    job.processes.iter().any(|p| p.pid == pid);
                                let identified = identify_foreground_job_with_hint(&job);
                                process_name = identified
                                    .as_ref()
                                    .map(|(_, process_name)| process_name.clone());
                                new_agent = identified.as_ref().map(|(agent, _)| *agent);
                            } else if foreground_pgid.is_some() {
                                process_group_id = foreground_pgid;
                                last_foreground_pgid = foreground_pgid;
                            }

                            if let Some(suppressed_agent) = suppressed_agent {
                                if new_agent == Some(suppressed_agent) {
                                    new_agent = None;
                                } else if let Ok(mut pending_release) =
                                    pending_release_for_task.lock()
                                {
                                    *pending_release = None;
                                }
                            }

                            let previous_agent = agent_presence.current_agent();
                            let changed = apply_foreground_shell_agent_action(
                                &mut agent_presence,
                                foreground_shell_agent_action(
                                    previous_agent,
                                    new_agent,
                                    foreground_is_pane_shell,
                                    foreground_shell_exit_reported,
                                ),
                                previous_agent,
                                new_agent,
                                &mut pending_foreground_shell_clear,
                                &mut foreground_shell_exit_reported,
                            );
                            if new_agent.is_some() {
                                last_foreground_pgid = process_group_id;
                                pending_restore_probe = false;
                            } else if agent_presence.current_agent().is_none() {
                                last_foreground_pgid = None;
                                pending_restore_probe = false;
                            }
                            if changed {
                                agent = agent_presence.current_agent();
                                if let Some(process_name) = process_name {
                                    info!(
                                        pane = pane_id.raw(),
                                        previous_agent = ?previous_agent,
                                        ?agent,
                                        process = %process_name,
                                        pgid = ?process_group_id,
                                        "agent changed"
                                    );
                                } else {
                                    info!(
                                        pane = pane_id.raw(),
                                        previous_agent = ?previous_agent,
                                        ?agent,
                                        pgid = ?process_group_id,
                                        "agent changed"
                                    );
                                }
                                agent_changed = true;
                            }
                        }
                    }

                    if agent_changed {
                        if let Some(detected) = agent {
                            pending_idle.clear();
                            last_screen_scan_detection_content_seq = None;
                            agent_startup_grace_until = Some(now + AGENT_STARTUP_GRACE_WINDOW);
                            terminal.clear_agent_osc_state();
                            publish_agent_process_detected_event(
                                state_events.clone(),
                                pane_id,
                                detected,
                                now,
                            )
                            .await;
                        }
                    }

                    let pid = child_pid.load(Ordering::Acquire);
                    // Keep the terminal restore side effect separate from render notification state.
                    #[allow(clippy::collapsible_if)]
                    if pid > 0 && terminal.maybe_restore_host_terminal_theme(pane_id, pid) {
                        if render_dirty.request_pty(pane_id) {
                            render_notify.notify_one();
                        }
                    }

                    let process_exited = pending_foreground_shell_clear
                        && agent.is_some()
                        && !foreground_shell_exit_reported;
                    if full_lifecycle_authority_should_skip_screen_detection(
                        full_lifecycle_authority_active.load(Ordering::Acquire),
                        process_exited,
                        suppressed_agent,
                    ) {
                        continue;
                    }
                    if let Some(until) = agent_startup_grace_until {
                        if now < until && !process_exited {
                            continue;
                        }
                        agent_startup_grace_until = None;
                    }

                    let current_detection_content_seq = if agent.is_some() {
                        Some(detection_content_seq_for_task.load(Ordering::Relaxed))
                    } else {
                        None
                    };

                    let read_decision = decide_detection_screen_read(DetectionScreenReadInput {
                        state,
                        agent,
                        pending_idle_active: pending_idle.active(),
                        agent_changed,
                        process_exited,
                        current_detection_content_seq,
                        last_screen_scan_detection_content_seq,
                    });

                    let publish_decision = match read_decision {
                        DetectionScreenReadDecision::Skip => continue,
                        DetectionScreenReadDecision::Read => {
                            let content = terminal.detection_text();
                            last_screen_scan_detection_content_seq = current_detection_content_seq;
                            let detection = if process_exited {
                                detect::AgentDetection {
                                    state: AgentState::Idle,
                                    skip_state_update: false,
                                    visible_blocker: false,
                                    visible_idle: false,
                                    visible_working: false,
                                }
                            } else {
                                let osc_title = terminal.agent_osc_title();
                                let osc_progress = terminal.agent_osc_progress();
                                detect::detect_agent_with_osc(
                                    agent,
                                    &content,
                                    &osc_title,
                                    &osc_progress,
                                )
                            };
                            decide_screen_detection_publish(
                                ScreenDetectionPublishInput {
                                    agent,
                                    current_state: state,
                                    last_visible_blocker,
                                    last_visible_idle,
                                    last_visible_working,
                                    last_visible_signal_refresh,
                                    screen_detection: detection,
                                    process_exited,
                                    agent_changed,
                                    now,
                                    last_claude_working_at: &mut last_claude_working_at,
                                },
                                &mut pending_idle,
                            )
                        }
                    };

                    if let DetectionPublishDecision::Publish {
                        state: new_state,
                        visible_blocker,
                        visible_idle,
                        visible_working,
                        process_exited: publish_process_exited,
                    } = publish_decision
                    {
                        debug!(
                            pane = pane_id.raw(),
                            ?state,
                            ?new_state,
                            ?agent,
                            "state changed"
                        );
                        state = new_state;
                        last_visible_blocker = visible_blocker;
                        last_visible_idle = visible_idle;
                        last_visible_working = visible_working;
                        if visible_blocker || visible_working {
                            last_visible_signal_refresh = Some(now);
                        } else {
                            last_visible_signal_refresh = None;
                        }
                        publish_state_changed_event(
                            state_events.clone(),
                            pane_id,
                            agent,
                            new_state,
                            visible_blocker,
                            visible_idle,
                            visible_working,
                            publish_process_exited,
                            now,
                        )
                        .await;
                        if publish_process_exited {
                            foreground_shell_exit_reported = true;
                        }
                    }
                }
            });
            (handle.abort_handle(), detect_reset_notify, pending_release)
        };

        Ok(Self {
            pane_id,
            terminal,
            io,
            current_size: Cell::new((rows, cols, 0, 0)),
            child_pid,
            child_wait_completed: Some(child_wait_completed),
            kitty_keyboard_flags,
            content_seq,
            full_lifecycle_authority_active,
            detection_content_seq,
            detect_reset_notify,
            pending_release,
            preserve_processes_on_drop: false,
            detect_handle: Some(detect_handle),
        })
    }
    pub fn set_full_lifecycle_authority_active(&self, active: bool) {
        self.full_lifecycle_authority_active
            .store(active, Ordering::Release);
    }

    pub fn begin_graceful_release(&self, agent: Agent) {
        if let Ok(mut pending_release) = self.pending_release.lock() {
            *pending_release = Some(PendingAgentRelease {
                agent,
                until: std::time::Instant::now() + RELEASE_REACQUIRE_SUPPRESSION,
            });
        }
        self.detect_reset_notify.notify_one();
    }
    pub fn reset_agent_detection(&self) {
        self.detect_reset_notify.notify_one();
    }

    #[cfg(test)]
    pub(crate) fn agent_detection_reset_notify_for_test(&self) -> Arc<Notify> {
        self.detect_reset_notify.clone()
    }

    pub(crate) fn current_size(&self) -> (u16, u16) {
        let (rows, cols, _, _) = self.current_size.get();
        (rows, cols)
    }

    pub(crate) fn current_cell_size_px(&self) -> Option<(u32, u32)> {
        let (_, _, width_px, height_px) = self.current_size.get();
        (width_px > 0 && height_px > 0).then_some((width_px, height_px))
    }

    /// Resize if the dimensions actually changed.
    pub fn resize(&self, rows: u16, cols: u16, cell_width_px: u32, cell_height_px: u32) {
        let rows = rows.max(2);
        let cols = cols.max(4);
        let size = (rows, cols, cell_width_px, cell_height_px);
        if self.current_size.get() == size {
            return;
        }
        self.current_size.set(size);
        let terminal_responses = self
            .terminal
            .resize(rows, cols, cell_width_px, cell_height_px);
        mark_detection_content_changed(&self.detection_content_seq);
        self.io.resize(
            rows,
            cols,
            cell_width_px,
            cell_height_px,
            terminal_responses,
        );
    }

    pub fn nudge_child_redraw_after_handoff(&self) {
        mark_detection_content_changed(&self.detection_content_seq);
        let (rows, cols, cell_width_px, cell_height_px) = self.current_size.get();
        self.io
            .nudge_child_redraw_after_handoff(rows, cols, cell_width_px, cell_height_px);
    }

    /// Scroll up by N lines (into scrollback history).
    pub fn scroll_up(&self, lines: usize) {
        self.terminal.scroll_up(lines);
    }

    /// Scroll down by N lines (toward live output).
    pub fn scroll_down(&self, lines: usize) {
        self.terminal.scroll_down(lines);
    }

    /// Reset scroll to live view (offset = 0).
    pub fn scroll_reset(&self) {
        self.terminal.scroll_reset();
    }

    /// Set scrollback offset measured from the live bottom of the terminal.
    pub fn set_scroll_offset_from_bottom(&self, lines: usize) {
        self.terminal.set_scroll_offset_from_bottom(lines);
    }

    pub fn scroll_metrics(&self) -> Option<ScrollMetrics> {
        self.terminal.scroll_metrics()
    }

    pub(crate) fn search_text_matches(
        &self,
        query: &str,
        case_sensitive: bool,
    ) -> Vec<TerminalTextMatch> {
        self.terminal.search_text_matches(query, case_sensitive)
    }

    pub(crate) fn text_match_is_current(&self, text_match: TerminalTextMatch) -> bool {
        self.terminal.text_match_is_current(text_match)
    }

    pub(crate) fn text_matches_are_current(&self, text_matches: &[TerminalTextMatch]) -> Vec<bool> {
        self.terminal.text_matches_are_current(text_matches)
    }

    pub(crate) fn word_motion_target(
        &self,
        row: u32,
        col: u16,
        motion: TerminalWordMotion,
    ) -> Option<TerminalTextPoint> {
        self.terminal.word_motion_target(row, col, motion)
    }

    pub fn input_state(&self) -> Option<InputState> {
        #[cfg(test)]
        AGGREGATE_INPUT_STATE_READS.set(AGGREGATE_INPUT_STATE_READS.get() + 1);
        self.terminal.input_state()
    }

    pub fn cursor_state(&self, area: Rect, show_cursor: bool) -> Option<TerminalCursorState> {
        if !show_cursor {
            return None;
        }
        let cursor = self.terminal.cursor_state()?;
        if cursor.x >= area.width || cursor.y >= area.height {
            return None;
        }
        Some(TerminalCursorState {
            x: area.x + cursor.x,
            y: area.y + cursor.y,
            visible: cursor.visible,
            shape: cursor.shape,
        })
    }

    pub fn visible_text(&self) -> String {
        self.terminal.visible_text()
    }

    pub fn visible_ansi(&self) -> String {
        self.terminal.visible_ansi()
    }
    pub fn detection_text(&self) -> String {
        self.terminal.detection_text()
    }

    pub fn agent_osc_title(&self) -> String {
        self.terminal.agent_osc_title()
    }

    pub fn take_agent_osc_title_dirty(&self) -> bool {
        self.terminal.take_agent_osc_title_dirty()
    }

    pub fn agent_osc_progress(&self) -> String {
        self.terminal.agent_osc_progress()
    }

    pub(crate) fn recent_text_snapshot(&self, lines: usize) -> TerminalReadSnapshot {
        self.terminal.recent_text_snapshot(lines)
    }

    pub(crate) fn recent_ansi_snapshot(&self, lines: usize) -> TerminalReadSnapshot {
        self.terminal.recent_ansi_snapshot(lines)
    }

    pub(crate) fn recent_unwrapped_text_snapshot(&self, lines: usize) -> TerminalReadSnapshot {
        self.terminal.recent_unwrapped_text_snapshot(lines)
    }

    pub(crate) fn recent_unwrapped_ansi_snapshot(&self, lines: usize) -> TerminalReadSnapshot {
        self.terminal.recent_unwrapped_ansi_snapshot(lines)
    }

    pub fn snapshot_history(&self) -> Option<String> {
        let ansi = self.recent_unwrapped_ansi_snapshot(usize::MAX).text;
        (!ansi.trim().is_empty()).then_some(ansi)
    }

    pub fn extract_selection(&self, selection: &crate::selection::Selection) -> Option<String> {
        self.terminal.extract_selection(selection)
    }

    pub fn render_with_theme_background(
        &self,
        frame: &mut Frame,
        area: Rect,
        show_cursor: bool,
        theme_default_bg: Option<Color>,
    ) {
        self.terminal
            .render_with_theme_background(frame, area, show_cursor, theme_default_bg);
    }

    pub fn render_view_with_theme_background(
        &self,
        frame: &mut Frame,
        viewport: crate::pane::TerminalViewport,
        show_cursor: bool,
        theme_default_bg: Option<Color>,
    ) {
        self.terminal.render_view_with_theme_background(
            frame,
            viewport,
            show_cursor,
            theme_default_bg,
        );
    }

    pub fn visible_hyperlinks(&self, area: Rect) -> Vec<((u16, u16), String, String)> {
        self.terminal.visible_hyperlinks(area)
    }

    pub fn kitty_image_placements_with_data_filter<F>(
        &self,
        needs_data: F,
    ) -> Vec<crate::ghostty::KittyImagePlacement>
    where
        F: FnMut(crate::ghostty::KittyImageDescriptor) -> bool,
    {
        self.terminal
            .kitty_image_placements_with_data_filter(needs_data)
    }

    pub fn keyboard_protocol(&self) -> crate::input::KeyboardProtocol {
        let fallback = crate::input::KeyboardProtocol::from_kitty_flags(
            self.kitty_keyboard_flags.load(Ordering::Relaxed),
        );
        self.terminal.keyboard_protocol(fallback)
    }

    pub fn encode_terminal_key(&self, key: crate::input::TerminalKey) -> Vec<u8> {
        self.terminal
            .encode_terminal_key(key, self.keyboard_protocol())
    }

    pub async fn send_bytes(&self, bytes: Bytes) -> Result<(), mpsc::error::SendError<Bytes>> {
        self.io.send_bytes(bytes).await
    }

    pub fn try_send_bytes(&self, bytes: Bytes) -> Result<(), mpsc::error::TrySendError<Bytes>> {
        self.io.try_send_bytes(bytes)
    }

    pub fn send_bytes_after(&self, bytes: Bytes, delay: std::time::Duration) {
        self.io.send_bytes_after(bytes, delay);
    }

    pub async fn send_paste(&self, text: String) -> Result<(), mpsc::error::SendError<Bytes>> {
        let bracketed = self
            .input_state()
            .map(|state| state.bracketed_paste)
            .unwrap_or(false);
        let payload = if bracketed {
            format!("\x1b[200~{text}\x1b[201~")
        } else {
            text
        };
        self.send_bytes(Bytes::from(payload)).await
    }

    pub fn try_send_focus_event(&self, event: crate::ghostty::FocusEvent) -> bool {
        if !self
            .input_state()
            .map(|state| state.focus_reporting)
            .unwrap_or(false)
        {
            return false;
        }

        let Ok(bytes) = crate::ghostty::encode_focus(event) else {
            return false;
        };
        if let Err(err) = self.try_send_bytes(Bytes::from(bytes)) {
            warn!(err = %err, ?event, "failed to forward pane focus event");
        }
        true
    }

    pub fn wheel_routing(&self) -> Option<WheelRouting> {
        self.terminal.wheel_routing()
    }

    pub(crate) fn content_seq(&self) -> u64 {
        self.content_seq.load(Ordering::Acquire)
    }

    pub(crate) fn screen_text_snapshot(
        &self,
    ) -> Option<(
        crate::ghostty::ActiveScreen,
        u16,
        Vec<crate::ghostty::ScreenTextRow>,
    )> {
        self.terminal.screen_text_snapshot()
    }

    pub(crate) fn synchronized_output_active(&self) -> bool {
        self.terminal.synchronized_output_active()
    }

    pub fn encode_mouse_button(
        &self,
        kind: crossterm::event::MouseEventKind,
        column: u16,
        row: u16,
        modifiers: crossterm::event::KeyModifiers,
    ) -> Option<Vec<u8>> {
        if !self.input_state()?.mouse_protocol_mode.reporting_enabled() {
            return None;
        }
        self.terminal
            .encode_mouse_button(kind, column, row, modifiers)
    }

    pub fn encode_mouse_wheel(
        &self,
        kind: crossterm::event::MouseEventKind,
        column: u16,
        row: u16,
        modifiers: crossterm::event::KeyModifiers,
    ) -> Option<Vec<u8>> {
        if self.wheel_routing()? != WheelRouting::MouseReport {
            return None;
        }
        self.terminal
            .encode_mouse_wheel(kind, column, row, modifiers)
    }

    pub fn encode_mouse_motion(
        &self,
        kind: crossterm::event::MouseEventKind,
        column: u16,
        row: u16,
        modifiers: crossterm::event::KeyModifiers,
    ) -> Option<Vec<u8>> {
        if self.input_state()?.mouse_protocol_mode != crate::input::MouseProtocolMode::AnyMotion {
            return None;
        }
        self.terminal
            .encode_mouse_motion(kind, column, row, modifiers)
    }

    pub fn encode_mouse_button_xy(
        &self,
        kind: crossterm::event::MouseEventKind,
        x: f32,
        y: f32,
        modifiers: crossterm::event::KeyModifiers,
    ) -> Option<Vec<u8>> {
        if !self.input_state()?.mouse_protocol_mode.reporting_enabled() {
            return None;
        }
        self.terminal.encode_mouse_button_xy(kind, x, y, modifiers)
    }

    pub fn encode_mouse_wheel_xy(
        &self,
        kind: crossterm::event::MouseEventKind,
        x: f32,
        y: f32,
        modifiers: crossterm::event::KeyModifiers,
    ) -> Option<Vec<u8>> {
        if self.wheel_routing()? != WheelRouting::MouseReport {
            return None;
        }
        self.terminal.encode_mouse_wheel_xy(kind, x, y, modifiers)
    }

    pub fn encode_mouse_motion_xy(
        &self,
        kind: crossterm::event::MouseEventKind,
        x: f32,
        y: f32,
        modifiers: crossterm::event::KeyModifiers,
    ) -> Option<Vec<u8>> {
        if self.input_state()?.mouse_protocol_mode != crate::input::MouseProtocolMode::AnyMotion {
            return None;
        }
        self.terminal.encode_mouse_motion_xy(kind, x, y, modifiers)
    }

    pub fn encode_alternate_scroll(
        &self,
        kind: crossterm::event::MouseEventKind,
    ) -> Option<Vec<u8>> {
        self.input_state()?;
        if self.wheel_routing()? != WheelRouting::AlternateScroll {
            return None;
        }
        let key = match kind {
            crossterm::event::MouseEventKind::ScrollUp => crossterm::event::KeyCode::Up,
            crossterm::event::MouseEventKind::ScrollDown => crossterm::event::KeyCode::Down,
            _ => return None,
        };
        Some(self.encode_terminal_key(crate::input::TerminalKey::new(
            key,
            crossterm::event::KeyModifiers::empty(),
        )))
    }

    /// Get the current working directory of the child shell process.
    pub fn cwd(&self) -> Option<std::path::PathBuf> {
        let pid = self.child_pid.load(Ordering::Relaxed);
        crate::platform::process_cwd(pid)
    }

    /// Get the current working directory of the process group controlling the pane PTY.
    pub fn foreground_cwd(&self) -> Option<std::path::PathBuf> {
        #[cfg(unix)]
        {
            let pid = self.child_pid.load(Ordering::Acquire);
            let shell_cwd = absolute_process_cwd(pid);
            let foreground_pgid = self
                .io
                .foreground_process_group_id()
                .or_else(|| crate::platform::foreground_process_group_id(pid));
            let leader_cwd = foreground_pgid.and_then(absolute_process_cwd);

            if leader_cwd.as_ref() == shell_cwd.as_ref() {
                foreground_member_cwd_different_from_shell(pid, shell_cwd.as_ref()).or(leader_cwd)
            } else {
                leader_cwd
                    .or_else(|| foreground_member_cwd_different_from_shell(pid, shell_cwd.as_ref()))
            }
        }

        #[cfg(not(unix))]
        {
            None
        }
    }
}

#[cfg(test)]
impl PaneRuntime {
    pub(crate) fn test_with_channel(cols: u16, rows: u16) -> (Self, mpsc::Receiver<Bytes>) {
        Self::test_with_channel_and_scrollback_bytes(cols, rows, 0, &[], 4)
    }

    pub(crate) fn test_with_channel_and_screen_bytes(
        cols: u16,
        rows: u16,
        bytes: &[u8],
    ) -> (Self, mpsc::Receiver<Bytes>) {
        Self::test_with_channel_and_scrollback_bytes(cols, rows, 0, bytes, 4)
    }

    pub(crate) fn test_with_channel_capacity(
        cols: u16,
        rows: u16,
        capacity: usize,
    ) -> (Self, mpsc::Receiver<Bytes>) {
        Self::test_with_channel_and_scrollback_bytes(cols, rows, 0, &[], capacity)
    }

    pub(crate) fn test_with_screen_bytes(cols: u16, rows: u16, bytes: &[u8]) -> Self {
        Self::test_with_scrollback_bytes(cols, rows, 0, bytes)
    }

    pub(crate) fn test_with_scrollback_bytes(
        cols: u16,
        rows: u16,
        scrollback_limit_bytes: usize,
        bytes: &[u8],
    ) -> Self {
        Self::test_with_channel_and_scrollback_bytes(cols, rows, scrollback_limit_bytes, bytes, 4).0
    }

    pub(crate) fn test_with_channel_and_scrollback_bytes(
        cols: u16,
        rows: u16,
        scrollback_limit_bytes: usize,
        bytes: &[u8],
        channel_capacity: usize,
    ) -> (Self, mpsc::Receiver<Bytes>) {
        let (tx, rx) = mpsc::channel(channel_capacity);
        let (resize_tx, _resize_rx) = watch::channel((rows, cols, 0, 0));
        let mut terminal =
            crate::ghostty::Terminal::new(cols, rows, scrollback_limit_bytes).unwrap();
        terminal.write(bytes);
        let ghostty = GhosttyPaneTerminal::new(terminal, tx.clone()).unwrap();
        if !bytes.is_empty() {
            if let Ok(mut core) = ghostty.core.lock() {
                core.kitty_keyboard.observe(bytes);
            }
        }

        (
            Self {
                pane_id: PaneId::from_raw(0),
                terminal: Arc::new(PaneTerminal::new(ghostty)),
                io: PaneRuntimeIo::TestChannel {
                    sender: tx,
                    resize_tx,
                },
                current_size: Cell::new((rows, cols, 0, 0)),
                child_pid: Arc::new(AtomicU32::new(0)),
                child_wait_completed: None,
                kitty_keyboard_flags: Arc::new(AtomicU16::new(0)),
                content_seq: Arc::new(AtomicU64::new(0)),
                full_lifecycle_authority_active: Arc::new(AtomicBool::new(false)),
                detection_content_seq: Arc::new(AtomicU64::new(0)),
                detect_reset_notify: Arc::new(Notify::new()),
                pending_release: Arc::new(Mutex::new(None)),
                preserve_processes_on_drop: true,
                detect_handle: None,
            },
            rx,
        )
    }
    pub(crate) fn test_process_pty_bytes(&self, pane_id: PaneId, bytes: &[u8]) {
        self.content_seq.fetch_add(1, Ordering::AcqRel);
        let (tx, _rx) = mpsc::channel(4);
        let shell_pid = self.child_pid.load(Ordering::Acquire);
        self.terminal
            .process_pty_bytes(pane_id, shell_pid, bytes, &tx);
        self.content_seq.fetch_add(1, Ordering::Release);
    }
}

trait ExitStatusParts {
    fn exit_code(&self) -> u32;
    fn signal(&self) -> Option<&str>;
}

impl ExitStatusParts for portable_pty::ExitStatus {
    fn exit_code(&self) -> u32 {
        portable_pty::ExitStatus::exit_code(self)
    }
    fn signal(&self) -> Option<&str> {
        portable_pty::ExitStatus::signal(self)
    }
}

fn exit_status_parts(status: &impl ExitStatusParts) -> (Option<i32>, Option<i32>) {
    if let Some(signal_name) = status.signal() {
        return (None, Some(signal_number(signal_name)));
    }
    let code = i32::try_from(status.exit_code()).unwrap_or(1);
    (Some(code), None)
}

fn signal_number(name: &str) -> i32 {
    let normalized = name.trim();
    let bare = normalized
        .strip_prefix("Signal ")
        .or_else(|| normalized.strip_prefix("SIG"))
        .unwrap_or(normalized);
    if let Ok(number) = bare.parse::<i32>() {
        return number;
    }
    match bare.to_ascii_uppercase().as_str() {
        "HUP" | "SIGHUP" => 1,
        "INT" | "SIGINT" => 2,
        "QUIT" | "SIGQUIT" => 3,
        "ILL" | "SIGILL" => 4,
        "TRAP" | "SIGTRAP" => 5,
        "ABRT" | "SIGABRT" | "IOT" | "SIGIOT" => 6,
        "EMT" | "SIGEMT" => 7,
        "FPE" | "SIGFPE" => 8,
        "KILL" | "SIGKILL" => 9,
        "BUS" | "SIGBUS" => 10,
        "SEGV" | "SIGSEGV" => 11,
        "SYS" | "SIGSYS" => 12,
        "PIPE" | "SIGPIPE" => 13,
        "ALRM" | "SIGALRM" => 14,
        "TERM" | "SIGTERM" => 15,
        "URG" | "SIGURG" => 16,
        "STOP" | "SIGSTOP" => 17,
        "TSTP" | "SIGTSTP" => 18,
        "CONT" | "SIGCONT" => 19,
        "CHLD" | "SIGCHLD" | "CLD" | "SIGCLD" => 20,
        "TTIN" | "SIGTTIN" => 21,
        "TTOU" | "SIGTTOU" => 22,
        "IO" | "SIGIO" | "POLL" | "SIGPOLL" => 23,
        "XCPU" | "SIGXCPU" => 24,
        "XFSZ" | "SIGXFSZ" => 25,
        "VTALRM" | "SIGVTALRM" => 26,
        "PROF" | "SIGPROF" => 27,
        "WINCH" | "SIGWINCH" => 28,
        "INFO" | "SIGINFO" => 29,
        "USR1" | "SIGUSR1" => 30,
        "USR2" | "SIGUSR2" => 31,
        _ => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn process_cwd_does_not_require_traversing_the_directory_path() {
        use std::os::unix::fs::PermissionsExt;

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let base = std::env::temp_dir().join(format!(
            "gardn-process-cwd-no-stat-{}-{stamp}",
            std::process::id()
        ));
        let private = base.join("private");
        let cwd = private.join("cwd");
        std::fs::create_dir_all(&cwd).expect("create process cwd");

        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "sleep 30"])
            .current_dir(&cwd)
            .spawn()
            .expect("spawn process in cwd");
        let expected_cwd = crate::platform::process_cwd(child.id())
            .expect("resolve process cwd before restricting traversal");
        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o000))
            .expect("make cwd path untraversable");

        let path_is_traversable = cwd.is_dir();
        let observed = (!path_is_traversable)
            .then(|| absolute_process_cwd(child.id()))
            .flatten();

        std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o755))
            .expect("restore cwd path permissions");
        let _ = child.kill();
        let _ = child.wait();
        std::fs::remove_dir_all(&base).expect("remove process cwd");

        if path_is_traversable {
            eprintln!("skipping untraversable cwd assertion for privileged test process");
            return;
        }
        assert_eq!(observed, Some(expected_cwd));
    }

    #[test]
    fn windows_foreground_observation_schedule_preserves_lifecycle_checks() {
        let recheck = std::time::Duration::from_secs(5);
        let before = recheck - std::time::Duration::from_millis(1);
        assert!(!should_observe_foreground_process_group(
            true, false, false, false, false, before, recheck
        ));
        assert!(should_observe_foreground_process_group(
            true, false, false, false, true, before, recheck
        ));
        assert!(should_observe_foreground_process_group(
            true, false, false, false, false, recheck, recheck
        ));
        assert!(should_observe_foreground_process_group(
            false,
            false,
            false,
            false,
            false,
            std::time::Duration::ZERO,
            recheck
        ));
        assert!(should_observe_foreground_process_group(
            true,
            true,
            false,
            false,
            false,
            std::time::Duration::ZERO,
            recheck
        ));
        assert!(should_observe_foreground_process_group(
            true,
            false,
            true,
            false,
            false,
            std::time::Duration::ZERO,
            recheck
        ));
        assert!(should_observe_foreground_process_group(
            true,
            false,
            false,
            true,
            false,
            std::time::Duration::ZERO,
            recheck
        ));
    }

    #[test]
    fn pane_launch_env_removes_outer_codex_thread_id() {
        let mut cmd = CommandBuilder::new("shell");
        cmd.env("CODEX_THREAD_ID", "outer-session");

        apply_pane_launch_env(&mut cmd, &PaneLaunchEnv::default(), PaneId::from_raw(1));

        assert!(cmd.get_env("CODEX_THREAD_ID").is_none());
    }

    #[test]
    fn pane_launch_env_publishes_herdr_identity_aliases() {
        let mut cmd = CommandBuilder::new("shell");
        let launch_env = PaneLaunchEnv::default().with_identity(
            "ws-1".to_string(),
            "tab-1".to_string(),
            "pane-1".to_string(),
        );

        apply_pane_launch_env(&mut cmd, &launch_env, PaneId::from_raw(1));

        for (gardn, herdr, value) in [
            ("GARDN_PANE_ID", "HERDR_PANE_ID", "pane-1"),
            ("GARDN_TAB_ID", "HERDR_TAB_ID", "tab-1"),
            ("GARDN_WORKSPACE_ID", "HERDR_WORKSPACE_ID", "ws-1"),
            ("GARDN_ENV", "HERDR_ENV", "1"),
        ] {
            assert_eq!(cmd.get_env(gardn).and_then(|v| v.to_str()), Some(value));
            assert_eq!(cmd.get_env(herdr).and_then(|v| v.to_str()), Some(value));
        }
        assert_eq!(
            cmd.get_env("GARDN_SOCKET_PATH").and_then(|v| v.to_str()),
            cmd.get_env("HERDR_SOCKET_PATH").and_then(|v| v.to_str())
        );
        assert!(cmd.get_env("HERDR_SOCKET_PATH").is_some());
    }

    #[test]
    fn shutdown_liveness_treats_reaped_direct_child_as_gone() {
        assert!(!process_alive_for_shutdown(42, 42, true, |_| true));
    }

    #[test]
    fn shutdown_liveness_keeps_unreaped_direct_child_alive() {
        assert!(process_alive_for_shutdown(42, 42, false, |_| true));
    }

    #[test]
    fn shutdown_liveness_keeps_other_session_processes_alive() {
        assert!(process_alive_for_shutdown(43, 42, true, |_| true));
    }

    #[test]
    fn shutdown_liveness_treats_missing_process_as_gone() {
        assert!(!process_alive_for_shutdown(43, 42, false, |_| false));
    }

    fn capture_shell_output(command: &str, extra_env: &[(&str, &str)]) -> String {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let output_path = std::env::temp_dir().join(format!(
            "gardn-pane-term-test-{}-{}.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.arg("-c");
        cmd.arg(format!("{command} > '{}'", output_path.display()));
        cmd.cwd(std::env::current_dir().unwrap());
        cmd.env("TERM", "xterm-ghostty");
        cmd.env("COLORTERM", "falsecolor");
        apply_pane_terminal_env(&mut cmd);
        for (key, value) in extra_env {
            cmd.env(key, value);
        }

        let mut child = pair.slave.spawn_command(cmd).unwrap();
        let status = child.wait().unwrap();
        assert!(status.success(), "shell command failed: {status:?}");

        let output = std::fs::read_to_string(&output_path).unwrap();
        let _ = std::fs::remove_file(output_path);
        output
    }

    #[test]
    fn pane_shell_prefers_configured_shell() {
        assert_eq!(
            pane_shell_from("/usr/bin/nu", Some("/bin/bash".to_string())),
            "/usr/bin/nu"
        );
    }

    #[test]
    fn pane_shell_falls_back_to_shell_env() {
        assert_eq!(
            pane_shell_from("", Some("/bin/bash".to_string())),
            "/bin/bash"
        );
    }

    #[test]
    fn pane_shell_ignores_empty_values() {
        assert_eq!(pane_shell_from("   ", Some("  ".to_string())), "/bin/sh");
        assert_eq!(pane_shell_from("", None), "/bin/sh");
    }

    #[test]
    fn pane_shell_uses_windows_defaults_for_windows_target() {
        assert_eq!(
            pane_shell_from_parts(
                "",
                Some("/bin/sh".to_string()),
                Some("C:\\Windows\\System32\\cmd.exe".to_string()),
                true,
            ),
            "C:\\Windows\\System32\\cmd.exe"
        );
        assert_eq!(pane_shell_from_parts("", None, None, true), "cmd.exe");
    }

    #[test]
    fn shell_mode_auto_uses_login_shell_only_on_macos() {
        assert!(shell_mode_uses_login_shell(
            crate::config::ShellModeConfig::Auto,
            true,
            false
        ));
        assert!(!shell_mode_uses_login_shell(
            crate::config::ShellModeConfig::Auto,
            false,
            false
        ));
        assert!(shell_mode_uses_login_shell(
            crate::config::ShellModeConfig::Login,
            false,
            false
        ));
        assert!(!shell_mode_uses_login_shell(
            crate::config::ShellModeConfig::NonLogin,
            true,
            false
        ));
        assert!(!shell_mode_uses_login_shell(
            crate::config::ShellModeConfig::Login,
            false,
            true
        ));
    }

    #[test]
    fn login_shell_builder_uses_default_prog_with_resolved_shell_env() {
        let cmd = pane_shell_command_builder_for_target(
            PaneShellConfig::new("/bin/sh", crate::config::ShellModeConfig::Login),
            false,
            false,
        )
        .unwrap();
        assert!(cmd.is_default_prog());
        assert_eq!(
            cmd.get_env("SHELL").and_then(std::ffi::OsStr::to_str),
            Some("/bin/sh")
        );
    }

    #[test]
    fn auto_shell_builder_uses_login_shell_on_macos_target() {
        let cmd = pane_shell_command_builder_for_target(
            PaneShellConfig::new("/bin/sh", crate::config::ShellModeConfig::Auto),
            true,
            false,
        )
        .unwrap();
        assert!(cmd.is_default_prog());
        assert_eq!(
            cmd.get_env("SHELL").and_then(std::ffi::OsStr::to_str),
            Some("/bin/sh")
        );
    }

    #[test]
    fn auto_shell_builder_keeps_direct_shell_on_non_macos_target() {
        let cmd = pane_shell_command_builder_for_target(
            PaneShellConfig::new("/bin/sh", crate::config::ShellModeConfig::Auto),
            false,
            false,
        )
        .unwrap();
        assert!(!cmd.is_default_prog());
        assert_eq!(cmd.get_argv(), &[std::ffi::OsString::from("/bin/sh")]);
    }

    #[test]
    fn login_shell_builder_rejects_missing_shell_instead_of_falling_back() {
        let err = pane_shell_command_builder_for_target(
            PaneShellConfig::new(
                "/__gardn_missing_shell__",
                crate::config::ShellModeConfig::Login,
            ),
            false,
            false,
        )
        .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn login_shell_builder_resolves_bare_shell_names_from_path() {
        let _lock = crate::integration::integration_env_lock();
        let base = std::env::temp_dir().join(format!(
            "gardn-login-shell-path-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bin = base.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let shell = bin.join("fake-shell");
        std::fs::write(&shell, "#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let _path_env = crate::config::TestEnvVar::set("PATH", &bin);

        let cmd = pane_shell_command_builder_for_target(
            PaneShellConfig::new("fake-shell", crate::config::ShellModeConfig::Login),
            false,
            false,
        )
        .unwrap();

        assert!(cmd.is_default_prog());
        assert_eq!(
            cmd.get_env("SHELL").and_then(std::ffi::OsStr::to_str),
            shell.to_str()
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn login_shell_resolution_preserves_shell_paths() {
        assert_eq!(resolve_shell_for_login_mode("/bin/sh").unwrap(), "/bin/sh");
    }

    #[test]
    fn windows_powershell_builder_injects_prompt_cwd_integration() {
        let cmd = pane_shell_command_builder_for_target(
            PaneShellConfig::new(
                "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
                crate::config::ShellModeConfig::NonLogin,
            ),
            false,
            true,
        )
        .unwrap();
        assert_eq!(
            cmd.get_argv(),
            &[
                std::ffi::OsString::from("C:\\Program Files\\PowerShell\\7\\pwsh.exe"),
                std::ffi::OsString::from("-NoExit"),
                std::ffi::OsString::from("-Command"),
                std::ffi::OsString::from(WINDOWS_POWERSHELL_SHELL_INTEGRATION_COMMAND),
            ]
        );
        assert!(WINDOWS_POWERSHELL_SHELL_INTEGRATION_COMMAND.contains("]9;9;"));
        assert!(!WINDOWS_POWERSHELL_SHELL_INTEGRATION_COMMAND.contains('"'));
        assert!(
            WINDOWS_POWERSHELL_SHELL_INTEGRATION_COMMAND
                .find("@(& $global:__GardnOriginalPrompt)")
                .unwrap()
                < WINDOWS_POWERSHELL_SHELL_INTEGRATION_COMMAND
                    .find("$loc =")
                    .unwrap()
        );
    }

    #[test]
    fn powershell_prompt_integration_requires_windows_non_login_shell() {
        let config = PaneShellConfig::new("pwsh.exe", crate::config::ShellModeConfig::NonLogin);
        assert!(uses_windows_powershell_pane_shell_for_target(config, true));
        assert!(!uses_windows_powershell_pane_shell_for_target(
            config, false
        ));
        assert!(!uses_windows_powershell_pane_shell_for_target(
            PaneShellConfig::new("pwsh.exe", crate::config::ShellModeConfig::Login),
            true,
        ));
        assert!(!uses_windows_powershell_pane_shell_for_target(
            PaneShellConfig::new("cmd.exe", crate::config::ShellModeConfig::NonLogin),
            true,
        ));
    }

    #[test]
    fn non_login_shell_builder_execs_resolved_shell_directly() {
        let cmd = pane_shell_command_builder(PaneShellConfig::new(
            "/bin/sh",
            crate::config::ShellModeConfig::NonLogin,
        ))
        .unwrap();
        assert!(!cmd.is_default_prog());
        assert_eq!(cmd.get_argv(), &[std::ffi::OsString::from("/bin/sh")]);
    }

    #[test]
    fn windows_shell_command_builder_uses_cmd_switches() {
        let cmd = shell_command_builder_for_target(
            Some(PaneShellConfig::new(
                "cmd.exe",
                crate::config::ShellModeConfig::Login,
            )),
            "echo ok",
            true,
        )
        .unwrap();

        assert_eq!(
            cmd.get_argv(),
            &[
                std::ffi::OsString::from("cmd.exe"),
                std::ffi::OsString::from("/C"),
                std::ffi::OsString::from("echo ok"),
            ]
        );
    }

    #[test]
    fn windows_shell_command_builder_uses_powershell_switches() {
        let cmd = shell_command_builder_for_target(
            Some(PaneShellConfig::new(
                "pwsh.exe",
                crate::config::ShellModeConfig::Login,
            )),
            "Write-Output ok",
            true,
        )
        .unwrap();

        assert_eq!(
            cmd.get_argv(),
            &[
                std::ffi::OsString::from("pwsh.exe"),
                std::ffi::OsString::from("-NoLogo"),
                std::ffi::OsString::from("-NoProfile"),
                std::ffi::OsString::from("-Command"),
                std::ffi::OsString::from("Write-Output ok"),
            ]
        );
    }

    #[test]
    fn pane_terminal_identity_overrides_outer_terminal_env() {
        let output = capture_shell_output("printf '%s\\n%s\\n' \"$TERM\" \"$COLORTERM\"", &[]);
        assert_eq!(output, "xterm-256color\ntruecolor\n");
    }

    #[test]
    fn pane_terminal_identity_allows_explicit_override() {
        let output = capture_shell_output(
            "printf '%s\\n%s\\n' \"$TERM\" \"$COLORTERM\"",
            &[("TERM", "vt100"), ("COLORTERM", "24bit")],
        );
        assert_eq!(output, "vt100\n24bit\n");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn handoff_history_ansi_captures_primary_screen() {
        let runtime =
            PaneRuntime::test_with_scrollback_bytes(40, 5, 4096, b"handoff-primary-history\r\n");

        let history = runtime.handoff_history_ansi().unwrap();

        assert!(
            history.contains("handoff-primary-history"),
            "history={history:?}"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn handoff_history_ansi_skips_alternate_screen() {
        let runtime = PaneRuntime::test_with_scrollback_bytes(
            40,
            5,
            4096,
            b"primary\r\n\x1b[?1049halt-screen",
        );

        assert!(runtime.handoff_history_ansi().is_none());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn handoff_runtime_state_captures_terminal_input_state() {
        let runtime = PaneRuntime::test_with_screen_bytes(
            80,
            24,
            b"\x1b[>5u\x1b[>4;2m\x1b[?1h\x1b[?2004h\x1b[?1004h\x1b[?1002h\x1b[?1006h",
        );

        let pane = runtime.handoff_runtime_state(12);

        assert_eq!(pane.keyboard_protocol_flags, 5);
        assert_eq!(
            pane.input_state,
            Some(InputState {
                alternate_screen: false,
                application_cursor: true,
                bracketed_paste: true,
                focus_reporting: true,
                mouse_protocol_mode: crate::input::MouseProtocolMode::ButtonMotion,
                mouse_protocol_encoding: crate::input::MouseProtocolEncoding::Sgr,
                mouse_alternate_scroll: true,
                modify_other_keys: true,
                color_scheme_reporting: false,
                mouse_sgr_pixels: false,
            })
        );
    }

    #[cfg(unix)]
    #[test]
    fn truncate_handoff_history_keeps_recent_utf8_boundary() {
        let history = format!("old\n{}\nrecent\n", "é".repeat(8));

        let truncated = truncate_handoff_history(history, 20);

        assert_eq!(truncated, "recent\n");
        assert!(truncated.is_char_boundary(0));
    }

    #[cfg(unix)]
    #[test]
    fn truncate_handoff_history_drops_partial_long_line() {
        let history = format!("old\n{}", "x".repeat(64));

        let truncated = truncate_handoff_history(history, 12);

        assert!(truncated.is_empty());
    }

    #[tokio::test]
    async fn focus_events_are_forwarded_when_enabled() {
        let (tx, mut rx) = mpsc::channel(4);
        let (resize_tx, _resize_rx) = watch::channel((80, 24, 0, 0));
        let mut terminal = crate::ghostty::Terminal::new(80, 24, 0).unwrap();
        terminal
            .mode_set(crate::ghostty::MODE_FOCUS_EVENT, true)
            .unwrap();
        let runtime = PaneRuntime {
            pane_id: PaneId::from_raw(0),
            terminal: Arc::new(PaneTerminal::new(
                GhosttyPaneTerminal::new(terminal, tx.clone()).unwrap(),
            )),
            io: PaneRuntimeIo::TestChannel {
                sender: tx,
                resize_tx,
            },
            current_size: Cell::new((80, 24, 0, 0)),
            child_pid: Arc::new(AtomicU32::new(0)),
            child_wait_completed: None,
            kitty_keyboard_flags: Arc::new(AtomicU16::new(0)),
            content_seq: Arc::new(AtomicU64::new(0)),
            full_lifecycle_authority_active: Arc::new(AtomicBool::new(false)),
            detection_content_seq: Arc::new(AtomicU64::new(0)),
            detect_reset_notify: Arc::new(Notify::new()),
            pending_release: Arc::new(Mutex::new(None)),
            preserve_processes_on_drop: true,
            detect_handle: None,
        };

        assert!(runtime.try_send_focus_event(crate::ghostty::FocusEvent::Gained));
        assert_eq!(rx.recv().await.unwrap(), Bytes::from_static(b"\x1b[I"));
    }
    #[tokio::test]
    async fn focus_events_are_suppressed_when_disabled() {
        let (tx, mut rx) = mpsc::channel(4);
        let (resize_tx, _resize_rx) = watch::channel((80, 24, 0, 0));
        let terminal = crate::ghostty::Terminal::new(80, 24, 0).unwrap();
        let runtime = PaneRuntime {
            pane_id: PaneId::from_raw(0),
            terminal: Arc::new(PaneTerminal::new(
                GhosttyPaneTerminal::new(terminal, tx.clone()).unwrap(),
            )),
            io: PaneRuntimeIo::TestChannel {
                sender: tx,
                resize_tx,
            },
            current_size: Cell::new((80, 24, 0, 0)),
            child_pid: Arc::new(AtomicU32::new(0)),
            child_wait_completed: None,
            kitty_keyboard_flags: Arc::new(AtomicU16::new(0)),
            content_seq: Arc::new(AtomicU64::new(0)),
            full_lifecycle_authority_active: Arc::new(AtomicBool::new(false)),
            detection_content_seq: Arc::new(AtomicU64::new(0)),
            detect_reset_notify: Arc::new(Notify::new()),
            pending_release: Arc::new(Mutex::new(None)),
            preserve_processes_on_drop: true,
            detect_handle: None,
        };

        assert!(!runtime.try_send_focus_event(crate::ghostty::FocusEvent::Gained));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), rx.recv())
                .await
                .is_err()
        );
    }

    #[test]
    fn foreground_shell_without_agent_is_immediate_clear_signal() {
        assert!(should_clear_agent_for_foreground_shell(
            Some(Agent::Claude),
            None,
            true
        ));
    }

    #[test]
    fn foreground_shell_reports_process_exit_before_clearing_agent() {
        assert_eq!(
            foreground_shell_agent_action(Some(Agent::Codex), None, true, false),
            ForegroundShellAgentAction::ReportProcessExit
        );
        assert_eq!(
            foreground_shell_agent_action(Some(Agent::Codex), None, true, true),
            ForegroundShellAgentAction::ClearAgent
        );
        assert_eq!(
            foreground_shell_agent_action(Some(Agent::Codex), Some(Agent::Codex), true, true),
            ForegroundShellAgentAction::ReportReplacementProcess
        );
    }

    #[test]
    fn lifecycle_authority_keeps_process_exit_and_release_checks_live() {
        assert!(full_lifecycle_authority_should_skip_screen_detection(
            true, false, None
        ));
        assert!(!full_lifecycle_authority_should_skip_screen_detection(
            true, true, None
        ));
        assert!(!full_lifecycle_authority_should_skip_screen_detection(
            true,
            false,
            Some(Agent::OhMyPi)
        ));
        assert!(!full_lifecycle_authority_should_skip_screen_detection(
            false, false, None
        ));
    }

    #[test]
    fn stable_visible_idle_republishes_for_stale_hook_deadline() {
        let now = std::time::Instant::now();
        let previous = DetectionPublishState {
            state: AgentState::Idle,
            visible_blocker: false,
            visible_idle: true,
            visible_working: false,
        };
        let refresh_due = stable_visible_signal_refresh_due(
            previous,
            previous,
            Some(now - STABLE_VISIBLE_SIGNAL_REFRESH),
            now,
        );

        assert!(should_publish_detection_update(
            previous,
            previous,
            false,
            false,
            refresh_due
        ));
    }

    #[test]
    fn stable_plain_idle_does_not_republish() {
        let now = std::time::Instant::now();
        let previous = DetectionPublishState {
            state: AgentState::Idle,
            visible_blocker: false,
            visible_idle: false,
            visible_working: false,
        };
        let refresh_due = stable_visible_signal_refresh_due(
            previous,
            previous,
            Some(now - STABLE_VISIBLE_SIGNAL_REFRESH),
            now,
        );

        assert!(!should_publish_detection_update(
            previous,
            previous,
            false,
            false,
            refresh_due
        ));
    }

    #[test]
    fn stable_visible_working_republishes_for_hook_override_refresh() {
        let now = std::time::Instant::now();
        let previous = DetectionPublishState {
            state: AgentState::Working,
            visible_blocker: false,
            visible_idle: false,
            visible_working: true,
        };
        let refresh_due = stable_visible_signal_refresh_due(
            previous,
            previous,
            Some(now - STABLE_VISIBLE_SIGNAL_REFRESH),
            now,
        );

        assert!(should_publish_detection_update(
            previous,
            previous,
            false,
            false,
            refresh_due
        ));
    }

    #[test]
    fn stable_visible_blocker_republishes_for_hook_override_refresh() {
        let now = std::time::Instant::now();
        let previous = DetectionPublishState {
            state: AgentState::Blocked,
            visible_blocker: true,
            visible_idle: false,
            visible_working: false,
        };
        let refresh_due = stable_visible_signal_refresh_due(
            previous,
            previous,
            Some(now - STABLE_VISIBLE_SIGNAL_REFRESH),
            now,
        );

        assert!(should_publish_detection_update(
            previous,
            previous,
            false,
            false,
            refresh_due
        ));
    }

    #[test]
    fn unknown_non_shell_foreground_job_is_not_immediate_clear_signal() {
        assert!(!should_clear_agent_for_foreground_shell(
            Some(Agent::Claude),
            None,
            false
        ));
    }

    #[test]
    fn foreground_agent_job_is_not_clear_signal() {
        assert!(!should_clear_agent_for_foreground_shell(
            Some(Agent::Claude),
            Some(Agent::OpenCode),
            true
        ));
    }

    #[test]
    fn transient_process_miss_keeps_current_agent_detected() {
        let mut presence = AgentDetectionPresence::from_agent(Some(Agent::Pi));

        let changed = presence.observe_process_probe(None);

        assert!(!changed, "one miss should not clear the detected agent");
        assert_eq!(presence.current_agent(), Some(Agent::Pi));
    }

    #[test]
    fn agent_only_clears_after_confirmation_misses() {
        let mut presence = AgentDetectionPresence::from_agent(Some(Agent::Pi));

        for attempt in 1..AGENT_MISS_CONFIRMATION_ATTEMPTS {
            let changed = presence.observe_process_probe(None);
            assert!(
                !changed,
                "miss {attempt} should stay in the confirmation window"
            );
            assert_eq!(presence.current_agent(), Some(Agent::Pi));
        }

        let changed = presence.observe_process_probe(None);
        assert!(changed, "last confirmation miss should clear the agent");
        assert_eq!(presence.current_agent(), None);
    }

    #[tokio::test]
    async fn state_changed_event_waits_for_queue_space_instead_of_dropping() {
        let (tx, mut rx) = mpsc::channel(1);
        let pane_id = PaneId::from_raw(42);

        tx.try_send(AppEvent::UpdateReady {
            version: "9.9.9".into(),
            install: crate::install::UpdateInstallAction::Direct,
        })
        .unwrap();

        let publish = publish_state_changed_event(
            tx.clone(),
            pane_id,
            Some(Agent::Pi),
            AgentState::Idle,
            false,
            false,
            false,
            false,
            std::time::Instant::now(),
        );
        tokio::pin!(publish);

        let blocked = tokio::time::timeout(std::time::Duration::from_millis(20), async {
            (&mut publish).await;
        })
        .await;
        assert!(
            blocked.is_err(),
            "publisher should wait for queue space instead of dropping StateChanged"
        );

        let first = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv())
            .await
            .expect("queue should yield first event")
            .expect("sender still alive");
        assert!(matches!(first, AppEvent::UpdateReady { .. }));

        tokio::time::timeout(std::time::Duration::from_millis(50), async {
            (&mut publish).await;
        })
        .await
        .expect("publisher should complete once queue space is available");

        let second = tokio::time::timeout(std::time::Duration::from_millis(50), rx.recv())
            .await
            .expect("queue should yield second event")
            .expect("sender still alive");
        assert!(matches!(
            second,
            AppEvent::StateChanged {
                pane_id: delivered_pane,
                agent: Some(Agent::Pi),
                state: AgentState::Idle,
                visible_blocker: false,
                visible_idle: false,
                visible_working: false,
                process_exited: false,
                observed_at: _,
            } if delivered_pane == pane_id
        ));
    }
}
