use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Rect};

use crate::{
    app::{
        state::{
            normalize_theme_name, theme_names_for_appearance, AppState, DragState, DragTarget,
            SettingsSection, SettingsSidebarSelection, SettingsState, THEME_NAMES,
        },
        view_state::ClientViewState,
        App, Mode,
    },
    config::{
        AgentPanelScopeConfig, CommandsConfig, ContextBarVisibilityConfig, NewTerminalCwdConfig,
        PaneBorderAgentInfoConfig, RightClickPassthroughModifierConfig, ShellModeConfig,
        SidebarArrangementConfig, SidebarInitialStateConfig, StatusIndicatorStyle, TerminalAccent,
        ThemeMode, ToastClipboardPosition, ToastDelivery, ToastGardnPosition,
        MAX_TOAST_DELAY_SECONDS,
    },
    settings_rows::{
        connection_editor_open as settings_connection_editor_open, next_option_index, option_count,
        option_hit_for_visual_row, previous_option_index, rows_for_section, selected_visual_row,
        visual_row_count, AdvancedRowId, BehaviorRowId, CommandAction, CommandField, CommandRowId,
        ConnectionField, ConnectionRowId, NotificationRowId, SettingsListRow, SettingsRowHit,
        GROUP_DEFAULTS_DIRECTORY, GROUP_DEFAULTS_HOST, GROUP_GENERAL_DELETE, GROUP_GENERAL_ICON,
        GROUP_GENERAL_NAME, GROUP_GITHUB_ORGANIZATION, WORKSPACE_GENERAL_DIRECTORY,
        WORKSPACE_GENERAL_HOST, WORKSPACE_GENERAL_NAME, WORKSPACE_GITHUB_AUTOMATIC,
        WORKSPACE_GITHUB_GROUP, WORKSPACE_GITHUB_REPOSITORIES, WORKSPACE_GITHUB_SELECTED,
    },
    terminal_theme::ThemeAppearance,
};

use super::ScrollbarClickTarget;

#[cfg(test)]
use crate::settings_rows::{
    CONNECTION_DELETE_INDEX, CONNECTION_DISCARD_INDEX, CONNECTION_SAVE_INDEX,
    CONNECTION_TARGET_INDEX, CONNECTION_TEST_INDEX,
};

#[derive(Debug, Clone, PartialEq, Eq)]
// The shared `Save` verb is semantic: these actions persist settings.
#[allow(clippy::enum_variant_names)]
pub(crate) enum SettingsAction {
    SaveSettings {
        light: String,
        dark: String,
        mode: ThemeMode,
        terminal_light_accent: TerminalAccent,
        terminal_dark_accent: TerminalAccent,
        sound_enabled: bool,
        toast_delivery: ToastDelivery,
        confirm_close: bool,
        prompt_new_tab_name: bool,
        show_counters: bool,
        pane_borders: bool,
        pane_scrollbars: bool,
        pane_gaps: bool,
        hide_tab_bar_when_single_tab: bool,
        copy_on_select: bool,
        prompt_new_workspace_name: bool,
        right_click_passthrough_modifier: RightClickPassthroughModifierConfig,
        new_terminal_cwd: NewTerminalCwdConfig,
        mouse_scroll_lines: usize,
        browser_command: String,
        review_command: String,
        editor_command: String,
        sidebar_width: u16,
        sidebar_min_width: u16,
        sidebar_max_width: u16,
        sidebar_arrangement: SidebarArrangementConfig,
        context_bar_visibility: ContextBarVisibilityConfig,
        sidebar_initial_state: SidebarInitialStateConfig,
        sidebar_initial_agent_scope: AgentPanelScopeConfig,
        pane_border_agent_info: PaneBorderAgentInfoConfig,
        status_indicators: StatusIndicatorStyle,
    },
    SaveSwitchAsciiInputSourceInPrefix(bool),
    SaveKittyGraphics(bool),
    SaveResumeAgentsOnRestore(bool),
    SaveWindowTitle(String),
    SaveHeadlessSize {
        cols: u16,
        rows: u16,
    },
    SaveDefaultShell(String),
    SaveShellMode(ShellModeConfig),
    SaveVersionCheck(bool),
    SaveManifestCheck(bool),
    SaveToastDelay(u64),
    SaveToastGardnPosition(ToastGardnPosition),
    SaveClipboardToastEnabled(bool),
    SaveClipboardToastPosition(ToastClipboardPosition),
    SaveGroupAccent {
        group_idx: usize,
        accent: Option<TerminalAccent>,
    },
    SaveGroupName {
        group_idx: usize,
        name: String,
    },
    SaveGroupIcon {
        group_idx: usize,
        icon: String,
    },
    SaveGroupGithubOrganization {
        group_idx: usize,
        organization: Option<crate::app::state::GithubOrganization>,
    },
    SaveGroupDefaultLocation {
        group_idx: usize,
        default_location: Option<crate::execution_host::ResourceLocation>,
    },
    SaveWorkspaceName {
        ws_idx: usize,
        name: String,
    },
    SaveWorkspaceDefaultLocation {
        ws_idx: usize,
        location: crate::execution_host::ResourceLocation,
    },
    SaveWorkspaceGithubScope {
        ws_idx: usize,
        scope: crate::github::GithubRepositoryScope,
    },
    DeleteGroup(usize),
    CycleIntegrationHost,
    InstallIntegration(crate::api::schema::IntegrationTarget),
    UninstallIntegration(crate::api::schema::IntegrationTarget),
    SaveAgentProfile(crate::agent_profiles::UserAgentProfileConfig),
    DeleteAgentProfile(String),
    SaveSshConnectionProfile(crate::persist::ssh_profiles::SshConnectionProfile),
    PreviewSshConnectionRetirement(String),
    ConfirmSshConnectionRetirement {
        profile_id: String,
        preview: crate::app::state::ConnectionRetirementPreview,
    },
    ConfirmLocalConnectionForget {
        profile_id: String,
        plan: crate::execution_host::connection_retirement::ConnectionRetirementPlan,
    },
    TestSshConnection {
        profile_id: String,
    },
    ConnectSshConnection {
        profile_id: String,
    },
    LaunchSshWorkspace {
        profile_id: String,
    },
    DisconnectSshConnection {
        profile_id: String,
    },
    RequestForgetRemoteTermination {
        terminal_id: crate::terminal::TerminalId,
    },
    ConfirmForgetRemoteTermination {
        terminal_id: crate::terminal::TerminalId,
    },
}

impl App {
    pub(crate) fn handle_settings_key(&mut self, key: KeyEvent) {
        let previous_section = self.state.settings.section;
        if let Some(action) = update_settings_state(&mut self.state, key) {
            self.apply_settings_action(action);
        }
        if previous_section != SettingsSection::Integrations
            && self.state.settings.section == SettingsSection::Integrations
        {
            self.apply_integration_operation(
                crate::integration::host::HostIntegrationOperation::Inspect,
            );
        }
    }

    pub(crate) fn apply_settings_action(&mut self, action: SettingsAction) {
        match action {
            SettingsAction::SaveSettings {
                light,
                dark,
                mode,
                terminal_light_accent,
                terminal_dark_accent,
                sound_enabled,
                toast_delivery,
                confirm_close,
                prompt_new_tab_name,
                show_counters,
                pane_borders,
                pane_scrollbars,
                pane_gaps,
                hide_tab_bar_when_single_tab,
                copy_on_select,
                prompt_new_workspace_name,
                right_click_passthrough_modifier,
                new_terminal_cwd,
                mouse_scroll_lines,
                browser_command,
                review_command,
                editor_command,
                sidebar_width,
                sidebar_min_width,
                sidebar_max_width,
                sidebar_arrangement,
                context_bar_visibility,
                sidebar_initial_state,
                sidebar_initial_agent_scope,
                pane_border_agent_info,
                status_indicators,
            } => {
                self.save_theme(
                    &light,
                    &dark,
                    mode,
                    terminal_light_accent,
                    terminal_dark_accent,
                );
                self.save_sound(sound_enabled);
                self.save_confirm_close(confirm_close);
                self.save_prompt_new_tab_name(prompt_new_tab_name);
                self.save_show_counters(show_counters);
                self.save_pane_appearance(
                    pane_borders,
                    pane_scrollbars,
                    pane_gaps,
                    hide_tab_bar_when_single_tab,
                );
                self.save_behavior_selection(
                    copy_on_select,
                    prompt_new_workspace_name,
                    right_click_passthrough_modifier,
                );
                self.save_new_terminal_cwd(&new_terminal_cwd);
                self.save_mouse_scroll_lines(mouse_scroll_lines);
                self.save_commands(&browser_command, &review_command, &editor_command);
                self.save_sidebar_widths(sidebar_width, sidebar_min_width, sidebar_max_width);
                self.save_sidebar_arrangement(sidebar_arrangement);
                self.save_context_bar_visibility(context_bar_visibility);
                self.save_sidebar_initial_view(sidebar_initial_state, sidebar_initial_agent_scope);
                self.save_toast_delivery(toast_delivery);
                self.save_pane_border_agent_info(pane_border_agent_info);
                self.save_status_indicators(status_indicators);
            }
            SettingsAction::SaveResumeAgentsOnRestore(enabled) => {
                self.save_resume_agents_on_restore(enabled);
            }
            SettingsAction::SaveWindowTitle(template) => {
                self.save_window_title(&template);
            }
            SettingsAction::SaveHeadlessSize { cols, rows } => {
                self.save_headless_size(cols, rows);
            }
            SettingsAction::SaveDefaultShell(shell) => {
                self.save_default_shell(&shell);
            }
            SettingsAction::SaveShellMode(mode) => {
                self.save_shell_mode(mode);
            }
            SettingsAction::SaveVersionCheck(enabled) => {
                self.save_version_check(enabled);
            }
            SettingsAction::SaveManifestCheck(enabled) => {
                self.save_manifest_check(enabled);
            }
            SettingsAction::SaveToastDelay(seconds) => {
                self.save_toast_delay(seconds);
            }
            SettingsAction::SaveToastGardnPosition(position) => {
                self.save_toast_gardn_position(position);
            }
            SettingsAction::SaveClipboardToastEnabled(enabled) => {
                self.save_clipboard_toast_enabled(enabled);
            }
            SettingsAction::SaveClipboardToastPosition(position) => {
                self.save_clipboard_toast_position(position);
            }
            SettingsAction::SaveWorkspaceName { ws_idx, name } => {
                self.state.rename_workspace(ws_idx, name);
            }
            SettingsAction::SaveWorkspaceDefaultLocation { ws_idx, location } => {
                self.state.set_workspace_default_location(ws_idx, location);
            }
            SettingsAction::SaveWorkspaceGithubScope { ws_idx, scope } => {
                self.state.set_workspace_github_scope(ws_idx, scope);
            }

            SettingsAction::SaveGroupName { group_idx, name } => {
                self.state.rename_group(group_idx, name);
            }
            SettingsAction::SaveGroupIcon { group_idx, icon } => {
                self.state.set_group_icon(group_idx, icon);
            }
            SettingsAction::SaveGroupGithubOrganization {
                group_idx,
                organization,
            } => {
                self.state
                    .set_group_github_organization(group_idx, organization);
            }
            SettingsAction::SaveGroupAccent { group_idx, accent } => {
                self.state.set_group_accent(group_idx, accent);
                self.query_host_terminal_theme();
            }

            SettingsAction::SaveGroupDefaultLocation {
                group_idx,
                default_location,
            } => {
                self.state
                    .set_group_default_location(group_idx, default_location);
            }
            SettingsAction::DeleteGroup(group_idx) => {
                super::modal::open_confirm_delete_group(&mut self.state, group_idx);
            }
            SettingsAction::SaveSwitchAsciiInputSourceInPrefix(enabled) => {
                self.save_switch_ascii_input_source_in_prefix(enabled)
            }
            SettingsAction::SaveKittyGraphics(enabled) => self.save_kitty_graphics(enabled),
            SettingsAction::CycleIntegrationHost => self.cycle_integration_host(),
            SettingsAction::InstallIntegration(target) => self.apply_integration_operation(
                crate::integration::host::HostIntegrationOperation::EnsureCurrent { target },
            ),
            SettingsAction::UninstallIntegration(target) => self.apply_integration_operation(
                crate::integration::host::HostIntegrationOperation::UninstallOwned { target },
            ),
            SettingsAction::SaveAgentProfile(profile) => {
                if self.save_agent_profile(profile) {
                    close_agent_profile_editor(&mut self.state);
                }
            }
            SettingsAction::DeleteAgentProfile(profile_id) => {
                self.delete_agent_profile(&profile_id)
            }
            SettingsAction::SaveSshConnectionProfile(profile) => {
                self.save_ssh_connection_profile(profile)
            }
            SettingsAction::PreviewSshConnectionRetirement(profile_id) => {
                let owner = crate::execution_host::auth::AuthenticationOwner::new(
                    self.default_client_view.id(),
                );
                self.preview_connection_retirement_for(owner, profile_id);
            }
            SettingsAction::ConfirmSshConnectionRetirement {
                profile_id,
                preview,
            } => {
                let owner = crate::execution_host::auth::AuthenticationOwner::new(
                    self.default_client_view.id(),
                );
                self.retire_connection_for(owner, profile_id, preview);
            }
            SettingsAction::ConfirmLocalConnectionForget { profile_id, plan } => {
                let owner = crate::execution_host::auth::AuthenticationOwner::new(
                    self.default_client_view.id(),
                );
                self.forget_connection_locally_for(owner, profile_id, plan);
            }
            SettingsAction::TestSshConnection { profile_id } => {
                let owner = crate::execution_host::auth::AuthenticationOwner::new(
                    self.default_client_view.id(),
                );
                self.request_connection_for(
                    owner,
                    &profile_id,
                    crate::execution_host::HostConnectionAction::Test,
                );
            }
            SettingsAction::ConnectSshConnection { profile_id } => {
                let owner = crate::execution_host::auth::AuthenticationOwner::new(
                    self.default_client_view.id(),
                );
                self.state.queue_ssh_connection_request(
                    profile_id,
                    crate::execution_host::HostConnectionAction::Connect,
                    owner,
                )
            }
            SettingsAction::LaunchSshWorkspace { profile_id } => {
                let Some(profile) = self
                    .state
                    .ssh_connection_profiles
                    .iter()
                    .find(|profile| profile.id() == profile_id)
                    .cloned()
                else {
                    return;
                };
                let path = profile.suggested_directory().cloned().unwrap_or_default();
                let location =
                    crate::execution_host::ResourceLocation::new(profile.execution_host_id(), path);
                let group_id = self.state.active_group_id().to_string();
                match self.begin_remote_workspace(location, true, group_id, None, Vec::new()) {
                    Ok(_) => close_settings(&mut self.state),
                    Err(error) => {
                        self.state.toast = Some(crate::app::state::ToastNotification {
                            kind: crate::app::state::ToastKind::NeedsAttention,
                            title: "Could Not Open Workspace".to_string(),
                            context: error,
                            position: None,
                            target: None,
                        });
                    }
                }
            }
            SettingsAction::DisconnectSshConnection { profile_id } => {
                let owner = crate::execution_host::auth::AuthenticationOwner::new(
                    self.default_client_view.id(),
                );
                self.state.queue_ssh_connection_request(
                    profile_id,
                    crate::execution_host::HostConnectionAction::Disconnect,
                    owner,
                )
            }
            SettingsAction::RequestForgetRemoteTermination { terminal_id } => {
                if let Some(editor) = self.state.settings.connection_editor.as_mut() {
                    editor.pending_forget_remote_terminal = Some(terminal_id);
                }
            }
            SettingsAction::ConfirmForgetRemoteTermination { terminal_id } => {
                match self.forget_remote_termination(&terminal_id) {
                    Ok(true) => {
                        if let Some(editor) = self.state.settings.connection_editor.as_mut() {
                            editor.pending_forget_remote_terminal = None;
                        }
                    }
                    Ok(false) => {}
                    Err(err) => {
                        self.state.toast = Some(crate::app::state::ToastNotification {
                            kind: crate::app::state::ToastKind::NeedsAttention,
                            title: "Remote Termination Not Forgotten".to_string(),
                            context: err.to_string(),
                            position: None,
                            target: None,
                        });
                    }
                }
            }
        }
    }

    /// Persist a connection profile after shared reference guards pass.
    pub(super) fn save_ssh_connection_profile(
        &mut self,
        profile: crate::persist::ssh_profiles::SshConnectionProfile,
    ) {
        if let Err(err) = self.commit_ssh_connection_profile(profile) {
            self.state.toast = Some(crate::app::state::ToastNotification {
                kind: crate::app::state::ToastKind::NeedsAttention,
                title: "Connection Profile Not Saved".to_string(),
                context: err.to_string(),
                position: None,
                target: None,
            });
        }
    }

    fn cycle_integration_host(&mut self) {
        let current = self.state.settings.integration_host_profile_id.as_deref();
        let next_index = current
            .and_then(|profile_id| {
                self.state
                    .ssh_connection_profiles
                    .iter()
                    .position(|profile| profile.id() == profile_id)
            })
            .map_or(0, |index| index + 1);
        let next = if next_index < self.state.ssh_connection_profiles.len() {
            Some(
                self.state.ssh_connection_profiles[next_index]
                    .id()
                    .to_string(),
            )
        } else {
            None
        };
        self.state.settings.integration_host_profile_id = next.clone();
        self.state.settings.list.selected = 0;
        if next.is_some() {
            self.apply_integration_operation(
                crate::integration::host::HostIntegrationOperation::Inspect,
            );
        }
    }

    pub(super) fn apply_integration_operation(
        &mut self,
        operation: crate::integration::host::HostIntegrationOperation,
    ) {
        let selected_profile_id = self.state.settings.integration_host_profile_id.clone();
        let host_id = crate::app::integration_host::resolve(&self.state, &self.state.settings)
            .host_id()
            .cloned();
        if selected_profile_id.is_some() && host_id.is_none() {
            self.state.settings.integration_host_profile_id = None;
        }
        let Some(host_id) = host_id else {
            match operation {
                crate::integration::host::HostIntegrationOperation::Inspect => {
                    self.refresh_integration_recommendations();
                }
                crate::integration::host::HostIntegrationOperation::EnsureCurrent { target } => {
                    self.install_integration(target);
                }
                crate::integration::host::HostIntegrationOperation::UninstallOwned { target } => {
                    self.uninstall_integration(target);
                }
            }
            return;
        };

        self.state
            .host_integration_install_messages
            .remove(&host_id);
        let request =
            crate::integration::host::request_for_catalog(operation, &self.state.agent_profiles);
        let outcome = self.execution_hosts.as_mut().map_or_else(
            || Err("execution host manager is unavailable".to_string()),
            |hosts| {
                hosts
                    .request_agent_integrations(host_id.clone(), request)
                    .map_err(|error| error.to_string())
            },
        );
        match outcome {
            Ok(request_id) => {
                self.state
                    .host_integration_request_ids
                    .insert(host_id.clone(), request_id);
                self.state.host_integration_observations.insert(
                    host_id,
                    crate::integration::host::HostIntegrationObservation::Pending,
                );
            }
            Err(message) => {
                self.state.host_integration_request_ids.remove(&host_id);
                self.state.host_integration_observations.insert(
                    host_id,
                    crate::integration::host::HostIntegrationObservation::Failed(message),
                );
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemeChoiceTarget {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ThemeChoice {
    name: &'static str,
    target: ThemeChoiceTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThemeSettingsChoice {
    SourceSystem,
    SourceCustom,
    GroupAccent(Option<TerminalAccent>),
    TerminalLightAccent(TerminalAccent),
    TerminalDarkAccent(TerminalAccent),
    Mode(ThemeMode),
    Theme(ThemeChoice),
}

fn pending_uses_system_theme_source(state: &AppState) -> bool {
    pending_theme_mode(state) == ThemeMode::System
        && normalize_theme_name(&pending_light_theme_name(state)) == "system"
        && normalize_theme_name(&pending_dark_theme_name(state)) == "system"
}

fn pending_shows_terminal_accent(state: &AppState) -> bool {
    state.settings.group_settings_target.is_none() && pending_uses_system_theme_source(state)
}

fn next_toast_delivery(delivery: ToastDelivery) -> ToastDelivery {
    match delivery {
        ToastDelivery::Off => ToastDelivery::Gardn,
        ToastDelivery::Gardn => ToastDelivery::Terminal,
        ToastDelivery::Terminal => ToastDelivery::System,
        ToastDelivery::System => ToastDelivery::Off,
    }
}

fn global_theme_choices(mode: ThemeMode) -> Vec<ThemeChoice> {
    match mode {
        ThemeMode::Light => theme_names_for_appearance(ThemeAppearance::Light)
            .iter()
            .copied()
            .map(|name| ThemeChoice {
                name,
                target: ThemeChoiceTarget::Light,
            })
            .collect(),
        ThemeMode::Dark => theme_names_for_appearance(ThemeAppearance::Dark)
            .iter()
            .copied()
            .map(|name| ThemeChoice {
                name,
                target: ThemeChoiceTarget::Dark,
            })
            .collect(),
        ThemeMode::System => {
            let mut choices = Vec::with_capacity(
                theme_names_for_appearance(ThemeAppearance::Light).len()
                    + theme_names_for_appearance(ThemeAppearance::Dark).len(),
            );
            choices.extend(
                theme_names_for_appearance(ThemeAppearance::Light)
                    .iter()
                    .copied()
                    .map(|name| ThemeChoice {
                        name,
                        target: ThemeChoiceTarget::Light,
                    }),
            );
            choices.extend(
                theme_names_for_appearance(ThemeAppearance::Dark)
                    .iter()
                    .copied()
                    .map(|name| ThemeChoice {
                        name,
                        target: ThemeChoiceTarget::Dark,
                    }),
            );
            choices
        }
    }
}
fn theme_settings_choices(state: &AppState) -> Vec<ThemeSettingsChoice> {
    if state.settings.group_settings_target.is_some() {
        let mut choices = Vec::with_capacity(1 + TerminalAccent::ALL.len());
        choices.push(ThemeSettingsChoice::GroupAccent(None));
        choices.extend(
            TerminalAccent::ALL
                .iter()
                .copied()
                .map(|accent| ThemeSettingsChoice::GroupAccent(Some(accent))),
        );
        return choices;
    }

    let mut choices = Vec::with_capacity(
        2 + (TerminalAccent::ALL.len() * 2) + ThemeMode::ALL.len() + THEME_NAMES.len(),
    );
    choices.push(ThemeSettingsChoice::SourceSystem);
    choices.push(ThemeSettingsChoice::SourceCustom);
    if pending_shows_terminal_accent(state) {
        choices.extend(
            TerminalAccent::ALL
                .iter()
                .copied()
                .map(ThemeSettingsChoice::TerminalLightAccent),
        );
        choices.extend(
            TerminalAccent::ALL
                .iter()
                .copied()
                .map(ThemeSettingsChoice::TerminalDarkAccent),
        );
    }
    if !pending_uses_system_theme_source(state) {
        let theme_choices = global_theme_choices(pending_theme_mode(state));
        choices.extend(
            ThemeMode::ALL
                .iter()
                .copied()
                .map(ThemeSettingsChoice::Mode),
        );
        choices.extend(theme_choices.into_iter().map(ThemeSettingsChoice::Theme));
    }
    choices
}

fn theme_choice_len(state: &AppState) -> usize {
    theme_settings_choices(state).len()
}

fn theme_rows(state: &AppState) -> Vec<crate::settings_rows::SettingsListRow> {
    rows_for_section(state, SettingsSection::Theme).unwrap_or_default()
}

fn theme_visual_len(state: &AppState) -> usize {
    visual_row_count(&theme_rows(state))
}

fn theme_visual_row_for_selection(state: &AppState, selected: usize) -> usize {
    selected_visual_row(&theme_rows(state), selected).unwrap_or(0)
}

fn settings_section_choice_len(state: &AppState, section: SettingsSection) -> usize {
    rows_for_section(state, section)
        .map(|rows| option_count(&rows))
        .unwrap_or_else(|| match section {
            SettingsSection::Integrations => state.integration_recommendations.len(),
            SettingsSection::Theme => theme_choice_len(state),
            SettingsSection::Layout
            | SettingsSection::Sound
            | SettingsSection::Toast
            | SettingsSection::PaneLabels
            | SettingsSection::Commands
            | SettingsSection::Experiments
            | SettingsSection::Agents
            | SettingsSection::Connections
            | SettingsSection::GroupProfiles
            | SettingsSection::GroupGeneral
            | SettingsSection::GroupDefaults
            | SettingsSection::GroupGithub
            | SettingsSection::WorkspaceGeneral
            | SettingsSection::WorkspaceGithub
            | SettingsSection::About => 0,
        })
}

fn settings_section_scroll_len(state: &AppState, section: SettingsSection) -> usize {
    rows_for_section(state, section)
        .map(|rows| visual_row_count(&rows))
        .unwrap_or_else(|| match section {
            SettingsSection::Theme => theme_visual_len(state),
            SettingsSection::Integrations => state.integration_recommendations.len(),
            _ => 0,
        })
}

fn settings_section_list_rect(state: &AppState, section: SettingsSection) -> Rect {
    let body_area = crate::ui::settings_section_list_rect(state.settings_content_rect());
    if section == SettingsSection::Integrations {
        let [list_area, _] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(2)]).areas::<2>(body_area);
        list_area
    } else {
        body_area
    }
}

fn settings_section_list_geometry(
    state: &AppState,
    section: SettingsSection,
) -> crate::ui::ModalListGeometry {
    crate::ui::ModalListGeometry::new(
        settings_section_list_rect(state, section),
        settings_section_scroll_len(state, section),
        state.settings.scroll,
    )
}

fn settings_section_viewport(
    state: &AppState,
    section: SettingsSection,
) -> crate::ui::ModalListViewport {
    settings_section_list_geometry(state, section).viewport
}

fn settings_theme_viewport(state: &AppState) -> crate::ui::ModalListViewport {
    settings_section_viewport(state, SettingsSection::Theme)
}

fn settings_section_max_scroll(state: &AppState, section: SettingsSection) -> usize {
    settings_section_viewport(state, section).max_scroll()
}

fn settings_theme_max_scroll(state: &AppState) -> usize {
    settings_section_max_scroll(state, SettingsSection::Theme)
}

fn ensure_settings_selection_visible(state: &mut AppState) {
    let section = state.settings.section;
    let viewport = settings_section_viewport(state, section);
    state.settings.scroll = viewport.scroll();

    let selected_row = rows_for_section(state, section)
        .and_then(|rows| selected_visual_row(&rows, state.settings.list.selected))
        .unwrap_or_else(|| {
            if section == SettingsSection::Theme {
                theme_visual_row_for_selection(state, state.settings.list.selected)
            } else {
                state.settings.list.selected
            }
        });
    state.settings.scroll = viewport.ensure_visible(selected_row, None);
}

fn set_settings_theme_offset_from_bottom(state: &mut AppState, offset_from_bottom: usize) {
    state.settings.scroll =
        settings_theme_viewport(state).scroll_from_offset_from_bottom(offset_from_bottom);
}

fn group_accent_choice_at_cursor(state: &AppState) -> Option<TerminalAccent> {
    theme_settings_choices(state)
        .get(state.settings.list.selected)
        .and_then(|choice| match choice {
            ThemeSettingsChoice::GroupAccent(accent) => Some(*accent),
            _ => None,
        })
        .unwrap_or(None)
}

fn group_accent_selection_index(state: &AppState) -> usize {
    let accent = state
        .settings
        .pending_group_accent_choice
        .unwrap_or_else(|| {
            state
                .settings
                .group_settings_target
                .and_then(|group_idx| state.groups.get(group_idx))
                .and_then(|group| group.accent)
        });
    accent
        .and_then(|accent| {
            TerminalAccent::ALL
                .iter()
                .position(|candidate| *candidate == accent)
                .map(|idx| idx + 1)
        })
        .unwrap_or(0)
}

fn checked_group_accent_choice(state: &AppState) -> Option<TerminalAccent> {
    state
        .settings
        .pending_group_accent_choice
        .unwrap_or_else(|| group_accent_choice_at_cursor(state))
}

fn pending_group_name(state: &AppState) -> String {
    state
        .settings
        .pending_group_name
        .clone()
        .or_else(|| {
            state
                .settings
                .group_settings_target
                .and_then(|group_idx| state.groups.get(group_idx))
                .map(|group| group.name.clone())
        })
        .unwrap_or_default()
}

fn set_pending_group_name(state: &mut AppState, name: String) {
    state.settings.pending_group_name = Some(name);
}

fn pending_group_icon(state: &AppState) -> String {
    state
        .settings
        .pending_group_icon
        .clone()
        .or_else(|| {
            state
                .settings
                .group_settings_target
                .and_then(|group_idx| state.groups.get(group_idx))
                .map(|group| group.icon.clone())
        })
        .unwrap_or_else(|| crate::app::state::DEFAULT_GROUP_ICON.to_string())
}

fn toggle_group_icon_picker(state: &mut AppState) {
    state.settings.group_icon_picker_open = !state.settings.group_icon_picker_open;
}

fn group_settings_icon_picker_hit(state: &AppState, col: u16, row: u16) -> Option<&'static str> {
    if !state.settings.group_icon_picker_open
        || state.settings.section != SettingsSection::GroupGeneral
    {
        return None;
    }
    let list = settings_section_list_geometry(state, SettingsSection::GroupGeneral);
    let rows = rows_for_section(state, SettingsSection::GroupGeneral)?;
    let icon_visual = selected_visual_row(&rows, GROUP_GENERAL_ICON)?;
    let picker_start = icon_visual + 1;
    let y = list.rect.y + picker_start.saturating_sub(state.settings.scroll) as u16;
    let origin = Rect::new(
        list.rect.x + 2,
        y,
        list.rect.width.saturating_sub(2).min(24),
        crate::ui::group_icon_picker_row_count(),
    );
    crate::ui::group_icon_picker_rects_at(origin)
        .into_iter()
        .find(|(rect, _)| {
            col >= rect.x
                && col < rect.x + rect.width
                && row >= rect.y
                && row < rect.y + rect.height
        })
        .map(|(_, icon)| icon)
}

fn pending_group_default_directory(state: &AppState) -> String {
    state
        .settings
        .pending_group_default_directory
        .clone()
        .or_else(|| {
            state
                .settings
                .group_settings_target
                .and_then(|group_idx| state.groups.get(group_idx))
                .and_then(|group| group.default_location.as_ref())
                .map(|location| location.path.as_path().display().to_string())
        })
        .unwrap_or_default()
}

fn pending_group_default_host(state: &AppState) -> crate::execution_host::ExecutionHostId {
    state
        .settings
        .pending_group_default_execution_host_id
        .clone()
        .or_else(|| {
            state
                .settings
                .group_settings_target
                .and_then(|group_idx| state.groups.get(group_idx))
                .and_then(|group| group.default_location.as_ref())
                .map(|location| location.execution_host_id.clone())
        })
        .unwrap_or_else(crate::execution_host::ExecutionHostId::local)
}

fn pending_workspace_default_host(state: &AppState) -> crate::execution_host::ExecutionHostId {
    state
        .settings
        .pending_workspace_default_execution_host_id
        .clone()
        .or_else(|| {
            state
                .settings
                .workspace_settings_target
                .and_then(|ws_idx| state.workspaces.get(ws_idx))
                .map(|workspace| workspace.default_location.execution_host_id.clone())
        })
        .unwrap_or_else(crate::execution_host::ExecutionHostId::local)
}

fn cycle_default_host(state: &mut AppState, workspace: bool) {
    if workspace {
        let current = pending_workspace_default_host(state);
        let mut choices = vec![crate::execution_host::ExecutionHostId::local()];
        choices.extend(
            state
                .ssh_connection_profiles
                .iter()
                .map(|profile| profile.execution_host_id()),
        );
        let next = choices
            .iter()
            .position(|host| host == &current)
            .map_or(0, |index| (index + 1) % choices.len());
        state.settings.pending_workspace_default_execution_host_id = choices.get(next).cloned();
        return;
    }
    let mut host = pending_group_default_host(state);
    let mut directory = pending_group_default_directory(state);
    super::apply_group_host_cycle(&state.ssh_connection_profiles, &mut host, &mut directory);
    state.settings.pending_group_default_execution_host_id = Some(host);
    state.settings.pending_group_default_directory = Some(directory);
}

fn set_pending_group_default_directory(state: &mut AppState, default_directory: String) {
    state.settings.pending_group_default_directory = Some(default_directory);
}

fn pending_group_github_organization(state: &AppState) -> String {
    state
        .settings
        .pending_group_github_organization
        .clone()
        .or_else(|| {
            state
                .settings
                .group_settings_target
                .and_then(|group_idx| state.groups.get(group_idx))
                .and_then(|group| group.github_organization.as_ref())
                .map(|organization| organization.as_str().to_string())
        })
        .unwrap_or_default()
}

fn pending_group_field(
    state: &AppState,
    section: SettingsSection,
    selected: usize,
) -> Option<String> {
    match section {
        SettingsSection::GroupGeneral if selected == GROUP_GENERAL_NAME => {
            Some(pending_group_name(state))
        }
        SettingsSection::GroupDefaults if selected == GROUP_DEFAULTS_DIRECTORY => {
            Some(pending_group_default_directory(state))
        }
        SettingsSection::GroupGithub if selected == GROUP_GITHUB_ORGANIZATION => {
            Some(pending_group_github_organization(state))
        }
        _ => None,
    }
}

fn set_pending_group_field(
    state: &mut AppState,
    section: SettingsSection,
    selected: usize,
    value: String,
) {
    match section {
        SettingsSection::GroupGeneral if selected == GROUP_GENERAL_NAME => {
            set_pending_group_name(state, value)
        }
        SettingsSection::GroupDefaults if selected == GROUP_DEFAULTS_DIRECTORY => {
            set_pending_group_default_directory(state, value)
        }
        SettingsSection::GroupGithub if selected == GROUP_GITHUB_ORGANIZATION => {
            state.settings.pending_group_github_organization = Some(value)
        }
        _ => {}
    }
}

fn delete_pending_group_field_word(
    state: &mut AppState,
    section: SettingsSection,
    selected: usize,
) {
    let Some(mut value) = pending_group_field(state, section, selected) else {
        return;
    };
    while value.chars().last().is_some_and(char::is_whitespace) {
        value.pop();
    }
    while value.chars().last().is_some_and(|ch| !ch.is_whitespace()) {
        value.pop();
    }
    set_pending_group_field(state, section, selected, value);
}

fn edit_pending_group_field(state: &mut AppState, key: KeyEvent) -> bool {
    let Some(selected) = state.settings.focused_input else {
        return false;
    };
    let section = state.settings.section;
    state.settings.list.select(selected);
    if !settings_row_accepts_text_input(state, selected) {
        return false;
    }

    match key.code {
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            set_pending_group_field(state, section, selected, String::new());
            true
        }
        KeyCode::Backspace if key.modifiers.contains(KeyModifiers::SUPER) => {
            set_pending_group_field(state, section, selected, String::new());
            true
        }
        KeyCode::Backspace
            if key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::ALT) =>
        {
            delete_pending_group_field_word(state, section, selected);
            true
        }
        KeyCode::Char('h' | 'w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            delete_pending_group_field_word(state, section, selected);
            true
        }
        KeyCode::Backspace => {
            let mut value = pending_group_field(state, section, selected).unwrap_or_default();
            value.pop();
            set_pending_group_field(state, section, selected, value);
            true
        }
        KeyCode::Char(c) if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
            let mut value = pending_group_field(state, section, selected).unwrap_or_default();
            value.push(c);
            set_pending_group_field(state, section, selected, value);
            true
        }
        _ => false,
    }
}

fn pending_workspace_name(state: &AppState) -> String {
    state
        .settings
        .pending_workspace_name
        .clone()
        .or_else(|| {
            state
                .settings
                .workspace_settings_target
                .and_then(|ws_idx| state.workspaces.get(ws_idx))
                .map(|workspace| workspace.display_name())
        })
        .unwrap_or_default()
}

fn pending_workspace_default_cwd(state: &AppState) -> String {
    state
        .settings
        .pending_workspace_default_cwd
        .clone()
        .or_else(|| {
            state
                .settings
                .workspace_settings_target
                .and_then(|ws_idx| state.workspaces.get(ws_idx))
                .map(|workspace| {
                    workspace
                        .default_location
                        .path
                        .as_path()
                        .display()
                        .to_string()
                })
        })
        .unwrap_or_default()
}
fn pending_workspace_github_scope(state: &AppState) -> crate::github::GithubRepositoryScope {
    state
        .settings
        .pending_workspace_github_scope
        .clone()
        .or_else(|| {
            state
                .settings
                .workspace_settings_target
                .and_then(|ws_idx| state.workspaces.get(ws_idx))
                .map(|workspace| workspace.github_scope.clone())
        })
        .unwrap_or_default()
}

fn pending_workspace_github_repositories(state: &AppState) -> String {
    state
        .settings
        .pending_workspace_github_repositories
        .clone()
        .or_else(|| match pending_workspace_github_scope(state) {
            crate::github::GithubRepositoryScope::Selected(repositories) => Some(
                repositories
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            crate::github::GithubRepositoryScope::Automatic
            | crate::github::GithubRepositoryScope::GroupOrganization => Some(String::new()),
        })
        .unwrap_or_default()
}

fn set_pending_workspace_field(
    state: &mut AppState,
    section: SettingsSection,
    selected: usize,
    value: String,
) {
    match section {
        SettingsSection::WorkspaceGeneral if selected == WORKSPACE_GENERAL_NAME => {
            state.settings.pending_workspace_name = Some(value)
        }
        SettingsSection::WorkspaceGeneral if selected == WORKSPACE_GENERAL_DIRECTORY => {
            state.settings.pending_workspace_default_cwd = Some(value)
        }
        SettingsSection::WorkspaceGithub if selected == WORKSPACE_GITHUB_REPOSITORIES => {
            state.settings.pending_workspace_github_repositories = Some(value)
        }
        _ => {}
    }
}

fn pending_workspace_field(
    state: &AppState,
    section: SettingsSection,
    selected: usize,
) -> Option<String> {
    match section {
        SettingsSection::WorkspaceGeneral if selected == WORKSPACE_GENERAL_NAME => {
            Some(pending_workspace_name(state))
        }
        SettingsSection::WorkspaceGeneral if selected == WORKSPACE_GENERAL_DIRECTORY => {
            Some(pending_workspace_default_cwd(state))
        }
        SettingsSection::WorkspaceGithub if selected == WORKSPACE_GITHUB_REPOSITORIES => {
            Some(pending_workspace_github_repositories(state))
        }
        _ => None,
    }
}

fn edit_pending_workspace_field(state: &mut AppState, key: KeyEvent) -> bool {
    let Some(selected) = state.settings.focused_input else {
        return false;
    };
    let section = state.settings.section;
    state.settings.list.select(selected);
    if !settings_row_accepts_text_input(state, selected) {
        return false;
    }
    match key.code {
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            set_pending_workspace_field(state, section, selected, String::new());
            true
        }
        KeyCode::Backspace if key.modifiers.contains(KeyModifiers::SUPER) => {
            set_pending_workspace_field(state, section, selected, String::new());
            true
        }
        KeyCode::Backspace => {
            let Some(mut value) = pending_workspace_field(state, section, selected) else {
                return false;
            };
            value.pop();
            set_pending_workspace_field(state, section, selected, value);
            true
        }
        KeyCode::Char(c) if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
            let Some(mut value) = pending_workspace_field(state, section, selected) else {
                return false;
            };
            value.push(c);
            set_pending_workspace_field(state, section, selected, value);
            true
        }
        _ => false,
    }
}

