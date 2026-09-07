use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::layout::Direction;
use tokio::sync::{mpsc, Notify};

use crate::events::AppEvent;
use crate::layout::PaneId;
#[cfg(test)]
use crate::layout::TileLayout;
use crate::pane::{PaneLaunchEnv, PaneState};
use crate::terminal::{TerminalId, TerminalRuntime, TerminalRuntimeRegistry, TerminalState};

mod aggregate;
mod git;
mod tab;

enum PaneSplitCommand<'a> {
    Argv(&'a [String]),
    Custom(&'a str),
}

pub(crate) use self::aggregate::PaneDetail;
#[cfg(test)]
use self::git::git_ahead_behind;
pub(crate) use self::git::{discover_github_repositories, git_repo_root};
use self::git::{git_work_summary, git_work_summary_for_root as load_git_work_summary_for_root};
pub(crate) use self::tab::MovedPane;
pub use self::{
    git::{
        derive_label_from_cwd, derive_label_from_location, fallback_label_from_cwd, git_branch,
        git_status_cache_key, GitStatusCacheEntry,
    },
    tab::{NewPane, Tab, TabRole},
};

pub const DEFAULT_GROUP_ID: &str = "default";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceGitStatus {
    pub workspace_id: String,
    pub resolved_identity_cwd: PathBuf,
    pub cwd_fingerprint: Vec<PathBuf>,
    pub status_cache_key: PathBuf,
    pub auto_label: String,
    pub branch: Option<String>,
    pub ahead_behind: Option<(usize, usize)>,
    pub work_summary: Option<GitWorkSummary>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GitWorkSummary {
    pub repo_count: usize,
    pub conflicted: usize,
    pub added: usize,
    pub modified: usize,
    pub deleted: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceGitStatusSnapshot {
    pub branch: Option<String>,
    pub ahead_behind: Option<(usize, usize)>,
}

pub(crate) fn discover_workspace_git_identity(
    cwd: &std::path::Path,
    is_local: bool,
) -> (String, PathBuf) {
    if !is_local {
        return (fallback_label_from_cwd(cwd), cwd.to_path_buf());
    }
    let auto_label = derive_label_from_cwd(cwd);
    let status_cache_key = git_status_cache_key(cwd).unwrap_or_else(|| cwd.to_path_buf());
    (auto_label, status_cache_key)
}

impl WorkspaceGitStatusSnapshot {
    pub fn into_workspace_status(
        self,
        workspace_id: String,
        resolved_identity_cwd: PathBuf,
        cwd_fingerprint: Vec<PathBuf>,
        status_cache_key: PathBuf,
        auto_label: String,
    ) -> WorkspaceGitStatus {
        let work_summary = git_work_summary(&cwd_fingerprint);
        WorkspaceGitStatus {
            workspace_id,
            resolved_identity_cwd,
            cwd_fingerprint,
            status_cache_key,
            auto_label,
            branch: self.branch,
            ahead_behind: self.ahead_behind,
            work_summary,
        }
    }
}

static NEXT_WORKSPACE_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn generate_workspace_id() -> String {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros())
        .unwrap_or(0);
    let counter = NEXT_WORKSPACE_ID.fetch_add(1, Ordering::Relaxed);
    format!("w{micros:x}{counter:x}")
}

const PUBLIC_ID_ALPHABET: &[u8; 32] = b"123456789ABCDEFGHJKMNPQRSTVWXYZ0";

pub(crate) fn encode_public_number(mut value: usize) -> String {
    if value == 0 {
        return "0".to_string();
    }

    let mut encoded = Vec::new();
    while value > 0 {
        let digit = (value - 1) % PUBLIC_ID_ALPHABET.len();
        encoded.push(PUBLIC_ID_ALPHABET[digit] as char);
        value = (value - 1) / PUBLIC_ID_ALPHABET.len();
    }
    encoded.iter().rev().collect()
}

pub(crate) fn decode_public_number(value: &str) -> Option<usize> {
    let mut decoded = 0usize;
    for ch in value.chars() {
        let digit = PUBLIC_ID_ALPHABET
            .iter()
            .position(|candidate| *candidate as char == ch)?;
        decoded = decoded
            .checked_mul(PUBLIC_ID_ALPHABET.len())?
            .checked_add(digit + 1)?;
    }
    Some(decoded)
}

pub(crate) fn public_pane_id_for_number(workspace_id: &str, pane_number: usize) -> String {
    format!("{workspace_id}:p{}", encode_public_number(pane_number))
}

pub(crate) fn public_tab_id_for_number(workspace_id: &str, tab_number: usize) -> String {
    format!("{workspace_id}:t{}", encode_public_number(tab_number))
}

/// A named workspace containing tabs.
pub struct Workspace {
    /// Stable public workspace identity, independent of display order.
    pub id: String,
    /// User-provided override. If set, auto-derived identity stops updating.
    pub custom_name: Option<String>,
    /// Sidebar group this workspace belongs to.
    pub group_id: String,
    /// Fallback workspace identity source for tests, old snapshots, or missing runtimes.
    pub identity_cwd: PathBuf,
    /// Durable host-qualified default for future terminals in this workspace.
    pub default_location: crate::execution_host::ResourceLocation,
    /// GitHub repository scope for this Space's companion launches.
    pub github_scope: crate::github::GithubRepositoryScope,
    /// CWD from which the cached automatic label and Git metadata were derived.
    pub(crate) cached_identity_cwd: PathBuf,
    /// Automatic workspace label cached outside the render path.
    pub(crate) cached_auto_label: String,
    /// Cache key for periodic Git status associated with `cached_identity_cwd`.
    pub(crate) cached_git_status_key: PathBuf,
    /// Cached current git branch for the workspace repo.
    pub(crate) cached_git_branch: Option<String>,
    /// Cached ahead/behind counts for the workspace repo's current branch upstream.
    pub(crate) cached_git_ahead_behind: Option<(usize, usize)>,
    /// Cached aggregate git working-tree state across this space's pane cwd set.
    pub(crate) cached_git_work_summary: Option<GitWorkSummary>,
    /// Public pane numbers within this workspace. Closed pane numbers are not reused.
    pub public_pane_numbers: HashMap<PaneId, usize>,
    pub(crate) next_public_pane_number: usize,
    pub(crate) next_public_tab_number: usize,
    pub tabs: Vec<Tab>,
    pub active_tab: usize,
    #[cfg(test)]
    pub(crate) test_runtimes: HashMap<PaneId, TerminalRuntime>,
}

impl Clone for Workspace {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            custom_name: self.custom_name.clone(),
            group_id: self.group_id.clone(),
            identity_cwd: self.identity_cwd.clone(),
            github_scope: self.github_scope.clone(),
            default_location: self.default_location.clone(),
            cached_identity_cwd: self.cached_identity_cwd.clone(),
            cached_auto_label: self.cached_auto_label.clone(),
            cached_git_status_key: self.cached_git_status_key.clone(),
            cached_git_branch: self.cached_git_branch.clone(),
            cached_git_ahead_behind: self.cached_git_ahead_behind,
            cached_git_work_summary: self.cached_git_work_summary,
            public_pane_numbers: self.public_pane_numbers.clone(),
            next_public_pane_number: self.next_public_pane_number,
            next_public_tab_number: self.next_public_tab_number,
            tabs: self.tabs.clone(),
            active_tab: self.active_tab,
            #[cfg(test)]
            test_runtimes: HashMap::new(),
        }
    }
}

enum NewWorkspaceTabCommand<'a> {
    Shell {
        command: &'a str,
        resolved_terminal_theme_override: Option<crate::terminal_theme::ResolvedTerminalTheme>,
        terminal_theme_binding: Option<crate::terminal_theme::TerminalThemeBinding>,
    },
    Profile {
        command: &'a str,
        shell_config: crate::pane::PaneShellConfig<'a>,
    },
}

impl Deref for Workspace {
    type Target = Tab;

    fn deref(&self) -> &Self::Target {
        self.active_tab()
            .expect("workspace must always have at least one active tab")
    }
}

