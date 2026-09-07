use std::collections::HashMap;
use std::path::PathBuf;

use ratatui::layout::Direction;
use serde::{Deserialize, Serialize};

use crate::execution_host::protocol::SessionNamespaceId;
use crate::layout::Node;
use crate::terminal::TerminalRuntimeRegistry;
use crate::workspace::Workspace;

/// Current snapshot format version.
pub(crate) const SNAPSHOT_VERSION: u32 = 6;

/// Serializable snapshot of the entire Gardn session.
// Legacy mirror fields stay on the in-memory struct so old snapshots migrate
// through one parser shape; new snapshots serialize `default_view` instead.
#[allow(dead_code)]
#[derive(Serialize, Deserialize)]
pub struct SessionSnapshot {
    /// Format version — used to detect incompatible changes.
    #[serde(default)]
    pub version: u32,
    /// Durable scope used to adopt worker-owned runtimes after coordinator restart.
    #[serde(default)]
    pub session_namespace_id: String,
    /// Remote runtimes removed from the live layout but still awaiting termination acknowledgement.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remote_termination_tombstones: Vec<RemoteTerminationTombstoneSnapshot>,
    #[serde(default = "default_groups")]
    pub groups: Vec<GroupSnapshot>,
    #[serde(default)]
    pub active_group: usize,
    #[serde(default = "default_true")]
    pub group_filter_enabled: bool,
    #[serde(default)]
    pub default_view: SessionDefaultViewSnapshot,
    pub workspaces: Vec<WorkspaceSnapshot>,
    #[serde(default, skip_serializing)]
    pub active: Option<usize>,
    #[serde(default, skip_serializing)]
    pub selected: usize,
    #[serde(default, skip_serializing)]
    pub agent_panel_scope: crate::app::state::AgentPanelScope,
    #[serde(default, skip_serializing)]
    pub sidebar_width: Option<u16>,
    #[serde(default, skip_serializing)]
    pub sidebar_collapsed: bool,
    #[serde(default, skip_serializing)]
    pub sidebar_section_split: Option<f32>,
    #[serde(default, skip_serializing)]
    pub right_sidebar_width: Option<u16>,
    #[serde(default, skip_serializing)]
    pub right_sidebar_collapsed: bool,
    #[serde(default, skip_serializing)]
    pub ui: SessionUiSnapshot,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub agent_follow_up: Vec<crate::app::state::AgentFollowUpEntry>,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub pane_id_aliases: std::collections::HashMap<u32, u32>,
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct SessionDefaultViewSnapshot {
    #[serde(default)]
    pub active: Option<usize>,
    #[serde(default)]
    pub selected: usize,
    #[serde(default)]
    pub agent_panel_scope: crate::app::state::AgentPanelScope,
    #[serde(default)]
    pub sidebar_width: Option<u16>,
    #[serde(default)]
    pub sidebar_collapsed: bool,
    #[serde(default)]
    pub sidebar_section_split: Option<f32>,
    #[serde(default)]
    pub right_sidebar_width: Option<u16>,
    #[serde(default)]
    pub right_sidebar_collapsed: bool,
    #[serde(default)]
    pub ui: SessionUiSnapshot,
}

impl SessionDefaultViewSnapshot {
    fn from_legacy(raw: &RawSessionSnapshot) -> Self {
        Self {
            active: raw.active,
            selected: raw.selected,
            agent_panel_scope: raw.agent_panel_scope,
            sidebar_width: raw.sidebar_width,
            sidebar_collapsed: raw.sidebar_collapsed,
            sidebar_section_split: raw.sidebar_section_split,
            right_sidebar_width: raw.right_sidebar_width,
            right_sidebar_collapsed: raw.right_sidebar_collapsed,
            ui: raw.ui.clone(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct SessionUiSnapshot {
    #[serde(default)]
    pub workspace_scroll: usize,
    #[serde(default)]
    pub agent_panel_scroll: usize,
    #[serde(default)]
    pub tab_scroll: usize,
    #[serde(default)]
    pub mobile_switcher_scroll: usize,
    #[serde(default = "default_true")]
    pub activity_agents_expanded: bool,
    #[serde(default)]
    pub activity_commands_expanded: bool,
    #[serde(default)]
    pub activity_ports_expanded: bool,
    #[serde(default)]
    pub collapsed_agent_sections: Vec<String>,
    #[serde(default)]
    pub collapsed_command_groups: Vec<String>,
    #[serde(default)]
    pub collapsed_command_status_groups: Vec<String>,
    #[serde(default)]
    pub collapsed_workspace_groups: Vec<String>,
}

impl Default for SessionUiSnapshot {
    fn default() -> Self {
        Self {
            workspace_scroll: 0,
            agent_panel_scroll: 0,
            tab_scroll: 0,
            mobile_switcher_scroll: 0,
            activity_agents_expanded: true,
            activity_commands_expanded: false,
            activity_ports_expanded: false,
            collapsed_agent_sections: Vec::new(),
            collapsed_command_groups: Vec::new(),
            collapsed_command_status_groups: Vec::new(),
            collapsed_workspace_groups: Vec::new(),
        }
    }
}

impl SessionUiSnapshot {
    pub fn from_app_state(state: &crate::app::state::AppState) -> Self {
        Self {
            workspace_scroll: state.workspace_scroll,
            agent_panel_scroll: state.agent_panel_scroll,
            tab_scroll: state.tab_scroll,
            mobile_switcher_scroll: state.mobile_switcher_scroll,
            activity_agents_expanded: state.activity_agents_expanded,
            activity_commands_expanded: state.activity_commands_expanded,
            activity_ports_expanded: state.activity_ports_expanded,
            collapsed_agent_sections: state.collapsed_agent_sections.clone(),
            collapsed_command_groups: state.collapsed_command_groups.clone(),
            collapsed_command_status_groups: state.collapsed_command_status_groups.clone(),
            collapsed_workspace_groups: state.collapsed_workspace_groups.clone(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct SessionHistorySnapshot {
    /// Format version follows the matching session snapshot version.
    #[serde(default)]
    pub version: u32,
    pub workspaces: Vec<WorkspaceHistorySnapshot>,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspaceHistorySnapshot {
    pub tabs: Vec<TabHistorySnapshot>,
}

#[derive(Serialize, Deserialize)]
pub struct TabHistorySnapshot {
    pub panes: HashMap<u32, PaneHistorySnapshot>,
}

#[derive(Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub custom_name: Option<String>,
    #[serde(default = "default_group_id")]
    pub group_id: String,
    pub identity_cwd: PathBuf,
    pub default_location: crate::execution_host::ResourceLocation,
    #[serde(default)]
    pub github_scope: crate::github::GithubRepositoryScope,
    #[serde(default)]
    pub public_pane_numbers: HashMap<u32, usize>,
    #[serde(default)]
    pub next_public_pane_number: usize,
    #[serde(default)]
    pub public_tab_numbers: Vec<usize>,
    #[serde(default)]
    pub next_public_tab_number: usize,
    pub tabs: Vec<TabSnapshot>,
    #[serde(default)]
    pub active_tab: usize,
}

#[derive(Serialize, Clone)]
pub struct GroupSnapshot {
    pub id: String,
    pub name: String,
    pub icon: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent: Option<crate::config::TerminalAccent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_location: Option<crate::execution_host::ResourceLocation>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub favorite_agent_profile_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_agent_profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_organization: Option<crate::app::state::GithubOrganization>,
}

#[derive(Deserialize)]
struct RawGroupSnapshot {
    id: String,
    name: String,
    #[serde(default = "default_group_icon")]
    icon: String,
    #[serde(default)]
    accent: Option<crate::config::TerminalAccent>,
    #[serde(default)]
    default_location: Option<crate::execution_host::ResourceLocation>,
    #[serde(default)]
    default_directory: Option<PathBuf>,
    #[serde(default)]
    favorite_agent_profile_ids: Vec<String>,
    #[serde(default)]
    default_agent_profile_id: Option<String>,
    #[serde(default)]
    github_organization: Option<String>,
}

impl<'de> Deserialize<'de> for GroupSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawGroupSnapshot::deserialize(deserializer)?;
        let default_location = match (raw.default_location, raw.default_directory) {
            (Some(location), _) => Some(location),
            (None, Some(path)) => Some(
                crate::execution_host::ResourceLocation::local(path)
                    .map_err(serde::de::Error::custom)?,
            ),
            (None, None) => None,
        };
        let github_organization = raw
            .github_organization
            .as_deref()
            .map(crate::app::state::GithubOrganization::parse)
            .transpose()
            .map_err(serde::de::Error::custom)?
            .flatten();
        Ok(Self {
            id: raw.id,
            name: raw.name,
            icon: raw.icon,
            accent: raw.accent,
            default_location,
            favorite_agent_profile_ids: raw.favorite_agent_profile_ids,
            default_agent_profile_id: raw.default_agent_profile_id,
            github_organization,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteTerminationTombstoneSnapshot {
    pub terminal_id: crate::terminal::TerminalId,
    pub location: crate::execution_host::ResourceLocation,
    pub remote_runtime_identity: crate::execution_host::protocol::RuntimeIdentity,
}

fn default_group_id() -> String {
    crate::workspace::DEFAULT_GROUP_ID.to_string()
}

fn default_group_icon() -> String {
    crate::app::state::DEFAULT_GROUP_ICON.to_string()
}

fn default_groups() -> Vec<GroupSnapshot> {
    vec![GroupSnapshot {
        id: crate::workspace::DEFAULT_GROUP_ID.to_string(),
        name: "group 1".to_string(),
        icon: default_group_icon(),
        accent: None,
        default_location: None,
        favorite_agent_profile_ids: Vec::new(),
        default_agent_profile_id: None,
        github_organization: None,
    }]
}

#[derive(Deserialize)]
struct LegacyWorkspaceSnapshot {
    #[serde(default)]
    custom_name: Option<String>,
    layout: LayoutSnapshot,
    panes: HashMap<u32, PaneSnapshot>,
    zoomed: bool,
    #[serde(default)]
    focused: Option<u32>,
    #[serde(default)]
    root_pane: Option<u32>,
}

#[derive(Serialize, Deserialize)]
pub struct TabSnapshot {
    #[serde(default)]
    pub custom_name: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "crate::workspace::TabRole::is_terminal"
    )]
    pub role: crate::workspace::TabRole,
    pub layout: LayoutSnapshot,
    pub panes: HashMap<u32, PaneSnapshot>,
    pub zoomed: bool,
    #[serde(default)]
    pub focused: Option<u32>,
    #[serde(default)]
    pub root_pane: Option<u32>,
}

#[derive(Serialize, Deserialize)]
pub struct PaneSnapshot {
    pub cwd: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<crate::execution_host::ResourceLocation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_runtime_identity: Option<crate::execution_host::protocol::RuntimeIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_pane_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<PaneAgentSessionSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch_argv: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub launch_env: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_theme_binding: Option<crate::terminal_theme::TerminalThemeBinding>,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub seen: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub right_click_passthrough: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_semantics: Option<crate::terminal::TerminalSemanticSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneAgentSessionSnapshot {
    pub source: String,
    pub agent: String,
    pub kind: crate::agent_resume::AgentSessionRefKind,
    pub value: String,
}

fn default_true() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Serialize, Deserialize)]
pub struct PaneHistorySnapshot {
    pub ansi: String,
    pub lines: usize,
}

/// Serializable BSP tree.
#[derive(Serialize, Deserialize)]
pub enum LayoutSnapshot {
    Pane(u32),
    Split {
        direction: DirectionSnapshot,
        ratio: f32,
        first: Box<LayoutSnapshot>,
        second: Box<LayoutSnapshot>,
    },
}

#[derive(Serialize, Deserialize)]
pub enum DirectionSnapshot {
    Horizontal,
    Vertical,
}

impl From<LegacyWorkspaceSnapshot> for WorkspaceSnapshot {
    fn from(snap: LegacyWorkspaceSnapshot) -> Self {
        let identity_cwd = legacy_identity_cwd(&snap);
        let tab = TabSnapshot {
            custom_name: None,
            role: crate::workspace::TabRole::Terminal,
            layout: snap.layout,
            panes: snap.panes,
            zoomed: snap.zoomed,
            focused: snap.focused,
            root_pane: snap.root_pane,
        };

        Self {
            id: None,
            custom_name: snap.custom_name,
            group_id: default_group_id(),
            identity_cwd: identity_cwd.clone(),
            default_location: crate::execution_host::ResourceLocation::new(
                crate::execution_host::ExecutionHostId::local(),
                crate::execution_host::HostPath::new(identity_cwd).unwrap_or_default(),
            ),
            github_scope: crate::github::GithubRepositoryScope::default(),
            public_pane_numbers: HashMap::new(),
            next_public_pane_number: 0,
            public_tab_numbers: Vec::new(),
            next_public_tab_number: 0,
            tabs: vec![tab],
            active_tab: 0,
        }
    }
}

#[derive(Deserialize)]
struct RawSessionSnapshot {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    session_namespace_id: String,
    #[serde(default)]
    remote_termination_tombstones: Vec<RemoteTerminationTombstoneSnapshot>,
    #[serde(default = "default_groups")]
    groups: Vec<GroupSnapshot>,
    #[serde(default)]
    active_group: usize,
    #[serde(default = "default_true")]
    group_filter_enabled: bool,
    #[serde(default)]
    default_view: Option<SessionDefaultViewSnapshot>,
    #[serde(default)]
    workspaces: Vec<serde_json::Value>,
    #[serde(default)]
    active: Option<usize>,
    #[serde(default)]
    selected: usize,
    #[serde(default)]
    agent_panel_scope: crate::app::state::AgentPanelScope,
    #[serde(default)]
    sidebar_width: Option<u16>,
    #[serde(default)]
    sidebar_collapsed: bool,
    #[serde(default)]
    sidebar_section_split: Option<f32>,
    #[serde(default)]
    right_sidebar_width: Option<u16>,
    #[serde(default)]
    right_sidebar_collapsed: bool,
    #[serde(default)]
    ui: SessionUiSnapshot,
    #[serde(default)]
    agent_follow_up: Vec<crate::app::state::AgentFollowUpEntry>,
    #[serde(default)]
    pane_id_aliases: std::collections::HashMap<u32, u32>,
}

fn migrate_snapshot(raw: RawSessionSnapshot) -> Result<SessionSnapshot, String> {
    let default_view = raw
        .default_view
        .clone()
        .unwrap_or_else(|| SessionDefaultViewSnapshot::from_legacy(&raw));
    Ok(SessionSnapshot {
        version: raw.version,
        session_namespace_id: raw.session_namespace_id,
        remote_termination_tombstones: raw.remote_termination_tombstones,
        groups: if raw.groups.is_empty() {
            default_groups()
        } else {
            raw.groups
        },
        active_group: raw.active_group,
        group_filter_enabled: raw.group_filter_enabled,
        workspaces: raw
            .workspaces
            .into_iter()
            .map(migrate_workspace)
            .collect::<Result<Vec<_>, _>>()?,
        active: default_view.active,
        selected: default_view.selected,
        agent_panel_scope: default_view.agent_panel_scope,
        sidebar_width: default_view.sidebar_width,
        sidebar_collapsed: default_view.sidebar_collapsed,
        sidebar_section_split: default_view.sidebar_section_split,
        right_sidebar_width: default_view.right_sidebar_width,
        right_sidebar_collapsed: default_view.right_sidebar_collapsed,
        ui: default_view.ui.clone(),
        default_view,
        agent_follow_up: raw.agent_follow_up,
        pane_id_aliases: raw.pane_id_aliases,
    })
}

fn migrate_workspace(mut raw: serde_json::Value) -> Result<WorkspaceSnapshot, String> {
    migrate_pane_locations(&mut raw)?;
    if raw.get("identity_cwd").is_some() {
        let Some(object) = raw.as_object_mut() else {
            return Err("workspace snapshot must be an object".to_string());
        };
        if object.get("default_location").is_none() {
            let path = object
                .remove("default_cwd")
                .or_else(|| object.get("identity_cwd").cloned())
                .ok_or_else(|| "legacy workspace default path is missing".to_string())?;
            object.insert("default_location".to_string(), local_location_value(path)?);
        } else {
            object.remove("default_cwd");
        }
        return serde_json::from_value(raw).map_err(|error| error.to_string());
    }

    if raw.get("layout").is_some() {
        let legacy =
            serde_json::from_value::<LegacyWorkspaceSnapshot>(raw).map_err(|e| e.to_string())?;
        return Ok(legacy.into());
    }

    Err("workspace snapshot is neither current nor legacy format".to_string())
}

fn migrate_pane_locations(raw: &mut serde_json::Value) -> Result<(), String> {
    if let Some(panes) = raw.get_mut("panes") {
        migrate_pane_map(panes)?;
    }
    if let Some(tabs) = raw
        .get_mut("tabs")
        .and_then(serde_json::Value::as_array_mut)
    {
        for tab in tabs {
            if let Some(panes) = tab.get_mut("panes") {
                migrate_pane_map(panes)?;
            }
        }
    }
    Ok(())
}

fn migrate_pane_map(panes: &mut serde_json::Value) -> Result<(), String> {
    let Some(panes) = panes.as_object_mut() else {
        return Err("workspace panes must be an object".to_string());
    };
    for pane in panes.values_mut() {
        let Some(pane) = pane.as_object_mut() else {
            return Err("workspace pane must be an object".to_string());
        };
        if pane.get("location").is_none() {
            let cwd = pane
                .get("cwd")
                .cloned()
                .ok_or_else(|| "legacy pane cwd is missing".to_string())?;
            pane.insert("location".to_string(), local_location_value(cwd)?);
        }
    }
    Ok(())
}

fn local_location_value(path: serde_json::Value) -> Result<serde_json::Value, String> {
    let path = serde_json::from_value::<PathBuf>(path).map_err(|error| error.to_string())?;
    let location =
        crate::execution_host::ResourceLocation::local(path).map_err(|error| error.to_string())?;
    serde_json::to_value(location).map_err(|error| error.to_string())
}

fn legacy_identity_cwd(snap: &LegacyWorkspaceSnapshot) -> PathBuf {
    let root_pane = snap
        .root_pane
        .or_else(|| first_pane_id_in_layout(&snap.layout));

    root_pane
        .and_then(|pane_id| snap.panes.get(&pane_id))
        .map(|pane| pane.cwd.clone())
        .or_else(|| {
            first_pane_id_in_layout(&snap.layout)
                .and_then(|pane_id| snap.panes.get(&pane_id))
                .map(|pane| pane.cwd.clone())
        })
        .or_else(|| {
            snap.panes
                .keys()
                .min()
                .and_then(|pane_id| snap.panes.get(pane_id))
                .map(|pane| pane.cwd.clone())
        })
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| "/".into()))
}

fn first_pane_id_in_layout(layout: &LayoutSnapshot) -> Option<u32> {
    match layout {
        LayoutSnapshot::Pane(id) => Some(*id),
        LayoutSnapshot::Split { first, second, .. } => {
            first_pane_id_in_layout(first).or_else(|| first_pane_id_in_layout(second))
        }
    }
}

/// Capture the current app state into a serializable snapshot.
#[allow(clippy::too_many_arguments)]
pub fn capture(
    groups: &[crate::app::state::Group],
    active_group: usize,
    group_filter_enabled: bool,
    session_namespace_id: &SessionNamespaceId,
    remote_termination_tombstones: &[crate::app::state::RemoteTerminationTombstone],
    workspaces: &[Workspace],
    terminals: &std::collections::HashMap<
        crate::terminal::TerminalId,
        crate::terminal::TerminalState,
    >,
    terminal_runtimes: &TerminalRuntimeRegistry,
    active: Option<usize>,
    selected: usize,
    agent_panel_scope: crate::app::state::AgentPanelScope,
    sidebar_width: u16,
    sidebar_collapsed: bool,
    sidebar_section_split: f32,
    right_sidebar_width: u16,
    right_sidebar_collapsed: bool,
    agent_follow_up: &[crate::app::state::AgentFollowUpEntry],
) -> SessionSnapshot {
    capture_inner(
        groups,
        active_group,
        group_filter_enabled,
        session_namespace_id,
        remote_termination_tombstones,
        workspaces,
        terminals,
        terminal_runtimes,
        active,
        selected,
        agent_panel_scope,
        sidebar_width,
        sidebar_collapsed,
        sidebar_section_split,
        right_sidebar_width,
        right_sidebar_collapsed,
        agent_follow_up,
        false,
    )
}

/// Capture a handoff snapshot, including live terminal semantics that should
/// survive a server replacement but should not be treated as durable session
/// state after a cold restart.
#[allow(clippy::too_many_arguments)]
pub fn capture_handoff(
    groups: &[crate::app::state::Group],
    active_group: usize,
    group_filter_enabled: bool,
    session_namespace_id: &SessionNamespaceId,
    remote_termination_tombstones: &[crate::app::state::RemoteTerminationTombstone],
    workspaces: &[Workspace],
    terminals: &std::collections::HashMap<
        crate::terminal::TerminalId,
        crate::terminal::TerminalState,
    >,
    terminal_runtimes: &TerminalRuntimeRegistry,
    active: Option<usize>,
    selected: usize,
    agent_panel_scope: crate::app::state::AgentPanelScope,
    sidebar_width: u16,
    sidebar_collapsed: bool,
    sidebar_section_split: f32,
    right_sidebar_width: u16,
    right_sidebar_collapsed: bool,
    agent_follow_up: &[crate::app::state::AgentFollowUpEntry],
) -> SessionSnapshot {
    capture_inner(
        groups,
        active_group,
        group_filter_enabled,
        session_namespace_id,
        remote_termination_tombstones,
        workspaces,
        terminals,
        terminal_runtimes,
        active,
        selected,
        agent_panel_scope,
        sidebar_width,
        sidebar_collapsed,
        sidebar_section_split,
        right_sidebar_width,
        right_sidebar_collapsed,
        agent_follow_up,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn capture_inner(
    groups: &[crate::app::state::Group],
    active_group: usize,
    group_filter_enabled: bool,
    session_namespace_id: &SessionNamespaceId,
    remote_termination_tombstones: &[crate::app::state::RemoteTerminationTombstone],
    workspaces: &[Workspace],
    terminals: &std::collections::HashMap<
        crate::terminal::TerminalId,
        crate::terminal::TerminalState,
    >,
    terminal_runtimes: &TerminalRuntimeRegistry,
    active: Option<usize>,
    selected: usize,
    agent_panel_scope: crate::app::state::AgentPanelScope,
    sidebar_width: u16,
    sidebar_collapsed: bool,
    sidebar_section_split: f32,
    right_sidebar_width: u16,
    right_sidebar_collapsed: bool,
    agent_follow_up: &[crate::app::state::AgentFollowUpEntry],
    include_terminal_semantics: bool,
) -> SessionSnapshot {
    let default_view = SessionDefaultViewSnapshot {
        active,
        selected,
        agent_panel_scope,
        sidebar_width: Some(sidebar_width),
        sidebar_collapsed,
        sidebar_section_split: Some(sidebar_section_split),
        right_sidebar_width: Some(right_sidebar_width),
        right_sidebar_collapsed,
        ui: SessionUiSnapshot::default(),
    };

    SessionSnapshot {
        version: SNAPSHOT_VERSION,
        session_namespace_id: session_namespace_id.as_str().to_string(),
        remote_termination_tombstones: remote_termination_tombstones
            .iter()
            .map(|tombstone| RemoteTerminationTombstoneSnapshot {
                terminal_id: tombstone.terminal_id.clone(),
                location: tombstone.location.clone(),
                remote_runtime_identity: tombstone.remote_runtime_identity.clone(),
            })
            .collect(),
        groups: groups.iter().map(capture_group).collect(),
        active_group,
        group_filter_enabled,
        default_view: default_view.clone(),
        workspaces: workspaces
            .iter()
            .map(|workspace| {
                capture_workspace(
                    workspace,
                    terminals,
                    terminal_runtimes,
                    include_terminal_semantics,
                )
            })
            .collect(),
        active: default_view.active,
        selected: default_view.selected,
        ui: default_view.ui.clone(),
        pane_id_aliases: std::collections::HashMap::new(),
        agent_panel_scope: default_view.agent_panel_scope,
        sidebar_width: default_view.sidebar_width,
        sidebar_collapsed: default_view.sidebar_collapsed,
        sidebar_section_split: default_view.sidebar_section_split,
        right_sidebar_width: default_view.right_sidebar_width,
        right_sidebar_collapsed: default_view.right_sidebar_collapsed,
        agent_follow_up: crate::app::state::AppState::restored_agent_follow_up(
            workspaces,
            agent_follow_up.to_vec(),
        ),
    }
}

fn capture_group(group: &crate::app::state::Group) -> GroupSnapshot {
    GroupSnapshot {
        id: group.id.clone(),
        name: group.name.clone(),
        icon: group.icon.clone(),
        accent: group.accent,
        default_location: group.default_location.clone(),
        favorite_agent_profile_ids: group.favorite_agent_profile_ids.clone(),
        default_agent_profile_id: group.default_agent_profile_id.clone(),
        github_organization: group.github_organization.clone(),
    }
}

fn capture_workspace(
    ws: &Workspace,
    terminals: &std::collections::HashMap<
        crate::terminal::TerminalId,
        crate::terminal::TerminalState,
    >,
    terminal_runtimes: &TerminalRuntimeRegistry,
    include_terminal_semantics: bool,
) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        id: Some(ws.id.clone()),
        custom_name: ws.custom_name.clone(),
        group_id: ws.group_id.clone(),
        identity_cwd: ws.identity_cwd.clone(),
        default_location: ws.default_location.clone(),
        github_scope: ws.github_scope.clone(),
        public_pane_numbers: ws
            .public_pane_numbers
            .iter()
            .map(|(pane_id, number)| (pane_id.raw(), *number))
            .collect(),
        next_public_pane_number: ws.next_public_pane_number,
        public_tab_numbers: ws.tabs.iter().map(|tab| tab.number).collect(),
        next_public_tab_number: ws.next_public_tab_number,
        tabs: ws
            .tabs
            .iter()
            .map(|tab| {
                capture_tab(
                    tab,
                    terminals,
                    terminal_runtimes,
                    include_terminal_semantics,
                )
            })
            .collect(),
        active_tab: ws.active_tab,
    }
}

fn capture_tab(
    tab: &crate::workspace::Tab,
    terminals: &std::collections::HashMap<
        crate::terminal::TerminalId,
        crate::terminal::TerminalState,
    >,
    terminal_runtimes: &TerminalRuntimeRegistry,
    include_terminal_semantics: bool,
) -> TabSnapshot {
    let mut panes = HashMap::new();
    for id in tab.panes.keys() {
        let cwd = tab
            .cwd_for_pane(*id, terminals, terminal_runtimes)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| "/".into()));
        let pane = tab.panes.get(id);
        let terminal = pane.and_then(|pane| terminals.get(&pane.attached_terminal_id));
        let label = terminal.and_then(|terminal| terminal.manual_label.clone());
        let agent_name = terminal.and_then(|terminal| terminal.agent_name.clone());
        let launch_argv = terminal.and_then(|terminal| terminal.launch_argv.clone());
        let launch_env = terminal
            .map(|terminal| terminal.launch_env.clone())
            .unwrap_or_default();
        let terminal_theme_binding = terminal.and_then(|terminal| terminal.terminal_theme_binding);
        let agent_session = terminal.and_then(|terminal| {
            if let Some(authority) = terminal.hook_authority.as_ref() {
                if let Some(session_ref) = authority.session_ref.as_ref() {
                    return Some(PaneAgentSessionSnapshot {
                        source: authority.source.clone(),
                        agent: authority.agent_label.clone(),
                        kind: session_ref.kind,
                        value: session_ref.value.clone(),
                    });
                }
            }
            terminal
                .persisted_agent_session
                .as_ref()
                .map(|session| PaneAgentSessionSnapshot {
                    source: session.source.clone(),
                    agent: session.agent.clone(),
                    kind: session.session_ref.kind,
                    value: session.session_ref.value.clone(),
                })
        });
        let seen = pane.is_none_or(|pane| pane.seen);
        let terminal_semantics = include_terminal_semantics
            .then(|| terminal.and_then(|terminal| terminal.capture_semantic_snapshot()))
            .flatten();
        panes.insert(
            id.raw(),
            PaneSnapshot {
                env_pane_id: pane
                    .and_then(|pane| pane.env_pane_id_raw)
                    .filter(|env_pane_id| *env_pane_id != id.raw()),
                cwd: cwd.clone(),
                location: Some(
                    terminal
                        .map(|terminal| terminal.location.clone())
                        .unwrap_or_else(|| {
                            crate::execution_host::ResourceLocation::new(
                                crate::execution_host::ExecutionHostId::local(),
                                crate::execution_host::HostPath::new(cwd.clone())
                                    .unwrap_or_default(),
                            )
                        }),
                ),
                remote_runtime_identity: terminal
                    .and_then(|terminal| terminal.remote_runtime_identity.clone()),
                label,
                agent_name,
                agent_session,
                launch_argv,
                launch_env,
                terminal_theme_binding,
                seen,
                right_click_passthrough: pane.is_some_and(|pane| pane.right_click_passthrough),
                terminal_semantics,
            },
        );
    }
    TabSnapshot {
        custom_name: tab.custom_name.clone(),
        role: tab.role,
        layout: capture_node(tab.layout.root()),
        panes,
        zoomed: tab.zoomed,
        focused: Some(tab.layout.focused().raw()),
        root_pane: Some(tab.root_pane.raw()),
    }
}

/// Capture pane screen history separately from the structural session snapshot.
pub fn capture_history(
    workspaces: &[Workspace],
    terminal_runtimes: &TerminalRuntimeRegistry,
) -> SessionHistorySnapshot {
    SessionHistorySnapshot {
        version: SNAPSHOT_VERSION,
        workspaces: workspaces
            .iter()
            .map(|workspace| WorkspaceHistorySnapshot {
                tabs: workspace
                    .tabs
                    .iter()
                    .map(|tab| TabHistorySnapshot {
                        panes: capture_tab_history(tab, terminal_runtimes),
                    })
                    .collect(),
            })
            .collect(),
    }
}

fn capture_tab_history(
    tab: &crate::workspace::Tab,
    terminal_runtimes: &TerminalRuntimeRegistry,
) -> HashMap<u32, PaneHistorySnapshot> {
    let mut panes = HashMap::new();
    for (id, pane) in &tab.panes {
        if let Some(history) = capture_pane_history(Some(pane), terminal_runtimes) {
            panes.insert(id.raw(), history);
        }
    }
    panes
}

fn capture_pane_history(
    pane: Option<&crate::pane::PaneState>,
    terminal_runtimes: &TerminalRuntimeRegistry,
) -> Option<PaneHistorySnapshot> {
    let ansi = terminal_runtimes
        .get(&pane?.attached_terminal_id)?
        .snapshot_history()?;
    let lines = ansi.lines().count();
    Some(PaneHistorySnapshot { ansi, lines })
}

pub(super) fn capture_node(node: &Node) -> LayoutSnapshot {
    match node {
        Node::Pane(id) => LayoutSnapshot::Pane(id.raw()),
        Node::Split {
            direction,
            ratio,
            first,
            second,
        } => LayoutSnapshot::Split {
            direction: match direction {
                Direction::Horizontal => DirectionSnapshot::Horizontal,
                Direction::Vertical => DirectionSnapshot::Vertical,
            },
            ratio: *ratio,
            first: Box::new(capture_node(first)),
            second: Box::new(capture_node(second)),
        },
    }
}

pub(super) fn parse_snapshot(content: &str) -> Result<SessionSnapshot, String> {
    let raw = serde_json::from_str::<RawSessionSnapshot>(content).map_err(|e| e.to_string())?;
    if raw.version > SNAPSHOT_VERSION {
        return Err(format!(
            "snapshot version {} is newer than supported {}",
            raw.version, SNAPSHOT_VERSION
        ));
    }
    migrate_snapshot(raw)
}

pub(super) fn parse_history_snapshot(content: &str) -> Result<SessionHistorySnapshot, String> {
    let snapshot =
        serde_json::from_str::<SessionHistorySnapshot>(content).map_err(|e| e.to_string())?;
    if snapshot.version > SNAPSHOT_VERSION {
        return Err(format!(
            "history snapshot version {} is newer than supported {}",
            snapshot.version, SNAPSHOT_VERSION
        ));
    }
    Ok(snapshot)
}

pub(super) fn snapshot_file_version(content: &str) -> Option<u32> {
    serde_json::from_str::<RawSessionSnapshot>(content)
        .ok()
        .map(|raw| raw.version)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;

    use ratatui::layout::{Direction, Rect};

    use super::*;
    use crate::app::{state::AgentPanelScope, AppState, Mode};
    use crate::layout::NavDirection;
    use crate::workspace::Workspace;

    fn session_fixture(name: &str) -> &'static str {
        match name {
            "current-gardn" => {
                include_str!("../../tests/fixtures/session/current-gardn-session.json")
            }
            "current-gardn-dev" => {
                include_str!("../../tests/fixtures/session/current-gardn-dev-session.json")
            }
            "legacy-pre-tabs-v2" => {
                include_str!("../../tests/fixtures/session/legacy-pre-tabs-v2.json")
            }
            other => panic!("unknown session fixture: {other}"),
        }
    }

    fn state_with_workspaces(names: &[&str]) -> AppState {
        let mut state = AppState::test_new();
        state.workspaces = names.iter().map(|name| Workspace::test_new(name)).collect();
        state.ensure_test_terminals();
        if !state.workspaces.is_empty() {
            state.active = Some(0);
            state.selected = 0;
            state.mode = Mode::Terminal;
        }
        state
    }

    fn capture_from_state(state: &AppState) -> SessionSnapshot {
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        capture_from_state_with_runtimes(state, &terminal_runtimes)
    }

    fn capture_from_state_with_runtimes(
        state: &AppState,
        terminal_runtimes: &TerminalRuntimeRegistry,
    ) -> SessionSnapshot {
        capture(
            &state.groups,
            state.active_group,
            state.group_filter_enabled,
            &state.session_namespace_id,
            &state.remote_termination_tombstones,
            &state.workspaces,
            &state.terminals,
            terminal_runtimes,
            state.active,
            state.selected,
            state.agent_panel_scope,
            state.sidebar_width,
            state.sidebar_collapsed,
            state.sidebar_section_split,
            state.right_sidebar_width,
            state.right_sidebar_collapsed,
            &state.agent_follow_up,
        )
    }

    fn capture_history_from_state_with_runtimes(
        state: &AppState,
        terminal_runtimes: &TerminalRuntimeRegistry,
    ) -> SessionHistorySnapshot {
        capture_history(&state.workspaces, terminal_runtimes)
    }

    #[test]
    fn capture_keeps_space_identity_separate_from_runtime_cwd() {
        let mut state = state_with_workspaces(&["space"]);
        state.workspaces[0].custom_name = None;
        state.workspaces[0].identity_cwd = PathBuf::from("/gardn-test/space");
        state.workspaces[0].default_location =
            crate::execution_host::ResourceLocation::local("/gardn-test/default").unwrap();
        let root_pane = state.workspaces[0].tabs[0].root_pane;
        let terminal_id = state.workspaces[0].terminal_id(root_pane).unwrap().clone();
        state.terminals.get_mut(&terminal_id).unwrap().cwd = PathBuf::from("/gardn-test/runtime");
        state.workspaces[0].tabs[0]
            .panes
            .get_mut(&root_pane)
            .unwrap()
            .env_pane_id_raw = Some(6);

        let snap = capture_from_state(&state);

        assert_eq!(
            snap.workspaces[0].identity_cwd,
            PathBuf::from("/gardn-test/space")
        );
        assert_eq!(
            snap.workspaces[0].default_location,
            crate::execution_host::ResourceLocation::local("/gardn-test/default").unwrap()
        );
        assert_eq!(
            snap.workspaces[0].tabs[0].panes[&root_pane.raw()].cwd,
            PathBuf::from("/gardn-test/runtime")
        );
        assert_eq!(
            snap.workspaces[0].tabs[0].panes[&root_pane.raw()].env_pane_id,
            Some(6)
        );
    }

    #[test]
    fn snapshots_without_terminal_theme_binding_restore_as_unmanaged() {
        let snapshot =
            parse_snapshot(session_fixture("current-gardn")).expect("parse pre-binding snapshot");

        assert!(snapshot
            .workspaces
            .iter()
            .flat_map(|workspace| &workspace.tabs)
            .flat_map(|tab| tab.panes.values())
            .all(|pane| pane.terminal_theme_binding.is_none()));
    }

    #[test]
    fn capture_handoff_keeps_terminal_semantics_out_of_durable_snapshot() {
        let mut state = state_with_workspaces(&["space"]);
        let root_pane = state.workspaces[0].tabs[0].root_pane;
        let terminal_id = state.workspaces[0].terminal_id(root_pane).unwrap().clone();
        let terminal = state.terminals.get_mut(&terminal_id).unwrap();
        let _ = terminal.set_hook_authority_with_session_ref(
            "gardn:omp".to_string(),
            "omp".to_string(),
            crate::detect::AgentState::Working,
            Some("processing".to_string()),
            Some("reading".to_string()),
            Some(crate::agent_resume::AgentSessionRef {
                kind: crate::agent_resume::AgentSessionRefKind::Id,
                value: "session-1".to_string(),
            }),
            Some(7),
        );
        let _ = terminal.set_agent_metadata(crate::terminal::AgentMetadataReport {
            source: "gardn:omp:metadata".to_string(),
            agent_label: Some("omp".to_string()),
            applies_to_source: Some("gardn:omp".to_string()),
            title: Some("Oracle".to_string()),
            display_agent: Some("OMP".to_string()),
            custom_status: Some("thinking".to_string()),
            state_labels: HashMap::from([("working".to_string(), "busy".to_string())]),
            tokens: HashMap::new(),
            clear_title: false,
            clear_display_agent: false,
            clear_custom_status: false,
            clear_state_labels: false,
            ttl: None,
            seq: Some(9),
        });
        state.workspaces[0].tabs[0]
            .panes
            .get_mut(&root_pane)
            .unwrap()
            .seen = false;

        let durable = capture_from_state(&state);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let handoff = capture_handoff(
            &state.groups,
            state.active_group,
            state.group_filter_enabled,
            &state.session_namespace_id,
            &state.remote_termination_tombstones,
            &state.workspaces,
            &state.terminals,
            &terminal_runtimes,
            state.active,
            state.selected,
            state.agent_panel_scope,
            state.sidebar_width,
            state.sidebar_collapsed,
            state.sidebar_section_split,
            state.right_sidebar_width,
            state.right_sidebar_collapsed,
            &state.agent_follow_up,
        );
        let durable_pane = &durable.workspaces[0].tabs[0].panes[&root_pane.raw()];
        let handoff_pane = &handoff.workspaces[0].tabs[0].panes[&root_pane.raw()];

        assert!(!durable_pane.seen);
        assert!(durable_pane.terminal_semantics.is_none());
        let semantics = handoff_pane
            .terminal_semantics
            .as_ref()
            .expect("handoff should include live terminal semantics");
        assert_eq!(
            semantics
                .hook_authority
                .as_ref()
                .map(|authority| authority.agent_label.as_str()),
            Some("omp")
        );
        assert_eq!(semantics.state, crate::detect::AgentState::Working);
        assert_eq!(semantics.agent_metadata.len(), 1);
        assert_eq!(semantics.hook_report_sequences["gardn:omp"], 7);
        assert_eq!(semantics.metadata_report_sequences["gardn:omp:metadata"], 9);
    }

    #[test]
    fn capture_tracks_public_identity_counters() {
        let mut state = state_with_workspaces(&["one"]);
        let second = state.workspaces[0].test_split(Direction::Horizontal);
        let third = state.workspaces[0].test_split(Direction::Vertical);
        let second_tab = state.workspaces[0].test_add_tab(None);

        state.workspaces[0].close_pane(second);

        let snapshot = capture_from_state(&state);
        let workspace = &snapshot.workspaces[0];
        assert_eq!(
            workspace.public_pane_numbers,
            HashMap::from([
                (state.workspaces[0].tabs[0].root_pane.raw(), 1),
                (third.raw(), 3),
                (state.workspaces[0].tabs[second_tab].root_pane.raw(), 4),
            ])
        );
        assert_eq!(workspace.next_public_pane_number, 5);
        assert_eq!(workspace.public_tab_numbers, vec![1, 2]);
        assert_eq!(workspace.next_public_tab_number, 3);
    }

    #[test]
    fn capture_preserves_typed_session_namespace_id() {
        let mut state = AppState::test_new();
        let namespace =
            crate::execution_host::protocol::SessionNamespaceId::new("session-capture-ok")
                .expect("valid namespace");
        state.session_namespace_id = namespace.clone();

        let snap = capture_from_state(&state);
        assert_eq!(snap.session_namespace_id, "session-capture-ok");

        let restored = crate::persist::installation::session_namespace_from_snapshot(
            &snap.session_namespace_id,
        );
        assert_eq!(restored, namespace);
    }

    #[test]
    fn snapshot_dto_keeps_raw_string_for_legacy_decode() {
        // DTO stays String so malformed legacy JSON still deserializes; healing
        // happens after decode via session_namespace_from_snapshot.
        let json = r#"{
            "version": 5,
            "session_namespace_id": "bad id with spaces",
            "groups": [],
            "workspaces": []
        }"#;
        let snap: SessionSnapshot = serde_json::from_str(json).expect("raw string DTO decodes");
        assert_eq!(snap.session_namespace_id, "bad id with spaces");
        let healed = crate::persist::installation::session_namespace_from_snapshot(
            &snap.session_namespace_id,
        );
        assert_ne!(healed.as_str(), "bad id with spaces");
        assert!(crate::persist::installation::is_valid_session_namespace_id(
            healed.as_str()
        ));
    }

    fn root_split_ratio(tab: &TabSnapshot) -> Option<f32> {
        match &tab.layout {
            LayoutSnapshot::Split { ratio, .. } => Some(*ratio),
            LayoutSnapshot::Pane(_) => None,
        }
    }

    #[test]
    fn follow_up_queue_round_trips_and_drops_stale_targets() {
        let mut state = AppState::test_new();
        state.workspaces = vec![crate::workspace::Workspace::test_new("kept")];
        let pane = state.workspaces[0].tabs[0].root_pane;
        assert!(state.insert_agent_follow_up(0, pane));
        state
            .agent_follow_up
            .push(crate::app::state::AgentFollowUpEntry {
                workspace_id: "missing".into(),
                pane_number: 99,
                added_at_unix_secs: 1,
            });
        let snap = capture_from_state(&state);
        assert_eq!(snap.version, SNAPSHOT_VERSION);
        assert_eq!(snap.agent_follow_up.len(), 1);
        assert_eq!(snap.agent_follow_up[0].pane_number, 1);
        let json = serde_json::to_string(&snap).unwrap();
        let parsed = parse_snapshot(&json).expect("parse");
        assert_eq!(parsed.agent_follow_up, snap.agent_follow_up);
        let restored = crate::app::state::AppState::restored_agent_follow_up(
            &state.workspaces,
            vec![
                snap.agent_follow_up[0].clone(),
                crate::app::state::AgentFollowUpEntry {
                    workspace_id: "gone".into(),
                    pane_number: 1,
                    added_at_unix_secs: 3,
                },
            ],
        );
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].workspace_id, state.workspaces[0].id);
    }

