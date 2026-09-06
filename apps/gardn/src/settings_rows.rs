use std::borrow::Cow;

use crate::{
    app::{
        state::{
            normalize_theme_name, theme_names_for_appearance, AppState, SettingsSection,
            SettingsState,
        },
        ClientViewState,
    },
    config::{CommandsConfig, NewTerminalCwdConfig, TerminalAccent, ThemeMode, ToastDelivery},
    terminal_theme::ThemeAppearance,
};

pub(crate) const GROUP_GENERAL_NAME: usize = 0;
pub(crate) const GROUP_GENERAL_ICON: usize = 1;
pub(crate) const GROUP_GENERAL_DELETE: usize = 2;
pub(crate) const GROUP_DEFAULTS_HOST: usize = 0;
pub(crate) const GROUP_DEFAULTS_DIRECTORY: usize = 1;
pub(crate) const GROUP_GITHUB_ORGANIZATION: usize = 0;
pub(crate) const WORKSPACE_GENERAL_NAME: usize = 0;
pub(crate) const WORKSPACE_GENERAL_HOST: usize = 1;
pub(crate) const WORKSPACE_GENERAL_DIRECTORY: usize = 2;
pub(crate) const WORKSPACE_GITHUB_AUTOMATIC: usize = 0;
pub(crate) const WORKSPACE_GITHUB_SELECTED: usize = 1;
pub(crate) const WORKSPACE_GITHUB_GROUP: usize = 2;
pub(crate) const WORKSPACE_GITHUB_REPOSITORIES: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SettingsListRow {
    Header(&'static str),
    Caption(Cow<'static, str>),
    Spacer,
    Toggle {
        index: usize,
        title: Cow<'static, str>,
        description: Cow<'static, str>,
        enabled: bool,
    },
    Value {
        index: usize,
        title: Cow<'static, str>,
        description: Cow<'static, str>,
        value: Cow<'static, str>,
        editable: bool,
    },
    TextInput {
        index: usize,
        title: Cow<'static, str>,
        value: Cow<'static, str>,
    },
    Choice {
        index: usize,
        label: Cow<'static, str>,
        checked: bool,
    },
    Action {
        index: usize,
        icon: Cow<'static, str>,
        label: Cow<'static, str>,
        tone: SettingsMarkerTone,
    },
    Status {
        index: usize,
        label: Cow<'static, str>,
        status: Cow<'static, str>,
        tone: SettingsMarkerTone,
    },
    Profile {
        index: usize,
        name: Cow<'static, str>,
        detail: Cow<'static, str>,
        badge: Option<Cow<'static, str>>,
        tone: SettingsMarkerTone,
    },
    GroupIconPicker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsMarkerTone {
    Good,
    Warning,
    Accent,
    Danger,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SettingsRowHit {
    pub index: usize,
    pub hoverable: bool,
}

pub(crate) fn option_hit_for_visual_row(
    rows: &[SettingsListRow],
    row: usize,
) -> Option<SettingsRowHit> {
    let mut visual_row = 0;
    for entry in rows {
        match entry {
            SettingsListRow::Header(_) | SettingsListRow::Caption(_) | SettingsListRow::Spacer => {
                if row == visual_row {
                    return None;
                }
                visual_row += 1;
            }
            SettingsListRow::GroupIconPicker => {
                let height = group_icon_picker_visual_rows();
                if row >= visual_row && row < visual_row + height {
                    return None;
                }
                visual_row += height;
            }
            SettingsListRow::Toggle { index, .. } => {
                if row == visual_row || row == visual_row + 1 {
                    return Some(SettingsRowHit {
                        index: *index,
                        hoverable: true,
                    });
                }
                visual_row += 2;
            }
            SettingsListRow::Value {
                index, editable, ..
            } => {
                if row == visual_row || row == visual_row + 1 {
                    return Some(SettingsRowHit {
                        index: *index,
                        hoverable: !editable,
                    });
                }
                visual_row += 2;
            }
            SettingsListRow::TextInput { index, .. } => {
                if row == visual_row + 1 {
                    return Some(SettingsRowHit {
                        index: *index,
                        hoverable: false,
                    });
                }
                visual_row += 2;
            }
            SettingsListRow::Choice { index, .. }
            | SettingsListRow::Action { index, .. }
            | SettingsListRow::Status { index, .. } => {
                if row == visual_row {
                    return Some(SettingsRowHit {
                        index: *index,
                        hoverable: true,
                    });
                }
                visual_row += 1;
            }
            SettingsListRow::Profile { index, .. } => {
                if row == visual_row {
                    return Some(SettingsRowHit {
                        index: *index,
                        hoverable: true,
                    });
                }
                visual_row += 1;
            }
        }
    }
    None
}

pub(crate) fn rows_for_section(
    app: &AppState,
    section: SettingsSection,
) -> Option<Vec<SettingsListRow>> {
    rows_for_section_with_settings(app, &app.settings, section)
}

fn rows_for_section_with_settings(
    app: &AppState,
    settings: &SettingsState,
    section: SettingsSection,
) -> Option<Vec<SettingsListRow>> {
    match section {
        SettingsSection::Theme => Some(appearance_rows(app, settings)),
        SettingsSection::Layout => Some(layout_rows(app, settings)),
        SettingsSection::Sound => Some(notification_rows(app, settings)),
        SettingsSection::Toast => Some(toast_rows(app, settings)),
        SettingsSection::PaneLabels => Some(behavior_rows(app, settings)),
        SettingsSection::Commands => Some(command_rows(app, settings)),
        SettingsSection::Experiments => Some(experiment_rows(app, settings)),
        SettingsSection::Agents => Some(agent_profile_rows(app, settings)),
        SettingsSection::Integrations => Some(integration_rows(app, settings)),
        SettingsSection::Connections => Some(connection_rows(app, settings)),
        SettingsSection::GroupGeneral => Some(group_general_rows(app, settings)),
        SettingsSection::GroupDefaults => Some(group_defaults_rows(app, settings)),
        SettingsSection::GroupProfiles => Some(group_profile_rows(app, settings)),
        SettingsSection::GroupGithub => Some(group_github_rows(app, settings)),
        SettingsSection::WorkspaceGeneral => Some(workspace_general_rows(app, settings)),
        SettingsSection::WorkspaceGithub => Some(workspace_github_rows(app, settings)),
        SettingsSection::About => Some(about_rows()),
    }
}

/// Builds settings rows for the requesting client's selected section.
///
/// Shared domain values remain derived from `AppState`; client-local drafts,
/// selection, and scrolling are consumed from the client's settings state.
pub(crate) fn rows_for_section_for_view(
    app: &AppState,
    view: &ClientViewState,
) -> Option<Vec<SettingsListRow>> {
    rows_for_section_with_settings(app, &view.settings, view.settings.section)
}

pub(crate) fn selected_visual_row(rows: &[SettingsListRow], selected: usize) -> Option<usize> {
    let mut visual_row = 0;
    for entry in rows {
        match entry {
            SettingsListRow::Header(_) | SettingsListRow::Caption(_) | SettingsListRow::Spacer => {
                visual_row += 1;
            }
            SettingsListRow::GroupIconPicker => {
                visual_row += group_icon_picker_visual_rows();
            }
            SettingsListRow::Toggle { index, .. } | SettingsListRow::Value { index, .. } => {
                if *index == selected {
                    return Some(visual_row);
                }
                visual_row += 2;
            }
            SettingsListRow::TextInput { index, .. } => {
                if *index == selected {
                    return Some(visual_row + 1);
                }
                visual_row += 2;
            }
            SettingsListRow::Choice { index, .. }
            | SettingsListRow::Action { index, .. }
            | SettingsListRow::Status { index, .. } => {
                if *index == selected {
                    return Some(visual_row);
                }
                visual_row += 1;
            }
            SettingsListRow::Profile { index, .. } => {
                if *index == selected {
                    return Some(visual_row);
                }
                visual_row += 1;
            }
        }
    }
    None
}

pub(crate) fn visual_row_count(rows: &[SettingsListRow]) -> usize {
    rows.iter()
        .map(|row| match row {
            SettingsListRow::Header(_)
            | SettingsListRow::Caption(_)
            | SettingsListRow::Spacer
            | SettingsListRow::Choice { .. }
            | SettingsListRow::Action { .. }
            | SettingsListRow::Status { .. }
            | SettingsListRow::Profile { .. } => 1,
            SettingsListRow::Toggle { .. }
            | SettingsListRow::Value { .. }
            | SettingsListRow::TextInput { .. } => 2,
            SettingsListRow::GroupIconPicker => group_icon_picker_visual_rows(),
        })
        .sum()
}

fn group_icon_picker_visual_rows() -> usize {
    crate::app::state::GROUP_ICONS.len().div_ceil(5)
}

fn option_index(row: &SettingsListRow) -> Option<usize> {
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

pub(crate) fn option_count(rows: &[SettingsListRow]) -> usize {
    rows.iter()
        .filter(|row| option_index(row).is_some())
        .count()
}

pub(crate) fn next_option_index(rows: &[SettingsListRow], selected: usize) -> Option<usize> {
    let first = rows.iter().find_map(option_index)?;
    let mut found_selected = false;
    for index in rows.iter().filter_map(option_index) {
        if found_selected {
            return Some(index);
        }
        found_selected = index == selected;
    }
    Some(first)
}

pub(crate) fn previous_option_index(rows: &[SettingsListRow], selected: usize) -> Option<usize> {
    let mut previous = None;
    let mut before_selected = None;
    let mut found_selected = false;
    for index in rows.iter().filter_map(option_index) {
        if index == selected {
            before_selected = previous;
            found_selected = true;
        }
        previous = Some(index);
    }
    if found_selected {
        before_selected.or(previous)
    } else {
        previous
    }
}

fn theme_settings_choices_group_accent(
    app: &AppState,
    settings: &SettingsState,
) -> Option<TerminalAccent> {
    if let Some(pending) = settings.pending_group_accent_choice {
        return pending;
    }

    settings
        .group_settings_target
        .and_then(|group_idx| app.groups.get(group_idx))
        .and_then(|group| group.accent)
}

fn theme_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    if settings.group_settings_target.is_some() {
        let active = theme_settings_choices_group_accent(app, settings);
        let mut rows = Vec::new();
        rows.push(SettingsListRow::Header("Accent"));
        rows.push(choice(0, "Inherit", active.is_none()));
        for (offset, accent) in TerminalAccent::ALL.iter().copied().enumerate() {
            rows.push(choice(
                offset + 1,
                accent.display_name(),
                active == Some(accent),
            ));
        }
        return rows;
    }

    let mode = settings.pending_theme_mode.unwrap_or(app.global_theme_mode);
    let pending_light_theme = settings
        .pending_light_theme_name
        .as_deref()
        .unwrap_or(&app.global_light_theme_name);
    let pending_dark_theme = settings
        .pending_dark_theme_name
        .as_deref()
        .unwrap_or(&app.global_dark_theme_name);
    let system_source = mode == ThemeMode::System
        && normalize_theme_name(pending_light_theme) == "system"
        && normalize_theme_name(pending_dark_theme) == "system";
    let show_terminal_accent = system_source;

    let mut rows = Vec::new();
    rows.push(SettingsListRow::Header("Colors"));
    rows.push(choice(0, "Terminal", system_source));
    rows.push(choice(1, "Palettes", !system_source));

    if show_terminal_accent {
        rows.push(SettingsListRow::Spacer);
        rows.push(SettingsListRow::Header("Light Accent"));
        let pending_light_accent = settings
            .pending_terminal_light_accent
            .unwrap_or(app.global_terminal_light_accent);
        for (offset, accent) in TerminalAccent::ALL.iter().copied().enumerate() {
            rows.push(choice(
                2 + offset,
                accent.display_name(),
                pending_light_accent == accent,
            ));
        }

        rows.push(SettingsListRow::Spacer);
        rows.push(SettingsListRow::Header("Dark Accent"));
        let pending_dark_accent = settings
            .pending_terminal_dark_accent
            .unwrap_or(app.global_terminal_dark_accent);
        let dark_base = 2 + TerminalAccent::ALL.len();
        for (offset, accent) in TerminalAccent::ALL.iter().copied().enumerate() {
            rows.push(choice(
                dark_base + offset,
                accent.display_name(),
                pending_dark_accent == accent,
            ));
        }
    }

    if !system_source {
        rows.push(SettingsListRow::Spacer);
        rows.push(SettingsListRow::Header("Appearance"));
        for (offset, candidate) in ThemeMode::ALL.iter().copied().enumerate() {
            rows.push(choice(
                2 + offset,
                theme_mode_display_name(candidate),
                mode == candidate,
            ));
        }

        let theme_base = 2 + ThemeMode::ALL.len();
        match mode {
            ThemeMode::System => {
                rows.push(SettingsListRow::Spacer);
                rows.push(SettingsListRow::Header("Light Appearance"));
                let mut option_idx = theme_base;
                for name in theme_names_for_appearance(ThemeAppearance::Light)
                    .iter()
                    .copied()
                {
                    rows.push(choice(
                        option_idx,
                        theme_display_name(name),
                        pending_light_theme == name,
                    ));
                    option_idx += 1;
                }

                rows.push(SettingsListRow::Spacer);
                rows.push(SettingsListRow::Header("Dark Appearance"));
                for name in theme_names_for_appearance(ThemeAppearance::Dark)
                    .iter()
                    .copied()
                {
                    rows.push(choice(
                        option_idx,
                        theme_display_name(name),
                        pending_dark_theme == name,
                    ));
                    option_idx += 1;
                }
            }
            ThemeMode::Light => {
                rows.push(SettingsListRow::Spacer);
                rows.push(SettingsListRow::Header("Light Appearance"));
                for (offset, name) in theme_names_for_appearance(ThemeAppearance::Light)
                    .iter()
                    .copied()
                    .enumerate()
                {
                    rows.push(choice(
                        theme_base + offset,
                        theme_display_name(name),
                        pending_light_theme == name,
                    ));
                }
            }
            ThemeMode::Dark => {
                rows.push(SettingsListRow::Spacer);
                rows.push(SettingsListRow::Header("Dark Appearance"));
                for (offset, name) in theme_names_for_appearance(ThemeAppearance::Dark)
                    .iter()
                    .copied()
                    .enumerate()
                {
                    rows.push(choice(
                        theme_base + offset,
                        theme_display_name(name),
                        pending_dark_theme == name,
                    ));
                }
            }
        }
    }

    rows
}

fn execution_host_label(
    app: &AppState,
    host_id: Option<&crate::execution_host::ExecutionHostId>,
) -> String {
    let target = host_id.map_or(
        crate::app::host_label::HostLabelTarget::Coordinator,
        crate::app::host_label::HostLabelTarget::ExecutionHost,
    );
    app.host_label(target).to_string()
}

fn group_general_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    let group = settings
        .group_settings_target
        .and_then(|group_idx| app.groups.get(group_idx));
    let group_name = settings
        .pending_group_name
        .clone()
        .or_else(|| group.map(|group| group.name.clone()))
        .unwrap_or_else(|| "Group".to_string());
    let icon = settings
        .pending_group_icon
        .clone()
        .or_else(|| group.map(|group| group.icon.clone()))
        .unwrap_or_else(|| crate::app::state::DEFAULT_GROUP_ICON.to_string());

    let mut rows = vec![
        SettingsListRow::TextInput {
            index: GROUP_GENERAL_NAME,
            title: "Name".into(),
            value: group_name.into(),
        },
        SettingsListRow::Spacer,
        SettingsListRow::Status {
            index: GROUP_GENERAL_ICON,
            label: "Icon".into(),
            status: format!("‹ {icon} ›").into(),
            tone: SettingsMarkerTone::Accent,
        },
    ];
    if settings.group_icon_picker_open {
        rows.push(SettingsListRow::GroupIconPicker);
    }
    rows.extend([
        SettingsListRow::Spacer,
        SettingsListRow::Header("Danger Zone"),
        SettingsListRow::Action {
            index: GROUP_GENERAL_DELETE,
            icon: "×".into(),
            label: "Delete Group".into(),
            tone: SettingsMarkerTone::Danger,
        },
    ]);
    rows
}

fn group_defaults_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    let group = settings
        .group_settings_target
        .and_then(|group_idx| app.groups.get(group_idx));
    let default_location = group.and_then(|group| group.default_location.as_ref());
    let default_directory = settings
        .pending_group_default_directory
        .clone()
        .or_else(|| default_location.map(|location| location.path.as_path().display().to_string()))
        .unwrap_or_default();
    let host_id = settings
        .pending_group_default_execution_host_id
        .as_ref()
        .or_else(|| default_location.map(|location| &location.execution_host_id));
    let host_label = execution_host_label(app, host_id);

    vec![
        SettingsListRow::Status {
            index: GROUP_DEFAULTS_HOST,
            label: "Default Location for New Spaces".into(),
            status: format!("‹ {host_label} ›").into(),
            tone: SettingsMarkerTone::Accent,
        },
        SettingsListRow::Spacer,
        SettingsListRow::TextInput {
            index: GROUP_DEFAULTS_DIRECTORY,
            title: "Directory".into(),
            value: default_directory.into(),
        },
    ]
}

fn group_github_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    let group = settings
        .group_settings_target
        .and_then(|group_idx| app.groups.get(group_idx));
    let github_organization = settings
        .pending_group_github_organization
        .clone()
        .or_else(|| {
            group
                .and_then(|group| group.github_organization.as_ref())
                .map(|organization| organization.as_str().to_string())
        })
        .unwrap_or_default();

    vec![SettingsListRow::TextInput {
        index: GROUP_GITHUB_ORGANIZATION,
        title: "GitHub Organization".into(),
        value: github_organization.into(),
    }]
}

fn workspace_general_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    let workspace = settings
        .workspace_settings_target
        .and_then(|ws_idx| app.workspaces.get(ws_idx));
    let name = settings
        .pending_workspace_name
        .clone()
        .or_else(|| workspace.map(|workspace| workspace.display_name()))
        .unwrap_or_else(|| "Space".to_string());
    let default_cwd = settings
        .pending_workspace_default_cwd
        .clone()
        .or_else(|| {
            workspace.map(|workspace| {
                workspace
                    .default_location
                    .path
                    .as_path()
                    .display()
                    .to_string()
            })
        })
        .unwrap_or_default();
    let host_id = settings
        .pending_workspace_default_execution_host_id
        .as_ref()
        .or_else(|| workspace.map(|workspace| &workspace.default_location.execution_host_id));
    let host_label = execution_host_label(app, host_id);

    vec![
        SettingsListRow::TextInput {
            index: WORKSPACE_GENERAL_NAME,
            title: "Name".into(),
            value: name.into(),
        },
        SettingsListRow::Spacer,
        SettingsListRow::Status {
            index: WORKSPACE_GENERAL_HOST,
            label: "Location for This Space".into(),
            status: format!("‹ {host_label} ›").into(),
            tone: SettingsMarkerTone::Accent,
        },
        SettingsListRow::Spacer,
        SettingsListRow::TextInput {
            index: WORKSPACE_GENERAL_DIRECTORY,
            title: "Directory".into(),
            value: default_cwd.into(),
        },
    ]
}