impl DerefMut for Workspace {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.active_tab_mut()
            .expect("workspace must always have at least one active tab")
    }
}

impl Workspace {
    pub(crate) fn from_existing_pane(
        custom_name: Option<String>,
        tab_label: Option<String>,
        identity_cwd: PathBuf,
        default_location: crate::execution_host::ResourceLocation,
        moved: MovedPane,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
    ) -> Self {
        let root_pane = moved.pane_id;
        let tab = Tab::from_existing_pane(1, tab_label, moved, events, render_notify, render_dirty);
        let mut public_pane_numbers = HashMap::new();
        public_pane_numbers.insert(root_pane, 1);
        let (cached_auto_label, cached_git_status_key) =
            discover_workspace_git_identity(&identity_cwd, default_location.is_local());
        Self {
            id: generate_workspace_id(),
            custom_name,
            group_id: DEFAULT_GROUP_ID.to_string(),
            identity_cwd: identity_cwd.clone(),
            github_scope: crate::github::GithubRepositoryScope::default(),
            default_location,
            cached_identity_cwd: identity_cwd,
            cached_auto_label,
            cached_git_status_key,
            cached_git_branch: None,
            cached_git_ahead_behind: None,
            cached_git_work_summary: None,
            public_pane_numbers,
            next_public_pane_number: 2,
            next_public_tab_number: 2,
            tabs: vec![tab],
            active_tab: 0,
            #[cfg(test)]
            test_runtimes: HashMap::new(),
        }
    }

    pub(crate) fn from_remote_tab(
        default_location: crate::execution_host::ResourceLocation,
        tab: Tab,
    ) -> Self {
        let initial_cwd = default_location.path.as_path().to_path_buf();
        let mut public_pane_numbers = HashMap::new();
        public_pane_numbers.insert(tab.root_pane, 1);
        let (cached_auto_label, cached_git_status_key) =
            discover_workspace_git_identity(&initial_cwd, default_location.is_local());
        Self {
            id: generate_workspace_id(),
            custom_name: None,
            group_id: DEFAULT_GROUP_ID.to_string(),
            identity_cwd: initial_cwd.clone(),
            github_scope: crate::github::GithubRepositoryScope::default(),
            default_location,
            cached_identity_cwd: initial_cwd,
            cached_auto_label,
            cached_git_status_key,
            cached_git_branch: None,
            cached_git_ahead_behind: None,
            cached_git_work_summary: None,
            public_pane_numbers,
            next_public_pane_number: 2,
            next_public_tab_number: 2,
            tabs: vec![tab],
            active_tab: 0,
            #[cfg(test)]
            test_runtimes: HashMap::new(),
        }
    }

    pub(crate) fn next_remote_tab_number(&self) -> usize {
        self.next_public_tab_number
    }

    pub(crate) fn add_remote_tab(&mut self, tab: Tab) -> usize {
        self.next_public_tab_number = self
            .next_public_tab_number
            .max(tab.number.saturating_add(1));
        self.register_new_pane(tab.root_pane);
        self.tabs.push(tab);
        self.tabs.len() - 1
    }