const AGENT_PROFILE_NAME_INDEX: usize = 0;
const AGENT_PROFILE_KIND_START_INDEX: usize = 1;
fn agent_profile_command_index(state: &AppState) -> usize {
    AGENT_PROFILE_KIND_START_INDEX + state.agent_profile_kind_choices().count()
}

fn agent_profile_enabled_index(state: &AppState) -> usize {
    agent_profile_command_index(state) + 1
}

fn agent_profile_save_index(state: &AppState) -> usize {
    agent_profile_command_index(state) + 2
}

fn agent_profile_discard_index(state: &AppState) -> usize {
    agent_profile_command_index(state) + 3
}

fn agent_profile_delete_index(state: &AppState) -> usize {
    agent_profile_command_index(state) + 4
}

fn pending_agent_profile_name(state: &AppState) -> String {
    state
        .settings
        .pending_agent_profile_name
        .clone()
        .unwrap_or_default()
}

fn pending_agent_profile_command(state: &AppState) -> String {
    state
        .settings
        .pending_agent_profile_command
        .clone()
        .unwrap_or_default()
}

fn set_pending_agent_profile_field(state: &mut AppState, selected: usize, value: String) {
    match selected {
        AGENT_PROFILE_NAME_INDEX => state.settings.pending_agent_profile_name = Some(value),
        index if index == agent_profile_command_index(state) => {
            state.settings.pending_agent_profile_command = Some(value)
        }
        _ => {}
    }
}

fn delete_pending_agent_profile_word(state: &mut AppState, selected: usize) {
    let mut value = if selected == AGENT_PROFILE_NAME_INDEX {
        pending_agent_profile_name(state)
    } else if selected == agent_profile_command_index(state) {
        pending_agent_profile_command(state)
    } else {
        return;
    };
    while value.chars().last().is_some_and(char::is_whitespace) {
        value.pop();
    }
    while value.chars().last().is_some_and(|ch| !ch.is_whitespace()) {
        value.pop();
    }
    set_pending_agent_profile_field(state, selected, value);
}

fn edit_pending_agent_profile_text(state: &mut AppState, key: KeyEvent) -> bool {
    let Some(selected) = state.settings.focused_input else {
        return false;
    };
    state.settings.list.select(selected);
    if selected != AGENT_PROFILE_NAME_INDEX && selected != agent_profile_command_index(state) {
        return false;
    }
    match key.code {
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            set_pending_agent_profile_field(state, selected, String::new());
            true
        }
        KeyCode::Backspace if key.modifiers.contains(KeyModifiers::SUPER) => {
            set_pending_agent_profile_field(state, selected, String::new());
            true
        }
        KeyCode::Backspace
            if key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::ALT) =>
        {
            delete_pending_agent_profile_word(state, selected);
            true
        }
        KeyCode::Char('h' | 'w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            delete_pending_agent_profile_word(state, selected);
            true
        }
        KeyCode::Backspace => {
            let mut value = if selected == AGENT_PROFILE_NAME_INDEX {
                pending_agent_profile_name(state)
            } else {
                pending_agent_profile_command(state)
            };
            value.pop();
            set_pending_agent_profile_field(state, selected, value);
            true
        }
        KeyCode::Char(c) if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
            let mut value = if selected == AGENT_PROFILE_NAME_INDEX {
                pending_agent_profile_name(state)
            } else {
                pending_agent_profile_command(state)
            };
            value.push(c);
            set_pending_agent_profile_field(state, selected, value);
            true
        }
        _ => false,
    }
}

fn agent_kind_for_settings_index(
    state: &AppState,
    index: usize,
) -> Option<crate::agent_profiles::AgentKind> {
    let kinds = state.agent_profile_kind_choices().collect::<Vec<_>>();
    let end = AGENT_PROFILE_KIND_START_INDEX + kinds.len();
    (AGENT_PROFILE_KIND_START_INDEX..end)
        .contains(&index)
        .then(|| kinds[index - AGENT_PROFILE_KIND_START_INDEX])
}

fn agent_profile_editor_open(state: &AppState) -> bool {
    state.settings.pending_agent_profile_id.is_some()
        || state.settings.pending_agent_profile_name.is_some()
        || state.settings.pending_agent_profile_command.is_some()
}

fn browse_agent_profile_id_for_settings_index(state: &AppState, selected: usize) -> Option<String> {
    if selected == 0 || agent_profile_editor_open(state) {
        return None;
    }
    state
        .agent_profiles
        .profiles()
        .iter()
        .filter(|profile| !profile.is_system())
        .nth(selected - 1)
        .map(|profile| profile.id.clone())
}

fn custom_profile_id_for_settings_index(state: &AppState, selected: usize) -> Option<String> {
    let profile_id = browse_agent_profile_id_for_settings_index(state, selected)?;
    state
        .agent_profiles
        .get(&profile_id)
        .is_some_and(|profile| !profile.is_system())
        .then_some(profile_id)
}

fn open_blank_agent_profile_editor(state: &mut AppState) {
    state.settings.pending_agent_profile_id = None;
    state.settings.pending_agent_profile_name = Some(String::new());
    let filtered_kind = state.settings.agent_profile_kind_filter;
    let kind = filtered_kind
        .filter(|kind| state.agent_profile_kind_available(*kind))
        .unwrap_or_else(|| state.default_agent_profile_kind_choice());
    state.settings.pending_agent_profile_kind = Some(kind);
    state.settings.pending_agent_profile_command = Some(String::new());
    state.settings.pending_agent_profile_enabled = Some(true);
    state.settings.list.select(AGENT_PROFILE_NAME_INDEX);
    state.settings.focused_input = Some(AGENT_PROFILE_NAME_INDEX);
    state.settings.scroll = 0;
}

fn reset_agent_profile_editor(
    settings: &mut SettingsState,
    default_kind: crate::agent_profiles::AgentKind,
) {
    settings.pending_agent_profile_id = None;
    settings.pending_agent_profile_name = None;
    settings.pending_agent_profile_kind = Some(default_kind);
    settings.pending_agent_profile_command = None;
    settings.pending_agent_profile_enabled = None;
    settings.list.selected = 0;
    settings.focused_input = None;
    settings.list.hide();
    settings.scroll = 0;
}

fn close_agent_profile_editor(state: &mut AppState) {
    let default_kind = state.default_agent_profile_kind_choice();
    reset_agent_profile_editor(&mut state.settings, default_kind);
}

pub(crate) fn close_agent_profile_editor_for_view(state: &AppState, view: &mut ClientViewState) {
    reset_agent_profile_editor(
        &mut view.settings,
        state.default_agent_profile_kind_choice(),
    );
}

fn load_custom_agent_profile_editor(state: &mut AppState, profile_id: &str) -> bool {
    let Some(profile) = state.agent_profiles.get(profile_id) else {
        return false;
    };
    if profile.is_system() {
        return false;
    }
    state.settings.pending_agent_profile_id = Some(profile.id.clone());
    state.settings.pending_agent_profile_name = Some(profile.name.clone());
    let kind = if state.agent_profile_kind_available(profile.kind) {
        profile.kind
    } else {
        crate::agent_profiles::AgentKind::Custom
    };
    state.settings.pending_agent_profile_kind = Some(kind);
    state.settings.pending_agent_profile_command = Some(profile.command.clone());
    state.settings.pending_agent_profile_enabled = Some(profile.enabled);
    state.settings.list.select(AGENT_PROFILE_NAME_INDEX);
    state.settings.focused_input = Some(AGENT_PROFILE_NAME_INDEX);
    true
}

fn slugify_agent_profile_id(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "custom-agent".to_string()
    } else {
        trimmed.to_string()
    }
}

fn next_custom_agent_profile_id(state: &AppState, name: &str) -> String {
    let base = slugify_agent_profile_id(name);
    let mut candidate = base.clone();
    let mut suffix = 2;
    while state
        .agent_profiles
        .get(&format!("user:{candidate}"))
        .is_some()
    {
        candidate = format!("{base}-{suffix}");
        suffix += 1;
    }
    candidate
}

fn save_pending_agent_profile(state: &mut AppState) -> Option<SettingsAction> {
    let name = pending_agent_profile_name(state).trim().to_string();
    let command = pending_agent_profile_command(state).trim().to_string();
    if name.is_empty() || command.is_empty() {
        return None;
    }
    let id = state
        .settings
        .pending_agent_profile_id
        .clone()
        .map(|id| id.trim_start_matches("user:").to_string())
        .unwrap_or_else(|| next_custom_agent_profile_id(state, &name));
    let kind = state
        .settings
        .pending_agent_profile_kind
        .filter(|kind| state.agent_profile_kind_available(*kind))
        .unwrap_or_else(|| state.default_agent_profile_kind_choice());
    let existing_id = format!("user:{id}");
    let env = state
        .agent_profiles
        .get(&existing_id)
        .map(|profile| profile.env.iter().cloned().collect())
        .unwrap_or_default();
    let enabled = state.settings.pending_agent_profile_enabled.unwrap_or(true);
    Some(SettingsAction::SaveAgentProfile(
        crate::agent_profiles::UserAgentProfileConfig {
            id,
            name,
            kind,
            command,
            env,
            enabled,
        },
    ))
}

fn selected_agent_profile_action(state: &mut AppState) -> Option<SettingsAction> {
    if !settings_selection_active(state) {
        return None;
    }
    let selected = state.settings.list.selected;
    if agent_profile_editor_open(state) {
        if let Some(kind) = agent_kind_for_settings_index(state, selected) {
            state.settings.pending_agent_profile_kind = Some(kind);
            return None;
        }
        return match selected {
            index if index == agent_profile_enabled_index(state) => {
                let enabled = state.settings.pending_agent_profile_enabled.unwrap_or(true);
                state.settings.pending_agent_profile_enabled = Some(!enabled);
                None
            }
            index if index == agent_profile_discard_index(state) => {
                close_agent_profile_editor(state);
                None
            }
            index if index == agent_profile_save_index(state) => save_pending_agent_profile(state),
            index if index == agent_profile_delete_index(state) => state
                .settings
                .pending_agent_profile_id
                .clone()
                .map(SettingsAction::DeleteAgentProfile),
            _ => None,
        };
    }

    if selected == 0 {
        open_blank_agent_profile_editor(state);
        return None;
    }
    let profile_id = browse_agent_profile_id_for_settings_index(state, selected)?;
    if load_custom_agent_profile_editor(state, &profile_id) {
        return None;
    }
    None
}

fn connection_editor_open(state: &AppState) -> bool {
    settings_connection_editor_open(&state.settings)
}

fn connection_editor(state: &AppState) -> Option<&crate::app::state::ConnectionEditorState> {
    state.settings.connection_editor.as_ref()
}

fn connection_editor_mut(
    state: &mut AppState,
) -> Option<&mut crate::app::state::ConnectionEditorState> {
    state.settings.connection_editor.as_mut()
}

fn pending_connection_name(state: &AppState) -> String {
    connection_editor(state)
        .map(|editor| editor.draft.name.clone())
        .unwrap_or_default()
}

fn pending_connection_target(state: &AppState) -> String {
    connection_editor(state)
        .map(|editor| editor.draft.target.clone())
        .unwrap_or_default()
}

fn pending_connection_directory(state: &AppState) -> String {
    connection_editor(state)
        .map(|editor| editor.draft.directory.clone())
        .unwrap_or_default()
}

fn set_pending_connection_field(state: &mut AppState, selected: usize, value: String) {
    let Some(editor) = connection_editor_mut(state) else {
        return;
    };
    match crate::settings_rows::ConnectionRowId::from_selection_index(selected) {
        Some(crate::settings_rows::ConnectionRowId::Field(
            crate::settings_rows::ConnectionField::Name,
        )) => editor.draft.name = value,
        Some(crate::settings_rows::ConnectionRowId::Field(
            crate::settings_rows::ConnectionField::Target,
        )) => editor.draft.target = value,
        Some(crate::settings_rows::ConnectionRowId::Field(
            crate::settings_rows::ConnectionField::Directory,
        )) => editor.draft.directory = value,
        _ => {}
    }
}

fn delete_pending_connection_word(state: &mut AppState, selected: usize) {
    let mut value = match crate::settings_rows::ConnectionRowId::from_selection_index(selected) {
        Some(crate::settings_rows::ConnectionRowId::Field(
            crate::settings_rows::ConnectionField::Name,
        )) => pending_connection_name(state),
        Some(crate::settings_rows::ConnectionRowId::Field(
            crate::settings_rows::ConnectionField::Target,
        )) => pending_connection_target(state),
        Some(crate::settings_rows::ConnectionRowId::Field(
            crate::settings_rows::ConnectionField::Directory,
        )) => pending_connection_directory(state),
        _ => return,
    };
    while value.chars().last().is_some_and(char::is_whitespace) {
        value.pop();
    }
    while value.chars().last().is_some_and(|ch| !ch.is_whitespace()) {
        value.pop();
    }
    set_pending_connection_field(state, selected, value);
}

fn edit_pending_connection_text(state: &mut AppState, key: KeyEvent) -> bool {
    let Some(selected) = state.settings.focused_input else {
        return false;
    };
    if !matches!(
        crate::settings_rows::ConnectionRowId::from_selection_index(selected),
        Some(crate::settings_rows::ConnectionRowId::Field(_))
    ) {
        return false;
    }
    match key.code {
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            set_pending_connection_field(state, selected, String::new());
            true
        }
        KeyCode::Backspace if key.modifiers.contains(KeyModifiers::SUPER) => {
            set_pending_connection_field(state, selected, String::new());
            true
        }
        KeyCode::Backspace
            if key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::META) =>
        {
            delete_pending_connection_word(state, selected);
            true
        }
        KeyCode::Char('h' | 'w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            delete_pending_connection_word(state, selected);
            true
        }
        KeyCode::Backspace => {
            let mut value =
                match crate::settings_rows::ConnectionRowId::from_selection_index(selected) {
                    Some(crate::settings_rows::ConnectionRowId::Field(
                        crate::settings_rows::ConnectionField::Name,
                    )) => pending_connection_name(state),
                    Some(crate::settings_rows::ConnectionRowId::Field(
                        crate::settings_rows::ConnectionField::Target,
                    )) => pending_connection_target(state),
                    _ => pending_connection_directory(state),
                };
            value.pop();
            set_pending_connection_field(state, selected, value);
            true
        }
        KeyCode::Char(c) if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
            let mut value =
                match crate::settings_rows::ConnectionRowId::from_selection_index(selected) {
                    Some(crate::settings_rows::ConnectionRowId::Field(
                        crate::settings_rows::ConnectionField::Name,
                    )) => pending_connection_name(state),
                    Some(crate::settings_rows::ConnectionRowId::Field(
                        crate::settings_rows::ConnectionField::Target,
                    )) => pending_connection_target(state),
                    _ => pending_connection_directory(state),
                };
            value.push(c);
            set_pending_connection_field(state, selected, value);
            true
        }
        _ => false,
    }
}

fn browse_connection_profile_id_for_index(state: &AppState, selected: usize) -> Option<String> {
    if selected == 0 || connection_editor_open(state) {
        return None;
    }
    state
        .ssh_connection_profiles
        .get(selected - 1)
        .map(|profile| profile.id().to_string())
}

fn open_blank_connection_editor(state: &mut AppState) {
    state.settings.connection_editor = Some(crate::app::state::ConnectionEditorState::new_draft());
    let target_index = ConnectionRowId::Field(ConnectionField::Target).selection_index();
    state.settings.list.select(target_index);
    state.settings.focused_input = Some(target_index);
    state.settings.scroll = 0;
}

fn close_connection_editor(state: &mut AppState) {
    state.settings.connection_editor = None;
    state.settings.list.selected = 0;
    state.settings.focused_input = None;
    clear_settings_selection(state);
    state.settings.scroll = 0;
}
fn back_from_connection_screen(state: &mut AppState) {
    let persisted_draft = state
        .settings
        .connection_editor
        .as_ref()
        .filter(|editor| editor.is_editing())
        .and_then(|editor| editor.profile_id())
        .and_then(|profile_id| {
            state
                .ssh_connection_profiles
                .iter()
                .find(|profile| profile.id() == profile_id)
        })
        .map(|profile| crate::app::state::ConnectionDraft {
            name: profile.name().to_string(),
            target: profile.target().to_string(),
            directory: profile
                .suggested_directory()
                .map(ToString::to_string)
                .unwrap_or_default(),
        });
    if let (Some(editor), Some(draft)) =
        (state.settings.connection_editor.as_mut(), persisted_draft)
    {
        if editor.show_detail() {
            editor.draft = draft;
            let edit_index =
                ConnectionRowId::Action(crate::settings_rows::ConnectionAction::EditDetails)
                    .selection_index();
            state.settings.list.select(edit_index);
            state.settings.focused_input = None;
            state.settings.scroll = 0;
            return;
        }
    }
    close_connection_editor(state);
}

fn load_connection_profile_detail(state: &mut AppState, profile_id: &str) -> bool {
    let Some(profile) = state
        .ssh_connection_profiles
        .iter()
        .find(|profile| profile.id() == profile_id)
    else {
        return false;
    };
    state.settings.connection_editor =
        Some(crate::app::state::ConnectionEditorState::detail_profile(
            profile.id(),
            profile.name(),
            profile.target(),
            profile
                .suggested_directory()
                .map(|directory| directory.to_string())
                .unwrap_or_default(),
        ));
    let toggle_index =
        ConnectionRowId::Action(crate::settings_rows::ConnectionAction::Toggle).selection_index();
    state.settings.list.select(toggle_index);
    state.settings.focused_input = None;
    state.settings.scroll = 0;
    true
}

/// Readable, id-safe slug for a connection profile display name.
fn slugify_connection_profile_id(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    // Leave room for the `ssh:` prefix, generation suffix, and numeric id suffix.
    let capped: String = trimmed.chars().take(48).collect();
    let capped = capped.trim_matches('-');
    if capped.is_empty() {
        "connection".to_string()
    } else {
        capped.to_string()
    }
}

/// Stable collision-free profile id: readable slug plus a deterministic
/// numeric suffix probed against the current catalog.
fn next_connection_profile_id(state: &AppState, name: &str) -> String {
    let base = slugify_connection_profile_id(name);
    let mut candidate = base.clone();
    let mut suffix = 2;
    while state
        .ssh_connection_profiles
        .iter()
        .any(|profile| profile.id() == candidate)
    {
        candidate = format!("{base}-{suffix}");
        suffix += 1;
    }
    candidate
}

fn save_pending_connection_profile(state: &mut AppState) -> Option<SettingsAction> {
    let target = pending_connection_target(state).trim().to_string();
    if target.is_empty() {
        return None;
    }
    let name_input = pending_connection_name(state).trim().to_string();
    let name = if name_input.is_empty() {
        target.clone()
    } else {
        name_input
    };
    let directory_input = pending_connection_directory(state).trim().to_string();
    let suggested_directory = if directory_input.is_empty() {
        None
    } else {
        crate::execution_host::HostPath::new(directory_input).ok()
    };
    let profile_id =
        connection_editor(state).and_then(|editor| editor.profile_id().map(str::to_string));
    let profile = if let Some(id) = profile_id {
        // Editing preserves the stable id; a target change bumps the binding generation.
        let mut profile = match state
            .ssh_connection_profiles
            .iter()
            .find(|profile| profile.id() == id)
            .cloned()
        {
            Some(profile) => profile,
            None => crate::persist::ssh_profiles::SshConnectionProfile::new(
                id,
                name.clone(),
                target.clone(),
                suggested_directory.clone(),
            )
            .ok()?,
        };
        profile.rename(name).ok()?;
        profile.set_suggested_directory(suggested_directory);
        profile.set_target(target).ok()?;
        profile
    } else {
        let id = next_connection_profile_id(state, &name);
        crate::persist::ssh_profiles::SshConnectionProfile::new(
            id,
            name,
            target,
            suggested_directory,
        )
        .ok()?
    };
    close_connection_editor(state);
    Some(SettingsAction::SaveSshConnectionProfile(profile))
}

fn selected_connection_profile_action(state: &mut AppState) -> Option<SettingsAction> {
    if !settings_selection_active(state) {
        return None;
    }
    let selected = state.settings.list.selected;
    if connection_editor_open(state) {
        let row = crate::settings_rows::ConnectionRowId::from_selection_index(selected)?;
        return match row {
            crate::settings_rows::ConnectionRowId::Action(
                crate::settings_rows::ConnectionAction::Discard,
            ) => {
                back_from_connection_screen(state);
                None
            }
            crate::settings_rows::ConnectionRowId::Action(
                crate::settings_rows::ConnectionAction::Save,
            ) => save_pending_connection_profile(state),
            crate::settings_rows::ConnectionRowId::Action(
                crate::settings_rows::ConnectionAction::EditDetails,
            ) => {
                let editor = connection_editor_mut(state)?;
                if editor.start_editing() {
                    let target_index =
                        ConnectionRowId::Field(ConnectionField::Target).selection_index();
                    state.settings.list.select(target_index);
                    state.settings.focused_input = Some(target_index);
                    state.settings.scroll = 0;
                }
                None
            }
            crate::settings_rows::ConnectionRowId::Action(
                crate::settings_rows::ConnectionAction::Delete,
            ) => {
                let editor = connection_editor(state)?;
                let profile_id = editor.profile_id()?.to_string();
                match editor.connection_retirement.as_ref() {
                    None
                    | Some(crate::app::state::ConnectionRetirementState::InventoryPending)
                    | Some(crate::app::state::ConnectionRetirementState::Failed) => {
                        Some(SettingsAction::PreviewSshConnectionRetirement(profile_id))
                    }
                    Some(crate::app::state::ConnectionRetirementState::Review(preview)) => {
                        Some(SettingsAction::ConfirmSshConnectionRetirement {
                            profile_id,
                            preview: preview.clone(),
                        })
                    }
                    Some(crate::app::state::ConnectionRetirementState::LocalForgetRunning)
                    | Some(crate::app::state::ConnectionRetirementState::Running(_)) => None,
                }
            }
            crate::settings_rows::ConnectionRowId::Action(
                crate::settings_rows::ConnectionAction::ForgetConnection,
            ) => {
                let editor = connection_editor(state)?;
                let profile_id = editor.profile_id()?.to_string();
                let plan = state
                    .ssh_connection_profiles
                    .iter()
                    .find(|profile| profile.id() == profile_id)
                    .and_then(|profile| {
                        crate::execution_host::connection_retirement::plan_connection_retirement(
                            &profile.execution_host_id(),
                        )
                        .ok()
                    })?;
                matches!(
                    editor.connection_retirement,
                    Some(crate::app::state::ConnectionRetirementState::Failed)
                )
                .then_some(SettingsAction::ConfirmLocalConnectionForget { profile_id, plan })
            }
            crate::settings_rows::ConnectionRowId::Action(
                crate::settings_rows::ConnectionAction::Test,
            ) => connection_editor(state)
                .and_then(|editor| editor.profile_id().map(str::to_string))
                .map(|profile_id| SettingsAction::TestSshConnection { profile_id }),
            crate::settings_rows::ConnectionRowId::Action(
                crate::settings_rows::ConnectionAction::Toggle,
            ) => {
                let profile_id = connection_editor(state)?.profile_id()?.to_string();
                let profile = state
                    .ssh_connection_profiles
                    .iter()
                    .find(|profile| profile.id() == profile_id)?;
                use crate::execution_host::ConnectionStatus;
                match state.ssh_connection_status(profile) {
                    ConnectionStatus::Disconnected | ConnectionStatus::AuthenticationRequired => {
                        Some(SettingsAction::ConnectSshConnection { profile_id })
                    }
                    ConnectionStatus::Connecting
                    | ConnectionStatus::Connected
                    | ConnectionStatus::Reconnecting { .. } => {
                        Some(SettingsAction::DisconnectSshConnection { profile_id })
                    }
                    ConnectionStatus::Disconnecting => None,
                }
            }
            crate::settings_rows::ConnectionRowId::Action(
                crate::settings_rows::ConnectionAction::LaunchWorkspace,
            ) => {
                let profile_id = connection_editor(state)?.profile_id()?.to_string();
                let profile = state
                    .ssh_connection_profiles
                    .iter()
                    .find(|profile| profile.id() == profile_id)?;
                matches!(
                    state.ssh_connection_status(profile),
                    crate::execution_host::ConnectionStatus::Connected
                )
                .then_some(SettingsAction::LaunchSshWorkspace { profile_id })
            }
            crate::settings_rows::ConnectionRowId::Action(
                crate::settings_rows::ConnectionAction::ForgetTermination { offset },
            ) => {
                let profile_id = connection_editor(state)?.profile_id()?.to_string();
                let tombstone = state
                    .remote_termination_tombstones_for_profile(&profile_id)
                    .into_iter()
                    .nth(offset)?;
                if connection_editor(state)
                    .and_then(|editor| editor.pending_forget_remote_terminal.as_ref())
                    == Some(&tombstone.terminal_id)
                {
                    Some(SettingsAction::ConfirmForgetRemoteTermination {
                        terminal_id: tombstone.terminal_id,
                    })
                } else {
                    Some(SettingsAction::RequestForgetRemoteTermination {
                        terminal_id: tombstone.terminal_id,
                    })
                }
            }
            crate::settings_rows::ConnectionRowId::Field(_) => None,
        };
    }

    if selected == 0 {
        open_blank_connection_editor(state);
        return None;
    }
    let profile_id = browse_connection_profile_id_for_index(state, selected)?;
    if load_connection_profile_detail(state, &profile_id) {
        return None;
    }
    None
}

fn pending_light_theme_name(state: &AppState) -> String {
    state
        .settings
        .pending_light_theme_name
        .clone()
        .unwrap_or_else(|| state.global_light_theme_name.clone())
}

fn pending_dark_theme_name(state: &AppState) -> String {
    state
        .settings
        .pending_dark_theme_name
        .clone()
        .unwrap_or_else(|| state.global_dark_theme_name.clone())
}

fn pending_terminal_light_accent(state: &AppState) -> TerminalAccent {
    state
        .settings
        .pending_terminal_light_accent
        .unwrap_or(state.global_terminal_light_accent)
}

fn pending_terminal_dark_accent(state: &AppState) -> TerminalAccent {
    state
        .settings
        .pending_terminal_dark_accent
        .unwrap_or(state.global_terminal_dark_accent)
}

fn selected_theme_settings_choice(state: &AppState) -> Option<ThemeSettingsChoice> {
    theme_settings_choices(state)
        .get(state.settings.list.selected)
        .copied()
}

fn pending_sound_enabled(state: &AppState) -> bool {
    state
        .settings
        .pending_sound_enabled
        .unwrap_or_else(|| state.sound_enabled())
}

fn pending_toast_delivery(state: &AppState) -> ToastDelivery {
    state
        .settings
        .pending_toast_delivery
        .unwrap_or_else(|| state.toast_delivery())
}

fn pending_default_shell(state: &AppState) -> String {
    state
        .settings
        .pending_default_shell
        .clone()
        .unwrap_or_else(|| state.default_shell.clone())
}

fn pending_shell_mode(state: &AppState) -> ShellModeConfig {
    state
        .settings
        .pending_shell_mode
        .unwrap_or(state.shell_mode)
}

fn pending_version_check(state: &AppState) -> bool {
    state
        .settings
        .pending_version_check
        .unwrap_or(state.update_version_check)
}

fn pending_manifest_check(state: &AppState) -> bool {
    state
        .settings
        .pending_manifest_check
        .unwrap_or(state.update_manifest_check)
}

fn pending_toast_delay(state: &AppState) -> String {
    state
        .settings
        .pending_toast_delay
        .clone()
        .unwrap_or_else(|| state.toast_config.delay_seconds.to_string())
}

fn pending_toast_gardn_position(state: &AppState) -> ToastGardnPosition {
    state
        .settings
        .pending_toast_gardn_position
        .unwrap_or(state.toast_config.gardn.position)
}

fn pending_clipboard_toast_enabled(state: &AppState) -> bool {
    state
        .settings
        .pending_clipboard_toast_enabled
        .unwrap_or(state.toast_config.clipboard.enabled)
}

fn pending_clipboard_toast_position(state: &AppState) -> ToastClipboardPosition {
    state
        .settings
        .pending_clipboard_toast_position
        .unwrap_or(state.toast_config.clipboard.position)
}

fn pending_confirm_close(state: &AppState) -> bool {
    state
        .settings
        .pending_confirm_close
        .unwrap_or_else(|| state.confirm_close_enabled())
}

fn pending_prompt_new_tab_name(state: &AppState) -> bool {
    state
        .settings
        .pending_prompt_new_tab_name
        .unwrap_or_else(|| state.prompt_new_tab_name_enabled())
}
fn pending_show_counters(state: &AppState) -> bool {
    state
        .settings
        .pending_show_counters
        .unwrap_or(state.show_counters)
}

fn pending_pane_borders(state: &AppState) -> bool {
    state
        .settings
        .pending_pane_borders
        .unwrap_or(state.pane_borders)
}

fn pending_pane_scrollbars(state: &AppState) -> bool {
    state
        .settings
        .pending_pane_scrollbars
        .unwrap_or(state.pane_scrollbars)
}

fn pending_pane_gaps(state: &AppState) -> bool {
    state.settings.pending_pane_gaps.unwrap_or(state.pane_gaps)
}

fn pending_hide_tab_bar_when_single_tab(state: &AppState) -> bool {
    state
        .settings
        .pending_hide_tab_bar_when_single_tab
        .unwrap_or(state.hide_tab_bar_when_single_tab)
}

fn pending_copy_on_select(state: &AppState) -> bool {
    state
        .settings
        .pending_copy_on_select
        .unwrap_or(state.copy_on_select)
}

fn pending_prompt_new_workspace_name(state: &AppState) -> bool {
    state
        .settings
        .pending_prompt_new_workspace_name
        .unwrap_or(state.prompt_new_workspace_name)
}

fn pending_right_click_passthrough_modifier(
    state: &AppState,
) -> RightClickPassthroughModifierConfig {
    state
        .settings
        .pending_right_click_passthrough_modifier
        .unwrap_or_else(|| {
            RightClickPassthroughModifierConfig::from_modifiers(
                state.right_click_passthrough_modifiers,
            )
        })
}

fn pending_new_terminal_cwd(state: &AppState) -> NewTerminalCwdConfig {
    state
        .settings
        .pending_new_terminal_cwd
        .clone()
        .unwrap_or_else(|| state.new_terminal_cwd.clone())
}

fn pending_mouse_scroll_lines(state: &AppState) -> usize {
    state
        .settings
        .pending_mouse_scroll_lines
        .unwrap_or(state.mouse_scroll_lines)
}
fn pending_resume_agents_on_restore(state: &AppState) -> bool {
    state
        .settings
        .pending_resume_agents_on_restore
        .unwrap_or(state.resume_agents_on_restore)
}

fn pending_window_title(state: &AppState) -> String {
    state
        .settings
        .pending_window_title
        .clone()
        .unwrap_or_else(|| state.window_title_template.clone())
}

fn pending_headless_cols(state: &AppState) -> String {
    state
        .settings
        .pending_headless_cols
        .clone()
        .unwrap_or_else(|| state.headless_size.0.to_string())
}

fn pending_headless_rows(state: &AppState) -> String {
    state
        .settings
        .pending_headless_rows
        .clone()
        .unwrap_or_else(|| state.headless_size.1.to_string())
}

fn headless_size_action(state: &AppState) -> Option<SettingsAction> {
    let cols = pending_headless_cols(state).parse::<u16>().ok()?;
    let rows = pending_headless_rows(state).parse::<u16>().ok()?;
    (cols > 0 && rows > 0).then_some(SettingsAction::SaveHeadlessSize { cols, rows })
}

#[derive(Clone, Copy)]
enum GeneralTextField {
    WindowTitle,
    HeadlessCols,
    HeadlessRows,
    DefaultShell,
    ToastDelay,
}

fn focused_general_text_field(state: &AppState) -> Option<GeneralTextField> {
    let focused = state.settings.focused_input?;
    let window_title_index = theme_choice_len(state) + 13;
    match state.settings.section {
        SettingsSection::Theme if focused == window_title_index => {
            Some(GeneralTextField::WindowTitle)
        }
        SettingsSection::Experiments
            if focused == AdvancedRowId::HeadlessCols.selection_index() =>
        {
            Some(GeneralTextField::HeadlessCols)
        }
        SettingsSection::Experiments
            if focused == AdvancedRowId::HeadlessRows.selection_index() =>
        {
            Some(GeneralTextField::HeadlessRows)
        }
        SettingsSection::PaneLabels if focused == BehaviorRowId::DefaultShell.selection_index() => {
            Some(GeneralTextField::DefaultShell)
        }
        SettingsSection::Sound if focused == NotificationRowId::ToastDelay.selection_index() => {
            Some(GeneralTextField::ToastDelay)
        }
        _ => None,
    }
}

fn pending_general_text(state: &AppState, field: GeneralTextField) -> String {
    match field {
        GeneralTextField::WindowTitle => pending_window_title(state),
        GeneralTextField::HeadlessCols => pending_headless_cols(state),
        GeneralTextField::HeadlessRows => pending_headless_rows(state),
        GeneralTextField::DefaultShell => pending_default_shell(state),
        GeneralTextField::ToastDelay => pending_toast_delay(state),
    }
}

fn set_pending_general_text(
    state: &mut AppState,
    field: GeneralTextField,
    value: String,
) -> Option<SettingsAction> {
    match field {
        GeneralTextField::WindowTitle => {
            state.settings.pending_window_title = Some(value.clone());
            crate::config::window_title_diagnostics(&value)
                .is_none()
                .then_some(SettingsAction::SaveWindowTitle(value))
        }
        GeneralTextField::HeadlessCols => {
            state.settings.pending_headless_cols = Some(value);
            headless_size_action(state)
        }
        GeneralTextField::HeadlessRows => {
            state.settings.pending_headless_rows = Some(value);
            headless_size_action(state)
        }
        GeneralTextField::DefaultShell => {
            state.settings.pending_default_shell = Some(value.clone());
            Some(SettingsAction::SaveDefaultShell(value))
        }
        GeneralTextField::ToastDelay => {
            state.settings.pending_toast_delay = Some(value.clone());
            value
                .parse::<u64>()
                .ok()
                .filter(|seconds| *seconds <= MAX_TOAST_DELAY_SECONDS)
                .map(SettingsAction::SaveToastDelay)
        }
    }
}

fn edit_pending_general_text(
    state: &mut AppState,
    key: KeyEvent,
) -> Option<Option<SettingsAction>> {
    let field = focused_general_text_field(state)?;
    let mut value = pending_general_text(state, field);
    match key.code {
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => value.clear(),
        KeyCode::Backspace if key.modifiers.contains(KeyModifiers::SUPER) => value.clear(),
        KeyCode::Backspace => {
            value.pop();
        }
        KeyCode::Char(c)
            if key.modifiers.difference(KeyModifiers::SHIFT).is_empty()
                && (matches!(
                    field,
                    GeneralTextField::WindowTitle | GeneralTextField::DefaultShell
                ) || c.is_ascii_digit()) =>
        {
            value.push(c);
        }
        _ => return None,
    }
    Some(set_pending_general_text(state, field, value))
}

pub(crate) fn paste_settings_text(
    state: &mut AppState,
    text: &str,
) -> Option<Option<SettingsAction>> {
    if matches!(
        state.settings.section,
        SettingsSection::GroupGeneral
            | SettingsSection::GroupDefaults
            | SettingsSection::GroupGithub
            | SettingsSection::WorkspaceGeneral
            | SettingsSection::WorkspaceGithub
    ) {
        let section = state.settings.section;
        let field = state.settings.focused_input?;
        let mut value = if matches!(
            section,
            SettingsSection::GroupGeneral
                | SettingsSection::GroupDefaults
                | SettingsSection::GroupGithub
        ) {
            pending_group_field(state, section, field)?
        } else {
            pending_workspace_field(state, section, field)?
        };
        value.extend(text.chars().filter(|character| !character.is_control()));
        if matches!(
            section,
            SettingsSection::GroupGeneral
                | SettingsSection::GroupDefaults
                | SettingsSection::WorkspaceGeneral
        ) {
            if matches!(
                section,
                SettingsSection::GroupGeneral | SettingsSection::GroupDefaults
            ) {
                set_pending_group_field(state, section, field, value);
            } else {
                set_pending_workspace_field(state, section, field, value);
            }
        } else if section == SettingsSection::GroupGithub {
            set_pending_group_field(state, section, field, value);
        } else {
            set_pending_workspace_field(state, section, field, value);
        }
        return Some(match section {
            SettingsSection::GroupGeneral => selected_group_general_action(state),
            SettingsSection::GroupDefaults => selected_group_defaults_action(state),
            SettingsSection::WorkspaceGeneral => selected_workspace_general_action(state),
            SettingsSection::GroupGithub | SettingsSection::WorkspaceGithub => None,
            _ => None,
        });
    }
    let field = focused_general_text_field(state)?;
    let mut value = pending_general_text(state, field);
    let original_len = value.len();
    value.extend(text.chars().filter(|ch| {
        if matches!(
            field,
            GeneralTextField::WindowTitle | GeneralTextField::DefaultShell
        ) {
            !ch.is_control()
        } else {
            ch.is_ascii_digit()
        }
    }));
    if value.len() == original_len {
        return Some(None);
    }
    Some(set_pending_general_text(state, field, value))
}