fn workspace_github_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    let workspace = settings
        .workspace_settings_target
        .and_then(|ws_idx| app.workspaces.get(ws_idx));
    let github_scope = settings
        .pending_workspace_github_scope
        .clone()
        .or_else(|| workspace.map(|workspace| workspace.github_scope.clone()))
        .unwrap_or_default();
    let github_repositories = settings
        .pending_workspace_github_repositories
        .clone()
        .or_else(|| match &github_scope {
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
        .unwrap_or_default();
    let github_organization = workspace
        .and_then(|workspace| {
            app.groups
                .iter()
                .find(|group| group.id == workspace.group_id)
        })
        .and_then(|group| group.github_organization.as_ref())
        .map_or("not configured", |organization| organization.as_str());

    vec![
        SettingsListRow::Choice {
            index: WORKSPACE_GITHUB_AUTOMATIC,
            label: "Automatic".into(),
            checked: matches!(
                &github_scope,
                crate::github::GithubRepositoryScope::Automatic
            ),
        },
        SettingsListRow::Choice {
            index: WORKSPACE_GITHUB_SELECTED,
            label: "Selected repositories".into(),
            checked: matches!(
                &github_scope,
                crate::github::GithubRepositoryScope::Selected(_)
            ),
        },
        SettingsListRow::Choice {
            index: WORKSPACE_GITHUB_GROUP,
            label: format!("Group organization ({github_organization})").into(),
            checked: matches!(
                &github_scope,
                crate::github::GithubRepositoryScope::GroupOrganization
            ),
        },
        SettingsListRow::Spacer,
        SettingsListRow::TextInput {
            index: WORKSPACE_GITHUB_REPOSITORIES,
            title: "Repositories (owner/repository)".into(),
            value: github_repositories.into(),
        },
    ]
}

fn agent_profile_editor_open(settings: &SettingsState) -> bool {
    settings.pending_agent_profile_id.is_some()
        || settings.pending_agent_profile_name.is_some()
        || settings.pending_agent_profile_command.is_some()
}

fn agent_profile_detail(profile: &crate::agent_profiles::AgentProfile) -> String {
    if profile.is_system() {
        return String::new();
    }

    if profile.kind.is_supported() {
        if profile.command == profile.kind.system_command() {
            profile.kind.display_name().to_string()
        } else {
            format!("{} · {}", profile.kind.display_name(), profile.command)
        }
    } else {
        "Custom · Launch-Only".to_string()
    }
}

fn agent_profile_badge(
    profile: &crate::agent_profiles::AgentProfile,
    is_favorite: bool,
    is_default: bool,
    integration_badge: Option<&str>,
) -> Option<Cow<'static, str>> {
    if let Some(badge) = integration_badge {
        Some(badge.to_string().into())
    } else if !profile.available() {
        Some("Unavailable".into())
    } else if is_default {
        Some("Default".into())
    } else if is_favorite {
        Some("Favorite".into())
    } else if !profile.kind.is_supported() {
        Some("Launch-Only".into())
    } else {
        None
    }
}