    #[test]
    fn round_trip_empty_session() {
        let snap = SessionSnapshot {
            version: SNAPSHOT_VERSION,
            session_namespace_id: "session-test".to_string(),
            remote_termination_tombstones: Vec::new(),
            groups: default_groups(),
            active_group: 0,
            group_filter_enabled: true,
            default_view: SessionDefaultViewSnapshot {
                active: None,
                selected: 0,
                agent_panel_scope: AgentPanelScope::CurrentWorkspace,
                sidebar_width: Some(26),
                sidebar_collapsed: false,
                sidebar_section_split: Some(0.5),
                right_sidebar_width: Some(28),
                right_sidebar_collapsed: false,
                ui: SessionUiSnapshot::default(),
            },
            workspaces: vec![],
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
        };
        let json = serde_json::to_string(&snap).unwrap();
        let restored = parse_snapshot(&json).unwrap();
        assert!(restored.workspaces.is_empty());
        assert_eq!(restored.active, None);
        assert_eq!(restored.sidebar_width, Some(26));
        assert!(!restored.sidebar_collapsed);
        assert_eq!(restored.sidebar_section_split, Some(0.5));
        assert_eq!(restored.right_sidebar_width, Some(28));
        assert!(!restored.right_sidebar_collapsed);
    }

    #[test]
    fn round_trip_groups_and_workspace_membership() {
        let mut state = state_with_workspaces(&["one", "two"]);
        let group_id = crate::app::state::generate_group_id();
        state.groups.push(crate::app::state::Group {
            id: group_id.clone(),
            name: "Side".to_string(),
            icon: "✿".to_string(),
            accent: Some(crate::config::TerminalAccent::Cyan),
            default_location: None,
            favorite_agent_profile_ids: Vec::new(),
            default_agent_profile_id: None,
            github_organization: crate::app::state::GithubOrganization::parse("masakirocorp")
                .expect("valid organization"),
        });
        state.active_group = 1;
        state.group_filter_enabled = false;
        state.workspaces[1].group_id = group_id.clone();

        let json = serde_json::to_string(&capture_from_state(&state)).unwrap();
        let restored = parse_snapshot(&json).unwrap();

        assert_eq!(restored.groups.len(), 2);
        assert_eq!(restored.groups[1].name, "Side");
        assert_eq!(restored.groups[1].icon, "✿");
        assert_eq!(
            restored.groups[1].accent,
            Some(crate::config::TerminalAccent::Cyan)
        );
        assert_eq!(
            restored.groups[1]
                .github_organization
                .as_ref()
                .map(crate::app::state::GithubOrganization::as_str),
            Some("masakirocorp")
        );
        assert!(!restored.group_filter_enabled);
        assert_eq!(restored.workspaces[1].group_id, group_id);
    }

