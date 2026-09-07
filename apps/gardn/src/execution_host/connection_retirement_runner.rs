//! Cross-session connection retirement runner.
//!
//! Applies dormant placement-only rewrites, ensures every session that still
//! references the retiring host is cleaned through the live coordinator API,
//! then invokes a caller-supplied remote-binding finalizer. Global SSH profile
//! deletion stays with the caller.

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::api::client::{parse_response_value, ApiClient, ConnectionTarget};
use crate::api::schema::{
    ConnectionRetireParams, EmptyParams, Method, Request, ResponseResult, SuccessResponse,
};
use crate::execution_host::connection_retirement::{
    apply_dormant_connection_retirement, ConnectionRetirementError, ConnectionRetirementPlan,
    DormantApplyOutcome, SessionRetirementImpact,
};
use crate::execution_host::runtime_paths::BindingRetirementReport;
use crate::session::{self, DEFAULT_SESSION_NAME};

const DEFAULT_API_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_COORDINATOR_READY_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_RETIRE_POLL_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_RETIRE_POLL_INTERVAL: Duration = Duration::from_millis(100);
const DEFAULT_STOP_TIMEOUT: Duration = Duration::from_secs(10);
const COORDINATOR_READY_POLL: Duration = Duration::from_millis(50);

/// Compact report for UI completion after a successful global retirement run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct ConnectionRetirementRunReport {
    pub(crate) sessions_handled: usize,
    pub(crate) dormant_rewrites: usize,
    pub(crate) dormant_already_clean: usize,
    pub(crate) coordinators_started: usize,
    pub(crate) remote_bindings_removed: usize,
}

/// Failures while executing a connection retirement plan across sessions.
#[derive(Debug)]
pub(crate) enum ConnectionRetirementRunError {
    DormantApply(ConnectionRetirementError),
    Session {
        session_name: String,
        detail: String,
    },
    RemoteBinding {
        detail: String,
        blocked_bindings: usize,
    },
    Io(std::io::Error),
}

impl fmt::Display for ConnectionRetirementRunError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DormantApply(error) => write!(formatter, "dormant retirement failed: {error}"),
            Self::Session {
                session_name,
                detail,
            } => write!(
                formatter,
                "connection retirement failed for session {session_name}: {detail}"
            ),
            Self::RemoteBinding {
                detail,
                blocked_bindings,
            } => {
                if *blocked_bindings > 0 {
                    write!(
                        formatter,
                        "remote worker binding retirement left {blocked_bindings} live binding(s): {detail}"
                    )
                } else {
                    write!(
                        formatter,
                        "remote worker binding retirement failed: {detail}"
                    )
                }
            }
            Self::Io(error) => write!(formatter, "connection retirement I/O failed: {error}"),
        }
    }
}

impl std::error::Error for ConnectionRetirementRunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::DormantApply(error) => Some(error),
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ConnectionRetirementError> for ConnectionRetirementRunError {
    fn from(error: ConnectionRetirementError) -> Self {
        Self::DormantApply(error)
    }
}

impl From<std::io::Error> for ConnectionRetirementRunError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Optional knobs for production and focused process-boundary tests.
#[derive(Debug, Clone)]
pub(crate) struct ConnectionRetirementRunnerOptions {
    pub(crate) api_request_timeout: Duration,
    pub(crate) coordinator_ready_timeout: Duration,
    pub(crate) retire_poll_timeout: Duration,
    pub(crate) retire_poll_interval: Duration,
    pub(crate) stop_timeout: Duration,
}

impl Default for ConnectionRetirementRunnerOptions {
    fn default() -> Self {
        Self {
            api_request_timeout: DEFAULT_API_REQUEST_TIMEOUT,
            coordinator_ready_timeout: DEFAULT_COORDINATOR_READY_TIMEOUT,
            retire_poll_timeout: DEFAULT_RETIRE_POLL_TIMEOUT,
            retire_poll_interval: DEFAULT_RETIRE_POLL_INTERVAL,
            stop_timeout: DEFAULT_STOP_TIMEOUT,
        }
    }
}

/// Process/API boundary used by the runner. Production uses real local sockets;
/// tests inject fakes only at this seam.
pub(crate) trait ConnectionRetirementCoordinatorTransport {
    fn is_api_ready(&self, socket_path: &Path) -> bool;
    fn start_coordinator(
        &self,
        session: &SessionRetirementImpact,
        socket_path: &Path,
    ) -> Result<OwnedCoordinator, ConnectionRetirementRunError>;
    fn stop_coordinator(
        &self,
        session_name: &str,
        socket_path: &Path,
        owned: &mut OwnedCoordinator,
        timeout: Duration,
    ) -> Result<(), ConnectionRetirementRunError>;
    fn request(
        &self,
        socket_path: &Path,
        request: &Request,
        timeout: Duration,
    ) -> Result<SuccessResponse, ConnectionRetirementRunError>;
}

/// Coordinator child owned by this retirement run (never a pre-existing user server).
#[derive(Debug)]
pub(crate) struct OwnedCoordinator {
    child: Option<Child>,
}

impl OwnedCoordinator {
    fn from_child(child: Child) -> Self {
        Self { child: Some(child) }
    }

    #[cfg(test)]
    fn marker() -> Self {
        Self { child: None }
    }

    fn kill_best_effort(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
    }
}

impl Drop for OwnedCoordinator {
    fn drop(&mut self) {
        self.kill_best_effort();
    }
}