fn agent_profile_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    if !agent_profile_editor_open(settings) {
        return agent_profile_browse_rows(app);
    }

    let mut rows = Vec::new();
    let name = settings
        .pending_agent_profile_name
        .clone()
        .unwrap_or_default();
    let command = settings
        .pending_agent_profile_command
        .clone()
        .unwrap_or_default();
    let mut kind = settings
        .pending_agent_profile_kind
        .unwrap_or_else(|| app.default_agent_profile_kind_choice());
    if !app.agent_profile_kind_available(kind) {
        kind = crate::agent_profiles::AgentKind::Custom;
    }
    let editing = settings.pending_agent_profile_id.is_some();

    rows.push(SettingsListRow::Caption("Label shown in menus".into()));
    rows.push(SettingsListRow::TextInput {
        index: 0,
        title: "Name".into(),
        value: name.into(),
    });
    rows.push(SettingsListRow::Spacer);
    rows.push(SettingsListRow::Header("Agent Type"));
    rows.push(SettingsListRow::Caption(
        "Choose an installed integration, or a custom command for launch-only use".into(),
    ));
    let kind_choices = app.agent_profile_kind_choices().collect::<Vec<_>>();
    for (offset, agent_kind) in kind_choices.iter().copied().enumerate() {
        rows.push(SettingsListRow::Choice {
            index: 1 + offset,
            label: agent_kind.display_name().into(),
            checked: agent_kind == kind,
        });
    }
    if !kind.is_supported() {
        rows.push(SettingsListRow::Caption(
            "Custom commands are launch-only. Status, restore, and integrations are unavailable."
                .into(),
        ));
    }
    rows.push(SettingsListRow::Spacer);
    rows.push(SettingsListRow::Caption("Shell command to run".into()));
    let command_index = 1 + kind_choices.len();
    rows.push(SettingsListRow::TextInput {
        index: command_index,
        title: "Command".into(),
        value: command.into(),
    });
    rows.push(SettingsListRow::Spacer);
    let enabled = settings.pending_agent_profile_enabled.unwrap_or(true);
    let enabled_index = command_index + 1;
    rows.push(option(
        enabled_index,
        "Enabled",
        "Include this profile in agent launch menus",
        enabled,
    ));
    rows.push(SettingsListRow::Spacer);
    let save_index = enabled_index + 1;
    rows.push(SettingsListRow::Action {
        index: save_index,
        icon: "".into(),
        label: if editing {
            "Save Profile".into()
        } else {
            "Create Profile".into()
        },
        tone: SettingsMarkerTone::Accent,
    });
    rows.push(SettingsListRow::Action {
        index: save_index + 1,
        icon: "×".into(),
        label: if editing {
            "Discard Changes".into()
        } else {
            "Cancel".into()
        },
        tone: SettingsMarkerTone::Disabled,
    });
    if editing {
        rows.push(SettingsListRow::Spacer);
        rows.push(SettingsListRow::Header("Danger Zone"));
        rows.push(SettingsListRow::Caption(
            "Remove this profile from agent launch menus".into(),
        ));
        rows.push(SettingsListRow::Action {
            index: save_index + 2,
            icon: "×".into(),
            label: "Delete Profile".into(),
            tone: SettingsMarkerTone::Danger,
        });
    }
    rows
}

fn agent_profile_browse_rows(app: &AppState) -> Vec<SettingsListRow> {
    let mut rows = vec![
        SettingsListRow::Action {
            index: 0,
            icon: "".into(),
            label: "New Agent Profile".into(),
            tone: SettingsMarkerTone::Accent,
        },
        SettingsListRow::Spacer,
        SettingsListRow::Header("Saved Profiles"),
    ];
    let custom_profiles = app
        .agent_profiles
        .profiles()
        .iter()
        .filter(|profile| !profile.is_system());
    let mut has_custom_profiles = false;
    for (index, profile) in (1..).zip(custom_profiles) {
        has_custom_profiles = true;
        let tone = if profile.available() {
            SettingsMarkerTone::Good
        } else {
            SettingsMarkerTone::Disabled
        };
        rows.push(agent_profile_row(profile, index, false, false, tone));
    }
    if !has_custom_profiles {
        rows.push(SettingsListRow::Caption(
            "None yet — create one to add a launch command".into(),
        ));
    }
    rows
}

fn agent_profile_row(
    profile: &crate::agent_profiles::AgentProfile,
    index: usize,
    is_favorite: bool,
    is_default: bool,
    tone: SettingsMarkerTone,
) -> SettingsListRow {
    let integration_badge = crate::integration::agent_profile_integration_badge(profile);
    let tone = if integration_badge.is_some() {
        SettingsMarkerTone::Warning
    } else {
        tone
    };
    SettingsListRow::Profile {
        index,
        name: profile.name.clone().into(),
        detail: agent_profile_detail(profile).into(),
        badge: agent_profile_badge(profile, is_favorite, is_default, integration_badge),
        tone,
    }
}

fn profile_visible_in_group_settings(
    app: &AppState,
    profile: &crate::agent_profiles::AgentProfile,
) -> bool {
    profile.available()
        && (app.agent_profile_launchable(profile)
            || crate::integration::agent_profile_integration_warning(profile).is_some())
}

fn group_profile_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    let group = settings
        .group_settings_target
        .and_then(|idx| app.groups.get(idx));
    let favorites = group
        .map(|group| group.favorite_agent_profile_ids.as_slice())
        .unwrap_or(&[]);
    let default_profile_id = group.and_then(|group| group.default_agent_profile_id.as_deref());
    let (favorite, available) = app.agent_profiles.group_sections(favorites);
    let favorite: Vec<_> = favorite
        .into_iter()
        .filter(|profile| profile_visible_in_group_settings(app, profile))
        .collect();
    let available: Vec<_> = available
        .into_iter()
        .filter(|profile| profile_visible_in_group_settings(app, profile))
        .collect();
    let mut rows = Vec::new();
    let mut index = 0;
    rows.push(SettingsListRow::Header("Favorites"));
    if favorite.is_empty() {
        rows.push(SettingsListRow::Caption("No Favorites".into()));
    } else {
        for profile in favorite {
            let is_default = default_profile_id == Some(profile.id.as_str());
            rows.push(agent_profile_row(
                profile,
                index,
                false,
                is_default,
                SettingsMarkerTone::Accent,
            ));
            index += 1;
        }
    }
    rows.push(SettingsListRow::Spacer);
    rows.push(SettingsListRow::Header("Available"));
    for profile in available {
        let is_default = default_profile_id == Some(profile.id.as_str());
        rows.push(agent_profile_row(
            profile,
            index,
            false,
            is_default,
            if is_default {
                SettingsMarkerTone::Accent
            } else {
                SettingsMarkerTone::Disabled
            },
        ));
        index += 1;
    }
    rows
}

fn appearance_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    if settings.group_settings_target.is_some() {
        return theme_rows(app, settings);
    }

    let mut rows = theme_rows(app, settings);
    let layout_base = option_count(&rows);
    rows.push(SettingsListRow::Spacer);
    rows.extend(layout_rows_with_base(app, settings, layout_base));
    rows.push(SettingsListRow::Spacer);
    rows.extend(setting_group(
        "Panes",
        [
            option(
                layout_base + 7,
                "Pane Borders",
                "Draw borders around split panes",
                settings.pending_pane_borders.unwrap_or(app.pane_borders),
            ),
            option(
                layout_base + 8,
                "Pane Scrollbars",
                "Draw scrollbars beside terminal panes",
                settings
                    .pending_pane_scrollbars
                    .unwrap_or(app.pane_scrollbars),
            ),
            option(
                layout_base + 9,
                "Pane Gaps",
                "Keep split panes visually separated",
                settings.pending_pane_gaps.unwrap_or(app.pane_gaps),
            ),
            option(
                layout_base + 10,
                "Hide Single-Tab Bar",
                "Hide the tab bar when a workspace has one tab",
                settings
                    .pending_hide_tab_bar_when_single_tab
                    .unwrap_or(app.hide_tab_bar_when_single_tab),
            ),
            value_option(
                layout_base + 11,
                "Pane Border Agent Info",
                "Agent metadata shown in split pane borders",
                settings
                    .pending_pane_border_agent_info
                    .unwrap_or_else(|| app.pane_border_agent_info())
                    .label(),
            ),
        ],
    ));
    rows.push(SettingsListRow::Spacer);
    rows.extend(setting_group(
        "Agent Status",
        [value_option(
            layout_base + 12,
            "Status Indicators",
            "How agent status is shown in lists",
            settings
                .pending_status_indicators
                .unwrap_or_else(|| app.status_indicators())
                .label(),
        )],
    ));
    rows.push(SettingsListRow::Spacer);
    rows.extend(setting_group(
        "Window",
        [SettingsListRow::TextInput {
            index: layout_base + 13,
            title: "Window Title".into(),
            value: settings
                .pending_window_title
                .clone()
                .unwrap_or_else(|| app.window_title_template.clone())
                .into(),
        }],
    ));
    rows
}

fn layout_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    layout_rows_with_base(app, settings, 0)
}

fn layout_rows_with_base(
    app: &AppState,
    settings: &SettingsState,
    base: usize,
) -> Vec<SettingsListRow> {
    let width = settings
        .pending_sidebar_width
        .unwrap_or(app.default_sidebar_width);
    let min = settings
        .pending_sidebar_min_width
        .unwrap_or(app.sidebar_min_width);
    let max = settings
        .pending_sidebar_max_width
        .unwrap_or(app.sidebar_max_width);
    let arrangement = settings
        .pending_sidebar_arrangement
        .unwrap_or(app.sidebar_arrangement);
    let context_bar_visibility = settings
        .pending_context_bar_visibility
        .unwrap_or(app.context_bar_visibility);
    let initial_state = settings
        .pending_sidebar_initial_state
        .unwrap_or(app.sidebar_config.initial_state);
    let initial_agent_scope = settings
        .pending_sidebar_initial_agent_scope
        .unwrap_or(app.sidebar_config.initial_agent_scope);
    setting_group(
        "Sidebar",
        [
            value_option(
                base,
                "Default Sidebar Width",
                "Preferred desktop sidebar width",
                format!("{width} cols"),
            ),
            value_option(
                base + 1,
                "Minimum Sidebar Width",
                "Smallest allowed desktop sidebar width",
                format!("{min} cols"),
            ),
            value_option(
                base + 2,
                "Maximum Sidebar Width",
                "Largest allowed desktop sidebar width",
                format!("{max} cols"),
            ),
            value_option(
                base + 3,
                "Sidebar Arrangement",
                "Where spaces and agents live on desktop",
                arrangement.label(),
            ),
            value_option(
                base + 4,
                "Context Bar",
                "Always visible or hidden",
                context_bar_visibility.label(),
            ),
            value_option(
                base + 5,
                "Initial Sidebar State",
                "Expanded or collapsed when a new client connects",
                initial_state.label(),
            ),
            value_option(
                base + 6,
                "Initial Agent Scope",
                "Agents shown when a new client connects",
                initial_agent_scope.label(),
            ),
        ],
    )
}