    #[test]
    fn round_trip_layout_snapshot() {
        let layout = LayoutSnapshot::Split {
            direction: DirectionSnapshot::Horizontal,
            ratio: 0.6,
            first: Box::new(LayoutSnapshot::Pane(0)),
            second: Box::new(LayoutSnapshot::Split {
                direction: DirectionSnapshot::Vertical,
                ratio: 0.5,
                first: Box::new(LayoutSnapshot::Pane(1)),
                second: Box::new(LayoutSnapshot::Pane(2)),
            }),
        };
        let json = serde_json::to_string(&layout).unwrap();
        let restored: LayoutSnapshot = serde_json::from_str(&json).unwrap();

        match restored {
            LayoutSnapshot::Split { ratio, .. } => assert!((ratio - 0.6).abs() < 0.01),
            _ => panic!("expected split"),
        }
    }

    #[test]
    fn round_trip_full_workspace_snapshot() {
        let mut panes = HashMap::new();
        panes.insert(
            0,
            PaneSnapshot {
                env_pane_id: None,
                cwd: PathBuf::from("/home/can/Projects/gardn"),
                location: None,
                remote_runtime_identity: None,
                label: None,
                agent_name: None,
                agent_session: None,
                launch_argv: None,
                launch_env: Vec::new(),
                terminal_theme_binding: None,
                seen: true,
                right_click_passthrough: false,
                terminal_semantics: None,
            },
        );
        panes.insert(
            1,
            PaneSnapshot {
                env_pane_id: None,
                cwd: PathBuf::from("/home/can/Projects/website"),
                location: None,
                remote_runtime_identity: None,
                label: Some("website".into()),
                agent_name: None,
                agent_session: None,
                launch_argv: None,
                launch_env: Vec::new(),
                terminal_theme_binding: None,
                seen: true,
                right_click_passthrough: false,
                terminal_semantics: None,
            },
        );

        let snap = SessionSnapshot {
            version: SNAPSHOT_VERSION,
            session_namespace_id: "session-test".to_string(),
            remote_termination_tombstones: Vec::new(),
            groups: default_groups(),
            active_group: 0,
            group_filter_enabled: true,
            default_view: SessionDefaultViewSnapshot {
                active: Some(0),
                selected: 0,
                agent_panel_scope: AgentPanelScope::CurrentWorkspace,
                sidebar_width: Some(26),
                sidebar_collapsed: false,
                sidebar_section_split: Some(0.5),
                right_sidebar_width: Some(28),
                right_sidebar_collapsed: false,
                ui: SessionUiSnapshot::default(),
            },
            workspaces: vec![WorkspaceSnapshot {
                id: Some("wproj".to_string()),
                custom_name: Some("pi-mono".to_string()),
                group_id: default_group_id(),
                identity_cwd: PathBuf::from("/home/can/Projects/gardn"),
                default_location: crate::execution_host::ResourceLocation::local(
                    "/home/can/Projects/gardn",
                )
                .unwrap(),
                github_scope: crate::github::GithubRepositoryScope::default(),
                public_pane_numbers: HashMap::from([(0, 1), (1, 2)]),
                next_public_pane_number: 3,
                public_tab_numbers: vec![1],
                next_public_tab_number: 2,
                tabs: vec![TabSnapshot {
                    custom_name: Some("api".to_string()),
                    role: crate::workspace::TabRole::Github,
                    layout: LayoutSnapshot::Split {
                        direction: DirectionSnapshot::Horizontal,
                        ratio: 0.5,
                        first: Box::new(LayoutSnapshot::Pane(0)),
                        second: Box::new(LayoutSnapshot::Pane(1)),
                    },
                    panes,
                    zoomed: false,
                    focused: Some(0),
                    root_pane: Some(0),
                }],
                active_tab: 0,
            }],
            active: Some(0),
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
        };

        let json = serde_json::to_string_pretty(&snap).unwrap();
        let restored = parse_snapshot(&json).unwrap();

        assert_eq!(restored.workspaces.len(), 1);
        assert_eq!(restored.workspaces[0].id.as_deref(), Some("wproj"));
        assert_eq!(
            restored.workspaces[0].custom_name.as_deref(),
            Some("pi-mono")
        );
        assert_eq!(restored.workspaces[0].tabs.len(), 1);
        assert_eq!(
            restored.workspaces[0].tabs[0].role,
            crate::workspace::TabRole::Github
        );
        assert_eq!(restored.workspaces[0].tabs[0].panes.len(), 2);
        assert_eq!(
            restored.workspaces[0].tabs[0].panes[&0].cwd,
            PathBuf::from("/home/can/Projects/gardn")
        );
        assert_eq!(
            restored.workspaces[0].tabs[0].panes[&1].label.as_deref(),
            Some("website")
        );
        assert_eq!(
            restored.agent_panel_scope,
            AgentPanelScope::CurrentWorkspace
        );
        assert_eq!(restored.sidebar_width, Some(26));
        assert_eq!(restored.sidebar_section_split, Some(0.5));
        assert_eq!(restored.right_sidebar_width, Some(28));
    }