fn pending_command(state: &AppState, field: CommandField) -> String {
    match field {
        CommandField::Browser => state
            .settings
            .pending_browser_command
            .clone()
            .unwrap_or_else(|| state.browser_command.clone()),
        CommandField::Review => state
            .settings
            .pending_review_command
            .clone()
            .unwrap_or_else(|| state.review_command.clone()),
        CommandField::Editor => state
            .settings
            .pending_editor_command
            .clone()
            .unwrap_or_else(|| state.editor_command.clone()),
    }
}

fn set_pending_command(state: &mut AppState, field: CommandField, value: String) {
    match field {
        CommandField::Browser => state.settings.pending_browser_command = Some(value),
        CommandField::Review => state.settings.pending_review_command = Some(value),
        CommandField::Editor => state.settings.pending_editor_command = Some(value),
    }
}

fn reset_pending_command(state: &mut AppState, field: CommandField) {
    set_pending_command(state, field, field.default_value().to_owned());
}

fn reset_all_pending_commands(state: &mut AppState) {
    let defaults = CommandsConfig::default();
    state.settings.pending_browser_command = Some(defaults.browser);
    state.settings.pending_review_command = Some(defaults.review);
    state.settings.pending_editor_command = Some(defaults.editor);
}

fn command_field_from_index(index: usize) -> Option<CommandField> {
    match CommandRowId::from_selection_index(index) {
        Some(CommandRowId::Field(field)) => Some(field),
        _ => None,
    }
}

fn delete_pending_command_word(state: &mut AppState, field: CommandField) {
    let mut value = pending_command(state, field);
    while value.chars().last().is_some_and(char::is_whitespace) {
        value.pop();
    }
    while value.chars().last().is_some_and(|ch| !ch.is_whitespace()) {
        value.pop();
    }
    set_pending_command(state, field, value);
}

fn edit_pending_command(state: &mut AppState, key: KeyEvent) -> bool {
    let Some(field) = state
        .settings
        .focused_input
        .and_then(command_field_from_index)
    else {
        return false;
    };
    state
        .settings
        .list
        .select(CommandRowId::Field(field).selection_index());
    match key.code {
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            set_pending_command(state, field, String::new());
            true
        }
        KeyCode::Backspace if key.modifiers.contains(KeyModifiers::SUPER) => {
            set_pending_command(state, field, String::new());
            true
        }
        KeyCode::Backspace
            if key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::ALT) =>
        {
            delete_pending_command_word(state, field);
            true
        }
        KeyCode::Char('h' | 'w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            delete_pending_command_word(state, field);
            true
        }
        KeyCode::Backspace => {
            let mut value = pending_command(state, field);
            value.pop();
            set_pending_command(state, field, value);
            true
        }
        KeyCode::Char(c) if key.modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
            let mut value = pending_command(state, field);
            value.push(c);
            set_pending_command(state, field, value);
            true
        }
        _ => false,
    }
}

fn pending_sidebar_width(state: &AppState) -> u16 {
    state
        .settings
        .pending_sidebar_width
        .unwrap_or(state.default_sidebar_width)
}

fn pending_sidebar_min_width(state: &AppState) -> u16 {
    state
        .settings
        .pending_sidebar_min_width
        .unwrap_or(state.sidebar_min_width)
}

fn pending_sidebar_max_width(state: &AppState) -> u16 {
    state
        .settings
        .pending_sidebar_max_width
        .unwrap_or(state.sidebar_max_width)
}

fn pending_sidebar_arrangement(state: &AppState) -> SidebarArrangementConfig {
    state
        .settings
        .pending_sidebar_arrangement
        .unwrap_or(state.sidebar_arrangement)
}
fn pending_context_bar_visibility(state: &AppState) -> ContextBarVisibilityConfig {
    state
        .settings
        .pending_context_bar_visibility
        .unwrap_or(state.context_bar_visibility)
}

fn pending_sidebar_initial_state(state: &AppState) -> SidebarInitialStateConfig {
    state
        .settings
        .pending_sidebar_initial_state
        .unwrap_or(state.sidebar_config.initial_state)
}

fn pending_sidebar_initial_agent_scope(state: &AppState) -> AgentPanelScopeConfig {
    state
        .settings
        .pending_sidebar_initial_agent_scope
        .unwrap_or(state.sidebar_config.initial_agent_scope)
}

fn pending_pane_border_agent_info(state: &AppState) -> PaneBorderAgentInfoConfig {
    state
        .settings
        .pending_pane_border_agent_info
        .unwrap_or_else(|| state.pane_border_agent_info())
}

fn pending_status_indicators(state: &AppState) -> StatusIndicatorStyle {
    state
        .settings
        .pending_status_indicators
        .unwrap_or_else(|| state.status_indicators())
}

fn selected_global_theme_name_for_mode(state: &AppState) -> String {
    match pending_theme_mode(state) {
        ThemeMode::Light => pending_light_theme_name(state),
        ThemeMode::Dark => pending_dark_theme_name(state),
        ThemeMode::System => match state.theme_appearance_for_mode(ThemeMode::System) {
            ThemeAppearance::Light => pending_light_theme_name(state),
            ThemeAppearance::Dark => pending_dark_theme_name(state),
        },
    }
}
fn target_theme_index(state: &AppState) -> usize {
    if pending_uses_system_theme_source(state) {
        0
    } else {
        2 + current_theme_mode_index(pending_theme_mode(state))
    }
}

fn current_theme_mode_index(mode: ThemeMode) -> usize {
    ThemeMode::ALL
        .iter()
        .position(|candidate| *candidate == mode)
        .unwrap_or(0)
}

fn preview_selected_theme(state: &mut AppState) {
    let Some(choice) = selected_theme_settings_choice(state) else {
        return;
    };
    match choice {
        ThemeSettingsChoice::GroupAccent(accent) => {
            state.settings.pending_group_accent_choice = Some(accent);
            preview_group_accent(state, accent);
        }
        ThemeSettingsChoice::SourceSystem => {
            state.settings.pending_theme_mode = Some(ThemeMode::System);
            state.settings.pending_light_theme_name = Some("system".to_string());
            state.settings.pending_dark_theme_name = Some("system".to_string());
            state.settings.list.selected = 0;
            ensure_settings_selection_visible(state);
            let mode = ThemeMode::System;
            let accent = state.terminal_accent_for_mode(mode);
            state.preview_theme_with_mode_and_terminal_accent("system", mode, accent);
        }
        ThemeSettingsChoice::TerminalLightAccent(accent) => {
            state.settings.pending_terminal_light_accent = Some(accent);
            state.preview_theme_with_mode_and_terminal_accent("system", ThemeMode::Light, accent);
        }
        ThemeSettingsChoice::TerminalDarkAccent(accent) => {
            state.settings.pending_terminal_dark_accent = Some(accent);
            state.preview_theme_with_mode_and_terminal_accent("system", ThemeMode::Dark, accent);
        }
        ThemeSettingsChoice::SourceCustom => {
            if normalize_theme_name(&pending_light_theme_name(state)) == "system" {
                state.settings.pending_light_theme_name =
                    Some(crate::app::state::DEFAULT_LIGHT_THEME_NAME.to_string());
            }
            if normalize_theme_name(&pending_dark_theme_name(state)) == "system" {
                state.settings.pending_dark_theme_name =
                    Some(crate::app::state::DEFAULT_DARK_THEME_NAME.to_string());
            }
            state.settings.list.selected = 1;
            ensure_settings_selection_visible(state);
            let mode = pending_theme_mode(state);
            let name = selected_global_theme_name_for_mode(state);
            state.preview_theme_with_mode(&name, mode);
        }
        ThemeSettingsChoice::Mode(mode) => {
            state.settings.pending_theme_mode = Some(mode);
            let next_selected = 2 + current_theme_mode_index(mode);
            state.settings.list.selected = next_selected;
            ensure_settings_selection_visible(state);
            let name = selected_global_theme_name_for_mode(state);
            state.preview_theme_with_mode(&name, mode);
        }
        ThemeSettingsChoice::Theme(choice) => {
            match choice.target {
                ThemeChoiceTarget::Light => {
                    state.settings.pending_light_theme_name = Some(choice.name.to_string());
                }
                ThemeChoiceTarget::Dark => {
                    state.settings.pending_dark_theme_name = Some(choice.name.to_string());
                }
            }
            let mode = pending_theme_mode(state);
            let name = selected_global_theme_name_for_mode(state);
            state.preview_theme_with_mode(&name, mode);
        }
    }
}

fn selected_integration_action(state: &AppState) -> Option<SettingsAction> {
    if !settings_selection_active(state) {
        return None;
    }
    let selected = state.settings.list.selected;
    let has_host_selector = !state.ssh_connection_profiles.is_empty();
    if has_host_selector && selected == 0 {
        return Some(SettingsAction::CycleIntegrationHost);
    }
    let entry_index = selected.saturating_sub(usize::from(has_host_selector));

    if let Some(host_id) = crate::app::integration_host::resolve(state, &state.settings).host_id() {
        let crate::integration::host::HostIntegrationObservation::Ready(snapshot) =
            state.host_integration_observations.get(host_id)?
        else {
            return None;
        };
        let entry = snapshot.entries.get(entry_index)?;
        return integration_action_for_status(
            entry.target,
            entry.state,
            entry.available,
            entry.missing_profile_hooks,
        );
    }

    let recommendation = state.integration_recommendations.get(entry_index)?;
    let missing_profile_hooks = crate::integration::missing_profile_hook_count_for_target(
        recommendation.target,
        &state.agent_profiles,
    );
    integration_action_for_status(
        recommendation.target,
        recommendation.state,
        recommendation.available,
        missing_profile_hooks,
    )
}

fn integration_action_for_status(
    target: crate::api::schema::IntegrationTarget,
    state: crate::integration::IntegrationStatusKind,
    available: bool,
    missing_profile_hooks: usize,
) -> Option<SettingsAction> {
    if state == crate::integration::IntegrationStatusKind::Current {
        return if missing_profile_hooks > 0 {
            Some(SettingsAction::InstallIntegration(target))
        } else {
            Some(SettingsAction::UninstallIntegration(target))
        };
    }
    match state {
        crate::integration::IntegrationStatusKind::Outdated => {
            Some(SettingsAction::InstallIntegration(target))
        }
        crate::integration::IntegrationStatusKind::NotInstalled if available => {
            Some(SettingsAction::InstallIntegration(target))
        }
        crate::integration::IntegrationStatusKind::NotInstalled
        | crate::integration::IntegrationStatusKind::Current => None,
    }
}

fn pending_theme_mode(state: &AppState) -> ThemeMode {
    state
        .settings
        .pending_theme_mode
        .unwrap_or(state.global_theme_mode)
}

fn preview_group_accent(state: &mut AppState, accent: Option<TerminalAccent>) {
    state.palette = state.global_palette.clone();
    state.theme_name = state.global_theme_name.clone();
    if let Some(accent) = accent {
        state.palette.accent = state.global_palette.theme_accent_color(accent);
    }
}
pub(super) fn close_settings(state: &mut AppState) {
    state.settings.original_palette = None;
    state.settings.original_theme = None;
    clear_settings_pending(state);
    super::modal::leave_modal(state);
}

fn selected_group_general_action(state: &mut AppState) -> Option<SettingsAction> {
    if !settings_selection_active(state) {
        return None;
    }
    let group_idx = state.settings.group_settings_target?;
    match state.settings.list.selected {
        GROUP_GENERAL_NAME => {
            let name = pending_group_name(state).trim().to_string();
            (!name.is_empty()).then_some(SettingsAction::SaveGroupName { group_idx, name })
        }
        GROUP_GENERAL_ICON => Some(SettingsAction::SaveGroupIcon {
            group_idx,
            icon: pending_group_icon(state),
        }),
        GROUP_GENERAL_DELETE => {
            close_settings(state);
            Some(SettingsAction::DeleteGroup(group_idx))
        }
        _ => None,
    }
}

fn selected_group_defaults_action(state: &mut AppState) -> Option<SettingsAction> {
    if !settings_selection_active(state) {
        return None;
    }
    let group_idx = state.settings.group_settings_target?;
    match state.settings.list.selected {
        GROUP_DEFAULTS_HOST | GROUP_DEFAULTS_DIRECTORY => {
            Some(SettingsAction::SaveGroupDefaultLocation {
                group_idx,
                default_location: super::group_default_location_for(
                    &state.ssh_connection_profiles,
                    &pending_group_default_host(state),
                    &pending_group_default_directory(state),
                ),
            })
        }
        _ => None,
    }
}

fn selected_group_github_action(state: &mut AppState) -> Option<SettingsAction> {
    if !settings_selection_active(state)
        || state.settings.list.selected != GROUP_GITHUB_ORGANIZATION
    {
        return None;
    }
    let group_idx = state.settings.group_settings_target?;
    let value = pending_group_github_organization(state);
    let organization = match crate::app::state::GithubOrganization::parse(&value) {
        Ok(organization) => organization,
        Err(context) => {
            state.toast = Some(crate::app::state::ToastNotification {
                kind: crate::app::state::ToastKind::NeedsAttention,
                title: "GitHub Organization Not Saved".to_string(),
                context,
                position: None,
                target: None,
            });
            return None;
        }
    };
    Some(SettingsAction::SaveGroupGithubOrganization {
        group_idx,
        organization,
    })
}

fn selected_workspace_general_action(state: &mut AppState) -> Option<SettingsAction> {
    if !settings_selection_active(state) {
        return None;
    }
    let ws_idx = state.settings.workspace_settings_target?;
    match state.settings.list.selected {
        WORKSPACE_GENERAL_NAME => {
            let name = pending_workspace_name(state).trim().to_string();
            (!name.is_empty()).then_some(SettingsAction::SaveWorkspaceName { ws_idx, name })
        }
        WORKSPACE_GENERAL_HOST | WORKSPACE_GENERAL_DIRECTORY => {
            let cwd = pending_workspace_default_cwd(state).trim().to_string();
            let path = crate::execution_host::HostPath::new(cwd).ok()?;
            Some(SettingsAction::SaveWorkspaceDefaultLocation {
                ws_idx,
                location: crate::execution_host::ResourceLocation::new(
                    pending_workspace_default_host(state),
                    path,
                ),
            })
        }
        _ => None,
    }
}

fn selected_workspace_github_action(state: &mut AppState) -> Option<SettingsAction> {
    if !settings_selection_active(state) {
        return None;
    }
    let ws_idx = state.settings.workspace_settings_target?;
    match state.settings.list.selected {
        WORKSPACE_GITHUB_AUTOMATIC => {
            let scope = crate::github::GithubRepositoryScope::Automatic;
            state.settings.pending_workspace_github_scope = Some(scope.clone());
            Some(SettingsAction::SaveWorkspaceGithubScope { ws_idx, scope })
        }
        WORKSPACE_GITHUB_SELECTED | WORKSPACE_GITHUB_REPOSITORIES => {
            let value = pending_workspace_github_repositories(state);
            let scope = match crate::github::GithubRepositoryScope::selected_from_input(&value) {
                Ok(scope) => scope,
                Err(context) => {
                    state.toast = Some(crate::app::state::ToastNotification {
                        kind: crate::app::state::ToastKind::NeedsAttention,
                        title: "GitHub Repositories Not Saved".to_string(),
                        context,
                        position: None,
                        target: None,
                    });
                    return None;
                }
            };
            state.settings.pending_workspace_github_scope = Some(scope.clone());
            state.settings.pending_workspace_github_repositories = Some(
                scope
                    .selected_repositories()
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", "),
            );
            Some(SettingsAction::SaveWorkspaceGithubScope { ws_idx, scope })
        }
        WORKSPACE_GITHUB_GROUP => {
            let scope = crate::github::GithubRepositoryScope::GroupOrganization;
            state.settings.pending_workspace_github_scope = Some(scope.clone());
            Some(SettingsAction::SaveWorkspaceGithubScope { ws_idx, scope })
        }
        _ => None,
    }
}

fn group_profile_id_for_index(state: &AppState, selected: usize) -> Option<String> {
    let group_idx = state.settings.group_settings_target?;
    let favorites = state
        .groups
        .get(group_idx)?
        .favorite_agent_profile_ids
        .as_slice();
    let (favorite, available) = state.agent_profiles.group_sections(favorites);
    favorite
        .into_iter()
        .chain(available)
        .filter(|profile| state.agent_profile_launchable(profile))
        .nth(selected)
        .map(|profile| profile.id.clone())
}

fn toggle_selected_group_profile_favorite(state: &mut AppState) {
    if !settings_selection_active(state) {
        return;
    }
    let Some(group_idx) = state.settings.group_settings_target else {
        return;
    };
    let Some(profile_id) = group_profile_id_for_index(state, state.settings.list.selected) else {
        return;
    };
    state.toggle_group_agent_profile_favorite(group_idx, &profile_id);
}

fn toggle_selected_group_profile_default(state: &mut AppState) {
    if !settings_selection_active(state) {
        return;
    }
    let Some(group_idx) = state.settings.group_settings_target else {
        return;
    };
    let Some(profile_id) = group_profile_id_for_index(state, state.settings.list.selected) else {
        return;
    };
    state.toggle_group_default_agent_profile(group_idx, &profile_id);
}
fn clear_settings_pending(state: &mut AppState) {
    state.settings.pending_theme_name = None;
    state.settings.pending_theme_mode = None;
    state.settings.pending_light_theme_name = None;
    state.settings.pending_dark_theme_name = None;
    state.settings.pending_terminal_light_accent = None;
    state.settings.pending_terminal_dark_accent = None;
    state.settings.pending_sound_enabled = None;
    state.settings.pending_toast_delivery = None;
    state.settings.pending_default_shell = None;
    state.settings.pending_shell_mode = None;
    state.settings.pending_version_check = None;
    state.settings.pending_manifest_check = None;
    state.settings.pending_toast_delay = None;
    state.settings.pending_toast_gardn_position = None;
    state.settings.pending_clipboard_toast_enabled = None;
    state.settings.pending_clipboard_toast_position = None;
    state.settings.pending_confirm_close = None;
    state.settings.pending_prompt_new_tab_name = None;
    state.settings.pending_show_counters = None;
    state.settings.pending_pane_borders = None;
    state.settings.pending_pane_scrollbars = None;
    state.settings.pending_pane_gaps = None;
    state.settings.pending_hide_tab_bar_when_single_tab = None;
    state.settings.pending_copy_on_select = None;
    state.settings.pending_prompt_new_workspace_name = None;
    state.settings.pending_right_click_passthrough_modifier = None;
    state.settings.pending_new_terminal_cwd = None;
    state.settings.pending_mouse_scroll_lines = None;
    state.settings.pending_resume_agents_on_restore = None;
    state.settings.pending_window_title = None;
    state.settings.pending_headless_cols = None;
    state.settings.pending_headless_rows = None;
    state.settings.pending_browser_command = None;
    state.settings.pending_review_command = None;
    state.settings.pending_editor_command = None;
    state.settings.pending_sidebar_width = None;
    state.settings.pending_sidebar_min_width = None;
    state.settings.pending_sidebar_max_width = None;
    state.settings.pending_sidebar_arrangement = None;
    state.settings.pending_context_bar_visibility = None;
    state.settings.pending_sidebar_initial_state = None;
    state.settings.pending_sidebar_initial_agent_scope = None;
    state.settings.pending_pane_border_agent_info = None;
    state.settings.pending_status_indicators = None;
    state.settings.pending_switch_ascii_input_source_in_prefix = None;
    state.settings.pending_group_accent_choice = None;
    state.settings.pending_group_name = None;
    state.settings.pending_group_icon = None;
    state.settings.pending_group_github_organization = None;
    state.settings.group_icon_picker_open = false;
    state.settings.pending_group_default_directory = None;

    state.settings.pending_workspace_name = None;
    state.settings.pending_workspace_default_cwd = None;
    state.settings.pending_workspace_github_scope = None;
    state.settings.pending_workspace_github_repositories = None;
    state.settings.pending_agent_profile_id = None;
    state.settings.pending_agent_profile_name = None;
    state.settings.pending_agent_profile_kind = None;
    state.settings.pending_agent_profile_command = None;
    state.settings.pending_agent_profile_enabled = None;
    state.settings.connection_editor = None;
    state.settings.group_settings_target = None;
    state.settings.workspace_settings_target = None;
}
fn current_settings_action(state: &AppState) -> SettingsAction {
    SettingsAction::SaveSettings {
        light: pending_light_theme_name(state),
        dark: pending_dark_theme_name(state),
        mode: pending_theme_mode(state),
        terminal_light_accent: pending_terminal_light_accent(state),
        terminal_dark_accent: pending_terminal_dark_accent(state),
        sound_enabled: pending_sound_enabled(state),
        toast_delivery: pending_toast_delivery(state),
        confirm_close: pending_confirm_close(state),
        prompt_new_tab_name: pending_prompt_new_tab_name(state),
        show_counters: pending_show_counters(state),
        pane_borders: pending_pane_borders(state),
        pane_scrollbars: pending_pane_scrollbars(state),
        pane_gaps: pending_pane_gaps(state),
        hide_tab_bar_when_single_tab: pending_hide_tab_bar_when_single_tab(state),
        copy_on_select: pending_copy_on_select(state),
        prompt_new_workspace_name: pending_prompt_new_workspace_name(state),
        right_click_passthrough_modifier: pending_right_click_passthrough_modifier(state),
        new_terminal_cwd: pending_new_terminal_cwd(state),
        mouse_scroll_lines: pending_mouse_scroll_lines(state),
        browser_command: pending_command(state, CommandField::Browser),
        review_command: pending_command(state, CommandField::Review),
        editor_command: pending_command(state, CommandField::Editor),
        sidebar_width: pending_sidebar_width(state),
        sidebar_min_width: pending_sidebar_min_width(state),
        sidebar_max_width: pending_sidebar_max_width(state),
        sidebar_arrangement: pending_sidebar_arrangement(state),
        context_bar_visibility: pending_context_bar_visibility(state),
        sidebar_initial_state: pending_sidebar_initial_state(state),
        sidebar_initial_agent_scope: pending_sidebar_initial_agent_scope(state),
        pane_border_agent_info: pending_pane_border_agent_info(state),
        status_indicators: pending_status_indicators(state),
    }
}

fn current_settings_or_group_accent_action(state: &AppState) -> SettingsAction {
    if let Some(group_idx) = state.settings.group_settings_target {
        SettingsAction::SaveGroupAccent {
            group_idx,
            accent: checked_group_accent_choice(state),
        }
    } else {
        current_settings_action(state)
    }
}

fn next_terminal_cwd_policy(policy: NewTerminalCwdConfig) -> NewTerminalCwdConfig {
    match policy {
        NewTerminalCwdConfig::Follow => NewTerminalCwdConfig::Home,
        NewTerminalCwdConfig::Home => NewTerminalCwdConfig::Current,
        NewTerminalCwdConfig::Current | NewTerminalCwdConfig::Path(_) => {
            NewTerminalCwdConfig::Follow
        }
    }
}

fn next_shell_mode(mode: ShellModeConfig) -> ShellModeConfig {
    match mode {
        ShellModeConfig::Auto => ShellModeConfig::Login,
        ShellModeConfig::Login => ShellModeConfig::NonLogin,
        ShellModeConfig::NonLogin => ShellModeConfig::Auto,
    }
}

fn next_toast_gardn_position(position: ToastGardnPosition) -> ToastGardnPosition {
    match position {
        ToastGardnPosition::TopLeft => ToastGardnPosition::TopRight,
        ToastGardnPosition::TopRight => ToastGardnPosition::BottomLeft,
        ToastGardnPosition::BottomLeft => ToastGardnPosition::BottomRight,
        ToastGardnPosition::BottomRight => ToastGardnPosition::TopLeft,
    }
}

fn next_toast_clipboard_position(position: ToastClipboardPosition) -> ToastClipboardPosition {
    match position {
        ToastClipboardPosition::TopLeft => ToastClipboardPosition::TopCenter,
        ToastClipboardPosition::TopCenter => ToastClipboardPosition::TopRight,
        ToastClipboardPosition::TopRight => ToastClipboardPosition::BottomLeft,
        ToastClipboardPosition::BottomLeft => ToastClipboardPosition::BottomCenter,
        ToastClipboardPosition::BottomCenter => ToastClipboardPosition::BottomRight,
        ToastClipboardPosition::BottomRight => ToastClipboardPosition::TopLeft,
    }
}

fn next_mouse_scroll_lines(lines: usize) -> usize {
    match lines {
        0 | 1 => 3,
        2 | 3 => 5,
        4 | 5 => 10,
        _ => 1,
    }
}

fn select_pending_layout_setting(state: &mut AppState) {
    select_pending_layout_setting_at(state, state.settings.list.selected);
}

fn select_pending_layout_setting_at(state: &mut AppState, selected: usize) {
    match selected {
        0 => {
            let min = pending_sidebar_min_width(state);
            let max = pending_sidebar_max_width(state);
            let current = pending_sidebar_width(state).clamp(min, max);
            let next = current.saturating_add(2);
            state.settings.pending_sidebar_width = Some(if next > max { min } else { next });
        }
        1 => {
            let max = pending_sidebar_max_width(state);
            let current = pending_sidebar_min_width(state);
            let next = current.saturating_add(2);
            let next = if next >= max {
                10.min(max)
            } else {
                next.max(10)
            };
            state.settings.pending_sidebar_min_width = Some(next);
            state.settings.pending_sidebar_width = Some(pending_sidebar_width(state).max(next));
        }
        2 => {
            let min = pending_sidebar_min_width(state);
            let current = pending_sidebar_max_width(state);
            let next = current.saturating_add(2);
            let next = if next > 48 {
                min.max(24)
            } else {
                next.max(min)
            };
            state.settings.pending_sidebar_max_width = Some(next);
            state.settings.pending_sidebar_width = Some(pending_sidebar_width(state).min(next));
        }
        3 => {
            state.settings.pending_sidebar_arrangement =
                Some(pending_sidebar_arrangement(state).next());
        }
        4 => {
            state.settings.pending_context_bar_visibility =
                Some(pending_context_bar_visibility(state).next());
        }
        5 => {
            state.settings.pending_sidebar_initial_state =
                Some(pending_sidebar_initial_state(state).next());
        }
        6 => {
            state.settings.pending_sidebar_initial_agent_scope =
                Some(pending_sidebar_initial_agent_scope(state).next());
        }
        _ => {}
    }
}

fn select_pending_appearance_setting(state: &mut AppState) -> Option<SettingsAction> {
    let selected = state.settings.list.selected;
    let theme_count = theme_choice_len(state);
    if state.settings.group_settings_target.is_some() || selected < theme_count {
        preview_selected_theme(state);
        return Some(current_settings_or_group_accent_action(state));
    }

    let appearance_selected = selected - theme_count;
    match appearance_selected {
        0..=6 => select_pending_layout_setting_at(state, appearance_selected),
        7 => state.settings.pending_pane_borders = Some(!pending_pane_borders(state)),
        8 => state.settings.pending_pane_scrollbars = Some(!pending_pane_scrollbars(state)),
        9 => state.settings.pending_pane_gaps = Some(!pending_pane_gaps(state)),
        10 => {
            state.settings.pending_hide_tab_bar_when_single_tab =
                Some(!pending_hide_tab_bar_when_single_tab(state))
        }
        11 => {
            state.settings.pending_pane_border_agent_info =
                Some(pending_pane_border_agent_info(state).next())
        }
        12 => {
            state.settings.pending_status_indicators = Some(pending_status_indicators(state).next())
        }
        13 => {}
        _ => {}
    }
    Some(current_settings_action(state))
}

fn select_pending_notification_setting(state: &mut AppState) -> Option<SettingsAction> {
    match NotificationRowId::from_selection_index(state.settings.list.selected) {
        Some(NotificationRowId::SoundAlerts) => {
            state.settings.pending_sound_enabled = Some(!pending_sound_enabled(state));
            Some(current_settings_action(state))
        }
        Some(NotificationRowId::ToastDelivery) => {
            state.settings.pending_toast_delivery =
                Some(next_toast_delivery(pending_toast_delivery(state)));
            Some(current_settings_action(state))
        }
        Some(NotificationRowId::ToastDelay) => None,
        Some(NotificationRowId::ToastGardnPosition) => {
            let next = next_toast_gardn_position(pending_toast_gardn_position(state));
            state.settings.pending_toast_gardn_position = Some(next);
            Some(SettingsAction::SaveToastGardnPosition(next))
        }
        Some(NotificationRowId::ClipboardEnabled) => {
            let next = !pending_clipboard_toast_enabled(state);
            state.settings.pending_clipboard_toast_enabled = Some(next);
            Some(SettingsAction::SaveClipboardToastEnabled(next))
        }
        Some(NotificationRowId::ClipboardPosition) => {
            let next = next_toast_clipboard_position(pending_clipboard_toast_position(state));
            state.settings.pending_clipboard_toast_position = Some(next);
            Some(SettingsAction::SaveClipboardToastPosition(next))
        }
        None => None,
    }
}

fn settings_selection_active(state: &AppState) -> bool {
    state.settings.list.is_engaged()
}

fn clear_settings_selection(state: &mut AppState) {
    state.settings.list.hide();
    state.settings.focused_input = None;
}

fn switch_settings_section(state: &mut AppState, section: SettingsSection, selected: usize) {
    state.settings.section = section;
    state.settings.list.selected = selected;
    state.settings.scroll = 0;
    state.settings.group_icon_picker_open = false;
    clear_settings_selection(state);
}

fn general_settings_section_selection(state: &AppState, section: SettingsSection) -> usize {
    match section {
        SettingsSection::Theme => target_theme_index(state),
        SettingsSection::Layout
        | SettingsSection::Sound
        | SettingsSection::Toast
        | SettingsSection::PaneLabels
        | SettingsSection::Commands
        | SettingsSection::Experiments
        | SettingsSection::Agents
        | SettingsSection::Integrations
        | SettingsSection::Connections
        | SettingsSection::GroupProfiles
        | SettingsSection::GroupGeneral
        | SettingsSection::GroupDefaults
        | SettingsSection::GroupGithub
        | SettingsSection::WorkspaceGeneral
        | SettingsSection::WorkspaceGithub
        | SettingsSection::About => 0,
    }
}

fn select_general_settings_section(state: &mut AppState, section: SettingsSection) {
    let selected = general_settings_section_selection(state, section);
    switch_settings_section(state, section, selected);
    if section == SettingsSection::Theme {
        ensure_settings_selection_visible(state);
    }
}

fn settings_row_option_index(row: &SettingsListRow) -> Option<usize> {
    match row {
        SettingsListRow::Toggle { index, .. }
        | SettingsListRow::Value { index, .. }
        | SettingsListRow::TextInput { index, .. }
        | SettingsListRow::Choice { index, .. }
        | SettingsListRow::Action { index, .. }
        | SettingsListRow::Status { index, .. }
        | SettingsListRow::Profile { index, .. } => Some(*index),
        SettingsListRow::Header(_)
        | SettingsListRow::Caption(_)
        | SettingsListRow::Spacer
        | SettingsListRow::GroupIconPicker => None,
    }
}

fn select_general_settings_subsection(
    state: &mut AppState,
    section: SettingsSection,
    subsection: usize,
) {
    select_general_settings_section(state, section);
    let anchor = crate::ui::settings_subsection_anchor(section, subsection);
    let Some(rows) = rows_for_section(state, section) else {
        return;
    };
    let header_index = anchor
        .and_then(|anchor| {
            rows.iter()
                .position(|row| matches!(row, SettingsListRow::Header(label) if *label == anchor))
        })
        .unwrap_or(0);
    state.settings.scroll = visual_row_count(&rows[..header_index]);
    if let Some(selected) = rows[header_index..]
        .iter()
        .find_map(settings_row_option_index)
    {
        state.settings.list.select(selected);
        focus_selected_settings_input(state);
    }
}

fn activate_settings_sidebar_entry(state: &mut AppState, entry: crate::ui::SettingsSidebarEntry) {
    state.settings.sidebar_selection = SettingsSidebarSelection {
        section: entry.section,
        subsection: entry.subsection,
    };
    if let Some(subsection) = entry.subsection {
        state.settings.sidebar_expanded = Some(entry.section);
        select_general_settings_subsection(state, entry.section, subsection);
        return;
    }

    let collapse = state.settings.sidebar_expanded == Some(entry.section)
        && state.settings.section == entry.section;
    state.settings.sidebar_expanded = (!collapse).then_some(entry.section);
    select_general_settings_section(state, entry.section);
}

fn move_settings_sidebar_selection(state: &mut AppState, forward: bool) {
    let entries = crate::ui::settings_sidebar_entries(&state.settings);
    let selected = entries
        .iter()
        .position(|entry| {
            entry.section == state.settings.sidebar_selection.section
                && entry.subsection == state.settings.sidebar_selection.subsection
        })
        .unwrap_or(0);
    let next = if forward {
        (selected + 1).min(entries.len().saturating_sub(1))
    } else {
        selected.saturating_sub(1)
    };
    if let Some(entry) = entries.get(next) {
        state.settings.sidebar_selection = SettingsSidebarSelection {
            section: entry.section,
            subsection: entry.subsection,
        };
    }
}

fn update_settings_sidebar_state(state: &mut AppState, key: KeyEvent) -> Option<SettingsAction> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => move_settings_sidebar_selection(state, false),
        KeyCode::Down | KeyCode::Char('j') => move_settings_sidebar_selection(state, true),
        KeyCode::Left | KeyCode::Char('h') => {
            if state.settings.sidebar_selection.subsection.is_some() {
                state.settings.sidebar_selection.subsection = None;
            } else if state.settings.sidebar_expanded
                == Some(state.settings.sidebar_selection.section)
            {
                state.settings.sidebar_expanded = None;
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            let section = state.settings.sidebar_selection.section;
            state.settings.sidebar_expanded = Some(section);
            let entries = crate::ui::settings_sidebar_entries(&state.settings);
            state.settings.sidebar_selection.subsection = entries
                .iter()
                .find(|entry| entry.section == section && entry.subsection.is_some())
                .and_then(|entry| entry.subsection);
            select_general_settings_section(state, section);
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            let selection = state.settings.sidebar_selection;
            activate_settings_sidebar_entry(
                state,
                crate::ui::SettingsSidebarEntry {
                    section: selection.section,
                    subsection: selection.subsection,
                    label: "",
                },
            );
        }
        KeyCode::Esc => return handle_settings_modal_action(state, &key),
        _ => {}
    }
    None
}

fn select_pending_setting(state: &mut AppState) -> Option<SettingsAction> {
    if !settings_selection_active(state) {
        return None;
    }
    match state.settings.section {
        SettingsSection::Theme => select_pending_appearance_setting(state),
        SettingsSection::Layout => {
            select_pending_layout_setting(state);
            Some(current_settings_action(state))
        }
        SettingsSection::Sound => select_pending_notification_setting(state),
        SettingsSection::Toast => {
            state.settings.pending_toast_delivery =
                Some(next_toast_delivery(pending_toast_delivery(state)));
            Some(current_settings_action(state))
        }
        SettingsSection::PaneLabels => {
            match BehaviorRowId::from_selection_index(state.settings.list.selected) {
                Some(BehaviorRowId::ConfirmClose) => {
                    state.settings.pending_confirm_close = Some(!pending_confirm_close(state));
                    Some(current_settings_action(state))
                }
                Some(BehaviorRowId::NameNewTabs) => {
                    state.settings.pending_prompt_new_tab_name =
                        Some(!pending_prompt_new_tab_name(state));
                    Some(current_settings_action(state))
                }
                Some(BehaviorRowId::NameNewWorkspaces) => {
                    state.settings.pending_prompt_new_workspace_name =
                        Some(!pending_prompt_new_workspace_name(state));
                    Some(current_settings_action(state))
                }
                Some(BehaviorRowId::ShowCounters) => {
                    state.settings.pending_show_counters = Some(!pending_show_counters(state));
                    Some(current_settings_action(state))
                }
                Some(BehaviorRowId::CopyOnSelect) => {
                    state.settings.pending_copy_on_select = Some(!pending_copy_on_select(state));
                    Some(current_settings_action(state))
                }
                Some(BehaviorRowId::RightClickPassthrough) => {
                    state.settings.pending_right_click_passthrough_modifier =
                        Some(pending_right_click_passthrough_modifier(state).next());
                    Some(current_settings_action(state))
                }
                Some(BehaviorRowId::DefaultShell) => None,
                Some(BehaviorRowId::ShellMode) => {
                    let next = next_shell_mode(pending_shell_mode(state));
                    state.settings.pending_shell_mode = Some(next);
                    Some(SettingsAction::SaveShellMode(next))
                }
                Some(BehaviorRowId::NewTerminalCwd) => {
                    let next = next_terminal_cwd_policy(pending_new_terminal_cwd(state));
                    state.settings.pending_new_terminal_cwd = Some(next);
                    Some(current_settings_action(state))
                }
                Some(BehaviorRowId::MouseWheelSpeed) => {
                    let next = next_mouse_scroll_lines(pending_mouse_scroll_lines(state));
                    state.settings.pending_mouse_scroll_lines = Some(next);
                    Some(current_settings_action(state))
                }
                Some(BehaviorRowId::ResumeAgents) => {
                    state.settings.pending_resume_agents_on_restore =
                        Some(!pending_resume_agents_on_restore(state));
                    Some(SettingsAction::SaveResumeAgentsOnRestore(
                        pending_resume_agents_on_restore(state),
                    ))
                }
                None => None,
            }
        }
        SettingsSection::Commands => selected_command_action(state),
        SettingsSection::Experiments => selected_experiment_action(state),
        SettingsSection::Agents => selected_agent_profile_action(state),
        SettingsSection::Integrations => selected_integration_action(state),
        SettingsSection::Connections => selected_connection_profile_action(state),
        SettingsSection::GroupGeneral => selected_group_general_action(state),
        SettingsSection::GroupDefaults => selected_group_defaults_action(state),
        SettingsSection::GroupGithub => selected_group_github_action(state),
        SettingsSection::GroupProfiles => None,
        SettingsSection::WorkspaceGeneral => selected_workspace_general_action(state),
        SettingsSection::WorkspaceGithub => selected_workspace_github_action(state),
        SettingsSection::About => None,
    }
}