fn behavior_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    let cwd_label = new_terminal_cwd_label(
        &settings
            .pending_new_terminal_cwd
            .clone()
            .unwrap_or_else(|| app.new_terminal_cwd.clone()),
    );
    let scroll_label = format!(
        "{} lines per wheel notch",
        settings
            .pending_mouse_scroll_lines
            .unwrap_or(app.mouse_scroll_lines)
    );

    let mut rows = setting_group(
        "General",
        [
            option(
                BehaviorRowId::ConfirmClose.selection_index(),
                "Confirm Before Closing Workspaces",
                "Ask before closing a workspace",
                settings
                    .pending_confirm_close
                    .unwrap_or_else(|| app.confirm_close_enabled()),
            ),
            option(
                BehaviorRowId::NameNewTabs.selection_index(),
                "Name New Tabs",
                "Ask for a tab name before creating a new tab",
                settings
                    .pending_prompt_new_tab_name
                    .unwrap_or_else(|| app.prompt_new_tab_name_enabled()),
            ),
            option(
                BehaviorRowId::NameNewWorkspaces.selection_index(),
                "Name New Workspaces",
                "Ask for a workspace name before creating a new workspace",
                settings
                    .pending_prompt_new_workspace_name
                    .unwrap_or(app.prompt_new_workspace_name),
            ),
            option(
                BehaviorRowId::ShowCounters.selection_index(),
                "Show Counters",
                "Show right-aligned topology and section counts",
                settings.pending_show_counters.unwrap_or(app.show_counters),
            ),
        ],
    );
    rows.push(SettingsListRow::Spacer);
    rows.extend(setting_group(
        "Selection",
        [
            option(
                BehaviorRowId::CopyOnSelect.selection_index(),
                "Copy on Select",
                "Copy drag selections when the mouse is released",
                settings
                    .pending_copy_on_select
                    .unwrap_or(app.copy_on_select),
            ),
            value_option(
                BehaviorRowId::RightClickPassthrough.selection_index(),
                "Right-click Passthrough",
                "Modifier that forwards right-clicks into pane apps",
                settings
                    .pending_right_click_passthrough_modifier
                    .unwrap_or_else(|| {
                        crate::config::RightClickPassthroughModifierConfig::from_modifiers(
                            app.right_click_passthrough_modifiers,
                        )
                    })
                    .label(),
            ),
        ],
    ));
    rows.push(SettingsListRow::Spacer);
    rows.extend(setting_group(
        "Terminal",
        [
            SettingsListRow::TextInput {
                index: BehaviorRowId::DefaultShell.selection_index(),
                title: "Default Shell".into(),
                value: settings
                    .pending_default_shell
                    .clone()
                    .unwrap_or_else(|| app.default_shell.clone())
                    .into(),
            },
            value_option(
                BehaviorRowId::ShellMode.selection_index(),
                "Shell Startup Mode",
                "How new interactive panes start their shell",
                shell_mode_label(settings.pending_shell_mode.unwrap_or(app.shell_mode)),
            ),
            value_option(
                BehaviorRowId::NewTerminalCwd.selection_index(),
                "New Terminal CWD",
                "Directory used by newly created terminal tabs",
                cwd_label,
            ),
            value_option(
                BehaviorRowId::MouseWheelSpeed.selection_index(),
                "Mouse Wheel Speed",
                "Terminal scroll amount per wheel notch",
                scroll_label,
            ),
        ],
    ));
    rows.push(SettingsListRow::Spacer);
    rows.extend(setting_group(
        "Sessions",
        [option(
            BehaviorRowId::ResumeAgents.selection_index(),
            "Resume Agent Sessions",
            "Resume supported agents after restoring a session",
            settings
                .pending_resume_agents_on_restore
                .unwrap_or(app.resume_agents_on_restore),
        )],
    ));
    rows
}

fn command_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    let mut rows = vec![SettingsListRow::Header("Project Commands")];
    for (offset, field) in CommandField::ALL.iter().copied().enumerate() {
        if offset > 0 {
            rows.push(SettingsListRow::Spacer);
        }
        let value = pending_command_value(app, settings, field);
        let title = if value.trim().is_empty() {
            format!("{} · Disabled", field.title())
        } else {
            field.title().to_string()
        };
        rows.push(SettingsListRow::TextInput {
            index: CommandRowId::Field(field).selection_index(),
            title: title.into(),
            value: value.into(),
        });
        rows.push(SettingsListRow::Action {
            index: CommandRowId::Action(CommandAction::Reset(field)).selection_index(),
            icon: "↻".into(),
            label: field.reset_label().into(),
            tone: SettingsMarkerTone::Accent,
        });
    }
    rows.push(SettingsListRow::Spacer);
    rows.push(SettingsListRow::Caption(
        "Restore all project commands to their built-in defaults.".into(),
    ));
    rows.push(SettingsListRow::Action {
        index: CommandRowId::Action(CommandAction::ResetAll).selection_index(),
        icon: "↻".into(),
        label: "Reset All Commands".into(),
        tone: SettingsMarkerTone::Accent,
    });
    rows
}

fn pending_command_value(app: &AppState, settings: &SettingsState, field: CommandField) -> String {
    match field {
        CommandField::Browser => settings
            .pending_browser_command
            .clone()
            .unwrap_or_else(|| app.browser_command.clone()),
        CommandField::Review => settings
            .pending_review_command
            .clone()
            .unwrap_or_else(|| app.review_command.clone()),
        CommandField::Editor => settings
            .pending_editor_command
            .clone()
            .unwrap_or_else(|| app.editor_command.clone()),
    }
}

/// Stable row identity for the Commands settings surface.
///
/// Input and render share these typed ids so keyboard/mouse dispatch never
/// depends on magic contiguous integers or row-order accidents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandField {
    Browser,
    Review,
    Editor,
}

impl CommandField {
    pub(crate) const ALL: [Self; 3] = [Self::Browser, Self::Review, Self::Editor];

    const fn title(self) -> &'static str {
        match self {
            Self::Browser => "Browser · Terminal Browser",
            Self::Review => "Review · Review UI",
            Self::Editor => "Editor · Project Editor",
        }
    }

    const fn reset_label(self) -> &'static str {
        match self {
            Self::Browser => "Reset to terminal-browser",
            Self::Review => "Reset to hunk diff --watch",
            Self::Editor => "Reset to fresh .",
        }
    }

    pub(crate) const fn default_value(self) -> &'static str {
        match self {
            Self::Browser => CommandsConfig::DEFAULT_BROWSER,
            Self::Review => CommandsConfig::DEFAULT_REVIEW,
            Self::Editor => CommandsConfig::DEFAULT_EDITOR,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandAction {
    Reset(CommandField),
    ResetAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommandRowId {
    Field(CommandField),
    Action(CommandAction),
}

impl CommandRowId {
    /// Dense index used by the existing list selection model.
    pub(crate) const fn selection_index(self) -> usize {
        match self {
            Self::Field(CommandField::Browser) => 0,
            Self::Field(CommandField::Review) => 1,
            Self::Field(CommandField::Editor) => 2,
            Self::Action(CommandAction::Reset(CommandField::Browser)) => 3,
            Self::Action(CommandAction::Reset(CommandField::Review)) => 4,
            Self::Action(CommandAction::Reset(CommandField::Editor)) => 5,
            Self::Action(CommandAction::ResetAll) => 6,
        }
    }

    pub(crate) fn from_selection_index(index: usize) -> Option<Self> {
        Some(match index {
            0 => Self::Field(CommandField::Browser),
            1 => Self::Field(CommandField::Review),
            2 => Self::Field(CommandField::Editor),
            3 => Self::Action(CommandAction::Reset(CommandField::Browser)),
            4 => Self::Action(CommandAction::Reset(CommandField::Review)),
            5 => Self::Action(CommandAction::Reset(CommandField::Editor)),
            6 => Self::Action(CommandAction::ResetAll),
            _ => return None,
        })
    }
}

#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BehaviorRowId {
    ConfirmClose,
    NameNewTabs,
    NameNewWorkspaces,
    ShowCounters,
    CopyOnSelect,
    RightClickPassthrough,
    DefaultShell,
    ShellMode,
    NewTerminalCwd,
    MouseWheelSpeed,
    ResumeAgents,
}

impl BehaviorRowId {
    const ALL: [Self; 11] = [
        Self::ConfirmClose,
        Self::NameNewTabs,
        Self::NameNewWorkspaces,
        Self::ShowCounters,
        Self::CopyOnSelect,
        Self::RightClickPassthrough,
        Self::DefaultShell,
        Self::ShellMode,
        Self::NewTerminalCwd,
        Self::MouseWheelSpeed,
        Self::ResumeAgents,
    ];

    pub(crate) const fn selection_index(self) -> usize {
        self as usize
    }

    pub(crate) fn from_selection_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }
}

#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotificationRowId {
    SoundAlerts,
    ToastDelivery,
    ToastDelay,
    ToastGardnPosition,
    ClipboardEnabled,
    ClipboardPosition,
}

impl NotificationRowId {
    const ALL: [Self; 6] = [
        Self::SoundAlerts,
        Self::ToastDelivery,
        Self::ToastDelay,
        Self::ToastGardnPosition,
        Self::ClipboardEnabled,
        Self::ClipboardPosition,
    ];

    pub(crate) const fn selection_index(self) -> usize {
        self as usize
    }

    pub(crate) fn from_selection_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }
}

#[repr(usize)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdvancedRowId {
    SwitchAscii,
    KittyGraphics,
    HeadlessCols,
    HeadlessRows,
    VersionCheck,
    ManifestCheck,
}

impl AdvancedRowId {
    const ALL: [Self; 6] = [
        Self::SwitchAscii,
        Self::KittyGraphics,
        Self::HeadlessCols,
        Self::HeadlessRows,
        Self::VersionCheck,
        Self::ManifestCheck,
    ];

    pub(crate) const fn selection_index(self) -> usize {
        self as usize
    }

    pub(crate) fn from_selection_index(index: usize) -> Option<Self> {
        Self::ALL.get(index).copied()
    }
}

fn experiment_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    let mut rows = setting_group(
        "Input",
        [option(
            AdvancedRowId::SwitchAscii.selection_index(),
            "Switch to ASCII Input Source in Prefix (macOS/Windows)",
            "Temporarily use an ASCII-capable layout for prefix commands",
            settings
                .pending_switch_ascii_input_source_in_prefix
                .unwrap_or_else(|| app.switch_ascii_input_source_in_prefix_enabled()),
        )],
    );
    rows.push(SettingsListRow::Spacer);
    rows.extend(setting_group(
        "Terminal Graphics",
        [option(
            AdvancedRowId::KittyGraphics.selection_index(),
            "Kitty Graphics",
            "Render inline images in Kitty-compatible terminals. Reconnect Gardn to apply.",
            app.kitty_graphics_enabled,
        )],
    ));
    rows.push(SettingsListRow::Spacer);
    rows.extend(setting_group(
        "Server",
        [
            SettingsListRow::TextInput {
                index: AdvancedRowId::HeadlessCols.selection_index(),
                title: "Headless Terminal Columns".into(),
                value: settings
                    .pending_headless_cols
                    .clone()
                    .unwrap_or_else(|| app.headless_size.0.to_string())
                    .into(),
            },
            SettingsListRow::TextInput {
                index: AdvancedRowId::HeadlessRows.selection_index(),
                title: "Headless Terminal Rows".into(),
                value: settings
                    .pending_headless_rows
                    .clone()
                    .unwrap_or_else(|| app.headless_size.1.to_string())
                    .into(),
            },
        ],
    ));
    rows.push(SettingsListRow::Spacer);
    rows.extend(setting_group(
        "Updates",
        [
            option(
                AdvancedRowId::VersionCheck.selection_index(),
                "Version Check",
                "Check GitHub for a newer Gardn release",
                settings
                    .pending_version_check
                    .unwrap_or(app.update_version_check),
            ),
            option(
                AdvancedRowId::ManifestCheck.selection_index(),
                "Manifest Check",
                "Check for remote agent-detection manifest updates",
                settings
                    .pending_manifest_check
                    .unwrap_or(app.update_manifest_check),
            ),
        ],
    ));
    rows
}