struct LocalCoordinatorTransport;

impl ConnectionRetirementCoordinatorTransport for LocalCoordinatorTransport {
    fn is_api_ready(&self, socket_path: &Path) -> bool {
        matches!(
            crate::api::read_runtime_status_at(socket_path, Duration::from_millis(250)),
            Ok(Some(_))
        )
    }

    fn start_coordinator(
        &self,
        session: &SessionRetirementImpact,
        socket_path: &Path,
    ) -> Result<OwnedCoordinator, ConnectionRetirementRunError> {
        if let Some(parent) = socket_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                session_error(
                    &session.session_name,
                    format!(
                        "failed to create session directory {}: {error}",
                        parent.display()
                    ),
                )
            })?;
        }

        let exe = std::env::current_exe().map_err(|error| {
            session_error(
                &session.session_name,
                format!("failed to resolve current gardn binary: {error}"),
            )
        })?;

        let mut command = Command::new(&exe);
        command.arg("server");
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // Force the child onto this session's API socket and clear inherited
        // overrides so retirement never attaches to the wrong coordinator.
        command.env(crate::api::SOCKET_PATH_ENV_VAR, socket_path);
        command.env_remove("GARDN_CLIENT_SOCKET_PATH");
        if session.is_default || session.session_name == DEFAULT_SESSION_NAME {
            command.env_remove(session::SESSION_ENV_VAR);
        } else {
            command.env(session::SESSION_ENV_VAR, &session.session_name);
        }

        let child = command.spawn().map_err(|error| {
            session_error(
                &session.session_name,
                format!("failed to start headless coordinator: {error}"),
            )
        })?;
        Ok(OwnedCoordinator::from_child(child))
    }

    fn stop_coordinator(
        &self,
        session_name: &str,
        socket_path: &Path,
        owned: &mut OwnedCoordinator,
        timeout: Duration,
    ) -> Result<(), ConnectionRetirementRunError> {
        let stop_request = Request {
            id: format!("connection-retire:stop:{session_name}"),
            method: Method::ServerStop(EmptyParams::default()),
        };
        // Best-effort graceful stop; ownership means we may hard-kill afterward.
        let _ = self.request(socket_path, &stop_request, DEFAULT_API_REQUEST_TIMEOUT);

        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let child_exited = match owned.child.as_mut() {
                Some(child) => match child.try_wait() {
                    Ok(Some(_)) => {
                        owned.child = None;
                        true
                    }
                    Ok(None) => false,
                    Err(_) => false,
                },
                None => true,
            };
            let api_gone =
                !socket_path.exists() || crate::ipc::connect_local_stream(socket_path).is_err();
            if child_exited && api_gone {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(25));
        }

        owned.kill_best_effort();
        if socket_path.exists() && crate::ipc::connect_local_stream(socket_path).is_ok() {
            return Err(session_error(
                session_name,
                format!(
                    "owned coordinator did not stop within {}ms at {}",
                    timeout.as_millis(),
                    socket_path.display()
                ),
            ));
        }
        Ok(())
    }

    fn request(
        &self,
        socket_path: &Path,
        request: &Request,
        timeout: Duration,
    ) -> Result<SuccessResponse, ConnectionRetirementRunError> {
        let client = ApiClient::for_target(ConnectionTarget::SocketPath(socket_path.to_path_buf()));
        client
            .request_value_with_timeout(request, timeout)
            .and_then(parse_response_value)
            .map_err(|error| {
                ConnectionRetirementRunError::Io(std::io::Error::other(error.to_string()))
            })
    }
}

/// Execute a prepared retirement plan across every affected session.
///
/// `retire_remote_bindings` is invoked only after every session is ready. The
/// caller owns SSH/auth (typically `WorkerInstaller::retire_owned_bindings`).
/// Any remaining live binding is treated as failure. The global SSH profile is
/// not deleted here.
pub(crate) fn run_connection_retirement<F>(
    plan: &ConnectionRetirementPlan,
    profile_id: &str,
    retire_remote_bindings: F,
) -> Result<ConnectionRetirementRunReport, ConnectionRetirementRunError>
where
    F: FnOnce() -> Result<BindingRetirementReport, String>,
{
    run_connection_retirement_with(
        plan,
        profile_id,
        retire_remote_bindings,
        &LocalCoordinatorTransport,
        ConnectionRetirementRunnerOptions::default(),
    )
}

/// Forget local session state without claiming remote runtime or binding cleanup.
pub(crate) fn run_connection_local_forget(
    plan: &ConnectionRetirementPlan,
    profile_id: &str,
) -> Result<ConnectionRetirementRunReport, ConnectionRetirementRunError> {
    run_connection_local_forget_with(
        plan,
        profile_id,
        &LocalCoordinatorTransport,
        ConnectionRetirementRunnerOptions::default(),
    )
}

fn run_connection_local_forget_with<T: ConnectionRetirementCoordinatorTransport>(
    plan: &ConnectionRetirementPlan,
    profile_id: &str,
    transport: &T,
    options: ConnectionRetirementRunnerOptions,
) -> Result<ConnectionRetirementRunReport, ConnectionRetirementRunError> {
    let mut report = ConnectionRetirementRunReport {
        sessions_handled: plan.sessions.len(),
        dormant_rewrites: 0,
        dormant_already_clean: 0,
        coordinators_started: 0,
        remote_bindings_removed: 0,
    };
    let mut owned_coordinators = Vec::new();
    let run_result: Result<(), ConnectionRetirementRunError> = (|| {
        for session in &plan.sessions {
            retire_one_session(
                plan,
                profile_id,
                session,
                transport,
                &options,
                &mut owned_coordinators,
                &mut report,
                true,
            )?;
        }
        Ok(())
    })();
    let cleanup_errors =
        stop_owned_coordinators(transport, &mut owned_coordinators, options.stop_timeout);
    run_result?;
    if let Some(error) = cleanup_errors.into_iter().next() {
        return Err(error);
    }
    Ok(report)
}