    #[test]
    fn current_session_fixture_parses() {
        let snap = parse_snapshot(session_fixture("current-gardn")).unwrap();

        assert_eq!(snap.version, 3);
        assert_eq!(snap.workspaces.len(), 2);
        assert_eq!(snap.active, Some(0));
        assert_eq!(snap.selected, 0);
        assert_eq!(snap.agent_panel_scope, AgentPanelScope::CurrentWorkspace);
        assert_eq!(snap.sidebar_width, None);
        assert!(!snap.sidebar_collapsed);
        assert_eq!(snap.sidebar_section_split, None);
        assert_eq!(snap.right_sidebar_width, None);
        assert!(!snap.right_sidebar_collapsed);
        assert_eq!(snap.workspaces[0].tabs.len(), 2);
        assert_eq!(
            snap.workspaces[1].identity_cwd,
            PathBuf::from("/home/test/projects/project-b")
        );
    }

    #[test]
    fn current_dev_session_fixture_parses_additive_fields() {
        let snap = parse_snapshot(session_fixture("current-gardn-dev")).unwrap();

        assert_eq!(snap.version, 3);
        assert_eq!(snap.workspaces.len(), 2);
        assert_eq!(snap.agent_panel_scope, AgentPanelScope::CurrentWorkspace);
        assert_eq!(snap.sidebar_section_split, Some(0.4));
        assert_eq!(snap.workspaces[0].active_tab, 1);
        assert_eq!(snap.workspaces[1].tabs[0].panes.len(), 2);
    }