fn notification_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    let sound_enabled = settings
        .pending_sound_enabled
        .unwrap_or_else(|| app.sound_enabled());
    let toast_delivery = settings
        .pending_toast_delivery
        .unwrap_or_else(|| app.toast_delivery());
    let toast_delay = settings
        .pending_toast_delay
        .clone()
        .unwrap_or_else(|| app.toast_config.delay_seconds.to_string());
    let gardn_position = settings
        .pending_toast_gardn_position
        .unwrap_or(app.toast_config.gardn.position);
    let clipboard_enabled = settings
        .pending_clipboard_toast_enabled
        .unwrap_or(app.toast_config.clipboard.enabled);
    let clipboard_position = settings
        .pending_clipboard_toast_position
        .unwrap_or(app.toast_config.clipboard.position);
    let mut rows = setting_group(
        "Sound Alerts",
        [option(
            NotificationRowId::SoundAlerts.selection_index(),
            "Sound Alerts",
            "Play sound when a background agent needs attention",
            sound_enabled,
        )],
    );
    rows.push(SettingsListRow::Spacer);
    rows.extend(setting_group(
        "Notification Popups",
        [
            value_option(
                NotificationRowId::ToastDelivery.selection_index(),
                "Toast Delivery",
                "Where command and agent notifications should appear",
                toast_delivery_label(toast_delivery),
            ),
            SettingsListRow::TextInput {
                index: NotificationRowId::ToastDelay.selection_index(),
                title: "Background Alert Delay".into(),
                value: toast_delay.into(),
            },
            value_option(
                NotificationRowId::ToastGardnPosition.selection_index(),
                "In-App Toast Position",
                "Corner used for in-app toasts",
                toast_gardn_position_label(gardn_position),
            ),
        ],
    ));
    rows.push(SettingsListRow::Spacer);
    rows.extend(setting_group(
        "Clipboard Feedback",
        [
            option(
                NotificationRowId::ClipboardEnabled.selection_index(),
                "Copy Confirmation",
                "Show a short confirmation after copying text",
                clipboard_enabled,
            ),
            value_option(
                NotificationRowId::ClipboardPosition.selection_index(),
                "Copy Confirmation Position",
                "Where copy confirmations appear",
                toast_clipboard_position_label(clipboard_position),
            ),
        ],
    ));
    rows
}

/// Stable row identity for the connection editor.
///
/// Input and render share these typed ids so keyboard/mouse dispatch never
/// depends on magic contiguous integers or row-order accidents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionField {
    Name,
    Target,
    Directory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionAction {
    Save,
    Discard,
    Delete,
    Test,
    Toggle,
    LaunchWorkspace,
    EditDetails,
    ForgetConnection,
    ForgetTermination { offset: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConnectionRowId {
    Field(ConnectionField),
    Action(ConnectionAction),
}

impl ConnectionRowId {
    /// Dense index used by the existing list selection model.
    pub(crate) const fn selection_index(self) -> usize {
        match self {
            Self::Field(ConnectionField::Target) => 0,
            Self::Field(ConnectionField::Name) => 1,
            Self::Field(ConnectionField::Directory) => 2,
            Self::Action(ConnectionAction::Save) => 3,
            Self::Action(ConnectionAction::Discard) => 4,
            Self::Action(ConnectionAction::Delete) => 5,
            Self::Action(ConnectionAction::Test) => 6,
            Self::Action(ConnectionAction::Toggle) => 7,
            Self::Action(ConnectionAction::LaunchWorkspace) => 8,
            Self::Action(ConnectionAction::EditDetails) => 9,
            Self::Action(ConnectionAction::ForgetConnection) => 10,
            Self::Action(ConnectionAction::ForgetTermination { offset }) => 11 + offset,
        }
    }

    pub(crate) fn from_selection_index(index: usize) -> Option<Self> {
        Some(match index {
            0 => Self::Field(ConnectionField::Target),
            1 => Self::Field(ConnectionField::Name),
            2 => Self::Field(ConnectionField::Directory),
            3 => Self::Action(ConnectionAction::Save),
            4 => Self::Action(ConnectionAction::Discard),
            5 => Self::Action(ConnectionAction::Delete),
            6 => Self::Action(ConnectionAction::Test),
            7 => Self::Action(ConnectionAction::Toggle),
            8 => Self::Action(ConnectionAction::LaunchWorkspace),
            9 => Self::Action(ConnectionAction::EditDetails),
            10 => Self::Action(ConnectionAction::ForgetConnection),
            offset if offset >= 11 => Self::Action(ConnectionAction::ForgetTermination {
                offset: offset - 11,
            }),
            _ => return None,
        })
    }
}

#[cfg(test)]
pub(crate) const CONNECTION_TARGET_INDEX: usize =
    ConnectionRowId::Field(ConnectionField::Target).selection_index();
#[cfg(test)]
pub(crate) const CONNECTION_SAVE_INDEX: usize =
    ConnectionRowId::Action(ConnectionAction::Save).selection_index();
#[cfg(test)]
pub(crate) const CONNECTION_DISCARD_INDEX: usize =
    ConnectionRowId::Action(ConnectionAction::Discard).selection_index();
#[cfg(test)]
pub(crate) const CONNECTION_DELETE_INDEX: usize =
    ConnectionRowId::Action(ConnectionAction::Delete).selection_index();
#[cfg(test)]
pub(crate) const CONNECTION_TEST_INDEX: usize =
    ConnectionRowId::Action(ConnectionAction::Test).selection_index();

fn connection_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    if connection_editor_open(settings) {
        connection_editor_rows(app, settings)
    } else {
        connection_browse_rows(app)
    }
}

pub(crate) fn connection_editor_open(settings: &SettingsState) -> bool {
    settings.connection_editor.is_some()
}

fn connection_status_tone(status: &crate::execution_host::ConnectionStatus) -> SettingsMarkerTone {
    use crate::execution_host::ConnectionStatus;
    match status {
        ConnectionStatus::Disconnected | ConnectionStatus::Disconnecting => {
            SettingsMarkerTone::Disabled
        }
        ConnectionStatus::Connecting => SettingsMarkerTone::Accent,
        ConnectionStatus::Connected => SettingsMarkerTone::Good,
        ConnectionStatus::Reconnecting { .. } | ConnectionStatus::AuthenticationRequired => {
            SettingsMarkerTone::Warning
        }
    }
}

fn connection_browse_rows(app: &AppState) -> Vec<SettingsListRow> {
    let mut rows = vec![
        SettingsListRow::Action {
            index: 0,
            icon: "".into(),
            label: "Add SSH Connection".into(),
            tone: SettingsMarkerTone::Accent,
        },
        SettingsListRow::Spacer,
        SettingsListRow::Header("Saved Profiles"),
    ];
    if app.ssh_connection_profiles.is_empty() {
        rows.push(SettingsListRow::Caption(
            "None yet — add one to connect to SSH hosts".into(),
        ));
    }
    for (index, profile) in (1..).zip(&app.ssh_connection_profiles) {
        let status = app.ssh_connection_status(profile);
        let detail = match profile.suggested_directory() {
            Some(directory) => format!("{} · {directory}", profile.target()),
            None => profile.target().to_string(),
        };
        rows.push(SettingsListRow::Profile {
            index,
            name: profile.name().to_string().into(),
            detail: detail.into(),
            badge: Some(status.label().into()),
            tone: connection_status_tone(&status),
        });
    }
    rows
}

fn connection_editor_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    let Some(editor) = settings.connection_editor.as_ref() else {
        return Vec::new();
    };
    if editor.is_detail() {
        connection_detail_rows(app, editor)
    } else {
        connection_form_rows(editor)
    }
}

fn connection_form_rows(editor: &crate::app::state::ConnectionEditorState) -> Vec<SettingsListRow> {
    let editing = editor.is_editing();
    let target = editor.draft.target.clone();
    let name = editor.draft.name.clone();
    let directory = editor.draft.directory.clone();
    let mut rows = vec![
        SettingsListRow::Caption(
            "OpenSSH destination, such as admin@example.com or an SSH config alias.".into(),
        ),
        SettingsListRow::TextInput {
            index: ConnectionRowId::Field(ConnectionField::Target).selection_index(),
            title: "SSH Target".into(),
            value: target.clone().into(),
        },
        SettingsListRow::Spacer,
        SettingsListRow::Caption(
            "Optional label. The SSH target is used when this is empty.".into(),
        ),
        SettingsListRow::TextInput {
            index: ConnectionRowId::Field(ConnectionField::Name).selection_index(),
            title: "Name (Optional)".into(),
            value: name.into(),
        },
        SettingsListRow::Spacer,
        SettingsListRow::Caption(
            "Optional remote directory for new workspaces on this connection.".into(),
        ),
        SettingsListRow::TextInput {
            index: ConnectionRowId::Field(ConnectionField::Directory).selection_index(),
            title: "Starting Directory (Optional)".into(),
            value: directory.into(),
        },
        SettingsListRow::Spacer,
    ];
    if target.trim().is_empty() {
        rows.push(SettingsListRow::Caption("SSH Target Is Required".into()));
    }
    rows.push(SettingsListRow::Action {
        index: ConnectionRowId::Action(ConnectionAction::Save).selection_index(),
        icon: "".into(),
        label: if editing {
            "Save Changes".into()
        } else {
            "Add Connection".into()
        },
        tone: SettingsMarkerTone::Accent,
    });
    rows.push(SettingsListRow::Action {
        index: ConnectionRowId::Action(ConnectionAction::Discard).selection_index(),
        icon: "×".into(),
        label: if editing {
            "Discard Changes".into()
        } else {
            "Cancel".into()
        },
        tone: SettingsMarkerTone::Disabled,
    });
    rows
}