fn selected_experiment_action(state: &mut AppState) -> Option<SettingsAction> {
    if !settings_selection_active(state) {
        return None;
    }
    match AdvancedRowId::from_selection_index(state.settings.list.selected) {
        Some(AdvancedRowId::SwitchAscii) => {
            Some(SettingsAction::SaveSwitchAsciiInputSourceInPrefix(
                !state.switch_ascii_input_source_in_prefix_enabled(),
            ))
        }
        Some(AdvancedRowId::KittyGraphics) => Some(SettingsAction::SaveKittyGraphics(
            !state.kitty_graphics_enabled,
        )),
        Some(AdvancedRowId::HeadlessCols | AdvancedRowId::HeadlessRows) => {
            headless_size_action(state)
        }
        Some(AdvancedRowId::VersionCheck) => {
            let next = !pending_version_check(state);
            state.settings.pending_version_check = Some(next);
            Some(SettingsAction::SaveVersionCheck(next))
        }
        Some(AdvancedRowId::ManifestCheck) => {
            let next = !pending_manifest_check(state);
            state.settings.pending_manifest_check = Some(next);
            Some(SettingsAction::SaveManifestCheck(next))
        }
        None => None,
    }
}
fn selected_command_action(state: &mut AppState) -> Option<SettingsAction> {
    match CommandRowId::from_selection_index(state.settings.list.selected) {
        Some(CommandRowId::Action(CommandAction::Reset(field))) => {
            reset_pending_command(state, field);
        }
        Some(CommandRowId::Action(CommandAction::ResetAll)) => {
            reset_all_pending_commands(state);
        }
        Some(CommandRowId::Field(_)) | None => {}
    }
    Some(current_settings_action(state))
}

fn settings_row_accepts_text_input(state: &AppState, selected: usize) -> bool {
    match state.settings.section {
        SettingsSection::Theme => selected == theme_choice_len(state) + 13,
        SettingsSection::Commands => command_field_from_index(selected).is_some(),
        SettingsSection::Experiments => matches!(
            AdvancedRowId::from_selection_index(selected),
            Some(AdvancedRowId::HeadlessCols | AdvancedRowId::HeadlessRows)
        ),
        SettingsSection::PaneLabels => {
            BehaviorRowId::from_selection_index(selected) == Some(BehaviorRowId::DefaultShell)
        }
        SettingsSection::Sound => {
            NotificationRowId::from_selection_index(selected) == Some(NotificationRowId::ToastDelay)
        }
        SettingsSection::GroupGeneral => selected == GROUP_GENERAL_NAME,
        SettingsSection::GroupDefaults => selected == GROUP_DEFAULTS_DIRECTORY,
        SettingsSection::GroupGithub => selected == GROUP_GITHUB_ORGANIZATION,
        SettingsSection::WorkspaceGeneral => matches!(
            selected,
            WORKSPACE_GENERAL_NAME | WORKSPACE_GENERAL_DIRECTORY
        ),
        SettingsSection::WorkspaceGithub => selected == WORKSPACE_GITHUB_REPOSITORIES,
        SettingsSection::Agents if agent_profile_editor_open(state) => {
            selected == AGENT_PROFILE_NAME_INDEX || selected == agent_profile_command_index(state)
        }
        SettingsSection::Connections if connection_editor_open(state) => {
            matches!(
                crate::settings_rows::ConnectionRowId::from_selection_index(selected),
                Some(crate::settings_rows::ConnectionRowId::Field(_))
            )
        }
        _ => false,
    }
}

fn focus_selected_settings_input(state: &mut AppState) {
    let selected = state.settings.list.selected;
    state.settings.focused_input =
        settings_row_accepts_text_input(state, selected).then_some(selected);
}

fn select_previous_setting(state: &mut AppState, item_count: usize) {
    if item_count == 0 {
        return;
    }

    let had_selection = state.settings.list.restore();
    if let Some(rows) = rows_for_section(state, state.settings.section) {
        let selected = if had_selection {
            previous_option_index(&rows, state.settings.list.selected)
        } else {
            previous_option_index(&rows, usize::MAX)
        };
        if let Some(selected) = selected {
            state.settings.list.select(selected);
        }
        focus_selected_settings_input(state);
        return;
    }

    let selected = state.settings.list.selected.min(item_count - 1);
    state
        .settings
        .list
        .select(if !had_selection || selected == 0 {
            item_count - 1
        } else {
            selected - 1
        });
    focus_selected_settings_input(state);
}

fn select_next_setting(state: &mut AppState, item_count: usize) {
    if item_count == 0 {
        return;
    }

    let had_selection = state.settings.list.restore();
    if let Some(rows) = rows_for_section(state, state.settings.section) {
        let selected = if had_selection {
            next_option_index(&rows, state.settings.list.selected)
        } else {
            next_option_index(&rows, usize::MAX)
        };
        if let Some(selected) = selected {
            state.settings.list.select(selected);
        }
        focus_selected_settings_input(state);
        return;
    }

    let selected = state.settings.list.selected.min(item_count - 1);
    state
        .settings
        .list
        .select(if !had_selection || selected + 1 == item_count {
            0
        } else {
            selected + 1
        });
    focus_selected_settings_input(state);
}

fn handle_settings_modal_action(state: &mut AppState, key: &KeyEvent) -> Option<SettingsAction> {
    match super::modal::modal_action_from_key(key, super::modal::SETTINGS_ACTIONS) {
        Some(super::modal::ModalAction::Close) => {
            close_settings(state);
            None
        }
        _ => None,
    }
}

pub(super) fn update_settings_state(state: &mut AppState, key: KeyEvent) -> Option<SettingsAction> {
    if state.settings.group_settings_target.is_some()
        && !matches!(
            state.settings.section,
            SettingsSection::Theme
                | SettingsSection::GroupGeneral
                | SettingsSection::GroupDefaults
                | SettingsSection::GroupProfiles
                | SettingsSection::GroupGithub
        )
    {
        switch_settings_section(state, SettingsSection::GroupGeneral, 0);
    }
    if state.settings.workspace_settings_target.is_some()
        && !matches!(
            state.settings.section,
            SettingsSection::WorkspaceGeneral | SettingsSection::WorkspaceGithub
        )
    {
        switch_settings_section(state, SettingsSection::WorkspaceGeneral, 0);
    }
    let general_settings = state.settings.group_settings_target.is_none()
        && state.settings.workspace_settings_target.is_none();
    if general_settings && matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
        state.settings.sidebar_focused = !state.settings.sidebar_focused;
        if state.settings.sidebar_focused {
            clear_settings_selection(state);
        } else {
            state.settings.list.restore();
            focus_selected_settings_input(state);
        }
        return None;
    }
    if general_settings && state.settings.sidebar_focused {
        return update_settings_sidebar_state(state, key);
    }

    let section_before_key = state.settings.section;
    if state.settings.section == SettingsSection::Agents
        && agent_profile_editor_open(state)
        && edit_pending_agent_profile_text(state, key)
    {
        return None;
    }
    if state.settings.section == SettingsSection::Commands && edit_pending_command(state, key) {
        return None;
    }
    if state.settings.section == SettingsSection::Connections
        && connection_editor_open(state)
        && edit_pending_connection_text(state, key)
    {
        return None;
    }
    if let Some(action) = edit_pending_general_text(state, key) {
        return action;
    }
    state.settings.list.restore();
    match state.settings.section {
        SettingsSection::Theme => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                select_previous_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Theme),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                select_next_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Theme),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::PageUp => {
                state.settings.scroll = state
                    .settings
                    .scroll
                    .saturating_sub(super::MODAL_PAGE_SCROLL_ROWS as usize);
            }
            KeyCode::PageDown => {
                state.settings.scroll = state
                    .settings
                    .scroll
                    .saturating_add(super::MODAL_PAGE_SCROLL_ROWS as usize)
                    .min(settings_theme_max_scroll(state));
            }
            KeyCode::Char(' ') => return select_pending_setting(state),
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                if state.settings.group_settings_target.is_some() {
                    switch_settings_section(state, SettingsSection::GroupProfiles, 0);
                } else {
                    switch_settings_section(state, SettingsSection::Sound, 0);
                }
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                if state.settings.group_settings_target.is_some() {
                    switch_settings_section(state, SettingsSection::GroupDefaults, 0);
                } else {
                    switch_settings_section(state, SettingsSection::About, 0);
                }
            }
            _ => {
                if let Some(action) = handle_settings_modal_action(state, &key) {
                    return Some(action);
                }
            }
        },
        SettingsSection::Layout => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                select_previous_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Layout),
                );
            }
            KeyCode::Down | KeyCode::Char('j') => {
                select_next_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Layout),
                );
            }
            KeyCode::Char(' ') => return select_pending_setting(state),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                state.settings.section = SettingsSection::Theme;
                state.settings.list.selected = target_theme_index(state);
                ensure_settings_selection_visible(state);
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                state.settings.section = SettingsSection::Sound;
                state.settings.list.selected = usize::from(!pending_sound_enabled(state));
            }
            _ => {
                if let Some(action) = handle_settings_modal_action(state, &key) {
                    return Some(action);
                }
            }
        },
        SettingsSection::Sound => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                select_previous_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Sound),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                select_next_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Sound),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::Enter | KeyCode::Char(' ') => return select_pending_setting(state),
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                switch_settings_section(state, SettingsSection::PaneLabels, 0);
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                switch_settings_section(state, SettingsSection::Theme, target_theme_index(state));
                ensure_settings_selection_visible(state);
            }
            _ => {
                if let Some(action) = handle_settings_modal_action(state, &key) {
                    return Some(action);
                }
            }
        },
        SettingsSection::Toast => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                select_previous_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Toast),
                );
            }
            KeyCode::Down | KeyCode::Char('j') => {
                select_next_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Toast),
                );
            }
            KeyCode::Char(' ') => return select_pending_setting(state),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                state.settings.section = SettingsSection::Sound;
                state.settings.list.selected = usize::from(!pending_sound_enabled(state));
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                state.settings.section = SettingsSection::PaneLabels;
                state.settings.list.selected = 0;
            }
            _ => {
                if let Some(action) = handle_settings_modal_action(state, &key) {
                    return Some(action);
                }
            }
        },
        SettingsSection::PaneLabels => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                select_previous_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::PaneLabels),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                select_next_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::PaneLabels),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::Enter => return select_pending_setting(state),
            KeyCode::Char(' ') => return select_pending_setting(state),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                switch_settings_section(state, SettingsSection::Sound, 0);
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                switch_settings_section(state, SettingsSection::Commands, 0);
            }
            _ => {
                if let Some(action) = handle_settings_modal_action(state, &key) {
                    return Some(action);
                }
            }
        },
        SettingsSection::Commands => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                select_previous_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Commands),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                select_next_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Commands),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::Enter | KeyCode::Char(' ') => return select_pending_setting(state),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                switch_settings_section(state, SettingsSection::PaneLabels, 0);
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                switch_settings_section(state, SettingsSection::Agents, 0);
            }
            _ => {
                if let Some(action) = handle_settings_modal_action(state, &key) {
                    return Some(action);
                }
            }
        },
        SettingsSection::Agents => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                select_previous_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Agents),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                select_next_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Agents),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::PageUp => {
                state.settings.scroll = state
                    .settings
                    .scroll
                    .saturating_sub(super::MODAL_PAGE_SCROLL_ROWS as usize);
            }
            KeyCode::PageDown => {
                state.settings.scroll = state
                    .settings
                    .scroll
                    .saturating_add(super::MODAL_PAGE_SCROLL_ROWS as usize)
                    .min(settings_section_max_scroll(state, SettingsSection::Agents));
            }
            KeyCode::Enter => {
                return selected_agent_profile_action(state);
            }
            KeyCode::Char(' ') => {
                return selected_agent_profile_action(state);
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(profile_id) =
                    custom_profile_id_for_settings_index(state, state.settings.list.selected)
                {
                    return Some(SettingsAction::DeleteAgentProfile(profile_id));
                }
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                switch_settings_section(state, SettingsSection::Commands, 0);
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                switch_settings_section(state, SettingsSection::Integrations, 0);
            }
            _ => {
                if let Some(action) = handle_settings_modal_action(state, &key) {
                    return Some(action);
                }
            }
        },
        SettingsSection::Connections => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                select_previous_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Connections),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                select_next_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Connections),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::PageUp => {
                state.settings.scroll = state
                    .settings
                    .scroll
                    .saturating_sub(super::MODAL_PAGE_SCROLL_ROWS as usize);
            }
            KeyCode::PageDown => {
                state.settings.scroll = state
                    .settings
                    .scroll
                    .saturating_add(super::MODAL_PAGE_SCROLL_ROWS as usize)
                    .min(settings_section_max_scroll(
                        state,
                        SettingsSection::Connections,
                    ));
            }
            KeyCode::Enter => {
                return selected_connection_profile_action(state);
            }
            KeyCode::Char(' ') => {
                return selected_connection_profile_action(state);
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(profile_id) =
                    browse_connection_profile_id_for_index(state, state.settings.list.selected)
                {
                    let _ = load_connection_profile_detail(state, &profile_id);
                    state.settings.list.select(
                        crate::settings_rows::ConnectionRowId::Action(
                            crate::settings_rows::ConnectionAction::Delete,
                        )
                        .selection_index(),
                    );
                    return Some(SettingsAction::PreviewSshConnectionRetirement(profile_id));
                }
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                switch_settings_section(state, SettingsSection::Integrations, 0);
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                switch_settings_section(state, SettingsSection::Experiments, 0);
            }
            _ => {
                if let Some(action) = handle_settings_modal_action(state, &key) {
                    return Some(action);
                }
            }
        },
        SettingsSection::Experiments => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                select_previous_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Experiments),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                select_next_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Experiments),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::Enter | KeyCode::Char(' ') => return selected_experiment_action(state),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                switch_settings_section(state, SettingsSection::Connections, 0);
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                switch_settings_section(state, SettingsSection::About, 0);
            }
            _ => {
                if let Some(action) = handle_settings_modal_action(state, &key) {
                    return Some(action);
                }
            }
        },
        SettingsSection::About => match key.code {
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                switch_settings_section(state, SettingsSection::Experiments, 0);
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                switch_settings_section(state, SettingsSection::Theme, target_theme_index(state));
                ensure_settings_selection_visible(state);
            }
            _ => {
                if let Some(action) = handle_settings_modal_action(state, &key) {
                    return Some(action);
                }
            }
        },
        SettingsSection::Integrations => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                select_previous_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Integrations),
                );
            }
            KeyCode::Down | KeyCode::Char('j') => {
                select_next_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::Integrations),
                );
            }
            KeyCode::Enter | KeyCode::Char(' ') => return selected_integration_action(state),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                switch_settings_section(state, SettingsSection::Agents, 0);
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                switch_settings_section(state, SettingsSection::Connections, 0);
            }
            _ => {
                if let Some(action) = handle_settings_modal_action(state, &key) {
                    return Some(action);
                }
            }
        },
        SettingsSection::GroupGeneral => {
            if state.settings.focused_input.is_some() && edit_pending_group_field(state, key) {
                return selected_group_general_action(state);
            }
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    select_previous_setting(
                        state,
                        settings_section_choice_len(state, SettingsSection::GroupGeneral),
                    );
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    select_next_setting(
                        state,
                        settings_section_choice_len(state, SettingsSection::GroupGeneral),
                    );
                }
                KeyCode::Enter if state.settings.list.selected == GROUP_GENERAL_ICON => {
                    toggle_group_icon_picker(state);
                    return None;
                }
                KeyCode::Enter => return selected_group_general_action(state),
                KeyCode::Char(' ') if state.settings.list.selected == GROUP_GENERAL_ICON => {
                    toggle_group_icon_picker(state);
                    return None;
                }
                KeyCode::Char(' ') if state.settings.list.selected == GROUP_GENERAL_DELETE => {
                    return selected_group_general_action(state);
                }
                KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                    switch_settings_section(state, SettingsSection::GroupGithub, 0);
                }
                KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                    switch_settings_section(state, SettingsSection::GroupDefaults, 0);
                }
                _ => {
                    if let Some(action) = handle_settings_modal_action(state, &key) {
                        return Some(action);
                    }
                }
            }
        }
        SettingsSection::GroupDefaults => {
            if state.settings.focused_input.is_some() && edit_pending_group_field(state, key) {
                return selected_group_defaults_action(state);
            }
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    select_previous_setting(
                        state,
                        settings_section_choice_len(state, SettingsSection::GroupDefaults),
                    );
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    select_next_setting(
                        state,
                        settings_section_choice_len(state, SettingsSection::GroupDefaults),
                    );
                }
                KeyCode::Enter | KeyCode::Char(' ')
                    if state.settings.list.selected == GROUP_DEFAULTS_HOST =>
                {
                    cycle_default_host(state, false);
                    return selected_group_defaults_action(state);
                }
                KeyCode::Enter => return selected_group_defaults_action(state),
                KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                    switch_settings_section(state, SettingsSection::GroupGeneral, 0);
                }
                KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                    switch_settings_section(
                        state,
                        SettingsSection::Theme,
                        group_accent_selection_index(state),
                    );
                    ensure_settings_selection_visible(state);
                }
                _ => {
                    if let Some(action) = handle_settings_modal_action(state, &key) {
                        return Some(action);
                    }
                }
            }
        }
        SettingsSection::GroupGithub => {
            if state.settings.focused_input.is_some() && edit_pending_group_field(state, key) {
                return None;
            }
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    select_previous_setting(
                        state,
                        settings_section_choice_len(state, SettingsSection::GroupGithub),
                    );
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    select_next_setting(
                        state,
                        settings_section_choice_len(state, SettingsSection::GroupGithub),
                    );
                }
                KeyCode::Enter => return selected_group_github_action(state),
                KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                    switch_settings_section(state, SettingsSection::GroupProfiles, 0);
                }
                KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                    switch_settings_section(state, SettingsSection::GroupGeneral, 0);
                }
                _ => {
                    if let Some(action) = handle_settings_modal_action(state, &key) {
                        return Some(action);
                    }
                }
            }
        }
        SettingsSection::GroupProfiles => match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                select_previous_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::GroupProfiles),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                select_next_setting(
                    state,
                    settings_section_choice_len(state, SettingsSection::GroupProfiles),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::PageUp => {
                state.settings.scroll = state
                    .settings
                    .scroll
                    .saturating_sub(super::MODAL_PAGE_SCROLL_ROWS as usize);
            }
            KeyCode::PageDown => {
                state.settings.scroll = state
                    .settings
                    .scroll
                    .saturating_add(super::MODAL_PAGE_SCROLL_ROWS as usize)
                    .min(settings_section_max_scroll(
                        state,
                        SettingsSection::GroupProfiles,
                    ));
            }
            KeyCode::Char('f') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                toggle_selected_group_profile_favorite(state);
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                toggle_selected_group_profile_default(state);
            }
            KeyCode::Enter | KeyCode::Char(' ') => {}
            KeyCode::Left | KeyCode::Right if key.modifiers.contains(KeyModifiers::SHIFT) => {}
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                switch_settings_section(
                    state,
                    SettingsSection::Theme,
                    group_accent_selection_index(state),
                );
                ensure_settings_selection_visible(state);
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                switch_settings_section(state, SettingsSection::GroupGithub, 0);
            }
            _ => {
                if let Some(action) = handle_settings_modal_action(state, &key) {
                    return Some(action);
                }
            }
        },
        SettingsSection::WorkspaceGeneral => {
            if edit_pending_workspace_field(state, key) {
                return selected_workspace_general_action(state);
            }
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    select_previous_setting(
                        state,
                        settings_section_choice_len(state, SettingsSection::WorkspaceGeneral),
                    );
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    select_next_setting(
                        state,
                        settings_section_choice_len(state, SettingsSection::WorkspaceGeneral),
                    );
                }
                KeyCode::Enter | KeyCode::Char(' ')
                    if state.settings.list.selected == WORKSPACE_GENERAL_HOST =>
                {
                    cycle_default_host(state, true);
                    return selected_workspace_general_action(state);
                }
                KeyCode::Enter => return selected_workspace_general_action(state),
                KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                    switch_settings_section(state, SettingsSection::WorkspaceGithub, 0);
                }
                KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                    switch_settings_section(state, SettingsSection::WorkspaceGithub, 0);
                }
                _ => {
                    if let Some(action) = handle_settings_modal_action(state, &key) {
                        return Some(action);
                    }
                }
            }
        }
        SettingsSection::WorkspaceGithub => {
            if edit_pending_workspace_field(state, key) {
                return None;
            }
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    select_previous_setting(
                        state,
                        settings_section_choice_len(state, SettingsSection::WorkspaceGithub),
                    );
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    select_next_setting(
                        state,
                        settings_section_choice_len(state, SettingsSection::WorkspaceGithub),
                    );
                }
                KeyCode::Enter | KeyCode::Char(' ')
                    if matches!(
                        state.settings.list.selected,
                        WORKSPACE_GITHUB_AUTOMATIC
                            | WORKSPACE_GITHUB_SELECTED
                            | WORKSPACE_GITHUB_GROUP
                    ) =>
                {
                    return selected_workspace_github_action(state);
                }
                KeyCode::Enter if state.settings.list.selected == WORKSPACE_GITHUB_REPOSITORIES => {
                    return selected_workspace_github_action(state);
                }
                KeyCode::Tab
                | KeyCode::Right
                | KeyCode::Char('l')
                | KeyCode::BackTab
                | KeyCode::Left
                | KeyCode::Char('h') => {
                    switch_settings_section(state, SettingsSection::WorkspaceGeneral, 0);
                }
                _ => {
                    if let Some(action) = handle_settings_modal_action(state, &key) {
                        return Some(action);
                    }
                }
            }
        }
    }

    if state.settings.section != section_before_key {
        clear_settings_selection(state);
    }

    None
}

pub(crate) fn update_settings_state_for_view(
    state: &mut AppState,
    view: &mut ClientViewState,
    key: KeyEvent,
) -> Option<SettingsAction> {
    let shared_mode = state.mode;
    let shared_settings = std::mem::replace(&mut state.settings, view.settings.clone());
    let shared_palette = state.palette.clone();
    let shared_theme_name = state.theme_name.clone();
    let shared_groups = state.groups.clone();

    state.mode = view.mode;
    let action = update_settings_state(state, key);

    view.mode = state.mode;
    view.settings = state.settings.clone();
    state.mode = shared_mode;
    state.settings = shared_settings;
    state.palette = shared_palette;
    state.theme_name = shared_theme_name;
    state.groups = shared_groups;
    action
}

pub(crate) fn paste_settings_text_for_view(
    state: &mut AppState,
    view: &mut ClientViewState,
    text: &str,
) -> Option<Option<SettingsAction>> {
    let shared_mode = state.mode;
    let shared_settings = std::mem::replace(&mut state.settings, view.settings.clone());
    state.mode = view.mode;

    let action = paste_settings_text(state, text);

    view.mode = state.mode;
    view.settings = state.settings.clone();
    state.mode = shared_mode;
    state.settings = shared_settings;
    action
}

pub(crate) fn update_settings_mouse_for_view(
    state: &mut AppState,
    view: &mut ClientViewState,
    mouse: MouseEvent,
) -> Option<SettingsAction> {
    let shared_mode = state.mode;
    let shared_settings = std::mem::replace(&mut state.settings, view.settings.clone());
    let shared_drag = std::mem::replace(&mut state.drag, view.drag.clone());
    let shared_view = std::mem::replace(&mut state.view, view.computed.clone());
    let shared_palette = state.palette.clone();
    let shared_theme_name = state.theme_name.clone();
    let shared_groups = state.groups.clone();

    state.mode = view.mode;
    let action = state.handle_settings_mouse(mouse);

    view.mode = state.mode;
    view.settings = state.settings.clone();
    view.drag = state.drag.clone();
    state.mode = shared_mode;
    state.settings = shared_settings;
    state.drag = shared_drag;
    state.view = shared_view;
    state.palette = shared_palette;
    state.theme_name = shared_theme_name;
    state.groups = shared_groups;
    action
}

pub(crate) fn open_settings(state: &mut AppState) {
    open_settings_at(state, SettingsSection::Theme);
}

pub(crate) fn prepare_general_settings_state(
    state: &AppState,
    settings: &mut SettingsState,
    section: SettingsSection,
) {
    settings.original_palette = Some(state.palette.clone());
    settings.original_theme = Some(state.theme_name.clone());
    settings.pending_theme_name = Some(state.global_theme_name.clone());
    settings.pending_theme_mode = Some(state.global_theme_mode);
    settings.pending_light_theme_name = Some(state.global_light_theme_name.clone());
    settings.pending_dark_theme_name = Some(state.global_dark_theme_name.clone());
    settings.pending_terminal_light_accent = Some(state.global_terminal_light_accent);
    settings.pending_terminal_dark_accent = Some(state.global_terminal_dark_accent);
    settings.pending_sound_enabled = Some(state.sound_enabled());
    settings.pending_toast_delivery = Some(state.toast_delivery());
    settings.pending_default_shell = Some(state.default_shell.clone());
    settings.pending_shell_mode = Some(state.shell_mode);
    settings.pending_version_check = Some(state.update_version_check);
    settings.pending_manifest_check = Some(state.update_manifest_check);
    settings.pending_toast_delay = Some(state.toast_config.delay_seconds.to_string());
    settings.pending_toast_gardn_position = Some(state.toast_config.gardn.position);
    settings.pending_clipboard_toast_enabled = Some(state.toast_config.clipboard.enabled);
    settings.pending_clipboard_toast_position = Some(state.toast_config.clipboard.position);
    settings.pending_confirm_close = Some(state.confirm_close_enabled());
    settings.pending_prompt_new_tab_name = Some(state.prompt_new_tab_name_enabled());
    settings.pending_show_counters = Some(state.show_counters);
    settings.pending_pane_borders = Some(state.pane_borders);
    settings.pending_pane_scrollbars = Some(state.pane_scrollbars);
    settings.pending_pane_gaps = Some(state.pane_gaps);
    settings.pending_hide_tab_bar_when_single_tab = Some(state.hide_tab_bar_when_single_tab);
    settings.pending_copy_on_select = Some(state.copy_on_select);
    settings.pending_prompt_new_workspace_name = Some(state.prompt_new_workspace_name);
    settings.pending_right_click_passthrough_modifier =
        Some(RightClickPassthroughModifierConfig::from_modifiers(
            state.right_click_passthrough_modifiers,
        ));
    settings.pending_new_terminal_cwd = Some(state.new_terminal_cwd.clone());
    settings.pending_mouse_scroll_lines = Some(state.mouse_scroll_lines);
    settings.pending_resume_agents_on_restore = Some(state.resume_agents_on_restore);
    settings.pending_window_title = Some(state.window_title_template.clone());
    settings.pending_headless_cols = Some(state.headless_size.0.to_string());
    settings.pending_headless_rows = Some(state.headless_size.1.to_string());
    settings.pending_browser_command = Some(state.browser_command.clone());
    settings.pending_review_command = Some(state.review_command.clone());
    settings.pending_editor_command = Some(state.editor_command.clone());
    settings.pending_sidebar_width = Some(state.default_sidebar_width);
    settings.pending_sidebar_min_width = Some(state.sidebar_min_width);
    settings.pending_sidebar_max_width = Some(state.sidebar_max_width);
    settings.pending_sidebar_arrangement = Some(state.sidebar_arrangement);
    settings.pending_context_bar_visibility = Some(state.context_bar_visibility);
    settings.pending_sidebar_initial_state = Some(state.sidebar_config.initial_state);
    settings.pending_sidebar_initial_agent_scope = Some(state.sidebar_config.initial_agent_scope);
    settings.pending_pane_border_agent_info = Some(state.pane_border_agent_info());
    settings.pending_status_indicators = Some(state.status_indicators());
    settings.pending_agent_profile_id = None;
    settings.pending_agent_profile_name = None;
    settings.pending_agent_profile_kind = Some(state.default_agent_profile_kind_choice());
    settings.pending_agent_profile_command = None;
    settings.pending_agent_profile_enabled = None;
    settings.connection_editor = None;
    settings.pending_workspace_name = None;
    settings.pending_workspace_default_cwd = None;
    settings.pending_workspace_github_scope = None;
    settings.pending_workspace_github_repositories = None;
    settings.group_settings_target = None;
    settings.workspace_settings_target = None;
    settings.section = section;
    settings.sidebar_expanded = Some(section);
    settings.sidebar_selection = SettingsSidebarSelection::section(section);
    settings.sidebar_focused = false;
    settings.list.selected = match section {
        SettingsSection::Theme => {
            if state.global_theme_mode == ThemeMode::System
                && normalize_theme_name(&state.global_light_theme_name) == "system"
                && normalize_theme_name(&state.global_dark_theme_name) == "system"
            {
                0
            } else {
                2 + current_theme_mode_index(state.global_theme_mode)
            }
        }
        SettingsSection::Layout => 0,
        SettingsSection::Sound => 0,
        SettingsSection::Toast => 0,
        SettingsSection::PaneLabels => 0,
        SettingsSection::Commands => 0,
        SettingsSection::Experiments => 0,
        SettingsSection::Agents => 0,
        SettingsSection::Integrations => 0,
        SettingsSection::Connections => 0,
        SettingsSection::GroupGeneral => 0,
        SettingsSection::GroupDefaults => 0,
        SettingsSection::GroupGithub => 0,
        SettingsSection::GroupProfiles => 0,
        SettingsSection::WorkspaceGeneral => 0,
        SettingsSection::WorkspaceGithub => 0,
        SettingsSection::About => 0,
    };
    settings.scroll = 0;
    settings.list = crate::app::state::ModalListState::hidden(settings.list.selected);
    settings.focused_input = None;
}

fn reset_settings_for_scoped_editor(state: &AppState, settings: &mut SettingsState) {
    settings.original_palette = Some(state.palette.clone());
    settings.original_theme = Some(state.theme_name.clone());
    settings.pending_theme_name = None;
    settings.pending_theme_mode = None;
    settings.pending_light_theme_name = None;
    settings.pending_dark_theme_name = None;
    settings.pending_terminal_light_accent = None;
    settings.pending_terminal_dark_accent = None;
    settings.pending_group_accent_choice = None;
    settings.pending_sound_enabled = None;
    settings.pending_toast_delivery = None;
    settings.pending_default_shell = None;
    settings.pending_shell_mode = None;
    settings.pending_version_check = None;
    settings.pending_manifest_check = None;
    settings.pending_toast_delay = None;
    settings.pending_toast_gardn_position = None;
    settings.pending_clipboard_toast_enabled = None;
    settings.pending_clipboard_toast_position = None;
    settings.pending_confirm_close = None;
    settings.pending_prompt_new_tab_name = None;
    settings.pending_show_counters = None;
    settings.pending_pane_borders = None;
    settings.pending_pane_scrollbars = None;
    settings.pending_pane_gaps = None;
    settings.pending_hide_tab_bar_when_single_tab = None;
    settings.pending_copy_on_select = None;
    settings.pending_prompt_new_workspace_name = None;
    settings.pending_right_click_passthrough_modifier = None;
    settings.pending_new_terminal_cwd = None;
    settings.pending_mouse_scroll_lines = None;
    settings.pending_resume_agents_on_restore = None;
    settings.pending_window_title = None;
    settings.pending_headless_cols = None;
    settings.pending_browser_command = None;
    settings.pending_review_command = None;
    settings.pending_editor_command = None;
    settings.pending_group_github_organization = None;
    settings.pending_workspace_github_scope = None;
    settings.pending_workspace_github_repositories = None;
    settings.pending_sidebar_width = None;
    settings.pending_sidebar_min_width = None;
    settings.pending_sidebar_max_width = None;
    settings.pending_sidebar_arrangement = None;
    settings.pending_context_bar_visibility = None;
    settings.pending_sidebar_initial_state = None;
    settings.pending_sidebar_initial_agent_scope = None;
    settings.pending_pane_border_agent_info = None;
    settings.pending_status_indicators = None;
    settings.pending_switch_ascii_input_source_in_prefix = None;
    settings.sidebar_expanded = None;
    settings.sidebar_selection = SettingsSidebarSelection::section(settings.section);
    settings.sidebar_focused = false;
}

pub(crate) fn prepare_group_settings_state(
    state: &AppState,
    settings: &mut SettingsState,
    group_idx: usize,
) -> bool {
    let Some(group) = state.groups.get(group_idx) else {
        return false;
    };
    reset_settings_for_scoped_editor(state, settings);
    settings.pending_group_name = Some(group.name.clone());
    settings.pending_group_icon = Some(group.icon.clone());
    settings.pending_group_github_organization = group
        .github_organization
        .as_ref()
        .map(|organization| organization.as_str().to_string());
    settings.pending_group_default_directory = None;

    settings.pending_group_default_execution_host_id = group
        .default_location
        .as_ref()
        .map(|location| location.execution_host_id.clone());
    settings.pending_workspace_name = None;
    settings.pending_workspace_default_cwd = None;
    settings.pending_workspace_github_scope = None;
    settings.pending_workspace_github_repositories = None;
    settings.pending_workspace_default_execution_host_id = None;
    settings.group_settings_target = Some(group_idx);
    settings.workspace_settings_target = None;
    settings.section = SettingsSection::GroupGeneral;
    settings.list = crate::app::state::ModalListState::hidden(0);
    settings.focused_input = None;
    settings.scroll = 0;
    true
}

pub(crate) fn prepare_workspace_settings_state(
    state: &AppState,
    settings: &mut SettingsState,
    ws_idx: usize,
) -> bool {
    let Some(workspace) = state.workspaces.get(ws_idx) else {
        return false;
    };
    reset_settings_for_scoped_editor(state, settings);
    settings.pending_group_name = None;
    settings.pending_group_icon = None;
    settings.pending_group_github_organization = None;
    settings.pending_group_default_directory = None;

    settings.pending_workspace_name = Some(workspace.display_name());
    settings.pending_workspace_default_cwd = Some(
        workspace
            .default_location
            .path
            .as_path()
            .display()
            .to_string(),
    );
    settings.pending_workspace_github_scope = Some(workspace.github_scope.clone());
    settings.pending_workspace_github_repositories = Some(
        workspace
            .github_scope
            .selected_repositories()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", "),
    );
    settings.pending_workspace_default_execution_host_id =
        Some(workspace.default_location.execution_host_id.clone());
    settings.group_settings_target = None;
    settings.workspace_settings_target = Some(ws_idx);
    settings.section = SettingsSection::WorkspaceGeneral;
    settings.list = crate::app::state::ModalListState::hidden(0);
    settings.focused_input = None;
    settings.scroll = 0;
    true
}

pub(crate) fn open_settings_at(state: &mut AppState, section: SettingsSection) {
    state.integration_install_messages.clear();
    let mut settings = state.settings.clone();
    prepare_general_settings_state(state, &mut settings, section);
    state.settings = settings;
    clear_settings_selection(state);
    ensure_settings_selection_visible(state);
    if section == SettingsSection::Theme {
        let theme_name = state
            .global_theme_name_for_mode(state.global_theme_mode)
            .to_string();
        state.preview_theme_with_mode(&theme_name, state.global_theme_mode);
    }
    state.mode = Mode::Settings;
}

pub(crate) fn open_group_settings(state: &mut AppState, group_idx: usize) {
    let Some(group) = state.groups.get(group_idx) else {
        return;
    };
    let group_accent = group.accent;
    let group_name = group.name.clone();
    let group_icon = group.icon.clone();
    let group_github_organization = group
        .github_organization
        .as_ref()
        .map(|organization| organization.as_str().to_string());
    state.settings.original_palette = Some(state.palette.clone());
    state.settings.original_theme = Some(state.theme_name.clone());
    state.settings.pending_theme_name = None;
    state.settings.pending_theme_mode = None;
    state.settings.pending_light_theme_name = None;
    state.settings.pending_dark_theme_name = None;
    state.settings.pending_terminal_light_accent = None;
    state.settings.pending_terminal_dark_accent = None;
    state.settings.pending_group_accent_choice = None;
    state.settings.pending_group_name = Some(group_name);
    state.settings.pending_group_icon = Some(group_icon);
    state.settings.pending_group_github_organization = group_github_organization;
    state.settings.pending_workspace_name = None;
    state.settings.pending_workspace_github_scope = None;
    state.settings.pending_workspace_github_repositories = None;

    state.settings.pending_group_default_execution_host_id = group
        .default_location
        .as_ref()
        .map(|location| location.execution_host_id.clone());
    state.settings.pending_workspace_default_cwd = None;
    state.settings.pending_sound_enabled = None;
    state.settings.pending_workspace_default_execution_host_id = None;
    state.settings.pending_toast_delivery = None;
    state.settings.pending_default_shell = None;
    state.settings.pending_shell_mode = None;
    state.settings.pending_version_check = None;
    state.settings.pending_manifest_check = None;
    state.settings.pending_toast_delay = None;
    state.settings.pending_toast_gardn_position = None;
    state.settings.pending_clipboard_toast_enabled = None;
    state.settings.pending_clipboard_toast_position = None;
    state.settings.pending_confirm_close = None;
    state.settings.pending_prompt_new_tab_name = None;
    state.settings.pending_show_counters = None;
    state.settings.pending_pane_borders = None;
    state.settings.pending_pane_scrollbars = None;
    state.settings.pending_pane_gaps = None;
    state.settings.pending_hide_tab_bar_when_single_tab = None;
    state.settings.pending_copy_on_select = None;
    state.settings.pending_prompt_new_workspace_name = None;
    state.settings.pending_right_click_passthrough_modifier = None;
    state.settings.pending_new_terminal_cwd = None;
    state.settings.pending_mouse_scroll_lines = None;
    state.settings.pending_resume_agents_on_restore = None;
    state.settings.pending_window_title = None;
    state.settings.pending_headless_cols = None;
    state.settings.pending_headless_rows = None;
    state.settings.pending_browser_command = None;
    state.settings.pending_review_command = None;
    state.settings.pending_editor_command = None;
    state.settings.pending_sidebar_width = None;
    state.settings.pending_sidebar_min_width = None;
    state.settings.pending_sidebar_max_width = None;
    state.settings.pending_sidebar_arrangement = None;
    state.settings.pending_context_bar_visibility = None;
    state.settings.pending_sidebar_initial_state = None;
    state.settings.pending_sidebar_initial_agent_scope = None;
    state.settings.pending_pane_border_agent_info = None;
    state.settings.pending_status_indicators = None;
    state.settings.pending_switch_ascii_input_source_in_prefix = None;
    state.settings.group_settings_target = Some(group_idx);
    state.settings.workspace_settings_target = None;
    state.settings.section = SettingsSection::GroupGeneral;
    state.settings.sidebar_expanded = None;
    state.settings.sidebar_selection =
        SettingsSidebarSelection::section(SettingsSection::GroupGeneral);
    state.settings.sidebar_focused = false;
    state.settings.list.selected = 0;
    state.settings.scroll = 0;
    clear_settings_selection(state);
    ensure_settings_selection_visible(state);
    preview_group_accent(state, group_accent);
    state.mode = Mode::Settings;
}

