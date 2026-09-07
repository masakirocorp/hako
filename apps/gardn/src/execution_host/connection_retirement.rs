//! Cross-session connection retirement planner and dormant-snapshot rewrite seam.
//!
//! Inventories every default and named session snapshot that references a
//! retiring execution host, discloses the exact placement and runtime impact,
//! and applies placement-only rewrites to dormant sessions that have no remote
//! panes or pending terminations.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::execution_host::{ExecutionHostId, ResourceLocation};
use crate::persist::SessionSnapshot;
use crate::session::SessionInfo;
use crate::terminal::TerminalId;

const RETIREMENT_JOURNAL_FILE: &str = "connection-retirement.json";
const RETIREMENT_JOURNAL_LOCK_FILE: &str = ".connection-retirement.lock";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub(crate) enum ApprovedConnectionRetirement {
    Full {
        plan: ConnectionRetirementPlan,
        bindings: crate::execution_host::runtime_paths::BindingInventoryReport,
    },
    LocalOnly {
        plan: ConnectionRetirementPlan,
    },
}

impl ApprovedConnectionRetirement {
    pub(crate) fn host_id(&self) -> &ExecutionHostId {
        match self {
            Self::Full { plan, .. } | Self::LocalOnly { plan } => &plan.host_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingConnectionRetirement {
    pub(crate) profile_id: String,
    pub(crate) approved: ApprovedConnectionRetirement,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectionRetirementJournal {
    profile_id: String,
    approved: ApprovedConnectionRetirement,
    status: ConnectionRetirementJournalStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConnectionRetirementJournalStatus {
    Running,
    Paused { error: String },
}

/// Holds the installation-global retirement lock for one execution attempt.
#[derive(Debug)]
pub(crate) struct ConnectionRetirementJournalGuard {
    _lock: File,
    journal: ConnectionRetirementJournal,
}

impl ConnectionRetirementJournalGuard {
    pub(crate) fn pause(&mut self, error: &str) -> io::Result<()> {
        self.journal.status = ConnectionRetirementJournalStatus::Paused {
            error: error.to_string(),
        };
        crate::persist::atomic_json::save_json(&retirement_journal_path(), &self.journal)
    }
}

pub(crate) fn begin_connection_retirement_journal(
    profile_id: &str,
    approved: ApprovedConnectionRetirement,
) -> io::Result<ConnectionRetirementJournalGuard> {
    let config_dir = crate::config::config_dir();
    std::fs::create_dir_all(&config_dir)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(config_dir.join(RETIREMENT_JOURNAL_LOCK_FILE))?;
    lock.try_lock().map_err(|error| {
        io::Error::new(
            io::ErrorKind::WouldBlock,
            format!("another connection retirement is already running: {error}"),
        )
    })?;

    if let Some(existing) = load_connection_retirement_journal()? {
        if existing.profile_id != profile_id || existing.approved != approved {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                format!(
                    "connection retirement for profile {} must finish before removing another connection",
                    existing.profile_id
                ),
            ));
        }
    }

    let journal = ConnectionRetirementJournal {
        profile_id: profile_id.to_string(),
        approved,
        status: ConnectionRetirementJournalStatus::Running,
    };
    crate::persist::atomic_json::save_json(&retirement_journal_path(), &journal)?;
    Ok(ConnectionRetirementJournalGuard {
        _lock: lock,
        journal,
    })
}

pub(crate) fn pending_connection_retirement() -> io::Result<Option<PendingConnectionRetirement>> {
    load_connection_retirement_journal().map(|journal| {
        journal.map(|journal| PendingConnectionRetirement {
            profile_id: journal.profile_id,
            approved: journal.approved,
        })
    })
}

pub(crate) fn pending_connection_retirement_host() -> io::Result<Option<ExecutionHostId>> {
    pending_connection_retirement()?
        .map(|pending| Ok(pending.approved.host_id().clone()))
        .transpose()
}

pub(crate) fn complete_connection_retirement_journal(profile_id: &str) -> io::Result<()> {
    crate::persist::atomic_json::with_path_lock(&retirement_journal_path(), || {
        let Some(journal) = load_connection_retirement_journal()? else {
            return Ok(());
        };
        if journal.profile_id != profile_id {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "retirement journal belongs to a different connection profile",
            ));
        }
        match std::fs::remove_file(retirement_journal_path()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    })
}

fn retirement_journal_path() -> PathBuf {
    crate::config::config_dir().join(RETIREMENT_JOURNAL_FILE)
}

fn load_connection_retirement_journal() -> io::Result<Option<ConnectionRetirementJournal>> {
    let path = retirement_journal_path();
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Deterministic preview of every session snapshot that still references `host_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ConnectionRetirementPlan {
    pub(crate) host_id: ExecutionHostId,
    pub(crate) sessions: Vec<SessionRetirementImpact>,
}

impl ConnectionRetirementPlan {
    /// Sessions that still need a live coordinator (remote panes or tombstones)
    /// or that are currently running.
    pub(crate) fn requires_coordinator_or_running(
        &self,
    ) -> impl Iterator<Item = &SessionRetirementImpact> {
        self.sessions
            .iter()
            .filter(|session| session.running || session.requires_live_coordinator())
    }
}

/// One session's disclosed impact for a retiring execution host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SessionRetirementImpact {
    /// Display name (`"default"` or the named session id).
    pub(crate) session_name: String,
    pub(crate) is_default: bool,
    pub(crate) running: bool,
    /// Absolute path to this session's `session.json`.
    pub(crate) snapshot_path: PathBuf,
    pub(crate) group_defaults: Vec<GroupDefaultImpact>,
    pub(crate) workspace_defaults: Vec<WorkspaceDefaultImpact>,
    /// Panes on the retiring host — identified for closure, never reinterpreted.
    pub(crate) remote_panes: Vec<RemotePaneImpact>,
    pub(crate) pending_terminations: Vec<PendingTerminationImpact>,
}

impl SessionRetirementImpact {
    pub(crate) fn requires_live_coordinator(&self) -> bool {
        !self.remote_panes.is_empty() || !self.pending_terminations.is_empty()
    }

    pub(crate) fn has_placement_rewrites(&self) -> bool {
        !self.group_defaults.is_empty() || !self.workspace_defaults.is_empty()
    }
}

/// Group default that will be cleared (unset) on retirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GroupDefaultImpact {
    pub(crate) group_id: String,
    pub(crate) group_name: String,
    pub(crate) previous: ResourceLocation,
    pub(crate) replacement: GroupDefaultReplacement,
}

/// Explicit replacement contract for an affected group default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum GroupDefaultReplacement {
    /// Group default becomes unset — never silently rewritten to Local.
    Unset,
}

/// Workspace default that will move to the coordinator user's local home directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct WorkspaceDefaultImpact {
    pub(crate) workspace_index: usize,
    pub(crate) workspace_id: Option<String>,
    pub(crate) previous: ResourceLocation,
    pub(crate) replacement: ResourceLocation,
}

/// A pane whose location is on the retiring host. Closure only — never Local.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RemotePaneImpact {
    pub(crate) workspace_index: usize,
    pub(crate) tab_index: usize,
    pub(crate) pane_id: u32,
    pub(crate) location: ResourceLocation,
}

/// A pending remote-termination tombstone on the retiring host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PendingTerminationImpact {
    pub(crate) terminal_id: TerminalId,
    pub(crate) location: ResourceLocation,
}

/// Result of a successful dormant placement rewrite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DormantApplyOutcome {
    /// Snapshot had no remaining placement references after re-validation.
    AlreadyClean { session_name: String },
    /// Placement defaults were rewritten and persisted.
    Rewritten {
        session_name: String,
        groups_unset: usize,
        workspaces_localized: usize,
    },
}

/// Failures while planning or applying connection retirement.
#[derive(Debug)]
pub(crate) enum ConnectionRetirementError {
    Io(std::io::Error),
    InvalidSnapshot {
        path: PathBuf,
        detail: String,
    },
    SessionRunning {
        session_name: String,
    },
    RequiresLiveCoordinator {
        session_name: String,
        remote_panes: usize,
        pending_terminations: usize,
    },
}

impl fmt::Display for ConnectionRetirementError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::InvalidSnapshot { path, detail } => {
                write!(
                    formatter,
                    "cannot read session snapshot {}: {detail}",
                    path.display()
                )
            }
            Self::SessionRunning { session_name } => {
                write!(
                    formatter,
                    "session {session_name} is running; dormant retirement requires a stopped session"
                )
            }
            Self::RequiresLiveCoordinator {
                session_name,
                remote_panes,
                pending_terminations,
            } => write!(
                formatter,
                "session {session_name} still has {remote_panes} remote pane(s) and {pending_terminations} pending termination(s); live coordinator handling is required"
            ),
        }
    }
}

impl std::error::Error for ConnectionRetirementError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ConnectionRetirementError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// Scan default and named session snapshots and build a retirement plan.
///
/// Missing snapshots are skipped. Unreadable or invalid snapshots fail closed.
pub(crate) fn plan_connection_retirement(
    host_id: &ExecutionHostId,
) -> Result<ConnectionRetirementPlan, ConnectionRetirementError> {
    let sessions = crate::session::list_sessions()?;
    plan_connection_retirement_for_sessions(host_id, &sessions)
}

/// Build a plan from an explicit session list (test seam and callers with a
/// pre-fetched catalog).
pub(crate) fn plan_connection_retirement_for_sessions(
    host_id: &ExecutionHostId,
    sessions: &[SessionInfo],
) -> Result<ConnectionRetirementPlan, ConnectionRetirementError> {
    let mut impacts = Vec::new();
    for session in sessions {
        let snapshot_path = PathBuf::from(&session.session_dir).join("session.json");
        let Some(snapshot) = load_snapshot_fail_closed(&snapshot_path)? else {
            continue;
        };
        if let Some(impact) = inventory_session(host_id, session, snapshot_path, &snapshot) {
            impacts.push(impact);
        }
    }
    Ok(ConnectionRetirementPlan {
        host_id: host_id.clone(),
        sessions: impacts,
    })
}

/// Apply placement-only rewrites for one dormant session impact.
///
/// Re-loads and re-validates the snapshot so the operation stays fail-closed and
/// idempotent. Refuses running sessions and any snapshot that still carries
/// remote panes or pending terminations on the retiring host.
pub(crate) fn apply_dormant_session_retirement(
    host_id: &ExecutionHostId,
    impact: &SessionRetirementImpact,
) -> Result<DormantApplyOutcome, ConnectionRetirementError> {
    if impact.running || session_is_running(impact) {
        return Err(ConnectionRetirementError::SessionRunning {
            session_name: impact.session_name.clone(),
        });
    }

    let snapshot = match load_snapshot_fail_closed(&impact.snapshot_path)? {
        Some(snapshot) => snapshot,
        None => {
            return Ok(DormantApplyOutcome::AlreadyClean {
                session_name: impact.session_name.clone(),
            });
        }
    };

    let session = SessionInfo {
        name: impact.session_name.clone(),
        default: impact.is_default,
        running: false,
        socket_path: String::new(),
        session_dir: impact
            .snapshot_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .display()
            .to_string(),
    };
    let fresh = inventory_session(host_id, &session, impact.snapshot_path.clone(), &snapshot);
    let Some(fresh) = fresh else {
        return Ok(DormantApplyOutcome::AlreadyClean {
            session_name: impact.session_name.clone(),
        });
    };

    if fresh.requires_live_coordinator() {
        return Err(ConnectionRetirementError::RequiresLiveCoordinator {
            session_name: fresh.session_name,
            remote_panes: fresh.remote_panes.len(),
            pending_terminations: fresh.pending_terminations.len(),
        });
    }

    if !fresh.has_placement_rewrites() {
        return Ok(DormantApplyOutcome::AlreadyClean {
            session_name: fresh.session_name,
        });
    }

    let mut rewritten = snapshot;
    let summary = rewrite_snapshot_placements(&mut rewritten, host_id);
    crate::persist::try_save_snapshot_at(&impact.snapshot_path, &rewritten)?;

    Ok(DormantApplyOutcome::Rewritten {
        session_name: fresh.session_name,
        groups_unset: summary.groups_unset,
        workspaces_localized: summary.workspaces_localized,
    })
}

/// Apply dormant placement rewrites for every eligible session in `plan`.
///
/// Sessions that are running or require a live coordinator are left untouched
/// and returned in `blocked`. Eligible sessions are rewritten atomically and
/// collected in `applied`.
pub(crate) fn apply_dormant_connection_retirement(
    plan: &ConnectionRetirementPlan,
) -> Result<DormantRetirementReport, ConnectionRetirementError> {
    let mut applied = Vec::new();
    let mut blocked = Vec::new();

    for session in &plan.sessions {
        if session.running || session.requires_live_coordinator() {
            blocked.push(session.clone());
            continue;
        }
        if !session.has_placement_rewrites() {
            continue;
        }
        applied.push(apply_dormant_session_retirement(&plan.host_id, session)?);
    }

    Ok(DormantRetirementReport { applied, blocked })
}

/// Outcome of applying a full retirement plan's dormant subset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DormantRetirementReport {
    pub(crate) applied: Vec<DormantApplyOutcome>,
    pub(crate) blocked: Vec<SessionRetirementImpact>,
}

/// Counts produced by an in-memory placement rewrite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct PlacementRewriteSummary {
    pub(crate) groups_unset: usize,
    pub(crate) workspaces_localized: usize,
}

/// Pure placement rewrite contract:
/// - affected group defaults become unset
/// - affected workspace defaults move to the coordinator user's local home directory
/// - remote host paths are never reinterpreted as Local
/// - panes and pending terminations are never rewritten here
pub(crate) fn rewrite_snapshot_placements(
    snapshot: &mut SessionSnapshot,
    host_id: &ExecutionHostId,
) -> PlacementRewriteSummary {
    let mut summary = PlacementRewriteSummary::default();

    for group in &mut snapshot.groups {
        if group
            .default_location
            .as_ref()
            .is_some_and(|location| &location.execution_host_id == host_id)
        {
            group.default_location = None;
            summary.groups_unset += 1;
        }
    }

    for workspace in &mut snapshot.workspaces {
        if &workspace.default_location.execution_host_id == host_id {
            workspace.default_location = local_retirement_replacement();
            summary.workspaces_localized += 1;
        }
    }

    summary
}

pub(crate) fn local_retirement_replacement() -> ResourceLocation {
    let path = std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("/"));
    ResourceLocation::new(
        ExecutionHostId::local(),
        crate::execution_host::HostPath::new(path).unwrap_or_default(),
    )
}

fn load_snapshot_fail_closed(
    path: &Path,
) -> Result<Option<SessionSnapshot>, ConnectionRetirementError> {
    match crate::persist::try_load_snapshot_at(path) {
        Ok(snapshot) => Ok(snapshot),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
            Err(ConnectionRetirementError::InvalidSnapshot {
                path: path.to_path_buf(),
                detail: error.to_string(),
            })
        }
        Err(error) => Err(ConnectionRetirementError::Io(error)),
    }
}

fn session_is_running(impact: &SessionRetirementImpact) -> bool {
    let name = if impact.is_default {
        None
    } else {
        Some(impact.session_name.as_str())
    };
    crate::session::session_info(name).running
}

fn inventory_session(
    host_id: &ExecutionHostId,
    session: &SessionInfo,
    snapshot_path: PathBuf,
    snapshot: &SessionSnapshot,
) -> Option<SessionRetirementImpact> {
    let mut group_defaults = Vec::new();
    for group in &snapshot.groups {
        if let Some(location) = group.default_location.as_ref() {
            if &location.execution_host_id == host_id {
                group_defaults.push(GroupDefaultImpact {
                    group_id: group.id.clone(),
                    group_name: group.name.clone(),
                    previous: location.clone(),
                    replacement: GroupDefaultReplacement::Unset,
                });
            }
        }
    }

    let mut workspace_defaults = Vec::new();
    let mut remote_panes = Vec::new();
    for (workspace_index, workspace) in snapshot.workspaces.iter().enumerate() {
        if &workspace.default_location.execution_host_id == host_id {
            workspace_defaults.push(WorkspaceDefaultImpact {
                workspace_index,
                workspace_id: workspace.id.clone(),
                previous: workspace.default_location.clone(),
                replacement: local_retirement_replacement(),
            });
        }

        for (tab_index, tab) in workspace.tabs.iter().enumerate() {
            let mut pane_ids: Vec<u32> = tab.panes.keys().copied().collect();
            pane_ids.sort_unstable();
            for pane_id in pane_ids {
                let Some(pane) = tab.panes.get(&pane_id) else {
                    continue;
                };
                if let Some(location) = pane.location.as_ref() {
                    if &location.execution_host_id == host_id {
                        remote_panes.push(RemotePaneImpact {
                            workspace_index,
                            tab_index,
                            pane_id,
                            location: location.clone(),
                        });
                    }
                }
            }
        }
    }

    let mut pending_terminations = Vec::new();
    for tombstone in &snapshot.remote_termination_tombstones {
        if &tombstone.location.execution_host_id == host_id {
            pending_terminations.push(PendingTerminationImpact {
                terminal_id: tombstone.terminal_id.clone(),
                location: tombstone.location.clone(),
            });
        }
    }

    if group_defaults.is_empty()
        && workspace_defaults.is_empty()
        && remote_panes.is_empty()
        && pending_terminations.is_empty()
    {
        return None;
    }

    Some(SessionRetirementImpact {
        session_name: session.name.clone(),
        is_default: session.default,
        running: session.running,
        snapshot_path,
        group_defaults,
        workspace_defaults,
        remote_panes,
        pending_terminations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AgentPanelScope;
    use crate::execution_host::{ExecutionHostId, HostPath, ResourceLocation};
    use crate::persist::{
        try_load_snapshot_at, try_save_snapshot_at, GroupSnapshot, LayoutSnapshot, PaneSnapshot,
        RemoteTerminationTombstoneSnapshot, SessionDefaultViewSnapshot, SessionSnapshot,
        SessionUiSnapshot, TabSnapshot, WorkspaceSnapshot,
    };

    const TEST_SNAPSHOT_VERSION: u32 = 6;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn host(id: &str) -> ExecutionHostId {
        ExecutionHostId::new(id).expect("host id")
    }

    fn location(host_id: &ExecutionHostId, path: &str) -> ResourceLocation {
        ResourceLocation::new(host_id.clone(), HostPath::new(path).expect("path"))
    }

    fn local_location(path: &str) -> ResourceLocation {
        ResourceLocation::local(path).expect("local path")
    }

    fn empty_snapshot() -> SessionSnapshot {
        SessionSnapshot {
            version: TEST_SNAPSHOT_VERSION,
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

    fn temp_config_home(label: &str) -> PathBuf {
        let unique = format!(
            "gardn-connection-retirement-{}-{}-{}",
            label,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    fn write_session(config_dir: &Path, name: Option<&str>, snapshot: &SessionSnapshot) -> PathBuf {
        let dir = match name {
            Some(name) => config_dir.join("sessions").join(name),
            None => config_dir.to_path_buf(),
        };
        std::fs::create_dir_all(&dir).expect("session dir");
        let path = dir.join("session.json");
        try_save_snapshot_at(&path, snapshot).expect("save snapshot");
        path
    }

    fn session_info_for(config_dir: &Path, name: Option<&str>, running: bool) -> SessionInfo {
        let (display, default, dir) = match name {
            Some(name) => (
                name.to_string(),
                false,
                config_dir.join("sessions").join(name),
            ),
            None => (
                crate::session::DEFAULT_SESSION_NAME.to_string(),
                true,
                config_dir.to_path_buf(),
            ),
        };
        SessionInfo {
            name: display,
            default,
            running,
            socket_path: dir.join("gardn.sock").display().to_string(),
            session_dir: dir.display().to_string(),
        }
    }

    #[test]
    fn multi_session_inventory_is_complete_and_deterministic() {
        let retiring = host("ssh:workbox:1");
        let other = host("ssh:other:1");

        let mut default_snap = empty_snapshot();
        default_snap.groups[0].default_location = Some(location(&retiring, "/srv/group"));
        default_snap.workspaces.push(workspace_at(
            "default-ws",
            location(&retiring, "/srv/default"),
        ));

        let mut alpha = empty_snapshot();
        alpha
            .workspaces
            .push(workspace_at("alpha-local", local_location("/tmp/alpha")));
        alpha.workspaces[0].tabs[0]
            .panes
            .get_mut(&0)
            .unwrap()
            .location = Some(location(&retiring, "/srv/alpha-pane"));
        alpha.workspaces[0].tabs[0].panes.get_mut(&0).unwrap().label =
            Some("alpha-remote".to_string());

        let mut beta = empty_snapshot();
        beta.workspaces
            .push(workspace_at("beta-other", location(&other, "/srv/beta")));
        beta.remote_termination_tombstones
            .push(RemoteTerminationTombstoneSnapshot {
                terminal_id: serde_json::from_str("\"term_beta\"").unwrap(),
                location: location(&retiring, "/srv/tomb"),
                remote_runtime_identity: crate::execution_host::protocol::RuntimeIdentity::new(
                    crate::execution_host::protocol::HostBindingGeneration::new(1),
                    crate::execution_host::protocol::WorkerInstanceId::new("worker-a").unwrap(),
                    crate::execution_host::protocol::WorkerRuntimeId::new("runtime-a").unwrap(),
                    crate::execution_host::protocol::RuntimeIncarnation::new(1),
                ),
            });

        let config_home = temp_config_home("multi");
        let config_dir = config_home.join(crate::config::app_dir_name());
        write_session(&config_dir, None, &default_snap);
        write_session(&config_dir, Some("alpha"), &alpha);
        write_session(&config_dir, Some("beta"), &beta);
        // Unrelated session must not appear.
        write_session(&config_dir, Some("clean"), &empty_snapshot());

        let sessions = vec![
            session_info_for(&config_dir, None, false),
            session_info_for(&config_dir, Some("alpha"), true),
            session_info_for(&config_dir, Some("beta"), false),
            session_info_for(&config_dir, Some("clean"), false),
        ];
        let plan = plan_connection_retirement_for_sessions(&retiring, &sessions).unwrap();

        assert_eq!(
            plan.sessions
                .iter()
                .map(|session| session.session_name.as_str())
                .collect::<Vec<_>>(),
            vec!["default", "alpha", "beta"]
        );
        assert!(!plan.sessions[0].running);
        assert!(plan.sessions[1].running);
        assert_eq!(plan.sessions[0].group_defaults.len(), 1);
        assert_eq!(plan.sessions[0].workspace_defaults.len(), 1);
        assert_eq!(plan.sessions[1].remote_panes.len(), 1);
        assert_eq!(plan.sessions[1].remote_panes[0].pane_id, 0);
        assert_eq!(plan.sessions[2].pending_terminations.len(), 1);
        assert!(plan.sessions.iter().all(|session| {
            session
                .group_defaults
                .iter()
                .all(|group| matches!(group.replacement, GroupDefaultReplacement::Unset))
        }));

        let _ = std::fs::remove_dir_all(config_home);
    }

    #[test]
    fn mixed_host_filtering_only_reports_retiring_host() {
        let retiring = host("ssh:workbox:1");
        let other = host("ssh:other:2");

        let mut snap = empty_snapshot();
        snap.groups[0].default_location = Some(location(&other, "/srv/other-group"));
        snap.groups.push(GroupSnapshot {
            id: "g2".to_string(),
            name: "group 2".to_string(),
            icon: crate::app::state::DEFAULT_GROUP_ICON.to_string(),
            accent: None,
            default_location: Some(location(&retiring, "/srv/retiring-group")),
            favorite_agent_profile_ids: Vec::new(),
            default_agent_profile_id: None,
            github_organization: None,
        });
        snap.workspaces
            .push(workspace_at("keep", location(&other, "/srv/keep")));
        snap.workspaces
            .push(workspace_at("move", location(&retiring, "/srv/move")));
        snap.workspaces[0].tabs[0]
            .panes
            .get_mut(&0)
            .unwrap()
            .location = Some(location(&other, "/srv/other-pane"));
        snap.workspaces[1].tabs[0]
            .panes
            .get_mut(&0)
            .unwrap()
            .location = Some(location(&retiring, "/srv/retiring-pane"));

        let config_home = temp_config_home("mixed");
        let config_dir = config_home.join(crate::config::app_dir_name());
        let path = write_session(&config_dir, None, &snap);
        let sessions = vec![session_info_for(&config_dir, None, false)];
        let plan = plan_connection_retirement_for_sessions(&retiring, &sessions).unwrap();

        assert_eq!(plan.sessions.len(), 1);
        let impact = &plan.sessions[0];
        assert_eq!(impact.group_defaults.len(), 1);
        assert_eq!(impact.group_defaults[0].group_id, "g2");
        assert_eq!(impact.workspace_defaults.len(), 1);
        assert_eq!(
            impact.workspace_defaults[0].workspace_id.as_deref(),
            Some("move")
        );
        assert_eq!(impact.remote_panes.len(), 1);
        assert_eq!(impact.remote_panes[0].workspace_index, 1);
        assert_eq!(impact.snapshot_path, path);

        let _ = std::fs::remove_dir_all(config_home);
    }

    #[test]
    fn invalid_snapshot_fails_closed() {
        let retiring = host("ssh:workbox:1");
        let config_home = temp_config_home("invalid");
        let config_dir = config_home.join(crate::config::app_dir_name());
        std::fs::create_dir_all(&config_dir).unwrap();
        let path = config_dir.join("session.json");
        std::fs::write(&path, "{not-json").unwrap();

        let sessions = vec![session_info_for(&config_dir, None, false)];
        let err = plan_connection_retirement_for_sessions(&retiring, &sessions).unwrap_err();
        match err {
            ConnectionRetirementError::InvalidSnapshot {
                path: err_path,
                detail,
            } => {
                assert_eq!(err_path, path);
                assert!(!detail.is_empty());
            }
            other => panic!("expected invalid snapshot error, got {other}"),
        }

        let _ = std::fs::remove_dir_all(config_home);
    }

    #[test]
    fn exact_rewrites_unset_groups_and_localize_workspaces_only() {
        let retiring = host("ssh:workbox:1");
        let other = host("ssh:other:1");

        let mut snap = empty_snapshot();
        snap.groups[0].default_location = Some(location(&retiring, "/srv/group"));
        snap.groups.push(GroupSnapshot {
            id: "keep".to_string(),
            name: "keep".to_string(),
            icon: crate::app::state::DEFAULT_GROUP_ICON.to_string(),
            accent: None,
            default_location: Some(location(&other, "/srv/keep-group")),
            default_agent_profile_id: Some("agent-a".to_string()),
            favorite_agent_profile_ids: vec!["agent-a".to_string()],
            github_organization: None,
        });
        snap.workspaces.push(workspace_at(
            "remote-default",
            location(&retiring, "/srv/ws"),
        ));
        snap.workspaces.push(workspace_at(
            "other-default",
            location(&other, "/srv/other-ws"),
        ));
        snap.workspaces[0].tabs[0]
            .panes
            .get_mut(&0)
            .unwrap()
            .location = Some(location(&retiring, "/srv/pane"));
        snap.workspaces[0].tabs[0].panes.get_mut(&0).unwrap().label =
            Some("must-stay-remote".to_string());
        snap.default_view.selected = 9;
        snap.ui.workspace_scroll = 3;

        let summary = rewrite_snapshot_placements(&mut snap, &retiring);
        assert_eq!(summary.groups_unset, 1);
        assert_eq!(summary.workspaces_localized, 1);

        assert!(snap.groups[0].default_location.is_none());
        assert_eq!(
            snap.groups[1]
                .default_location
                .as_ref()
                .unwrap()
                .execution_host_id,
            other
        );
        assert_eq!(
            snap.groups[1].favorite_agent_profile_ids,
            vec!["agent-a".to_string()]
        );
        assert_eq!(
            snap.workspaces[0].default_location,
            local_retirement_replacement()
        );
        assert_eq!(snap.workspaces[1].default_location.execution_host_id, other);
        // Panes are disclosed for closure — never rewritten to Local.
        assert_eq!(
            snap.workspaces[0].tabs[0]
                .panes
                .get(&0)
                .unwrap()
                .location
                .as_ref()
                .unwrap()
                .execution_host_id,
            retiring
        );
        assert_eq!(
            snap.workspaces[0].tabs[0]
                .panes
                .get(&0)
                .unwrap()
                .label
                .as_deref(),
            Some("must-stay-remote")
        );
        assert_eq!(snap.default_view.selected, 9);
        assert_eq!(snap.ui.workspace_scroll, 3);
    }

    #[test]
    fn dormant_apply_refuses_remote_panes_and_tombstones() {
        let retiring = host("ssh:workbox:1");
        let config_home = temp_config_home("refuse");
        let config_dir = config_home.join(crate::config::app_dir_name());

        let mut with_pane = empty_snapshot();
        with_pane
            .workspaces
            .push(workspace_at("ws", location(&retiring, "/srv/ws")));
        with_pane.workspaces[0].tabs[0]
            .panes
            .get_mut(&0)
            .unwrap()
            .location = Some(location(&retiring, "/srv/pane"));
        let pane_path = write_session(&config_dir, Some("with-pane"), &with_pane);

        let mut with_tomb = empty_snapshot();
        with_tomb.groups[0].default_location = Some(location(&retiring, "/srv/group"));
        with_tomb
            .remote_termination_tombstones
            .push(RemoteTerminationTombstoneSnapshot {
                terminal_id: serde_json::from_str("\"term_tomb\"").unwrap(),
                location: location(&retiring, "/srv/tomb"),
                remote_runtime_identity: crate::execution_host::protocol::RuntimeIdentity::new(
                    crate::execution_host::protocol::HostBindingGeneration::new(1),
                    crate::execution_host::protocol::WorkerInstanceId::new("worker-a").unwrap(),
                    crate::execution_host::protocol::WorkerRuntimeId::new("runtime-a").unwrap(),
                    crate::execution_host::protocol::RuntimeIncarnation::new(1),
                ),
            });
        let tomb_path = write_session(&config_dir, Some("with-tomb"), &with_tomb);

        let pane_sessions = vec![session_info_for(&config_dir, Some("with-pane"), false)];
        let pane_plan = plan_connection_retirement_for_sessions(&retiring, &pane_sessions).unwrap();
        let pane_err =
            apply_dormant_session_retirement(&retiring, &pane_plan.sessions[0]).unwrap_err();
        assert!(matches!(
            pane_err,
            ConnectionRetirementError::RequiresLiveCoordinator {
                remote_panes: 1,
                pending_terminations: 0,
                ..
            }
        ));
        // Snapshot unchanged.
        let reloaded = try_load_snapshot_at(&pane_path).unwrap().unwrap();
        assert_eq!(
            reloaded.workspaces[0].tabs[0]
                .panes
                .get(&0)
                .unwrap()
                .location
                .as_ref()
                .unwrap()
                .execution_host_id,
            retiring
        );

        let tomb_sessions = vec![session_info_for(&config_dir, Some("with-tomb"), false)];
        let tomb_plan = plan_connection_retirement_for_sessions(&retiring, &tomb_sessions).unwrap();
        let tomb_err =
            apply_dormant_session_retirement(&retiring, &tomb_plan.sessions[0]).unwrap_err();
        assert!(matches!(
            tomb_err,
            ConnectionRetirementError::RequiresLiveCoordinator {
                remote_panes: 0,
                pending_terminations: 1,
                ..
            }
        ));
        let reloaded = try_load_snapshot_at(&tomb_path).unwrap().unwrap();
        assert!(reloaded.groups[0].default_location.is_some());
        assert_eq!(reloaded.remote_termination_tombstones.len(), 1);

        let _ = std::fs::remove_dir_all(config_home);
    }

    #[test]
    fn dormant_apply_is_idempotent_and_preserves_unrelated_data() {
        let retiring = host("ssh:workbox:1");
        let other = host("ssh:other:1");
        let config_home = temp_config_home("idempotent");
        let config_dir = config_home.join(crate::config::app_dir_name());

        let mut snap = empty_snapshot();
        snap.session_namespace_id = "ns-preserve".to_string();
        snap.default_view.selected = 4;
        snap.default_view.ui.tab_scroll = 2;
        snap.groups[0].default_location = Some(location(&retiring, "/srv/group"));
        snap.groups[0].favorite_agent_profile_ids = vec!["keep-profile".to_string()];
        snap.workspaces
            .push(workspace_at("ws", location(&retiring, "/srv/ws")));
        snap.workspaces
            .push(workspace_at("other", location(&other, "/srv/other")));
        snap.workspaces[1].custom_name = Some("unrelated-name".to_string());
        let path = write_session(&config_dir, Some("work"), &snap);

        let sessions = vec![session_info_for(&config_dir, Some("work"), false)];
        let plan = plan_connection_retirement_for_sessions(&retiring, &sessions).unwrap();
        assert_eq!(plan.sessions.len(), 1);
        assert!(!plan.sessions[0].running);
        assert!(plan.sessions[0].has_placement_rewrites());
        assert!(!plan.sessions[0].requires_live_coordinator());

        let first = apply_dormant_session_retirement(&retiring, &plan.sessions[0]).unwrap();
        assert_eq!(
            first,
            DormantApplyOutcome::Rewritten {
                session_name: "work".to_string(),
                groups_unset: 1,
                workspaces_localized: 1,
            }
        );

        let after = try_load_snapshot_at(&path).unwrap().unwrap();
        assert!(after.groups[0].default_location.is_none());
        assert_eq!(
            after.groups[0].favorite_agent_profile_ids,
            vec!["keep-profile".to_string()]
        );
        assert_eq!(
            after.workspaces[0].default_location,
            local_retirement_replacement()
        );
        assert_eq!(
            after.workspaces[1].default_location.execution_host_id,
            other
        );
        assert_eq!(
            after.workspaces[1].custom_name.as_deref(),
            Some("unrelated-name")
        );
        assert_eq!(after.session_namespace_id, "ns-preserve");
        // Durable view lives on default_view; top-level ui/selected are legacy mirrors.
        assert_eq!(after.default_view.selected, 4);
        assert_eq!(after.default_view.ui.tab_scroll, 2);

        let second = apply_dormant_session_retirement(&retiring, &plan.sessions[0]).unwrap();
        assert_eq!(
            second,
            DormantApplyOutcome::AlreadyClean {
                session_name: "work".to_string(),
            }
        );

        // Running sessions are refused even when placement-only.
        let mut running_impact = plan.sessions[0].clone();
        running_impact.running = true;
        let running_err = apply_dormant_session_retirement(&retiring, &running_impact).unwrap_err();
        assert!(matches!(
            running_err,
            ConnectionRetirementError::SessionRunning { .. }
        ));

        let _ = std::fs::remove_dir_all(config_home);
    }

    #[test]
    fn plan_level_apply_rewrites_eligible_and_blocks_runtime_bearing() {
        let retiring = host("ssh:workbox:1");
        let config_home = temp_config_home("batch");
        let config_dir = config_home.join(crate::config::app_dir_name());

        let mut clean = empty_snapshot();
        clean.groups[0].default_location = Some(location(&retiring, "/srv/group"));
        write_session(&config_dir, Some("clean"), &clean);

        let mut runtime = empty_snapshot();
        runtime
            .workspaces
            .push(workspace_at("ws", local_location("/tmp/ws")));
        runtime.workspaces[0].tabs[0]
            .panes
            .get_mut(&0)
            .unwrap()
            .location = Some(location(&retiring, "/srv/pane"));
        write_session(&config_dir, Some("runtime"), &runtime);

        let sessions = vec![
            session_info_for(&config_dir, Some("clean"), false),
            session_info_for(&config_dir, Some("runtime"), false),
        ];
        let plan = plan_connection_retirement_for_sessions(&retiring, &sessions).unwrap();
        let report = apply_dormant_connection_retirement(&plan).unwrap();

        assert_eq!(report.applied.len(), 1);
        assert_eq!(
            report.applied[0],
            DormantApplyOutcome::Rewritten {
                session_name: "clean".to_string(),
                groups_unset: 1,
                workspaces_localized: 0,
            }
        );
        assert_eq!(report.blocked.len(), 1);
        assert_eq!(report.blocked[0].session_name, "runtime");
        assert_eq!(report.blocked[0].remote_panes.len(), 1);

        let _ = std::fs::remove_dir_all(config_home);
    }

    #[test]
    fn retirement_journal_survives_failure_and_serializes_retry() {
        let _lock = crate::config::test_config_env_lock()
            .lock()
            .expect("config env lock");
        let config_home = temp_config_home("journal");
        let _config_home = crate::config::TestEnvVar::set("XDG_CONFIG_HOME", &config_home);
        let retiring = host("ssh:journal:1");
        let approved = ApprovedConnectionRetirement::LocalOnly {
            plan: ConnectionRetirementPlan {
                host_id: retiring.clone(),
                sessions: Vec::new(),
            },
        };

        let mut first = begin_connection_retirement_journal("profile-a", approved.clone())
            .expect("begin journal");
        assert!(begin_connection_retirement_journal("profile-a", approved.clone()).is_err());
        first.pause("host unavailable").expect("pause journal");
        drop(first);

        assert_eq!(
            pending_connection_retirement_host()
                .expect("load pending journal")
                .as_ref(),
            Some(&retiring)
        );
        let other = ApprovedConnectionRetirement::LocalOnly {
            plan: ConnectionRetirementPlan {
                host_id: host("ssh:other:1"),
                sessions: Vec::new(),
            },
        };
        assert!(begin_connection_retirement_journal("profile-b", other).is_err());

        let resumed = begin_connection_retirement_journal("profile-a", approved.clone())
            .expect("resume journal");
        let completion = crate::events::AppEvent::ConnectionRetired {
            authentication_owner: crate::execution_host::auth::AuthenticationOwner::SYSTEM,
            profile_id: "profile-a".to_string(),
            result: Ok("retired".to_string()),
            journal: Some(resumed),
        };
        assert!(begin_connection_retirement_journal("profile-a", approved).is_err());
        drop(completion);
        complete_connection_retirement_journal("profile-a").expect("complete journal");
        assert_eq!(
            pending_connection_retirement_host().expect("journal removed"),
            None
        );

        let _ = std::fs::remove_dir_all(config_home);
    }
}