    #[test]
    fn old_snapshot_defaults_agent_panel_scope() {
        let json = serde_json::json!({
            "version": SNAPSHOT_VERSION,
            "workspaces": [],
            "active": null,
            "selected": 0
        })
        .to_string();

        let restored = parse_snapshot(&json).unwrap();

        assert_eq!(
            restored.agent_panel_scope,
            AgentPanelScope::CurrentWorkspace
        );
        assert_eq!(restored.sidebar_width, None);
        assert!(!restored.sidebar_collapsed);
        assert_eq!(restored.sidebar_section_split, None);
        assert_eq!(restored.right_sidebar_width, None);
        assert!(!restored.right_sidebar_collapsed);
    }

    #[test]
    fn old_pane_snapshot_with_embedded_history_is_ignored() {
        let json = serde_json::json!({
            "version": SNAPSHOT_VERSION,
            "workspaces": [{
                "id": "wtest",
                "identity_cwd": "/tmp",
                "tabs": [{
                    "layout": { "Pane": 0 },
                    "panes": {
                        "0": {
                            "cwd": "/tmp",
                            "history": {
                                "ansi": "legacy-secret",
                                "lines": 1
                            }
                        }
                    },
                    "zoomed": false,
                    "focused": 0,
                    "root_pane": 0
                }],
                "active_tab": 0
            }],
            "active": 0,
            "selected": 0
        })
        .to_string();

        let restored = parse_snapshot(&json).unwrap();

        let encoded = serde_json::to_string(&restored).unwrap();
        assert!(!encoded.contains("legacy-secret"));
        assert!(!encoded.contains("\"history\""));
    }