pub(crate) fn open_workspace_settings(state: &mut AppState, ws_idx: usize) {
    let Some(workspace) = state.workspaces.get(ws_idx) else {
        return;
    };
    let workspace_name = workspace.display_name();
    let default_cwd = workspace
        .default_location
        .path
        .as_path()
        .display()
        .to_string();
    let default_execution_host_id = workspace.default_location.execution_host_id.clone();
    let github_scope = workspace.github_scope.clone();
    let github_repositories = github_scope
        .selected_repositories()
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    state.settings.original_palette = Some(state.palette.clone());
    state.settings.original_theme = Some(state.theme_name.clone());
    state.settings.pending_theme_name = None;
    state.settings.pending_theme_mode = None;
    state.settings.pending_light_theme_name = None;
    state.settings.pending_dark_theme_name = None;
    state.settings.pending_terminal_light_accent = None;
    state.settings.pending_terminal_dark_accent = None;
    state.settings.pending_group_accent_choice = None;
    state.settings.pending_group_name = None;
    state.settings.pending_group_icon = None;
    state.settings.pending_group_github_organization = None;
    state.settings.pending_workspace_name = Some(workspace_name);

    state.settings.pending_workspace_default_cwd = Some(default_cwd);
    state.settings.pending_workspace_github_scope = Some(github_scope);
    state.settings.pending_workspace_github_repositories = Some(github_repositories);
    state.settings.pending_workspace_default_execution_host_id = Some(default_execution_host_id);
    state.settings.pending_sound_enabled = None;
    state.settings.pending_toast_delivery = None;
    state.settings.pending_default_shell = None;
    state.settings.pending_shell_mode = None;
    state.settings.pending_version_check = None;
    state.settings.pending_manifest_check = None;
    state.settings.pending_toast_delay = None;
    state.settings.pending_toast_gardn_position = None;
    state.settings.pending_clipboard_toast_enabled = None;
    state.settings.pending_clipboard_toast_position = None;
    state.settings.pending_confirm_close = None;
    state.settings.pending_prompt_new_tab_name = None;
    state.settings.pending_show_counters = None;
    state.settings.pending_pane_borders = None;
    state.settings.pending_pane_scrollbars = None;
    state.settings.pending_pane_gaps = None;
    state.settings.pending_hide_tab_bar_when_single_tab = None;
    state.settings.pending_copy_on_select = None;
    state.settings.pending_prompt_new_workspace_name = None;
    state.settings.pending_right_click_passthrough_modifier = None;
    state.settings.pending_new_terminal_cwd = None;
    state.settings.pending_mouse_scroll_lines = None;
    state.settings.pending_resume_agents_on_restore = None;
    state.settings.pending_window_title = None;
    state.settings.pending_headless_cols = None;
    state.settings.pending_headless_rows = None;
    state.settings.pending_browser_command = None;
    state.settings.pending_review_command = None;
    state.settings.pending_editor_command = None;
    state.settings.pending_sidebar_width = None;
    state.settings.pending_sidebar_min_width = None;
    state.settings.pending_sidebar_max_width = None;
    state.settings.pending_sidebar_arrangement = None;
    state.settings.pending_context_bar_visibility = None;
    state.settings.pending_sidebar_initial_state = None;
    state.settings.pending_sidebar_initial_agent_scope = None;
    state.settings.pending_pane_border_agent_info = None;
    state.settings.pending_status_indicators = None;
    state.settings.pending_switch_ascii_input_source_in_prefix = None;
    state.settings.group_settings_target = None;
    state.settings.workspace_settings_target = Some(ws_idx);
    state.settings.section = SettingsSection::WorkspaceGeneral;
    state.settings.sidebar_expanded = None;
    state.settings.sidebar_selection =
        SettingsSidebarSelection::section(SettingsSection::WorkspaceGeneral);
    state.settings.sidebar_focused = false;
    state.settings.list.selected = 0;
    state.settings.scroll = 0;
    clear_settings_selection(state);
    ensure_settings_selection_visible(state);
    state.mode = Mode::Settings;
}
impl AppState {
    fn settings_popup_rect(&self) -> Rect {
        crate::ui::centered_popup_rect(self.settings_overlay_rect(), 92, 26).unwrap_or_default()
    }

    fn settings_sidebar_entry_at(
        &self,
        col: u16,
        row: u16,
    ) -> Option<crate::ui::SettingsSidebarEntry> {
        if self.settings.group_settings_target.is_some()
            || self.settings.workspace_settings_target.is_some()
        {
            return None;
        }
        let inner = self.settings_inner_rect();
        let content = crate::ui::settings_stack_areas(self, inner).content;
        let navigation = crate::ui::settings_sidebar_areas(content).navigation;
        crate::ui::settings_sidebar_hit_areas(&self.settings, navigation)
            .into_iter()
            .find_map(|(entry, rect)| {
                (col >= rect.x && col < rect.x.saturating_add(rect.width) && row == rect.y)
                    .then_some(entry)
            })
    }

    fn settings_overlay_rect(&self) -> Rect {
        self.screen_rect()
    }

    fn settings_inner_rect(&self) -> Rect {
        let popup = self.settings_popup_rect();
        Rect::new(
            popup.x + 1,
            popup.y + 1,
            popup.width.saturating_sub(2),
            popup.height.saturating_sub(2),
        )
    }

    fn settings_tab_at(&self, col: u16, row: u16) -> Option<SettingsSection> {
        if self.settings.group_settings_target.is_none()
            && self.settings.workspace_settings_target.is_none()
        {
            return None;
        }
        let inner = self.settings_inner_rect();
        let header_rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas::<4>(crate::ui::settings_stack_areas(self, inner).header);
        let tab_row = header_rows[2];
        if row != tab_row.y {
            return None;
        }
        if let Some(section) = crate::ui::settings_tab_chevron_at(self, tab_row, col) {
            return Some(section);
        }

        crate::ui::settings_tab_hit_areas(self, tab_row)
            .into_iter()
            .find_map(|(section, rect)| {
                (col >= rect.x && col < rect.x + rect.width).then_some(section)
            })
    }

    fn settings_editor_back_at(&self, col: u16, row: u16) -> bool {
        let area = self.settings_content_rect();
        let Some(rect) = crate::ui::settings_editor_back_button_rect(self, area) else {
            return false;
        };
        col >= rect.x && col < rect.x + rect.width && row == rect.y
    }

    pub(crate) fn settings_content_rect(&self) -> Rect {
        let inner = self.settings_inner_rect();
        let content = crate::ui::settings_stack_areas(self, inner).content;
        if self.settings.group_settings_target.is_none()
            && self.settings.workspace_settings_target.is_none()
        {
            crate::ui::settings_sidebar_areas(content).content
        } else {
            content
        }
    }

    fn settings_list_hit_at(&self, col: u16, row: u16) -> Option<SettingsRowHit> {
        let area = self.settings_content_rect();
        if row < area.y || row >= area.y + area.height || col < area.x || col >= area.x + area.width
        {
            return None;
        }

        match self.settings.section {
            SettingsSection::Theme => {
                let list = settings_section_list_geometry(self, SettingsSection::Theme);
                let visual_row = list.hit_visual_row(col, row)?;
                option_hit_for_visual_row(&theme_rows(self), visual_row)
            }
            SettingsSection::Layout
            | SettingsSection::Sound
            | SettingsSection::Toast
            | SettingsSection::PaneLabels
            | SettingsSection::Commands
            | SettingsSection::Experiments
            | SettingsSection::Agents
            | SettingsSection::Connections
            | SettingsSection::GroupGeneral
            | SettingsSection::GroupDefaults
            | SettingsSection::GroupGithub
            | SettingsSection::GroupProfiles
            | SettingsSection::WorkspaceGeneral
            | SettingsSection::WorkspaceGithub
            | SettingsSection::About
            | SettingsSection::Integrations => {
                let list = settings_section_list_geometry(self, self.settings.section);
                let visual_row = list.hit_visual_row(col, row)?;
                let rows = rows_for_section(self, self.settings.section)?;
                option_hit_for_visual_row(&rows, visual_row)
            }
        }
    }

    fn settings_theme_scrollbar_target_at(
        &self,
        col: u16,
        row: u16,
    ) -> Option<ScrollbarClickTarget> {
        if self.settings.section != SettingsSection::Theme {
            return None;
        }
        let list = settings_section_list_geometry(self, SettingsSection::Theme);
        let metrics = list.metrics();
        let track = list.scroll_area.track?;
        if !(col >= track.x
            && col < track.x + track.width
            && row >= track.y
            && row < track.y + track.height)
        {
            return None;
        }
        if let Some(grab_row_offset) = crate::ui::scrollbar_thumb_grab_offset(metrics, track, row) {
            Some(ScrollbarClickTarget::Thumb { grab_row_offset })
        } else {
            Some(ScrollbarClickTarget::Track {
                offset_from_bottom: crate::ui::scrollbar_offset_from_row(metrics, track, row),
            })
        }
    }

    fn settings_theme_offset_for_drag_row(&self, row: u16, grab_row_offset: u16) -> Option<usize> {
        if self.settings.section != SettingsSection::Theme {
            return None;
        }
        let list = settings_section_list_geometry(self, SettingsSection::Theme);
        let metrics = list.metrics();
        let track = list.scroll_area.track?;
        Some(crate::ui::scrollbar_offset_from_drag_row(
            metrics,
            track,
            row,
            grab_row_offset,
        ))
    }

    pub(super) fn handle_settings_mouse(&mut self, mouse: MouseEvent) -> Option<SettingsAction> {
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(target) =
                    self.settings_theme_scrollbar_target_at(mouse.column, mouse.row)
                {
                    match target {
                        ScrollbarClickTarget::Thumb { grab_row_offset } => {
                            self.drag = Some(DragState {
                                target: DragTarget::SettingsThemeScrollbar { grab_row_offset },
                            });
                        }
                        ScrollbarClickTarget::Track { offset_from_bottom } => {
                            set_settings_theme_offset_from_bottom(self, offset_from_bottom);
                        }
                    }
                    return None;
                }

                if let Some(entry) = self.settings_sidebar_entry_at(mouse.column, mouse.row) {
                    self.settings.sidebar_focused = true;
                    activate_settings_sidebar_entry(self, entry);
                    return None;
                }

                if let Some(section) = self.settings_tab_at(mouse.column, mouse.row) {
                    let selected = match section {
                        SettingsSection::Theme => {
                            if self.settings.group_settings_target.is_some() {
                                group_accent_selection_index(self)
                            } else {
                                target_theme_index(self)
                            }
                        }
                        SettingsSection::Layout
                        | SettingsSection::Sound
                        | SettingsSection::Toast
                        | SettingsSection::PaneLabels
                        | SettingsSection::Commands
                        | SettingsSection::Experiments
                        | SettingsSection::Agents
                        | SettingsSection::Integrations
                        | SettingsSection::Connections
                        | SettingsSection::GroupGeneral
                        | SettingsSection::GroupDefaults
                        | SettingsSection::GroupGithub
                        | SettingsSection::GroupProfiles
                        | SettingsSection::WorkspaceGeneral
                        | SettingsSection::WorkspaceGithub
                        | SettingsSection::About => 0,
                    };
                    switch_settings_section(self, section, selected);
                    self.settings.group_icon_picker_open = false;
                    if section == SettingsSection::Theme {
                        ensure_settings_selection_visible(self);
                    }
                    return None;
                }

                self.settings.sidebar_focused = false;
                if self.settings_editor_back_at(mouse.column, mouse.row) {
                    match self.settings.section {
                        SettingsSection::Agents => close_agent_profile_editor(self),
                        SettingsSection::Connections => back_from_connection_screen(self),
                        _ => {}
                    }
                    return None;
                }
                if let Some(icon) = group_settings_icon_picker_hit(self, mouse.column, mouse.row) {
                    self.settings.pending_group_icon = Some(icon.to_string());
                    self.settings.group_icon_picker_open = false;
                    self.settings.list.select(GROUP_GENERAL_ICON);
                    return selected_group_general_action(self);
                }
                if let Some(target) = self.settings_list_hit_at(mouse.column, mouse.row) {
                    let idx = target.index;
                    self.settings.list.select(idx);
                    self.settings.focused_input = (!target.hoverable).then_some(idx);
                    if !target.hoverable {
                        return None;
                    }
                    if self.settings.section == SettingsSection::Theme {
                        ensure_settings_selection_visible(self);
                    }
                    return match self.settings.section {
                        SettingsSection::Theme
                        | SettingsSection::Layout
                        | SettingsSection::Sound
                        | SettingsSection::Toast
                        | SettingsSection::PaneLabels
                        | SettingsSection::Commands => select_pending_setting(self),
                        SettingsSection::Experiments => selected_experiment_action(self),
                        SettingsSection::Agents => selected_agent_profile_action(self),
                        SettingsSection::Connections => selected_connection_profile_action(self),
                        SettingsSection::Integrations => selected_integration_action(self),
                        SettingsSection::GroupGeneral => match idx {
                            GROUP_GENERAL_ICON => {
                                toggle_group_icon_picker(self);
                                None
                            }
                            GROUP_GENERAL_DELETE => {
                                self.settings.group_icon_picker_open = false;
                                selected_group_general_action(self)
                            }
                            _ => {
                                self.settings.group_icon_picker_open = false;
                                None
                            }
                        },
                        SettingsSection::GroupDefaults => match idx {
                            GROUP_DEFAULTS_HOST => {
                                self.settings.group_icon_picker_open = false;
                                cycle_default_host(self, false);
                                selected_group_defaults_action(self)
                            }
                            _ => None,
                        },
                        SettingsSection::GroupGithub => selected_group_github_action(self),
                        SettingsSection::GroupProfiles => None,
                        SettingsSection::WorkspaceGeneral => match idx {
                            WORKSPACE_GENERAL_HOST => {
                                cycle_default_host(self, true);
                                selected_workspace_general_action(self)
                            }
                            _ => None,
                        },
                        SettingsSection::WorkspaceGithub => selected_workspace_github_action(self),
                        SettingsSection::About => None,
                    };
                }

                let inner = self.settings_inner_rect();
                let close = crate::ui::settings_close_button_rect(inner);
                match super::modal::modal_action_from_buttons(
                    mouse.column,
                    mouse.row,
                    &[(close, super::modal::ModalAction::Close)],
                ) {
                    Some(super::modal::ModalAction::Close) => {
                        close_settings(self);
                        None
                    }
                    _ => {
                        let popup = self.settings_popup_rect();
                        let inside = popup.width > 0
                            && popup.height > 0
                            && mouse.column >= popup.x
                            && mouse.column < popup.x + popup.width
                            && mouse.row >= popup.y
                            && mouse.row < popup.y + popup.height;
                        if !inside {
                            close_settings(self);
                        }
                        None
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(DragState {
                    target: DragTarget::SettingsThemeScrollbar { grab_row_offset },
                }) = &self.drag
                {
                    if let Some(offset_from_bottom) =
                        self.settings_theme_offset_for_drag_row(mouse.row, *grab_row_offset)
                    {
                        set_settings_theme_offset_from_bottom(self, offset_from_bottom);
                    }
                }
                None
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.drag.as_ref().is_some_and(|drag| {
                    matches!(drag.target, DragTarget::SettingsThemeScrollbar { .. })
                }) {
                    self.drag = None;
                }
                None
            }
            MouseEventKind::Moved => {
                let hovered = self
                    .settings_list_hit_at(mouse.column, mouse.row)
                    .filter(|target| target.hoverable)
                    .map(|target| target.index);
                self.settings.list.hover(hovered);
                None
            }
            MouseEventKind::ScrollUp => {
                self.settings.scroll = self
                    .settings
                    .scroll
                    .saturating_sub(super::MODAL_WHEEL_SCROLL_ROWS as usize);
                None
            }
            MouseEventKind::ScrollDown => {
                self.settings.scroll = self
                    .settings
                    .scroll
                    .saturating_add(super::MODAL_WHEEL_SCROLL_ROWS as usize)
                    .min(settings_section_max_scroll(self, self.settings.section));
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};

    use super::super::{app_for_mouse_test, mouse, state_with_workspaces};
    use super::*;

    fn rendered_text_point(
        app: &crate::app::App,
        text: &str,
        width: u16,
        height: u16,
    ) -> (u16, u16) {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| crate::ui::render(&app.state, frame))
            .expect("render settings");
        let buffer = terminal.backend().buffer();
        let symbols = text.chars().map(|ch| ch.to_string()).collect::<Vec<_>>();
        let text_width = symbols.len() as u16;

        for y in 0..height {
            for x in 0..=width.saturating_sub(text_width) {
                if symbols
                    .iter()
                    .enumerate()
                    .all(|(idx, ch)| buffer[(x + idx as u16, y)].symbol() == ch.as_str())
                {
                    return (x, y);
                }
            }
        }

        panic!("rendered text not found: {text}");
    }
    fn rendered_contains_text(app: &crate::app::App, text: &str, width: u16, height: u16) -> bool {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| crate::ui::render(&app.state, frame))
            .expect("render settings");
        let buffer = terminal.backend().buffer();
        let symbols = text.chars().map(|ch| ch.to_string()).collect::<Vec<_>>();
        let text_width = symbols.len() as u16;

        (0..height).any(|y| {
            (0..=width.saturating_sub(text_width)).any(|x| {
                symbols
                    .iter()
                    .enumerate()
                    .all(|(idx, ch)| buffer[(x + idx as u16, y)].symbol() == ch.as_str())
            })
        })
    }

    fn rendered_text_point_at_or_after_row(
        app: &crate::app::App,
        text: &str,
        min_row: u16,
        width: u16,
        height: u16,
    ) -> (u16, u16) {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| crate::ui::render(&app.state, frame))
            .expect("render settings");
        let buffer = terminal.backend().buffer();
        let symbols = text.chars().map(|ch| ch.to_string()).collect::<Vec<_>>();
        let text_width = symbols.len() as u16;

        for y in min_row..height {
            for x in 0..=width.saturating_sub(text_width) {
                if symbols
                    .iter()
                    .enumerate()
                    .all(|(idx, ch)| buffer[(x + idx as u16, y)].symbol() == ch.as_str())
                {
                    return (x, y);
                }
            }
        }