/// Testable entry point with an injected coordinator transport and timeouts.
pub(crate) fn run_connection_retirement_with<F, T>(
    plan: &ConnectionRetirementPlan,
    profile_id: &str,
    retire_remote_bindings: F,
    transport: &T,
    options: ConnectionRetirementRunnerOptions,
) -> Result<ConnectionRetirementRunReport, ConnectionRetirementRunError>
where
    F: FnOnce() -> Result<BindingRetirementReport, String>,
    T: ConnectionRetirementCoordinatorTransport,
{
    let dormant = apply_dormant_connection_retirement(plan)?;
    let mut report = ConnectionRetirementRunReport {
        sessions_handled: plan.sessions.len(),
        dormant_rewrites: 0,
        dormant_already_clean: 0,
        coordinators_started: 0,
        remote_bindings_removed: 0,
    };
    for outcome in &dormant.applied {
        match outcome {
            DormantApplyOutcome::Rewritten { .. } => report.dormant_rewrites += 1,
            DormantApplyOutcome::AlreadyClean { .. } => report.dormant_already_clean += 1,
        }
    }

    let mut owned_coordinators: Vec<(String, PathBuf, OwnedCoordinator)> = Vec::new();
    let run_result: Result<(), ConnectionRetirementRunError> = (|| {
        for session in plan.requires_coordinator_or_running() {
            retire_one_session(
                plan,
                profile_id,
                session,
                transport,
                &options,
                &mut owned_coordinators,
                &mut report,
                false,
            )?;
        }
        Ok(())
    })();

    let cleanup_errors =
        stop_owned_coordinators(transport, &mut owned_coordinators, options.stop_timeout);
    run_result?;
    if let Some(error) = cleanup_errors.into_iter().next() {
        return Err(error);
    }

    let binding_report =
        retire_remote_bindings().map_err(|detail| ConnectionRetirementRunError::RemoteBinding {
            detail,
            blocked_bindings: 0,
        })?;
    if !binding_report.blocked_bindings.is_empty() {
        return Err(ConnectionRetirementRunError::RemoteBinding {
            detail: format!(
                "refusing to finish retirement while {} live execution-worker binding(s) remain",
                binding_report.blocked_bindings.len()
            ),
            blocked_bindings: binding_report.blocked_bindings.len(),
        });
    }
    report.remote_bindings_removed = binding_report.removed_bindings.len();
    Ok(report)
}

fn retire_one_session<T: ConnectionRetirementCoordinatorTransport>(
    plan: &ConnectionRetirementPlan,
    profile_id: &str,
    session: &SessionRetirementImpact,
    transport: &T,
    options: &ConnectionRetirementRunnerOptions,
    owned_coordinators: &mut Vec<(String, PathBuf, OwnedCoordinator)>,
    report: &mut ConnectionRetirementRunReport,
    local_only: bool,
) -> Result<(), ConnectionRetirementRunError> {
    let socket_path = session_socket_path(session);
    let mut started_here = false;

    if transport.is_api_ready(&socket_path) {
        // Pre-existing coordinator: use it, never take ownership/stop it.
    } else if session.running {
        // Plan said running but API is unreachable — fail closed.
        return Err(session_error(
            &session.session_name,
            format!(
                "session is marked running but API is unreachable at {} (remaining remote panes: {}, pending terminations: {})",
                socket_path.display(),
                session.remote_panes.len(),
                session.pending_terminations.len()
            ),
        ));
    } else {
        let owned = transport.start_coordinator(session, &socket_path)?;
        owned_coordinators.push((session.session_name.clone(), socket_path.clone(), owned));
        started_here = true;
        report.coordinators_started += 1;
        wait_for_api_ready(
            transport,
            &session.session_name,
            &socket_path,
            options.coordinator_ready_timeout,
        )?;
    }

    // If we did not start it, still require readiness before retire RPCs.
    if !started_here {
        wait_for_api_ready(
            transport,
            &session.session_name,
            &socket_path,
            options.coordinator_ready_timeout,
        )?;
    }

    let params = ConnectionRetireParams {
        profile_id: profile_id.to_string(),
        execution_host_id: plan.host_id.as_str().to_string(),
        local_only,
    };

    let start = transport.request(
        &socket_path,
        &Request {
            id: format!("connection-retire:start:{}", session.session_name),
            method: Method::ConnectionRetireStart(params.clone()),
        },
        options.api_request_timeout,
    );
    let start = start.map_err(|error| annotate_session(&session.session_name, error))?;
    match start.result {
        ResponseResult::ConnectionRetireStart {
            accepted,
            remaining_panes,
            remaining_terminals,
            pending_terminations,
            ..
        } => {
            if !accepted {
                return Err(session_error(
                    &session.session_name,
                    format!(
                        "connection.retire.start was not accepted (remaining panes: {remaining_panes}, terminals: {remaining_terminals}, pending terminations: {pending_terminations})"
                    ),
                ));
            }
        }
        other => {
            return Err(session_error(
                &session.session_name,
                format!("connection.retire.start returned unexpected result: {other:?}"),
            ));
        }
    }

    poll_until_ready(
        transport,
        &session.session_name,
        &socket_path,
        &params,
        options,
    )
}