    #[test]
    fn legacy_workspace_snapshot_migrates_to_single_tab() {
        let snap = parse_snapshot(session_fixture("legacy-pre-tabs-v2")).unwrap();
        let ws = &snap.workspaces[0];

        assert_eq!(snap.version, 2);
        assert_eq!(snap.workspaces.len(), 1);
        assert_eq!(ws.custom_name.as_deref(), Some("legacy"));
        assert_eq!(ws.identity_cwd, PathBuf::from("/tmp/pion"));
        assert_eq!(ws.active_tab, 0);
        assert_eq!(ws.tabs.len(), 1);
        assert_eq!(ws.tabs[0].role, crate::workspace::TabRole::Terminal);
        assert_eq!(ws.tabs[0].focused, Some(1));
        assert_eq!(ws.tabs[0].root_pane, Some(0));
        assert_eq!(ws.tabs[0].panes[&0].cwd, PathBuf::from("/tmp/pion"));
        assert_eq!(ws.tabs[0].panes[&1].cwd, PathBuf::from("/tmp/gardn"));
    }

    #[test]
    fn capture_contract_tracks_workspace_order_active_and_selected() {
        let mut state = state_with_workspaces(&["a", "b", "c"]);
        state.active = Some(1);
        state.selected = 2;

        state.move_workspace(1, 0);

        let snapshot = capture_from_state(&state);
        let ids: Vec<_> = state.workspaces.iter().map(|ws| ws.id.clone()).collect();
        let captured_ids: Vec<_> = snapshot
            .workspaces
            .iter()
            .map(|ws| ws.id.clone().unwrap())
            .collect();
        assert_eq!(captured_ids, ids);
        assert_eq!(snapshot.active, state.active);
        assert_eq!(snapshot.selected, state.selected);
    }