        panic!("rendered text not found after row {min_row}: {text}");
    }
    fn click_settings_tab(app: &mut crate::app::App, section: SettingsSection) {
        let inner = app.state.settings_inner_rect();
        let header_rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas::<4>(crate::ui::settings_stack_areas(&app.state, inner).header);
        let rect = crate::ui::settings_tab_hit_areas(&app.state, header_rows[2])
            .into_iter()
            .find_map(|(candidate, rect)| (candidate == section).then_some(rect))
            .expect("settings tab");
        assert_eq!(
            app.state.handle_settings_mouse(mouse(
                MouseEventKind::Down(MouseButton::Left),
                rect.x,
                rect.y,
            )),
            None
        );
        assert_eq!(app.state.settings.section, section);
    }

    #[test]
    fn settings_escape_closes_without_reverting_previewed_theme() {
        let mut state = state_with_workspaces(&["test"]);
        let original_theme = state.theme_name.clone();

        open_settings(&mut state);
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        assert_ne!(state.theme_name, original_theme);
        let selected_theme = state.theme_name.clone();

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()),
        );
        assert!(state.settings.sidebar_focused);
        assert_eq!(state.settings.section, SettingsSection::Theme);

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::empty()),
        );

        assert_eq!(state.mode, Mode::Terminal);
        assert_eq!(state.theme_name, selected_theme);
    }

    #[test]
    fn group_accent_settings_selection_returns_group_accent_action() {
        let mut state = state_with_workspaces(&["test"]);
        let group_idx = state.create_group("Side".to_string());

        open_group_settings(&mut state, group_idx);
        state.settings.section = SettingsSection::Theme;
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert_eq!(
            action,
            Some(SettingsAction::SaveGroupAccent {
                group_idx,
                accent: Some(TerminalAccent::Blue),
            })
        );
        assert_eq!(state.settings.group_settings_target, Some(group_idx));
        assert_eq!(state.mode, Mode::Settings);
    }

    #[test]
    fn group_accent_settings_default_keeps_group_on_global_accent() {
        let mut state = state_with_workspaces(&["test"]);
        let group_idx = state.create_group("Side".to_string());

        open_group_settings(&mut state, group_idx);
        state.settings.section = SettingsSection::Theme;
        assert_eq!(state.settings.list.selected, 0);
        state.settings.list.show();
        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert_eq!(
            action,
            Some(SettingsAction::SaveGroupAccent {
                group_idx,
                accent: None,
            })
        );
    }

    #[test]
    fn group_settings_inherit_previews_global_theme_accent() {
        let mut state = state_with_workspaces(&["test"]);
        let group_idx = state.create_group("Side".to_string());
        state.global_palette = crate::app::state::Palette::dracula();
        state.palette = state.global_palette.clone();
        state.active_group = group_idx;
        assert!(state.set_group_accent(group_idx, Some(TerminalAccent::Blue)));
        assert_ne!(
            state.group_accent_color(group_idx),
            state.global_palette.accent
        );

        open_group_settings(&mut state, group_idx);
        state.settings.section = SettingsSection::Theme;
        state.settings.list.selected = 0;
        state.settings.list.show();
        state.settings.list.show();
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert_eq!(state.palette.accent, state.global_palette.accent);
    }

    #[test]
    fn group_accent_hover_does_not_move_checkmark() {
        let mut state = state_with_workspaces(&["test"]);
        let group_idx = state.create_group("Side".to_string());
        assert!(state.set_group_accent(group_idx, Some(TerminalAccent::Blue)));

        open_group_settings(&mut state, group_idx);
        state.settings.section = SettingsSection::Theme;
        state.settings.list.selected = group_accent_selection_index(&state);
        assert_eq!(state.settings.list.selected, 1);
        state.settings.list.show();
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );

        let rows = rows_for_section(&state, SettingsSection::Theme).expect("theme rows");
        let checked = rows
            .iter()
            .find_map(|row| match row {
                crate::settings_rows::SettingsListRow::Choice {
                    label,
                    checked: true,
                    ..
                } => Some(label.as_ref().to_string()),
                _ => None,
            })
            .expect("checked accent");
        assert_eq!(state.settings.list.selected, 3);
        assert_eq!(checked, TerminalAccent::Blue.display_name());
    }

    #[test]
    fn agent_settings_add_profile_returns_config_action() {
        let mut state = state_with_workspaces(&["test"]);
        state.integration_recommendations = vec![integration_recommendation_for(
            crate::api::schema::IntegrationTarget::Omp,
            crate::integration::IntegrationStatusKind::Current,
            true,
        )];
        open_settings_at(&mut state, SettingsSection::Agents);
        state.settings.pending_agent_profile_name = Some("omp mk".to_string());
        state.settings.pending_agent_profile_command = Some("omp-mk --profile main".to_string());
        state.settings.pending_agent_profile_kind = Some(crate::agent_profiles::AgentKind::Omp);
        state.settings.list.selected = agent_profile_save_index(&state);
        state.settings.list.show();
        state.settings.list.show();

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(
            action,
            Some(SettingsAction::SaveAgentProfile(
                crate::agent_profiles::UserAgentProfileConfig {
                    id: "omp-mk".to_string(),
                    name: "omp mk".to_string(),
                    kind: crate::agent_profiles::AgentKind::Omp,
                    command: "omp-mk --profile main".to_string(),
                    env: std::collections::BTreeMap::new(),
                    enabled: true,
                }
            ))
        );
    }

    #[test]
    fn agent_profile_editor_only_lists_installed_families() {
        let mut state = state_with_workspaces(&["test"]);
        state.integration_recommendations = vec![integration_recommendation_for(
            crate::api::schema::IntegrationTarget::Codex,
            crate::integration::IntegrationStatusKind::Outdated,
            true,
        )];
        open_settings_at(&mut state, SettingsSection::Agents);
        open_blank_agent_profile_editor(&mut state);

        let rows = rows_for_section(&state, SettingsSection::Agents).expect("agent rows");

        assert!(rows.iter().any(|row| {
            matches!(
                row,
                crate::settings_rows::SettingsListRow::Choice { label, .. }
                    if label.as_ref() == "Codex"
            )
        }));
        assert!(rows.iter().any(|row| {
            matches!(
                row,
                crate::settings_rows::SettingsListRow::Choice { label, .. }
                    if label.as_ref() == "Custom"
            )
        }));
        assert!(!rows.iter().any(|row| {
            matches!(
                row,
                crate::settings_rows::SettingsListRow::Choice { label, .. }
                    if label.as_ref() == "Omp"
            )
        }));
    }

    #[test]
    fn agent_settings_back_returns_to_profile_list_from_rendered_button() {
        let mut app = app_for_mouse_test();
        app.state.view.terminal_area = Rect::new(0, 0, 100, 40);
        open_settings_at(&mut app.state, SettingsSection::Agents);
        open_blank_agent_profile_editor(&mut app.state);

        let (back_x, back_y) = rendered_text_point(&app, "← Back", 100, 40);
        let action = app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            back_x,
            back_y,
        ));
        assert_eq!(action, None);
        assert_eq!(app.state.settings.pending_agent_profile_name, None);
        assert_eq!(app.state.settings.pending_agent_profile_command, None);
        assert_eq!(app.state.settings.list.selected, 0);
    }

    #[test]
    fn agent_settings_delete_custom_profile_row_returns_delete_action() {
        let mut state = state_with_workspaces(&["test"]);
        state.agent_profiles = crate::agent_profiles::AgentProfileCatalog::from_config(
            &crate::agent_profiles::AgentProfilesConfig {
                order: vec!["user:omp-mk".to_string()],
                custom: vec![crate::agent_profiles::UserAgentProfileConfig {
                    id: "omp-mk".to_string(),
                    name: "omp mk".to_string(),
                    kind: crate::agent_profiles::AgentKind::Omp,
                    command: "omp-mk".to_string(),
                    env: std::collections::BTreeMap::new(),
                    enabled: true,
                }],
            },
        );
        open_settings_at(&mut state, SettingsSection::Agents);
        assert!(load_custom_agent_profile_editor(&mut state, "user:omp-mk"));
        state.settings.list.selected = agent_profile_delete_index(&state);
        state.settings.list.show();
        state.settings.focused_input = None;

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(
            action,
            Some(SettingsAction::DeleteAgentProfile(
                "user:omp-mk".to_string()
            ))
        );
    }

    #[test]
    fn agent_settings_can_navigate_to_delete_custom_profile_row() {
        let mut state = state_with_workspaces(&["test"]);
        state.agent_profiles = crate::agent_profiles::AgentProfileCatalog::from_config(
            &crate::agent_profiles::AgentProfilesConfig {
                order: vec!["user:omp-mk".to_string()],
                custom: vec![crate::agent_profiles::UserAgentProfileConfig {
                    id: "omp-mk".to_string(),
                    name: "omp mk".to_string(),
                    kind: crate::agent_profiles::AgentKind::Omp,
                    command: "omp-mk".to_string(),
                    env: std::collections::BTreeMap::new(),
                    enabled: true,
                }],
            },
        );
        open_settings_at(&mut state, SettingsSection::Agents);
        assert!(load_custom_agent_profile_editor(&mut state, "user:omp-mk"));

        for _ in 0..agent_profile_delete_index(&state) {
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
            );
        }

        assert_eq!(
            state.settings.list.selected,
            agent_profile_delete_index(&state)
        );
    }
    #[test]
    fn agent_settings_ctrl_f_does_not_toggle_group_favorite() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Agents);
        state.settings.list.selected = 1;
        state.settings.list.show();

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
        );

        assert_eq!(action, None);
        assert!(state.groups[state.active_group]
            .favorite_agent_profile_ids
            .is_empty());
        assert!(!state.session_dirty);
    }

    #[test]
    fn agent_settings_editor_treats_vim_motion_keys_as_text() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Agents);
        state.settings.pending_agent_profile_name = Some(String::new());
        state.settings.pending_agent_profile_command = Some(String::new());
        state.settings.list.selected = 0;
        state.settings.list.show();
        state.settings.focused_input = Some(0);

        for ch in "ompmk".chars() {
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty()),
            );
        }

        assert_eq!(
            state.settings.pending_agent_profile_name.as_deref(),
            Some("ompmk")
        );
        assert_eq!(state.settings.list.selected, 0);
    }

    #[test]
    fn agent_settings_space_does_not_toggle_favorite() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Agents);
        state.settings.list.selected = 1;
        state.settings.list.show();

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert_eq!(action, None);
        assert!(state.groups[state.active_group]
            .favorite_agent_profile_ids
            .is_empty());
        assert!(!state.session_dirty);
    }

    #[test]
    fn global_agent_settings_leave_group_favorites_to_group_profiles() {
        let mut state = state_with_workspaces(&["test"]);
        let group_idx = state.active_group;
        state.groups[group_idx]
            .favorite_agent_profile_ids
            .push("system:pi".to_string());
        state.session_dirty = false;

        open_settings_at(&mut state, SettingsSection::Agents);
        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
        );

        assert_eq!(action, None);
        assert_eq!(
            state.groups[group_idx].favorite_agent_profile_ids,
            vec!["system:pi".to_string()]
        );
        assert!(!state.session_dirty);

        state.integration_recommendations = vec![integration_recommendation_for(
            crate::api::schema::IntegrationTarget::Pi,
            crate::integration::IntegrationStatusKind::Current,
            true,
        )];
        open_group_settings(&mut state, group_idx);
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Left, KeyModifiers::empty()),
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Left, KeyModifiers::empty()),
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
        );

        assert_eq!(action, None);
        assert_eq!(state.settings.section, SettingsSection::GroupProfiles);
        assert!(state.groups[group_idx]
            .favorite_agent_profile_ids
            .is_empty());
        assert!(state.session_dirty);
    }

    #[test]
    fn agent_settings_rows_manage_custom_profiles_only() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Agents);

        let rows = rows_for_section(&state, SettingsSection::Agents).expect("agent rows");

        assert!(rows.iter().any(|row| {
            matches!(
                row,
                crate::settings_rows::SettingsListRow::Action { label, .. }
                    if label.as_ref() == "New Agent Profile"
            )
        }));
        assert!(!rows.iter().any(|row| {
            matches!(
                row,
                crate::settings_rows::SettingsListRow::Profile { name, .. }
                    if name.as_ref() == "Omp" || name.as_ref() == "Codex"
            )
        }));
    }

    #[test]
    fn agent_settings_left_right_moves_settings_section_not_family_filter() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Agents);

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Right, KeyModifiers::empty()),
        );

        assert_eq!(state.settings.section, SettingsSection::Integrations);
        assert_eq!(state.settings.agent_profile_kind_filter, None);
    }

    #[test]
    fn agent_settings_shift_left_right_moves_settings_section_not_family_filter() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Agents);

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT),
        );

        assert_eq!(state.settings.section, SettingsSection::Integrations);
        assert_eq!(state.settings.agent_profile_kind_filter, None);
    }

    #[test]
    fn agent_settings_custom_kind_is_launch_only_and_saveable() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Agents);
        state.settings.pending_agent_profile_name = Some("kilocode".to_string());
        state.settings.pending_agent_profile_command = Some("kilocode --profile main".to_string());
        state.settings.pending_agent_profile_kind = Some(crate::agent_profiles::AgentKind::Custom);
        state.settings.list.selected = agent_profile_save_index(&state);
        state.settings.list.show();
        state.settings.list.show();

        let rows = rows_for_section(&state, SettingsSection::Agents).expect("agent rows");
        assert!(rows.iter().any(|row| {
            matches!(
                row,
                crate::settings_rows::SettingsListRow::Caption(text)
                    if text.as_ref().contains("Custom commands are launch-only")
                        && text.as_ref().contains("Status, restore, and integrations are unavailable")
            )
        }));
        assert!(!rows.iter().any(|row| {
            matches!(
                row,
                crate::settings_rows::SettingsListRow::Caption(text)
                    if text.as_ref().contains("/resume")
            )
        }));

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(
            action,
            Some(SettingsAction::SaveAgentProfile(
                crate::agent_profiles::UserAgentProfileConfig {
                    id: "kilocode".to_string(),
                    name: "kilocode".to_string(),
                    kind: crate::agent_profiles::AgentKind::Custom,
                    command: "kilocode --profile main".to_string(),
                    env: std::collections::BTreeMap::new(),
                    enabled: true,
                }
            ))
        );
    }

    #[test]
    fn agent_settings_custom_tab_filters_to_unsupported_profiles() {
        let mut state = state_with_workspaces(&["test"]);
        state.agent_profiles = crate::agent_profiles::AgentProfileCatalog::from_config(
            &crate::agent_profiles::AgentProfilesConfig {
                order: vec!["user:kilocode".to_string()],
                custom: vec![crate::agent_profiles::UserAgentProfileConfig {
                    id: "kilocode".to_string(),
                    name: "kilocode".to_string(),
                    kind: crate::agent_profiles::AgentKind::Custom,
                    command: "kilocode".to_string(),
                    env: std::collections::BTreeMap::new(),
                    enabled: true,
                }],
            },
        );
        open_settings_at(&mut state, SettingsSection::Agents);
        state.settings.agent_profile_kind_filter = Some(crate::agent_profiles::AgentKind::Custom);

        let rows = rows_for_section(&state, SettingsSection::Agents).expect("agent rows");

        assert!(rows.iter().any(|row| {
            matches!(
                row,
                crate::settings_rows::SettingsListRow::Profile { name, detail, .. }
                    if name.as_ref() == "kilocode" && detail.as_ref().contains("Launch-Only")
            )
        }));
        assert!(!rows.iter().any(|row| {
            matches!(
                row,
                crate::settings_rows::SettingsListRow::Profile { name, .. }
                    if name.as_ref() == "Omp"
            )
        }));
    }

    #[test]
    fn agent_settings_hover_tracks_visual_rows_without_moving_selection() {
        let mut app = app_for_mouse_test();
        app.state.view.terminal_area = Rect::new(0, 0, 100, 40);
        open_settings_at(&mut app.state, SettingsSection::Agents);
        open_blank_agent_profile_editor(&mut app.state);

        app.state.settings.list.selected = 9;
        app.state.settings.list.show();
        let list_area = settings_section_list_rect(&app.state, SettingsSection::Agents);
        let rows = rows_for_section(&app.state, SettingsSection::Agents).unwrap();
        let row_for = |index| selected_visual_row(&rows, index).unwrap() as u16;
        app.handle_mouse(mouse(
            MouseEventKind::Moved,
            list_area.x + 2,
            list_area.y + 1,
        ));
        assert_eq!(app.state.settings.list.selected, 9);
        assert_eq!(app.state.settings.list.visible(), None);

        app.handle_mouse(mouse(
            MouseEventKind::Moved,
            list_area.x + 2,
            list_area.y + row_for(0),
        ));
        assert_eq!(app.state.settings.list.selected, 9);
        assert_eq!(app.state.settings.list.visible(), None);

        app.handle_mouse(mouse(
            MouseEventKind::Moved,
            list_area.x + 2,
            list_area.y + row_for(1),
        ));
        assert_eq!(app.state.settings.list.selected, 9);
        assert_eq!(app.state.settings.list.visible(), Some(1));
    }

    #[test]
    fn group_profiles_ctrl_f_toggles_favorite_immediately() {
        let mut state = state_with_workspaces(&["test"]);
        let group_idx = state.create_group("Side".to_string());

        state.integration_recommendations = vec![integration_recommendation_for(
            crate::api::schema::IntegrationTarget::Codex,
            crate::integration::IntegrationStatusKind::Current,
            true,
        )];
        open_group_settings(&mut state, group_idx);
        state.settings.section = SettingsSection::GroupProfiles;
        state.settings.list.selected = 0;
        state.settings.list.show();
        state.settings.list.show();
        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
        );

        assert_eq!(action, None);
        assert_eq!(
            state.groups[group_idx].favorite_agent_profile_ids,
            vec!["system:codex".to_string()]
        );
        assert!(state.session_dirty);

        state.session_dirty = false;
        state.settings.list.selected = 0;
        state.settings.list.show();
        state.settings.list.show();
        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
        );

        assert_eq!(action, None);
        assert!(state.groups[group_idx]
            .favorite_agent_profile_ids
            .is_empty());
        assert!(state.session_dirty);
    }

    #[test]
    fn group_profiles_ctrl_d_toggles_default_and_favorites_profile() {
        let mut state = state_with_workspaces(&["test"]);
        let group_idx = state.create_group("Side".to_string());

        state.integration_recommendations = vec![
            integration_recommendation_for(
                crate::api::schema::IntegrationTarget::Codex,
                crate::integration::IntegrationStatusKind::Current,
                true,
            ),
            integration_recommendation_for(
                crate::api::schema::IntegrationTarget::Claude,
                crate::integration::IntegrationStatusKind::Current,
                true,
            ),
        ];
        open_group_settings(&mut state, group_idx);
        state.settings.section = SettingsSection::GroupProfiles;
        state.settings.list.selected = 1;
        state.settings.list.show();
        state.settings.list.show();
        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        );

        assert_eq!(action, None);
        assert_eq!(
            state.groups[group_idx].default_agent_profile_id.as_deref(),
            Some("system:codex")
        );
        assert_eq!(
            state.groups[group_idx].favorite_agent_profile_ids,
            vec!["system:codex".to_string()]
        );
        assert!(state.session_dirty);

        state.session_dirty = false;
        state.settings.list.selected = 0;
        state.settings.list.show();
        state.settings.list.show();
        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        );

        assert_eq!(action, None);
        assert!(state.groups[group_idx].default_agent_profile_id.is_none());
        assert_eq!(
            state.groups[group_idx].favorite_agent_profile_ids,
            vec!["system:codex".to_string()]
        );
        assert!(state.session_dirty);
    }

    #[test]
    fn group_profiles_do_not_use_family_filters() {
        let mut state = state_with_workspaces(&["test"]);
        let group_idx = state.create_group("Side".to_string());

        state.integration_recommendations = vec![
            integration_recommendation_for(
                crate::api::schema::IntegrationTarget::Codex,
                crate::integration::IntegrationStatusKind::Current,
                true,
            ),
            integration_recommendation_for(
                crate::api::schema::IntegrationTarget::Claude,
                crate::integration::IntegrationStatusKind::Current,
                true,
            ),
        ];
        open_group_settings(&mut state, group_idx);
        state.settings.section = SettingsSection::GroupProfiles;
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT),
        );
        assert_eq!(state.settings.agent_profile_kind_filter, None);

        state.settings.list.selected = 1;
        state.settings.list.show();
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
        );
        assert_eq!(
            state.groups[group_idx].favorite_agent_profile_ids,
            vec!["system:codex".to_string()]
        );
    }

    #[test]
    fn group_settings_tabs_follow_general_defaults_appearance_agents_github_order() {
        let mut state = state_with_workspaces(&["test"]);
        let group_idx = state.create_group("Side".to_string());

        open_group_settings(&mut state, group_idx);
        assert_eq!(state.settings.section, SettingsSection::GroupGeneral);

        for expected in [
            SettingsSection::GroupDefaults,
            SettingsSection::Theme,
            SettingsSection::GroupProfiles,
            SettingsSection::GroupGithub,
            SettingsSection::GroupGeneral,
        ] {
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()),
            );
            assert_eq!(state.settings.section, expected);
            assert_eq!(state.settings.group_settings_target, Some(group_idx));
            assert!(state.settings.focused_input.is_none());
        }

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::BackTab, KeyModifiers::empty()),
        );
        assert_eq!(state.settings.section, SettingsSection::GroupGithub);
    }
    #[test]
    fn scoped_settings_tabs_render_only_their_controls_and_save_github_names() {
        let mut group_app = app_for_mouse_test();
        let group_idx = group_app.state.create_group("Side".to_string());
        open_group_settings(&mut group_app.state, group_idx);
        crate::ui::compute_view(&mut group_app.state, Rect::new(0, 0, 150, 45));
        assert!(rendered_contains_text(&group_app, "Name", 150, 45));
        assert!(!rendered_contains_text(&group_app, "Directory", 150, 45));
        assert!(!rendered_contains_text(
            &group_app,
            "GitHub Organization",
            150,
            45
        ));

        click_settings_tab(&mut group_app, SettingsSection::GroupDefaults);
        crate::ui::compute_view(&mut group_app.state, Rect::new(0, 0, 150, 45));
        assert!(rendered_contains_text(&group_app, "Directory", 150, 45));
        assert!(!rendered_contains_text(
            &group_app,
            "GitHub Organization",
            150,
            45
        ));

        click_settings_tab(&mut group_app, SettingsSection::GroupGithub);
        crate::ui::compute_view(&mut group_app.state, Rect::new(0, 0, 150, 45));
        assert!(rendered_contains_text(
            &group_app,
            "GitHub Organization",
            150,
            45
        ));
        assert!(!rendered_contains_text(&group_app, "Directory", 150, 45));
        let (x, y) = rendered_text_point(&group_app, "GitHub Organization", 150, 45);
        assert_eq!(
            group_app.state.handle_settings_mouse(mouse(
                MouseEventKind::Down(MouseButton::Left),
                x,
                y + 1
            )),
            None
        );
        for character in "acme-tools".chars() {
            group_app.handle_settings_key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::empty(),
            ));
        }
        assert!(group_app.state.groups[group_idx]
            .github_organization
            .is_none());
        click_settings_tab(&mut group_app, SettingsSection::GroupDefaults);
        click_settings_tab(&mut group_app, SettingsSection::GroupGithub);
        let (x, y) = rendered_text_point(&group_app, "acme-tools", 150, 45);
        group_app
            .state
            .handle_settings_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y));
        group_app.handle_settings_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert_eq!(
            group_app.state.groups[group_idx]
                .github_organization
                .as_ref()
                .map(crate::app::state::GithubOrganization::as_str),
            Some("acme-tools")
        );

        let mut workspace_app = app_for_mouse_test();
        workspace_app.state.workspaces = vec![crate::workspace::Workspace::test_new("space")];
        open_workspace_settings(&mut workspace_app.state, 0);
        crate::ui::compute_view(&mut workspace_app.state, Rect::new(0, 0, 150, 45));
        assert!(rendered_contains_text(&workspace_app, "Directory", 150, 45));
        assert!(!rendered_contains_text(
            &workspace_app,
            "Repositories (owner/repository)",
            150,
            45
        ));
        click_settings_tab(&mut workspace_app, SettingsSection::WorkspaceGithub);
        crate::ui::compute_view(&mut workspace_app.state, Rect::new(0, 0, 150, 45));
        assert!(rendered_contains_text(
            &workspace_app,
            "Repositories (owner/repository)",
            150,
            45
        ));
        assert!(!rendered_contains_text(
            &workspace_app,
            "Directory",
            150,
            45
        ));
    }

    #[test]
    fn group_general_settings_edits_name_inline_and_opens_delete_action() {
        let mut state = state_with_workspaces(&["test"]);
        let group_idx = state.create_group("Side".to_string());

        open_group_settings(&mut state, group_idx);
        state.settings.section = SettingsSection::GroupGeneral;
        state.settings.list.selected = 0;
        state.settings.list.show();
        state.settings.list.show();
        state.settings.focused_input = Some(0);
        let first_action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        assert_eq!(
            first_action,
            Some(SettingsAction::SaveGroupName {
                group_idx,
                name: "Side".to_string(),
            })
        );
        let second_action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char('A'), KeyModifiers::empty()),
        );
        assert_eq!(
            second_action,
            Some(SettingsAction::SaveGroupName {
                group_idx,
                name: "Side A".to_string(),
            })
        );
        assert_eq!(state.settings.pending_group_name.as_deref(), Some("Side A"));
        let rename_action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );
        assert_eq!(
            rename_action,
            Some(SettingsAction::SaveGroupName {
                group_idx,
                name: "Side A".to_string(),
            })
        );
        assert_eq!(state.mode, Mode::Settings);
        assert_eq!(state.settings.group_settings_target, Some(group_idx));

        open_group_settings(&mut state, group_idx);
        state.settings.section = SettingsSection::GroupGeneral;
        state.settings.list.selected = crate::settings_rows::GROUP_GENERAL_DELETE;
        state.settings.list.show();
        state.settings.focused_input = None;
        let delete_action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );
        assert_eq!(delete_action, Some(SettingsAction::DeleteGroup(group_idx)));
        assert_eq!(state.mode, Mode::Terminal);
        assert_eq!(state.settings.group_settings_target, None);
    }

    #[test]
    fn group_github_settings_clears_organization() {
        let mut app = app_for_mouse_test();
        let group_idx = app.state.create_group("Side".to_string());
        app.state.groups[group_idx].github_organization =
            crate::app::state::GithubOrganization::parse("masakirocorp")
                .expect("valid organization");

        open_group_settings(&mut app.state, group_idx);
        app.state.settings.section = SettingsSection::GroupGithub;
        app.state.settings.list.show();
        app.state.settings.list.select(GROUP_GITHUB_ORGANIZATION);
        app.state.settings.pending_group_github_organization = Some(String::new());

        app.handle_settings_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert_eq!(app.state.groups[group_idx].github_organization, None);
    }

    #[test]
    fn group_github_organization_accepts_navigation_letters() {
        let mut app = app_for_mouse_test();
        let group_idx = app.state.create_group("Side".to_string());
        open_group_settings(&mut app.state, group_idx);
        app.state.settings.section = SettingsSection::GroupGithub;
        app.state.settings.list.show();
        app.state.settings.list.select(GROUP_GITHUB_ORGANIZATION);
        app.state.settings.focused_input = Some(GROUP_GITHUB_ORGANIZATION);
        app.state.settings.pending_group_github_organization = Some(String::new());
        for character in ['h', 'j', 'k', 'l'] {
            app.handle_settings_key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::empty(),
            ));
        }
        app.handle_settings_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert_eq!(
            app.state.groups[group_idx]
                .github_organization
                .as_ref()
                .map(crate::app::state::GithubOrganization::as_str),
            Some("hjkl")
        );
        assert_eq!(
            app.state.settings.focused_input,
            Some(GROUP_GITHUB_ORGANIZATION)
        );
    }
    #[test]
    fn group_github_organization_saves_hyphenated_name_only_on_enter() {
        let mut app = app_for_mouse_test();
        let group_idx = app.state.create_group("Flex".to_string());
        let original = crate::app::state::GithubOrganization::parse("masakirocorp")
            .expect("valid organization");
        app.state.groups[group_idx].github_organization = original.clone();
        open_group_settings(&mut app.state, group_idx);
        app.state.settings.section = SettingsSection::GroupGithub;
        app.state.settings.list.show();
        app.state.settings.list.select(GROUP_GITHUB_ORGANIZATION);
        app.state.settings.focused_input = Some(GROUP_GITHUB_ORGANIZATION);
        app.handle_settings_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));

        for character in "flex-rental-solutions".chars() {
            app.handle_settings_key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::empty(),
            ));
            assert!(
                app.state.toast.is_none(),
                "typing {character:?} raised an error"
            );
        }
        assert_eq!(app.state.groups[group_idx].github_organization, original);

        app.handle_settings_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert_eq!(
            app.state.groups[group_idx]
                .github_organization
                .as_ref()
                .map(crate::app::state::GithubOrganization::as_str),
            Some("flex-rental-solutions")
        );
        open_group_settings(&mut app.state, group_idx);
        app.state.settings.section = SettingsSection::GroupGithub;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 100, 40));
        rendered_text_point(&app, "flex-rental-solutions", 100, 40);
    }

    #[test]
    fn group_github_organization_rejects_invalid_value() {
        let mut app = app_for_mouse_test();
        let group_idx = app.state.create_group("Side".to_string());
        let original = crate::app::state::GithubOrganization::parse("masakirocorp")
            .expect("valid organization");
        app.state.groups[group_idx].github_organization = original.clone();

        open_group_settings(&mut app.state, group_idx);
        app.state.settings.section = SettingsSection::GroupGithub;
        app.state.settings.list.show();
        app.state.settings.list.select(GROUP_GITHUB_ORGANIZATION);
        app.state.settings.focused_input = Some(GROUP_GITHUB_ORGANIZATION);
        app.state.settings.pending_group_github_organization = Some("invalid--org".to_string());

        app.handle_settings_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert_eq!(app.state.groups[group_idx].github_organization, original);
        let toast = app
            .state
            .toast
            .as_ref()
            .expect("invalid organization toast");
        assert_eq!(toast.title, "GitHub Organization Not Saved");
        assert!(toast.context.contains("single hyphens"));
    }

    #[test]
    fn group_defaults_settings_edits_default_location_for_future_spaces_inline() {
        let mut state = state_with_workspaces(&["test"]);
        let group_idx = state.create_group("Side".to_string());
        state.set_group_default_location(
            group_idx,
            Some(crate::execution_host::ResourceLocation::local("/tmp/gardn-old").unwrap()),
        );
        open_group_settings(&mut state, group_idx);
        state.settings.section = SettingsSection::GroupDefaults;
        state.settings.list.selected = crate::settings_rows::GROUP_DEFAULTS_DIRECTORY;
        state.settings.list.show();
        state.settings.focused_input = Some(crate::settings_rows::GROUP_DEFAULTS_DIRECTORY);

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char('2'), KeyModifiers::empty()),
        );

        assert_eq!(
            action,
            Some(SettingsAction::SaveGroupDefaultLocation {
                group_idx,
                default_location: Some(
                    crate::execution_host::ResourceLocation::local("/tmp/gardn-old2").unwrap(),
                ),
            })
        );
    }

    #[test]
    fn group_defaults_settings_cycles_the_execution_host() {
        let (_lock, _xdg) = isolated_ssh_catalog("group-host-cycle");
        let mut app = app_for_mouse_test();
        let profile = seed_connection_profile(
            &mut app,
            "robotbox",
            "Robotbox",
            "robotbox",
            Some("/home/charlie/projects"),
        );
        let group_idx = app.state.create_group("Side".to_string());
        app.state.set_group_default_location(
            group_idx,
            Some(crate::execution_host::ResourceLocation::local("/tmp/work").unwrap()),
        );
        open_group_settings(&mut app.state, group_idx);
        app.state.settings.section = SettingsSection::GroupDefaults;
        app.state.settings.list.selected = crate::settings_rows::GROUP_DEFAULTS_HOST;

        app.state.settings.list.show();

        let action = update_settings_state(
            &mut app.state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert_eq!(
            action,
            Some(SettingsAction::SaveGroupDefaultLocation {
                group_idx,
                default_location: Some(crate::execution_host::ResourceLocation::new(
                    profile.execution_host_id(),
                    crate::execution_host::HostPath::new("/tmp/work").unwrap(),
                )),
            })
        );
    }

    #[test]
    fn group_general_settings_opens_icon_picker_and_saves_a_clicked_icon() {
        let mut app = app_for_mouse_test();
        app.state.view.terminal_area = Rect::new(26, 0, 100, 30);
        let group_idx = app.state.create_group("Side".to_string());
        open_group_settings(&mut app.state, group_idx);
        app.state.settings.section = SettingsSection::GroupGeneral;
        app.state.settings.list.selected = crate::settings_rows::GROUP_GENERAL_ICON;

        app.state.settings.list.show();

        let action = update_settings_state(
            &mut app.state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        assert_eq!(action, None);
        assert!(app.state.settings.group_icon_picker_open);

        let list = settings_section_list_geometry(&app.state, SettingsSection::GroupGeneral);
        let rows = rows_for_section(&app.state, SettingsSection::GroupGeneral).expect("group rows");
        let icon_visual =
            selected_visual_row(&rows, crate::settings_rows::GROUP_GENERAL_ICON).expect("icon row");
        let origin = Rect::new(
            list.rect.x + 2,
            list.rect.y + icon_visual as u16 + 1,
            list.rect.width.saturating_sub(2).min(24),
            crate::ui::group_icon_picker_row_count(),
        );
        let (cell, icon) = crate::ui::group_icon_picker_rects_at(origin)
            .into_iter()
            .find(|(_, candidate)| *candidate == crate::app::state::GROUP_ICONS[1])
            .expect("next group icon cell");

        let action = app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            cell.x,
            cell.y,
        ));
        assert_eq!(
            action,
            Some(SettingsAction::SaveGroupIcon {
                group_idx,
                icon: icon.to_string(),
            })
        );
        assert!(!app.state.settings.group_icon_picker_open);
    }

    #[test]
    fn group_defaults_settings_cycles_ssh_host_without_a_directory() {
        let (_lock, _xdg) = isolated_ssh_catalog("group-host-cycle-empty-dir");
        let mut app = app_for_mouse_test();
        let profile = seed_connection_profile(
            &mut app,
            "robotbox",
            "Robotbox",
            "robotbox",
            Some("/home/charlie/projects"),
        );
        let group_idx = app.state.create_group("Side".to_string());
        open_group_settings(&mut app.state, group_idx);
        app.state.settings.section = SettingsSection::GroupDefaults;
        app.state.settings.list.selected = crate::settings_rows::GROUP_DEFAULTS_HOST;
        app.state.settings.list.show();

        let action = update_settings_state(
            &mut app.state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert_eq!(
            action,
            Some(SettingsAction::SaveGroupDefaultLocation {
                group_idx,
                default_location: Some(crate::execution_host::ResourceLocation::new(
                    profile.execution_host_id(),
                    crate::execution_host::HostPath::new("/home/charlie/projects").unwrap(),
                )),
            })
        );
    }

    #[test]
    fn group_general_keyboard_navigation_focuses_editable_rows() {
        let mut state = state_with_workspaces(&["test"]);
        let group_idx = state.create_group("Side".to_string());
        open_group_settings(&mut state, group_idx);
        state.settings.section = SettingsSection::GroupGeneral;
        state.settings.list =
            crate::app::state::ModalListState::hidden(crate::settings_rows::GROUP_GENERAL_DELETE);

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        assert!(state.settings.list.is_active());
        assert!(state.settings.focused_input.is_some());

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char('2'), KeyModifiers::empty()),
        );
        assert_eq!(
            action,
            Some(SettingsAction::SaveGroupName {
                group_idx,
                name: "Side2".to_string(),
            })
        );
    }

    #[test]
    fn group_general_mouse_hover_is_inert_and_click_focuses_name_without_saving() {
        let mut app = app_for_mouse_test();
        let group_idx = app.state.create_group("Side".to_string());
        app.state.view.terminal_area = Rect::new(26, 0, 100, 30);
        open_group_settings(&mut app.state, group_idx);
        app.state.settings.list.hide();
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 126, 30));

        let (x, y) = rendered_text_point(&app, "Name", 126, 30);
        assert_eq!(
            app.state
                .handle_settings_mouse(mouse(MouseEventKind::Moved, x, y)),
            None
        );
        assert!(!app.state.settings.list.is_active());

        assert_eq!(
            app.state.handle_settings_mouse(mouse(
                MouseEventKind::Down(MouseButton::Left),
                x,
                y + 1,
            )),
            None
        );
        assert!(app.state.settings.focused_input.is_some());
        let edit_action = update_settings_state(
            &mut app.state,
            KeyEvent::new(KeyCode::Char('!'), KeyModifiers::empty()),
        );
        assert_eq!(
            edit_action,
            Some(SettingsAction::SaveGroupName {
                group_idx,
                name: "Side!".to_string(),
            })
        );
        assert_eq!(
            app.state.settings.pending_group_name.as_deref(),
            Some("Side!")
        );
        assert_eq!(app.state.mode, Mode::Settings);
        assert_eq!(app.state.groups[group_idx].name, "Side");
    }

    #[test]
    fn group_settings_uses_full_settings_modal() {
        let mut state = state_with_workspaces(&["test"]);
        let group_idx = state.create_group("Side".to_string());
        state.view.terminal_area = Rect::new(0, 0, 100, 40);

        open_group_settings(&mut state, group_idx);
        let group_rect = state.settings_popup_rect();
        open_settings(&mut state);
        let settings_rect = state.settings_popup_rect();
        assert_eq!(group_rect, settings_rect);
    }

    #[test]
    fn workspace_general_settings_edits_name_and_default_location_inline() {
        let mut state = state_with_workspaces(&["space"]);
        state.workspaces[0].record_default_location(
            crate::execution_host::ResourceLocation::local("/tmp/gardn-old").unwrap(),
        );

        open_workspace_settings(&mut state, 0);
        assert_eq!(state.settings.section, SettingsSection::WorkspaceGeneral);
        assert_eq!(state.settings.workspace_settings_target, Some(0));
        assert_eq!(
            state.settings.pending_workspace_name.as_deref(),
            Some("space")
        );
        assert_eq!(
            state.settings.pending_workspace_default_cwd.as_deref(),
            Some("/tmp/gardn-old")
        );

        state.settings.list.selected = 0;
        state.settings.list.show();
        state.settings.focused_input = Some(0);
        let name_action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char('2'), KeyModifiers::empty()),
        );
        assert_eq!(
            name_action,
            Some(SettingsAction::SaveWorkspaceName {
                ws_idx: 0,
                name: "space2".to_string(),
            })
        );

        state.settings.list.selected = 2;
        state.settings.focused_input = Some(2);
        let cwd_action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char('2'), KeyModifiers::empty()),
        );
        assert_eq!(
            cwd_action,
            Some(SettingsAction::SaveWorkspaceDefaultLocation {
                ws_idx: 0,
                location: crate::execution_host::ResourceLocation::local("/tmp/gardn-old2")
                    .unwrap(),
            })
        );
    }

    #[test]
    fn workspace_github_scope_input_saves_and_reopens_selected_repositories() {
        let mut app = app_for_mouse_test();
        app.state.workspaces = vec![crate::workspace::Workspace::test_new("space")];
        let group_idx = app.state.create_group("Team".to_string());
        app.state.groups[group_idx].github_organization =
            crate::app::state::GithubOrganization::parse("masakirocorp").unwrap();
        app.state.workspaces[0].group_id = app.state.groups[group_idx].id.clone();

        open_workspace_settings(&mut app.state, 0);
        switch_settings_section(&mut app.state, SettingsSection::WorkspaceGithub, 0);
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 150, 45));
        let (x, y) = rendered_text_point(&app, "Repositories (owner/repository)", 150, 45);
        app.state
            .handle_settings_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y + 1));
        for character in "Jack/Onex".chars() {
            app.handle_settings_key(KeyEvent::new(
                KeyCode::Char(character),
                KeyModifiers::empty(),
            ));
        }
        app.handle_settings_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()));
        assert_eq!(
            paste_settings_text(&mut app.state, ", jack/Two"),
            Some(None)
        );
        rendered_text_point(&app, "Jack/One, jack/Two", 150, 45);
        assert_eq!(
            app.state.workspaces[0].github_scope,
            crate::github::GithubRepositoryScope::Automatic
        );
        app.handle_settings_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));
        assert_eq!(
            app.state.workspaces[0]
                .github_scope
                .selected_repositories()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["jack/one", "jack/two"],
        );

        open_workspace_settings(&mut app.state, 0);
        switch_settings_section(&mut app.state, SettingsSection::WorkspaceGithub, 0);
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 150, 45));
        rendered_text_point(&app, "jack/one, jack/two", 150, 45);
        for (label, expected) in [
            (
                "Group organization",
                crate::github::GithubRepositoryScope::GroupOrganization,
            ),
            ("Automatic", crate::github::GithubRepositoryScope::Automatic),
        ] {
            let (x, y) = rendered_text_point(&app, label, 150, 45);
            let action = app
                .state
                .handle_settings_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y))
                .expect("click saves scope choice");
            app.apply_settings_action(action);
            assert_eq!(app.state.workspaces[0].github_scope, expected);
        }
        let (x, y) = rendered_text_point(&app, "Selected repositories", 150, 45);
        let action = app
            .state
            .handle_settings_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, y))
            .expect("click restores selected repositories");
        app.apply_settings_action(action);
        assert_eq!(
            app.state.workspaces[0]
                .github_scope
                .selected_repositories()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["jack/one", "jack/two"],
        );
    }

    #[test]
    fn workspace_general_settings_cycles_the_execution_host_override() {
        let (_lock, _xdg) = isolated_ssh_catalog("workspace-host-cycle");
        let mut app = app_for_mouse_test();
        let profile = seed_connection_profile(
            &mut app,
            "robotbox",
            "Robotbox",
            "robotbox",
            Some("/home/charlie/projects"),
        );
        app.state.workspaces = vec![crate::workspace::Workspace::test_new("space")];
        app.state.workspaces[0].record_default_location(
            crate::execution_host::ResourceLocation::local("/tmp/work").unwrap(),
        );
        open_workspace_settings(&mut app.state, 0);
        app.state.settings.list.selected = 1;
        app.state.settings.list.show();

        let action = update_settings_state(
            &mut app.state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert_eq!(
            action,
            Some(SettingsAction::SaveWorkspaceDefaultLocation {
                ws_idx: 0,
                location: crate::execution_host::ResourceLocation::new(
                    profile.execution_host_id(),
                    crate::execution_host::HostPath::new("/tmp/work").unwrap(),
                ),
            })
        );
    }

    #[test]
    fn group_accent_selection_saves_immediately_with_right_sidebar() {
        let mut app = app_for_mouse_test();
        app.state.view.right_sidebar_rect = Rect::new(106, 0, 34, 20);
        let group_idx = app.state.create_group("Side".to_string());

        open_group_settings(&mut app.state, group_idx);
        app.state.settings.section = SettingsSection::Theme;
        app.state.settings.list.selected = 1;
        app.state.settings.list.show();
        app.state.settings.list.show();
        app.handle_settings_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()));

        assert_eq!(
            app.state.groups[group_idx].accent,
            Some(TerminalAccent::Blue)
        );
        assert_eq!(app.state.mode, Mode::Settings);
    }

    #[test]
    fn global_theme_settings_apply_returns_pending_settings() {
        let mut state = state_with_workspaces(&["test"]);

        open_settings(&mut state);
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert_eq!(
            action,
            Some(SettingsAction::SaveSettings {
                light: "catppuccin-latte".to_string(),
                dark: "catppuccin".to_string(),
                mode: ThemeMode::System,
                terminal_light_accent: TerminalAccent::Blue,
                terminal_dark_accent: TerminalAccent::Blue,
                sound_enabled: false,
                toast_delivery: ToastDelivery::Off,
                confirm_close: true,
                prompt_new_tab_name: true,
                show_counters: false,
                pane_borders: true,
                pane_scrollbars: true,
                pane_gaps: true,
                hide_tab_bar_when_single_tab: false,
                copy_on_select: true,
                prompt_new_workspace_name: false,
                right_click_passthrough_modifier: RightClickPassthroughModifierConfig::default(),
                new_terminal_cwd: NewTerminalCwdConfig::Follow,
                mouse_scroll_lines: crate::config::DEFAULT_MOUSE_SCROLL_LINES,
                browser_command: "terminal-browser".to_string(),
                review_command: "hunk diff --watch".to_string(),
                editor_command: "fresh .".to_string(),
                sidebar_width: 26,
                sidebar_min_width: 18,
                sidebar_max_width: 36,
                sidebar_arrangement: SidebarArrangementConfig::Auto,
                context_bar_visibility: ContextBarVisibilityConfig::Always,
                sidebar_initial_state: SidebarInitialStateConfig::Expanded,
                sidebar_initial_agent_scope: AgentPanelScopeConfig::All,
                pane_border_agent_info: PaneBorderAgentInfoConfig::Hidden,
                status_indicators: StatusIndicatorStyle::Dots,
            })
        );
        assert_eq!(state.mode, Mode::Settings);
    }

    #[test]
    fn behavior_settings_toggle_counter_visibility() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings(&mut state);
        state.settings.section = SettingsSection::PaneLabels;
        state.settings.list.selected = 3;
        state.settings.list.show();

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert!(matches!(
            action,
            Some(SettingsAction::SaveSettings {
                show_counters: true,
                ..
            })
        ));
        assert_eq!(state.settings.pending_show_counters, Some(true));
    }

    #[test]
    fn terminal_theme_accent_selection_is_saved() {
        let mut state = state_with_workspaces(&["test"]);
        state.global_theme_mode = ThemeMode::System;
        state.global_light_theme_name = "system".to_string();
        state.global_dark_theme_name = "system".to_string();
        state.global_terminal_light_accent = TerminalAccent::Blue;
        state.global_terminal_dark_accent = TerminalAccent::Red;

        open_settings(&mut state);
        for _ in 0..4 {
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
            );
        }
        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert!(matches!(
            action,
            Some(SettingsAction::SaveSettings {
                light,
                dark,
                mode: ThemeMode::System,
                terminal_light_accent: TerminalAccent::Magenta,
                terminal_dark_accent: TerminalAccent::Red,
                ..
            }) if light == "system" && dark == "system"
        ));
    }

    #[test]
    fn dark_accent_rows_skip_heading_before_options() {
        let mut state = state_with_workspaces(&["test"]);
        state.global_theme_mode = ThemeMode::System;
        state.global_light_theme_name = "system".to_string();
        state.global_dark_theme_name = "system".to_string();
        open_settings(&mut state);

        let choices = theme_settings_choices(&state);
        let first_dark_accent_selection = choices
            .iter()
            .position(|choice| matches!(choice, ThemeSettingsChoice::TerminalDarkAccent(_)))
            .expect("first dark accent option");
        let first_dark_accent_row =
            theme_visual_row_for_selection(&state, first_dark_accent_selection);

        assert!(first_dark_accent_row > 0);
        assert_eq!(
            option_hit_for_visual_row(&theme_rows(&state), first_dark_accent_row - 1)
                .map(|hit| hit.index),
            None
        );

        let mut dark_accent_count = 0;
        for (selection, choice) in choices.iter().enumerate() {
            if matches!(choice, ThemeSettingsChoice::TerminalDarkAccent(_)) {
                let row = theme_visual_row_for_selection(&state, selection);
                assert_eq!(
                    option_hit_for_visual_row(&theme_rows(&state), row).map(|hit| hit.index),
                    Some(selection)
                );
                dark_accent_count += 1;
            }
        }
        assert_eq!(dark_accent_count, TerminalAccent::ALL.len());
    }

    #[test]
    fn space_selects_theme_without_closing_settings() {
        let mut state = state_with_workspaces(&["test"]);

        open_settings(&mut state);
        state.settings.list.selected = 2 + ThemeMode::ALL.len() + 1;
        state.settings.list.show();
        state.settings.list.show();
        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert!(matches!(action, Some(SettingsAction::SaveSettings { .. })));
        assert_eq!(
            state.settings.pending_light_theme_name.as_deref(),
            theme_names_for_appearance(ThemeAppearance::Light)
                .get(1)
                .copied()
        );
        assert_eq!(state.mode, Mode::Settings);
    }

    #[test]
    fn settings_action_keys_do_nothing_without_active_selection() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Sound);
        assert!(!state.settings.list.is_active());
        state.settings.list.hover(Some(1));
        assert_eq!(state.settings.list.visible(), Some(1));

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert_eq!(action, None);
        assert_eq!(
            state.settings.pending_sound_enabled,
            Some(state.sound_enabled())
        );
    }
    #[test]
    fn space_switches_between_system_source_and_custom_themes() {
        let mut state = state_with_workspaces(&["test"]);

        open_settings(&mut state);
        state.settings.pending_theme_mode = Some(ThemeMode::System);
        state.settings.pending_light_theme_name = Some("system".to_string());
        state.settings.pending_dark_theme_name = Some("system".to_string());
        state.settings.list.selected = 1;
        state.settings.list.show();
        state.settings.list.show();

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert!(matches!(action, Some(SettingsAction::SaveSettings { .. })));
        assert_eq!(state.settings.pending_theme_mode, Some(ThemeMode::System));
        assert_eq!(
            state.settings.pending_light_theme_name.as_deref(),
            Some(crate::app::state::DEFAULT_LIGHT_THEME_NAME)
        );
        assert_eq!(
            state.settings.pending_dark_theme_name.as_deref(),
            Some(crate::app::state::DEFAULT_DARK_THEME_NAME)
        );

        state.settings.list.selected = 0;
        state.settings.list.show();
        state.settings.list.show();
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert_eq!(state.settings.pending_theme_mode, Some(ThemeMode::System));
        assert_eq!(
            state.settings.pending_light_theme_name.as_deref(),
            Some("system")
        );
        assert_eq!(
            state.settings.pending_dark_theme_name.as_deref(),
            Some("system")
        );
    }
    #[test]
    fn settings_sound_space_updates_pending_value_without_closing() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings(&mut state);
        state.settings.section = crate::app::state::SettingsSection::Sound;
        state.settings.list.selected = 0;
        state.settings.list.show();
        state.settings.list.show();

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert!(matches!(
            action,
            Some(SettingsAction::SaveSettings {
                sound_enabled: true,
                ..
            })
        ));
        assert_eq!(state.settings.pending_sound_enabled, Some(true));
        assert!(!state.sound.enabled);
        assert_eq!(state.mode, Mode::Settings);
    }

    #[test]
    fn settings_layout_cycles_sidebar_widths() {
        let mut state = state_with_workspaces(&["test"]);
        state.default_sidebar_width = 26;
        state.sidebar_min_width = 18;
        state.sidebar_max_width = 36;
        open_settings_at(&mut state, SettingsSection::Layout);
        state.settings.list.show();

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        assert_eq!(state.settings.pending_sidebar_width, Some(28));

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        assert_eq!(state.settings.pending_sidebar_min_width, Some(20));

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        assert_eq!(state.settings.pending_sidebar_max_width, Some(38));

        let action = current_settings_action(&state);

        assert!(matches!(
            action,
            SettingsAction::SaveSettings {
                sidebar_width: 28,
                sidebar_min_width: 20,
                sidebar_max_width: 38,
                ..
            }
        ));
    }
    #[test]
    fn settings_layout_cycles_context_bar_visibility() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Layout);
        state.settings.list.show();
        state.settings.list.select(4);

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert_eq!(
            state.settings.pending_context_bar_visibility,
            Some(ContextBarVisibilityConfig::Never)
        );
        assert!(matches!(
            action,
            Some(SettingsAction::SaveSettings {
                context_bar_visibility: ContextBarVisibilityConfig::Never,
                ..
            })
        ));
    }

    #[test]
    fn settings_layout_changes_future_client_sidebar_defaults() {
        let mut state = state_with_workspaces(&["test"]);
        state.sidebar_collapsed = false;
        state.agent_panel_scope = crate::app::state::AgentPanelScope::AllWorkspaces;
        open_settings_at(&mut state, SettingsSection::Layout);
        state.settings.list.show();

        state.settings.list.select(5);
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        state.settings.list.select(6);
        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );

        assert!(matches!(
            action,
            Some(SettingsAction::SaveSettings {
                sidebar_initial_state: SidebarInitialStateConfig::Collapsed,
                sidebar_initial_agent_scope: AgentPanelScopeConfig::Group,
                ..
            })
        ));
        assert!(!state.sidebar_collapsed);
        assert_eq!(
            state.agent_panel_scope,
            crate::app::state::AgentPanelScope::AllWorkspaces
        );
    }

    #[test]
    fn appearance_settings_cycle_pane_border_agent_info_levels() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Theme);
        state.settings.list.select(theme_choice_len(&state) + 11);

        for expected in [
            PaneBorderAgentInfoConfig::Name,
            PaneBorderAgentInfoConfig::NameAndStatus,
            PaneBorderAgentInfoConfig::Hidden,
        ] {
            let action = update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
            );
            assert_eq!(
                state.settings.pending_pane_border_agent_info,
                Some(expected)
            );
            assert!(matches!(
                action,
                Some(SettingsAction::SaveSettings {
                    pane_border_agent_info,
                    ..
                }) if pane_border_agent_info == expected
            ));
        }
    }

    #[test]
    fn appearance_settings_cycle_status_indicator_styles() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Theme);
        state.settings.list.select(theme_choice_len(&state) + 12);

        for expected in [StatusIndicatorStyle::Symbols, StatusIndicatorStyle::Dots] {
            let action = update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
            );
            assert_eq!(state.settings.pending_status_indicators, Some(expected));
            assert!(matches!(
                action,
                Some(SettingsAction::SaveSettings {
                    status_indicators,
                    ..
                }) if status_indicators == expected
            ));
        }
    }

    #[test]
    fn appearance_settings_toggle_pane_controls_without_changing_neighbors() {
        let mut state = state_with_workspaces(&["test"]);
        state.pane_borders = true;
        state.pane_scrollbars = true;
        state.pane_gaps = true;
        state.hide_tab_bar_when_single_tab = false;
        open_settings_at(&mut state, SettingsSection::Theme);
        let theme_count = theme_choice_len(&state);
        state.settings.list.select(theme_count + 7);
        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        assert_eq!(state.settings.pending_pane_borders, Some(false));
        assert_eq!(state.settings.pending_pane_scrollbars, Some(true));
        assert_eq!(state.settings.pending_pane_gaps, Some(true));
        assert_eq!(
            state.settings.pending_hide_tab_bar_when_single_tab,
            Some(false)
        );
        assert!(matches!(
            action,
            Some(SettingsAction::SaveSettings {
                pane_borders: false,
                pane_scrollbars: true,
                pane_gaps: true,
                hide_tab_bar_when_single_tab: false,
                ..
            })
        ));
    }

    #[test]
    fn settings_behavior_toggles_close_prompt_and_terminal_options() {
        let mut state = state_with_workspaces(&["test"]);
        state.confirm_close = true;
        state.prompt_new_tab_name = true;
        state.pane_border_agent_info = PaneBorderAgentInfoConfig::Hidden;
        state.new_terminal_cwd = NewTerminalCwdConfig::Follow;
        state.mouse_scroll_lines = 3;
        open_settings_at(&mut state, SettingsSection::PaneLabels);
        state.settings.list.show();

        assert!(matches!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
            ),
            Some(SettingsAction::SaveSettings {
                confirm_close: false,
                ..
            })
        ));
        assert_eq!(state.settings.pending_confirm_close, Some(false));

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        assert_eq!(state.settings.pending_prompt_new_tab_name, Some(false));

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        assert_eq!(state.settings.pending_prompt_new_workspace_name, Some(true));

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        assert_eq!(state.settings.pending_show_counters, Some(true));

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        assert_eq!(state.settings.pending_copy_on_select, Some(false));

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        assert_eq!(
            state.settings.pending_right_click_passthrough_modifier,
            Some(RightClickPassthroughModifierConfig::default().next())
        );

        state
            .settings
            .list
            .select(BehaviorRowId::NewTerminalCwd.selection_index());
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        assert_eq!(
            state.settings.pending_new_terminal_cwd,
            Some(NewTerminalCwdConfig::Home)
        );
        state
            .settings
            .list
            .select(BehaviorRowId::MouseWheelSpeed.selection_index());
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        assert_eq!(state.settings.pending_mouse_scroll_lines, Some(5));

        let action = current_settings_action(&state);

        assert!(matches!(
            action,
            SettingsAction::SaveSettings {
                confirm_close: false,
                prompt_new_tab_name: false,
                prompt_new_workspace_name: true,
                show_counters: true,
                copy_on_select: false,
                new_terminal_cwd: NewTerminalCwdConfig::Home,
                mouse_scroll_lines: 5,
                pane_border_agent_info: PaneBorderAgentInfoConfig::Hidden,
                ..
            }
        ));
        assert_eq!(state.mode, Mode::Settings);
    }

    #[test]
    fn commands_settings_edits_and_saves_diff_command() {
        let mut state = state_with_workspaces(&["test"]);
        state.review_command = "git diff".to_string();
        open_settings_at(&mut state, SettingsSection::Commands);
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        assert_eq!(state.settings.focused_input, Some(0));
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        );
        for ch in "terminal-browser".chars() {
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty()),
            );
        }
        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );
        assert!(matches!(
            action,
            Some(SettingsAction::SaveSettings {
                browser_command,
                ..
            }) if browser_command == "terminal-browser"
        ));
    }

    #[test]
    fn commands_settings_edits_ide_command_independently() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Commands);
        state.settings.list.select(2);
        state.settings.focused_input = Some(2);

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        );
        for ch in "hx .".chars() {
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty()),
            );
        }
        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert!(matches!(
            action,
            Some(SettingsAction::SaveSettings {
                browser_command,
                review_command,
                editor_command,
                ..
            }) if browser_command == "terminal-browser"
                && review_command == "hunk diff --watch"
                && editor_command == "hx ."
        ));
    }

    #[test]
    fn commands_settings_reset_one_preserves_other_drafts() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Commands);
        state.settings.pending_browser_command = Some("tig".into());
        state.settings.pending_review_command = Some(String::new());
        state.settings.pending_editor_command = Some("hx .".into());
        state.settings.list.select(
            CommandRowId::Action(CommandAction::Reset(CommandField::Editor)).selection_index(),
        );

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(
            state.settings.pending_browser_command.as_deref(),
            Some("tig")
        );
        assert_eq!(state.settings.pending_review_command.as_deref(), Some(""));
        assert_eq!(
            state.settings.pending_editor_command.as_deref(),
            Some("fresh .")
        );
        assert!(matches!(
            action,
            Some(SettingsAction::SaveSettings {
                browser_command,
                review_command,
                editor_command,
                ..
            }) if browser_command == "tig"
                && review_command.is_empty()
                && editor_command == "fresh ."
        ));
    }

    #[test]
    fn commands_settings_reset_all_restores_built_ins() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Commands);
        state.settings.pending_browser_command = Some("tig".into());
        state.settings.pending_review_command = Some(String::new());
        state.settings.pending_editor_command = Some("hx .".into());
        state
            .settings
            .list
            .select(CommandRowId::Action(CommandAction::ResetAll).selection_index());

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(
            state.settings.pending_browser_command.as_deref(),
            Some("terminal-browser")
        );
        assert_eq!(
            state.settings.pending_review_command.as_deref(),
            Some("hunk diff --watch")
        );
        assert_eq!(
            state.settings.pending_editor_command.as_deref(),
            Some("fresh .")
        );
        assert!(matches!(
            action,
            Some(SettingsAction::SaveSettings {
                browser_command,
                review_command,
                editor_command,
                ..
            }) if browser_command == "terminal-browser"
                && review_command == "hunk diff --watch"
                && editor_command == "fresh ."
        ));
    }

    #[test]
    fn commands_settings_mouse_click_resets_command() {
        let mut app = app_for_mouse_test();
        app.state.view.terminal_area = Rect::new(26, 0, 100, 30);
        open_settings_at(&mut app.state, SettingsSection::Commands);
        app.state.settings.pending_editor_command = Some("hx .".into());

        let list_area = settings_section_list_rect(&app.state, SettingsSection::Commands);
        let rows = rows_for_section(&app.state, SettingsSection::Commands).unwrap();
        let reset_ide =
            CommandRowId::Action(CommandAction::Reset(CommandField::Editor)).selection_index();
        let reset_row = selected_visual_row(&rows, reset_ide).unwrap() as u16;
        let action = app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            list_area.x + 2,
            list_area.y + reset_row,
        ));

        assert!(matches!(
            action,
            Some(SettingsAction::SaveSettings { editor_command, .. })
                if editor_command == "fresh ."
        ));
    }

    #[test]
    fn settings_experiments_toggles_input_source() {
        let mut state = state_with_workspaces(&["test"]);
        state.switch_ascii_input_source_in_prefix = false;
        open_settings_at(&mut state, SettingsSection::Experiments);
        state.settings.list.show();

        let input_source_action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(
            input_source_action,
            Some(SettingsAction::SaveSwitchAsciiInputSourceInPrefix(true))
        );
        assert_eq!(state.mode, Mode::Settings);
    }

    #[test]
    fn settings_experiments_toggles_kitty_graphics() {
        let mut state = state_with_workspaces(&["test"]);
        state.kitty_graphics_enabled = false;
        open_settings_at(&mut state, SettingsSection::Experiments);
        state.settings.list.show();
        state
            .settings
            .list
            .select(AdvancedRowId::KittyGraphics.selection_index());

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(action, Some(SettingsAction::SaveKittyGraphics(true)));
    }
    #[test]
    fn settings_restores_resume_title_and_headless_controls() {
        let mut state = state_with_workspaces(&["test"]);
        state.resume_agents_on_restore = false;
        open_settings_at(&mut state, SettingsSection::PaneLabels);
        state.settings.list.show();
        state
            .settings
            .list
            .select(BehaviorRowId::ResumeAgents.selection_index());
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            ),
            Some(SettingsAction::SaveResumeAgentsOnRestore(true))
        );

        switch_settings_section(&mut state, SettingsSection::Theme, 0);
        state
            .settings
            .list
            .select(theme_choice_len(&state).saturating_add(13));
        focus_selected_settings_input(&mut state);
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
            ),
            Some(SettingsAction::SaveWindowTitle(String::new()))
        );
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char('{'), KeyModifiers::empty()),
            ),
            None
        );
        assert_eq!(state.settings.pending_window_title.as_deref(), Some("{"));
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
            ),
            Some(SettingsAction::SaveWindowTitle(String::new()))
        );
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty()),
            ),
            Some(SettingsAction::SaveWindowTitle("x".to_string()))
        );

        switch_settings_section(
            &mut state,
            SettingsSection::Experiments,
            AdvancedRowId::HeadlessCols.selection_index(),
        );
        focus_selected_settings_input(&mut state);
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
            ),
            None
        );
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char('0'), KeyModifiers::empty()),
            ),
            None
        );
        assert_eq!(state.settings.pending_headless_cols.as_deref(), Some("0"));
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty()),
        );
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char('8'), KeyModifiers::empty()),
            ),
            Some(SettingsAction::SaveHeadlessSize { cols: 8, rows: 40 })
        );
    }

    #[test]
    fn settings_tab_moves_focus_between_content_and_sidebar() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::PaneLabels);

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()),
        );
        assert!(state.settings.sidebar_focused);
        assert_eq!(state.settings.section, SettingsSection::PaneLabels);

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        assert_eq!(state.settings.sidebar_selection.subsection, Some(0));
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        assert_eq!(state.settings.sidebar_selection.subsection, Some(1));
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        assert_eq!(state.settings.sidebar_selection.subsection, Some(2));
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        assert_eq!(state.settings.sidebar_selection.subsection, Some(3));
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        assert_eq!(
            state.settings.sidebar_selection,
            SettingsSidebarSelection::section(SettingsSection::Commands)
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );
        assert_eq!(state.settings.section, SettingsSection::Commands);
        assert_eq!(
            state.settings.sidebar_expanded,
            Some(SettingsSection::Commands)
        );

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()),
        );
        assert!(!state.settings.sidebar_focused);
    }

    #[test]
    fn settings_sidebar_subsection_jumps_to_anchored_content() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::PaneLabels);
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()),
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Right, KeyModifiers::empty()),
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(state.settings.section, SettingsSection::PaneLabels);
        assert_eq!(state.settings.sidebar_selection.subsection, Some(1));
        assert!(state.settings.scroll > 0);
        assert_eq!(state.settings.list.selected, 4);
    }

    #[test]
    fn integrations_enter_does_nothing_when_nothing_needs_install() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Integrations);

        let enter_action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );
        assert_eq!(enter_action, None);

        let space_action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        assert_eq!(space_action, None);
    }

    #[test]
    fn integrations_arrows_select_rows() {
        let mut state = state_with_workspaces(&["test"]);
        state.integration_recommendations = vec![
            integration_recommendation_for(
                crate::api::schema::IntegrationTarget::Pi,
                crate::integration::IntegrationStatusKind::Current,
                true,
            ),
            integration_recommendation_for(
                crate::api::schema::IntegrationTarget::Omp,
                crate::integration::IntegrationStatusKind::NotInstalled,
                true,
            ),
        ];
        open_settings_at(&mut state, SettingsSection::Integrations);

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        assert_eq!(state.settings.list.selected, 0);

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        assert_eq!(state.settings.list.selected, 1);

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Up, KeyModifiers::empty()),
        );
        assert_eq!(state.settings.list.selected, 0);
    }

    #[test]
    fn settings_arrows_wrap_top_and_bottom_in_each_section() {
        fn assert_open_section_wraps(state: &mut AppState, section: SettingsSection) {
            let first_selection = state.settings.list.selected;
            let last_selection = settings_section_choice_len(state, section) - 1;

            update_settings_state(state, KeyEvent::new(KeyCode::Up, KeyModifiers::empty()));
            assert_eq!(state.settings.list.selected, last_selection);

            update_settings_state(state, KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
            assert_eq!(state.settings.list.selected, first_selection);
        }

        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Theme);
        state.settings.pending_theme_mode = Some(ThemeMode::Light);
        state.settings.pending_light_theme_name =
            Some(crate::app::state::DEFAULT_LIGHT_THEME_NAME.to_string());
        state.settings.pending_dark_theme_name =
            Some(crate::app::state::DEFAULT_DARK_THEME_NAME.to_string());
        state.settings.list.selected = 0;
        state.settings.list.show();
        assert_open_section_wraps(&mut state, SettingsSection::Theme);

        open_settings_at(&mut state, SettingsSection::Layout);
        state.settings.list.selected = 0;
        state.settings.list.show();
        assert_open_section_wraps(&mut state, SettingsSection::Layout);

        open_settings_at(&mut state, SettingsSection::Sound);
        state.settings.list.selected = 0;
        state.settings.list.show();
        assert_open_section_wraps(&mut state, SettingsSection::Sound);

        open_settings_at(&mut state, SettingsSection::Toast);
        state.settings.list.selected = 0;
        state.settings.list.show();
        assert_open_section_wraps(&mut state, SettingsSection::Toast);

        open_settings_at(&mut state, SettingsSection::PaneLabels);
        state.settings.list.selected = 0;
        state.settings.list.show();
        assert_open_section_wraps(&mut state, SettingsSection::PaneLabels);

        state.integration_recommendations = vec![
            integration_recommendation_for(
                crate::api::schema::IntegrationTarget::Pi,
                crate::integration::IntegrationStatusKind::Current,
                true,
            ),
            integration_recommendation_for(
                crate::api::schema::IntegrationTarget::Omp,
                crate::integration::IntegrationStatusKind::NotInstalled,
                true,
            ),
        ];
        open_settings_at(&mut state, SettingsSection::Integrations);
        state.settings.list.selected = 0;
        state.settings.list.show();
        assert_open_section_wraps(&mut state, SettingsSection::Integrations);
    }

    #[test]
    fn settings_tabs_wrap_between_theme_and_about() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Theme);

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Left, KeyModifiers::empty()),
        );
        assert_eq!(state.settings.section, SettingsSection::About);

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Right, KeyModifiers::empty()),
        );
        assert_eq!(state.settings.section, SettingsSection::Theme);
    }

    #[test]
    fn about_settings_ignores_selection_input() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::About);
        state.settings.list.show();

        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            ),
            None
        );
        assert_eq!(state.settings.section, SettingsSection::About);
        assert_eq!(state.settings.list.selected, 0);
    }

    #[test]
    fn integrations_enter_installs_selected_available_row() {
        let mut state = state_with_workspaces(&["test"]);
        state.integration_recommendations = vec![integration_recommendation_for(
            crate::api::schema::IntegrationTarget::Omp,
            crate::integration::IntegrationStatusKind::NotInstalled,
            true,
        )];
        open_settings_at(&mut state, SettingsSection::Integrations);
        state.settings.list.show();

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(
            action,
            Some(SettingsAction::InstallIntegration(
                crate::api::schema::IntegrationTarget::Omp
            ))
        );
    }

    #[test]
    fn integrations_stale_host_selection_falls_back_to_local_actions() {
        let mut state = state_with_workspaces(&["test"]);
        state.integration_recommendations = vec![integration_recommendation_for(
            crate::api::schema::IntegrationTarget::Omp,
            crate::integration::IntegrationStatusKind::NotInstalled,
            true,
        )];
        open_settings_at(&mut state, SettingsSection::Integrations);
        state.settings.integration_host_profile_id = Some("deleted-host".to_string());
        state.settings.list.show();

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(
            action,
            Some(SettingsAction::InstallIntegration(
                crate::api::schema::IntegrationTarget::Omp
            ))
        );
    }

    #[test]
    fn integrations_enter_installs_selected_missing_profile_hooks_row() {
        let _lock = crate::integration::integration_env_lock();
        let base = std::env::temp_dir().join(format!(
            "gardn-settings-enter-codex-profile-hook-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = base.join("home");
        let default_codex_dir = home.join(".codex");
        let custom_codex_dir = home.join(".codex-frs");
        std::fs::create_dir_all(&default_codex_dir).unwrap();
        std::fs::create_dir_all(&custom_codex_dir).unwrap();
        std::fs::write(
            default_codex_dir.join("config.toml"),
            "model = \"gpt-5.4\"\n",
        )
        .unwrap();
        let _codex_home_env = crate::config::TestEnvVar::remove("CODEX_HOME");
        let _home_env = crate::config::TestEnvVar::set("HOME", &home);

        crate::integration::install_target(crate::api::schema::IntegrationTarget::Codex)
            .expect("install default codex integration");
        assert!(default_codex_dir.join("gardn-agent-state.sh").is_file());
        assert!(!custom_codex_dir.join("gardn-agent-state.sh").exists());

        let mut state = state_with_workspaces(&["test"]);
        state.agent_profiles = crate::agent_profiles::AgentProfileCatalog::from_config(
            &crate::agent_profiles::AgentProfilesConfig {
                order: vec!["user:codex-frs".to_string()],
                custom: vec![crate::agent_profiles::UserAgentProfileConfig {
                    id: "codex-frs".to_string(),
                    name: "codex frs".to_string(),
                    kind: crate::agent_profiles::AgentKind::Codex,
                    command: "codex-frs".to_string(),
                    env: std::collections::BTreeMap::new(),
                    enabled: true,
                }],
            },
        );
        state.integration_recommendations = vec![integration_recommendation_for(
            crate::api::schema::IntegrationTarget::Codex,
            crate::integration::IntegrationStatusKind::Current,
            true,
        )];
        open_settings_at(&mut state, SettingsSection::Integrations);
        state.settings.list.show();

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(
            action,
            Some(SettingsAction::InstallIntegration(
                crate::api::schema::IntegrationTarget::Codex
            ))
        );
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn integrations_enter_uninstalls_selected_current_row() {
        let mut state = state_with_workspaces(&["test"]);
        state.integration_recommendations = vec![integration_recommendation_for(
            crate::api::schema::IntegrationTarget::Omp,
            crate::integration::IntegrationStatusKind::Current,
            true,
        )];
        open_settings_at(&mut state, SettingsSection::Integrations);
        state.settings.list.show();

        let action = update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );

        assert_eq!(
            action,
            Some(SettingsAction::UninstallIntegration(
                crate::api::schema::IntegrationTarget::Omp
            ))
        );
    }

    #[test]
    fn integrations_mouse_click_selects_row() {
        let mut app = app_for_mouse_test();
        app.state.ssh_connection_profiles.clear();
        app.state.integration_recommendations = vec![
            integration_recommendation_for(
                crate::api::schema::IntegrationTarget::Pi,
                crate::integration::IntegrationStatusKind::Current,
                true,
            ),
            integration_recommendation_for(
                crate::api::schema::IntegrationTarget::Omp,
                crate::integration::IntegrationStatusKind::NotInstalled,
                true,
            ),
        ];
        open_settings_at(&mut app.state, SettingsSection::Integrations);
        let list_area = settings_section_list_rect(&app.state, SettingsSection::Integrations);

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            list_area.x + 2,
            list_area.y + 1,
        ));

        assert_eq!(app.state.settings.list.selected, 1);
    }

    #[test]
    fn integrations_mouse_click_installs_available_not_installed_row() {
        let mut app = app_for_mouse_test();
        app.state.ssh_connection_profiles.clear();
        app.state.integration_recommendations = vec![
            integration_recommendation_for(
                crate::api::schema::IntegrationTarget::Pi,
                crate::integration::IntegrationStatusKind::Current,
                true,
            ),
            integration_recommendation_for(
                crate::api::schema::IntegrationTarget::Omp,
                crate::integration::IntegrationStatusKind::NotInstalled,
                true,
            ),
        ];
        open_settings_at(&mut app.state, SettingsSection::Integrations);

        let list_area = settings_section_list_rect(&app.state, SettingsSection::Integrations);
        let rows = rows_for_section(&app.state, SettingsSection::Integrations).unwrap();
        let row_for = |index| selected_visual_row(&rows, index).unwrap() as u16;
        let action = app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            list_area.x + 2,
            list_area.y + row_for(1),
        ));

        assert_eq!(
            action,
            Some(SettingsAction::InstallIntegration(
                crate::api::schema::IntegrationTarget::Omp
            ))
        );
        assert_eq!(app.state.settings.list.selected, 1);
    }

    #[test]
    fn integrations_mouse_click_uninstalls_current_row() {
        let mut app = app_for_mouse_test();
        app.state.ssh_connection_profiles.clear();
        app.state.integration_recommendations = vec![
            integration_recommendation_for(
                crate::api::schema::IntegrationTarget::Pi,
                crate::integration::IntegrationStatusKind::NotInstalled,
                false,
            ),
            integration_recommendation_for(
                crate::api::schema::IntegrationTarget::Omp,
                crate::integration::IntegrationStatusKind::Current,
                true,
            ),
        ];
        open_settings_at(&mut app.state, SettingsSection::Integrations);

        let list_area = settings_section_list_rect(&app.state, SettingsSection::Integrations);
        let rows = rows_for_section(&app.state, SettingsSection::Integrations).unwrap();
        let row_for = |index| selected_visual_row(&rows, index).unwrap() as u16;
        let action = app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            list_area.x + 2,
            list_area.y + row_for(1),
        ));

        assert_eq!(
            action,
            Some(SettingsAction::UninstallIntegration(
                crate::api::schema::IntegrationTarget::Omp
            ))
        );
        assert_eq!(app.state.settings.list.selected, 1);
    }

    #[test]
    fn integrations_mouse_click_unavailable_not_installed_row_only_selects() {
        let mut app = app_for_mouse_test();
        app.state.ssh_connection_profiles.clear();
        app.state.integration_recommendations = vec![
            integration_recommendation_for(
                crate::api::schema::IntegrationTarget::Omp,
                crate::integration::IntegrationStatusKind::NotInstalled,
                true,
            ),
            integration_recommendation_for(
                crate::api::schema::IntegrationTarget::Pi,
                crate::integration::IntegrationStatusKind::NotInstalled,
                false,
            ),
        ];
        open_settings_at(&mut app.state, SettingsSection::Integrations);

        let list_area = settings_section_list_rect(&app.state, SettingsSection::Integrations);
        let rows = rows_for_section(&app.state, SettingsSection::Integrations).unwrap();
        let row_for = |index| selected_visual_row(&rows, index).unwrap() as u16;
        let action = app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            list_area.x + 2,
            list_area.y + row_for(1),
        ));

        assert_eq!(action, None);
        assert_eq!(app.state.settings.list.selected, 1);
    }

    #[test]
    fn settings_hover_highlights_without_moving_keyboard_selection() {
        let mut app = app_for_mouse_test();
        open_settings(&mut app.state);
        app.state.settings.list.select(0);
        app.state.settings.pending_theme_mode = Some(ThemeMode::System);

        let list_area = settings_section_list_rect(&app.state, SettingsSection::Theme);
        app.handle_mouse(mouse(
            MouseEventKind::Moved,
            list_area.x + 2,
            list_area.y + 2,
        ));

        assert_eq!(app.state.settings.list.selected, 0);
        assert_eq!(app.state.settings.list.visible(), Some(1));
        assert_eq!(
            app.state.settings.pending_theme_mode,
            Some(ThemeMode::System)
        );
    }

    #[test]
    fn settings_theme_hover_ignores_scrollbar_column() {
        let mut app = app_for_mouse_test();
        open_settings(&mut app.state);
        app.state.global_theme_mode = ThemeMode::System;
        app.state.settings.pending_theme_mode = Some(ThemeMode::System);
        app.state.settings.list.select(0);
        let list_area = settings_section_list_rect(&app.state, SettingsSection::Theme);
        let track = settings_theme_viewport(&app.state)
            .scroll_area(list_area)
            .track
            .expect("scrollbar track");

        app.handle_mouse(mouse(MouseEventKind::Moved, track.x, track.y + 1));

        assert_eq!(app.state.settings.list.selected, 0);
        assert_eq!(app.state.settings.list.visible(), None);
    }

    #[test]
    fn settings_theme_wheel_scrolls_without_changing_selection() {
        let mut app = app_for_mouse_test();
        open_settings(&mut app.state);
        app.state.settings.list.select(0);

        let area = app.state.settings_content_rect();
        let expected_scroll = settings_theme_max_scroll(&app.state)
            .min(super::super::MODAL_WHEEL_SCROLL_ROWS as usize);
        app.handle_mouse(mouse(MouseEventKind::ScrollDown, area.x + 2, area.y + 2));

        assert_eq!(app.state.settings.list.selected, 0);
        assert_eq!(app.state.settings.scroll, expected_scroll);

        app.handle_mouse(mouse(MouseEventKind::ScrollUp, area.x + 2, area.y + 2));

        assert_eq!(app.state.settings.list.selected, 0);
        assert_eq!(app.state.settings.scroll, 0);
    }

    #[test]
    fn settings_theme_system_scrollbar_drag_reveals_dark_options() {
        let mut app = app_for_mouse_test();
        app.state.global_theme_mode = ThemeMode::System;
        open_settings(&mut app.state);

        let list_area = settings_section_list_rect(&app.state, SettingsSection::Theme);
        let track = settings_theme_viewport(&app.state)
            .scroll_area(list_area)
            .track
            .expect("scrollbar track");
        assert_eq!(app.state.settings.scroll, 0);

        app.handle_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            track.x,
            track.y,
        ));
        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            track.x,
            track.y + track.height.saturating_sub(1),
        ));

        assert!(
            app.state.settings.scroll > theme_names_for_appearance(ThemeAppearance::Light).len()
        );
    }

    #[test]
    fn settings_mouse_click_toggles_behavior_options() {
        let mut app = app_for_mouse_test();
        app.state.confirm_close = true;
        app.state.prompt_new_tab_name = true;
        app.state.pane_border_agent_info = PaneBorderAgentInfoConfig::Hidden;
        app.state.new_terminal_cwd = NewTerminalCwdConfig::Follow;
        app.state.mouse_scroll_lines = 3;
        app.state.view.terminal_area = Rect::new(26, 0, 100, 30);
        open_settings_at(&mut app.state, SettingsSection::PaneLabels);

        let list_area = settings_section_list_rect(&app.state, SettingsSection::PaneLabels);
        let rows = rows_for_section(&app.state, SettingsSection::PaneLabels).unwrap();
        let row_for = |index| selected_visual_row(&rows, index).unwrap() as u16;
        assert!(matches!(
            app.state.handle_settings_mouse(mouse(
                MouseEventKind::Down(crossterm::event::MouseButton::Left),
                list_area.x + 2,
                list_area.y + row_for(0),
            )),
            Some(SettingsAction::SaveSettings {
                confirm_close: false,
                ..
            })
        ));
        assert_eq!(app.state.settings.pending_confirm_close, Some(false));
        assert_eq!(app.state.settings.list.selected, 0);

        app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            list_area.x + 2,
            list_area.y + row_for(1),
        ));
        assert_eq!(app.state.settings.pending_prompt_new_tab_name, Some(false));
        assert_eq!(app.state.settings.list.selected, 1);

        app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            list_area.x + 2,
            list_area.y + row_for(2),
        ));
        assert_eq!(
            app.state.settings.pending_prompt_new_workspace_name,
            Some(true)
        );
        assert_eq!(app.state.settings.list.selected, 2);

        app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            list_area.x + 2,
            list_area.y + row_for(3),
        ));
        assert_eq!(app.state.settings.pending_show_counters, Some(true));
        assert_eq!(app.state.settings.list.selected, 3);

        let cwd_index = BehaviorRowId::NewTerminalCwd.selection_index();
        let cwd_scroll = row_for(cwd_index).saturating_sub(list_area.height.saturating_sub(1));
        app.state.settings.scroll = cwd_scroll as usize;
        app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            list_area.x + 2,
            list_area.y + row_for(cwd_index) - cwd_scroll,
        ));
        assert_eq!(
            app.state.settings.pending_new_terminal_cwd,
            Some(NewTerminalCwdConfig::Home)
        );
        assert_eq!(app.state.settings.list.selected, cwd_index);
        let mouse_index = BehaviorRowId::MouseWheelSpeed.selection_index();
        let scroll = row_for(mouse_index).saturating_sub(list_area.height.saturating_sub(1));
        app.state.settings.scroll = scroll as usize;
        app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            list_area.x + 2,
            list_area.y + row_for(mouse_index) - scroll,
        ));
        assert_eq!(app.state.settings.pending_mouse_scroll_lines, Some(5));
        assert_eq!(app.state.settings.list.selected, mouse_index);
    }

    #[test]
    fn settings_mouse_ignores_section_headers_and_separators() {
        let mut app = app_for_mouse_test();
        open_settings_at(&mut app.state, SettingsSection::PaneLabels);

        let list_area = settings_section_list_rect(&app.state, SettingsSection::PaneLabels);
        app.state.settings.list.selected = 4;
        app.state.settings.list.show();
        assert_eq!(
            app.state.handle_settings_mouse(mouse(
                MouseEventKind::Moved,
                list_area.x + 2,
                list_area.y
            )),
            None
        );
        assert_eq!(app.state.settings.list.selected, 4);
        app.state.handle_settings_mouse(mouse(
            MouseEventKind::Moved,
            list_area.x + 2,
            list_area.y + 7,
        ));
        assert_eq!(app.state.settings.list.selected, 4);

        app.state.handle_settings_mouse(mouse(
            MouseEventKind::Moved,
            list_area.x + 2,
            list_area.y + 1,
        ));
        assert_eq!(app.state.settings.list.selected, 4);
        assert_eq!(app.state.settings.list.visible(), Some(0));
    }

    #[test]
    fn settings_mouse_click_toggles_experiment_rows() {
        let mut app = app_for_mouse_test();
        app.state.switch_ascii_input_source_in_prefix = false;
        app.state.view.sidebar_rect = Rect::new(0, 0, 26, 30);
        app.state.view.terminal_area = Rect::new(26, 0, 80, 30);
        open_settings_at(&mut app.state, SettingsSection::Experiments);

        let list_area = settings_section_list_rect(&app.state, SettingsSection::Experiments);
        let input_source_action = app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            list_area.x + 2,
            list_area.y + 1,
        ));

        assert_eq!(
            input_source_action,
            Some(SettingsAction::SaveSwitchAsciiInputSourceInPrefix(true))
        );
        assert_eq!(app.state.settings.list.selected, 0);
    }

    #[test]
    fn settings_mouse_focuses_text_inputs_without_saving_stale_drafts() {
        let mut app = app_for_mouse_test();
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 150, 40));
        open_settings_at(&mut app.state, SettingsSection::Theme);

        let title_index = theme_choice_len(&app.state) + 13;
        let title_rows = theme_rows(&app.state);
        let title_visual_row =
            selected_visual_row(&title_rows, title_index).expect("window title row");
        app.state.settings.list.select(title_index);
        ensure_settings_selection_visible(&mut app.state);
        let title_list = settings_section_list_geometry(&app.state, SettingsSection::Theme);
        let title_action = app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            title_list.rect.x + 2,
            title_list.rect.y + (title_visual_row - app.state.settings.scroll) as u16,
        ));

        assert_eq!(title_action, None);
        assert_eq!(app.state.settings.focused_input, Some(title_index));

        switch_settings_section(
            &mut app.state,
            SettingsSection::Experiments,
            AdvancedRowId::HeadlessCols.selection_index(),
        );
        app.state.settings.pending_headless_cols = Some("80".to_string());
        app.state.settings.pending_headless_rows = Some("24".to_string());
        let headless_rows =
            rows_for_section(&app.state, SettingsSection::Experiments).expect("advanced rows");
        let cols_visual_row = selected_visual_row(
            &headless_rows,
            AdvancedRowId::HeadlessCols.selection_index(),
        )
        .expect("headless columns row");
        let headless_list =
            settings_section_list_geometry(&app.state, SettingsSection::Experiments);
        let headless_action = app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            headless_list.rect.x + 2,
            headless_list.rect.y + cols_visual_row as u16,
        ));

        assert_eq!(headless_action, None);
        assert_eq!(
            app.state.settings.focused_input,
            Some(AdvancedRowId::HeadlessCols.selection_index())
        );
    }

    #[test]
    fn integration_update_badge_only_tracks_outdated_recommendations() {
        let mut state = state_with_workspaces(&["test"]);
        state.integration_recommendations = vec![integration_recommendation(
            crate::integration::IntegrationStatusKind::NotInstalled,
            true,
        )];
        assert!(!state.integration_updates_available());

        state.integration_recommendations = vec![integration_recommendation(
            crate::integration::IntegrationStatusKind::NotInstalled,
            false,
        )];
        assert!(!state.integration_updates_available());

        state.integration_recommendations = vec![integration_recommendation(
            crate::integration::IntegrationStatusKind::Current,
            true,
        )];
        assert!(!state.integration_updates_available());

        state.integration_recommendations = vec![integration_recommendation(
            crate::integration::IntegrationStatusKind::Outdated,
            true,
        )];
        assert!(state.integration_updates_available());
    }

    #[test]
    fn settings_sidebar_hit_area_is_stable_with_integration_update_badge() {
        let mut app = app_for_mouse_test();
        app.state.view.terminal_area = Rect::new(0, 0, 100, 30);
        app.state.integration_recommendations = vec![integration_recommendation(
            crate::integration::IntegrationStatusKind::Outdated,
            true,
        )];
        open_settings(&mut app.state);

        let inner = app.state.settings_inner_rect();
        let content = crate::ui::settings_stack_areas(&app.state, inner).content;
        let navigation = crate::ui::settings_sidebar_areas(content).navigation;
        let (_, integrations_rect) =
            crate::ui::settings_sidebar_hit_areas(&app.state.settings, navigation)
                .into_iter()
                .find(|(entry, _)| {
                    entry.section == SettingsSection::Integrations && entry.subsection.is_none()
                })
                .expect("integrations sidebar entry");

        app.handle_mouse(mouse(
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            integrations_rect.x + 1,
            integrations_rect.y,
        ));

        assert_eq!(app.state.settings.section, SettingsSection::Integrations);
    }

    #[test]
    fn settings_sidebar_is_clickable_at_mobile_width_boundary() {
        let mut app = app_for_mouse_test();
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 119, 24));
        open_settings_at(&mut app.state, SettingsSection::Theme);

        let inner = app.state.settings_inner_rect();
        let content = crate::ui::settings_stack_areas(&app.state, inner).content;
        let navigation = crate::ui::settings_sidebar_areas(content).navigation;
        let (_, notifications_rect) =
            crate::ui::settings_sidebar_hit_areas(&app.state.settings, navigation)
                .into_iter()
                .find(|(entry, _)| {
                    entry.section == SettingsSection::Sound && entry.subsection.is_none()
                })
                .expect("notifications sidebar entry");

        app.handle_mouse(mouse(
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            notifications_rect.x + 1,
            notifications_rect.y,
        ));

        assert_eq!(app.state.settings.section, SettingsSection::Sound);
        assert_eq!(
            app.state.settings.sidebar_expanded,
            Some(SettingsSection::Sound)
        );
    }

    #[test]
    fn clicking_sidebar_width_recomputes_terminal_layout() {
        let mut app = app_for_mouse_test();
        app.state.sidebar_arrangement = crate::config::SidebarArrangementConfig::CombinedLeft;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 150, 40));
        open_settings_at(&mut app.state, SettingsSection::Theme);
        app.state.settings.pending_theme_mode = Some(ThemeMode::System);
        app.state.settings.pending_light_theme_name = Some("system".into());
        app.state.settings.pending_dark_theme_name = Some("system".into());

        let rows = rows_for_section(&app.state, SettingsSection::Theme).unwrap();
        let width_row = rows
            .iter()
            .enumerate()
            .find_map(|(row, setting)| match setting {
                crate::settings_rows::SettingsListRow::Value { title, .. }
                    if title.as_ref() == "Default Sidebar Width" =>
                {
                    Some(row)
                }
                _ => None,
            })
            .expect("default sidebar width row");
        let list_area = settings_section_list_rect(&app.state, SettingsSection::Theme);
        app.state.settings.scroll = width_row.saturating_sub(3);
        let (width_x, width_y) = rendered_text_point_at_or_after_row(
            &app,
            "Default Sidebar Width",
            list_area.y,
            150,
            40,
        );

        app.handle_mouse(mouse(
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            width_x,
            width_y,
        ));

        assert_eq!(app.state.default_sidebar_width, 28);
        assert_eq!(app.state.sidebar_width, 28);
        assert_eq!(app.state.view.sidebar_rect.width, 28);
    }

    #[test]
    fn appearance_sidebar_arrangement_click_does_not_toggle_pane_labels() {
        let mut app = app_for_mouse_test();
        app.state.sidebar_arrangement = crate::config::SidebarArrangementConfig::Auto;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 150, 40));
        open_settings_at(&mut app.state, SettingsSection::Theme);
        app.state.settings.pending_theme_mode = Some(ThemeMode::System);
        app.state.settings.pending_light_theme_name = Some("system".into());
        app.state.settings.pending_dark_theme_name = Some("system".into());

        let rows = rows_for_section(&app.state, SettingsSection::Theme).unwrap();
        let mut visual_row: usize = 0;
        let mut arrangement_index: Option<usize> = None;
        let mut arrangement_row: Option<usize> = None;
        for row in &rows {
            match row {
                crate::settings_rows::SettingsListRow::Value { index, title, .. }
                    if title.as_ref() == "Sidebar Arrangement" =>
                {
                    arrangement_index = Some(*index);
                    arrangement_row = Some(visual_row);
                    break;
                }
                crate::settings_rows::SettingsListRow::Header(_)
                | crate::settings_rows::SettingsListRow::Caption(_)
                | crate::settings_rows::SettingsListRow::Spacer
                | crate::settings_rows::SettingsListRow::Choice { .. }
                | crate::settings_rows::SettingsListRow::Action { .. }
                | crate::settings_rows::SettingsListRow::Status { .. }
                | crate::settings_rows::SettingsListRow::Profile { .. } => visual_row += 1,
                crate::settings_rows::SettingsListRow::GroupIconPicker => {
                    visual_row += crate::ui::group_icon_picker_row_count() as usize;
                }
                crate::settings_rows::SettingsListRow::Toggle { .. }
                | crate::settings_rows::SettingsListRow::Value { .. }
                | crate::settings_rows::SettingsListRow::TextInput { .. } => visual_row += 2,
            }
        }
        let arrangement_index = arrangement_index.expect("sidebar arrangement index");
        let arrangement_row = arrangement_row.expect("sidebar arrangement row");
        let list_area = settings_section_list_rect(&app.state, SettingsSection::Theme);
        app.state.settings.scroll = arrangement_row.saturating_sub(3);

        let (arrangement_x, arrangement_y) =
            rendered_text_point_at_or_after_row(&app, "Sidebar Arrangement", list_area.y, 150, 40);
        let initial_pane_border_agent_info = app.state.settings.pending_pane_border_agent_info;
        let action = app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            arrangement_x,
            arrangement_y,
        ));
        let action_to_apply = action.clone().expect("settings action");

        assert_eq!(
            app.state.settings.pending_sidebar_arrangement,
            Some(crate::config::SidebarArrangementConfig::Separate)
        );
        assert_eq!(app.state.settings.list.selected, arrangement_index);
        assert_eq!(
            app.state.settings.pending_pane_border_agent_info,
            initial_pane_border_agent_info
        );
        match action {
            Some(SettingsAction::SaveSettings {
                sidebar_arrangement,
                pane_border_agent_info,
                ..
            }) => {
                assert_eq!(
                    sidebar_arrangement,
                    crate::config::SidebarArrangementConfig::Separate
                );
                assert_eq!(
                    pane_border_agent_info,
                    initial_pane_border_agent_info
                        .unwrap_or_else(|| app.state.pane_border_agent_info())
                );
            }
            other => panic!("expected settings save action, got {other:?}"),
        }
        app.apply_settings_action(action_to_apply);
        assert_eq!(
            app.state.sidebar_arrangement,
            crate::config::SidebarArrangementConfig::Separate
        );
    }

    #[test]
    fn settings_mouse_click_hits_rendered_one_line_option_in_odd_height_modal() {
        const WIDTH: u16 = 150;
        const HEIGHT: u16 = 41;

        let mut app = app_for_mouse_test();
        app.state.sound.enabled = true;
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, WIDTH, HEIGHT));
        open_settings_at(&mut app.state, SettingsSection::Sound);

        let (_, heading_y) = rendered_text_point(&app, "Sound Alerts", WIDTH, HEIGHT);
        let (sound_x, sound_y) =
            rendered_text_point_at_or_after_row(&app, "Sound Alerts", heading_y + 1, WIDTH, HEIGHT);
        let action = app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            sound_x,
            sound_y,
        ));

        assert_eq!(
            app.state.settings.list.selected, 0,
            "clicking the rendered sound alerts row at {sound_x},{sound_y} should select the sound toggle"
        );
        assert_eq!(app.state.settings.pending_sound_enabled, Some(false));
        match action {
            Some(SettingsAction::SaveSettings { sound_enabled, .. }) => assert!(
                !sound_enabled,
                "clicking the rendered sound alerts row should disable sound"
            ),
            other => panic!(
                "expected sound save action, got {other:?}; point={sound_x},{sound_y}; content={:?}; screen={:?}",
                app.state.settings_content_rect(),
                app.state.screen_rect()
            ),
        }
    }

    #[test]
    fn settings_mouse_clicks_sidebar_and_one_line_option_rows() {
        let mut app = app_for_mouse_test();
        crate::ui::compute_view(&mut app.state, Rect::new(0, 0, 150, 40));
        open_settings_at(&mut app.state, SettingsSection::Theme);

        let (notifications_x, navigation_y) = rendered_text_point(&app, "Notifications", 150, 40);
        app.handle_mouse(mouse(
            MouseEventKind::Down(crossterm::event::MouseButton::Left),
            notifications_x,
            navigation_y,
        ));
        assert_eq!(app.state.settings.section, SettingsSection::Sound);

        let (_, behavior_y) = rendered_text_point(&app, "Behavior", 150, 40);
        assert_ne!(behavior_y, navigation_y);

        let list_area = settings_section_list_rect(&app.state, SettingsSection::Sound);
        let rows = rows_for_section(&app.state, SettingsSection::Sound).expect("sound rows");
        let sound_row = selected_visual_row(&rows, 0).expect("sound alert row") as u16;
        let (off_x, off_y) = (list_area.x + 2, list_area.y + sound_row);
        let action = app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            off_x,
            off_y,
        ));
        match action {
            Some(SettingsAction::SaveSettings { sound_enabled, .. }) => assert!(
                !sound_enabled,
                "clicking the visible one-line off row should disable sound"
            ),
            other => panic!(
                "expected sound save action, got {other:?}; point={off_x},{off_y}; list_area={list_area:?}"
            ),
        }
    }
    #[test]
    fn settings_sidebar_keyboard_expands_and_collapses_sections() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Theme);
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Tab, KeyModifiers::empty()),
        );

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Right, KeyModifiers::empty()),
        );
        assert_eq!(state.settings.sidebar_selection.subsection, Some(0));
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Left, KeyModifiers::empty()),
        );
        assert_eq!(state.settings.sidebar_selection.subsection, None);
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Left, KeyModifiers::empty()),
        );
        assert_eq!(state.settings.sidebar_expanded, None);

        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        assert_eq!(
            state.settings.sidebar_selection,
            SettingsSidebarSelection::section(SettingsSection::Sound)
        );
        update_settings_state(
            &mut state,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
        );
        assert_eq!(state.settings.section, SettingsSection::Sound);
        assert_eq!(
            state.settings.sidebar_expanded,
            Some(SettingsSection::Sound)
        );
    }

    // ------------------------------------------------------------------
    // Settings → Connections
    // ------------------------------------------------------------------

    fn isolated_ssh_catalog(
        name: &str,
    ) -> (
        std::sync::MutexGuard<'static, ()>,
        crate::config::TestEnvVar,
    ) {
        let lock = match crate::config::test_config_env_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let base = std::env::temp_dir().join(format!(
            "gardn-connections-settings-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let guard = crate::config::TestEnvVar::set("XDG_CONFIG_HOME", &base);
        (lock, guard)
    }

    fn connection_key(state: &mut AppState, code: KeyCode) -> Option<SettingsAction> {
        update_settings_state(state, KeyEvent::new(code, KeyModifiers::empty()))
    }

    fn connection_type(state: &mut AppState, text: &str) {
        for ch in text.chars() {
            let modifiers = if ch.is_uppercase() {
                KeyModifiers::SHIFT
            } else {
                KeyModifiers::empty()
            };
            update_settings_state(state, KeyEvent::new(KeyCode::Char(ch), modifiers));
        }
    }

    fn seed_connection_profile(
        app: &mut App,
        id: &str,
        name: &str,
        target: &str,
        directory: Option<&str>,
    ) -> crate::persist::ssh_profiles::SshConnectionProfile {
        let suggested_directory = directory
            .map(crate::execution_host::HostPath::new)
            .transpose()
            .unwrap();
        let profile = crate::persist::ssh_profiles::SshConnectionProfile::new(
            id,
            name,
            target,
            suggested_directory,
        )
        .unwrap();
        app.state.ssh_connection_profiles =
            crate::persist::ssh_profiles::upsert(profile.clone()).unwrap();
        profile
    }

    #[test]
    fn connection_keyboard_create_save_persists_profile() {
        let (_lock, _xdg) = isolated_ssh_catalog("create-save");
        let mut app = app_for_mouse_test();
        open_settings_at(&mut app.state, SettingsSection::Connections);

        // Open a blank form and fill target, optional name, and optional directory.
        assert_eq!(connection_key(&mut app.state, KeyCode::Down), None);
        assert_eq!(connection_key(&mut app.state, KeyCode::Char(' ')), None);
        assert!(app.state.settings.connection_editor.is_some());
        connection_type(&mut app.state, "builder@example.com");
        connection_key(&mut app.state, KeyCode::Down);
        connection_type(&mut app.state, "build box");
        connection_key(&mut app.state, KeyCode::Down);
        connection_type(&mut app.state, "~/src");
        connection_key(&mut app.state, KeyCode::Down);
        assert_eq!(app.state.settings.list.selected, CONNECTION_SAVE_INDEX);

        let action = connection_key(&mut app.state, KeyCode::Enter);
        match &action {
            Some(SettingsAction::SaveSshConnectionProfile(profile)) => {
                assert_eq!(profile.id(), "build-box");
                assert_eq!(profile.host_binding_generation(), 1);
            }
            other => panic!("expected save action, got {other:?}"),
        }
        app.apply_settings_action(action.expect("save action"));

        assert_eq!(app.state.ssh_connection_profiles.len(), 1);
        let saved = &app.state.ssh_connection_profiles[0];
        assert_eq!(saved.id(), "build-box");
        assert_eq!(saved.name(), "build box");
        assert_eq!(saved.target(), "builder@example.com");
        assert_eq!(
            saved
                .suggested_directory()
                .map(|dir| dir.to_string())
                .as_deref(),
            Some("~/src")
        );
        let on_disk = crate::persist::ssh_profiles::load();
        assert_eq!(on_disk.len(), 1);
        assert_eq!(on_disk[0].id(), "build-box");
        assert!(app.state.settings.connection_editor.is_none());
    }

    #[test]
    fn connection_edit_rename_preserves_id_and_target_edit_bumps_generation() {
        let (_lock, _xdg) = isolated_ssh_catalog("edit-generation");
        let mut app = app_for_mouse_test();
        seed_connection_profile(
            &mut app,
            "build-box",
            "build box",
            "builder@example.com",
            None,
        );
        open_settings_at(&mut app.state, SettingsSection::Connections);

        // Open the saved profile, then choose the explicit edit action.
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Down);
        assert_eq!(connection_key(&mut app.state, KeyCode::Enter), None);
        app.state.settings.list.select(
            ConnectionRowId::Action(crate::settings_rows::ConnectionAction::EditDetails)
                .selection_index(),
        );
        assert!(app
            .state
            .settings
            .connection_editor
            .as_ref()
            .is_some_and(crate::app::state::ConnectionEditorState::is_detail));
        assert_eq!(connection_key(&mut app.state, KeyCode::Enter), None);
        assert!(app
            .state
            .settings
            .connection_editor
            .as_ref()
            .is_some_and(crate::app::state::ConnectionEditorState::is_editing));
        assert_eq!(
            app.state
                .settings
                .connection_editor
                .as_ref()
                .and_then(|editor| editor.profile_id()),
            Some("build-box")
        );

        // Rename only: id and binding generation stay.
        connection_key(&mut app.state, KeyCode::Down);
        update_settings_state(
            &mut app.state,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        );
        connection_type(&mut app.state, "build farm");
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Down);
        let action = connection_key(&mut app.state, KeyCode::Enter);
        match &action {
            Some(SettingsAction::SaveSshConnectionProfile(profile)) => {
                assert_eq!(profile.id(), "build-box");
                assert_eq!(profile.name(), "build farm");
                assert_eq!(profile.target(), "builder@example.com");
                assert_eq!(profile.host_binding_generation(), 1);
            }
            other => panic!("expected save action, got {other:?}"),
        }
        app.apply_settings_action(action.expect("save action"));
        assert_eq!(app.state.ssh_connection_profiles.len(), 1);
        assert_eq!(app.state.ssh_connection_profiles[0].name(), "build farm");
        assert_eq!(
            app.state.ssh_connection_profiles[0].host_binding_generation(),
            1
        );

        // Edit the target: same id, generation bumps.
        assert!(load_connection_profile_detail(&mut app.state, "build-box"));
        app.state.settings.list.select(
            ConnectionRowId::Action(crate::settings_rows::ConnectionAction::EditDetails)
                .selection_index(),
        );
        assert_eq!(connection_key(&mut app.state, KeyCode::Enter), None);
        update_settings_state(
            &mut app.state,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        );
        connection_type(&mut app.state, "deploy@example.com");
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Down);
        assert_eq!(
            app.state
                .settings
                .connection_editor
                .as_ref()
                .map(|e| e.draft.target.as_str()),
            Some("deploy@example.com")
        );
        assert_eq!(app.state.settings.list.selected, CONNECTION_SAVE_INDEX);
        assert_eq!(
            app.state
                .settings
                .connection_editor
                .as_ref()
                .and_then(|e| e.profile_id()),
            Some("build-box")
        );
        assert_eq!(
            app.state
                .settings
                .connection_editor
                .as_ref()
                .map(|e| e.draft.name.as_str()),
            Some("build farm")
        );
        let action = connection_key(&mut app.state, KeyCode::Enter);
        match &action {
            Some(SettingsAction::SaveSshConnectionProfile(profile)) => {
                assert_eq!(profile.id(), "build-box");
                assert_eq!(profile.target(), "deploy@example.com");
                assert_eq!(profile.host_binding_generation(), 2);
                assert_eq!(profile.execution_host_id().to_string(), "ssh:build-box:2");
            }
            other => panic!("expected save action, got {other:?}"),
        }
        app.apply_settings_action(action.expect("save action"));
        assert_eq!(app.state.ssh_connection_profiles.len(), 1);
        assert_eq!(
            app.state.ssh_connection_profiles[0].host_binding_generation(),
            2
        );
    }

    #[test]
    fn connection_discard_returns_to_detail_and_restores_persisted_fields() {
        let (_lock, _xdg) = isolated_ssh_catalog("discard-edit");
        let mut app = app_for_mouse_test();
        seed_connection_profile(
            &mut app,
            "build-box",
            "build box",
            "builder@example.com",
            None,
        );
        open_settings_at(&mut app.state, SettingsSection::Connections);
        assert!(load_connection_profile_detail(&mut app.state, "build-box"));
        app.state.settings.list.select(
            ConnectionRowId::Action(crate::settings_rows::ConnectionAction::EditDetails)
                .selection_index(),
        );
        assert_eq!(connection_key(&mut app.state, KeyCode::Enter), None);
        update_settings_state(
            &mut app.state,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        );
        connection_type(&mut app.state, "changed@example.com");

        app.state.settings.list.select(CONNECTION_DISCARD_INDEX);
        assert_eq!(connection_key(&mut app.state, KeyCode::Enter), None);
        let editor = app
            .state
            .settings
            .connection_editor
            .as_ref()
            .expect("detail remains open");
        assert!(editor.is_detail());
        assert_eq!(editor.draft.target, "builder@example.com");
        assert_eq!(app.state.settings.focused_input, None);
    }

    #[test]
    fn connection_ids_get_deterministic_numeric_suffix() {
        let (_lock, _xdg) = isolated_ssh_catalog("id-suffix");
        let mut app = app_for_mouse_test();
        seed_connection_profile(
            &mut app,
            "build-box",
            "primary",
            "builder@example.com",
            None,
        );
        seed_connection_profile(
            &mut app,
            "build-box-2",
            "secondary",
            "deploy@example.com",
            None,
        );
        open_settings_at(&mut app.state, SettingsSection::Connections);
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Char(' '));
        connection_type(&mut app.state, "ops@example.com");
        connection_key(&mut app.state, KeyCode::Down);
        connection_type(&mut app.state, "build box");
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Down);
        let action = connection_key(&mut app.state, KeyCode::Enter);
        match &action {
            Some(SettingsAction::SaveSshConnectionProfile(profile)) => {
                assert_eq!(profile.id(), "build-box-3");
            }
            other => panic!("expected save action, got {other:?}"),
        }
    }

    #[test]
    fn connection_delete_requires_retirement_preview_before_catalog_mutation() {
        let (_lock, _xdg) = isolated_ssh_catalog("delete");
        let mut app = app_for_mouse_test();
        seed_connection_profile(
            &mut app,
            "build-box",
            "build box",
            "builder@example.com",
            None,
        );
        seed_connection_profile(&mut app, "staging", "staging", "staging@example.com", None);
        open_settings_at(&mut app.state, SettingsSection::Connections);

        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Enter);
        app.state.settings.list.select(CONNECTION_DELETE_INDEX);
        let action = connection_key(&mut app.state, KeyCode::Enter);
        assert_eq!(
            action,
            Some(SettingsAction::PreviewSshConnectionRetirement(
                "build-box".to_string()
            ))
        );
        assert_eq!(app.state.ssh_connection_profiles.len(), 2);
        assert!(app.state.settings.connection_editor.is_some());
    }

    #[test]
    fn connection_detail_down_reaches_test_action() {
        let (_lock, _xdg) = isolated_ssh_catalog("detail-keyboard-test");
        let mut app = app_for_mouse_test();
        seed_connection_profile(
            &mut app,
            "build-box",
            "build box",
            "builder@example.com",
            None,
        );
        open_settings_at(&mut app.state, SettingsSection::Connections);
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Enter);

        connection_key(&mut app.state, KeyCode::Down);

        assert_eq!(
            connection_key(&mut app.state, KeyCode::Enter),
            Some(SettingsAction::TestSshConnection {
                profile_id: "build-box".to_string(),
            })
        );
    }

    #[test]
    fn connection_detail_up_reaches_remove_action() {
        let (_lock, _xdg) = isolated_ssh_catalog("detail-keyboard-remove");
        let mut app = app_for_mouse_test();
        seed_connection_profile(
            &mut app,
            "build-box",
            "build box",
            "builder@example.com",
            None,
        );
        open_settings_at(&mut app.state, SettingsSection::Connections);
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Enter);

        connection_key(&mut app.state, KeyCode::Up);
        connection_key(&mut app.state, KeyCode::Up);

        assert_eq!(
            connection_key(&mut app.state, KeyCode::Enter),
            Some(SettingsAction::PreviewSshConnectionRetirement(
                "build-box".to_string()
            ))
        );
    }

    #[test]
    fn failed_retirement_offers_one_step_local_removal() {
        let (_lock, _xdg) = isolated_ssh_catalog("local-forget");
        let mut app = app_for_mouse_test();
        seed_connection_profile(
            &mut app,
            "build-box",
            "build box",
            "builder@example.com",
            None,
        );
        open_settings_at(&mut app.state, SettingsSection::Connections);
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Enter);
        app.state
            .settings
            .connection_editor
            .as_mut()
            .expect("editor")
            .connection_retirement = Some(crate::app::state::ConnectionRetirementState::Failed);
        app.state.settings.list.selected = crate::settings_rows::ConnectionRowId::Action(
            crate::settings_rows::ConnectionAction::ForgetConnection,
        )
        .selection_index();

        match selected_connection_profile_action(&mut app.state) {
            Some(SettingsAction::ConfirmLocalConnectionForget { profile_id, plan }) => {
                assert_eq!(profile_id, "build-box");
                assert_eq!(plan.host_id.as_str(), "ssh:build-box:1");
            }
            other => panic!("expected local removal confirmation, got {other:?}"),
        }

        let rows = crate::settings_rows::rows_for_section(
            &app.state,
            crate::app::state::SettingsSection::Connections,
        )
        .expect("connection rows");
        let mut captions = rows.iter().filter_map(|row| match row {
            crate::settings_rows::SettingsListRow::Caption(text) => Some(text.as_ref()),
            _ => None,
        });
        assert!(captions
            .clone()
            .any(|text| text.contains("Full cleanup is unavailable")));
        assert!(captions.any(|text| text.contains("Remove the saved connection anyway?")));
        let mut actions = rows.iter().filter_map(|row| match row {
            crate::settings_rows::SettingsListRow::Action { label, .. } => Some(label.as_ref()),
            _ => None,
        });
        assert!(actions
            .clone()
            .any(|label| label == "Remove Saved Connection"));
        assert!(actions.clone().any(|label| label == "Try Again"));
        assert!(actions.any(|label| label == "Cancel"));
    }

    #[test]
    fn connection_browse_ctrl_d_opens_retirement_confirmation() {
        let (_lock, _xdg) = isolated_ssh_catalog("ctrl-d-delete");
        let mut app = app_for_mouse_test();
        seed_connection_profile(
            &mut app,
            "build-box",
            "build box",
            "builder@example.com",
            None,
        );
        open_settings_at(&mut app.state, SettingsSection::Connections);
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Down);
        let action = update_settings_state(
            &mut app.state,
            KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL),
        );
        assert_eq!(
            action,
            Some(SettingsAction::PreviewSshConnectionRetirement(
                "build-box".to_string()
            ))
        );
        assert!(app.state.settings.connection_editor.is_some());
        assert_eq!(app.state.settings.list.selected, CONNECTION_DELETE_INDEX);
        assert_eq!(app.state.ssh_connection_profiles.len(), 1);
    }

    #[test]
    fn connection_save_requires_target_and_defaults_name() {
        let (_lock, _xdg) = isolated_ssh_catalog("validation");
        let mut app = app_for_mouse_test();
        open_settings_at(&mut app.state, SettingsSection::Connections);
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Char(' '));

        // Empty and whitespace-only targets are refused.
        app.state.settings.list.select(CONNECTION_SAVE_INDEX);
        assert_eq!(connection_key(&mut app.state, KeyCode::Enter), None);
        assert!(app.state.settings.connection_editor.is_some());
        assert!(app.state.ssh_connection_profiles.is_empty());
        app.state.settings.list.select(CONNECTION_TARGET_INDEX);
        app.state.settings.focused_input = Some(CONNECTION_TARGET_INDEX);
        connection_type(&mut app.state, "   ");
        app.state.settings.list.select(CONNECTION_SAVE_INDEX);
        assert_eq!(connection_key(&mut app.state, KeyCode::Enter), None);
        assert!(app.state.ssh_connection_profiles.is_empty());

        // A target is sufficient; it becomes the display name when no label is given.
        app.state.settings.list.select(CONNECTION_TARGET_INDEX);
        app.state.settings.focused_input = Some(CONNECTION_TARGET_INDEX);
        update_settings_state(
            &mut app.state,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        );
        connection_type(&mut app.state, "builder@example.com");
        app.state.settings.list.select(CONNECTION_SAVE_INDEX);
        let action = connection_key(&mut app.state, KeyCode::Enter);
        let Some(SettingsAction::SaveSshConnectionProfile(profile)) = &action else {
            panic!("expected save action, got {action:?}");
        };
        assert_eq!(profile.name(), "builder@example.com");
        assert_eq!(profile.target(), "builder@example.com");
        app.apply_settings_action(action.expect("save action"));
        assert_eq!(app.state.ssh_connection_profiles.len(), 1);
    }

    #[test]
    fn connection_lifecycle_actions_queue_typed_requests_without_success() {
        let (_lock, _xdg) = isolated_ssh_catalog("requests");
        let mut app = app_for_mouse_test();
        let profile = seed_connection_profile(
            &mut app,
            "build-box",
            "build box",
            "builder@example.com",
            None,
        );
        open_settings_at(&mut app.state, SettingsSection::Connections);

        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Enter);

        app.state.settings.list.select(CONNECTION_TEST_INDEX);
        let action = connection_key(&mut app.state, KeyCode::Enter);
        assert_eq!(
            action,
            Some(SettingsAction::TestSshConnection {
                profile_id: "build-box".to_string()
            })
        );
        app.apply_settings_action(action.expect("test action"));
        let toast = app
            .state
            .toast
            .as_ref()
            .expect("test should toast immediately");
        assert_eq!(toast.title, "SSH connection request failed");
        assert_eq!(toast.context, "unknown SSH connection profile build-box");
        app.state.toast = None;

        app.state.settings.list.select(
            ConnectionRowId::Action(crate::settings_rows::ConnectionAction::Toggle)
                .selection_index(),
        );
        let action = connection_key(&mut app.state, KeyCode::Enter);
        assert_eq!(
            action,
            Some(SettingsAction::ConnectSshConnection {
                profile_id: "build-box".to_string()
            })
        );
        app.apply_settings_action(action.expect("connect action"));

        let owner =
            crate::execution_host::auth::AuthenticationOwner::new(app.default_client_view.id());
        assert_eq!(
            app.state.pending_ssh_connection_requests,
            vec![crate::app::state::SshConnectionRequest {
                profile_id: "build-box".to_string(),
                action: crate::execution_host::HostConnectionAction::Connect,
                authentication_owner: owner,
            }]
        );
        // Connect still queues; Test already reported through the toast path.
        assert!(app.state.host_connection_states.is_empty());
        assert_eq!(
            app.state.ssh_connection_status(&profile),
            crate::execution_host::ConnectionStatus::Disconnected
        );
        assert!(app.state.toast.is_none());

        // When pure state reports Connected, the same row offers Disconnect.
        app.state.host_connection_states.insert(
            profile.execution_host_id(),
            crate::execution_host::ConnectionStatus::Connected,
        );
        let action = connection_key(&mut app.state, KeyCode::Enter);
        assert_eq!(
            action,
            Some(SettingsAction::DisconnectSshConnection {
                profile_id: "build-box".to_string()
            })
        );
        app.apply_settings_action(action.expect("disconnect action"));
        assert_eq!(app.state.pending_ssh_connection_requests.len(), 2);
        assert_eq!(
            app.state.pending_ssh_connection_requests[1].action,
            crate::execution_host::HostConnectionAction::Disconnect
        );
    }

    #[test]
    fn connection_drafts_are_client_local_across_views() {
        let (_lock, _xdg) = isolated_ssh_catalog("draft-isolation");
        let mut app = app_for_mouse_test();
        let mut first = ClientViewState::from_default_client_state(&app.state);
        let mut second = ClientViewState::from_default_client_state(&app.state);
        prepare_general_settings_state(
            &app.state,
            &mut first.settings,
            SettingsSection::Connections,
        );
        first.mode = Mode::Settings;
        prepare_general_settings_state(
            &app.state,
            &mut second.settings,
            SettingsSection::Connections,
        );
        second.mode = Mode::Settings;

        // First client opens the form and drafts an SSH target.
        update_settings_state_for_view(
            &mut app.state,
            &mut first,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        update_settings_state_for_view(
            &mut app.state,
            &mut first,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        for ch in "alpha".chars() {
            update_settings_state_for_view(
                &mut app.state,
                &mut first,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty()),
            );
        }

        assert_eq!(
            first
                .settings
                .connection_editor
                .as_ref()
                .map(|e| e.draft.target.as_str()),
            Some("alpha")
        );
        assert!(second.settings.connection_editor.is_none());
        assert!(app.state.settings.connection_editor.is_none());

        // Second client drafts independently; the first draft is untouched.
        update_settings_state_for_view(
            &mut app.state,
            &mut second,
            KeyEvent::new(KeyCode::Down, KeyModifiers::empty()),
        );
        update_settings_state_for_view(
            &mut app.state,
            &mut second,
            KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
        );
        for ch in "beta".chars() {
            update_settings_state_for_view(
                &mut app.state,
                &mut second,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty()),
            );
        }

        assert_eq!(
            second
                .settings
                .connection_editor
                .as_ref()
                .map(|e| e.draft.target.as_str()),
            Some("beta")
        );
        assert_eq!(
            first
                .settings
                .connection_editor
                .as_ref()
                .map(|e| e.draft.target.as_str()),
            Some("alpha")
        );
        assert!(app.state.settings.connection_editor.is_none());
    }

    #[test]
    fn settings_sidebar_orders_connections_before_advanced() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::Connections);
        let entries = crate::ui::settings_sidebar_entries(&state.settings);
        let sections = entries
            .iter()
            .filter(|entry| entry.subsection.is_none())
            .map(|entry| entry.section)
            .collect::<Vec<_>>();

        assert_eq!(sections, SettingsSection::ALL);
        assert!(
            sections
                .iter()
                .position(|section| *section == SettingsSection::Connections)
                < sections
                    .iter()
                    .position(|section| *section == SettingsSection::Experiments)
        );
    }

    #[test]
    fn connections_sidebar_entry_is_clickable() {
        let mut app = app_for_mouse_test();
        app.state.view.terminal_area = Rect::new(0, 0, 100, 30);
        open_settings_at(&mut app.state, SettingsSection::Connections);

        let click_section = |app: &mut App, section| {
            let inner = app.state.settings_inner_rect();
            let content = crate::ui::settings_stack_areas(&app.state, inner).content;
            let navigation = crate::ui::settings_sidebar_areas(content).navigation;
            let (_, rect) = crate::ui::settings_sidebar_hit_areas(&app.state.settings, navigation)
                .into_iter()
                .find(|(entry, _)| entry.section == section && entry.subsection.is_none())
                .expect("sidebar entry");
            app.state.handle_settings_mouse(mouse(
                MouseEventKind::Down(MouseButton::Left),
                rect.x + 1,
                rect.y,
            ));
        };

        click_section(&mut app, SettingsSection::Integrations);
        assert_eq!(app.state.settings.section, SettingsSection::Integrations);
        click_section(&mut app, SettingsSection::Connections);
        assert_eq!(app.state.settings.section, SettingsSection::Connections);
    }

    #[test]
    fn connections_mouse_click_opens_editor_focuses_fields_and_discards() {
        let (_lock, _xdg) = isolated_ssh_catalog("mouse-editor");
        let mut app = app_for_mouse_test();
        app.state.view.terminal_area = Rect::new(26, 0, 100, 30);
        open_settings_at(&mut app.state, SettingsSection::Connections);

        let list_area = settings_section_list_rect(&app.state, SettingsSection::Connections);
        let rows = rows_for_section(&app.state, SettingsSection::Connections).unwrap();
        let row_for = |index| selected_visual_row(&rows, index).unwrap() as u16;

        // Click "add ssh connection" to open the blank form.
        let action = app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            list_area.x + 2,
            list_area.y + row_for(0),
        ));
        assert_eq!(action, None);
        assert!(app.state.settings.connection_editor.is_some());
        assert_eq!(
            app.state.settings.focused_input,
            Some(CONNECTION_TARGET_INDEX)
        );

        // Click the target field to focus it, then type into it.
        let editor_rows = rows_for_section(&app.state, SettingsSection::Connections).unwrap();
        let editor_row_for = |index| selected_visual_row(&editor_rows, index).unwrap() as u16;
        app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            list_area.x + 2,
            list_area.y + editor_row_for(CONNECTION_TARGET_INDEX),
        ));
        assert_eq!(
            app.state.settings.focused_input,
            Some(CONNECTION_TARGET_INDEX)
        );
        connection_type(&mut app.state, "builder@example.com");
        assert_eq!(
            app.state
                .settings
                .connection_editor
                .as_ref()
                .map(|e| e.draft.target.as_str()),
            Some("builder@example.com")
        );

        // Scroll to the action rows, then click "cancel" to close without saving.
        let current_rows =
            rows_for_section(&app.state, SettingsSection::Connections).expect("connection rows");
        let discard_row = selected_visual_row(&current_rows, CONNECTION_DISCARD_INDEX)
            .expect("cancel row") as u16;
        app.state.settings.scroll =
            settings_section_max_scroll(&app.state, SettingsSection::Connections);
        let list = settings_section_list_geometry(&app.state, SettingsSection::Connections);
        let visible_discard_row = discard_row.saturating_sub(list.viewport.scroll() as u16);
        assert_eq!(
            app.state
                .settings_list_hit_at(list.rect.x + 2, list.rect.y + visible_discard_row)
                .map(|target| target.index),
            Some(CONNECTION_DISCARD_INDEX)
        );
        app.state.handle_settings_mouse(mouse(
            MouseEventKind::Down(MouseButton::Left),
            list.rect.x + 2,
            list.rect.y + visible_discard_row,
        ));
        assert!(app.state.settings.connection_editor.is_none());
        assert!(app.state.ssh_connection_profiles.is_empty());
    }

    #[test]
    fn connections_render_shows_saved_profile_and_status() {
        let (_lock, _xdg) = isolated_ssh_catalog("render-browse");
        let mut app = app_for_mouse_test();
        let profile = seed_connection_profile(
            &mut app,
            "build-box",
            "build box",
            "builder@example.com",
            Some("~/src"),
        );
        app.state.host_connection_states.insert(
            profile.execution_host_id(),
            crate::execution_host::ConnectionStatus::Connected,
        );
        open_settings_at(&mut app.state, SettingsSection::Connections);

        rendered_text_point(&app, "Connections", 100, 30);
        rendered_text_point(&app, "Saved Profiles", 100, 30);
        rendered_text_point(&app, "Add SSH Connection", 100, 30);
        rendered_text_point(&app, "build box", 100, 30);
        rendered_text_point(&app, "builder@example.com", 100, 30);
        rendered_text_point(&app, "~/src", 100, 30);
        rendered_text_point(&app, "Connected", 100, 30);
    }

    #[test]
    fn connection_detail_render_prioritizes_runtime_controls_over_editing() {
        let (_lock, _xdg) = isolated_ssh_catalog("render-detail");
        let mut app = app_for_mouse_test();
        let profile = seed_connection_profile(
            &mut app,
            "build-box",
            "build box",
            "builder@example.com",
            Some("~/src"),
        );
        app.state.host_connection_states.insert(
            profile.execution_host_id(),
            crate::execution_host::ConnectionStatus::Connected,
        );
        open_settings_at(&mut app.state, SettingsSection::Connections);
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Enter);

        rendered_text_point(&app, "Connection Details", 100, 30);
        let (_, open_workspace_row) = rendered_text_point(&app, "Open Workspace", 100, 30);
        let (_, disconnect_row) = rendered_text_point(&app, "Disconnect", 100, 30);
        let (_, test_row) = rendered_text_point(&app, "Test Connection", 100, 30);
        assert!(
            open_workspace_row < disconnect_row && disconnect_row < test_row,
            "the primary workspace action should appear before maintenance controls"
        );
        rendered_text_point(&app, "SSH target  builder@example.com", 100, 30);
        rendered_text_point(&app, "Edit Details", 100, 30);
        let backend = ratatui::backend::TestBackend::new(100, 30);
        let mut terminal = ratatui::Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| crate::ui::render(&app.state, frame))
            .expect("render connection details");
        let buffer = terminal.backend().buffer();
        let mut rendered = String::new();
        for y in 0..30 {
            for x in 0..100 {
                rendered.push_str(buffer[(x, y)].symbol());
            }
        }
        assert!(
            !rendered.contains("execution worker"),
            "connection details should not expose managed implementation details"
        );

        app.state.settings.scroll =
            settings_section_max_scroll(&app.state, SettingsSection::Connections);
        rendered_text_point(&app, "Danger Zone", 100, 30);
        rendered_text_point(&app, "Remove Connection", 100, 30);
    }

    #[test]
    fn connection_editor_render_shows_form_labels() {
        let (_lock, _xdg) = isolated_ssh_catalog("render-editor");
        let mut app = app_for_mouse_test();
        open_settings_at(&mut app.state, SettingsSection::Connections);
        connection_key(&mut app.state, KeyCode::Down);
        connection_key(&mut app.state, KeyCode::Char(' '));

        rendered_text_point(&app, "Add Connection", 100, 30);
        rendered_text_point(&app, "SSH Target", 100, 30);
        rendered_text_point(&app, "Name (Optional)", 100, 30);
        rendered_text_point(&app, "Starting Directory (Optional)", 100, 30);
        rendered_text_point(&app, "Credentials and host keys stay with OpenSSH", 100, 30);

        // Move to the action rows so they scroll into view.
        for _ in 0..CONNECTION_DISCARD_INDEX {
            connection_key(&mut app.state, KeyCode::Down);
        }
        rendered_text_point(&app, "SSH Target Is Required", 100, 30);
        rendered_text_point(&app, "Add Connection", 100, 30);
        rendered_text_point(&app, "Cancel", 100, 30);
    }

    fn integration_recommendation_for(
        target: crate::api::schema::IntegrationTarget,
        state: crate::integration::IntegrationStatusKind,
        available: bool,
    ) -> crate::integration::IntegrationRecommendation {
        crate::integration::IntegrationRecommendation {
            target,
            label: crate::integration::integration_target_label(target),
            command: crate::integration::integration_target_command(target),
            available,
            path: std::path::PathBuf::from("/tmp/gardn-test-integration"),
            state,
        }
    }

    fn integration_recommendation(
        state: crate::integration::IntegrationStatusKind,
        available: bool,
    ) -> crate::integration::IntegrationRecommendation {
        integration_recommendation_for(
            crate::api::schema::IntegrationTarget::Claude,
            state,
            available,
        )
    }
    #[test]
    fn settings_behavior_edits_shell_and_invalid_toast_delay() {
        let mut state = state_with_workspaces(&["test"]);
        state.default_shell = String::new();
        state.shell_mode = ShellModeConfig::Auto;
        open_settings_at(&mut state, SettingsSection::PaneLabels);
        state
            .settings
            .list
            .select(BehaviorRowId::DefaultShell.selection_index());
        focus_selected_settings_input(&mut state);
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char('/'), KeyModifiers::empty()),
            ),
            Some(SettingsAction::SaveDefaultShell("/".to_string()))
        );
        for ch in "bin/zsh".chars() {
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char(ch), KeyModifiers::empty()),
            );
        }
        assert_eq!(
            state.settings.pending_default_shell.as_deref(),
            Some("/bin/zsh")
        );

        state
            .settings
            .list
            .select(BehaviorRowId::ShellMode.selection_index());
        state.settings.focused_input = None;
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
            ),
            Some(SettingsAction::SaveShellMode(ShellModeConfig::Login))
        );

        switch_settings_section(&mut state, SettingsSection::Sound, 0);
        state
            .settings
            .list
            .select(NotificationRowId::ToastDelay.selection_index());
        focus_selected_settings_input(&mut state);
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
            ),
            None
        );
        assert_eq!(state.settings.pending_toast_delay.as_deref(), Some(""));
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char('9'), KeyModifiers::empty()),
            ),
            Some(SettingsAction::SaveToastDelay(9))
        );
    }

    #[test]
    fn settings_text_commit_updates_client_owned_shell_and_delay_drafts() {
        let mut state = state_with_workspaces(&["test"]);
        open_settings_at(&mut state, SettingsSection::PaneLabels);
        state
            .settings
            .list
            .select(BehaviorRowId::DefaultShell.selection_index());
        focus_selected_settings_input(&mut state);
        let shared_draft = state.settings.pending_default_shell.clone();
        let mut view = ClientViewState::from_default_client_state(&state);

        assert_eq!(
            paste_settings_text_for_view(&mut state, &mut view, "/bin/zsh"),
            Some(Some(SettingsAction::SaveDefaultShell(
                "/bin/zsh".to_string()
            )))
        );
        assert_eq!(
            view.settings.pending_default_shell.as_deref(),
            Some("/bin/zsh")
        );
        assert_eq!(state.settings.pending_default_shell, shared_draft);

        view.settings.section = SettingsSection::Sound;
        view.settings
            .list
            .select(NotificationRowId::ToastDelay.selection_index());
        view.settings.focused_input = Some(NotificationRowId::ToastDelay.selection_index());
        view.settings.pending_toast_delay = Some(String::new());
        assert_eq!(
            paste_settings_text_for_view(&mut state, &mut view, "3601x"),
            Some(None)
        );
        assert_eq!(view.settings.pending_toast_delay.as_deref(), Some("3601"));
    }

    #[test]
    fn settings_notifications_cycle_positions_and_copy_confirmation() {
        let mut state = state_with_workspaces(&["test"]);
        state.toast_config.clipboard.position = ToastClipboardPosition::BottomRight;
        open_settings_at(&mut state, SettingsSection::Sound);
        state.settings.list.show();
        state
            .settings
            .list
            .select(NotificationRowId::ToastGardnPosition.selection_index());
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
            ),
            Some(SettingsAction::SaveToastGardnPosition(
                ToastGardnPosition::TopLeft
            ))
        );
        state
            .settings
            .list
            .select(NotificationRowId::ClipboardEnabled.selection_index());
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            ),
            Some(SettingsAction::SaveClipboardToastEnabled(false))
        );
        state
            .settings
            .list
            .select(NotificationRowId::ClipboardPosition.selection_index());
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Char(' '), KeyModifiers::empty()),
            ),
            Some(SettingsAction::SaveClipboardToastPosition(
                ToastClipboardPosition::TopLeft
            ))
        );
    }

    #[test]
    fn settings_advanced_toggles_update_checks() {
        let mut state = state_with_workspaces(&["test"]);
        state.update_version_check = true;
        state.update_manifest_check = true;
        open_settings_at(&mut state, SettingsSection::Experiments);
        state.settings.list.show();
        state
            .settings
            .list
            .select(AdvancedRowId::VersionCheck.selection_index());
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            ),
            Some(SettingsAction::SaveVersionCheck(false))
        );
        state
            .settings
            .list
            .select(AdvancedRowId::ManifestCheck.selection_index());
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            ),
            Some(SettingsAction::SaveManifestCheck(false))
        );
    }

    #[test]
    fn saving_disabled_agent_profile_preserves_enabled_false() {
        let mut state = state_with_workspaces(&["test"]);
        state.agent_profiles = crate::agent_profiles::AgentProfileCatalog::from_config(
            &crate::agent_profiles::AgentProfilesConfig {
                order: vec!["user:quiet".to_string()],
                custom: vec![crate::agent_profiles::UserAgentProfileConfig {
                    id: "quiet".to_string(),
                    name: "quiet".to_string(),
                    kind: crate::agent_profiles::AgentKind::Custom,
                    command: "true".to_string(),
                    env: std::collections::BTreeMap::from([(
                        "KEEP".to_string(),
                        "yes".to_string(),
                    )]),
                    enabled: false,
                }],
            },
        );
        open_settings_at(&mut state, SettingsSection::Agents);
        assert!(load_custom_agent_profile_editor(&mut state, "user:quiet"));
        assert_eq!(state.settings.pending_agent_profile_enabled, Some(false));
        state.settings.list.selected = agent_profile_save_index(&state);
        state.settings.list.show();
        state.settings.focused_input = None;
        assert_eq!(
            update_settings_state(
                &mut state,
                KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()),
            ),
            Some(SettingsAction::SaveAgentProfile(
                crate::agent_profiles::UserAgentProfileConfig {
                    id: "quiet".to_string(),
                    name: "quiet".to_string(),
                    kind: crate::agent_profiles::AgentKind::Custom,
                    command: "true".to_string(),
                    env: std::collections::BTreeMap::from([(
                        "KEEP".to_string(),
                        "yes".to_string(),
                    )]),
                    enabled: false,
                }
            ))
        );
        assert_eq!(state.settings.pending_agent_profile_enabled, Some(false));
        assert_eq!(
            state.settings.pending_agent_profile_id.as_deref(),
            Some("user:quiet")
        );
    }
}