fn connection_detail_rows(
    app: &AppState,
    editor: &crate::app::state::ConnectionEditorState,
) -> Vec<SettingsListRow> {
    let Some(profile) = editor.profile_id().and_then(|profile_id| {
        app.ssh_connection_profiles
            .iter()
            .find(|profile| profile.id() == profile_id)
    }) else {
        return vec![SettingsListRow::Caption(
            "This connection profile no longer exists.".into(),
        )];
    };
    let status = app.ssh_connection_status(profile);
    let mut status_text = status.label().to_string();
    if let crate::execution_host::ConnectionStatus::Reconnecting { error } = &status {
        status_text = format!("{status_text} · {error}");
    }
    let mut rows = vec![SettingsListRow::Caption(
        format!("{} · {status_text}", profile.name()).into(),
    )];
    use crate::execution_host::ConnectionStatus;
    let (toggle_label, toggle_tone) = match &status {
        ConnectionStatus::Disconnected | ConnectionStatus::AuthenticationRequired => {
            ("Connect", SettingsMarkerTone::Good)
        }
        ConnectionStatus::Connecting
        | ConnectionStatus::Connected
        | ConnectionStatus::Reconnecting { .. } => ("Disconnect", SettingsMarkerTone::Danger),
        ConnectionStatus::Disconnecting => ("Disconnect", SettingsMarkerTone::Disabled),
    };
    rows.push(SettingsListRow::Action {
        index: ConnectionRowId::Action(ConnectionAction::LaunchWorkspace).selection_index(),
        icon: "".into(),
        label: "Open Workspace".into(),
        tone: if matches!(status, ConnectionStatus::Connected) {
            SettingsMarkerTone::Good
        } else {
            SettingsMarkerTone::Disabled
        },
    });
    rows.push(SettingsListRow::Action {
        index: ConnectionRowId::Action(ConnectionAction::Toggle).selection_index(),
        icon: "".into(),
        label: toggle_label.into(),
        tone: toggle_tone,
    });
    rows.push(SettingsListRow::Action {
        index: ConnectionRowId::Action(ConnectionAction::Test).selection_index(),
        icon: "".into(),
        label: "Test Connection".into(),
        tone: SettingsMarkerTone::Accent,
    });
    rows.push(SettingsListRow::Spacer);
    rows.push(SettingsListRow::Header("Details"));
    rows.push(SettingsListRow::Caption(
        format!("SSH target  {}", profile.target()).into(),
    ));
    rows.push(SettingsListRow::Caption(
        format!(
            "Starting directory  {}",
            profile
                .suggested_directory()
                .map_or("(Not Set)".to_string(), ToString::to_string)
        )
        .into(),
    ));
    rows.push(SettingsListRow::Action {
        index: ConnectionRowId::Action(ConnectionAction::EditDetails).selection_index(),
        icon: "".into(),
        label: "Edit Details".into(),
        tone: SettingsMarkerTone::Accent,
    });
    let tombstones = app.remote_termination_tombstones_for_profile(profile.id());
    if !tombstones.is_empty() {
        rows.push(SettingsListRow::Spacer);
        rows.push(SettingsListRow::Header("Pending Cleanup"));
        rows.push(SettingsListRow::Caption(
            "The remote host has not acknowledged terminal shutdown.".into(),
        ));
        for (offset, tombstone) in tombstones.iter().enumerate() {
            let confirming =
                editor.pending_forget_remote_terminal.as_ref() == Some(&tombstone.terminal_id);
            rows.push(SettingsListRow::Caption(
                format!(
                    "{} · {}",
                    tombstone.terminal_id,
                    tombstone.path.as_path().display()
                )
                .into(),
            ));
            if confirming {
                rows.push(SettingsListRow::Caption(
                    "Warning: the remote process may remain running.".into(),
                ));
            }
            rows.push(SettingsListRow::Action {
                index: ConnectionRowId::Action(ConnectionAction::ForgetTermination { offset })
                    .selection_index(),
                icon: "×".into(),
                label: if confirming {
                    "Confirm Forget Without Terminating".into()
                } else {
                    "Forget Without Terminating".into()
                },
                tone: SettingsMarkerTone::Danger,
            });
        }
    }
    rows.push(SettingsListRow::Spacer);
    rows.push(SettingsListRow::Header("Danger Zone"));
    if matches!(
        editor.connection_retirement,
        Some(crate::app::state::ConnectionRetirementState::Failed)
    ) {
        rows.extend(connection_retirement_preview_rows(app, editor));
        rows.push(SettingsListRow::Action {
            index: ConnectionRowId::Action(ConnectionAction::ForgetConnection).selection_index(),
            icon: "×".into(),
            label: "Remove Saved Connection".into(),
            tone: SettingsMarkerTone::Danger,
        });
        rows.push(SettingsListRow::Action {
            index: ConnectionRowId::Action(ConnectionAction::Delete).selection_index(),
            icon: "↻".into(),
            label: "Try Again".into(),
            tone: SettingsMarkerTone::Accent,
        });
        rows.push(SettingsListRow::Action {
            index: ConnectionRowId::Action(ConnectionAction::Discard).selection_index(),
            icon: "".into(),
            label: "Cancel".into(),
            tone: SettingsMarkerTone::Accent,
        });
        return rows;
    }
    rows.push(SettingsListRow::Caption(
        "Review affected sessions and managed workers before removal.".into(),
    ));
    let delete_icon = if editor.retirement_in_progress() {
        crate::ui::spinner_frame(app.spinner_tick)
    } else {
        "×"
    };
    let (delete_label, delete_tone) = match editor.connection_retirement.as_ref() {
        None => ("Remove Connection", SettingsMarkerTone::Danger),
        Some(crate::app::state::ConnectionRetirementState::InventoryPending) => {
            ("Checking Removal Impact...", SettingsMarkerTone::Disabled)
        }
        Some(crate::app::state::ConnectionRetirementState::Review(_)) => (
            "Confirm Stop Managed Work and Remove Connection",
            SettingsMarkerTone::Danger,
        ),
        Some(crate::app::state::ConnectionRetirementState::Running(_)) => {
            ("Removing Connection...", SettingsMarkerTone::Disabled)
        }
        Some(crate::app::state::ConnectionRetirementState::LocalForgetRunning) => {
            ("Removing Saved Connection...", SettingsMarkerTone::Disabled)
        }
        Some(crate::app::state::ConnectionRetirementState::Failed) => unreachable!(),
    };
    rows.push(SettingsListRow::Action {
        index: ConnectionRowId::Action(ConnectionAction::Delete).selection_index(),
        icon: delete_icon.into(),
        label: delete_label.into(),
        tone: delete_tone,
    });
    rows.extend(connection_retirement_preview_rows(app, editor));

    rows
}

fn connection_retirement_preview_rows(
    app: &AppState,
    editor: &crate::app::state::ConnectionEditorState,
) -> Vec<SettingsListRow> {
    use crate::app::state::ConnectionRetirementState;

    let Some(state) = editor.connection_retirement.as_ref() else {
        return Vec::new();
    };
    if matches!(state, ConnectionRetirementState::InventoryPending) {
        return vec![SettingsListRow::Caption(
            "Checking every session and managed worker binding. No resources have changed.".into(),
        )];
    }
    if matches!(state, ConnectionRetirementState::Failed) {
        let connection_name = if editor.draft.name.trim().is_empty() {
            editor.draft.target.trim()
        } else {
            editor.draft.name.trim()
        };
        return vec![
            SettingsListRow::Caption(
                format!("Full cleanup is unavailable for {connection_name}.").into(),
            ),
            SettingsListRow::Caption(
                "Processes or worker files on that machine might remain.".into(),
            ),
            SettingsListRow::Caption("Remove the saved connection anyway?".into()),
        ];
    }
    let (preview, running) = match state {
        ConnectionRetirementState::Review(preview) => (preview, false),
        ConnectionRetirementState::Running(preview) => (preview, true),
        ConnectionRetirementState::InventoryPending
        | ConnectionRetirementState::Failed
        | ConnectionRetirementState::LocalForgetRunning => unreachable!(),
    };

    let mut rows = vec![SettingsListRow::Caption(
        if running {
            "Stopping only Gardn-managed work. Unrelated remote processes are untouched."
        } else {
            "Review all effects. Confirmation stops Gardn-managed processes across sessions."
        }
        .into(),
    )];
    rows.extend(connection_retirement_plan_rows(app, &preview.plan));
    for binding in &preview.bindings.bindings {
        rows.push(SettingsListRow::Caption(
            format!(
                "Managed worker PID {}: {}",
                binding.ownership.pid,
                if binding.lock_live {
                    "Running"
                } else {
                    "Stopped"
                },
            )
            .into(),
        ));
    }
    if preview.plan.sessions.is_empty() && preview.bindings.bindings.is_empty() {
        rows.push(SettingsListRow::Caption(
            "No session resources or managed worker bindings remain.".into(),
        ));
    }
    rows
}
fn connection_retirement_plan_rows(
    app: &AppState,
    plan: &crate::execution_host::connection_retirement::ConnectionRetirementPlan,
) -> Vec<SettingsListRow> {
    let mut rows = Vec::new();
    for session in &plan.sessions {
        rows.push(SettingsListRow::Caption(
            format!(
                "Session {}: {} pane(s), {} pending termination(s)",
                session.session_name,
                session.remote_panes.len(),
                session.pending_terminations.len(),
            )
            .into(),
        ));
        for group in &session.group_defaults {
            rows.push(SettingsListRow::Caption(
                format!("Group default cleared: {}", group.group_name).into(),
            ));
        }
        for workspace in &session.workspace_defaults {
            let name = workspace.workspace_id.as_deref().map_or_else(
                || format!("#{}", workspace.workspace_index + 1),
                str::to_string,
            );
            rows.push(SettingsListRow::Caption(
                format!(
                    "Workspace {name}: default becomes {} at {}",
                    app.host_label(crate::app::host_label::HostLabelTarget::Coordinator),
                    workspace.replacement.path.as_path().display(),
                )
                .into(),
            ));
        }
    }
    rows
}

fn integration_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    let selection = crate::app::integration_host::resolve(app, settings);
    let host_label = selection.label(app);
    let host_id = selection.host_id().cloned();
    let has_host_selector = !app.ssh_connection_profiles.is_empty();
    let mut rows = if has_host_selector {
        vec![
            SettingsListRow::Value {
                index: 0,
                title: "Integration Host".into(),
                description: "Choose the coordinator or a configured SSH connection.".into(),
                value: host_label.to_string().into(),
                editable: true,
            },
            SettingsListRow::Spacer,
        ]
    } else {
        Vec::new()
    };
    let first_integration_index = usize::from(has_host_selector);

    let Some(host_id) = host_id else {
        rows.extend(
            app.integration_recommendations
                .iter()
                .enumerate()
                .map(|(offset, item)| {
                    let missing_profile_hooks =
                        crate::integration::missing_profile_hook_count_for_target(
                            item.target,
                            &app.agent_profiles,
                        );
                    let profile_hooks_missing = item.state
                        == crate::integration::IntegrationStatusKind::Current
                        && missing_profile_hooks > 0;
                    let tone = if profile_hooks_missing {
                        SettingsMarkerTone::Warning
                    } else {
                        integration_status_tone(item.state, item.available)
                    };
                    let status = if profile_hooks_missing {
                        missing_profile_hook_label(missing_profile_hooks)
                    } else {
                        item.status_label().to_string()
                    };

                    SettingsListRow::Status {
                        index: offset + first_integration_index,
                        label: crate::agent_profiles::AgentKind::from(item.target)
                            .display_name()
                            .into(),
                        status: status.into(),
                        tone,
                    }
                }),
        );
        return rows;
    };

    match app.host_integration_observations.get(&host_id) {
        Some(crate::integration::host::HostIntegrationObservation::Ready(snapshot)) => {
            rows.extend(snapshot.entries.iter().enumerate().map(|(offset, entry)| {
                SettingsListRow::Status {
                    index: offset + first_integration_index,
                    label: crate::agent_profiles::AgentKind::from(entry.target)
                        .display_name()
                        .into(),
                    status: entry.status_label().into(),
                    tone: if entry.state == crate::integration::IntegrationStatusKind::Current
                        && entry.missing_profile_hooks > 0
                    {
                        SettingsMarkerTone::Warning
                    } else {
                        integration_status_tone(entry.state, entry.available)
                    },
                }
            }));
        }
        Some(crate::integration::host::HostIntegrationObservation::Failed(message)) => {
            rows.push(SettingsListRow::Caption(
                format!("Could not inspect {host_label}: {message}").into(),
            ));
        }
        Some(crate::integration::host::HostIntegrationObservation::Pending) | None => {
            rows.push(SettingsListRow::Caption(
                format!("Checking integrations on {host_label}...").into(),
            ));
        }
    }
    rows
}