fn poll_until_ready<T: ConnectionRetirementCoordinatorTransport>(
    transport: &T,
    session_name: &str,
    socket_path: &Path,
    params: &ConnectionRetireParams,
    options: &ConnectionRetirementRunnerOptions,
) -> Result<(), ConnectionRetirementRunError> {
    let deadline = Instant::now() + options.retire_poll_timeout;
    let mut last_detail = format!(
        "connection.retire.status did not report ready within {}ms",
        options.retire_poll_timeout.as_millis()
    );

    while Instant::now() < deadline {
        let status = transport.request(
            socket_path,
            &Request {
                id: format!("connection-retire:status:{session_name}"),
                method: Method::ConnectionRetireStatus(params.clone()),
            },
            options.api_request_timeout,
        );
        match status {
            Ok(response) => match response.result {
                ResponseResult::ConnectionRetireStatus {
                    ready,
                    remaining_panes,
                    remaining_terminals,
                    pending_terminations,
                    ..
                } => {
                    if ready {
                        if remaining_panes != 0
                            || remaining_terminals != 0
                            || pending_terminations != 0
                        {
                            return Err(session_error(
                                session_name,
                                format!(
                                    "connection.retire.status reported ready with remaining work (panes: {remaining_panes}, terminals: {remaining_terminals}, pending terminations: {pending_terminations})"
                                ),
                            ));
                        }
                        return Ok(());
                    }
                    last_detail = format!(
                        "session still has remaining panes: {remaining_panes}, terminals: {remaining_terminals}, pending terminations: {pending_terminations}"
                    );
                }
                other => {
                    return Err(session_error(
                        session_name,
                        format!("connection.retire.status returned unexpected result: {other:?}"),
                    ));
                }
            },
            Err(error) => {
                last_detail = error.to_string();
                // Keep polling on transient transport errors until the deadline.
            }
        }
        thread::sleep(options.retire_poll_interval);
    }

    Err(session_error(session_name, last_detail))
}

fn wait_for_api_ready<T: ConnectionRetirementCoordinatorTransport>(
    transport: &T,
    session_name: &str,
    socket_path: &Path,
    timeout: Duration,
) -> Result<(), ConnectionRetirementRunError> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if transport.is_api_ready(socket_path) {
            return Ok(());
        }
        thread::sleep(COORDINATOR_READY_POLL);
    }
    Err(session_error(
        session_name,
        format!(
            "coordinator API did not become ready within {}ms at {}",
            timeout.as_millis(),
            socket_path.display()
        ),
    ))
}

fn stop_owned_coordinators<T: ConnectionRetirementCoordinatorTransport>(
    transport: &T,
    owned: &mut Vec<(String, PathBuf, OwnedCoordinator)>,
    timeout: Duration,
) -> Vec<ConnectionRetirementRunError> {
    let mut errors = Vec::new();
    while let Some((session_name, socket_path, mut child)) = owned.pop() {
        if let Err(error) =
            transport.stop_coordinator(&session_name, &socket_path, &mut child, timeout)
        {
            errors.push(error);
        }
    }
    errors
}

fn session_socket_path(session: &SessionRetirementImpact) -> PathBuf {
    session
        .snapshot_path
        .parent()
        .map(|dir| dir.join("gardn.sock"))
        .unwrap_or_else(|| PathBuf::from("gardn.sock"))
}

fn session_error(session_name: &str, detail: impl Into<String>) -> ConnectionRetirementRunError {
    ConnectionRetirementRunError::Session {
        session_name: session_name.to_string(),
        detail: detail.into(),
    }
}