    #[test]
    fn capture_contract_tracks_workspace_and_tab_names_and_active_tab() {
        let mut state = state_with_workspaces(&["one"]);
        state.workspaces[0].set_custom_name("renamed-workspace".into());
        let second_tab = state.workspaces[0].test_add_tab(Some("logs"));
        state.workspaces[0].switch_tab(second_tab);
        state.workspaces[0].tabs[0].set_custom_name("main".into());

        let snapshot = capture_from_state(&state);
        let workspace = &snapshot.workspaces[0];
        assert_eq!(workspace.custom_name.as_deref(), Some("renamed-workspace"));
        assert_eq!(workspace.active_tab, second_tab);
        assert_eq!(workspace.tabs[0].custom_name.as_deref(), Some("main"));
        assert_eq!(workspace.tabs[1].custom_name.as_deref(), Some("logs"));
    }

    #[test]
    fn capture_contract_tracks_workspace_closure() {
        let mut state = state_with_workspaces(&["one", "two"]);
        state.selected = 1;
        state.active = Some(1);

        state.close_selected_workspace();

        let snapshot = capture_from_state(&state);
        assert_eq!(snapshot.workspaces.len(), 1);
        assert_eq!(snapshot.workspaces[0].custom_name.as_deref(), Some("one"));
        assert_eq!(snapshot.active, Some(0));
        assert_eq!(snapshot.selected, 0);
    }

    #[test]
    fn capture_contract_tracks_sidebar_state() {
        let mut state = state_with_workspaces(&["one"]);
        state.sidebar_width = 31;
        state.sidebar_collapsed = true;
        state.sidebar_section_split = 0.4;
        state.right_sidebar_width = 34;
        state.right_sidebar_collapsed = true;
        state.agent_panel_scope = AgentPanelScope::AllWorkspaces;

        let snapshot = capture_from_state(&state);
        assert_eq!(snapshot.sidebar_width, Some(31));
        assert!(snapshot.sidebar_collapsed);
        assert_eq!(snapshot.sidebar_section_split, Some(0.4));
        assert_eq!(snapshot.right_sidebar_width, Some(34));
        assert!(snapshot.right_sidebar_collapsed);
        assert_eq!(snapshot.agent_panel_scope, AgentPanelScope::AllWorkspaces);
    }

    #[test]
    fn capture_contract_tracks_layout_focus_zoom_and_root_pane() {
        let mut state = state_with_workspaces(&["one"]);
        let root = state.workspaces[0].tabs[0].root_pane;
        let second = state.workspaces[0].test_split(Direction::Horizontal);
        state.workspaces[0].tabs[0].layout.focus_pane(second);
        state.toggle_zoom();

        let snapshot = capture_from_state(&state);
        let tab = &snapshot.workspaces[0].tabs[0];
        assert!(matches!(tab.layout, LayoutSnapshot::Split { .. }));
        assert_eq!(tab.focused, Some(second.raw()));
        assert_eq!(tab.root_pane, Some(root.raw()));
        assert!(tab.zoomed);
        assert_eq!(tab.panes.len(), 2);
    }

    #[test]
    fn capture_contract_tracks_focus_navigation() {
        let mut state = state_with_workspaces(&["one"]);
        let root = state.workspaces[0].tabs[0].root_pane;
        let second = state.workspaces[0].test_split(Direction::Horizontal);
        crate::ui::compute_view(&mut state, Rect::new(0, 0, 106, 20));

        state.navigate_pane(NavDirection::Right);

        let snapshot = capture_from_state(&state);
        assert_eq!(snapshot.workspaces[0].tabs[0].focused, Some(second.raw()));
        assert_ne!(snapshot.workspaces[0].tabs[0].focused, Some(root.raw()));
    }

    #[test]
    fn capture_contract_tracks_resize_ratio_changes() {
        let mut state = state_with_workspaces(&["one"]);
        state.workspaces[0].test_split(Direction::Horizontal);
        crate::ui::compute_view(&mut state, Rect::new(0, 0, 106, 20));
        let before = capture_from_state(&state);

        state.resize_pane(NavDirection::Right);

        let after = capture_from_state(&state);
        let before_ratio = root_split_ratio(&before.workspaces[0].tabs[0]).unwrap();
        let after_ratio = root_split_ratio(&after.workspaces[0].tabs[0]).unwrap();
        assert_ne!(before_ratio, after_ratio);
    }

    #[test]
    fn capture_contract_tracks_last_tab_closure_as_empty_workspace() {
        let mut state = state_with_workspaces(&["one"]);

        state.close_tab();

        let snapshot = capture_from_state(&state);
        let workspace = &snapshot.workspaces[0];
        assert!(workspace.tabs.is_empty());
        assert_eq!(workspace.active_tab, 0);
        assert_eq!(snapshot.active, Some(0));
    }

    #[test]
    fn capture_contract_tracks_non_last_tab_closure() {
        let mut state = state_with_workspaces(&["one"]);
        let second_tab = state.workspaces[0].test_add_tab(Some("logs"));
        state.switch_tab(second_tab);

        state.close_tab();

        let snapshot = capture_from_state(&state);
        let workspace = &snapshot.workspaces[0];
        assert_eq!(workspace.tabs.len(), 1);
        assert_eq!(workspace.active_tab, 0);
        assert!(workspace.tabs[0].custom_name.is_none());
    }