fn integration_status_tone(
    state: crate::integration::IntegrationStatusKind,
    available: bool,
) -> SettingsMarkerTone {
    match state {
        crate::integration::IntegrationStatusKind::Current => SettingsMarkerTone::Good,
        crate::integration::IntegrationStatusKind::Outdated => SettingsMarkerTone::Warning,
        crate::integration::IntegrationStatusKind::NotInstalled if available => {
            SettingsMarkerTone::Accent
        }
        crate::integration::IntegrationStatusKind::NotInstalled => SettingsMarkerTone::Disabled,
    }
}

fn missing_profile_hook_label(count: usize) -> String {
    if count == 1 {
        "Installed · 1 Profile Hook Missing".to_string()
    } else {
        format!("Installed · {count} Profile Hooks Missing")
    }
}

fn toast_delivery_label(delivery: ToastDelivery) -> &'static str {
    match delivery {
        ToastDelivery::Off => "Off",
        ToastDelivery::Gardn => "Inside Gardn",
        ToastDelivery::Terminal => "Via Terminal",
        ToastDelivery::System => "Via System",
    }
}
fn shell_mode_label(mode: crate::config::ShellModeConfig) -> &'static str {
    match mode {
        crate::config::ShellModeConfig::Auto => "Auto",
        crate::config::ShellModeConfig::Login => "Login",
        crate::config::ShellModeConfig::NonLogin => "Non-login",
    }
}

fn toast_gardn_position_label(position: crate::config::ToastGardnPosition) -> &'static str {
    match position {
        crate::config::ToastGardnPosition::TopLeft => "Top Left",
        crate::config::ToastGardnPosition::TopRight => "Top Right",
        crate::config::ToastGardnPosition::BottomLeft => "Bottom Left",
        crate::config::ToastGardnPosition::BottomRight => "Bottom Right",
    }
}

fn toast_clipboard_position_label(position: crate::config::ToastClipboardPosition) -> &'static str {
    match position {
        crate::config::ToastClipboardPosition::TopLeft => "Top Left",
        crate::config::ToastClipboardPosition::TopCenter => "Top Center",
        crate::config::ToastClipboardPosition::TopRight => "Top Right",
        crate::config::ToastClipboardPosition::BottomLeft => "Bottom Left",
        crate::config::ToastClipboardPosition::BottomCenter => "Bottom Center",
        crate::config::ToastClipboardPosition::BottomRight => "Bottom Right",
    }
}

fn toast_rows(app: &AppState, settings: &SettingsState) -> Vec<SettingsListRow> {
    let current = settings
        .pending_toast_delivery
        .unwrap_or_else(|| app.toast_delivery());
    setting_group(
        "Notification Popups",
        [value_option(
            0,
            "Toast Delivery",
            "Where notification popups should appear",
            toast_delivery_label(current),
        )],
    )
}

fn about_rows() -> Vec<SettingsListRow> {
    let mut rows = setting_group(
        "Acknowledgments",
        [SettingsListRow::Caption(
            "GitHub workflow behavior adapted from ghui by Kit Langton.".into(),
        )],
    );
    rows.push(SettingsListRow::Spacer);
    rows.extend(
        include_str!("github/LICENSE")
            .lines()
            .map(|line| SettingsListRow::Caption(line.into())),
    );
    rows
}

fn setting_group(
    header: &'static str,
    settings: impl IntoIterator<Item = SettingsListRow>,
) -> Vec<SettingsListRow> {
    let mut rows = vec![SettingsListRow::Header(header)];
    push_spaced_settings(&mut rows, settings);
    rows
}

fn push_spaced_settings(
    rows: &mut Vec<SettingsListRow>,
    settings: impl IntoIterator<Item = SettingsListRow>,
) {
    let mut first = true;
    for setting in settings {
        if !first {
            rows.push(SettingsListRow::Spacer);
        }
        rows.push(setting);
        first = false;
    }
}

fn option(
    index: usize,
    title: impl Into<Cow<'static, str>>,
    description: impl Into<Cow<'static, str>>,
    enabled: bool,
) -> SettingsListRow {
    SettingsListRow::Toggle {
        index,
        title: title.into(),
        description: description.into(),
        enabled,
    }
}

fn value_option(
    index: usize,
    title: impl Into<Cow<'static, str>>,
    description: impl Into<Cow<'static, str>>,
    value: impl Into<Cow<'static, str>>,
) -> SettingsListRow {
    SettingsListRow::Value {
        index,
        title: title.into(),
        description: description.into(),
        value: value.into(),
        editable: false,
    }
}

fn choice(index: usize, label: impl Into<Cow<'static, str>>, checked: bool) -> SettingsListRow {
    SettingsListRow::Choice {
        index,
        label: label.into(),
        checked,
    }
}

fn theme_display_name(name: &'static str) -> &'static str {
    match name {
        "catppuccin-latte" => "Catppuccin Latte",
        "catppuccin" => "Catppuccin Mocha",
        "catppuccin-frappe" => "Catppuccin Frappé",
        "catppuccin-macchiato" => "Catppuccin Macchiato",
        "tokyo-night-day" => "Tokyo Night Day",
        "gruvbox-light" => "Gruvbox",
        "one-light" => "One",
        "solarized-light" => "Solarized",
        "kanagawa-lotus" => "Kanagawa Lotus",
        "rose-pine-dawn" => "Rosé Pine Dawn",
        "tokyo-night" => "Tokyo Night",
        "dracula" => "Dracula",
        "ethereal" => "Ethereal",
        "everforest" => "Everforest",
        "flexoki" => "Flexoki",
        "gruvbox" => "Gruvbox",
        "kanagawa" => "Kanagawa",
        "nord" => "Nord",
        "one-dark" => "One Dark",
        "rose-pine" => "Rosé Pine",
        "monokai-pro" => "Monokai Pro",
        "monokai-pro-light" => "Monokai Pro Light",
        "monokai-pro-light-sun" => "Monokai Pro Sun",
        "monokai-pro-spectrum" => "Monokai Pro Spectrum",
        "monokai-pro-ristretto" => "Monokai Pro Ristretto",
        "monokai-pro-octagon" => "Monokai Pro Octagon",
        "monokai-pro-machine" => "Monokai Pro Machine",
        "monokai-classic" => "Monokai Classic",
        "flexoki-light" => "Flexoki Light",
        "gardn-day" => "Gardn Day",
        "gardn-night" => "Gardn Night",
        "hackerman" => "Hackerman",
        "last-horizon" => "Last Horizon",
        "lumon" => "Lumon",
        "matte-black" => "Matte Black",
        "miasma" => "Miasma",
        "osaka-jade" => "Osaka Jade",
        "retro-82" => "Retro 82",
        "solitude" => "Solitude",
        "vantablack" => "Vantablack",
        "white" => "White",
        "flexoki-dark" => "Flexoki Dark",
        "omarchy" => "Omarchy",
        "solarized" => "Solarized",
        "terminal" => "Terminal",
        "vesper" => "Vesper",
        other => other,
    }
}

fn theme_mode_display_name(mode: ThemeMode) -> &'static str {
    match mode {
        ThemeMode::System => "Automatic",
        ThemeMode::Light => "Light",
        ThemeMode::Dark => "Dark",
    }
}

