use std::path::PathBuf;

use tracing::error;

use std::collections::HashSet;

use super::{
    api_helpers::{pane_agent_status, tab_attention_priority},
    App, ClientViewState, Mode,
};
use crate::{
    config::NewTerminalCwdConfig,
    workspace::{derive_label_from_cwd, Workspace},
};

pub(crate) fn resolve_new_terminal_cwd(
    policy: &NewTerminalCwdConfig,
    follow_cwd: Option<PathBuf>,
) -> PathBuf {
    match policy {
        NewTerminalCwdConfig::Follow => follow_cwd
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/")),
        NewTerminalCwdConfig::Home => std::env::var_os("HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("/")),
        NewTerminalCwdConfig::Current => {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
        }
        NewTerminalCwdConfig::Path(path) => expand_new_terminal_cwd_path(path),
    }
}

fn expand_new_terminal_cwd_path(path: &str) -> PathBuf {
    if path == "~" {
        return std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

pub(crate) struct PendingRemoteCreation {
    runtime: crate::terminal::TerminalRuntime,
    plan: PendingRemoteCreationPlan,
    client_focus: Option<(u64, crate::api::PendingFocusMarker)>,
}

impl PendingRemoteCreation {
    fn new(runtime: crate::terminal::TerminalRuntime, plan: PendingRemoteCreationPlan) -> Self {
        Self {
            runtime,
            plan,
            client_focus: None,
        }
    }

    fn set_client_focus(&mut self, client_view_id: u64, marker: crate::api::PendingFocusMarker) {
        self.client_focus = Some((client_view_id, marker));
    }

    pub(crate) fn requested_location(&self) -> &crate::execution_host::ResourceLocation {
        match &self.plan {
            PendingRemoteCreationPlan::Workspace { terminal, .. }
            | PendingRemoteCreationPlan::Tab { terminal, .. }
            | PendingRemoteCreationPlan::Split { terminal, .. } => &terminal.location,
        }
    }
}

enum PendingRemoteCreationPlan {
    Workspace {
        workspace: Box<Workspace>,
        terminal: Box<crate::terminal::TerminalState>,
        focus: bool,
    },
    Tab {
        workspace_id: String,
        tab: Box<crate::workspace::Tab>,
        terminal: Box<crate::terminal::TerminalState>,
        focus: bool,
    },
    Split {
        workspace_id: String,
        target_pane_id: crate::layout::PaneId,
        new_pane_id: crate::layout::PaneId,
        tab_number: usize,
        direction: ratatui::layout::Direction,
        ratio: Option<f32>,
        terminal: Box<crate::terminal::TerminalState>,
        focus: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommittedRemoteCreation {
    Workspace {
        ws_idx: usize,
    },
    Tab {
        ws_idx: usize,
        tab_idx: usize,
    },
    Split {
        ws_idx: usize,
        tab_idx: usize,
        pane_id: crate::layout::PaneId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteCreationCompletion {
    pub terminal_id: crate::terminal::TerminalId,
    pub result: Result<CommittedRemoteCreation, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PendingRemoteCreationTarget {
    pub workspace_id: String,
    pub tab_number: usize,
    pub pane_id: crate::layout::PaneId,
    pub location: crate::execution_host::ResourceLocation,
    pub focus: bool,
}

pub(crate) enum WorkspaceCreation {
    Committed(usize),
    Pending(crate::terminal::TerminalId),
}

pub(crate) enum TabCreation {
    Committed(usize),
    Pending(crate::terminal::TerminalId),
}

pub(crate) fn workspace_create_label(input: &str, suggested_name: &str) -> Option<String> {
    let name = input.trim();
    (!name.is_empty() && name != suggested_name).then(|| name.to_string())
}

impl App {
    pub(super) fn collision_free_workspace_name(
        &self,
        initial_cwd: &std::path::Path,
        group_id: &str,
    ) -> Option<String> {
        let base = derive_label_from_cwd(initial_cwd);
        let names: HashSet<_> = self
            .state
            .workspaces
            .iter()
            .filter(|ws| ws.group_id == group_id)
            .map(|ws| ws.display_name())
            .collect();

        if !names.contains(&base) {
            return None;
        }

        (2..)
            .map(|suffix| format!("{base} {suffix}"))
            .find(|candidate| !names.contains(candidate))
    }

    pub(super) fn seed_cwd_from_workspace(&self, ws_idx: usize) -> Option<std::path::PathBuf> {
        self.state.workspaces.get(ws_idx).map(|workspace| {
            workspace.effective_default_cwd_from(&self.state.terminals, &self.terminal_runtimes)
        })
    }

    pub(super) fn focused_live_terminal_location(
        &self,
        ws_idx: usize,
    ) -> Option<crate::execution_host::ResourceLocation> {
        let workspace = self.state.workspaces.get(ws_idx)?;
        let pane_id = workspace.focused_pane_id()?;
        let terminal_id = workspace.terminal_id(pane_id)?;
        self.terminal_runtimes.get(terminal_id)?;
        self.state
            .terminals
            .get(terminal_id)
            .map(|terminal| terminal.location.clone())
    }

    pub(super) fn tab_creation_location(
        &self,
        ws_idx: usize,
    ) -> Option<crate::execution_host::ResourceLocation> {
        let workspace = self.state.workspaces.get(ws_idx)?;
        Some(crate::execution_host::placement::resolve_tab_creation(
            None,
            self.focused_live_terminal_location(ws_idx),
            workspace.default_location.clone(),
        ))
    }

    pub(super) fn resolve_new_terminal_cwd(&self, follow_cwd: Option<PathBuf>) -> PathBuf {
        resolve_new_terminal_cwd(&self.state.new_terminal_cwd, follow_cwd)
    }

    pub(super) fn workspace_creation_source(&self) -> Option<usize> {
        if self.state.mode == Mode::Navigate
            && self.state.workspaces.get(self.state.selected).is_some()
            && self.state.workspace_in_active_group(self.state.selected)
        {
            return Some(self.state.selected);
        }

        self.state
            .active
            .filter(|idx| self.state.workspace_in_active_group(*idx))
            .or_else(|| {
                self.state
                    .workspaces
                    .get(self.state.selected)
                    .filter(|_| self.state.workspace_in_active_group(self.state.selected))
                    .map(|_| self.state.selected)
            })
    }

    pub(super) fn workspace_creation_group_id(&self, source: Option<usize>) -> String {
        source
            .and_then(|ws_idx| self.state.workspaces.get(ws_idx))
            .map(|ws| ws.group_id.clone())
            .unwrap_or_else(|| self.state.active_group_id().to_string())
    }

    pub(super) fn group_default_location(
        &self,
        group_id: &str,
    ) -> Option<crate::execution_host::ResourceLocation> {
        self.state
            .groups
            .iter()
            .find(|group| group.id == group_id)
            .and_then(|group| group.default_location.clone())
    }

    pub(super) fn begin_tui_workspace_create(&mut self, request_id: &'static str) {
        if self.state.prompt_new_workspace_name {
            let source = self.workspace_creation_source();
            let group_id = self.workspace_creation_group_id(source);
            let local_fallback = match crate::execution_host::ResourceLocation::local(
                self.resolve_new_terminal_cwd(None),
            ) {
                Ok(location) => location,
                Err(error) => {
                    error!(%error, "failed to resolve local workspace location");
                    return;
                }
            };
            let location = crate::execution_host::placement::resolve_workspace_creation(
                None,
                self.group_default_location(&group_id),
                local_fallback,
            );
            super::input::open_new_workspace_dialog_at_location(&mut self.state, location);
            return;
        }

        self.dispatch_runtime_mutation(
            request_id,
            crate::api::schema::Method::WorkspaceCreate(
                crate::api::schema::WorkspaceCreateParams {
                    cwd: None,
                    location: None,
                    focus: true,
                    label: None,
                    env: Default::default(),
                },
            ),
        );
        self.state.mode = if self.state.active.is_some() {
            Mode::Terminal
        } else {
            Mode::Navigate
        };
    }

    /// Create a workspace with a real PTY (needs event_tx).
    pub(crate) fn create_workspace(&mut self) {
        let custom_name = self.state.requested_new_workspace_name.take();
        let source = self.workspace_creation_source();
        let group_id = self.workspace_creation_group_id(source);
        let local_fallback = match crate::execution_host::ResourceLocation::local(
            self.resolve_new_terminal_cwd(None),
        ) {
            Ok(location) => location,
            Err(error) => {
                error!(%error, "failed to resolve local workspace location");
                self.state.mode = Mode::Navigate;
                return;
            }
        };
        let location = crate::execution_host::placement::resolve_workspace_creation(
            self.state.pending_workspace_create_location.take(),
            self.group_default_location(&group_id),
            local_fallback,
        );
        match self.create_workspace_with_location_in_group(location, true, group_id, Vec::new()) {
            Ok(WorkspaceCreation::Committed(ws_idx)) => {
                if let Some(name) = custom_name {
                    if let Some(workspace) = self.state.workspaces.get_mut(ws_idx) {
                        workspace.set_custom_name(name);
                        self.state.mark_session_dirty();
                    }
                }
            }
            Ok(WorkspaceCreation::Pending(terminal_id)) => {
                if let Some(name) = custom_name {
                    self.set_pending_remote_container_name(&terminal_id, name);
                }
            }
            Err(error) => {
                error!(%error, "failed to create workspace");
                self.state.mode = Mode::Navigate;
            }
        }
    }
    pub(crate) fn create_tab(&mut self) {
        let custom_name = self.state.requested_new_tab_name.take();
        let Some(ws_idx) = self.state.active else {
            let initial_cwd = self.resolve_new_terminal_cwd(None);
            if let Err(error) = self.create_workspace_with_options(initial_cwd, true) {
                error!(%error, "failed to create workspace for tab");
            }
            return;
        };
        let Some(location) = self.tab_creation_location(ws_idx) else {
            error!("failed to resolve tab location: workspace not found");
            return;
        };
        let result = if location.is_local() {
            self.create_tab_with_options(location.path.as_path().to_path_buf(), true)
                .map(TabCreation::Committed)
                .map_err(|error| error.to_string())
        } else {
            self.begin_remote_tab(ws_idx, location, true, None, Vec::new())
                .map(TabCreation::Pending)
        };
        match result {
            Ok(TabCreation::Committed(tab_idx)) => {
                if let Some(name) = custom_name {
                    if let Some(tab) = self
                        .state
                        .workspaces
                        .get_mut(ws_idx)
                        .and_then(|workspace| workspace.tabs.get_mut(tab_idx))
                    {
                        tab.set_custom_name(name);
                        self.schedule_session_save();
                    }
                }
            }
            Ok(TabCreation::Pending(terminal_id)) => {
                if let Some(name) = custom_name {
                    self.set_pending_remote_container_name(&terminal_id, name);
                }
            }
            Err(error) => error!(%error, "failed to create tab"),
        }
    }

    pub(crate) fn create_tab_for_workspace(
        &mut self,
        ws_idx: usize,
        custom_name: Option<String>,
        role: crate::workspace::TabRole,
    ) -> Result<TabCreation, String> {
        let previous_active = self.state.active;
        let previous_mode = self.state.mode;
        let location = self
            .tab_creation_location(ws_idx)
            .ok_or_else(|| "workspace not found".to_string())?;
        self.state.active = Some(ws_idx);
        let result = if location.is_local() {
            self.create_tab_with_options(location.path.as_path().to_path_buf(), false)
                .map(TabCreation::Committed)
                .map_err(|error| error.to_string())
        } else {
            self.begin_remote_tab(ws_idx, location, false, None, Vec::new())
                .map(TabCreation::Pending)
        };
        match &result {
            Ok(TabCreation::Committed(tab_idx)) => {
                let tab = &mut self.state.workspaces[ws_idx].tabs[*tab_idx];
                tab.role = role;
                if let Some(name) = custom_name {
                    tab.set_custom_name(name);
                }
                self.schedule_session_save();
            }
            Ok(TabCreation::Pending(terminal_id)) => {
                if let Some(PendingRemoteCreation {
                    plan: PendingRemoteCreationPlan::Tab { tab, .. },
                    ..
                }) = self.pending_remote_creations.get_mut(terminal_id)
                {
                    tab.role = role;
                    if let Some(name) = custom_name {
                        tab.set_custom_name(name);
                    }
                }
            }
            Err(_) => {}
        }
        self.state.active = previous_active;
        self.state.mode = previous_mode;
        result
    }

    pub(super) fn create_tab_with_options(
        &mut self,
        initial_cwd: PathBuf,
        focus: bool,
    ) -> std::io::Result<usize> {
        let Some(ws_idx) = self.state.active else {
            return self.create_workspace_with_options(initial_cwd, focus);
        };
        let (rows, cols) = self.state.estimate_pane_size();
        let scrollback_limit_bytes = self.state.pane_scrollback_limit_bytes;
        let host_terminal_theme = self.state.host_terminal_theme;
        let default_shell = self.state.default_shell.clone();
        let shell_mode = self.state.shell_mode;
        let event_tx = self.event_tx.clone();
        let render_notify = self.render_notify.clone();
        let render_dirty = self.render_dirty.clone();
        let (idx, terminal, runtime, root_pane) = {
            let ws = &mut self.state.workspaces[ws_idx];
            let (idx, terminal, runtime) = ws.create_tab_with_handles(
                rows,
                cols,
                initial_cwd,
                scrollback_limit_bytes,
                host_terminal_theme,
                crate::pane::PaneShellConfig::new(&default_shell, shell_mode),
                event_tx,
                render_notify,
                render_dirty,
            )?;
            let root_pane = ws.tabs[idx].root_pane;
            (idx, terminal, runtime, root_pane)
        };
        self.terminal_runtimes.insert(terminal.id.clone(), runtime);
        self.state.terminals.insert(terminal.id.clone(), terminal);
        self.state.remove_alias_shadowed_by_new_pane(root_pane);
        if focus {
            self.state.workspaces[ws_idx].switch_tab(idx);
            self.state.mode = Mode::Terminal;
        }
        let workspace_id = self.state.workspaces[ws_idx].id.clone();
        let tab_id = self
            .public_tab_id(ws_idx, idx)
            .unwrap_or_else(|| format!("{}:{}", workspace_id, idx + 1));
        let root_pane = self.state.workspaces[ws_idx].tabs[idx].root_pane.raw();
        crate::logging::tab_created(&workspace_id, &tab_id, root_pane);
        self.schedule_session_save();
        Ok(idx)
    }

    pub(crate) fn create_agent_profile_tab(
        &mut self,
        ws_idx: usize,
        profile_id: &str,
    ) -> std::io::Result<usize> {
        let Some(profile) = self.state.agent_profiles.get(profile_id).cloned() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "agent profile not found",
            ));
        };
        if let Some(warning) = crate::integration::agent_profile_integration_warning(&profile) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                warning,
            ));
        }
        if !self.state.agent_profile_launchable(&profile) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "agent profile requires an installed integration",
            ));
        }
        let launch_env = profile.managed_launch_env();
        let follow_cwd = self.seed_cwd_from_workspace(ws_idx);
        let initial_cwd = self.resolve_new_terminal_cwd(follow_cwd);
        let (rows, cols) = self.state.estimate_pane_size();
        let scrollback_limit_bytes = self.state.pane_scrollback_limit_bytes;
        let host_terminal_theme = self.state.host_terminal_theme;
        let default_shell = self.state.default_shell.clone();
        let shell_mode = self.state.shell_mode;
        let (idx, terminal, runtime, root_pane) = {
            let ws = &mut self.state.workspaces[ws_idx];
            let (idx, mut terminal, runtime) = ws.create_profile_command_tab(
                rows,
                cols,
                initial_cwd,
                crate::pane::PaneShellConfig::new(&default_shell, shell_mode),
                &profile.command,
                &launch_env,
                scrollback_limit_bytes,
                host_terminal_theme,
            )?;
            terminal.launch_argv = Some(profile.argv.clone());
            terminal.launch_env = launch_env;
            if let Some(tab) = ws.tabs.get_mut(idx) {
                tab.set_custom_name(profile.name.clone());
            }
            let root_pane = ws.tabs[idx].root_pane;
            (idx, terminal, runtime, root_pane)
        };
        self.terminal_runtimes.insert(terminal.id.clone(), runtime);
        self.state.terminals.insert(terminal.id.clone(), terminal);
        self.state.remove_alias_shadowed_by_new_pane(root_pane);
        self.state.workspaces[ws_idx].switch_tab(idx);
        self.state.active = Some(ws_idx);
        self.state.mode = Mode::Terminal;
        let workspace_id = self.state.workspaces[ws_idx].id.clone();
        let tab_id = self
            .public_tab_id(ws_idx, idx)
            .unwrap_or_else(|| format!("{}:{}", workspace_id, idx + 1));
        crate::logging::tab_created(&workspace_id, &tab_id, root_pane.raw());
        self.schedule_session_save();
        Ok(idx)
    }

    pub(super) fn create_workspace_with_options(
        &mut self,
        initial_cwd: PathBuf,
        focus: bool,
    ) -> std::io::Result<usize> {
        let group_id = self.state.active_group_id().to_string();
        self.create_workspace_with_launch_env_in_group(initial_cwd, focus, group_id, Vec::new())
    }

    pub(crate) fn create_workspace_with_launch_env_in_group(
        &mut self,
        initial_cwd: PathBuf,
        focus: bool,
        group_id: String,
        extra_env: Vec<(String, String)>,
    ) -> std::io::Result<usize> {
        let (rows, cols) = self.state.estimate_pane_size();
        let custom_name = self.collision_free_workspace_name(&initial_cwd, &group_id);
        let (mut ws, terminal, runtime) = Workspace::new_with_extra_env(
            initial_cwd,
            rows,
            cols,
            self.state.pane_scrollback_limit_bytes,
            self.state.host_terminal_theme,
            crate::pane::PaneShellConfig::new(&self.state.default_shell, self.state.shell_mode),
            self.event_tx.clone(),
            self.render_notify.clone(),
            self.render_dirty.clone(),
            extra_env,
        )?;
        ws.group_id = group_id;
        if let Some(name) = custom_name {
            ws.set_custom_name(name);
        }
        self.terminal_runtimes.insert(terminal.id.clone(), runtime);
        self.state.terminals.insert(terminal.id.clone(), terminal);
        self.state.workspaces.push(ws);
        let idx = self.state.workspaces.len() - 1;
        self.state
            .remove_alias_shadowed_by_new_pane(self.state.workspaces[idx].tabs[0].root_pane);
        let workspace_id = self.state.workspaces[idx].id.clone();
        let root_pane = self.state.workspaces[idx].tabs[0].root_pane.raw();
        crate::logging::workspace_created(&workspace_id, root_pane);
        if focus || self.state.active.is_none() {
            self.state.switch_workspace(idx);
            self.state.mode = Mode::Terminal;
        }
        self.schedule_session_save();
        Ok(idx)
    }
    pub(crate) fn create_workspace_with_location_in_group(
        &mut self,
        location: crate::execution_host::ResourceLocation,
        focus: bool,
        group_id: String,
        extra_env: Vec<(String, String)>,
    ) -> Result<WorkspaceCreation, String> {
        if location.is_local() {
            return self
                .create_workspace_with_launch_env_in_group(
                    location.path.as_path().to_path_buf(),
                    focus,
                    group_id,
                    extra_env,
                )
                .map(WorkspaceCreation::Committed)
                .map_err(|error| error.to_string());
        }
        self.begin_remote_workspace(location, focus, group_id, None, extra_env)
            .map(WorkspaceCreation::Pending)
    }

    pub(crate) fn begin_remote_workspace(
        &mut self,
        location: crate::execution_host::ResourceLocation,
        focus: bool,
        group_id: String,
        command: Option<crate::execution_host::protocol::CommandSpec>,
        extra_env: Vec<(String, String)>,
    ) -> Result<crate::terminal::TerminalId, String> {
        if location.is_local() {
            return Err("remote workspace creation requires a non-local location".to_string());
        }
        let (rows, cols) = self.state.estimate_pane_size();
        let terminal_id = crate::terminal::TerminalId::alloc();
        let tab = crate::workspace::Tab::remote(
            1,
            terminal_id.clone(),
            self.event_tx.clone(),
            self.render_notify.clone(),
            self.render_dirty.clone(),
        );
        let pane_id = tab.root_pane;
        let mut workspace = Workspace::from_remote_tab(location.clone(), tab);
        workspace.group_id = group_id.clone();
        if let Some(name) = self.collision_free_workspace_name(location.path.as_path(), &group_id) {
            workspace.set_custom_name(name);
        }
        let hosts = self
            .execution_hosts
            .as_mut()
            .ok_or_else(|| "execution host manager is unavailable".to_string())?;
        let runtime = hosts.create_terminal(
            terminal_id.clone(),
            pane_id,
            location.clone(),
            rows,
            cols,
            self.state.pane_scrollback_limit_bytes,
            self.state.host_terminal_theme,
            self.event_tx.clone(),
            command,
            extra_env.clone(),
        )?;
        let terminal = crate::terminal::TerminalState::new_at(terminal_id.clone(), location)
            .with_launch_env(extra_env);
        self.pending_remote_creations.insert(
            terminal_id.clone(),
            PendingRemoteCreation::new(
                runtime,
                PendingRemoteCreationPlan::Workspace {
                    workspace: Box::new(workspace),
                    terminal: Box::new(terminal),
                    focus,
                },
            ),
        );
        Ok(terminal_id)
    }

    pub(crate) fn create_named_workspace_for_client_view(
        &mut self,
        client_view: &mut ClientViewState,
        location: crate::execution_host::ResourceLocation,
        group_idx: usize,
        label: Option<String>,
    ) {
        let Some(group_id) = self
            .state
            .groups
            .get(group_idx)
            .map(|group| group.id.clone())
        else {
            return;
        };
        match self.create_workspace_with_location_in_group(location, false, group_id, Vec::new()) {
            Ok(WorkspaceCreation::Committed(ws_idx)) => {
                if let Some(name) = label {
                    if let Some(workspace) = self.state.workspaces.get_mut(ws_idx) {
                        workspace.set_custom_name(name);
                        self.state.mark_session_dirty();
                    }
                }
                client_view.active_group = group_idx;
                self.switch_client_view_workspace(client_view, ws_idx);
            }
            Ok(WorkspaceCreation::Pending(terminal_id)) => {
                if let Some(name) = label {
                    self.set_pending_remote_container_name(&terminal_id, name);
                }
                client_view.active_group = group_idx;
                if let Some(workspace_id) = self.pending_remote_workspace_id(&terminal_id) {
                    client_view.pending_active_workspace = Some(workspace_id.clone());
                    if let Some(pending) = self.pending_remote_creations.get_mut(&terminal_id) {
                        pending.set_client_focus(
                            client_view.id(),
                            crate::api::PendingFocusMarker::Workspace { workspace_id },
                        );
                    }
                }
            }
            Err(err) => {
                tracing::error!(err = %err, "failed to create named client workspace");
                self.show_remote_create_failed_toast(err);
                client_view.mode = Mode::Navigate;
            }
        }
    }

    pub(super) fn begin_remote_tab(
        &mut self,
        ws_idx: usize,
        location: crate::execution_host::ResourceLocation,
        focus: bool,
        command: Option<crate::execution_host::protocol::CommandSpec>,
        extra_env: Vec<(String, String)>,
    ) -> Result<crate::terminal::TerminalId, String> {
        let workspace = self
            .state
            .workspaces
            .get(ws_idx)
            .ok_or_else(|| "workspace not found".to_string())?;
        let workspace_id = workspace.id.clone();
        let number = self
            .pending_remote_creations
            .values()
            .filter_map(|pending| match &pending.plan {
                PendingRemoteCreationPlan::Tab {
                    workspace_id: pending_workspace_id,
                    tab,
                    ..
                } if pending_workspace_id == &workspace_id => Some(tab.number.saturating_add(1)),
                _ => None,
            })
            .fold(workspace.next_remote_tab_number(), usize::max);
        let terminal_id = crate::terminal::TerminalId::alloc();
        let tab = crate::workspace::Tab::remote(
            number,
            terminal_id.clone(),
            self.event_tx.clone(),
            self.render_notify.clone(),
            self.render_dirty.clone(),
        );
        let pane_id = tab.root_pane;
        let (rows, cols) = self.state.estimate_pane_size();
        let hosts = self
            .execution_hosts
            .as_mut()
            .ok_or_else(|| "execution host manager is unavailable".to_string())?;
        let runtime = hosts.create_terminal(
            terminal_id.clone(),
            pane_id,
            location.clone(),
            rows,
            cols,
            self.state.pane_scrollback_limit_bytes,
            self.state.host_terminal_theme,
            self.event_tx.clone(),
            command,
            extra_env.clone(),
        )?;
        let terminal = crate::terminal::TerminalState::new_at(terminal_id.clone(), location)
            .with_launch_env(extra_env);
        self.pending_remote_creations.insert(
            terminal_id.clone(),
            PendingRemoteCreation::new(
                runtime,
                PendingRemoteCreationPlan::Tab {
                    workspace_id,
                    tab: Box::new(tab),
                    terminal: Box::new(terminal),
                    focus,
                },
            ),
        );
        Ok(terminal_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn begin_remote_split(
        &mut self,
        ws_idx: usize,
        target_pane_id: crate::layout::PaneId,
        direction: ratatui::layout::Direction,
        ratio: Option<f32>,
        location: crate::execution_host::ResourceLocation,
        focus: bool,
        command: Option<crate::execution_host::protocol::CommandSpec>,
        extra_env: Vec<(String, String)>,
    ) -> Result<crate::terminal::TerminalId, String> {
        let workspace = self
            .state
            .workspaces
            .get(ws_idx)
            .ok_or_else(|| "workspace not found".to_string())?;
        let tab_number = workspace
            .find_tab_index_for_pane(target_pane_id)
            .and_then(|tab_idx| workspace.tabs.get(tab_idx))
            .map(|tab| tab.number)
            .ok_or_else(|| "pane not found".to_string())?;
        let workspace_id = workspace.id.clone();
        let new_pane_id = crate::layout::PaneId::alloc();
        let terminal_id = crate::terminal::TerminalId::alloc();
        let (rows, cols) = self.state.estimate_pane_size();
        let hosts = self
            .execution_hosts
            .as_mut()
            .ok_or_else(|| "execution host manager is unavailable".to_string())?;
        let runtime = hosts.create_terminal(
            terminal_id.clone(),
            new_pane_id,
            location.clone(),
            rows,
            cols,
            self.state.pane_scrollback_limit_bytes,
            self.state.host_terminal_theme,
            self.event_tx.clone(),
            command,
            extra_env.clone(),
        )?;
        let terminal = crate::terminal::TerminalState::new_at(terminal_id.clone(), location)
            .with_launch_env(extra_env);
        self.pending_remote_creations.insert(
            terminal_id.clone(),
            PendingRemoteCreation::new(
                runtime,
                PendingRemoteCreationPlan::Split {
                    workspace_id,
                    target_pane_id,
                    tab_number,
                    new_pane_id,
                    direction,
                    ratio,
                    terminal: Box::new(terminal),
                    focus,
                },
            ),
        );
        Ok(terminal_id)
    }

    pub(crate) fn complete_remote_creation_ready(
        &mut self,
        terminal_id: crate::terminal::TerminalId,
        identity: crate::execution_host::protocol::RuntimeIdentity,
        resolved_location: crate::execution_host::ResourceLocation,
    ) {
        let Some(pending) = self.pending_remote_creations.remove(&terminal_id) else {
            if let Some(terminal) = self.state.terminals.get_mut(&terminal_id) {
                terminal.remote_runtime_identity = Some(identity);
                self.state.mark_session_dirty();
            }
            return;
        };
        let result =
            self.commit_remote_creation(pending, identity.clone(), resolved_location.clone());
        if result.is_err() {
            // Commit rejected after worker ACK: terminate the returned runtime before
            // reporting failure so the worker process is not orphaned.
            if let Some(hosts) = self.execution_hosts.as_mut() {
                hosts.begin_runtime_termination(terminal_id.clone(), resolved_location, identity);
            }
        }
        self.remote_creation_completions
            .push(RemoteCreationCompletion {
                terminal_id,
                result,
            });
    }

    pub(crate) fn complete_remote_creation_failed(
        &mut self,
        terminal_id: crate::terminal::TerminalId,
        message: String,
    ) {
        if let Some(pending) = self.pending_remote_creations.remove(&terminal_id) {
            // Keep host-side request identity for late ACK reconcile; mark termination-pending.
            if let Some(hosts) = self.execution_hosts.as_mut() {
                let _ = hosts.cancel_pending_create(&terminal_id);
            }
            pending.runtime.shutdown();
            let api_pending = self.pending_remote_api_responses.contains_key(&terminal_id);
            if !api_pending {
                if let Some((client_view_id, marker)) = pending.client_focus {
                    self.queue_pending_focus_cleanup(Some(client_view_id), Some(marker));
                }
                self.show_remote_create_failed_toast(message.clone());
            }
            self.remote_creation_completions
                .push(RemoteCreationCompletion {
                    terminal_id,
                    result: Err(message),
                });
        }
    }

    fn show_remote_create_failed_toast(&mut self, message: String) {
        let previous_toast = self.state.toast.clone();
        self.state.toast = Some(super::state::ToastNotification {
            kind: super::state::ToastKind::NeedsAttention,
            title: "Workspace create failed".to_string(),
            context: message,
            position: None,
            target: None,
        });
        self.sync_toast_deadline(previous_toast);
    }

    /// Explicit disconnect / source teardown: fail deferred responder promptly while
    /// retaining enough host-side identity for late ACK reconcile.
    pub(crate) fn abort_pending_remote_creation(
        &mut self,
        terminal_id: crate::terminal::TerminalId,
        message: impl Into<String>,
    ) -> bool {
        if !self.pending_remote_creations.contains_key(&terminal_id) {
            return false;
        }
        self.complete_remote_creation_failed(terminal_id, message.into());
        true
    }

    pub(crate) fn take_remote_creation_completions(&mut self) -> Vec<RemoteCreationCompletion> {
        std::mem::take(&mut self.remote_creation_completions)
    }
    pub(crate) fn take_github_remote_completion(
        &mut self,
        terminal_id: &crate::terminal::TerminalId,
    ) -> Option<RemoteCreationCompletion> {
        let index = self
            .remote_creation_completions
            .iter()
            .position(|completion| completion.terminal_id == *terminal_id)?;
        Some(self.remote_creation_completions.remove(index))
    }

    fn commit_remote_creation(
        &mut self,
        pending: PendingRemoteCreation,
        identity: crate::execution_host::protocol::RuntimeIdentity,
        resolved_location: crate::execution_host::ResourceLocation,
    ) -> Result<CommittedRemoteCreation, String> {
        let PendingRemoteCreation {
            runtime,
            plan,
            client_focus: _,
        } = pending;
        match plan {
            PendingRemoteCreationPlan::Workspace {
                mut workspace,
                mut terminal,
                focus,
            } => {
                workspace.identity_cwd = resolved_location.path.as_path().to_path_buf();
                workspace.default_location = resolved_location.clone();
                terminal.cwd = resolved_location.path.as_path().to_path_buf();
                terminal.location = resolved_location;
                terminal.remote_runtime_identity = Some(identity);
                let terminal_id = terminal.id.clone();
                let root_pane = workspace.tabs[0].root_pane;
                let workspace_id = workspace.id.clone();
                self.terminal_runtimes.insert(terminal_id.clone(), runtime);
                self.state.terminals.insert(terminal_id, *terminal);
                self.state.workspaces.push(*workspace);
                let ws_idx = self.state.workspaces.len() - 1;
                self.state.remove_alias_shadowed_by_new_pane(root_pane);
                crate::logging::workspace_created(&workspace_id, root_pane.raw());
                if focus || self.state.active.is_none() {
                    self.state.switch_workspace(ws_idx);
                    self.state.mode = Mode::Terminal;
                }
                self.schedule_session_save();
                Ok(CommittedRemoteCreation::Workspace { ws_idx })
            }
            PendingRemoteCreationPlan::Tab {
                workspace_id,
                tab,
                mut terminal,
                focus,
            } => {
                let Some(ws_idx) = self
                    .state
                    .workspaces
                    .iter()
                    .position(|workspace| workspace.id == workspace_id)
                else {
                    runtime.shutdown();
                    return Err("workspace no longer exists".to_string());
                };
                terminal.cwd = resolved_location.path.as_path().to_path_buf();
                terminal.location = resolved_location;
                terminal.remote_runtime_identity = Some(identity);
                let terminal_id = terminal.id.clone();
                let root_pane = tab.root_pane;
                let tab_idx = self.state.workspaces[ws_idx].add_remote_tab(*tab);
                self.terminal_runtimes.insert(terminal_id.clone(), runtime);
                self.state.terminals.insert(terminal_id, *terminal);
                self.state.remove_alias_shadowed_by_new_pane(root_pane);
                if focus {
                    self.state.workspaces[ws_idx].switch_tab(tab_idx);
                    self.state.active = Some(ws_idx);
                    self.state.mode = Mode::Terminal;
                }
                self.schedule_session_save();
                Ok(CommittedRemoteCreation::Tab { ws_idx, tab_idx })
            }
            PendingRemoteCreationPlan::Split {
                workspace_id,
                target_pane_id,
                new_pane_id,
                tab_number: _,
                direction,
                ratio,
                mut terminal,
                focus,
            } => {
                let Some(ws_idx) = self
                    .state
                    .workspaces
                    .iter()
                    .position(|workspace| workspace.id == workspace_id)
                else {
                    runtime.shutdown();
                    return Err("workspace no longer exists".to_string());
                };
                terminal.cwd = resolved_location.path.as_path().to_path_buf();
                terminal.location = resolved_location;
                terminal.remote_runtime_identity = Some(identity);
                let terminal_id = terminal.id.clone();
                let Some((tab_idx, new_pane)) = self.state.workspaces[ws_idx].commit_remote_split(
                    target_pane_id,
                    new_pane_id,
                    direction,
                    ratio,
                    focus,
                    *terminal,
                    runtime,
                ) else {
                    // Source pane deleted before ACK commit. Outer ready handler terminates
                    // the worker runtime identity before reporting failure.
                    return Err("source pane no longer exists".to_string());
                };
                self.terminal_runtimes
                    .insert(terminal_id.clone(), new_pane.runtime);
                self.state.terminals.insert(terminal_id, new_pane.terminal);
                self.state.remove_alias_shadowed_by_new_pane(new_pane_id);
                self.schedule_session_save();
                Ok(CommittedRemoteCreation::Split {
                    ws_idx,
                    tab_idx,
                    pane_id: new_pane_id,
                })
            }
        }
    }

    pub(super) fn collect_panes_for_workspace(
        &self,
        workspace_id: Option<&str>,
    ) -> Result<Vec<crate::api::schema::PaneInfo>, (String, String)> {
        if let Some(workspace_id) = workspace_id {
            let Some(ws_idx) = self.parse_workspace_id(workspace_id) else {
                return Err((
                    "workspace_not_found".into(),
                    format!("workspace {workspace_id} not found"),
                ));
            };
            let Some(ws) = self.state.workspaces.get(ws_idx) else {
                return Err((
                    "workspace_not_found".into(),
                    format!("workspace {workspace_id} not found"),
                ));
            };
            Ok(ws
                .tabs
                .iter()
                .flat_map(|tab| tab.layout.pane_ids().into_iter())
                .filter_map(|pane_id| self.pane_info(ws_idx, pane_id))
                .collect())
        } else {
            Ok(self
                .state
                .workspaces
                .iter()
                .enumerate()
                .flat_map(|(ws_idx, ws)| {
                    ws.tabs
                        .iter()
                        .flat_map(|tab| tab.layout.pane_ids().into_iter())
                        .filter_map(move |pane_id| self.pane_info(ws_idx, pane_id))
                })
                .collect())
        }
    }

    pub(super) fn collect_panes_for_workspace_for_view(
        &self,
        view: &crate::app::view_state::ClientViewState,
        workspace_id: Option<&str>,
    ) -> Result<Vec<crate::api::schema::PaneInfo>, (String, String)> {
        if let Some(workspace_id) = workspace_id {
            let Some(ws_idx) = self.parse_workspace_id(workspace_id) else {
                return Err((
                    "workspace_not_found".into(),
                    format!("workspace {workspace_id} not found"),
                ));
            };
            let Some(ws) = self.state.workspaces.get(ws_idx) else {
                return Err((
                    "workspace_not_found".into(),
                    format!("workspace {workspace_id} not found"),
                ));
            };
            Ok(ws
                .tabs
                .iter()
                .flat_map(|tab| tab.layout.pane_ids().into_iter())
                .filter_map(|pane_id| self.pane_info_for_view(view, ws_idx, pane_id))
                .collect())
        } else {
            Ok(self
                .state
                .workspaces
                .iter()
                .enumerate()
                .flat_map(|(ws_idx, ws)| {
                    ws.tabs
                        .iter()
                        .flat_map(|tab| tab.layout.pane_ids().into_iter())
                        .filter_map(move |pane_id| self.pane_info_for_view(view, ws_idx, pane_id))
                })
                .collect())
        }
    }

    pub(super) fn tab_info(
        &self,
        ws_idx: usize,
        tab_idx: usize,
    ) -> Option<crate::api::schema::TabInfo> {
        let ws = self.state.workspaces.get(ws_idx)?;
        let tab = ws.tabs.get(tab_idx)?;
        let (agg_state, seen) = tab
            .panes
            .values()
            .filter_map(|pane| {
                self.state
                    .terminals
                    .get(&pane.attached_terminal_id)
                    .map(|terminal| (terminal.state, pane.seen))
            })
            .max_by_key(|(state, seen)| tab_attention_priority(*state, *seen))
            .unwrap_or((crate::detect::AgentState::Unknown, true));
        Some(crate::api::schema::TabInfo {
            tab_id: self.public_tab_id(ws_idx, tab_idx)?,
            workspace_id: self.public_workspace_id(ws_idx),
            number: tab_idx + 1,
            label: ws.tab_display_name(tab_idx)?,
            focused: self.state.active == Some(ws_idx) && ws.active_tab == tab_idx,
            pane_count: tab.panes.len(),
            agent_status: pane_agent_status(agg_state, seen),
        })
    }

    #[cfg(test)]
    pub(super) fn workspace_created_result(
        &self,
        ws_idx: usize,
    ) -> Option<crate::api::schema::ResponseResult> {
        Some(crate::api::schema::ResponseResult::WorkspaceCreated {
            workspace: self.workspace_info(ws_idx),
            tab: self.tab_info(ws_idx, 0)?,
            root_pane: self.root_pane_info(ws_idx, 0)?,
        })
    }

    pub(super) fn tab_created_result(
        &self,
        ws_idx: usize,
        tab_idx: usize,
    ) -> Option<crate::api::schema::ResponseResult> {
        Some(crate::api::schema::ResponseResult::TabCreated {
            tab: self.tab_info(ws_idx, tab_idx)?,
            root_pane: self.root_pane_info(ws_idx, tab_idx)?,
        })
    }

    pub(super) fn root_pane_info(
        &self,
        ws_idx: usize,
        tab_idx: usize,
    ) -> Option<crate::api::schema::PaneInfo> {
        let ws = self.state.workspaces.get(ws_idx)?;
        let tab = ws.tabs.get(tab_idx)?;
        self.pane_info(ws_idx, tab.root_pane)
    }

    pub(super) fn pane_info(
        &self,
        ws_idx: usize,
        pane_id: crate::layout::PaneId,
    ) -> Option<crate::api::schema::PaneInfo> {
        let ws = self.state.workspaces.get(ws_idx)?;
        let pane = ws.pane_state(pane_id)?;
        let terminal = self.state.terminals.get(&pane.attached_terminal_id)?;
        let tab_idx = ws.find_tab_index_for_pane(pane_id)?;
        let focused = self.state.active == Some(ws_idx)
            && ws.active_tab == tab_idx
            && ws
                .focused_pane_id()
                .is_some_and(|focused| focused == pane_id);
        let presentation = terminal.effective_presentation();
        Some(crate::api::schema::PaneInfo {
            pane_id: self.public_pane_id(ws_idx, pane_id)?,
            terminal_id: terminal.id.to_string(),
            location: crate::api::schema::resource_location_params_from(&terminal.location),
            workspace_id: self.public_workspace_id(ws_idx),
            tab_id: self.public_tab_id(ws_idx, tab_idx)?,
            focused,
            cwd: ws.tabs[tab_idx]
                .cwd_for_pane(pane_id, &self.state.terminals, &self.terminal_runtimes)
                .map(|cwd| cwd.display().to_string()),
            foreground_cwd: ws.tabs[tab_idx]
                .foreground_cwd_for_pane(pane_id, &self.terminal_runtimes)
                .map(|cwd| cwd.display().to_string()),
            label: terminal.manual_label.clone(),
            agent: terminal.lifecycle_agent_label().map(str::to_string),
            title: presentation.title,
            display_agent: presentation.display_agent,
            agent_status: pane_agent_status(terminal.state, pane.seen),
            custom_status: presentation.custom_status,
            state_labels: presentation.state_labels,
            tokens: presentation.tokens,
            agent_session: terminal_agent_session_info(terminal),
            scroll: self.pane_scroll_info(ws_idx, pane_id),
            revision: terminal.revision,
        })
    }

    pub(super) fn pane_info_for_view(
        &self,
        view: &crate::app::view_state::ClientViewState,
        ws_idx: usize,
        pane_id: crate::layout::PaneId,
    ) -> Option<crate::api::schema::PaneInfo> {
        let ws = self.state.workspaces.get(ws_idx)?;
        let pane = ws.pane_state(pane_id)?;
        let terminal = self.state.terminals.get(&pane.attached_terminal_id)?;
        let tab_idx = ws.find_tab_index_for_pane(pane_id)?;
        let focused = view.active_workspace == Some(ws_idx)
            && view.active_tab_index_for_workspace(&self.state, ws_idx) == Some(tab_idx)
            && view
                .focused_pane_for_tab(&ws.id, tab_idx + 1)
                .is_some_and(|focused| focused == pane_id);
        let presentation = terminal.effective_presentation();
        Some(crate::api::schema::PaneInfo {
            pane_id: self.public_pane_id(ws_idx, pane_id)?,
            terminal_id: terminal.id.to_string(),
            location: crate::api::schema::resource_location_params_from(&terminal.location),
            workspace_id: self.public_workspace_id(ws_idx),
            tab_id: self.public_tab_id(ws_idx, tab_idx)?,
            focused,
            cwd: ws.tabs[tab_idx]
                .cwd_for_pane(pane_id, &self.state.terminals, &self.terminal_runtimes)
                .map(|cwd| cwd.display().to_string()),
            foreground_cwd: ws.tabs[tab_idx]
                .foreground_cwd_for_pane(pane_id, &self.terminal_runtimes)
                .map(|cwd| cwd.display().to_string()),
            label: terminal.manual_label.clone(),
            agent: terminal.lifecycle_agent_label().map(str::to_string),
            title: presentation.title,
            display_agent: presentation.display_agent,
            agent_status: pane_agent_status(terminal.state, pane.seen),
            custom_status: presentation.custom_status,
            state_labels: presentation.state_labels,
            tokens: presentation.tokens,
            agent_session: terminal_agent_session_info(terminal),
            scroll: self.pane_scroll_info(ws_idx, pane_id),
            revision: terminal.revision,
        })
    }

    fn pane_scroll_info(
        &self,
        ws_idx: usize,
        pane_id: crate::layout::PaneId,
    ) -> Option<crate::api::schema::PaneScrollInfo> {
        let metrics = self.state.pane_scroll_metrics_in_workspace(
            &self.terminal_runtimes,
            ws_idx,
            pane_id,
        )?;
        Some(crate::api::schema::PaneScrollInfo {
            offset_from_bottom: metrics.offset_from_bottom as u64,
            max_offset_from_bottom: metrics.max_offset_from_bottom as u64,
            viewport_rows: metrics.viewport_rows as u64,
        })
    }

    pub(super) fn lookup_runtime(
        &self,
        ws_idx: usize,
        pane_id: crate::layout::PaneId,
    ) -> Option<(&crate::terminal::TerminalRuntime, String)> {
        let runtime =
            self.state
                .runtime_for_pane_in_workspace(&self.terminal_runtimes, ws_idx, pane_id)?;
        Some((runtime, self.public_workspace_id(ws_idx)))
    }

    pub(super) fn lookup_runtime_sender(
        &self,
        ws_idx: usize,
        pane_id: crate::layout::PaneId,
    ) -> Option<&crate::terminal::TerminalRuntime> {
        self.state
            .runtime_for_pane_in_workspace(&self.terminal_runtimes, ws_idx, pane_id)
    }

    pub(super) fn workspace_info(&self, index: usize) -> crate::api::schema::WorkspaceInfo {
        let ws = &self.state.workspaces[index];
        let (agg_state, seen) = ws.aggregate_state(&self.state.terminals);
        crate::api::schema::WorkspaceInfo {
            workspace_id: self.public_workspace_id(index),
            group_id: ws.group_id.clone(),
            default_location: crate::api::schema::resource_location_params_from(
                &ws.default_location,
            ),
            number: index + 1,
            label: ws.display_name_from(&self.state.terminals, &self.terminal_runtimes),
            focused: self.state.active == Some(index),
            pane_count: ws.public_pane_numbers.len(),
            tab_count: ws.tabs.len(),
            active_tab_id: self
                .public_tab_id(index, ws.active_tab)
                .unwrap_or_else(|| format!("{}:{}", ws.id, ws.active_tab + 1)),
            agent_status: pane_agent_status(agg_state, seen),
        }
    }

    pub(super) fn group_info(&self, index: usize) -> crate::api::schema::GroupInfo {
        let group = &self.state.groups[index];
        let workspace_count = self
            .state
            .workspaces
            .iter()
            .filter(|workspace| workspace.group_id == group.id)
            .count();
        crate::api::schema::GroupInfo {
            group_id: group.id.clone(),
            number: index + 1,
            name: group.name.clone(),
            icon: group.icon.clone(),
            focused: self.state.active_group == index,
            workspace_count,
            default_location: group
                .default_location
                .as_ref()
                .map(crate::api::schema::resource_location_params_from),
            accent: group.accent.map(|accent| accent.as_str().to_string()),
            github_organization: group
                .github_organization
                .as_ref()
                .map(|organization| organization.as_str().to_string()),
        }
    }
    pub(crate) fn has_pending_github_tab_for_workspace(&self, workspace_id: &str) -> bool {
        self.pending_remote_creations.values().any(|pending| {
            matches!(
                &pending.plan,
                PendingRemoteCreationPlan::Tab {
                    workspace_id: pending_workspace_id,
                    tab,
                    ..
                } if pending_workspace_id == workspace_id
                    && tab.role == crate::workspace::TabRole::Github
            )
        })
    }

    pub(crate) fn set_pending_remote_container_name(
        &mut self,
        terminal_id: &crate::terminal::TerminalId,
        name: String,
    ) -> bool {
        let Some(pending) = self.pending_remote_creations.get_mut(terminal_id) else {
            return false;
        };
        match &mut pending.plan {
            PendingRemoteCreationPlan::Workspace { workspace, .. } => {
                workspace.set_custom_name(name);
                true
            }
            PendingRemoteCreationPlan::Tab { tab, .. } => {
                tab.set_custom_name(name);
                true
            }
            PendingRemoteCreationPlan::Split { .. } => false,
        }
    }

    pub(crate) fn store_pending_remote_api_response(
        &mut self,
        terminal_id: crate::terminal::TerminalId,
        pending: super::PendingRemoteApiResponse,
    ) {
        self.pending_remote_api_responses
            .insert(terminal_id, pending);
    }

    /// Encode and send any deferred API responses for completed remote creates.
    /// Sender disconnection must not affect commit (already done); send errors are ignored.
    pub(crate) fn finish_remote_api_completions(&mut self) -> bool {
        use super::api::responses::{encode_error, encode_success};
        use crate::api::schema::{EventData, EventEnvelope, EventKind, ResponseResult};

        let completions = self.take_remote_creation_completions();
        if completions.is_empty() {
            return false;
        }
        let mut changed = false;
        for completion in completions {
            if self.github_remote_completion_owned(&completion.terminal_id) {
                self.remote_creation_completions.push(completion);
                changed = true;
                continue;
            }
            let Some(pending) = self
                .pending_remote_api_responses
                .remove(&completion.terminal_id)
            else {
                // Non-API create (TUI/settings) — state already committed/dropped.
                changed = true;
                continue;
            };
            let response = match (completion.result, pending.kind) {
                (
                    Ok(crate::app::creation::CommittedRemoteCreation::Workspace { ws_idx }),
                    super::PendingRemoteApiKind::WorkspaceCreate { label },
                ) => {
                    if let Some(label) = label {
                        if let Some(workspace) = self.state.workspaces.get_mut(ws_idx) {
                            workspace.set_custom_name(label);
                            crate::logging::workspace_renamed(&workspace.id);
                        }
                    }
                    self.apply_pending_remote_focus(
                        pending.focus,
                        pending.client_view_id,
                        &crate::app::creation::CommittedRemoteCreation::Workspace { ws_idx },
                    );
                    let encode_view = self.response_view_for_pending_focus(
                        pending.focus,
                        pending.client_view_id,
                        &crate::app::creation::CommittedRemoteCreation::Workspace { ws_idx },
                    );
                    let workspace = match encode_view.as_ref() {
                        Some(view) => self.workspace_info_for_view(view, ws_idx),
                        None => self.workspace_info(ws_idx),
                    };
                    let tab = match encode_view.as_ref() {
                        Some(view) => self.tab_info_for_view(view, ws_idx, 0),
                        None => self.tab_info(ws_idx, 0),
                    };
                    let root_pane = match encode_view.as_ref() {
                        Some(view) => self.root_pane_info_for_view(view, ws_idx, 0),
                        None => self.root_pane_info(ws_idx, 0),
                    };
                    if let (Some(tab), Some(root_pane)) = (tab, root_pane) {
                        self.emit_event(EventEnvelope {
                            event: EventKind::WorkspaceCreated,
                            data: EventData::WorkspaceCreated {
                                workspace: workspace.clone(),
                            },
                        });
                        self.emit_event(EventEnvelope {
                            event: EventKind::TabCreated,
                            data: EventData::TabCreated { tab: tab.clone() },
                        });
                        self.emit_event(EventEnvelope {
                            event: EventKind::PaneCreated,
                            data: EventData::PaneCreated {
                                pane: root_pane.clone(),
                            },
                        });
                        encode_success(
                            pending.request_id,
                            ResponseResult::WorkspaceCreated {
                                workspace,
                                tab,
                                root_pane,
                            },
                        )
                    } else {
                        self.queue_pending_focus_cleanup(
                            pending.client_view_id,
                            pending.pending_focus.clone(),
                        );
                        encode_error(
                            pending.request_id,
                            "workspace_create_failed",
                            "workspace create completed without a root pane",
                        )
                    }
                }
                (
                    Ok(crate::app::creation::CommittedRemoteCreation::Tab { ws_idx, tab_idx }),
                    super::PendingRemoteApiKind::TabCreate { label },
                ) => {
                    if let Some(label) = label {
                        let workspace_id = self.state.workspaces[ws_idx].id.clone();
                        let tab_id = self
                            .public_tab_id(ws_idx, tab_idx)
                            .unwrap_or_else(|| format!("{}:{}", workspace_id, tab_idx + 1));
                        if let Some(tab) = self
                            .state
                            .workspaces
                            .get_mut(ws_idx)
                            .and_then(|ws| ws.tabs.get_mut(tab_idx))
                        {
                            tab.set_custom_name(label);
                            crate::logging::tab_renamed(&workspace_id, &tab_id);
                        }
                    }
                    self.apply_pending_remote_focus(
                        pending.focus,
                        pending.client_view_id,
                        &crate::app::creation::CommittedRemoteCreation::Tab { ws_idx, tab_idx },
                    );
                    let encode_view = self.response_view_for_pending_focus(
                        pending.focus,
                        pending.client_view_id,
                        &crate::app::creation::CommittedRemoteCreation::Tab { ws_idx, tab_idx },
                    );
                    let result = match encode_view.as_ref() {
                        Some(view) => self.tab_created_result_for_view(view, ws_idx, tab_idx),
                        None => self.tab_created_result(ws_idx, tab_idx),
                    };
                    if let Some(result) = result {
                        if let ResponseResult::TabCreated { tab, root_pane } = &result {
                            self.emit_event(EventEnvelope {
                                event: EventKind::TabCreated,
                                data: EventData::TabCreated { tab: tab.clone() },
                            });
                            self.emit_event(EventEnvelope {
                                event: EventKind::PaneCreated,
                                data: EventData::PaneCreated {
                                    pane: root_pane.clone(),
                                },
                            });
                        }
                        encode_success(pending.request_id, result)
                    } else {
                        self.queue_pending_focus_cleanup(
                            pending.client_view_id,
                            pending.pending_focus.clone(),
                        );
                        encode_error(
                            pending.request_id,
                            "tab_create_failed",
                            "tab create completed without a root pane",
                        )
                    }
                }
                (
                    Ok(crate::app::creation::CommittedRemoteCreation::Split {
                        ws_idx,
                        tab_idx,
                        pane_id,
                    }),
                    super::PendingRemoteApiKind::PaneSplit,
                ) => {
                    self.apply_pending_remote_focus(
                        pending.focus,
                        pending.client_view_id,
                        &crate::app::creation::CommittedRemoteCreation::Split {
                            ws_idx,
                            tab_idx,
                            pane_id,
                        },
                    );
                    let encode_view = self.response_view_for_pending_focus(
                        pending.focus,
                        pending.client_view_id,
                        &crate::app::creation::CommittedRemoteCreation::Split {
                            ws_idx,
                            tab_idx,
                            pane_id,
                        },
                    );
                    let pane = match encode_view.as_ref() {
                        Some(view) => self.pane_info_for_view(view, ws_idx, pane_id),
                        None => self.pane_info(ws_idx, pane_id),
                    };
                    if let Some(pane) = pane {
                        self.emit_event(EventEnvelope {
                            event: EventKind::PaneCreated,
                            data: EventData::PaneCreated { pane: pane.clone() },
                        });
                        self.emit_layout_updated_event(ws_idx, tab_idx);
                        encode_success(pending.request_id, ResponseResult::PaneInfo { pane })
                    } else {
                        self.queue_pending_focus_cleanup(
                            pending.client_view_id,
                            pending.pending_focus.clone(),
                        );
                        encode_error(
                            pending.request_id,
                            "pane_split_failed",
                            "pane split completed without pane info",
                        )
                    }
                }
                (Ok(committed), super::PendingRemoteApiKind::AgentStart { argv }) => {
                    self.apply_pending_remote_focus(
                        pending.focus,
                        pending.client_view_id,
                        &committed,
                    );
                    let agent = match committed {
                        crate::app::creation::CommittedRemoteCreation::Workspace { ws_idx } => {
                            let pane_id = self.state.workspaces[ws_idx].tabs[0].root_pane;
                            self.agent_info(ws_idx, pane_id)
                        }
                        crate::app::creation::CommittedRemoteCreation::Tab { ws_idx, tab_idx } => {
                            let pane_id = self.state.workspaces[ws_idx].tabs[tab_idx].root_pane;
                            self.agent_info(ws_idx, pane_id)
                        }
                        crate::app::creation::CommittedRemoteCreation::Split {
                            ws_idx,
                            pane_id,
                            ..
                        } => self.agent_info(ws_idx, pane_id),
                    };
                    match agent {
                        Some(agent) => encode_success(
                            pending.request_id,
                            ResponseResult::AgentStarted { agent, argv },
                        ),
                        None => {
                            self.queue_pending_focus_cleanup(
                                pending.client_view_id,
                                pending.pending_focus.clone(),
                            );
                            encode_error(
                                pending.request_id,
                                "agent_start_failed",
                                "agent start completed without agent info",
                            )
                        }
                    }
                }
                (Ok(_), kind) => {
                    self.queue_pending_focus_cleanup(
                        pending.client_view_id,
                        pending.pending_focus.clone(),
                    );
                    encode_error(
                        pending.request_id,
                        match kind {
                            super::PendingRemoteApiKind::WorkspaceCreate { .. } => {
                                "workspace_create_failed"
                            }
                            super::PendingRemoteApiKind::TabCreate { .. } => "tab_create_failed",
                            super::PendingRemoteApiKind::PaneSplit => "pane_split_failed",
                            super::PendingRemoteApiKind::AgentStart { .. } => "agent_start_failed",
                        },
                        "remote create completed with mismatched API kind",
                    )
                }
                (Err(message), kind) => {
                    self.queue_pending_focus_cleanup(
                        pending.client_view_id,
                        pending.pending_focus.clone(),
                    );
                    encode_error(
                        pending.request_id,
                        match kind {
                            super::PendingRemoteApiKind::WorkspaceCreate { .. } => {
                                "workspace_create_failed"
                            }
                            super::PendingRemoteApiKind::TabCreate { .. } => "tab_create_failed",
                            super::PendingRemoteApiKind::PaneSplit => "pane_split_failed",
                            super::PendingRemoteApiKind::AgentStart { .. } => "agent_start_failed",
                        },
                        message,
                    )
                }
            };
            let _ = pending.respond_to.send(response);
            changed = true;
        }
        changed
    }

    /// Queue a client-scoped cleanup for the exact pending focus marker of a failed create.
    fn queue_pending_focus_cleanup(
        &mut self,
        client_view_id: Option<u64>,
        pending_focus: Option<crate::api::PendingFocusMarker>,
    ) {
        let Some(client_view_id) = client_view_id else {
            return;
        };
        let Some(marker) = pending_focus else {
            return;
        };
        // Apply immediately to default_client_view when it is the owner.
        let effect = crate::app::view_state::ClientViewEffect::ClearPendingFocus {
            client_view_id,
            marker,
        };
        let _ = self.default_client_view.apply_client_view_effect(&effect);
        self.pending_client_view_effects.push(effect);
    }

    /// Drain queued client-scoped effects for external routers (headless/tests).
    pub(crate) fn take_client_view_effects(
        &mut self,
    ) -> Vec<crate::app::view_state::ClientViewEffect> {
        std::mem::take(&mut self.pending_client_view_effects)
    }

    pub(crate) fn pending_remote_workspace_id(
        &self,
        terminal_id: &crate::terminal::TerminalId,
    ) -> Option<String> {
        let pending = self.pending_remote_creations.get(terminal_id)?;
        match &pending.plan {
            PendingRemoteCreationPlan::Workspace { workspace, .. } => Some(workspace.id.clone()),
            PendingRemoteCreationPlan::Tab { workspace_id, .. }
            | PendingRemoteCreationPlan::Split { workspace_id, .. } => Some(workspace_id.clone()),
        }
    }

    pub(crate) fn pending_remote_creation_target(
        &self,
        terminal_id: &crate::terminal::TerminalId,
    ) -> Option<PendingRemoteCreationTarget> {
        let pending = self.pending_remote_creations.get(terminal_id)?;
        match &pending.plan {
            PendingRemoteCreationPlan::Workspace {
                workspace,
                terminal,
                focus,
            } => {
                let tab = workspace.tabs.first()?;
                Some(PendingRemoteCreationTarget {
                    workspace_id: workspace.id.clone(),
                    tab_number: tab.number,
                    pane_id: tab.root_pane,
                    location: terminal.location.clone(),
                    focus: *focus,
                })
            }
            PendingRemoteCreationPlan::Tab {
                workspace_id,
                tab,
                terminal,
                focus,
            } => Some(PendingRemoteCreationTarget {
                workspace_id: workspace_id.clone(),
                tab_number: tab.number,
                pane_id: tab.root_pane,
                location: terminal.location.clone(),
                focus: *focus,
            }),
            PendingRemoteCreationPlan::Split {
                workspace_id,
                tab_number,
                new_pane_id,
                terminal,
                focus,
                ..
            } => Some(PendingRemoteCreationTarget {
                workspace_id: workspace_id.clone(),
                tab_number: *tab_number,
                pane_id: *new_pane_id,
                location: terminal.location.clone(),
                focus: *focus,
            }),
        }
    }

    fn apply_pending_remote_focus(
        &mut self,
        focus: bool,
        client_view_id: Option<u64>,
        committed: &CommittedRemoteCreation,
    ) {
        if !focus {
            return;
        }
        let Some(view_id) = client_view_id else {
            // Ambient/shared create already applied focus during commit.
            return;
        };
        if self.default_client_view.id() != view_id {
            return;
        }
        self.focus_default_client_view_on_committed(committed);
    }

    fn response_view_for_pending_focus(
        &self,
        focus: bool,
        client_view_id: Option<u64>,
        committed: &CommittedRemoteCreation,
    ) -> Option<ClientViewState> {
        let view_id = client_view_id.filter(|_| focus)?;
        // Encode as the requester would see it after deferred focus application.
        // Do not mutate default_client_view for non-default requesters here.
        let mut view = self
            .default_client_view
            .clone_reconciled(&self.state)
            .clone_for_encode_as(view_id);
        self.focus_view_on_committed(&mut view, committed);
        Some(view)
    }

    fn focus_default_client_view_on_committed(&mut self, committed: &CommittedRemoteCreation) {
        let mut view = self.default_client_view.clone_reconciled(&self.state);
        self.focus_view_on_committed(&mut view, committed);
        self.default_client_view = view;
    }

    fn focus_view_on_committed(
        &self,
        view: &mut ClientViewState,
        committed: &CommittedRemoteCreation,
    ) {
        match *committed {
            CommittedRemoteCreation::Workspace { ws_idx } => {
                if let Some(workspace) = self.state.workspaces.get(ws_idx) {
                    view.active_workspace = Some(ws_idx);
                    view.selected_workspace = ws_idx;
                    if let Some(group_idx) = self
                        .state
                        .groups
                        .iter()
                        .position(|group| group.id == workspace.group_id)
                    {
                        view.active_group = group_idx;
                    }
                    view.mode = Mode::Terminal;
                    view.pending_active_workspace = None;
                    view.reconcile(&self.state);
                }
            }
            CommittedRemoteCreation::Tab { ws_idx, tab_idx } => {
                if let Some(workspace) = self.state.workspaces.get(ws_idx) {
                    if let Some(tab) = workspace.tabs.get(tab_idx) {
                        view.focus_pane_in_workspace(&self.state, ws_idx, tab_idx, tab.root_pane);
                    }
                }
            }
            CommittedRemoteCreation::Split {
                ws_idx,
                tab_idx,
                pane_id,
            } => {
                view.focus_pane_in_workspace(&self.state, ws_idx, tab_idx, pane_id);
            }
        }
    }

    pub(crate) fn configure_pending_remote_agent(
        &mut self,
        terminal_id: &crate::terminal::TerminalId,
        agent_name: Option<String>,
        manual_label: Option<String>,
        launch_argv: Option<Vec<String>>,
    ) -> Option<PendingRemoteCreationTarget> {
        let pending = self.pending_remote_creations.get_mut(terminal_id)?;
        let (terminal, target) = match &mut pending.plan {
            PendingRemoteCreationPlan::Workspace {
                workspace,
                terminal,
                focus,
            } => {
                let tab = workspace.tabs.first()?;
                let location = terminal.location.clone();
                (
                    terminal,
                    PendingRemoteCreationTarget {
                        workspace_id: workspace.id.clone(),
                        tab_number: tab.number,
                        pane_id: tab.root_pane,
                        location,
                        focus: *focus,
                    },
                )
            }
            PendingRemoteCreationPlan::Tab {
                workspace_id,
                tab,
                terminal,
                focus,
            } => {
                let location = terminal.location.clone();
                (
                    terminal,
                    PendingRemoteCreationTarget {
                        workspace_id: workspace_id.clone(),
                        tab_number: tab.number,
                        pane_id: tab.root_pane,
                        location,
                        focus: *focus,
                    },
                )
            }
            PendingRemoteCreationPlan::Split {
                workspace_id,
                tab_number,
                new_pane_id,
                terminal,
                focus,
                ..
            } => {
                let location = terminal.location.clone();
                (
                    terminal,
                    PendingRemoteCreationTarget {
                        workspace_id: workspace_id.clone(),
                        tab_number: *tab_number,
                        pane_id: *new_pane_id,
                        location,
                        focus: *focus,
                    },
                )
            }
        };
        terminal.agent_name = agent_name;
        terminal.manual_label = manual_label;
        terminal.launch_argv = launch_argv;
        Some(target)
    }
}

fn terminal_agent_session_info(
    terminal: &crate::terminal::TerminalState,
) -> Option<crate::api::schema::AgentSessionInfo> {
    if let Some(authority) = terminal.hook_authority.as_ref() {
        if let Some(session_ref) = authority.session_ref.as_ref() {
            return Some(crate::api::schema::AgentSessionInfo {
                source: authority.source.clone(),
                agent: authority.agent_label.clone(),
                kind: crate::api::schema::agent_session_ref_kind_from_resume(session_ref.kind),
                value: session_ref.value.clone(),
            });
        }
    }

    terminal
        .persisted_agent_session
        .as_ref()
        .map(|session| crate::api::schema::AgentSessionInfo {
            source: session.source.clone(),
            agent: session.agent.clone(),
            kind: crate::api::schema::agent_session_ref_kind_from_resume(session.session_ref.kind),
            value: session.session_ref.value.clone(),
        })
}

#[cfg(test)]
mod placement_creation_tests {
    use super::*;

    fn test_app() -> App {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &crate::config::Config::default(),
            true,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.state.workspaces.clear();
        app.state.terminals.clear();
        app.state.active = None;
        app
    }

    fn remote_location(
        host_id: &crate::execution_host::ExecutionHostId,
        path: &str,
    ) -> crate::execution_host::ResourceLocation {
        crate::execution_host::ResourceLocation::new(
            host_id.clone(),
            crate::execution_host::HostPath::new(path).unwrap(),
        )
    }

    fn runtime_identity() -> crate::execution_host::protocol::RuntimeIdentity {
        crate::execution_host::protocol::RuntimeIdentity::new(
            crate::execution_host::protocol::HostBindingGeneration::new(1),
            crate::execution_host::protocol::WorkerInstanceId::new("worker-a").unwrap(),
            crate::execution_host::protocol::WorkerRuntimeId::new("runtime-a").unwrap(),
            crate::execution_host::protocol::RuntimeIncarnation::new(1),
        )
    }

    #[tokio::test]
    async fn remote_workspace_failure_never_commits_provisional_layout() {
        let mut app = test_app();
        let host_id = crate::execution_host::ExecutionHostId::new("ssh:atomic").unwrap();
        app.execution_hosts
            .as_mut()
            .unwrap()
            .connect_test_host(host_id.clone());
        let terminal_id = app
            .begin_remote_workspace(
                remote_location(&host_id, "/srv/requested"),
                true,
                crate::workspace::DEFAULT_GROUP_ID.to_string(),
                None,
                Vec::new(),
            )
            .unwrap();

        assert!(app.state.workspaces.is_empty());
        assert!(app.state.terminals.is_empty());
        assert!(app.pending_remote_creations.contains_key(&terminal_id));

        app.complete_remote_creation_failed(terminal_id.clone(), "worker rejected".into());

        assert!(app.state.workspaces.is_empty());
        assert!(app.state.terminals.is_empty());
        assert!(!app.pending_remote_creations.contains_key(&terminal_id));
        let completions = app.take_remote_creation_completions();
        let [completion] = completions.as_slice() else {
            panic!("expected one failed completion");
        };
        assert!(completion.result.is_err());
    }

    #[tokio::test]
    async fn remote_workspace_commits_resolved_location_only_after_ready() {
        let mut app = test_app();
        let host_id = crate::execution_host::ExecutionHostId::new("ssh:atomic").unwrap();
        app.execution_hosts
            .as_mut()
            .unwrap()
            .connect_test_host(host_id.clone());
        let terminal_id = app
            .begin_remote_workspace(
                remote_location(&host_id, "/srv/requested"),
                true,
                crate::workspace::DEFAULT_GROUP_ID.to_string(),
                None,
                Vec::new(),
            )
            .unwrap();
        let resolved = remote_location(&host_id, "/srv/resolved");

        app.complete_remote_creation_ready(
            terminal_id.clone(),
            runtime_identity(),
            resolved.clone(),
        );

        assert_eq!(app.state.workspaces.len(), 1);
        assert_eq!(app.state.workspaces[0].default_location, resolved);
        assert_eq!(
            app.state.terminals.get(&terminal_id).unwrap().location,
            resolved
        );
        assert!(app.pending_remote_creations.is_empty());
        let completions = app.take_remote_creation_completions();
        let [completion] = completions.as_slice() else {
            panic!("expected one successful completion");
        };
        assert!(matches!(
            &completion.result,
            Ok(CommittedRemoteCreation::Workspace { ws_idx: 0 })
        ));
    }

    #[tokio::test]
    async fn remote_create_send_failure_rolls_back_without_pending() {
        let mut app = test_app();
        let host_id = crate::execution_host::ExecutionHostId::new("ssh:unconnected").unwrap();
        // Host is not connected and has no test worker — create_terminal must fail before
        // any pending_remote_creations entry is recorded.
        let err = app
            .begin_remote_workspace(
                remote_location(&host_id, "/srv/requested"),
                true,
                crate::workspace::DEFAULT_GROUP_ID.to_string(),
                None,
                Vec::new(),
            )
            .expect_err("create against disconnected host must fail");
        assert!(
            err.contains("not connected") || err.contains("unavailable"),
            "unexpected error: {err}"
        );
        assert!(app.pending_remote_creations.is_empty());
        assert!(app.state.workspaces.is_empty());
        assert!(app.remote_creation_completions.is_empty());
    }

    #[tokio::test]
    async fn named_remote_workspace_failure_clears_pending_focus_and_toasts() {
        let mut app = test_app();
        let host_id = crate::execution_host::ExecutionHostId::new("ssh:atomic").unwrap();
        app.execution_hosts
            .as_mut()
            .unwrap()
            .connect_test_host(host_id.clone());
        let location = remote_location(&host_id, "/srv/requested");
        let mut view = app.default_client_view.clone_reconciled(&app.state);
        app.create_named_workspace_for_client_view(&mut view, location, 0, Some("lab".into()));
        let workspace_id = view
            .pending_active_workspace
            .clone()
            .expect("named remote create must mark pending workspace focus");
        app.default_client_view = view;
        assert!(app.state.workspaces.is_empty());
        let terminal_id = app
            .pending_remote_creations
            .keys()
            .next()
            .cloned()
            .expect("pending remote create");

        app.complete_remote_creation_failed(terminal_id, "worker rejected".into());

        assert!(app.state.workspaces.is_empty());
        assert_eq!(
            app.default_client_view.pending_active_workspace.as_deref(),
            None,
            "failed named create must clear the matching pending workspace marker"
        );
        let toast = app.state.toast.expect("failed named create must toast");
        assert_eq!(toast.title, "Workspace create failed");
        assert_eq!(toast.context, "worker rejected");
        assert!(!workspace_id.is_empty());
    }

    #[tokio::test]
    async fn source_deletion_before_ack_fails_once_and_terminates_runtime() {
        let mut app = test_app();
        let host_id = crate::execution_host::ExecutionHostId::new("ssh:atomic").unwrap();
        app.execution_hosts
            .as_mut()
            .unwrap()
            .connect_test_host(host_id.clone());

        // Seed a workspace with a pane that we will delete before split commit.
        let mut seed = Workspace::test_new("seed");
        seed.default_location = remote_location(&host_id, "/srv/seed");
        let target_pane = seed.tabs[0].root_pane;
        let seed_terminal = seed.tabs[0].panes[&target_pane]
            .attached_terminal_id
            .clone();
        app.state.terminals.insert(
            seed_terminal.clone(),
            crate::terminal::TerminalState::new_at(
                seed_terminal,
                remote_location(&host_id, "/srv/seed"),
            ),
        );
        app.state.workspaces.push(seed);

        let terminal_id = app
            .begin_remote_split(
                0,
                target_pane,
                ratatui::layout::Direction::Horizontal,
                None,
                remote_location(&host_id, "/srv/split"),
                true,
                None,
                Vec::new(),
            )
            .unwrap();
        assert!(app.pending_remote_creations.contains_key(&terminal_id));

        // Delete the source pane/workspace before the worker ACK arrives.
        app.state.workspaces.clear();
        app.state.terminals.clear();

        let (respond_to, response_rx) = std::sync::mpsc::channel();
        app.store_pending_remote_api_response(
            terminal_id.clone(),
            crate::app::PendingRemoteApiResponse {
                request_id: "split-source-gone".into(),
                kind: crate::app::PendingRemoteApiKind::PaneSplit,
                respond_to,
                focus: true,
                client_view_id: None,
                pending_focus: None,
            },
        );

        app.complete_remote_creation_ready(
            terminal_id.clone(),
            runtime_identity(),
            remote_location(&host_id, "/srv/split"),
        );
        assert!(app.finish_remote_api_completions());

        let response = response_rx
            .try_recv()
            .expect("source deletion must finish deferred responder once");
        let body: serde_json::Value = serde_json::from_str(&response).expect("json");
        assert_eq!(body["id"], "split-source-gone");
        assert!(body.get("error").is_some(), "expected error body: {body}");
        assert!(response_rx.try_recv().is_err(), "must respond exactly once");
        assert!(!app.pending_remote_api_responses.contains_key(&terminal_id));
        assert!(!app.pending_remote_creations.contains_key(&terminal_id));

        // Worker identity must be termination-pending (not orphaned).
        let hosts = app.execution_hosts.as_ref().unwrap();
        assert!(
            hosts.has_host_references(&host_id),
            "cancelled/failed create with returned identity must retain termination mapping"
        );
    }

    #[tokio::test]
    async fn explicit_disconnect_fails_deferred_create_once() {
        let mut app = test_app();
        let host_id = crate::execution_host::ExecutionHostId::new("ssh:atomic").unwrap();
        app.execution_hosts
            .as_mut()
            .unwrap()
            .connect_test_host(host_id.clone());
        let terminal_id = app
            .begin_remote_workspace(
                remote_location(&host_id, "/srv/requested"),
                true,
                crate::workspace::DEFAULT_GROUP_ID.to_string(),
                None,
                Vec::new(),
            )
            .unwrap();

        let (respond_to, response_rx) = std::sync::mpsc::channel();
        app.store_pending_remote_api_response(
            terminal_id.clone(),
            crate::app::PendingRemoteApiResponse {
                request_id: "disconnect-create".into(),
                kind: crate::app::PendingRemoteApiKind::WorkspaceCreate { label: None },
                respond_to,
                focus: true,
                client_view_id: None,
                pending_focus: None,
            },
        );

        assert!(app.abort_pending_remote_creation(
            terminal_id.clone(),
            format!("execution host {host_id} disconnected before create completed"),
        ));
        assert!(app.finish_remote_api_completions());

        let response = response_rx
            .try_recv()
            .expect("disconnect must finish deferred responder once");
        let body: serde_json::Value = serde_json::from_str(&response).expect("json");
        assert_eq!(body["id"], "disconnect-create");
        assert_eq!(body["error"]["code"], "workspace_create_failed");
        assert!(
            body["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains("disconnected")),
            "unexpected body: {body}"
        );
        assert!(response_rx.try_recv().is_err(), "must respond exactly once");
        assert!(!app.pending_remote_creations.contains_key(&terminal_id));
        // Second abort is a no-op.
        assert!(!app.abort_pending_remote_creation(terminal_id.clone(), "again"));
    }

    #[tokio::test]
    async fn late_ack_after_cancellation_does_not_commit_or_double_respond() {
        let mut app = test_app();
        let host_id = crate::execution_host::ExecutionHostId::new("ssh:atomic").unwrap();
        app.execution_hosts
            .as_mut()
            .unwrap()
            .connect_test_host(host_id.clone());
        let terminal_id = app
            .begin_remote_workspace(
                remote_location(&host_id, "/srv/requested"),
                true,
                crate::workspace::DEFAULT_GROUP_ID.to_string(),
                None,
                Vec::new(),
            )
            .unwrap();

        let (respond_to, response_rx) = std::sync::mpsc::channel();
        app.store_pending_remote_api_response(
            terminal_id.clone(),
            crate::app::PendingRemoteApiResponse {
                request_id: "late-ack".into(),
                kind: crate::app::PendingRemoteApiKind::WorkspaceCreate { label: None },
                respond_to,
                focus: true,
                client_view_id: None,
                pending_focus: None,
            },
        );

        app.complete_remote_creation_failed(terminal_id.clone(), "cancelled".into());
        assert!(app.finish_remote_api_completions());
        let first = response_rx.try_recv().expect("cancel responds once");
        assert!(response_rx.try_recv().is_err());

        // Late ready after cancellation must not commit layout or respond again.
        app.complete_remote_creation_ready(
            terminal_id.clone(),
            runtime_identity(),
            remote_location(&host_id, "/srv/late"),
        );
        assert!(app.state.workspaces.is_empty());
        assert!(app.pending_remote_creations.is_empty());
        assert!(app.take_remote_creation_completions().is_empty());
        assert!(
            response_rx.try_recv().is_err(),
            "no second response after late ACK"
        );
        let _ = first;
    }
}
