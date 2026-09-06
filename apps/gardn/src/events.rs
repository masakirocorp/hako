//! Internal app events delivered via channel.
//!
//! Background tasks (PTY child watchers, future hook listeners, etc.) send
//! events to the main loop through this channel. No polling needed.

use std::time::Instant;

use crate::detect::{Agent, AgentState};
use crate::layout::PaneId;
use crate::workspace::{GitStatusCacheEntry, WorkspaceGitStatus};

/// An event from a background task to the main loop.
#[derive(Debug)]
pub enum AppEvent {
    /// A pane's child process exited.
    PaneDied {
        pane_id: PaneId,
        child_pid: u32,
        exit_success: bool,
        /// Present when the process exited with a normal status code.
        exit_code: Option<i32>,
        /// Present when the process was terminated by a signal.
        exit_signal: Option<i32>,
    },
    /// Process detection identified an agent before its screen state was confirmed.
    AgentProcessDetected {
        pane_id: PaneId,
        agent: Agent,
        observed_at: Instant,
    },
    /// Fallback detector state changed in a pane.
    StateChanged {
        pane_id: PaneId,
        agent: Option<Agent>,
        state: AgentState,
        visible_blocker: bool,
        visible_idle: bool,
        visible_working: bool,
        process_exited: bool,
        observed_at: Instant,
    },
    /// Hook-authoritative agent state was reported for a pane.
    HookStateReported {
        pane_id: PaneId,
        source: String,
        agent_label: String,
        state: AgentState,
        message: Option<String>,
        custom_status: Option<String>,
        seq: Option<u64>,
        session_ref: Option<crate::agent_resume::AgentSessionRef>,
        launch_env: Vec<(String, String)>,
    },
    /// Hook-reported native agent session identity without lifecycle authority.
    HookSessionReported {
        pane_id: PaneId,
        source: String,
        agent_label: String,
        seq: Option<u64>,
        session_start_source: Option<String>,
        session_ref: Option<crate::agent_resume::AgentSessionRef>,
        launch_env: Vec<(String, String)>,
    },
    /// Display-only agent metadata was reported for a pane.
    HookMetadataReported {
        pane_id: PaneId,
        source: String,
        agent_label: Option<String>,
        applies_to_source: Option<String>,
        title: Option<String>,
        display_agent: Option<String>,
        custom_status: Option<String>,
        state_labels: std::collections::HashMap<String, String>,
        tokens: std::collections::HashMap<String, Option<String>>,
        clear_title: bool,
        clear_display_agent: bool,
        clear_custom_status: bool,
        clear_state_labels: bool,
        seq: Option<u64>,
        ttl: Option<std::time::Duration>,
    },
    /// Hook authority was explicitly cleared for a pane.
    HookAuthorityCleared {
        pane_id: PaneId,
        source: Option<String>,
        seq: Option<u64>,
    },
    /// The current detected agent gracefully released this pane back to the shell.
    HookAgentReleased {
        pane_id: PaneId,
        source: String,
        agent_label: String,
        known_agent: Option<Agent>,
        session_ref: Option<crate::agent_resume::AgentSessionRef>,
        seq: Option<u64>,
    },
    /// A new version is available through the active installation manager.
    UpdateReady {
        version: String,
        install: crate::install::UpdateInstallAction,
    },
    /// Remote agent detection manifest update check finished.
    AgentDetectionManifestsUpdated {
        updated: Vec<crate::detect::manifest_update::ManifestUpdateCommit>,
        status: crate::detect::manifest_update::ManifestUpdateStatus,
    },
    /// A user interaction requested a write to the invoking rendering client's clipboard.
    ClipboardWrite {
        content: Vec<u8>,
    },
    ClientClipboardWrite {
        view_id: u64,
        content: Vec<u8>,
    },
    /// A pane emitted a valid OSC 52 clipboard write.
    TerminalClipboardWrite {
        pane_id: PaneId,
        content: Vec<u8>,
    },
    /// A pane child emitted one or more executable BEL characters.
    /// The host-facing process forwards them to its outer terminal.
    TerminalBell {
        pane_id: PaneId,
        count: u16,
    },
    /// An execution host finished staging a private temporary file.
    ExecutionFileStaged {
        host_id: crate::execution_host::ExecutionHostId,
        request_id: crate::execution_host::protocol::RequestId,
        location: crate::execution_host::ResourceLocation,
        result:
            Result<crate::execution_host::HostPath, crate::execution_host::protocol::WorkerError>,
    },
    /// A terminal hyperlink requested opening on its rendering client's host.
    OpenUrl {
        pane_id: PaneId,
        url: String,
    },
    ClientOpenUrl {
        view_id: u64,
        url: String,
    },
    /// Prefix-mode ASCII input-source intent. The foreground client applies
    /// the host-local switch in server mode; the monolithic app applies it
    /// in-process.
    PrefixInputSource {
        active: bool,
    },
    /// Background git status refresh completed for workspaces.
    GitStatusRefreshed {
        results: Vec<WorkspaceGitStatus>,
        cache_updates: Vec<(crate::execution_host::ResourceLocation, GitStatusCacheEntry)>,
        repo_summaries: Vec<(std::path::PathBuf, crate::workspace::GitWorkSummary)>,
    },
    /// A plugin action or event command finished.
    PluginCommandFinished {
        log_id: String,
        finished_unix_ms: u64,
        exit_code: Option<i32>,
        stdout: String,
        stderr: String,
        error: Option<String>,
    },
    /// Cross-session and managed-binding inventory completed for destructive removal.
    ConnectionRetirementPreviewed {
        authentication_owner: crate::execution_host::auth::AuthenticationOwner,
        profile_id: String,
        result: Result<crate::app::state::ConnectionRetirementPreview, String>,
    },
    /// Confirmed connection retirement started for one client-owned editor.
    ConnectionRetirementStarted {
        authentication_owner: crate::execution_host::auth::AuthenticationOwner,
        profile_id: String,
        preview: crate::app::state::ConnectionRetirementPreview,
    },
    /// Global connection retirement finished or stopped safely.
    ConnectionRetired {
        authentication_owner: crate::execution_host::auth::AuthenticationOwner,
        profile_id: String,
        result: Result<String, String>,
        journal:
            Option<crate::execution_host::connection_retirement::ConnectionRetirementJournalGuard>,
    },
}