fn annotate_session(
    session_name: &str,
    error: ConnectionRetirementRunError,
) -> ConnectionRetirementRunError {
    match error {
        ConnectionRetirementRunError::Session { .. } => error,
        other => session_error(session_name, other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AgentPanelScope;
    use crate::execution_host::connection_retirement::{
        GroupDefaultImpact, GroupDefaultReplacement, RemotePaneImpact, WorkspaceDefaultImpact,
    };
    use crate::execution_host::runtime_paths::{
        BindingOwnershipManifest, OwnedBindingInventoryEntry,
    };
    use crate::execution_host::{ExecutionHostId, HostPath, ResourceLocation};
    use crate::persist::{
        try_load_snapshot_at, try_save_snapshot_at, GroupSnapshot, LayoutSnapshot, PaneSnapshot,
        SessionDefaultViewSnapshot, SessionSnapshot, SessionUiSnapshot, TabSnapshot,
        WorkspaceSnapshot,
    };
    use std::collections::{HashMap, VecDeque};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Debug, Clone)]
    enum ScriptedResponse {
        Success(Box<ResponseResult>),
        Unreachable,
    }

    #[derive(Debug, Default)]
    struct FakeTransportState {
        ready_sockets: HashMap<PathBuf, bool>,
        started: Vec<String>,
        stopped: Vec<String>,
        requests: Vec<(String, String)>,
        local_forget_starts: usize,
        responses: HashMap<PathBuf, VecDeque<ScriptedResponse>>,
        start_error: HashMap<String, String>,
    }

    #[derive(Clone, Default)]
    struct FakeTransport {
        state: Arc<Mutex<FakeTransportState>>,
    }

    impl FakeTransport {
        fn lock(&self) -> std::sync::MutexGuard<'_, FakeTransportState> {
            self.state.lock().expect("transport lock")
        }

        fn mark_ready(&self, socket: impl Into<PathBuf>, ready: bool) {
            self.lock().ready_sockets.insert(socket.into(), ready);
        }

        fn push_response(&self, socket: impl Into<PathBuf>, response: ScriptedResponse) {
            self.lock()
                .responses
                .entry(socket.into())
                .or_default()
                .push_back(response);
        }

        fn started(&self) -> Vec<String> {
            self.lock().started.clone()
        }

        fn stopped(&self) -> Vec<String> {
            self.lock().stopped.clone()
        }

        fn request_methods(&self) -> Vec<String> {
            self.lock()
                .requests
                .iter()
                .map(|(_, method)| method.clone())
                .collect()
        }
        fn local_forget_starts(&self) -> usize {
            self.lock().local_forget_starts
        }
    }

    impl ConnectionRetirementCoordinatorTransport for FakeTransport {
        fn is_api_ready(&self, socket_path: &Path) -> bool {
            self.lock()
                .ready_sockets
                .get(socket_path)
                .copied()
                .unwrap_or(false)
        }

        fn start_coordinator(
            &self,
            session: &SessionRetirementImpact,
            socket_path: &Path,
        ) -> Result<OwnedCoordinator, ConnectionRetirementRunError> {
            let mut state = self.lock();
            if let Some(detail) = state.start_error.get(&session.session_name).cloned() {
                return Err(session_error(&session.session_name, detail));
            }
            state.started.push(session.session_name.clone());
            state.ready_sockets.insert(socket_path.to_path_buf(), true);
            Ok(OwnedCoordinator::marker())
        }

        fn stop_coordinator(
            &self,
            session_name: &str,
            _socket_path: &Path,
            owned: &mut OwnedCoordinator,
            _timeout: Duration,
        ) -> Result<(), ConnectionRetirementRunError> {
            self.lock().stopped.push(session_name.to_string());
            owned.child = None;
            Ok(())
        }

        fn request(
            &self,
            socket_path: &Path,
            request: &Request,
            _timeout: Duration,
        ) -> Result<SuccessResponse, ConnectionRetirementRunError> {
            let method = match &request.method {
                Method::ConnectionRetireStart(_) => "connection.retire.start",
                Method::ConnectionRetireStatus(_) => "connection.retire.status",
                Method::ServerStop(_) => "server.stop",
                _ => "other",
            };
            let mut state = self.lock();
            if matches!(
                &request.method,
                Method::ConnectionRetireStart(ConnectionRetireParams {
                    local_only: true,
                    ..
                })
            ) {
                state.local_forget_starts += 1;
            }
            state
                .requests
                .push((socket_path.display().to_string(), method.to_string()));
            let next = state
                .responses
                .get_mut(socket_path)
                .and_then(|queue| queue.pop_front())
                .unwrap_or(ScriptedResponse::Unreachable);
            match next {
                ScriptedResponse::Success(result) => Ok(SuccessResponse {
                    id: request.id.clone(),
                    result: *result,
                }),
                ScriptedResponse::Unreachable => {
                    Err(ConnectionRetirementRunError::Io(std::io::Error::new(
                        std::io::ErrorKind::ConnectionRefused,
                        format!("unreachable {}", socket_path.display()),
                    )))
                }
            }
        }
    }

    fn host(id: &str) -> ExecutionHostId {
        ExecutionHostId::new(id).expect("host id")
    }

    fn location(host_id: &ExecutionHostId, path: &str) -> ResourceLocation {
        ResourceLocation::new(host_id.clone(), HostPath::new(path).expect("path"))
    }

    fn empty_snapshot() -> SessionSnapshot {
        SessionSnapshot {
            version: 6,
            session_namespace_id: "session-test".to_string(),
            remote_termination_tombstones: Vec::new(),
            groups: vec![GroupSnapshot {
                id: crate::workspace::DEFAULT_GROUP_ID.to_string(),
                name: "group 1".to_string(),
                icon: crate::app::state::DEFAULT_GROUP_ICON.to_string(),
                accent: None,
                default_location: None,
                favorite_agent_profile_ids: Vec::new(),
                default_agent_profile_id: None,
                github_organization: None,
            }],
            active_group: 0,
            group_filter_enabled: true,
            default_view: SessionDefaultViewSnapshot::default(),
            workspaces: Vec::new(),
            active: None,
            selected: 0,
            agent_panel_scope: AgentPanelScope::CurrentWorkspace,
            sidebar_width: Some(26),
            sidebar_collapsed: false,
            sidebar_section_split: Some(0.5),
            right_sidebar_width: Some(28),
            right_sidebar_collapsed: false,
            ui: SessionUiSnapshot::default(),
            agent_follow_up: Vec::new(),
            pane_id_aliases: HashMap::new(),
        }
    }

    fn workspace_at(id: &str, default_location: ResourceLocation) -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            id: Some(id.to_string()),
            custom_name: Some(id.to_string()),
            group_id: crate::workspace::DEFAULT_GROUP_ID.to_string(),
            identity_cwd: PathBuf::from("/tmp/project"),
            default_location,
            github_scope: crate::github::GithubRepositoryScope::default(),
            public_pane_numbers: HashMap::new(),
            next_public_pane_number: 0,
            public_tab_numbers: Vec::new(),
            next_public_tab_number: 0,
            tabs: vec![TabSnapshot {
                custom_name: None,
                role: crate::workspace::TabRole::Terminal,
                layout: LayoutSnapshot::Pane(0),
                panes: HashMap::from([(
                    0,
                    PaneSnapshot {
                        cwd: PathBuf::from("/tmp/project"),
                        location: None,
                        remote_runtime_identity: None,
                        env_pane_id: None,
                        label: Some("keep-me".to_string()),
                        agent_name: None,
                        agent_session: None,
                        launch_argv: None,
                        launch_env: Vec::new(),
                        terminal_theme_binding: None,
                        seen: true,
                        right_click_passthrough: false,
                        terminal_semantics: None,
                    },
                )]),
                zoomed: false,
                focused: Some(0),
                root_pane: Some(0),
            }],
            active_tab: 0,
        }
    }

    fn temp_dir(label: &str) -> PathBuf {
        let unique = format!(
            "gardn-connection-retirement-runner-{}-{}-{}",
            label,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }

    fn impact(
        name: &str,
        is_default: bool,
        running: bool,
        snapshot_path: PathBuf,
        remote_panes: Vec<RemotePaneImpact>,
        group_defaults: Vec<GroupDefaultImpact>,
        workspace_defaults: Vec<WorkspaceDefaultImpact>,
    ) -> SessionRetirementImpact {
        SessionRetirementImpact {
            session_name: name.to_string(),
            is_default,
            running,
            snapshot_path,
            group_defaults,
            workspace_defaults,
            remote_panes,
            pending_terminations: Vec::new(),
        }
    }

    fn retire_start_ok() -> ScriptedResponse {
        ScriptedResponse::Success(Box::new(ResponseResult::ConnectionRetireStart {
            profile_id: "workbox".into(),
            execution_host_id: "ssh:workbox:1".into(),
            accepted: true,
            remaining_panes: 0,
            remaining_terminals: 0,
            pending_terminations: 0,
        }))
    }

    fn retire_status(ready: bool, panes: usize) -> ScriptedResponse {
        ScriptedResponse::Success(Box::new(ResponseResult::ConnectionRetireStatus {
            profile_id: "workbox".into(),
            execution_host_id: "ssh:workbox:1".into(),
            ready,
            remaining_panes: panes,
            remaining_terminals: panes,
            pending_terminations: 0,
        }))
    }

    fn binding_report(removed: usize, blocked: usize) -> BindingRetirementReport {
        let entry = |idx: usize| OwnedBindingInventoryEntry {
            ownership: BindingOwnershipManifest {
                coordinator_installation_id: "install-a".into(),
                session_namespace_id: format!("session-{idx}"),
                execution_host_id: "ssh:workbox:1".into(),
                host_binding_generation: 1,
                worker_instance_id: format!("worker-{idx}"),
                pid: 1,
                app_version: "test".into(),
                worker_protocol: 1,
                daemon_lifecycle_version: 1,
            },
            binding_root: format!("/tmp/binding-{idx}"),
            runtime_dir: format!("/tmp/binding-{idx}/runtime"),
            lock_live: blocked > 0,
        };
        BindingRetirementReport {
            removed_bindings: (0..removed).map(entry).collect(),
            blocked_bindings: (0..blocked).map(|idx| entry(100 + idx)).collect(),
        }
    }

    fn fast_options() -> ConnectionRetirementRunnerOptions {
        ConnectionRetirementRunnerOptions {
            api_request_timeout: Duration::from_millis(50),
            coordinator_ready_timeout: Duration::from_millis(50),
            retire_poll_timeout: Duration::from_millis(80),
            retire_poll_interval: Duration::from_millis(5),
            stop_timeout: Duration::from_millis(50),
        }
    }

    #[test]
    fn finalizer_runs_only_after_every_session_is_ready() {
        let retiring = host("ssh:workbox:1");
        let root = temp_dir("finalizer-order");
        let running_socket = root.join("running").join("gardn.sock");
        let dormant_socket = root.join("dormant").join("gardn.sock");
        std::fs::create_dir_all(running_socket.parent().unwrap()).unwrap();
        std::fs::create_dir_all(dormant_socket.parent().unwrap()).unwrap();

        let plan = ConnectionRetirementPlan {
            host_id: retiring.clone(),
            sessions: vec![
                impact(
                    "running",
                    false,
                    true,
                    running_socket.parent().unwrap().join("session.json"),
                    vec![RemotePaneImpact {
                        workspace_index: 0,
                        tab_index: 0,
                        pane_id: 1,
                        location: location(&retiring, "/srv/a"),
                    }],
                    Vec::new(),
                    Vec::new(),
                ),
                impact(
                    "dormant-remote",
                    false,
                    false,
                    dormant_socket.parent().unwrap().join("session.json"),
                    vec![RemotePaneImpact {
                        workspace_index: 0,
                        tab_index: 0,
                        pane_id: 2,
                        location: location(&retiring, "/srv/b"),
                    }],
                    Vec::new(),
                    Vec::new(),
                ),
            ],
        };

        let transport = FakeTransport::default();
        transport.mark_ready(&running_socket, true);
        // dormant starts unreadied; start_coordinator marks ready.
        transport.push_response(&running_socket, retire_start_ok());
        transport.push_response(&running_socket, retire_status(true, 0));
        transport.push_response(&dormant_socket, retire_start_ok());
        transport.push_response(&dormant_socket, retire_status(false, 1));
        transport.push_response(&dormant_socket, retire_status(true, 0));

        let order = Arc::new(Mutex::new(Vec::new()));
        let order_for_finalizer = Arc::clone(&order);
        let report = run_connection_retirement_with(
            &plan,
            "workbox",
            move || {
                order_for_finalizer
                    .lock()
                    .expect("order")
                    .push("finalizer".to_string());
                Ok(binding_report(2, 0))
            },
            &transport,
            fast_options(),
        )
        .expect("retirement should succeed");

        assert_eq!(report.sessions_handled, 2);
        assert_eq!(report.coordinators_started, 1);
        assert_eq!(report.remote_bindings_removed, 2);
        assert_eq!(transport.started(), vec!["dormant-remote".to_string()]);
        assert_eq!(transport.stopped(), vec!["dormant-remote".to_string()]);
        // Pre-existing running coordinator must not be stopped.
        assert!(!transport.stopped().iter().any(|name| name == "running"));

        let methods = transport.request_methods();
        assert!(
            methods
                .iter()
                .any(|method| method == "connection.retire.start"),
            "expected retire start calls, got {methods:?}"
        );
        assert!(
            methods
                .iter()
                .any(|method| method == "connection.retire.status"),
            "expected retire status calls, got {methods:?}"
        );
        let recorded = order.lock().expect("order");
        assert_eq!(recorded.as_slice(), ["finalizer"]);
        // Finalizer is last side effect after session retire traffic.
        assert_eq!(
            methods.last().map(String::as_str).unwrap_or(""),
            "connection.retire.status"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_forget_coordinates_dormant_placement_only_sessions() {
        let retiring = host("ssh:workbox:1");
        let root = temp_dir("local-forget");
        let snapshot_path = root.join("sessions").join("work").join("session.json");
        let socket_path = snapshot_path
            .parent()
            .expect("session dir")
            .join("gardn.sock");
        let plan = ConnectionRetirementPlan {
            host_id: retiring.clone(),
            sessions: vec![impact(
                "work",
                false,
                false,
                snapshot_path,
                Vec::new(),
                vec![GroupDefaultImpact {
                    group_id: crate::workspace::DEFAULT_GROUP_ID.to_string(),
                    group_name: "group 1".to_string(),
                    previous: location(&retiring, "/srv/group"),
                    replacement: GroupDefaultReplacement::Unset,
                }],
                Vec::new(),
            )],
        };
        let transport = FakeTransport::default();
        transport.push_response(&socket_path, retire_start_ok());
        transport.push_response(&socket_path, retire_status(true, 0));

        let report = run_connection_local_forget_with(&plan, "workbox", &transport, fast_options())
            .expect("local forget");

        assert_eq!(report.sessions_handled, 1);
        assert_eq!(report.coordinators_started, 1);
        assert_eq!(report.remote_bindings_removed, 0);
        assert_eq!(transport.started(), vec!["work".to_string()]);
        assert_eq!(transport.stopped(), vec!["work".to_string()]);
        assert_eq!(transport.local_forget_starts(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dormant_placement_only_sessions_are_rewritten_without_coordinator() {
        let retiring = host("ssh:workbox:1");
        let root = temp_dir("dormant-rewrite");
        // Named session avoids probing the developer's live default API socket
        // via session_is_running() inside apply_dormant_session_retirement.
        let session_dir = root.join("sessions").join("work");
        std::fs::create_dir_all(&session_dir).expect("session dir");
        let snapshot_path = session_dir.join("session.json");

        let mut snapshot = empty_snapshot();
        snapshot.groups[0].default_location = Some(location(&retiring, "/srv/group"));
        snapshot
            .workspaces
            .push(workspace_at("ws", location(&retiring, "/srv/ws")));
        try_save_snapshot_at(&snapshot_path, &snapshot).expect("save");

        let plan = ConnectionRetirementPlan {
            host_id: retiring.clone(),
            sessions: vec![impact(
                "work",
                false,
                false,
                snapshot_path.clone(),
                Vec::new(),
                vec![GroupDefaultImpact {
                    group_id: crate::workspace::DEFAULT_GROUP_ID.to_string(),
                    group_name: "group 1".to_string(),
                    previous: location(&retiring, "/srv/group"),
                    replacement: GroupDefaultReplacement::Unset,
                }],
                vec![WorkspaceDefaultImpact {
                    workspace_index: 0,
                    workspace_id: Some("ws".to_string()),
                    previous: location(&retiring, "/srv/ws"),
                    replacement: ResourceLocation::new(
                        ExecutionHostId::local(),
                        HostPath::new("/srv/ws").expect("path"),
                    ),
                }],
            )],
        };

        let transport = FakeTransport::default();
        let finalizer_calls = Arc::new(Mutex::new(0usize));
        let finalizer_calls_for_fn = Arc::clone(&finalizer_calls);
        let report = run_connection_retirement_with(
            &plan,
            "workbox",
            move || {
                *finalizer_calls_for_fn.lock().expect("calls") += 1;
                Ok(binding_report(1, 0))
            },
            &transport,
            fast_options(),
        )
        .expect("dormant rewrite path");

        assert_eq!(report.sessions_handled, 1);
        assert_eq!(report.dormant_rewrites, 1);
        assert_eq!(report.coordinators_started, 0);
        assert!(transport.started().is_empty());
        assert!(transport.request_methods().is_empty());
        assert_eq!(*finalizer_calls.lock().expect("calls"), 1);

        let rewritten = try_load_snapshot_at(&snapshot_path)
            .expect("load")
            .expect("snapshot");
        assert!(rewritten.groups[0].default_location.is_none());
        assert_eq!(
            rewritten.workspaces[0].default_location.execution_host_id,
            ExecutionHostId::local()
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn unreachable_running_session_fails_closed_without_finalizer() {
        let retiring = host("ssh:workbox:1");
        let root = temp_dir("unreachable");
        let snapshot_path = root.join("session.json");
        let socket_path = root.join("gardn.sock");

        let plan = ConnectionRetirementPlan {
            host_id: retiring.clone(),
            sessions: vec![impact(
                "alpha",
                false,
                true,
                snapshot_path,
                vec![RemotePaneImpact {
                    workspace_index: 0,
                    tab_index: 0,
                    pane_id: 9,
                    location: location(&retiring, "/srv/alpha"),
                }],
                Vec::new(),
                Vec::new(),
            )],
        };

        let transport = FakeTransport::default();
        transport.mark_ready(&socket_path, false);

        let finalizer_calls = Arc::new(Mutex::new(0usize));
        let finalizer_calls_for_fn = Arc::clone(&finalizer_calls);
        let error = run_connection_retirement_with(
            &plan,
            "workbox",
            move || {
                *finalizer_calls_for_fn.lock().expect("calls") += 1;
                Ok(binding_report(0, 0))
            },
            &transport,
            fast_options(),
        )
        .expect_err("unreachable running session must fail");

        match error {
            ConnectionRetirementRunError::Session {
                session_name,
                detail,
            } => {
                assert_eq!(session_name, "alpha");
                assert!(
                    detail.contains("unreachable") || detail.contains("marked running"),
                    "detail={detail}"
                );
            }
            other => panic!("unexpected error: {other}"),
        }
        assert_eq!(*finalizer_calls.lock().expect("calls"), 0);
        assert!(transport.started().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn blocked_remote_binding_fails_after_sessions_are_ready() {
        let retiring = host("ssh:workbox:1");
        let root = temp_dir("blocked-binding");
        let socket_path = root.join("gardn.sock");
        std::fs::create_dir_all(root.as_path()).unwrap();

        let plan = ConnectionRetirementPlan {
            host_id: retiring.clone(),
            sessions: vec![impact(
                "beta",
                false,
                true,
                root.join("session.json"),
                vec![RemotePaneImpact {
                    workspace_index: 0,
                    tab_index: 0,
                    pane_id: 3,
                    location: location(&retiring, "/srv/beta"),
                }],
                Vec::new(),
                Vec::new(),
            )],
        };

        let transport = FakeTransport::default();
        transport.mark_ready(&socket_path, true);
        transport.push_response(&socket_path, retire_start_ok());
        transport.push_response(&socket_path, retire_status(true, 0));

        let error = run_connection_retirement_with(
            &plan,
            "workbox",
            || Ok(binding_report(0, 1)),
            &transport,
            fast_options(),
        )
        .expect_err("live binding must fail closed");

        match error {
            ConnectionRetirementRunError::RemoteBinding {
                blocked_bindings, ..
            } => assert_eq!(blocked_bindings, 1),
            other => panic!("unexpected error: {other}"),
        }
        // Session retire completed before finalizer failure.
        assert!(transport
            .request_methods()
            .iter()
            .any(|method| method == "connection.retire.status"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn owned_coordinator_is_stopped_on_retire_failure() {
        let retiring = host("ssh:workbox:1");
        let root = temp_dir("stop-on-error");
        let socket_path = root.join("gardn.sock");
        std::fs::create_dir_all(&root).unwrap();

        let plan = ConnectionRetirementPlan {
            host_id: retiring.clone(),
            sessions: vec![impact(
                "gamma",
                false,
                false,
                root.join("session.json"),
                vec![RemotePaneImpact {
                    workspace_index: 0,
                    tab_index: 0,
                    pane_id: 4,
                    location: location(&retiring, "/srv/gamma"),
                }],
                Vec::new(),
                Vec::new(),
            )],
        };

        let transport = FakeTransport::default();
        // start marks ready; status never becomes ready.
        transport.push_response(&socket_path, retire_start_ok());
        transport.push_response(&socket_path, retire_status(false, 1));
        transport.push_response(&socket_path, retire_status(false, 1));
        transport.push_response(&socket_path, retire_status(false, 1));

        let finalizer_calls = Arc::new(Mutex::new(0usize));
        let finalizer_calls_for_fn = Arc::clone(&finalizer_calls);
        let error = run_connection_retirement_with(
            &plan,
            "workbox",
            move || {
                *finalizer_calls_for_fn.lock().expect("calls") += 1;
                Ok(binding_report(0, 0))
            },
            &transport,
            fast_options(),
        )
        .expect_err("hung retire must fail");

        match error {
            ConnectionRetirementRunError::Session { session_name, .. } => {
                assert_eq!(session_name, "gamma");
            }
            other => panic!("unexpected error: {other}"),
        }
        assert_eq!(transport.started(), vec!["gamma".to_string()]);
        assert_eq!(transport.stopped(), vec!["gamma".to_string()]);
        assert_eq!(*finalizer_calls.lock().expect("calls"), 0);
        let _ = std::fs::remove_dir_all(root);
    }
}