    // Test modules construct workspaces through the default constructor; production paths
    // use the env-aware variant so pane identity env is always explicit.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(
        initial_cwd: PathBuf,
        rows: u16,
        cols: u16,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        shell_config: crate::pane::PaneShellConfig<'_>,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
    ) -> std::io::Result<(Self, TerminalState, TerminalRuntime)> {
        Self::new_with_extra_env(
            initial_cwd,
            rows,
            cols,
            scrollback_limit_bytes,
            host_terminal_theme,
            shell_config,
            events,
            render_notify,
            render_dirty,
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_extra_env(
        initial_cwd: PathBuf,
        rows: u16,
        cols: u16,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        shell_config: crate::pane::PaneShellConfig<'_>,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
        extra_env: Vec<(String, String)>,
    ) -> std::io::Result<(Self, TerminalState, TerminalRuntime)> {
        Self::new_with_tab(
            initial_cwd,
            rows,
            cols,
            scrollback_limit_bytes,
            host_terminal_theme,
            shell_config,
            events,
            render_notify,
            render_dirty,
            None,
            extra_env,
        )
    }

    // Kept for tests that do not need launch-env customization.
    #[allow(dead_code)]
    pub fn new_argv_command(
        initial_cwd: PathBuf,
        rows: u16,
        cols: u16,
        argv: &[String],
        extra_env: &[(String, String)],
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
    ) -> std::io::Result<(Self, TerminalState, TerminalRuntime)> {
        Self::new_argv_command_with_extra_env(
            initial_cwd,
            rows,
            cols,
            argv,
            scrollback_limit_bytes,
            host_terminal_theme,
            events,
            render_notify,
            render_dirty,
            extra_env.to_vec(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_argv_command_with_extra_env(
        initial_cwd: PathBuf,
        rows: u16,
        cols: u16,
        argv: &[String],
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
        extra_env: Vec<(String, String)>,
    ) -> std::io::Result<(Self, TerminalState, TerminalRuntime)> {
        Self::new_with_tab(
            initial_cwd,
            rows,
            cols,
            scrollback_limit_bytes,
            host_terminal_theme,
            crate::pane::PaneShellConfig::new("", crate::config::ShellModeConfig::NonLogin),
            events,
            render_notify,
            render_dirty,
            Some(argv),
            extra_env,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_with_tab(
        initial_cwd: PathBuf,
        rows: u16,
        cols: u16,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        shell_config: crate::pane::PaneShellConfig<'_>,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
        argv: Option<&[String]>,
        extra_env: Vec<(String, String)>,
    ) -> std::io::Result<(Self, TerminalState, TerminalRuntime)> {
        let id = generate_workspace_id();
        let launch_env = PaneLaunchEnv::from_extra(extra_env).with_identity(
            id.clone(),
            public_tab_id_for_number(&id, 1),
            public_pane_id_for_number(&id, 1),
        );
        let (tab, terminal, runtime) = if let Some(argv) = argv {
            Tab::new_argv_command(
                1,
                initial_cwd.clone(),
                rows,
                cols,
                argv,
                scrollback_limit_bytes,
                host_terminal_theme,
                &launch_env,
                events,
                render_notify,
                render_dirty,
            )?
        } else {
            Tab::new(
                1,
                initial_cwd.clone(),
                rows,
                cols,
                scrollback_limit_bytes,
                host_terminal_theme,
                shell_config,
                &launch_env,
                events,
                render_notify,
                render_dirty,
            )?
        };
        let mut public_pane_numbers = HashMap::new();
        public_pane_numbers.insert(tab.root_pane, 1);
        let (cached_auto_label, cached_git_status_key) =
            discover_workspace_git_identity(&initial_cwd, terminal.location.is_local());
        Ok((
            Self {
                id,
                custom_name: None,
                group_id: DEFAULT_GROUP_ID.to_string(),
                identity_cwd: initial_cwd.clone(),
                github_scope: crate::github::GithubRepositoryScope::default(),
                default_location: terminal.location.clone(),
                cached_identity_cwd: initial_cwd.clone(),
                cached_auto_label,
                cached_git_status_key,
                cached_git_branch: git_branch(&initial_cwd),
                cached_git_ahead_behind: None,
                cached_git_work_summary: None,
                public_pane_numbers,
                next_public_pane_number: 2,
                next_public_tab_number: 2,
                tabs: vec![tab],
                active_tab: 0,
                #[cfg(test)]
                test_runtimes: HashMap::new(),
            },
            terminal,
            runtime,
        ))
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active_tab)
    }

    pub fn active_tab_index(&self) -> usize {
        self.active_tab
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active_tab)
    }

    pub fn active_tab_display_name(&self) -> Option<String> {
        self.tab_display_name(self.active_tab)
    }

    pub fn tab_display_name(&self, tab_idx: usize) -> Option<String> {
        let tab = self.tabs.get(tab_idx)?;
        Some(
            tab.custom_name
                .clone()
                .unwrap_or_else(|| (tab_idx + 1).to_string()),
        )
    }

    pub fn switch_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.active_tab = idx;
            if let Some(tab) = self.tabs.get_mut(idx) {
                for pane in tab.panes.values_mut() {
                    pane.seen = true;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_tab_with_handles(
        &mut self,
        rows: u16,
        cols: u16,
        cwd: PathBuf,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        shell_config: crate::pane::PaneShellConfig<'_>,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
    ) -> std::io::Result<(usize, TerminalState, TerminalRuntime)> {
        self.create_tab_with_handles_and_env(
            rows,
            cols,
            cwd,
            scrollback_limit_bytes,
            host_terminal_theme,
            shell_config,
            Vec::new(),
            events,
            render_notify,
            render_dirty,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_tab_with_handles_and_env(
        &mut self,
        rows: u16,
        cols: u16,
        cwd: PathBuf,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        shell_config: crate::pane::PaneShellConfig<'_>,
        extra_env: Vec<(String, String)>,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
    ) -> std::io::Result<(usize, TerminalState, TerminalRuntime)> {
        self.create_tab_with_runtime(
            rows,
            cols,
            cwd,
            scrollback_limit_bytes,
            host_terminal_theme,
            shell_config,
            None,
            None,
            extra_env,
            Some(events),
            Some(render_notify),
            Some(render_dirty),
        )
    }

    pub fn create_tab_argv_command(
        &mut self,
        rows: u16,
        cols: u16,
        cwd: PathBuf,
        argv: &[String],
        extra_env: Vec<(String, String)>,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
    ) -> std::io::Result<(usize, TerminalState, TerminalRuntime)> {
        self.create_tab_with_runtime(
            rows,
            cols,
            cwd,
            scrollback_limit_bytes,
            host_terminal_theme,
            crate::pane::PaneShellConfig::new("", crate::config::ShellModeConfig::NonLogin),
            None,
            Some(argv),
            extra_env,
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_command_tab(
        &mut self,
        rows: u16,
        cols: u16,
        cwd: PathBuf,
        command: &str,
        extra_env: &[(String, String)],
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        terminal_theme_binding: Option<crate::terminal_theme::TerminalThemeBinding>,
        resolved_terminal_theme_override: Option<crate::terminal_theme::ResolvedTerminalTheme>,
    ) -> std::io::Result<(usize, TerminalState, TerminalRuntime)> {
        self.create_tab_with_runtime(
            rows,
            cols,
            cwd,
            scrollback_limit_bytes,
            host_terminal_theme,
            crate::pane::PaneShellConfig::new("", crate::config::ShellModeConfig::NonLogin),
            Some(NewWorkspaceTabCommand::Shell {
                command,
                terminal_theme_binding,
                resolved_terminal_theme_override,
            }),
            None,
            extra_env.to_vec(),
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn create_profile_command_tab(
        &mut self,
        rows: u16,
        cols: u16,
        cwd: PathBuf,
        shell_config: crate::pane::PaneShellConfig<'_>,
        command: &str,
        extra_env: &[(String, String)],
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
    ) -> std::io::Result<(usize, TerminalState, TerminalRuntime)> {
        self.create_tab_with_runtime(
            rows,
            cols,
            cwd,
            scrollback_limit_bytes,
            host_terminal_theme,
            crate::pane::PaneShellConfig::new("", crate::config::ShellModeConfig::NonLogin),
            Some(NewWorkspaceTabCommand::Profile {
                command,
                shell_config,
            }),
            None,
            extra_env.to_vec(),
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn create_tab_with_runtime(
        &mut self,
        rows: u16,
        cols: u16,
        cwd: PathBuf,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        shell_config: crate::pane::PaneShellConfig<'_>,
        command: Option<NewWorkspaceTabCommand<'_>>,
        argv: Option<&[String]>,
        extra_env: Vec<(String, String)>,
        fallback_events: Option<mpsc::Sender<AppEvent>>,
        fallback_render_notify: Option<Arc<Notify>>,
        fallback_render_dirty: Option<Arc<crate::render_signal::RenderSignal>>,
    ) -> std::io::Result<(usize, TerminalState, TerminalRuntime)> {
        let number = self.next_public_tab_number;
        let Some((events, render_notify, render_dirty)) = self
            .active_tab()
            .map(|tab| {
                (
                    tab.events.clone(),
                    tab.render_notify.clone(),
                    tab.render_dirty.clone(),
                )
            })
            .or_else(|| {
                Some((
                    fallback_events?,
                    fallback_render_notify?,
                    fallback_render_dirty?,
                ))
            })
        else {
            return Err(std::io::Error::other(
                "cannot create tab in empty workspace without runtime handles",
            ));
        };
        self.next_public_tab_number += 1;
        let pane_number = self.next_public_pane_number;
        let launch_env = self.launch_env_for_new_pane(number, pane_number, extra_env);

        let (tab, terminal, runtime) = if let Some(command) = command {
            match command {
                NewWorkspaceTabCommand::Shell {
                    command,
                    resolved_terminal_theme_override,
                    terminal_theme_binding,
                } => Tab::new_shell_command(
                    number,
                    cwd,
                    rows,
                    cols,
                    command,
                    &launch_env,
                    scrollback_limit_bytes,
                    host_terminal_theme,
                    resolved_terminal_theme_override,
                    terminal_theme_binding,
                    events,
                    render_notify,
                    render_dirty,
                )?,
                NewWorkspaceTabCommand::Profile {
                    command,
                    shell_config,
                } => Tab::new_profile_command(
                    number,
                    cwd,
                    rows,
                    cols,
                    shell_config,
                    command,
                    &launch_env,
                    scrollback_limit_bytes,
                    host_terminal_theme,
                    events,
                    render_notify,
                    render_dirty,
                )?,
            }
        } else if let Some(argv) = argv {
            Tab::new_argv_command(
                number,
                cwd,
                rows,
                cols,
                argv,
                scrollback_limit_bytes,
                host_terminal_theme,
                &launch_env,
                events,
                render_notify,
                render_dirty,
            )?
        } else {
            Tab::new(
                number,
                cwd,
                rows,
                cols,
                scrollback_limit_bytes,
                host_terminal_theme,
                shell_config,
                &launch_env,
                events,
                render_notify,
                render_dirty,
            )?
        };
        self.register_new_pane(tab.root_pane);
        self.tabs.push(tab);
        Ok((self.tabs.len() - 1, terminal, runtime))
    }

    pub fn close_tab(&mut self, idx: usize) -> bool {
        if self.tabs.len() <= 1 || idx >= self.tabs.len() {
            return false;
        }
        self.close_tab_allow_empty(idx)
    }

    pub fn close_tab_allow_empty(&mut self, idx: usize) -> bool {
        if idx >= self.tabs.len() {
            return false;
        }
        let tab = self.tabs.remove(idx);
        for pane_id in tab.panes.keys() {
            self.unregister_pane(*pane_id);
        }
        if self.tabs.is_empty() {
            self.active_tab = 0;
        } else if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if idx <= self.active_tab && self.active_tab > 0 {
            self.active_tab -= 1;
        }
        true
    }

    pub fn move_tab(&mut self, source_idx: usize, insert_idx: usize) -> bool {
        if source_idx >= self.tabs.len() || insert_idx > self.tabs.len() {
            return false;
        }

        let target_idx = if source_idx < insert_idx {
            insert_idx.saturating_sub(1)
        } else {
            insert_idx
        }
        .min(self.tabs.len().saturating_sub(1));

        if source_idx == target_idx {
            return false;
        }

        let active_root_pane = self.tabs.get(self.active_tab).map(|tab| tab.root_pane);
        let tab = self.tabs.remove(source_idx);
        self.tabs.insert(target_idx, tab);
        self.active_tab = active_root_pane
            .and_then(|root_pane| self.tabs.iter().position(|tab| tab.root_pane == root_pane))
            .unwrap_or(target_idx);
        true
    }

    pub fn close_active_tab(&mut self) -> bool {
        self.close_tab(self.active_tab)
    }

    pub fn split_focused(
        &mut self,
        direction: Direction,
        rows: u16,
        cols: u16,
        cwd: Option<PathBuf>,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        shell_config: crate::pane::PaneShellConfig<'_>,
        extra_env: Vec<(String, String)>,
    ) -> std::io::Result<crate::workspace::tab::NewPane> {
        let pane_id = self
            .active_tab()
            .map(|tab| tab.layout.focused())
            .expect("workspace must always have at least one tab");
        self.split_pane(
            pane_id,
            direction,
            rows,
            cols,
            cwd,
            scrollback_limit_bytes,
            host_terminal_theme,
            shell_config,
            extra_env,
            true,
        )
        .expect("active tab pane is in the workspace")
        .map(|(_tab_idx, new_pane)| new_pane)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn commit_remote_split(
        &mut self,
        target_pane_id: PaneId,
        new_pane_id: PaneId,
        direction: Direction,
        ratio: Option<f32>,
        focus_new_pane: bool,
        terminal: TerminalState,
        runtime: TerminalRuntime,
    ) -> Option<(usize, crate::workspace::tab::NewPane)> {
        let tab_idx = self.find_tab_index_for_pane(target_pane_id)?;
        let tab = &mut self.tabs[tab_idx];
        if !tab.layout.insert_pane_near(
            target_pane_id,
            new_pane_id,
            direction,
            ratio.unwrap_or(0.5),
            focus_new_pane,
        ) {
            return None;
        }
        tab.panes.insert(
            new_pane_id,
            PaneState::new_with_env_pane_id(terminal.id.clone(), new_pane_id),
        );
        tab.zoomed = false;
        self.register_new_pane(new_pane_id);
        Some((
            tab_idx,
            crate::workspace::tab::NewPane {
                pane_id: new_pane_id,
                terminal,
                runtime,
            },
        ))
    }

    pub fn split_pane(
        &mut self,
        pane_id: PaneId,
        direction: Direction,
        rows: u16,
        cols: u16,
        cwd: Option<PathBuf>,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        shell_config: crate::pane::PaneShellConfig<'_>,
        extra_env: Vec<(String, String)>,
        focus_new_pane: bool,
    ) -> Option<std::io::Result<(usize, crate::workspace::tab::NewPane)>> {
        self.split_pane_with_runtime(
            pane_id,
            direction,
            None,
            rows,
            cols,
            cwd,
            scrollback_limit_bytes,
            host_terminal_theme,
            shell_config,
            extra_env,
            focus_new_pane,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn split_pane_with_ratio(
        &mut self,
        pane_id: PaneId,
        direction: Direction,
        ratio: f32,
        rows: u16,
        cols: u16,
        cwd: Option<PathBuf>,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        shell_config: crate::pane::PaneShellConfig<'_>,
        extra_env: Vec<(String, String)>,
        focus_new_pane: bool,
    ) -> Option<std::io::Result<(usize, crate::workspace::tab::NewPane)>> {
        self.split_pane_with_runtime(
            pane_id,
            direction,
            Some(ratio),
            rows,
            cols,
            cwd,
            scrollback_limit_bytes,
            host_terminal_theme,
            shell_config,
            extra_env,
            focus_new_pane,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn split_pane_custom_command(
        &mut self,
        pane_id: PaneId,
        direction: Direction,
        rows: u16,
        cols: u16,
        cwd: Option<PathBuf>,
        command: &str,
        extra_env: Vec<(String, String)>,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        focus_new_pane: bool,
    ) -> Option<std::io::Result<(usize, crate::workspace::tab::NewPane)>> {
        self.split_pane_with_runtime(
            pane_id,
            direction,
            None,
            rows,
            cols,
            cwd,
            scrollback_limit_bytes,
            host_terminal_theme,
            crate::pane::PaneShellConfig::new("", crate::config::ShellModeConfig::NonLogin),
            extra_env,
            focus_new_pane,
            Some(PaneSplitCommand::Custom(command)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn split_pane_argv_command(
        &mut self,
        pane_id: PaneId,
        direction: Direction,
        rows: u16,
        cols: u16,
        cwd: Option<PathBuf>,
        argv: &[String],
        extra_env: Vec<(String, String)>,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        focus_new_pane: bool,
    ) -> Option<std::io::Result<(usize, crate::workspace::tab::NewPane)>> {
        self.split_pane_with_runtime(
            pane_id,
            direction,
            None,
            rows,
            cols,
            cwd,
            scrollback_limit_bytes,
            host_terminal_theme,
            crate::pane::PaneShellConfig::new("", crate::config::ShellModeConfig::NonLogin),
            extra_env,
            focus_new_pane,
            Some(PaneSplitCommand::Argv(argv)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn split_pane_argv_command_with_ratio(
        &mut self,
        pane_id: PaneId,
        direction: Direction,
        ratio: f32,
        rows: u16,
        cols: u16,
        cwd: Option<PathBuf>,
        argv: &[String],
        extra_env: Vec<(String, String)>,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        focus_new_pane: bool,
    ) -> Option<std::io::Result<(usize, crate::workspace::tab::NewPane)>> {
        self.split_pane_with_runtime(
            pane_id,
            direction,
            Some(ratio),
            rows,
            cols,
            cwd,
            scrollback_limit_bytes,
            host_terminal_theme,
            crate::pane::PaneShellConfig::new("", crate::config::ShellModeConfig::NonLogin),
            extra_env,
            focus_new_pane,
            Some(PaneSplitCommand::Argv(argv)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn split_pane_with_runtime(
        &mut self,
        pane_id: PaneId,
        direction: Direction,
        ratio: Option<f32>,
        rows: u16,
        cols: u16,
        cwd: Option<PathBuf>,
        scrollback_limit_bytes: usize,
        host_terminal_theme: crate::terminal_theme::TerminalTheme,
        shell_config: crate::pane::PaneShellConfig<'_>,
        extra_env: Vec<(String, String)>,
        focus_new_pane: bool,
        command: Option<PaneSplitCommand<'_>>,
    ) -> Option<std::io::Result<(usize, crate::workspace::tab::NewPane)>> {
        let tab_idx = self.find_tab_index_for_pane(pane_id)?;
        let pane_number = self.next_public_pane_number;
        let tab_number = self.tabs[tab_idx].number;
        let launch_env = self.launch_env_for_new_pane(tab_number, pane_number, extra_env);
        let tab = &mut self.tabs[tab_idx];
        let new_pane_result = match command {
            Some(PaneSplitCommand::Argv(argv)) => tab.split_pane_argv(
                pane_id,
                focus_new_pane,
                direction,
                ratio,
                rows,
                cols,
                cwd,
                argv,
                &launch_env,
                scrollback_limit_bytes,
                host_terminal_theme,
            ),
            Some(PaneSplitCommand::Custom(command)) => {
                debug_assert!(ratio.is_none(), "custom command splits do not use ratios");
                tab.split_pane_custom(
                    pane_id,
                    focus_new_pane,
                    direction,
                    rows,
                    cols,
                    cwd,
                    command,
                    &launch_env,
                    scrollback_limit_bytes,
                    host_terminal_theme,
                )
            }
            None => tab.split_pane_shell(
                pane_id,
                focus_new_pane,
                direction,
                ratio,
                rows,
                cols,
                cwd,
                scrollback_limit_bytes,
                host_terminal_theme,
                shell_config,
                &launch_env,
            ),
        };
        let new_pane = match new_pane_result {
            Ok(new_pane) => new_pane,
            Err(err) => return Some(Err(err)),
        };
        self.register_new_pane(new_pane.pane_id);
        Some(Ok((tab_idx, new_pane)))
    }

    pub(crate) fn take_pane_for_move(&mut self, pane_id: PaneId) -> Option<TakenPane> {
        let tab_idx = self.find_tab_index_for_pane(pane_id)?;
        let pane_count = self.tabs[tab_idx].layout.pane_count();
        if pane_count <= 1 {
            let mut tab = self.tabs.remove(tab_idx);
            let moved = tab.take_pane_for_move(pane_id)?;
            if self.tabs.is_empty() {
                self.active_tab = 0;
            } else if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            } else if tab_idx <= self.active_tab && self.active_tab > 0 {
                self.active_tab -= 1;
            }
            return Some(TakenPane {
                moved,
                removed_tab_idx: Some(tab_idx),
                workspace_empty: self.tabs.is_empty(),
            });
        }

        let moved = self.tabs[tab_idx].take_pane_for_move(pane_id)?;
        Some(TakenPane {
            moved,
            removed_tab_idx: None,
            workspace_empty: false,
        })
    }

    // Failed insertion returns the moved pane so callers can restore it losslessly.
    #[allow(clippy::result_large_err)]
    pub(crate) fn insert_moved_pane_into_tab(
        &mut self,
        tab_idx: usize,
        target_pane_id: PaneId,
        moved: MovedPane,
        direction: Direction,
        ratio: f32,
        focus: bool,
    ) -> Result<PaneId, MovedPane> {
        let Some(tab) = self.tabs.get_mut(tab_idx) else {
            return Err(moved);
        };
        let pane_id = tab.insert_moved_pane_near(target_pane_id, moved, direction, ratio, focus)?;
        self.register_moved_pane(pane_id);
        Ok(pane_id)
    }

    pub(crate) fn create_tab_from_existing_pane(
        &mut self,
        moved: MovedPane,
        label: Option<String>,
        events: mpsc::Sender<AppEvent>,
        render_notify: Arc<Notify>,
        render_dirty: Arc<crate::render_signal::RenderSignal>,
    ) -> usize {
        let pane_id = moved.pane_id;
        let number = self.next_public_tab_number;
        self.next_public_tab_number += 1;
        let tab =
            Tab::from_existing_pane(number, label, moved, events, render_notify, render_dirty);
        self.register_moved_pane(pane_id);
        self.tabs.push(tab);
        self.tabs.len() - 1
    }

    /// Close the focused pane. Returns true if the workspace should close.
    pub fn close_focused(&mut self) -> bool {
        let pane_count = self
            .active_tab()
            .map(|tab| tab.layout.pane_count())
            .unwrap_or(0);
        let tab_count = self.tabs.len();
        if pane_count <= 1 {
            return tab_count <= 1 || self.close_active_tab_and_report();
        }

        if let Some((removed, _terminal_id)) = self.active_tab_mut().and_then(Tab::close_focused) {
            self.unregister_pane(removed);
        }
        false
    }

    /// Remove a specific pane from this workspace without terminating its runtime.
    /// Returns true if the workspace should close.
    pub fn remove_pane(&mut self, pane_id: PaneId) -> bool {
        let Some(tab_idx) = self.find_tab_index_for_pane(pane_id) else {
            return false;
        };
        let pane_count = self.tabs[tab_idx].layout.pane_count();
        let tab_count = self.tabs.len();
        if pane_count <= 1 {
            if tab_count <= 1 {
                return true;
            }
            self.tabs.remove(tab_idx);
            self.unregister_pane(pane_id);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            } else if tab_idx <= self.active_tab && self.active_tab > 0 {
                self.active_tab -= 1;
            }
            return false;
        }

        if let Some((removed, _terminal_id)) = self.tabs[tab_idx].remove_pane(pane_id) {
            self.unregister_pane(removed);
        }
        false
    }

    pub fn public_pane_number(&self, pane_id: PaneId) -> Option<usize> {
        self.public_pane_numbers.get(&pane_id).copied()
    }

    pub fn pane_display_number(&self, pane_id: PaneId) -> Option<usize> {
        let tab_idx = self.find_tab_index_for_pane(pane_id)?;
        self.tabs.get(tab_idx)?.layout.pane_ordinal(pane_id)
    }

    fn launch_env_for_new_pane(
        &self,
        tab_number: usize,
        pane_number: usize,
        extra_env: Vec<(String, String)>,
    ) -> PaneLaunchEnv {
        PaneLaunchEnv::from_extra(extra_env).with_identity(
            self.id.clone(),
            public_tab_id_for_number(&self.id, tab_number),
            public_pane_id_for_number(&self.id, pane_number),
        )
    }

    pub fn public_tab_number(&self, tab_idx: usize) -> Option<usize> {
        self.tabs.get(tab_idx).map(|tab| tab.number)
    }

    #[cfg(test)]
    pub fn public_tab_number_for_pane(&self, pane_id: PaneId) -> Option<usize> {
        let tab_idx = self.find_tab_index_for_pane(pane_id)?;
        self.public_tab_number(tab_idx)
    }

    pub fn set_custom_name(&mut self, name: String) {
        self.custom_name = Some(name);
    }

    pub fn effective_default_cwd_from(
        &self,
        terminals: &HashMap<TerminalId, TerminalState>,
        terminal_runtimes: &TerminalRuntimeRegistry,
    ) -> PathBuf {
        self.active_tab()
            .and_then(|tab| tab.cwd_for_pane(tab.layout.focused(), terminals, terminal_runtimes))
            .unwrap_or_else(|| self.default_location.path.as_path().to_path_buf())
    }

    pub fn record_default_location(
        &mut self,
        location: crate::execution_host::ResourceLocation,
    ) -> bool {
        if self.default_location == location {
            return false;
        }
        self.default_location = location;
        true
    }

    pub fn resolved_identity_cwd_from(
        &self,
        terminals: &HashMap<TerminalId, TerminalState>,
        terminal_runtimes: &TerminalRuntimeRegistry,
    ) -> Option<PathBuf> {
        self.resolved_identity_location_from(terminals, terminal_runtimes)
            .map(|location| location.path.as_path().to_path_buf())
    }

    /// Resolve one atomic host+path identity for the focused terminal, falling
    /// back to the workspace default location. Host and path always come from
    /// the same source — never pair a focused path with the default host.
    pub fn resolved_identity_location_from(
        &self,
        terminals: &HashMap<TerminalId, TerminalState>,
        terminal_runtimes: &TerminalRuntimeRegistry,
    ) -> Option<crate::execution_host::ResourceLocation> {
        if let Some(tab) = self.active_tab() {
            let pane_id = tab.layout.focused();
            if let Some(terminal_id) = tab.terminal_id(pane_id) {
                if let Some(terminal) = terminals.get(terminal_id) {
                    let path = terminal_runtimes
                        .get(terminal_id)
                        .and_then(TerminalRuntime::cwd)
                        .unwrap_or_else(|| terminal.cwd.clone());
                    let mut location = terminal.location.clone();
                    if let Ok(host_path) = crate::execution_host::HostPath::new(path) {
                        location.path = host_path;
                    }
                    return Some(location);
                }
            }
        }
        Some(self.default_location.clone())
    }

    /// Seed coordinator-local git branch cache from the atomic identity
    /// location. Remote locations stay empty for worker observation refresh.
    pub fn seed_cached_git_branch_from(
        &mut self,
        terminals: &HashMap<TerminalId, TerminalState>,
        terminal_runtimes: &TerminalRuntimeRegistry,
    ) {
        self.cached_git_branch = self
            .resolved_identity_location_from(terminals, terminal_runtimes)
            .filter(|location| location.is_local())
            .as_ref()
            .and_then(|location| git_branch(location.path.as_path()));
    }

    pub fn display_name(&self) -> String {
        if let Some(name) = &self.custom_name {
            return name.clone();
        }

        self.automatic_display_name_for_cwd(&self.identity_cwd)
    }

    pub fn display_name_from(
        &self,
        terminals: &HashMap<TerminalId, TerminalState>,
        terminal_runtimes: &TerminalRuntimeRegistry,
    ) -> String {
        if let Some(name) = &self.custom_name {
            return name.clone();
        }

        self.resolved_identity_location_from(terminals, terminal_runtimes)
            .map(|location| {
                if location.is_local() {
                    self.automatic_display_name_for_cwd(location.path.as_path())
                } else {
                    derive_label_from_location(&location)
                }
            })
            .unwrap_or_else(|| "workspace".into())
    }

    fn automatic_display_name_for_cwd(&self, cwd: &std::path::Path) -> String {
        if cwd == self.cached_identity_cwd {
            self.cached_auto_label.clone()
        } else {
            fallback_label_from_cwd(cwd)
        }
    }

    #[cfg(test)]
    pub fn branch(&self) -> Option<String> {
        self.cached_git_branch.clone()
    }

    #[cfg(test)]
    pub fn git_ahead_behind(&self) -> Option<(usize, usize)> {
        self.cached_git_ahead_behind
    }

    #[cfg(test)]
    pub fn git_work_summary_label(&self) -> String {
        let Some(summary) = self.cached_git_work_summary else {
            return String::new();
        };

        let mut parts = Vec::new();
        if summary.conflicted > 0 {
            parts.push(format!("!{}", summary.conflicted));
        }
        if summary.added > 0 {
            parts.push(format!("+{}", summary.added));
        }
        if summary.modified > 0 {
            parts.push(format!("~{}", summary.modified));
        }
        if summary.deleted > 0 {
            parts.push(format!("-{}", summary.deleted));
        }

        let state = if parts.is_empty() {
            String::new()
        } else {
            parts.join(" ")
        };

        if summary.repo_count > 1 {
            if state.is_empty() {
                format!("{} repos", summary.repo_count)
            } else {
                format!("{} repos · {state}", summary.repo_count)
            }
        } else {
            state
        }
    }

    pub fn git_work_summary_for_root(root: &std::path::Path) -> Option<GitWorkSummary> {
        load_git_work_summary_for_root(root)
    }

    #[cfg(test)]
    pub fn refresh_git_ahead_behind(&mut self) {
        let cwd = &self.identity_cwd;
        self.cached_git_branch = git_branch(cwd);
        self.cached_git_ahead_behind = git_ahead_behind(cwd);
        self.cached_git_work_summary = git_work_summary(&self.git_status_cwds());
    }

    #[cfg(test)]
    pub fn git_status_cwds(&self) -> Vec<PathBuf> {
        let mut cwds = self
            .tabs
            .iter()
            .flat_map(|tab| {
                tab.layout.pane_ids().into_iter().filter_map(|id| {
                    self.test_runtimes
                        .get(&id)
                        .and_then(TerminalRuntime::cwd)
                        .or_else(|| tab.runtimes.get(&id).and_then(TerminalRuntime::cwd))
                })
            })
            .collect::<Vec<_>>();
        cwds.sort();
        cwds.dedup();
        if cwds.is_empty() && self.default_location.is_local() {
            cwds.push(self.default_location.path.as_path().to_path_buf());
        }
        cwds
    }

    pub fn git_status_cwds_from(
        &self,
        terminals: &HashMap<TerminalId, TerminalState>,
        terminal_runtimes: &TerminalRuntimeRegistry,
    ) -> Vec<PathBuf> {
        let mut cwds = self
            .tabs
            .iter()
            .flat_map(|tab| {
                tab.layout
                    .pane_ids()
                    .into_iter()
                    .filter_map(|id| tab.cwd_for_pane(id, terminals, terminal_runtimes))
            })
            .collect::<Vec<_>>();
        cwds.sort();
        cwds.dedup();
        if cwds.is_empty() && self.default_location.is_local() {
            cwds.push(self.default_location.path.as_path().to_path_buf());
        }
        cwds
    }

    pub fn git_status_snapshot_for_cwd_with_cache(
        resolved_identity_cwd: &std::path::Path,
        cached: Option<&GitStatusCacheEntry>,
    ) -> (WorkspaceGitStatusSnapshot, Option<GitStatusCacheEntry>) {
        self::git::git_status_snapshot_for_cwd(resolved_identity_cwd, cached)
    }

    pub fn find_tab_index_for_pane(&self, pane_id: PaneId) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.panes.contains_key(&pane_id))
    }

    pub fn pane_state(&self, pane_id: PaneId) -> Option<&PaneState> {
        self.tabs.iter().find_map(|tab| tab.panes.get(&pane_id))
    }

    pub fn pane_state_mut(&mut self, pane_id: PaneId) -> Option<&mut PaneState> {
        self.tabs
            .iter_mut()
            .find_map(|tab| tab.panes.get_mut(&pane_id))
    }

    pub fn terminal_id(&self, pane_id: PaneId) -> Option<&TerminalId> {
        self.tabs.iter().find_map(|tab| tab.terminal_id(pane_id))
    }

    pub fn focused_pane_id(&self) -> Option<PaneId> {
        self.active_tab().map(|tab| tab.layout.focused())
    }

    pub fn close_pane(&mut self, pane_id: PaneId) -> bool {
        let tab_idx = match self.find_tab_index_for_pane(pane_id) {
            Some(idx) => idx,
            None => return false,
        };
        let pane_count = self.tabs[tab_idx].layout.pane_count();
        let tab_count = self.tabs.len();
        if pane_count <= 1 {
            if tab_count <= 1 {
                return true;
            }
            self.tabs.remove(tab_idx);
            self.unregister_pane(pane_id);
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            } else if tab_idx <= self.active_tab && self.active_tab > 0 {
                self.active_tab -= 1;
            }
            return false;
        }

        if let Some((removed, _terminal_id)) = self.tabs[tab_idx].close_pane(pane_id) {
            self.unregister_pane(removed);
        }
        false
    }

    fn register_new_pane(&mut self, pane_id: PaneId) {
        self.register_new_pane_with_number(pane_id, self.next_public_pane_number);
    }

    fn register_moved_pane(&mut self, pane_id: PaneId) {
        if !self.public_pane_numbers.contains_key(&pane_id) {
            self.register_new_pane(pane_id);
        }
    }

    pub(crate) fn unregister_moved_pane(&mut self, pane_id: PaneId) {
        self.unregister_pane(pane_id);
    }

    fn register_new_pane_with_number(&mut self, pane_id: PaneId, number: usize) {
        self.public_pane_numbers.insert(pane_id, number);
        self.next_public_pane_number = self.next_public_pane_number.max(number + 1);
    }

    fn unregister_pane(&mut self, pane_id: PaneId) {
        self.public_pane_numbers.remove(&pane_id);
    }

    fn close_active_tab_and_report(&mut self) -> bool {
        if self.tabs.len() <= 1 {
            return true;
        }
        self.close_active_tab();
        false
    }
}

#[cfg(test)]
impl Workspace {
    pub(crate) fn test_new(name: &str) -> Self {
        let (events, _) = mpsc::channel(64);
        let render_notify = Arc::new(Notify::new());
        let render_dirty = Arc::new(crate::render_signal::RenderSignal::new());
        let identity_cwd = std::env::current_dir().unwrap_or_else(|_| "/".into());
        let (layout, root_id) = TileLayout::new();
        let terminal_id = TerminalId::alloc();
        let mut panes = HashMap::new();
        panes.insert(
            root_id,
            PaneState::new_with_env_pane_id(terminal_id, root_id),
        );
        let tab = Tab {
            custom_name: None,
            number: 1,
            role: TabRole::Terminal,
            root_pane: root_id,
            layout,
            panes,
            runtimes: HashMap::new(),
            zoomed: false,
            events,
            render_notify,
            render_dirty,
        };
        let mut public_pane_numbers = HashMap::new();
        public_pane_numbers.insert(tab.root_pane, 1);
        let (cached_auto_label, cached_git_status_key) =
            discover_workspace_git_identity(&identity_cwd, true);
        Self {
            id: generate_workspace_id(),
            custom_name: Some(name.to_string()),
            group_id: DEFAULT_GROUP_ID.to_string(),
            identity_cwd: identity_cwd.clone(),
            github_scope: crate::github::GithubRepositoryScope::default(),
            default_location: crate::execution_host::ResourceLocation::local(identity_cwd.clone())
                .expect("test workspace cwd is non-empty"),
            cached_identity_cwd: identity_cwd.clone(),
            cached_auto_label,
            cached_git_status_key,
            cached_git_branch: git_branch(&identity_cwd),
            cached_git_ahead_behind: None,
            cached_git_work_summary: None,
            public_pane_numbers,
            next_public_pane_number: 2,
            next_public_tab_number: 2,
            tabs: vec![tab],
            active_tab: 0,
            test_runtimes: HashMap::new(),
        }
    }

    pub(crate) fn insert_test_runtime(&mut self, pane_id: PaneId, runtime: TerminalRuntime) {
        self.test_runtimes.insert(pane_id, runtime);
    }

    pub(crate) fn test_split(&mut self, direction: Direction) -> PaneId {
        let tab = self.active_tab_mut().expect("workspace must have tab");
        let new_id = tab.layout.split_focused(direction);
        tab.panes.insert(
            new_id,
            PaneState::new_with_env_pane_id(TerminalId::alloc(), new_id),
        );
        self.register_new_pane(new_id);
        new_id
    }

    pub(crate) fn test_add_tab(&mut self, name: Option<&str>) -> usize {
        let (events, _) = mpsc::channel(64);
        let render_notify = Arc::new(Notify::new());
        let render_dirty = Arc::new(crate::render_signal::RenderSignal::new());
        let (layout, root_id) = TileLayout::new();
        let mut panes = HashMap::new();
        panes.insert(
            root_id,
            PaneState::new_with_env_pane_id(TerminalId::alloc(), root_id),
        );
        let tab = Tab {
            custom_name: name.map(str::to_string),
            number: self.next_public_tab_number,
            role: TabRole::Terminal,
            root_pane: root_id,
            layout,
            panes,
            runtimes: HashMap::new(),
            zoomed: false,
            events,
            render_notify,
            render_dirty,
        };
        self.next_public_tab_number += 1;
        self.register_new_pane(root_id);
        self.tabs.push(tab);
        self.tabs.len() - 1
    }
}

pub(crate) struct TakenPane {
    pub moved: MovedPane,
    pub removed_tab_idx: Option<usize>,
    pub workspace_empty: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_display_name_from_uses_live_runtime_cwd() {
        let mut ws = Workspace::test_new("ignored");
        ws.custom_name = None;
        ws.identity_cwd = PathBuf::from("/gardn-test/original");
        let root_pane = ws.tabs[0].root_pane;
        let terminal_id = ws.tabs[0].terminal_id(root_pane).unwrap().clone();
        let mut terminals = HashMap::new();
        terminals.insert(
            terminal_id.clone(),
            TerminalState::new(terminal_id, PathBuf::from("/gardn-test/pion")),
        );
        let terminal_runtimes = TerminalRuntimeRegistry::new();

        assert_eq!(ws.display_name(), "original");

        assert_eq!(ws.display_name_from(&terminals, &terminal_runtimes), "pion");
        assert_eq!(
            ws.resolved_identity_cwd_from(&terminals, &terminal_runtimes),
            Some(PathBuf::from("/gardn-test/pion"))
        );
    }

    #[test]
    fn display_name_reads_cached_identity_without_rechecking_filesystem() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "gardn-workspace-label-cache-{}-{stamp}",
            std::process::id()
        ));
        let cwd = root.join("deep/nested");
        std::fs::create_dir_all(&cwd).expect("create nested cwd");

        let mut ws = Workspace::test_new("ignored");
        ws.custom_name = None;
        ws.identity_cwd = cwd.clone();
        ws.tabs.clear();
        ws.cached_identity_cwd = cwd;
        ws.cached_auto_label = "cached-repo".into();

        std::fs::remove_dir_all(root).expect("remove cwd after cache admission");

        assert_eq!(ws.display_name(), "cached-repo");
    }

    #[test]
    fn new_workspace_retains_discovered_git_identity_cache() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "gardn-workspace-git-identity-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join(".git")).expect("create git directory");
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/main\n").expect("write git head");

        let (cached_auto_label, cached_git_status_key) =
            discover_workspace_git_identity(&root, true);

        assert_eq!(
            cached_auto_label,
            root.file_name().and_then(|name| name.to_str()).unwrap()
        );
        assert_eq!(
            cached_git_status_key,
            std::fs::canonicalize(&root).unwrap_or_else(|_| root.clone())
        );

        std::fs::remove_dir_all(root).expect("remove test repo");
    }

    #[test]
    fn workspace_manual_name_overrides_live_runtime_cwd() {
        let mut ws = Workspace::test_new("manual");
        ws.identity_cwd = PathBuf::from("/gardn-test/original");
        let root_pane = ws.tabs[0].root_pane;
        let terminal_id = ws.tabs[0].terminal_id(root_pane).unwrap().clone();
        let mut terminals = HashMap::new();
        terminals.insert(
            terminal_id.clone(),
            TerminalState::new(terminal_id, PathBuf::from("/gardn-test/live")),
        );
        let terminal_runtimes = TerminalRuntimeRegistry::new();

        assert_eq!(
            ws.display_name_from(&terminals, &terminal_runtimes),
            "manual"
        );
        assert_eq!(
            ws.resolved_identity_cwd_from(&terminals, &terminal_runtimes),
            Some(PathBuf::from("/gardn-test/live"))
        );
    }

    #[test]
    fn remote_restore_seed_does_not_read_coordinator_local_git_branch() {
        let root = std::env::temp_dir().join(format!(
            "gardn-remote-restore-branch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/local-only\n").unwrap();

        let remote = crate::execution_host::ResourceLocation::new(
            crate::execution_host::ExecutionHostId::new("ssh:workbox").unwrap(),
            crate::execution_host::HostPath::new(root.clone()).unwrap(),
        );
        let mut ws = Workspace::test_new("ignored");
        ws.custom_name = None;
        ws.identity_cwd = root.clone();
        ws.default_location = remote.clone();
        ws.cached_git_branch = None;

        let root_pane = ws.tabs[0].root_pane;
        let terminal_id = ws.tabs[0].terminal_id(root_pane).unwrap().clone();
        let mut terminals = HashMap::new();
        terminals.insert(
            terminal_id.clone(),
            TerminalState::new_at(terminal_id, remote),
        );
        let terminal_runtimes = TerminalRuntimeRegistry::new();

        // Same path exists on the coordinator with a real branch, but the
        // identity location is remote — never inspect the local filesystem.
        assert_eq!(git_branch(&root).as_deref(), Some("local-only"));
        ws.seed_cached_git_branch_from(&terminals, &terminal_runtimes);
        assert_eq!(ws.cached_git_branch, None);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mixed_host_display_name_uses_focused_terminal_location_atomically() {
        let local_root = std::env::temp_dir().join(format!(
            "gardn-mixed-host-local-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(local_root.join(".git")).unwrap();
        std::fs::write(local_root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();

        let local_default = crate::execution_host::ResourceLocation::local(local_root.clone())
            .expect("local default location");
        let remote_focus = crate::execution_host::ResourceLocation::new(
            crate::execution_host::ExecutionHostId::new("ssh:workbox").unwrap(),
            crate::execution_host::HostPath::new("/srv/project/remote-app").unwrap(),
        );

        let mut ws = Workspace::test_new("ignored");
        ws.custom_name = None;
        ws.identity_cwd = local_root.clone();
        ws.default_location = local_default;

        let root_pane = ws.tabs[0].root_pane;
        let terminal_id = ws.tabs[0].terminal_id(root_pane).unwrap().clone();
        let mut terminals = HashMap::new();
        terminals.insert(
            terminal_id.clone(),
            TerminalState::new_at(terminal_id, remote_focus.clone()),
        );
        let terminal_runtimes = TerminalRuntimeRegistry::new();

        let location = ws
            .resolved_identity_location_from(&terminals, &terminal_runtimes)
            .expect("focused identity location");
        assert_eq!(location, remote_focus);
        // Path basename only — never pair remote path with the local default host
        // (which would label from the coordinator repo root name).
        assert_eq!(
            ws.display_name_from(&terminals, &terminal_runtimes),
            "remote-app"
        );
        assert_ne!(
            ws.display_name_from(&terminals, &terminal_runtimes),
            local_root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap()
        );

        std::fs::remove_dir_all(local_root).unwrap();
    }

    #[test]
    fn local_seed_still_reads_coordinator_git_branch() {
        let root = std::env::temp_dir().join(format!(
            "gardn-local-restore-branch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/HEAD"), "ref: refs/heads/feature\n").unwrap();

        let local = crate::execution_host::ResourceLocation::local(root.clone()).unwrap();
        let mut ws = Workspace::test_new("ignored");
        ws.custom_name = None;
        ws.identity_cwd = root.clone();
        ws.default_location = local.clone();
        ws.cached_git_branch = None;

        let root_pane = ws.tabs[0].root_pane;
        let terminal_id = ws.tabs[0].terminal_id(root_pane).unwrap().clone();
        let mut terminals = HashMap::new();
        terminals.insert(
            terminal_id.clone(),
            TerminalState::new_at(terminal_id, local),
        );
        let terminal_runtimes = TerminalRuntimeRegistry::new();

        ws.seed_cached_git_branch_from(&terminals, &terminal_runtimes);
        assert_eq!(ws.cached_git_branch.as_deref(), Some("feature"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn git_work_summary_label_describes_shell_clean_and_dirty_spaces_without_clean_noise() {
        let mut ws = Workspace::test_new("test");
        assert_eq!(ws.git_work_summary_label(), "");

        ws.cached_git_work_summary = Some(GitWorkSummary {
            repo_count: 1,
            ..GitWorkSummary::default()
        });
        assert_eq!(ws.git_work_summary_label(), "");

        ws.cached_git_work_summary = Some(GitWorkSummary {
            repo_count: 2,
            ..GitWorkSummary::default()
        });
        assert_eq!(ws.git_work_summary_label(), "2 repos");

        ws.cached_git_work_summary = Some(GitWorkSummary {
            repo_count: 2,
            added: 2,
            modified: 1,
            deleted: 1,
            ..GitWorkSummary::default()
        });
        assert_eq!(ws.git_work_summary_label(), "2 repos · +2 ~1 -1");
    }

    #[test]
    fn pane_public_numbers_are_stable_and_not_reused_after_close() {
        let mut ws = Workspace::test_new("test");
        let root = ws.tabs[0].root_pane;
        let second = ws.test_split(Direction::Horizontal);
        let third = ws.test_split(Direction::Vertical);

        assert_eq!(ws.public_pane_number(root), Some(1));
        assert_eq!(ws.public_pane_number(second), Some(2));
        assert_eq!(ws.public_pane_number(third), Some(3));

        assert!(!ws.close_pane(second));

        assert_eq!(ws.public_pane_number(root), Some(1));
        assert_eq!(ws.public_pane_number(second), None);
        assert_eq!(ws.public_pane_number(third), Some(3));

        let fourth = ws.test_split(Direction::Horizontal);
        assert_eq!(ws.public_pane_number(fourth), Some(4));
    }

    #[test]
    fn close_focused_returns_to_the_pane_that_opened_a_split() {
        let mut ws = Workspace::test_new("test");
        let first = ws.tabs[0].root_pane;
        let second = ws.test_split(Direction::Horizontal);
        let third = ws.test_split(Direction::Vertical);

        ws.tabs[0].layout.focus_pane(first);
        let opened = ws.test_split(Direction::Horizontal);
        assert_eq!(ws.tabs[0].layout.focused(), opened);

        assert!(!ws.close_focused());

        assert_eq!(ws.tabs[0].layout.focused(), first);
        assert!(ws.tabs[0].layout.pane_ids().contains(&second));
        assert!(ws.tabs[0].layout.pane_ids().contains(&third));
    }

    #[test]
    fn pane_display_numbers_are_dense_and_tab_local() {
        let mut ws = Workspace::test_new("test");
        let first_tab_root = ws.tabs[0].root_pane;
        let second_tab = ws.test_add_tab(None);
        ws.active_tab = second_tab;
        let second_tab_root = ws.tabs[second_tab].root_pane;
        let second_tab_split = ws.test_split(Direction::Horizontal);

        assert_eq!(ws.pane_display_number(first_tab_root), Some(1));
        assert_eq!(ws.pane_display_number(second_tab_root), Some(1));
        assert_eq!(ws.pane_display_number(second_tab_split), Some(2));

        assert!(!ws.close_pane(second_tab_root));

        assert_eq!(ws.public_pane_number(second_tab_split), Some(3));
        assert_eq!(ws.pane_display_number(second_tab_split), Some(1));
    }

    #[test]
    fn tab_public_numbers_are_stable_across_close_and_move() {
        let mut ws = Workspace::test_new("test");
        let first_root = ws.tabs[0].root_pane;
        let second_tab = ws.test_add_tab(None);
        let second_root = ws.tabs[second_tab].root_pane;
        let third_tab = ws.test_add_tab(None);
        let third_root = ws.tabs[third_tab].root_pane;

        assert_eq!(ws.public_tab_number_for_pane(first_root), Some(1));
        assert_eq!(ws.public_tab_number_for_pane(second_root), Some(2));
        assert_eq!(ws.public_tab_number_for_pane(third_root), Some(3));

        assert!(ws.close_tab(second_tab));
        assert!(ws.move_tab(1, 0));

        assert_eq!(ws.public_tab_number_for_pane(third_root), Some(3));
        assert_eq!(ws.public_tab_number_for_pane(first_root), Some(1));

        let fourth_tab = ws.test_add_tab(None);
        let fourth_root = ws.tabs[fourth_tab].root_pane;
        assert_eq!(ws.public_tab_number_for_pane(fourth_root), Some(4));
    }

    #[test]
    fn moving_tab_keeps_active_identity_and_stable_tab_numbers() {
        let mut ws = Workspace::test_new("test");
        let moved_root = ws.tabs[0].root_pane;
        ws.test_add_tab(Some("foo"));
        let final_auto_idx = ws.test_add_tab(None);
        let active_root = ws.tabs[final_auto_idx].root_pane;
        ws.switch_tab(final_auto_idx);

        assert!(ws.move_tab(0, ws.tabs.len()));

        let labels: Vec<_> = ws.tabs.iter().map(|tab| tab.display_name()).collect();
        assert_eq!(labels, vec!["foo", "3", "1"]);
        assert_eq!(ws.tabs[0].custom_name.as_deref(), Some("foo"));
        assert!(ws.tabs[1].custom_name.is_none());
        assert!(ws.tabs[2].custom_name.is_none());
        assert_eq!(ws.tabs[0].number, 2);
        assert_eq!(ws.tabs[1].number, 3);
        assert_eq!(ws.tabs[2].number, 1);
        assert_eq!(ws.tabs[2].root_pane, moved_root);
        assert_eq!(ws.tabs[ws.active_tab].root_pane, active_root);
    }

    #[tokio::test]
    async fn workspace_can_create_tab_after_all_tabs_are_closed() {
        let mut ws = Workspace::test_new("test");
        assert!(ws.close_tab_allow_empty(0));
        assert!(ws.tabs.is_empty());

        let (events, _) = mpsc::channel(64);
        let render_notify = Arc::new(Notify::new());
        let render_dirty = Arc::new(crate::render_signal::RenderSignal::new());
        let cwd = std::env::current_dir().unwrap_or_else(|_| "/".into());

        let (tab_idx, _terminal, _runtime) = ws
            .create_tab_with_handles(
                24,
                80,
                cwd,
                0,
                crate::terminal_theme::TerminalTheme::default(),
                crate::pane::PaneShellConfig::new("", crate::config::ShellModeConfig::NonLogin),
                events,
                render_notify,
                render_dirty,
            )
            .expect("empty workspace creates new tab");

        assert_eq!(tab_idx, 0);
        assert_eq!(ws.tabs.len(), 1);
        assert_eq!(ws.active_tab, 0);
        assert_eq!(ws.tabs[0].number, 2);
    }
}