fn new_terminal_cwd_label(policy: &NewTerminalCwdConfig) -> String {
    match policy {
        NewTerminalCwdConfig::Follow => "Follow Focused Pane".to_string(),
        NewTerminalCwdConfig::Home => "Home Directory".to_string(),
        NewTerminalCwdConfig::Current => "Gardn Process Directory".to_string(),
        NewTerminalCwdConfig::Path(path) => format!("Custom Path: {path}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_headers_and_spacers_are_not_selectable() {
        let rows = [
            SettingsListRow::Header("Section"),
            SettingsListRow::Spacer,
            SettingsListRow::Caption("caption".into()),
            SettingsListRow::Toggle {
                index: 7,
                title: "option".into(),
                description: "description".into(),
                enabled: true,
            },
            SettingsListRow::Choice {
                index: 11,
                label: "choice".into(),
                checked: false,
            },
        ];

        let index_for_row = |row| option_hit_for_visual_row(&rows, row).map(|hit| hit.index);
        assert_eq!(index_for_row(0), None);
        assert_eq!(index_for_row(1), None);
        assert_eq!(index_for_row(2), None);
        assert_eq!(index_for_row(3), Some(7));
        assert_eq!(index_for_row(4), Some(7));
        assert_eq!(index_for_row(5), Some(11));
        assert_eq!(index_for_row(6), None);
    }

    #[test]
    fn system_profile_rows_do_not_repeat_builtin_details() {
        let profile = crate::agent_profiles::AgentProfile {
            id: "system:cursor".to_string(),
            name: "cursor".to_string(),
            kind: crate::agent_profiles::AgentKind::Cursor,
            command: "cursor-agent".to_string(),
            argv: vec!["cursor-agent".to_string()],
            env: Vec::new(),
            enabled: true,
            source: crate::agent_profiles::AgentProfileSource::System,
            parse_error: None,
        };

        assert_eq!(agent_profile_detail(&profile), "");
    }

    #[test]
    fn profile_rows_take_one_visual_row() {
        let rows = [
            SettingsListRow::Header("profiles"),
            SettingsListRow::Profile {
                index: 3,
                name: "cursor".into(),
                detail: "".into(),
                badge: None,
                tone: SettingsMarkerTone::Good,
            },
            SettingsListRow::Profile {
                index: 4,
                name: "omp-mk".into(),
                detail: "omp".into(),
                badge: None,
                tone: SettingsMarkerTone::Good,
            },
        ];

        assert_eq!(visual_row_count(&rows), 3);
        let index_for_row = |row| option_hit_for_visual_row(&rows, row).map(|hit| hit.index);
        assert_eq!(index_for_row(1), Some(3));
        assert_eq!(index_for_row(2), Some(4));
        assert_eq!(index_for_row(3), None);
    }

    #[test]
    fn connection_retirement_preview_discloses_scope_before_confirmation() {
        let mut editor = crate::app::state::ConnectionEditorState::detail_profile(
            "robotbox", "Robotbox", "robotbox", "",
        );
        editor.connection_retirement = Some(crate::app::state::ConnectionRetirementState::Review(
            crate::app::state::ConnectionRetirementPreview {
                plan: crate::execution_host::connection_retirement::ConnectionRetirementPlan {
                    host_id: crate::execution_host::ExecutionHostId::new("ssh:robotbox:1").unwrap(),
                    sessions: Vec::new(),
                },
                bindings: crate::execution_host::runtime_paths::BindingInventoryReport {
                    bindings: Vec::new(),
                },
            },
        ));
        let app = AppState::test_new();
        let rows = connection_retirement_preview_rows(&app, &editor);
        let captions: Vec<&str> = rows
            .iter()
            .filter_map(|row| match row {
                SettingsListRow::Caption(text) => Some(text.as_ref()),
                _ => None,
            })
            .collect();
        assert!(captions
            .iter()
            .any(|text| text.contains("Review all effects")));
        assert!(captions
            .iter()
            .any(|text| text.contains("No session resources")));
    }

    #[test]
    fn custom_codex_profile_rows_mark_missing_profile_hook_as_warning() {
        let _lock = crate::integration::integration_env_lock();
        let base = std::env::temp_dir().join(format!(
            "gardn-settings-codex-profile-warning-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let home = base.join("home");
        std::fs::create_dir_all(home.join(".codex-mk")).unwrap();
        let _codex_home_env = crate::config::TestEnvVar::remove("CODEX_HOME");
        let _home_env = crate::config::TestEnvVar::set("HOME", &home);
        let mut app = AppState::test_new();
        app.agent_profiles = crate::agent_profiles::AgentProfileCatalog::from_config(
            &crate::agent_profiles::AgentProfilesConfig {
                order: vec!["user:codex-mk".to_string()],
                custom: vec![crate::agent_profiles::UserAgentProfileConfig {
                    id: "codex-mk".to_string(),
                    name: "codex mk".to_string(),
                    kind: crate::agent_profiles::AgentKind::Codex,
                    command: "codex-mk".to_string(),
                    env: std::collections::BTreeMap::new(),
                    enabled: true,
                }],
            },
        );
        app.integration_recommendations = vec![crate::integration::IntegrationRecommendation {
            target: crate::api::schema::IntegrationTarget::Codex,
            label: "codex",
            command: "codex",
            available: true,
            path: std::path::PathBuf::from("/tmp/gardn-test-codex"),
            state: crate::integration::IntegrationStatusKind::Current,
        }];

        let rows = rows_for_section(&app, SettingsSection::Agents).expect("agent rows");
        let row = rows
            .iter()
            .find(|row| {
                matches!(
                    row,
                    SettingsListRow::Profile { name, .. } if name.as_ref() == "codex mk"
                )
            })
            .expect("custom codex profile row remains visible");

        match row {
            SettingsListRow::Profile { badge, tone, .. } => {
                assert_eq!(*tone, SettingsMarkerTone::Warning);
                assert_eq!(
                    badge
                        .as_ref()
                        .expect("profile row should expose missing hook badge")
                        .as_ref(),
                    "Hook Missing"
                );
            }
            _ => unreachable!("matched profile row"),
        }

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn integrations_row_warns_when_custom_codex_profile_home_missing_hook() {
        let _lock = crate::integration::integration_env_lock();
        let base = std::env::temp_dir().join(format!(
            "gardn-settings-integrations-codex-profile-hook-{}-{}",
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

        let mut app = AppState::test_new();
        app.agent_profiles = crate::agent_profiles::AgentProfileCatalog::from_config(
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
        app.integration_recommendations = vec![crate::integration::IntegrationRecommendation {
            target: crate::api::schema::IntegrationTarget::Codex,
            label: "codex",
            command: "codex",
            available: true,
            path: default_codex_dir.join("gardn-agent-state.sh"),
            state: crate::integration::IntegrationStatusKind::Current,
        }];

        let rows = rows_for_section(&app, SettingsSection::Integrations).expect("integration rows");
        let codex_row = rows
            .iter()
            .find(|row| {
                matches!(
                    row,
                    SettingsListRow::Status { label, .. } if label.as_ref() == "Codex"
                )
            })
            .expect("codex integration row");

        match codex_row {
            SettingsListRow::Status { status, tone, .. } => {
                assert_eq!(status.as_ref(), "Installed · 1 Profile Hook Missing");
                assert_eq!(*tone, SettingsMarkerTone::Warning);
            }
            _ => unreachable!("matched status row"),
        }

        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn integrations_rows_select_local_or_configured_ssh_host() {
        let mut app = AppState::test_new();
        let profile = crate::persist::ssh_profiles::SshConnectionProfile::new(
            "workbox", "Work box", "workbox", None,
        )
        .unwrap();
        let host_id = profile.execution_host_id();
        app.ssh_connection_profiles.push(profile);
        app.integration_recommendations = vec![crate::integration::IntegrationRecommendation {
            target: crate::api::schema::IntegrationTarget::Codex,
            label: "codex",
            command: "codex",
            available: true,
            path: "/tmp/gardn-test-codex".into(),
            state: crate::integration::IntegrationStatusKind::Current,
        }];

        let local_rows =
            rows_for_section(&app, SettingsSection::Integrations).expect("local integration rows");
        assert!(matches!(
            &local_rows[0],
            SettingsListRow::Value { title, value, .. }
                if title.as_ref() == "Integration Host" && value.as_ref() == "test-host"
        ));

        app.settings.integration_host_profile_id = Some("workbox".to_string());
        app.host_integration_observations.insert(
            host_id,
            crate::integration::host::HostIntegrationObservation::Ready(
                crate::integration::host::HostIntegrationSnapshot {
                    entries: vec![crate::integration::host::HostIntegrationEntry {
                        target: crate::api::schema::IntegrationTarget::Codex,
                        available: true,
                        state: crate::integration::IntegrationStatusKind::Outdated,
                        missing_profile_hooks: 0,
                    }],
                },
            ),
        );
        let remote_rows =
            rows_for_section(&app, SettingsSection::Integrations).expect("remote integration rows");
        assert!(matches!(
            &remote_rows[0],
            SettingsListRow::Value { value, .. } if value.as_ref() == "Work box"
        ));
        assert!(remote_rows.iter().any(|row| matches!(
            row,
            SettingsListRow::Status {
                label,
                status,
                tone: SettingsMarkerTone::Warning,
                ..
            } if label.as_ref() == "Codex" && status.as_ref() == "Update Available"
        )));
    }

    #[test]
    fn client_rows_use_the_client_pending_sidebar_width() {
        let mut app = AppState::test_new();
        app.settings.pending_sidebar_width = Some(22);
        let mut view = ClientViewState::from_default_client_state(&app);
        view.settings.section = SettingsSection::Layout;
        view.settings.pending_sidebar_width = Some(77);

        let rows = rows_for_section_for_view(&app, &view).expect("layout rows");
        let width = rows
            .iter()
            .find_map(|row| match row {
                SettingsListRow::Value { title, value, .. }
                    if title.as_ref() == "Default Sidebar Width" =>
                {
                    Some(value.as_ref())
                }
                _ => None,
            })
            .expect("default sidebar width row");

        assert_eq!(width, "77 cols");
    }

    #[test]
    fn appearance_rows_keep_blank_line_between_sidebar_and_panes() {
        let app = AppState::test_new();
        let rows = appearance_rows(&app, &app.settings);
        let initial_agent_scope = rows
            .iter()
            .position(|row| {
                matches!(
                    row,
                    SettingsListRow::Value { title, .. } if title.as_ref() == "Initial Agent Scope"
                )
            })
            .expect("initial agent scope row");
        assert!(matches!(
            rows[initial_agent_scope + 1],
            SettingsListRow::Spacer
        ));
        assert!(matches!(
            rows[initial_agent_scope + 2],
            SettingsListRow::Header("Panes")
        ));
    }

    #[test]
    fn experimental_settings_expose_kitty_graphics_with_reconnect_guidance() {
        let app = AppState::test_new();
        let rows = experiment_rows(&app, &app.settings);
        assert!(rows.iter().any(|row| matches!(
            row,
            SettingsListRow::Toggle {
                title,
                description,
                enabled: false,
                ..
            } if title.as_ref() == "Kitty Graphics"
                && description.contains("Reconnect Gardn")
        )));
    }

    #[test]
    fn every_settings_section_separates_group_headers() {
        let app = AppState::test_new();

        for section in SettingsSection::ALL {
            let rows = rows_for_section(&app, *section).expect("settings rows");
            for (index, row) in rows.iter().enumerate() {
                if index > 0 && matches!(row, SettingsListRow::Header(_)) {
                    assert!(
                        matches!(rows[index - 1], SettingsListRow::Spacer),
                        "{section:?} header at row {index} has no blank line before it"
                    );
                }
            }
        }
    }

    #[test]
    fn appearance_and_behavior_rows_expose_approved_settings() {
        let app = AppState::test_new();
        let appearance = rows_for_section(&app, SettingsSection::Theme).expect("appearance rows");
        for title in [
            "Pane Borders",
            "Pane Scrollbars",
            "Pane Gaps",
            "Hide Single-Tab Bar",
        ] {
            assert!(
                appearance.iter().any(|row| matches!(
                    row,
                    SettingsListRow::Toggle { title: row_title, .. } if row_title.as_ref() == title
                )),
                "missing {title}"
            );
        }

        let behavior = rows_for_section(&app, SettingsSection::PaneLabels).expect("behavior rows");
        assert!(behavior.iter().any(|row| matches!(
            row,
            SettingsListRow::Toggle { title, .. } if title.as_ref() == "Name New Workspaces"
        )));
        assert!(behavior.iter().any(|row| matches!(
            row,
            SettingsListRow::Toggle { title, .. } if title.as_ref() == "Copy on Select"
        )));
        assert!(behavior.iter().any(|row| matches!(
            row,
            SettingsListRow::Value { title, .. } if title.as_ref() == "Right-click Passthrough"
        )));
        assert!(behavior.iter().any(|row| matches!(
            row,
            SettingsListRow::TextInput { title, .. } if title.as_ref() == "Default Shell"
        )));
        assert!(behavior.iter().any(|row| matches!(
            row,
            SettingsListRow::Value { title, .. } if title.as_ref() == "Shell Startup Mode"
        )));
        assert!(behavior
            .iter()
            .any(|row| { matches!(row, SettingsListRow::Header("Terminal")) }));

        let notifications =
            rows_for_section(&app, SettingsSection::Sound).expect("notification rows");
        assert!(notifications
            .iter()
            .any(|row| matches!(row, SettingsListRow::Header("Sound Alerts"))));
        assert!(notifications
            .iter()
            .any(|row| matches!(row, SettingsListRow::Header("Notification Popups"))));
        assert!(notifications
            .iter()
            .any(|row| matches!(row, SettingsListRow::Header("Clipboard Feedback"))));
        assert!(notifications.iter().any(|row| matches!(
            row,
            SettingsListRow::TextInput { title, .. } if title.as_ref() == "Background Alert Delay"
        )));
        assert!(notifications.iter().any(|row| matches!(
            row,
            SettingsListRow::Value { title, .. } if title.as_ref() == "In-App Toast Position"
        )));
        assert!(notifications.iter().any(|row| matches!(
            row,
            SettingsListRow::Toggle { title, .. } if title.as_ref() == "Copy Confirmation"
        )));

        let advanced = rows_for_section(&app, SettingsSection::Experiments).expect("advanced rows");
        assert!(advanced
            .iter()
            .any(|row| matches!(row, SettingsListRow::Header("Updates"))));
        assert!(advanced.iter().any(|row| matches!(
            row,
            SettingsListRow::Toggle { title, .. } if title.as_ref() == "Version Check"
        )));
        assert!(advanced.iter().any(|row| matches!(
            row,
            SettingsListRow::Toggle { title, .. } if title.as_ref() == "Manifest Check"
        )));
    }

    #[test]
    fn typed_row_ids_are_the_only_selection_mapping() {
        for index in 0..=10 {
            let id = BehaviorRowId::from_selection_index(index).expect("behavior id");
            assert_eq!(id.selection_index(), index);
        }
        assert!(BehaviorRowId::from_selection_index(11).is_none());
        for index in 0..=5 {
            let id = NotificationRowId::from_selection_index(index).expect("notification id");
            assert_eq!(id.selection_index(), index);
        }
        assert!(NotificationRowId::from_selection_index(6).is_none());
        for index in 0..=5 {
            let id = AdvancedRowId::from_selection_index(index).expect("advanced id");
            assert_eq!(id.selection_index(), index);
        }
        assert!(AdvancedRowId::from_selection_index(6).is_none());
    }
}