    #[test]
    fn capture_contract_tracks_pane_closure() {
        let mut state = state_with_workspaces(&["one"]);
        state.workspaces[0].test_split(Direction::Horizontal);

        state.close_pane();

        let snapshot = capture_from_state(&state);
        let tab = &snapshot.workspaces[0].tabs[0];
        assert_eq!(tab.panes.len(), 1);
        assert!(matches!(tab.layout, LayoutSnapshot::Pane(_)));
        assert!(!tab.zoomed);
    }

    #[test]
    fn capture_contract_tracks_workspace_identity_and_pane_cwds() {
        let mut state = state_with_workspaces(&["one"]);
        let root = state.workspaces[0].tabs[0].root_pane;
        state.workspaces[0].identity_cwd = PathBuf::from("/tmp/pion");
        let second = state.workspaces[0].test_split(Direction::Horizontal);
        state.ensure_test_terminals();
        let root_terminal_id = state.workspaces[0].tabs[0].panes[&root]
            .attached_terminal_id
            .clone();
        state.terminals.get_mut(&root_terminal_id).unwrap().cwd = PathBuf::from("/tmp/pion");
        let second_terminal_id = state.workspaces[0].tabs[0].panes[&second]
            .attached_terminal_id
            .clone();
        state.terminals.get_mut(&second_terminal_id).unwrap().cwd = PathBuf::from("/tmp/gardn");

        let snapshot = capture_from_state(&state);
        let workspace = &snapshot.workspaces[0];
        let tab = &workspace.tabs[0];
        assert_eq!(workspace.identity_cwd, PathBuf::from("/tmp/pion"));
        assert_eq!(tab.panes[&root.raw()].cwd, PathBuf::from("/tmp/pion"));
        assert_eq!(tab.panes[&second.raw()].cwd, PathBuf::from("/tmp/gardn"));
    }

    #[tokio::test]
    async fn capture_contract_tracks_pane_history_from_runtime() {
        let state = state_with_workspaces(&["one"]);
        let root = state.workspaces[0].tabs[0].root_pane;
        let terminal_id = state.workspaces[0].tabs[0].panes[&root]
            .attached_terminal_id
            .clone();
        let mut terminal_runtimes = TerminalRuntimeRegistry::new();
        terminal_runtimes.insert(
            terminal_id,
            crate::terminal::TerminalRuntime::test_with_scrollback_bytes(
                20,
                3,
                4096,
                b"alpha\r\nbeta\r\ngamma\r\n",
            ),
        );

        let snapshot = capture_from_state_with_runtimes(&state, &terminal_runtimes);
        let encoded = serde_json::to_string(&snapshot).unwrap();
        assert!(!encoded.contains("alpha"));
        assert!(!encoded.contains("\"history\""));

        let history_snapshot = capture_history_from_state_with_runtimes(&state, &terminal_runtimes);
        let history = &history_snapshot.workspaces[0].tabs[0].panes[&root.raw()];

        assert!(history.ansi.contains("alpha"));
        assert!(history.ansi.contains("gamma"));
        assert!(history.lines >= 3);
    }

    #[tokio::test]
    async fn capture_contract_tracks_history_for_each_pane() {
        let mut state = state_with_workspaces(&["one"]);
        let first = state.workspaces[0].tabs[0].root_pane;
        let second = state.workspaces[0].test_split(Direction::Horizontal);
        let first_terminal_id = state.workspaces[0].tabs[0].panes[&first]
            .attached_terminal_id
            .clone();
        let second_terminal_id = state.workspaces[0].tabs[0].panes[&second]
            .attached_terminal_id
            .clone();
        let mut terminal_runtimes = TerminalRuntimeRegistry::new();
        terminal_runtimes.insert(
            first_terminal_id,
            crate::terminal::TerminalRuntime::test_with_scrollback_bytes(
                20,
                3,
                4096,
                b"first-pane-history\r\n",
            ),
        );
        terminal_runtimes.insert(
            second_terminal_id,
            crate::terminal::TerminalRuntime::test_with_scrollback_bytes(
                20,
                3,
                4096,
                b"second-pane-history\r\n",
            ),
        );

        let snapshot = capture_from_state_with_runtimes(&state, &terminal_runtimes);
        let encoded = serde_json::to_string(&snapshot).unwrap();
        assert!(!encoded.contains("first-pane-history"));
        assert!(!encoded.contains("second-pane-history"));

        let history_snapshot = capture_history_from_state_with_runtimes(&state, &terminal_runtimes);
        let tab = &history_snapshot.workspaces[0].tabs[0];
        let first_history = &tab.panes[&first.raw()];
        let second_history = &tab.panes[&second.raw()];

        assert!(first_history.ansi.contains("first-pane-history"));
        assert!(second_history.ansi.contains("second-pane-history"));
    }

    #[test]
    fn capture_contract_tracks_hook_authority_agent_session() {
        let mut state = state_with_workspaces(&["one"]);
        let root = state.workspaces[0].tabs[0].root_pane;
        state.ensure_test_terminals();
        let terminal_id = state.workspaces[0].tabs[0].panes[&root]
            .attached_terminal_id
            .clone();
        state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .set_hook_authority_with_session_ref(
                "gardn:pi".into(),
                "pi".into(),
                crate::detect::AgentState::Working,
                None,
                None,
                crate::agent_resume::AgentSessionRef::path("/tmp/pi-session.jsonl"),
                Some(20),
            );

        let snapshot = capture_from_state(&state);
        let agent_session = snapshot.workspaces[0].tabs[0].panes[&root.raw()]
            .agent_session
            .as_ref()
            .expect("agent session should be captured");

        assert_eq!(agent_session.source, "gardn:pi");
        assert_eq!(agent_session.agent, "pi");
        assert_eq!(
            agent_session.kind,
            crate::agent_resume::AgentSessionRefKind::Path
        );
        assert_eq!(agent_session.value, "/tmp/pi-session.jsonl");
    }

    #[test]
    fn capture_contract_preserves_restored_agent_session() {
        let mut state = state_with_workspaces(&["one"]);
        let root = state.workspaces[0].tabs[0].root_pane;
        state.ensure_test_terminals();
        let terminal_id = state.workspaces[0].tabs[0].panes[&root]
            .attached_terminal_id
            .clone();
        state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .set_persisted_agent_session(crate::agent_resume::PersistedAgentSession {
                source: "gardn:opencode".into(),
                agent: "opencode".into(),
                session_ref: crate::agent_resume::AgentSessionRef::id("opencode-session").unwrap(),
            });

        let snapshot = capture_from_state(&state);
        let agent_session = snapshot.workspaces[0].tabs[0].panes[&root.raw()]
            .agent_session
            .as_ref()
            .expect("persisted agent session should be captured");

        assert_eq!(agent_session.source, "gardn:opencode");
        assert_eq!(agent_session.agent, "opencode");
        assert_eq!(
            agent_session.kind,
            crate::agent_resume::AgentSessionRefKind::Id
        );
        assert_eq!(agent_session.value, "opencode-session");
    }

    #[test]
    fn old_unversioned_snapshot_loads_as_version_0() {
        let json = r#"{"workspaces":[],"active":null,"selected":0}"#;
        let snap = parse_snapshot(json).unwrap();
        assert_eq!(snap.version, 0);
    }

    #[test]
    fn future_version_is_rejected() {
        let json = r#"{"version":999,"workspaces":[],"active":null,"selected":0}"#;
        let err = match parse_snapshot(json) {
            Ok(_) => panic!("future snapshot version should be rejected"),
            Err(err) => err,
        };
        assert!(
            err.contains("snapshot version 999 is newer than supported"),
            "error should identify unsupported future version: {err}"
        );
    }

    #[test]
    fn active_tab_default_is_zero() {
        let json = r#"{"custom_name":"test","identity_cwd":"/tmp","default_location":{"execution_host_id":"local","path":"/tmp"},"tabs":[]}"#;
        let ws: WorkspaceSnapshot = serde_json::from_str(json).unwrap();
        assert_eq!(ws.active_tab, 0);
    }

    #[test]
    fn legacy_path_defaults_migrate_once_to_local_locations() {
        let json = r#"{
            "version": 4,
            "groups": [{
                "id": "group-legacy",
                "name": "legacy",
                "default_directory": "/legacy/group"
            }],
            "workspaces": [{
                "id": "workspace-legacy",
                "group_id": "group-legacy",
                "identity_cwd": "/legacy/identity",
                "default_cwd": "/legacy/workspace",
                "tabs": []
            }]
        }"#;

        let snapshot = parse_snapshot(json).unwrap();

        let group_location = snapshot.groups[0].default_location.as_ref().unwrap();
        assert!(group_location.is_local());
        assert_eq!(
            group_location.path.as_path(),
            std::path::Path::new("/legacy/group")
        );
        let workspace_location = &snapshot.workspaces[0].default_location;
        assert!(workspace_location.is_local());
        assert_eq!(
            workspace_location.path.as_path(),
            std::path::Path::new("/legacy/workspace")
        );
    }
}
