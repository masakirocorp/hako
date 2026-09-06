use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph},
    Frame,
};
use unicode_width::UnicodeWidthStr;

use super::scrollbar::render_scrollbar;
use super::widgets::{
    action_button_width, modal_hint_line_count, modal_section_heading_style, modal_stack_areas,
    panel_contrast_fg, render_action_button, render_modal_description, render_modal_divider,
    render_modal_frame, secondary_action_style, ModalFrameSpec,
};
use crate::{
    app::{
        state::{Palette, SettingsSection, SettingsState},
        AppState,
    },
    settings_rows::{
        rows_for_section, rows_for_section_for_view, visual_row_count, SettingsListRow,
        SettingsMarkerTone,
    },
};

#[cfg(test)]
use crate::config::ThemeMode;
const GROUP_SETTINGS_SECTIONS: &[SettingsSection] = &[
    SettingsSection::GroupGeneral,
    SettingsSection::GroupDefaults,
    SettingsSection::Theme,
    SettingsSection::GroupProfiles,
    SettingsSection::GroupGithub,
];
const WORKSPACE_SETTINGS_SECTIONS: &[SettingsSection] = &[
    SettingsSection::WorkspaceGeneral,
    SettingsSection::WorkspaceGithub,
];

const SETTINGS_SIDEBAR_WIDTH: u16 = 21;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SettingsSidebarEntry {
    pub(crate) section: SettingsSection,
    pub(crate) subsection: Option<usize>,
    pub(crate) label: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SettingsSidebarAreas {
    pub(crate) navigation: Rect,
    pub(crate) divider: Rect,
    pub(crate) content: Rect,
}

#[derive(Clone, Copy)]
struct SettingsSubsection {
    label: &'static str,
    anchor: Option<&'static str>,
}

const APPEARANCE_SUBSECTIONS: &[SettingsSubsection] = &[
    SettingsSubsection {
        label: "Colors",
        anchor: Some("Colors"),
    },
    SettingsSubsection {
        label: "Themes",
        anchor: Some("Appearance"),
    },
    SettingsSubsection {
        label: "Sidebar",
        anchor: Some("Sidebar"),
    },
    SettingsSubsection {
        label: "Panes",
        anchor: Some("Panes"),
    },
    SettingsSubsection {
        label: "Agent Status",
        anchor: Some("Agent Status"),
    },
    SettingsSubsection {
        label: "Window",
        anchor: Some("Window"),
    },
];
const NOTIFICATION_SUBSECTIONS: &[SettingsSubsection] = &[
    SettingsSubsection {
        label: "Sound alerts",
        anchor: Some("Sound Alerts"),
    },
    SettingsSubsection {
        label: "Popups",
        anchor: Some("Notification Popups"),
    },
    SettingsSubsection {
        label: "Clipboard",
        anchor: Some("Clipboard Feedback"),
    },
];
const BEHAVIOR_SUBSECTIONS: &[SettingsSubsection] = &[
    SettingsSubsection {
        label: "General",
        anchor: Some("General"),
    },
    SettingsSubsection {
        label: "Selection",
        anchor: Some("Selection"),
    },
    SettingsSubsection {
        label: "Terminal",
        anchor: Some("Terminal"),
    },
    SettingsSubsection {
        label: "Sessions",
        anchor: Some("Sessions"),
    },
];
const COMMAND_SUBSECTIONS: &[SettingsSubsection] = &[SettingsSubsection {
    label: "Project commands",
    anchor: Some("Project Commands"),
}];
const AGENT_SUBSECTIONS: &[SettingsSubsection] = &[SettingsSubsection {
    label: "Profiles",
    anchor: Some("Saved Profiles"),
}];
const INTEGRATION_SUBSECTIONS: &[SettingsSubsection] = &[SettingsSubsection {
    label: "Agent tools",
    anchor: None,
}];
const CONNECTION_SUBSECTIONS: &[SettingsSubsection] = &[SettingsSubsection {
    label: "SSH profiles",
    anchor: Some("Saved Profiles"),
}];
const ADVANCED_SUBSECTIONS: &[SettingsSubsection] = &[
    SettingsSubsection {
        label: "Input",
        anchor: Some("Input"),
    },
    SettingsSubsection {
        label: "Server",
        anchor: Some("Server"),
    },
    SettingsSubsection {
        label: "Updates",
        anchor: Some("Updates"),
    },
];

fn settings_sidebar_section_label(section: SettingsSection) -> &'static str {
    match section {
        SettingsSection::Theme => "Appearance",
        SettingsSection::Sound => "Notifications",
        SettingsSection::PaneLabels => "Behavior",
        SettingsSection::Commands => "Commands",
        SettingsSection::Agents => "Agents",
        SettingsSection::Integrations => "Integrations",
        SettingsSection::Connections => "Connections",
        SettingsSection::Experiments => "Advanced",
        SettingsSection::About => "About",
        SettingsSection::Layout
        | SettingsSection::Toast
        | SettingsSection::GroupProfiles
        | SettingsSection::GroupGeneral
        | SettingsSection::GroupDefaults
        | SettingsSection::GroupGithub
        | SettingsSection::WorkspaceGeneral
        | SettingsSection::WorkspaceGithub => section.label(),
    }
}

fn settings_subsections(section: SettingsSection) -> &'static [SettingsSubsection] {
    match section {
        SettingsSection::Theme => APPEARANCE_SUBSECTIONS,
        SettingsSection::Sound => NOTIFICATION_SUBSECTIONS,
        SettingsSection::PaneLabels => BEHAVIOR_SUBSECTIONS,
        SettingsSection::Commands => COMMAND_SUBSECTIONS,
        SettingsSection::Agents => AGENT_SUBSECTIONS,
        SettingsSection::Integrations => INTEGRATION_SUBSECTIONS,
        SettingsSection::Connections => CONNECTION_SUBSECTIONS,
        SettingsSection::Experiments => ADVANCED_SUBSECTIONS,
        SettingsSection::About
        | SettingsSection::Layout
        | SettingsSection::Toast
        | SettingsSection::GroupProfiles
        | SettingsSection::GroupGeneral
        | SettingsSection::GroupDefaults
        | SettingsSection::GroupGithub
        | SettingsSection::WorkspaceGeneral
        | SettingsSection::WorkspaceGithub => &[],
    }
}

pub(crate) fn settings_subsection_anchor(
    section: SettingsSection,
    subsection: usize,
) -> Option<&'static str> {
    settings_subsections(section)
        .get(subsection)
        .and_then(|subsection| subsection.anchor)
}

pub(crate) fn settings_sidebar_entries(settings: &SettingsState) -> Vec<SettingsSidebarEntry> {
    let mut entries = Vec::with_capacity(SettingsSection::ALL.len() + 4);
    for &section in SettingsSection::ALL {
        entries.push(SettingsSidebarEntry {
            section,
            subsection: None,
            label: settings_sidebar_section_label(section),
        });
        if settings.sidebar_expanded == Some(section) {
            entries.extend(settings_subsections(section).iter().enumerate().map(
                |(subsection, item)| SettingsSidebarEntry {
                    section,
                    subsection: Some(subsection),
                    label: item.label,
                },
            ));
        }
    }
    entries
}

pub(crate) fn settings_sidebar_areas(area: Rect) -> SettingsSidebarAreas {
    let [navigation, divider, _, content] = Layout::horizontal([
        Constraint::Length(SETTINGS_SIDEBAR_WIDTH.min(area.width.saturating_sub(3))),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas::<4>(area);
    SettingsSidebarAreas {
        navigation,
        divider,
        content,
    }
}

pub(crate) fn settings_sidebar_hit_areas(
    settings: &SettingsState,
    area: Rect,
) -> Vec<(SettingsSidebarEntry, Rect)> {
    settings_sidebar_entries(settings)
        .into_iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let y = area.y.saturating_add(2).saturating_add(index as u16);
            (y < area.y.saturating_add(area.height))
                .then_some((entry, Rect::new(area.x, y, area.width, 1)))
        })
        .collect()
}

fn settings_title(app: &AppState) -> &'static str {
    if app.settings.group_settings_target.is_some() {
        "Group Settings"
    } else if app.settings.workspace_settings_target.is_some() {
        "Space Settings"
    } else {
        "Settings"
    }
}

fn settings_sections(app: &AppState) -> &'static [SettingsSection] {
    if app.settings.group_settings_target.is_some() {
        GROUP_SETTINGS_SECTIONS
    } else if app.settings.workspace_settings_target.is_some() {
        WORKSPACE_SETTINGS_SECTIONS
    } else {
        SettingsSection::ALL
    }
}

fn settings_section_label(app: &AppState, section: SettingsSection) -> &'static str {
    if app.settings.group_settings_target.is_some() && section == SettingsSection::Theme {
        "Appearance"
    } else {
        section.label()
    }
}

fn settings_tab_text(app: &AppState, section: SettingsSection) -> &'static str {
    settings_section_label(app, section)
}

fn settings_tab_width(app: &AppState, section: SettingsSection) -> u16 {
    settings_tab_text(app, section).width() as u16 + 2
}

fn settings_visible_tab_range(app: &AppState, row_width: u16) -> (usize, usize) {
    let sections = settings_sections(app);
    let selected = sections
        .iter()
        .position(|section| *section == app.settings.section)
        .unwrap_or(0);
    super::modal_tabs::visible_tab_range(sections.len(), selected, row_width, |idx| {
        settings_tab_width(app, sections[idx])
    })
}

pub(crate) fn settings_tab_hit_areas(app: &AppState, row: Rect) -> Vec<(SettingsSection, Rect)> {
    let sections = settings_sections(app);
    let (start, end) = settings_visible_tab_range(app, row.width);
    super::modal_tabs::tab_hit_areas(row, start, end, |idx| {
        settings_tab_width(app, sections[idx])
    })
    .into_iter()
    .map(|(idx, rect)| (sections[idx], rect))
    .collect()
}

pub(crate) fn settings_tab_chevron_at(
    app: &AppState,
    row: Rect,
    col: u16,
) -> Option<SettingsSection> {
    let sections = settings_sections(app);
    let (start, end) = settings_visible_tab_range(app, row.width);
    super::modal_tabs::chevron_tab_at(sections.len(), row, col, start, end, |idx| {
        settings_tab_width(app, sections[idx])
    })
    .and_then(|idx| sections.get(idx).copied())
}
#[cfg(test)]
pub(crate) fn settings_tab_hit_areas_for_view(
    client_view: &crate::app::ClientViewState,
    row: Rect,
) -> Vec<(SettingsSection, Rect)> {
    let settings = &client_view.settings;
    let sections = settings_sections_for(settings);
    let (start, end) = settings_visible_tab_range_for(settings, row.width);
    super::modal_tabs::tab_hit_areas(row, start, end, |idx| {
        settings_tab_width_for(settings, sections[idx])
    })
    .into_iter()
    .map(|(idx, rect)| (sections[idx], rect))
    .collect()
}

#[cfg(test)]
pub(crate) fn settings_tab_chevron_at_for_view(
    client_view: &crate::app::ClientViewState,
    row: Rect,
    col: u16,
) -> Option<SettingsSection> {
    let settings = &client_view.settings;
    let sections = settings_sections_for(settings);
    let (start, end) = settings_visible_tab_range_for(settings, row.width);
    super::modal_tabs::chevron_tab_at(sections.len(), row, col, start, end, |idx| {
        settings_tab_width_for(settings, sections[idx])
    })
    .and_then(|idx| sections.get(idx).copied())
}

fn settings_sections_for(
    settings: &crate::app::state::SettingsState,
) -> &'static [SettingsSection] {
    if settings.group_settings_target.is_some() {
        GROUP_SETTINGS_SECTIONS
    } else if settings.workspace_settings_target.is_some() {
        WORKSPACE_SETTINGS_SECTIONS
    } else {
        SettingsSection::ALL
    }
}

fn settings_tab_text_for(
    settings: &crate::app::state::SettingsState,
    section: SettingsSection,
) -> &'static str {
    if settings.group_settings_target.is_some() && section == SettingsSection::Theme {
        "Appearance"
    } else {
        section.label()
    }
}

fn settings_tab_width_for(
    settings: &crate::app::state::SettingsState,
    section: SettingsSection,
) -> u16 {
    settings_tab_text_for(settings, section).width() as u16 + 2
}

fn settings_visible_tab_range_for(
    settings: &crate::app::state::SettingsState,
    row_width: u16,
) -> (usize, usize) {
    let sections = settings_sections_for(settings);
    let selected = sections
        .iter()
        .position(|section| *section == settings.section)
        .unwrap_or(0);
    super::modal_tabs::visible_tab_range(sections.len(), selected, row_width, |idx| {
        settings_tab_width_for(settings, sections[idx])
    })
}

fn settings_palette(app: &AppState) -> crate::app::state::Palette {
    if let Some(group_idx) = app.settings.group_settings_target {
        app.palette_for_group(group_idx)
    } else if let Some(ws_idx) = app.settings.workspace_settings_target {
        app.palette_for_workspace(ws_idx)
    } else {
        app.palette.clone()
    }
}

fn render_settings_tabs(
    app: &AppState,
    frame: &mut Frame,
    row: Rect,
    p: &crate::app::state::Palette,
) {
    let sections = settings_sections(app);
    let (start, end) = settings_visible_tab_range(app, row.width);
    let mut spans = Vec::new();

    if start > 0 {
        spans.push(Span::styled("‹ ", Style::default().fg(p.overlay0)));
    }

    for (visible_idx, section) in sections[start..end].iter().copied().enumerate() {
        if visible_idx > 0 {
            spans.push(Span::raw(" "));
        }

        let selected = section == app.settings.section;
        let tab_style = if selected {
            Style::default()
                .fg(panel_contrast_fg(p))
                .bg(p.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.overlay1)
        };
        spans.push(Span::styled(" ", tab_style));
        spans.push(Span::styled(settings_tab_text(app, section), tab_style));
        spans.push(Span::styled(" ", tab_style));
    }

    if end < sections.len() {
        spans.push(Span::styled(" ›", Style::default().fg(p.overlay0)));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), row);
}
fn render_settings_tabs_for_view(
    client_view: &crate::app::ClientViewState,
    frame: &mut Frame,
    row: Rect,
    p: &crate::app::state::Palette,
) {
    let settings = &client_view.settings;
    let sections = settings_sections_for(settings);
    let (start, end) = settings_visible_tab_range_for(settings, row.width);
    let mut spans = Vec::new();

    if start > 0 {
        spans.push(Span::styled("‹ ", Style::default().fg(p.overlay0)));
    }

    for (visible_idx, section) in sections[start..end].iter().copied().enumerate() {
        if visible_idx > 0 {
            spans.push(Span::raw(" "));
        }

        let selected = section == settings.section;
        let tab_style = if selected {
            Style::default()
                .fg(panel_contrast_fg(p))
                .bg(p.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.overlay1)
        };
        spans.push(Span::styled(" ", tab_style));
        spans.push(Span::styled(
            settings_tab_text_for(settings, section),
            tab_style,
        ));
        spans.push(Span::styled(" ", tab_style));
    }

    if end < sections.len() {
        spans.push(Span::styled(" ›", Style::default().fg(p.overlay0)));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), row);
}

fn render_settings_sidebar(settings: &SettingsState, frame: &mut Frame, area: Rect, p: &Palette) {
    if area.is_empty() {
        return;
    }
    frame.render_widget(
        Paragraph::new("SETTINGS")
            .style(Style::default().fg(p.overlay0).add_modifier(Modifier::BOLD)),
        Rect::new(area.x, area.y, area.width, 1),
    );

    for (entry, row) in settings_sidebar_hit_areas(settings, area) {
        let selected = settings.sidebar_selection.section == entry.section
            && settings.sidebar_selection.subsection == entry.subsection;
        let active = if let Some(subsection) = entry.subsection {
            settings.section == entry.section
                && (settings.sidebar_selection.subsection == Some(subsection)
                    || (settings.sidebar_selection.subsection.is_none() && subsection == 0))
        } else {
            settings.section == entry.section
        };
        let mut style = if active {
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.overlay1)
        };
        if selected && settings.sidebar_focused {
            style = style.bg(p.surface1);
        }

        let text = if entry.subsection.is_some() {
            let marker = if active { "*" } else { " " };
            format!("  {marker} {}", entry.label)
        } else {
            let chevron = if settings.sidebar_expanded == Some(entry.section) {
                "▾"
            } else {
                "▸"
            };
            format!("{chevron} {}", entry.label)
        };
        frame.render_widget(Paragraph::new(text).style(style), row);
    }
}

fn render_settings_sidebar_divider(frame: &mut Frame, area: Rect, p: &Palette) {
    for y in area.y..area.y.saturating_add(area.height) {
        frame.render_widget(
            Paragraph::new("│").style(Style::default().fg(p.surface1)),
            Rect::new(area.x, y, area.width, 1),
        );
    }
}

fn settings_section_title(app: &AppState, section: SettingsSection) -> &'static str {
    if section == SettingsSection::Agents && settings_agents_editor_open(app) {
        if app.settings.pending_agent_profile_id.is_some() {
            "Edit Agent Profile"
        } else {
            "New Agent Profile"
        }
    } else if section == SettingsSection::Connections
        && crate::settings_rows::connection_editor_open(&app.settings)
    {
        let editor = app.settings.connection_editor.as_ref();
        if editor.is_some_and(|editor| editor.is_detail()) {
            "Connection Details"
        } else if editor.is_some_and(|editor| editor.is_editing()) {
            "Edit Connection"
        } else {
            "Add Connection"
        }
    } else {
        match section {
            SettingsSection::Theme => "Appearance",
            SettingsSection::Layout => "Layout",
            SettingsSection::Sound => "Notifications",
            SettingsSection::Toast => "Toasts",
            SettingsSection::PaneLabels => "Behavior",
            SettingsSection::Commands => "Commands",
            SettingsSection::Experiments => "Advanced",
            SettingsSection::Agents => "Agents",
            SettingsSection::Integrations => "Agent Integrations",
            SettingsSection::Connections => "Connections",
            SettingsSection::GroupGeneral => "General",
            SettingsSection::GroupDefaults => "Space Defaults",
            SettingsSection::GroupProfiles => "Agents",
            SettingsSection::GroupGithub => "GitHub",
            SettingsSection::WorkspaceGeneral => "General",
            SettingsSection::WorkspaceGithub => "GitHub",
            SettingsSection::About => "About",
        }
    }
}

fn settings_section_description(app: &AppState, section: SettingsSection) -> &'static str {
    match section {
        SettingsSection::Theme if app.settings.group_settings_target.is_some() => {
            "Choose a theme accent for this group, or inherit the global accent"
        }
        SettingsSection::Theme => "Configure theme, sidebar layout, and pane appearance",
        SettingsSection::Layout => "Set sidebar width bounds",
        SettingsSection::Sound => "Choose sound and toast notification behavior",
        SettingsSection::Toast => "Choose where command and agent notifications are delivered",
        SettingsSection::PaneLabels => {
            "Control workspace prompts and terminal interaction defaults"
        }
        SettingsSection::Commands => "Edit launch commands; clear one to disable and hide it",
        SettingsSection::Experiments => "Configure advanced or platform-specific behavior",
        SettingsSection::Agents if settings_agents_editor_open(app) => {
            "Configure the label, agent type, and launch command"
        }
        SettingsSection::Agents => "Create and manage agent launch profiles",
        SettingsSection::Integrations => "Install hooks so agents report state directly",
        SettingsSection::Connections
            if app
                .settings
                .connection_editor
                .as_ref()
                .is_some_and(|editor| editor.is_detail()) =>
        {
            "Connect, test, or open a workspace on this SSH host"
        }
        SettingsSection::Connections
            if crate::settings_rows::connection_editor_open(&app.settings) =>
        {
            "Credentials and host keys stay with OpenSSH; Gardn never stores them"
        }
        SettingsSection::Connections => "Add SSH hosts and manage their connections",
        SettingsSection::GroupGeneral => "Rename this group, change its icon, or delete it",
        SettingsSection::GroupDefaults => "Choose where new Spaces start",
        SettingsSection::GroupProfiles => {
            "Choose favorite and default agent profiles for this group"
        }
        SettingsSection::GroupGithub => "Spaces inherit this organization. Enter saves.",
        SettingsSection::WorkspaceGeneral => "Set this Space's name, host, and directory",
        SettingsSection::WorkspaceGithub => "Set repository scope. Enter saves the list.",
        SettingsSection::About => "Open-source projects and contributors behind Gardn",
    }
}

pub(crate) fn settings_editor_back_button_rect(app: &AppState, area: Rect) -> Option<Rect> {
    let editor_open = match app.settings.section {
        SettingsSection::Agents => settings_agents_editor_open(app),
        SettingsSection::Connections => crate::settings_rows::connection_editor_open(&app.settings),
        _ => false,
    };
    editor_open.then(|| {
        let width = action_button_width(None, "← Back");
        Rect::new(area.x + area.width.saturating_sub(width), area.y, width, 1)
    })
}

pub(crate) fn settings_section_list_rect(area: Rect) -> Rect {
    let [_, _, list_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Min(2),
    ])
    .areas::<3>(area);
    list_area
}

fn render_settings_section_intro(
    app: &AppState,
    frame: &mut Frame,
    area: Rect,
    section: SettingsSection,
    p: &crate::app::state::Palette,
) -> Rect {
    let [desc_area, divider_area, list_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Min(2),
    ])
    .areas::<3>(area);
    let [title_area, description_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas::<2>(desc_area);
    let title_width = settings_editor_back_button_rect(app, title_area)
        .map(|back| back.x.saturating_sub(title_area.x).saturating_sub(1))
        .unwrap_or(title_area.width);
    render_modal_description(
        frame,
        Rect::new(title_area.x, title_area.y, title_width, title_area.height),
        settings_section_title(app, section),
        Style::default().fg(p.accent),
    );
    if let Some(back) = settings_editor_back_button_rect(app, title_area) {
        render_action_button(frame, back, None, "← Back", secondary_action_style(p));
    }
    render_modal_description(
        frame,
        description_area,
        settings_section_description(app, section),
        Style::default().fg(p.overlay0),
    );
    render_modal_divider(frame, divider_area, p);
    list_area
}

const SETTINGS_INTEGRATIONS_HINTS: &[(&str, &str)] =
    &[("Move", "↑↓"), ("Action", "Space/↵"), ("Section", "←→/Tab")];
const SETTINGS_AGENTS_EDITOR_HINTS: &[(&str, &str)] =
    &[("Move", "↑↓"), ("Action", "Space/↵"), ("Section", "←→/Tab")];
const SETTINGS_AGENTS_HINTS: &[(&str, &str)] = &[
    ("Move", "↑↓"),
    ("New/Edit", "Space/↵"),
    ("Delete", "Ctrl+D"),
    ("Section", "←→/Tab"),
];
const SETTINGS_CONNECTIONS_HINTS: &[(&str, &str)] = &[
    ("Move", "↑↓"),
    ("New/Edit", "Space/↵"),
    ("Delete", "Ctrl+D"),
    ("Section", "←→/Tab"),
];
const SETTINGS_CONNECTIONS_EDITOR_HINTS: &[(&str, &str)] =
    &[("Move", "↑↓"), ("Action", "Space/↵"), ("Section", "←→/Tab")];
const SETTINGS_GROUP_PROFILES_HINTS: &[(&str, &str)] = &[
    ("Move", "↑↓"),
    ("Favorite", "Ctrl+F"),
    ("Default", "Ctrl+D"),
    ("Section", "←→/tab"),
];
const SETTINGS_GROUP_HINTS: &[(&str, &str)] =
    &[("Move", "↑↓"), ("Action", "Space/↵"), ("Section", "←→/Tab")];
const SETTINGS_DEFAULT_HINTS: &[(&str, &str)] =
    &[("Move", "↑↓"), ("Action", "Space/↵"), ("Section", "←→/Tab")];
const SETTINGS_SIDEBAR_HINTS: &[(&str, &str)] =
    &[("Move", "↑↓"), ("Action", "Space/↵"), ("Sidebar", "Tab")];
const SETTINGS_SIDEBAR_AGENTS_HINTS: &[(&str, &str)] = &[
    ("Move", "↑↓"),
    ("New/Edit", "Space/↵"),
    ("Delete", "Ctrl+D"),
    ("Sidebar", "Tab"),
];
const SETTINGS_SIDEBAR_CONNECTIONS_HINTS: &[(&str, &str)] = SETTINGS_SIDEBAR_AGENTS_HINTS;

fn settings_footer_hints(
    app: &AppState,
    _group_settings: bool,
) -> &'static [(&'static str, &'static str)] {
    settings_footer_hints_for(&app.settings)
}

fn general_settings_sidebar_visible(settings: &SettingsState) -> bool {
    settings.group_settings_target.is_none() && settings.workspace_settings_target.is_none()
}

fn settings_stack_areas_for(
    settings: &SettingsState,
    inner: Rect,
) -> super::widgets::ModalStackAreas {
    let footer_rows = modal_hint_line_count(inner.width, settings_footer_hints_for(settings), 2);
    let header_rows = if general_settings_sidebar_visible(settings) {
        1
    } else {
        4
    };
    modal_stack_areas(inner, header_rows, footer_rows, 0, 1)
}

pub(crate) fn settings_stack_areas(app: &AppState, inner: Rect) -> super::widgets::ModalStackAreas {
    settings_stack_areas_for(&app.settings, inner)
}

pub(super) fn render_settings_overlay_for_view(
    app: &AppState,
    client_view: &crate::app::ClientViewState,
    frame: &mut Frame,
    area: Rect,
) {
    render_settings_overlay_with(app, client_view, frame, area);
}

fn render_settings_overlay_with(
    app: &AppState,
    client_view: &crate::app::ClientViewState,
    frame: &mut Frame,
    area: Rect,
) {
    let settings = &client_view.settings;
    let palette = if let Some(group_idx) = settings.group_settings_target {
        app.palette_for_group(group_idx)
    } else if let Some(workspace_idx) = settings.workspace_settings_target {
        app.palette_for_workspace(workspace_idx)
    } else {
        app.palette.clone()
    };
    let title = if settings.group_settings_target.is_some() {
        "Group Settings"
    } else if settings.workspace_settings_target.is_some() {
        "Space Settings"
    } else {
        "Settings"
    };
    super::dim_background(frame, area);
    let Some(frame_areas) = render_modal_frame(
        frame,
        area,
        &palette,
        ModalFrameSpec {
            title,
            width: 92,
            height: 26,
            header_rows: if general_settings_sidebar_visible(settings) {
                1
            } else {
                4
            },
            footer_hints: settings_footer_hints_for(settings),
            footer_max_rows: 2,
            gap: 1,
            actions_rows: 0,
            show_close: true,
        },
    ) else {
        return;
    };
    let inner = frame_areas.inner;
    if inner.height < 4 || inner.width < 10 {
        return;
    }
    let stack = settings_stack_areas_for(settings, inner);
    if general_settings_sidebar_visible(settings) {
        let sidebar = settings_sidebar_areas(stack.content);
        render_settings_sidebar(settings, frame, sidebar.navigation, &palette);
        render_settings_sidebar_divider(frame, sidebar.divider, &palette);
        render_settings_content_for_view(app, client_view, frame, sidebar.content, &palette);
    } else {
        let header_rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas::<4>(stack.header);
        render_settings_tabs_for_view(client_view, frame, header_rows[2], &palette);
        render_modal_divider(frame, header_rows[3], &palette);
        render_settings_content_for_view(app, client_view, frame, stack.content, &palette);
    }
}

fn settings_agents_editor_open_for(settings: &crate::app::state::SettingsState) -> bool {
    settings.pending_agent_profile_id.is_some()
        || settings.pending_agent_profile_name.is_some()
        || settings.pending_agent_profile_command.is_some()
}

fn settings_footer_hints_for(
    settings: &crate::app::state::SettingsState,
) -> &'static [(&'static str, &'static str)] {
    if general_settings_sidebar_visible(settings) {
        return if settings.section == SettingsSection::Agents
            && !settings_agents_editor_open_for(settings)
        {
            SETTINGS_SIDEBAR_AGENTS_HINTS
        } else if settings.section == SettingsSection::Connections
            && !crate::settings_rows::connection_editor_open(settings)
        {
            SETTINGS_SIDEBAR_CONNECTIONS_HINTS
        } else {
            SETTINGS_SIDEBAR_HINTS
        };
    }
    if settings.section == SettingsSection::Integrations {
        SETTINGS_INTEGRATIONS_HINTS
    } else if settings.section == SettingsSection::Agents {
        if settings_agents_editor_open_for(settings) {
            SETTINGS_AGENTS_EDITOR_HINTS
        } else {
            SETTINGS_AGENTS_HINTS
        }
    } else if settings.section == SettingsSection::Connections {
        if crate::settings_rows::connection_editor_open(settings) {
            SETTINGS_CONNECTIONS_EDITOR_HINTS
        } else {
            SETTINGS_CONNECTIONS_HINTS
        }
    } else if settings.section == SettingsSection::GroupProfiles {
        SETTINGS_GROUP_PROFILES_HINTS
    } else if settings.group_settings_target.is_some() {
        SETTINGS_GROUP_HINTS
    } else {
        SETTINGS_DEFAULT_HINTS
    }
}

fn settings_section_title_for(
    settings: &crate::app::state::SettingsState,
    section: SettingsSection,
) -> &'static str {
    if section == SettingsSection::Agents && settings_agents_editor_open_for(settings) {
        if settings.pending_agent_profile_id.is_some() {
            "Edit Agent Profile"
        } else {
            "New Agent Profile"
        }
    } else if section == SettingsSection::Connections
        && crate::settings_rows::connection_editor_open(settings)
    {
        let editor = settings.connection_editor.as_ref();
        if editor.is_some_and(|editor| editor.is_detail()) {
            "Connection Details"
        } else if editor.is_some_and(|editor| editor.is_editing()) {
            "Edit Connection"
        } else {
            "Add Connection"
        }
    } else {
        settings_section_title_for_non_editor(section)
    }
}

fn settings_section_title_for_non_editor(section: SettingsSection) -> &'static str {
    match section {
        SettingsSection::Theme => "Appearance",
        SettingsSection::Layout => "Layout",
        SettingsSection::Sound => "Notifications",
        SettingsSection::Toast => "Toasts",
        SettingsSection::PaneLabels => "Behavior",
        SettingsSection::Commands => "Commands",
        SettingsSection::Experiments => "Advanced",
        SettingsSection::Agents => "Agents",
        SettingsSection::Integrations => "Agent Integrations",
        SettingsSection::Connections => "Connections",
        SettingsSection::GroupGeneral => "General",
        SettingsSection::GroupDefaults => "Space Defaults",
        SettingsSection::GroupProfiles => "Agents",
        SettingsSection::GroupGithub => "GitHub",
        SettingsSection::WorkspaceGeneral => "General",
        SettingsSection::WorkspaceGithub => "GitHub",
        SettingsSection::About => "About",
    }
}

fn settings_section_description_for(
    settings: &crate::app::state::SettingsState,
    section: SettingsSection,
) -> &'static str {
    match section {
        SettingsSection::Theme if settings.group_settings_target.is_some() => {
            "Choose a theme accent for this group, or inherit the global accent"
        }
        SettingsSection::Theme => "Configure theme, sidebar layout, and pane appearance",
        SettingsSection::Layout => "Set sidebar width bounds",
        SettingsSection::Sound => "Choose sound and toast notification behavior",
        SettingsSection::Toast => "Choose where command and agent notifications are delivered",
        SettingsSection::PaneLabels => {
            "Control workspace prompts and terminal interaction defaults"
        }
        SettingsSection::Commands => "Edit launch commands; clear one to disable and hide it",
        SettingsSection::Experiments => "Configure advanced or platform-specific behavior",
        SettingsSection::Agents if settings_agents_editor_open_for(settings) => {
            "Configure the label, agent type, and launch command"
        }
        SettingsSection::Agents => "Create and manage agent launch profiles",
        SettingsSection::Integrations => "Install hooks so agents report state directly",
        SettingsSection::Connections
            if settings
                .connection_editor
                .as_ref()
                .is_some_and(|editor| editor.is_detail()) =>
        {
            "Connect, test, or open a workspace on this SSH host"
        }
        SettingsSection::Connections if crate::settings_rows::connection_editor_open(settings) => {
            "Credentials and host keys stay with OpenSSH; Gardn never stores them"
        }
        SettingsSection::Connections => "Add SSH hosts and manage their connections",
        SettingsSection::GroupGeneral => "Rename this group, change its icon, or delete it",
        SettingsSection::GroupDefaults => "Choose where new Spaces start",
        SettingsSection::GroupProfiles => {
            "Choose favorite and default agent profiles for this group"
        }
        SettingsSection::GroupGithub => "Spaces inherit this organization. Enter saves.",
        SettingsSection::WorkspaceGeneral => "Set this Space's name, host, and directory",
        SettingsSection::WorkspaceGithub => "Set repository scope. Enter saves the list.",
        SettingsSection::About => "Open-source projects and contributors behind Gardn",
    }
}

fn render_settings_section_intro_for_view(
    client_view: &crate::app::ClientViewState,
    frame: &mut Frame,
    area: Rect,
    p: &crate::app::state::Palette,
) -> Rect {
    let settings = &client_view.settings;
    let [desc_area, divider_area, list_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Min(2),
    ])
    .areas::<3>(area);
    let [title_area, description_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas::<2>(desc_area);
    let editor_open = match settings.section {
        SettingsSection::Agents => settings_agents_editor_open_for(settings),
        SettingsSection::Connections => crate::settings_rows::connection_editor_open(settings),
        _ => false,
    };
    let back = editor_open.then(|| {
        let width = action_button_width(None, "← back");
        Rect::new(
            title_area.x + title_area.width.saturating_sub(width),
            title_area.y,
            width,
            1,
        )
    });
    let title_width = back
        .map(|back| back.x.saturating_sub(title_area.x).saturating_sub(1))
        .unwrap_or(title_area.width);
    render_modal_description(
        frame,
        Rect::new(title_area.x, title_area.y, title_width, title_area.height),
        settings_section_title_for(settings, settings.section),
        Style::default().fg(p.accent),
    );
    if let Some(back) = back {
        render_action_button(frame, back, None, "← Back", secondary_action_style(p));
    }
    render_modal_description(
        frame,
        description_area,
        settings_section_description_for(settings, settings.section),
        Style::default().fg(p.overlay0),
    );
    render_modal_divider(frame, divider_area, p);
    list_area
}

fn render_settings_content_for_view(
    app: &AppState,
    client_view: &crate::app::ClientViewState,
    frame: &mut Frame,
    area: Rect,
    p: &crate::app::state::Palette,
) {
    let settings = &client_view.settings;
    let body_area = render_settings_section_intro_for_view(client_view, frame, area, p);
    if settings.section != SettingsSection::Integrations {
        render_settings_rows_for_view(
            rows_for_section_for_view(app, client_view),
            settings,
            frame,
            body_area,
            p,
        );
        return;
    }

    let [list_area, hint_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(2)]).areas::<2>(body_area);
    if app.integration_recommendations.is_empty() && app.ssh_connection_profiles.is_empty() {
        frame.render_widget(
            Paragraph::new(settings_description_line(
                "No integration targets available",
                list_area.width as usize,
                Style::default().fg(p.overlay1),
                false,
            )),
            list_area,
        );
    } else {
        render_settings_rows_for_view(
            rows_for_section_for_view(app, client_view),
            settings,
            frame,
            list_area,
            p,
        );
    }

    let feedback = integration_feedback_for_settings(app, settings);
    if !feedback.is_empty() {
        render_settings_integration_feedback(frame, hint_area, p, feedback);
        return;
    }

    let hint = integration_hint_for_selection(app, settings);
    frame.render_widget(
        Paragraph::new(format!(" {hint}")).style(Style::default().fg(p.overlay0)),
        settings_integration_hint_row(hint_area),
    );
}

fn integration_feedback_for_settings<'a>(
    app: &'a AppState,
    settings: &crate::app::state::SettingsState,
) -> &'a [String] {
    let selection = crate::app::integration_host::resolve(app, settings);
    let Some(host_id) = selection.host_id() else {
        return &app.integration_install_messages;
    };
    app.host_integration_install_messages
        .get(host_id)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn integration_hint_for_selection(
    app: &AppState,
    settings: &crate::app::state::SettingsState,
) -> String {
    let has_host_selector = !app.ssh_connection_profiles.is_empty();
    let selected = settings.list.selected;
    if has_host_selector && selected == 0 {
        return "Press enter to change the integration host".to_string();
    }
    let entry_index = selected.saturating_sub(usize::from(has_host_selector));

    let selection = crate::app::integration_host::resolve(app, settings);
    if let Some(host_id) = selection.host_id() {
        return match app.host_integration_observations.get(host_id) {
            Some(crate::integration::host::HostIntegrationObservation::Ready(snapshot)) => {
                let selected_entry = snapshot.entries.get(entry_index);
                let needs_install = snapshot.entries.iter().any(|entry| {
                    integration_status_needs_install(
                        entry.state,
                        entry.available,
                        entry.missing_profile_hooks,
                    )
                });
                let found_any = snapshot.entries.iter().any(|entry| {
                    entry.available
                        || entry.state != crate::integration::IntegrationStatusKind::NotInstalled
                });
                integration_hint_for_status(
                    selected_entry
                        .map(|entry| (entry.state, entry.available, entry.missing_profile_hooks)),
                    needs_install,
                    found_any,
                )
            }
            Some(crate::integration::host::HostIntegrationObservation::Failed(_)) => {
                "Integration status check failed on the selected host".to_string()
            }
            Some(crate::integration::host::HostIntegrationObservation::Pending) | None => {
                "Waiting for integration status from the selected host".to_string()
            }
        };
    }

    let selected_entry = app
        .integration_recommendations
        .get(entry_index)
        .map(|item| {
            (
                item.state,
                item.available,
                crate::integration::missing_profile_hook_count_for_target(
                    item.target,
                    &app.agent_profiles,
                ),
            )
        });
    let needs_install = app
        .integration_recommendations
        .iter()
        .any(crate::integration::IntegrationRecommendation::needs_install);
    let found_any = app.integration_recommendations.iter().any(|item| {
        item.available || item.state != crate::integration::IntegrationStatusKind::NotInstalled
    });
    integration_hint_for_status(selected_entry, needs_install, found_any)
}

fn integration_hint_for_status(
    selected: Option<(crate::integration::IntegrationStatusKind, bool, usize)>,
    needs_install: bool,
    found_any: bool,
) -> String {
    if let Some((state, available, missing_profile_hooks)) = selected {
        match state {
            crate::integration::IntegrationStatusKind::Current if missing_profile_hooks > 0 => {
                "Press enter to repair profile hooks".to_string()
            }
            crate::integration::IntegrationStatusKind::Current => {
                "Press enter to uninstall selected integration (affects configured profiles)"
                    .to_string()
            }
            crate::integration::IntegrationStatusKind::Outdated => {
                "Press enter to update selected integration".to_string()
            }
            crate::integration::IntegrationStatusKind::NotInstalled if available => {
                "Press enter to install selected integration".to_string()
            }
            crate::integration::IntegrationStatusKind::NotInstalled => {
                "Selected integration is unavailable".to_string()
            }
        }
    } else if needs_install {
        "Press enter to add available or outdated integrations".to_string()
    } else if found_any {
        "All detected integrations are installed".to_string()
    } else {
        "No supported agent CLIs found on PATH".to_string()
    }
}

fn integration_status_needs_install(
    state: crate::integration::IntegrationStatusKind,
    available: bool,
    missing_profile_hooks: usize,
) -> bool {
    state == crate::integration::IntegrationStatusKind::Outdated
        || (state == crate::integration::IntegrationStatusKind::NotInstalled && available)
        || (state == crate::integration::IntegrationStatusKind::Current
            && missing_profile_hooks > 0)
}
fn render_settings_rows_for_view(
    model_rows: Option<Vec<SettingsListRow>>,
    settings: &crate::app::state::SettingsState,
    frame: &mut Frame,
    area: Rect,
    p: &crate::app::state::Palette,
) {
    let Some(model_rows) = model_rows else {
        return;
    };
    let total_items = visual_row_count(&model_rows);
    let viewport =
        crate::ui::ModalListViewport::new(total_items, area.height as usize, settings.scroll);
    let scroll = viewport.scroll();
    let scroll_area = viewport.scroll_area(area);
    let list_width = scroll_area.body.width as usize;
    let mut selected_row = None;
    let mut rows = Vec::with_capacity(total_items);
    for row in &model_rows {
        let selected_index = match row {
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
        };
        let selected =
            settings.list.visible() == selected_index || settings.focused_input == selected_index;
        if selected {
            selected_row = Some(rows.len());
        }
        let style = if selected {
            modal_option_style(p, true)
        } else {
            Style::default().fg(p.text)
        };
        match row {
            SettingsListRow::Header(title) => rows.push(ListItem::new(Line::from(Span::styled(
                format!(" {title}"),
                modal_section_heading_style(p),
            )))),
            SettingsListRow::Caption(text) => rows.push(ListItem::new(Line::from(Span::styled(
                format!(" {text}"),
                Style::default().fg(p.subtext0),
            )))),
            SettingsListRow::Spacer => rows.push(ListItem::new(Line::from(""))),
            SettingsListRow::GroupIconPicker => {
                rows.extend(group_icon_picker_list_items(
                    group_settings_picker_icon(settings),
                    p,
                ));
            }

            SettingsListRow::Toggle {
                title,
                description,
                enabled,
                ..
            } => {
                let value_style = if selected {
                    style.add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(p.accent)
                };
                rows.push(ListItem::new(settings_title_value_line(
                    title,
                    if *enabled { "On" } else { "Off" },
                    list_width,
                    style,
                    value_style,
                    selected,
                )));
                rows.push(ListItem::new(settings_setting_description_line(
                    description,
                    list_width,
                    if selected {
                        style
                    } else {
                        Style::default().fg(p.subtext0)
                    },
                    selected,
                )));
            }
            SettingsListRow::Value {
                index,
                title,
                description,
                value,
                editable,
            } => {
                let value_style = if selected {
                    style.add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(p.accent)
                };
                let edited_value = (*editable && settings.focused_input == Some(*index))
                    .then(|| format!("{value}█"));
                let displayed_value = edited_value.as_deref().unwrap_or(value.as_ref());
                rows.push(ListItem::new(settings_title_value_line(
                    title,
                    displayed_value,
                    list_width,
                    style,
                    value_style,
                    selected,
                )));
                rows.push(ListItem::new(settings_setting_description_line(
                    description,
                    list_width,
                    if selected {
                        style
                    } else {
                        Style::default().fg(p.subtext0)
                    },
                    selected,
                )));
            }
            SettingsListRow::TextInput { title, value, .. } => {
                if selected {
                    selected_row = Some(rows.len() + 1);
                }
                rows.push(ListItem::new(settings_description_line(
                    title,
                    list_width,
                    Style::default().fg(p.text),
                    false,
                )));
                let input_value = if selected {
                    format!("{value}█")
                } else {
                    value.to_string()
                };
                rows.push(ListItem::new(settings_description_line(
                    &input_value,
                    list_width,
                    Style::default().fg(p.text).bg(p.surface0),
                    false,
                )));
            }
            SettingsListRow::Choice { label, checked, .. } => {
                let check_style = if selected {
                    style
                } else if *checked {
                    Style::default().fg(p.accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(p.overlay0)
                };
                rows.push(ListItem::new(settings_choice_line(
                    label,
                    *checked,
                    list_width,
                    style,
                    check_style,
                    selected,
                )))
            }
            SettingsListRow::Action {
                icon, label, tone, ..
            } => {
                let action_style = if selected && *tone == SettingsMarkerTone::Danger {
                    settings_danger_selected_style(p)
                } else if selected {
                    style
                } else if *tone == SettingsMarkerTone::Danger {
                    settings_marker_style(p, *tone)
                } else {
                    Style::default().fg(p.text)
                };
                rows.push(ListItem::new(settings_action_line(
                    icon,
                    label,
                    list_width,
                    action_style,
                    selected,
                )));
            }
            SettingsListRow::Status {
                label,
                status,
                tone,
                ..
            } => {
                let status_style = if selected {
                    style.add_modifier(Modifier::BOLD)
                } else {
                    settings_marker_style(p, *tone)
                };
                rows.push(ListItem::new(settings_status_line(
                    label,
                    status,
                    list_width,
                    style,
                    status_style,
                    selected,
                )))
            }
            SettingsListRow::Profile {
                name,
                detail,
                badge,
                tone,
                ..
            } => {
                let detail_style = if selected {
                    style
                } else {
                    Style::default().fg(p.subtext0)
                };
                let badge_style = if selected {
                    style.add_modifier(Modifier::BOLD)
                } else {
                    settings_marker_style(p, *tone).add_modifier(Modifier::BOLD)
                };
                rows.push(ListItem::new(settings_profile_name_line(
                    name,
                    detail,
                    badge.as_deref(),
                    list_width,
                    style,
                    detail_style,
                    badge_style,
                    selected,
                )));
            }
        }
    }
    let selected = selected_row
        .and_then(|row| (row >= scroll && row < scroll + area.height as usize).then_some(row));
    let mut state = ListState::default()
        .with_selected(selected)
        .with_offset(scroll);
    frame.render_stateful_widget(List::new(rows), scroll_area.body, &mut state);
    if let Some(track) = scroll_area.track {
        render_scrollbar(
            frame,
            viewport.metrics(),
            track,
            p.surface_dim,
            p.overlay0,
            "▐",
        );
    }
}

pub(super) fn render_settings_overlay(app: &AppState, frame: &mut Frame, area: Rect) {
    use crate::app::state::SettingsSection;

    let group_settings = app.settings.group_settings_target.is_some();

    let palette = settings_palette(app);
    let p = &palette;
    super::dim_background(frame, area);

    let Some(frame_areas) = render_modal_frame(
        frame,
        area,
        p,
        ModalFrameSpec {
            title: settings_title(app),
            width: 92,
            height: 26,
            header_rows: if general_settings_sidebar_visible(&app.settings) {
                1
            } else {
                4
            },
            footer_hints: settings_footer_hints(app, group_settings),
            footer_max_rows: 2,
            gap: 1,
            actions_rows: 0,
            show_close: true,
        },
    ) else {
        return;
    };
    let inner = frame_areas.inner;
    if inner.height < 4 || inner.width < 10 {
        return;
    }

    let stack = settings_stack_areas(app, inner);
    let content_area = if general_settings_sidebar_visible(&app.settings) {
        let sidebar = settings_sidebar_areas(stack.content);
        render_settings_sidebar(&app.settings, frame, sidebar.navigation, p);
        render_settings_sidebar_divider(frame, sidebar.divider, p);
        sidebar.content
    } else {
        let header_rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas::<4>(stack.header);
        render_settings_tabs(app, frame, header_rows[2], p);
        render_modal_divider(frame, header_rows[3], p);
        stack.content
    };

    match app.settings.section {
        SettingsSection::Theme
        | SettingsSection::Layout
        | SettingsSection::Sound
        | SettingsSection::Toast
        | SettingsSection::PaneLabels
        | SettingsSection::Commands
        | SettingsSection::Experiments
        | SettingsSection::Agents
        | SettingsSection::Connections
        | SettingsSection::GroupGeneral
        | SettingsSection::GroupDefaults
        | SettingsSection::GroupProfiles
        | SettingsSection::GroupGithub
        | SettingsSection::WorkspaceGeneral
        | SettingsSection::WorkspaceGithub
        | SettingsSection::About => {
            render_settings_sectioned_toggle_list(app, frame, content_area, p);
        }
        SettingsSection::Integrations => render_settings_integrations(app, frame, content_area, p),
    }
}

fn settings_agents_editor_open(app: &AppState) -> bool {
    app.settings.pending_agent_profile_id.is_some()
        || app.settings.pending_agent_profile_name.is_some()
        || app.settings.pending_agent_profile_command.is_some()
}

pub(crate) fn settings_close_button_rect(inner: Rect) -> Rect {
    let header = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas::<4>(modal_stack_areas(inner, 4, 1, 0, 1).header)[0];
    super::widgets::modal_close_button_rect(header)
}

#[cfg(test)]
fn render_settings_theme(app: &AppState, frame: &mut Frame, area: Rect) {
    let palette = settings_palette(app);
    render_settings_sectioned_toggle_list(app, frame, area, &palette);
}

fn render_settings_integrations(
    app: &AppState,
    frame: &mut Frame,
    area: Rect,
    p: &crate::app::state::Palette,
) {
    let body_area =
        render_settings_section_intro(app, frame, area, SettingsSection::Integrations, p);
    let [list_area, hint_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(2)]).areas::<2>(body_area);

    if app.integration_recommendations.is_empty() && app.ssh_connection_profiles.is_empty() {
        frame.render_widget(
            Paragraph::new(settings_description_line(
                "No integration targets available",
                list_area.width as usize,
                Style::default().fg(p.overlay1),
                false,
            )),
            list_area,
        );
    } else {
        render_settings_rows(app, frame, list_area, p);
    }

    let feedback = integration_feedback_for_settings(app, &app.settings);
    if !feedback.is_empty() {
        render_settings_integration_feedback(frame, hint_area, p, feedback);
        return;
    }

    let hint = integration_hint_for_selection(app, &app.settings);
    frame.render_widget(
        Paragraph::new(format!(" {hint}")).style(Style::default().fg(p.overlay0)),
        settings_integration_hint_row(hint_area),
    );
}

fn settings_integration_hint_row(area: Rect) -> Rect {
    if area.height > 1 {
        Rect::new(area.x, area.y + 1, area.width, 1)
    } else {
        area
    }
}

fn render_settings_integration_feedback(
    frame: &mut Frame,
    area: Rect,
    p: &crate::app::state::Palette,
    messages: &[String],
) {
    let Some(first) = messages.first() else {
        return;
    };
    let (label, accent, text_style, text) =
        if let Some(warning) = first.strip_prefix(crate::integration::INSTALL_WARNING_PREFIX) {
            (
                " Warning ",
                p.yellow,
                Style::default().fg(p.text),
                warning.trim_start().to_string(),
            )
        } else if first.contains(": ") {
            (
                " Error ",
                p.red,
                Style::default().fg(p.text),
                first.to_string(),
            )
        } else {
            (
                " Hint ",
                p.green,
                Style::default().fg(p.subtext0),
                first.to_string(),
            )
        };
    let mut spans = vec![
        Span::raw(" "),
        Span::styled(
            label,
            Style::default()
                .fg(panel_contrast_fg(p))
                .bg(accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(text, text_style),
    ];
    if messages.len() > 1 {
        spans.push(Span::styled(
            format!(" · {} More", messages.len() - 1),
            Style::default().fg(p.overlay1),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)),
        settings_integration_hint_row(area),
    );
}

fn render_settings_sectioned_toggle_list(
    app: &AppState,
    frame: &mut Frame,
    area: Rect,
    p: &crate::app::state::Palette,
) {
    let body_area = render_settings_section_intro(app, frame, area, app.settings.section, p);
    render_settings_rows(app, frame, body_area, p);
}

const SETTINGS_BODY_INDENT: usize = 2;

fn group_icon_picker_list_items(
    selected_icon: &str,
    p: &crate::app::state::Palette,
) -> Vec<ListItem<'static>> {
    let row_count = crate::ui::group_icon_picker_row_count() as usize;
    (0..row_count)
        .map(|row| {
            let mut spans = vec![Span::raw(" ".repeat(SETTINGS_BODY_INDENT))];
            for col in 0..5 {
                let idx = row * 5 + col;
                let Some(icon) = crate::app::state::GROUP_ICONS.get(idx) else {
                    break;
                };
                let selected = *icon == selected_icon;
                spans.push(Span::styled(
                    format!(" {icon} "),
                    if selected {
                        Style::default().fg(panel_contrast_fg(p)).bg(p.accent)
                    } else {
                        Style::default().fg(p.text).bg(p.surface0)
                    },
                ));
                spans.push(Span::raw(" "));
            }
            ListItem::new(Line::from(spans))
        })
        .collect()
}

fn group_settings_picker_icon(settings: &SettingsState) -> &str {
    settings
        .pending_group_icon
        .as_deref()
        .unwrap_or(crate::app::state::DEFAULT_GROUP_ICON)
}

const SETTINGS_DESCRIPTION_INDENT: usize = 4;

fn settings_body_width(width: usize) -> usize {
    width.saturating_sub(SETTINGS_BODY_INDENT)
}

fn settings_description_width(width: usize) -> usize {
    width.saturating_sub(SETTINGS_DESCRIPTION_INDENT)
}

fn settings_padded_text(text: &str, width: usize) -> String {
    format!("{text:<width$}")
}

fn settings_title_value_line(
    title: &str,
    value: &str,
    width: usize,
    title_style: Style,
    value_style: Style,
    selected: bool,
) -> Line<'static> {
    let body_width = settings_body_width(width);
    let title_width = title.width();
    let value_width = value.width();
    let gap = if body_width > title_width + value_width {
        body_width - title_width - value_width
    } else {
        1
    };
    let filler_style = if selected {
        title_style
    } else {
        Style::default()
    };
    Line::from(vec![
        Span::styled(" ".repeat(SETTINGS_BODY_INDENT), filler_style),
        Span::styled(title.to_string(), title_style),
        Span::styled(" ".repeat(gap), filler_style),
        Span::styled(value.to_string(), value_style),
    ])
}

fn settings_description_line(
    text: &str,
    width: usize,
    style: Style,
    selected: bool,
) -> Line<'static> {
    let body_width = settings_body_width(width);
    let content = settings_padded_text(text, body_width);
    let filler_style = if selected { style } else { Style::default() };
    Line::from(vec![
        Span::styled(" ".repeat(SETTINGS_BODY_INDENT), filler_style),
        Span::styled(content, style),
    ])
}

fn settings_setting_description_line(
    text: &str,
    width: usize,
    style: Style,
    selected: bool,
) -> Line<'static> {
    let body_width = settings_description_width(width);
    let content = settings_padded_text(text, body_width);
    let filler_style = if selected { style } else { Style::default() };
    Line::from(vec![
        Span::styled(" ".repeat(SETTINGS_DESCRIPTION_INDENT), filler_style),
        Span::styled(content, style),
    ])
}

fn settings_status_line(
    label: &str,
    status: &str,
    width: usize,
    label_style: Style,
    status_style: Style,
    selected: bool,
) -> Line<'static> {
    settings_title_value_line(label, status, width, label_style, status_style, selected)
}

fn settings_action_line(
    icon: &str,
    label: &str,
    width: usize,
    style: Style,
    selected: bool,
) -> Line<'static> {
    let body_width = settings_body_width(width);
    let text = if icon.is_empty() {
        label.to_string()
    } else {
        format!("{icon} {label}")
    };
    let content = settings_padded_text(&text, body_width);
    let filler_style = if selected { style } else { Style::default() };
    Line::from(vec![
        Span::styled(" ".repeat(SETTINGS_BODY_INDENT), filler_style),
        Span::styled(content, style),
    ])
}

fn settings_choice_line(
    label: &str,
    checked: bool,
    width: usize,
    label_style: Style,
    check_style: Style,
    selected: bool,
) -> Line<'static> {
    let body_width = settings_body_width(width);
    let marker = if checked { "✓" } else { " " };
    let label_width = label.width();
    let padding = body_width.saturating_sub(2 + label_width);
    let filler_style = if selected {
        label_style
    } else {
        Style::default()
    };
    Line::from(vec![
        Span::styled(" ".repeat(SETTINGS_BODY_INDENT), filler_style),
        Span::styled(marker.to_string(), check_style),
        Span::styled(" ", filler_style),
        Span::styled(label.to_string(), label_style),
        Span::styled(" ".repeat(padding), filler_style),
    ])
}

fn settings_profile_name_line(
    name: &str,
    detail: &str,
    badge: Option<&str>,
    width: usize,
    name_style: Style,
    detail_style: Style,
    badge_style: Style,
    selected: bool,
) -> Line<'static> {
    let badge = badge.unwrap_or("");
    let detail_text = if detail.is_empty() {
        String::new()
    } else {
        format!(" · {detail}")
    };
    let body_width = settings_body_width(width);
    let used_width = name.width() + detail_text.width() + badge.width();
    let gap = if body_width > used_width {
        body_width - used_width
    } else {
        1
    };
    let filler_style = if selected {
        name_style
    } else {
        Style::default()
    };
    Line::from(vec![
        Span::styled(" ".repeat(SETTINGS_BODY_INDENT), filler_style),
        Span::styled(name.to_string(), name_style),
        Span::styled(detail_text, detail_style),
        Span::styled(" ".repeat(gap), filler_style),
        Span::styled(badge.to_string(), badge_style),
    ])
}

fn render_settings_rows(
    app: &AppState,
    frame: &mut Frame,
    area: Rect,
    p: &crate::app::state::Palette,
) {
    let selected_style = modal_option_style(p, true);

    let Some(model_rows) = rows_for_section(app, app.settings.section) else {
        return;
    };

    let total_items = visual_row_count(&model_rows);
    let viewport =
        crate::ui::ModalListViewport::new(total_items, area.height as usize, app.settings.scroll);
    let scroll = viewport.scroll();
    let scroll_area = viewport.scroll_area(area);
    let list_width = scroll_area.body.width as usize;
    let viewport_rows = area.height as usize;
    let mut selected_row = None;
    let mut rows = Vec::with_capacity(total_items);

    for row in &model_rows {
        match row {
            SettingsListRow::Header(title) => {
                rows.push(ListItem::new(Line::from(Span::styled(
                    format!(" {title}"),
                    modal_section_heading_style(p),
                ))));
            }
            SettingsListRow::Caption(text) => {
                rows.push(ListItem::new(Line::from(Span::styled(
                    format!(" {text}"),
                    Style::default().fg(p.subtext0),
                ))));
            }
            SettingsListRow::Spacer => rows.push(ListItem::new(Line::from(""))),
            SettingsListRow::GroupIconPicker => {
                rows.extend(group_icon_picker_list_items(
                    group_settings_picker_icon(&app.settings),
                    p,
                ));
            }
            SettingsListRow::Toggle {
                index,
                title,
                description,
                enabled,
            } => {
                let selected = app.settings.list.visible() == Some(*index)
                    || app.settings.focused_input == Some(*index);
                if selected {
                    selected_row = Some(rows.len());
                }
                let value = if *enabled { "On" } else { "Off" };
                let label_style = if selected {
                    selected_style
                } else {
                    Style::default().fg(p.text)
                };
                let value_style = if selected {
                    selected_style.add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(p.accent)
                };
                rows.push(ListItem::new(settings_title_value_line(
                    title,
                    value,
                    list_width,
                    label_style,
                    value_style,
                    selected,
                )));
                rows.push(ListItem::new(settings_setting_description_line(
                    description,
                    list_width,
                    if selected {
                        selected_style
                    } else {
                        Style::default().fg(p.subtext0)
                    },
                    selected,
                )));
            }
            SettingsListRow::Value {
                index,
                title,
                description,
                value,
                editable,
            } => {
                let selected = app.settings.list.visible() == Some(*index)
                    || app.settings.focused_input == Some(*index);
                if selected {
                    selected_row = Some(rows.len());
                }
                let label_style = if selected {
                    selected_style
                } else {
                    Style::default().fg(p.text)
                };
                let value_style = if selected {
                    selected_style.add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(p.accent)
                };
                let edited_value = (*editable && app.settings.focused_input == Some(*index))
                    .then(|| format!("{value}█"));
                let displayed_value = edited_value.as_deref().unwrap_or(value.as_ref());
                rows.push(ListItem::new(settings_title_value_line(
                    title,
                    displayed_value,
                    list_width,
                    label_style,
                    value_style,
                    selected,
                )));
                rows.push(ListItem::new(settings_setting_description_line(
                    description,
                    list_width,
                    if selected {
                        selected_style
                    } else {
                        Style::default().fg(p.subtext0)
                    },
                    selected,
                )));
            }
            SettingsListRow::TextInput {
                index,
                title,
                value,
            } => {
                let selected = app.settings.list.visible() == Some(*index)
                    || app.settings.focused_input == Some(*index);
                if selected {
                    selected_row = Some(rows.len() + 1);
                }
                rows.push(ListItem::new(settings_description_line(
                    title,
                    list_width,
                    Style::default().fg(p.text),
                    false,
                )));
                let input_value = if selected {
                    format!("{value}█")
                } else {
                    value.to_string()
                };
                let input_style = Style::default().fg(p.text).bg(p.surface0);
                rows.push(ListItem::new(settings_description_line(
                    &input_value,
                    list_width,
                    input_style,
                    false,
                )));
            }
            SettingsListRow::Choice {
                index,
                label,
                checked,
            } => {
                let selected = app.settings.list.visible() == Some(*index)
                    || app.settings.focused_input == Some(*index);
                if selected {
                    selected_row = Some(rows.len());
                }
                let label_style = if selected {
                    selected_style
                } else {
                    Style::default().fg(p.text)
                };
                let check_style = if selected {
                    selected_style
                } else if *checked {
                    Style::default().fg(p.accent).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(p.overlay0)
                };
                rows.push(ListItem::new(settings_choice_line(
                    label,
                    *checked,
                    list_width,
                    label_style,
                    check_style,
                    selected,
                )));
            }
            SettingsListRow::Action {
                index,
                icon,
                label,
                tone,
            } => {
                let selected = app.settings.list.visible() == Some(*index)
                    || app.settings.focused_input == Some(*index);
                if selected {
                    selected_row = Some(rows.len());
                }
                let style = if selected && *tone == SettingsMarkerTone::Danger {
                    settings_danger_selected_style(p)
                } else if selected {
                    selected_style
                } else if *tone == SettingsMarkerTone::Danger {
                    settings_marker_style(p, *tone)
                } else {
                    Style::default().fg(p.text)
                };
                rows.push(ListItem::new(settings_action_line(
                    icon, label, list_width, style, selected,
                )));
            }
            SettingsListRow::Status {
                index,
                label,
                status,
                tone,
            } => {
                let selected = app.settings.list.visible() == Some(*index)
                    || app.settings.focused_input == Some(*index);
                if selected {
                    selected_row = Some(rows.len());
                }
                let label_style = if selected {
                    selected_style
                } else {
                    Style::default().fg(p.text)
                };
                let status_style = if selected {
                    selected_style.add_modifier(Modifier::BOLD)
                } else {
                    settings_marker_style(p, *tone)
                };
                rows.push(ListItem::new(settings_status_line(
                    label,
                    status,
                    list_width,
                    label_style,
                    status_style,
                    selected,
                )));
            }
            SettingsListRow::Profile {
                index,
                name,
                detail,
                badge,
                tone,
            } => {
                let selected = app.settings.list.visible() == Some(*index)
                    || app.settings.focused_input == Some(*index);
                if selected {
                    selected_row = Some(rows.len());
                }
                let name_style = if selected {
                    selected_style
                } else {
                    Style::default().fg(p.text)
                };
                let detail_style = if selected {
                    selected_style
                } else {
                    Style::default().fg(p.subtext0)
                };
                let badge_style = if selected {
                    selected_style.add_modifier(Modifier::BOLD)
                } else {
                    settings_marker_style(p, *tone).add_modifier(Modifier::BOLD)
                };
                rows.push(ListItem::new(settings_profile_name_line(
                    name,
                    detail,
                    badge.as_deref(),
                    list_width,
                    name_style,
                    detail_style,
                    badge_style,
                    selected,
                )));
            }
        }
    }

    let selected =
        selected_row.and_then(|row| (row >= scroll && row < scroll + viewport_rows).then_some(row));

    let list = List::new(rows);
    let mut state = ListState::default()
        .with_selected(selected)
        .with_offset(scroll);
    frame.render_stateful_widget(list, scroll_area.body, &mut state);
    if let Some(track) = scroll_area.track {
        render_scrollbar(
            frame,
            viewport.metrics(),
            track,
            p.surface_dim,
            p.overlay0,
            "▐",
        );
    }
}

fn modal_option_style(p: &Palette, selected: bool) -> Style {
    if selected {
        Style::default()
            .fg(panel_contrast_fg(p))
            .bg(p.accent)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(p.text)
    }
}

fn settings_marker_style(p: &Palette, tone: SettingsMarkerTone) -> Style {
    match tone {
        SettingsMarkerTone::Good => Style::default().fg(p.green),
        SettingsMarkerTone::Warning => Style::default().fg(p.yellow),
        SettingsMarkerTone::Accent => Style::default().fg(p.accent),
        SettingsMarkerTone::Danger => Style::default().fg(p.red).add_modifier(Modifier::BOLD),
        SettingsMarkerTone::Disabled => Style::default().fg(p.overlay0),
    }
}

fn settings_danger_selected_style(p: &Palette) -> Style {
    Style::default()
        .fg(panel_contrast_fg(p))
        .bg(p.red)
        .add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

    use super::*;
    use crate::app::state::{AppState, SettingsSection};
    use crate::{
        app::state::theme_names_for_appearance,
        config::{TerminalAccent, ToastDelivery},
        terminal_theme::ThemeAppearance,
    };

    #[test]
    fn group_settings_overlay_uses_main_settings_layout_with_group_tabs() {
        let mut app = AppState::test_new();
        let group_idx = app.create_group("Work".to_string());
        app.settings.group_settings_target = Some(group_idx);
        app.settings.section = SettingsSection::Theme;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, Rect::new(0, 0, 80, 24)))
            .expect("render group settings overlay");

        let text = buffer_text(terminal.backend().buffer(), 80, 24);
        assert!(text.contains("Group Settings"));
        assert!(text.contains("Appearance"));
        assert!(text.contains("General"));
        assert!(text.contains("Accent"));
        assert!(!text.contains("Sound"));
    }

    #[test]
    fn commands_settings_show_edit_cursor_in_input_field() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Commands;
        app.settings.pending_review_command = Some("hunk diff --watch".to_string());
        app.settings.list.select(1);
        app.settings.list.show();
        app.settings.focused_input = Some(1);

        let area = Rect::new(0, 0, 100, 40);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render Commands settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(text.contains("hunk diff --watch█"));
    }

    #[test]
    fn commands_settings_mark_an_empty_input_disabled() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Commands;
        app.settings.pending_review_command = Some(String::new());

        let area = Rect::new(0, 0, 100, 40);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render Commands settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(text.contains("Review · Review UI · Disabled"));
        assert!(text.contains("Edit launch commands; clear one to disable and hide it"));
    }

    #[test]
    fn group_general_settings_lists_name_and_danger_actions() {
        let mut app = AppState::test_new();
        let group_idx = app.create_group("Work".to_string());
        app.settings.group_settings_target = Some(group_idx);
        app.settings.section = SettingsSection::GroupGeneral;

        let backend = TestBackend::new(80, 60);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, Rect::new(0, 0, 80, 60)))
            .expect("render group settings overlay");

        let text = buffer_text(terminal.backend().buffer(), 80, 60);
        assert!(text.contains("Name"));
        assert!(text.contains("Work"));
        assert!(text.contains("Icon"));
        assert!(text.contains("Danger Zone"));
        assert!(text.contains("× Delete Group"));
        assert!(!text.contains("Default Location for New Spaces"));
        assert!(!text.contains("Directory"));
        assert!(!text.contains("GitHub Organization"));
    }

    #[test]
    fn client_group_general_input_shows_focus_without_highlighting_its_label() {
        let mut app = AppState::test_new();
        let group_idx = app.create_group("Work".to_string());
        let mut client_view = crate::app::ClientViewState::from_default_client_state(&app);
        client_view.settings.group_settings_target = Some(group_idx);
        client_view.settings.section = SettingsSection::GroupGeneral;
        client_view.settings.pending_group_name = Some("Work".to_string());
        client_view.settings.list.selected = 0;
        client_view.settings.list.show();

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| {
                render_settings_overlay_for_view(
                    &app,
                    &client_view,
                    frame,
                    Rect::new(0, 0, 80, 24),
                );
            })
            .expect("render client group settings overlay");

        let buffer = terminal.backend().buffer();
        let text = buffer_text(buffer, 80, 24);
        let (label_y, label_x) = find_text_cell(&text, "Name").expect("name label");
        let (input_y, input_x) = find_text_cell(&text, "Work█").expect("focused name input");
        assert_ne!(
            buffer[(label_x, label_y)].style().bg,
            Some(app.palette.accent)
        );
        assert_eq!(
            buffer[(input_x, input_y)].style().bg,
            Some(app.palette.surface0)
        );
    }

    #[test]
    fn group_general_delete_action_uses_red_text_and_hover_background() {
        let mut app = AppState::test_new();
        let group_idx = app.create_group("Work".to_string());
        app.settings.group_settings_target = Some(group_idx);
        app.settings.section = SettingsSection::GroupGeneral;
        app.settings.scroll = 0;

        let backend = TestBackend::new(80, 60);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, Rect::new(0, 0, 80, 60)))
            .expect("render group settings overlay");

        let buffer = terminal.backend().buffer();
        let text = buffer_text(buffer, 80, 60);
        let (y, x) = find_text_cell(&text, "× Delete Group").expect("delete action row");
        assert_eq!(buffer[(x, y)].style().fg, Some(app.palette.red));
        assert_eq!(buffer[(x + 2, y)].style().fg, Some(app.palette.red));
        assert_ne!(buffer[(x, y)].style().bg, Some(app.palette.red));

        app.settings.list.selected = crate::settings_rows::GROUP_GENERAL_DELETE;
        app.settings.list.show();
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, Rect::new(0, 0, 80, 60)))
            .expect("render group settings overlay");

        let buffer = terminal.backend().buffer();
        let text = buffer_text(buffer, 80, 60);
        let (y, x) = find_text_cell(&text, "× Delete Group").expect("delete action row");
        assert_eq!(buffer[(x, y)].style().bg, Some(app.palette.red));
        assert_eq!(buffer[(x + 2, y)].style().bg, Some(app.palette.red));
        assert_eq!(
            buffer[(x, y)].style().fg,
            Some(panel_contrast_fg(&app.palette))
        );
    }

    #[test]
    fn workspace_settings_overlay_uses_workspace_group_accent() {
        let mut app = AppState::test_new();
        app.workspaces = vec![crate::workspace::Workspace::test_new("Space")];
        let group_idx = app.create_group("Work".to_string());
        app.move_workspace_to_group(0, group_idx);
        app.set_group_accent(group_idx, Some(TerminalAccent::Red));
        app.settings.workspace_settings_target = Some(0);
        app.settings.section = SettingsSection::WorkspaceGeneral;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, Rect::new(0, 0, 80, 24)))
            .expect("render workspace settings overlay");

        let frame_areas = super::super::widgets::modal_frame_areas(
            Rect::new(0, 0, 80, 24),
            ModalFrameSpec {
                title: settings_title(&app),
                width: 92,
                height: 26,
                header_rows: 4,
                footer_hints: settings_footer_hints(&app, false),
                footer_max_rows: 2,
                gap: 1,
                actions_rows: 0,
                show_close: true,
            },
        )
        .expect("settings frame areas");
        let buffer = terminal.backend().buffer();
        assert_eq!(
            buffer[(frame_areas.popup.x, frame_areas.popup.y)]
                .style()
                .fg,
            Some(app.group_accent_color(group_idx))
        );
    }

    #[test]
    fn theme_settings_light_mode_only_lists_light_themes() {
        let mut app = AppState::test_new();
        app.global_theme_mode = ThemeMode::Light;
        app.settings.section = SettingsSection::Theme;
        app.settings.pending_theme_mode = Some(ThemeMode::Light);
        app.settings.pending_light_theme_name = Some("catppuccin-latte".to_string());

        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_theme(&app, frame, Rect::new(0, 0, 100, 50)))
            .expect("render theme settings");

        let text = buffer_text(terminal.backend().buffer(), 100, 50);
        assert!(text.contains("Catppuccin Latte"));
        assert!(text.contains("Solarized"));
        assert!(!text.contains("Dracula"));
        assert!(!text.contains("Nord"));
        assert!(!text.contains("Vesper"));
        assert_no_option_line(&text, "Terminal");
    }

    #[test]
    fn theme_settings_dark_mode_only_lists_dark_themes() {
        let mut app = AppState::test_new();
        app.global_theme_mode = ThemeMode::Dark;
        app.settings.section = SettingsSection::Theme;
        app.settings.pending_theme_mode = Some(ThemeMode::Dark);
        app.settings.pending_dark_theme_name = Some("catppuccin".to_string());

        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_theme(&app, frame, Rect::new(0, 0, 100, 50)))
            .expect("render theme settings");

        let text = buffer_text(terminal.backend().buffer(), 100, 50);
        assert!(text.contains("Catppuccin Mocha"));
        assert!(text.contains("Dracula"));
        assert!(!text.contains("Catppuccin Latte"));
        assert_no_option_line(&text, "Terminal");
    }

    #[test]
    fn theme_settings_system_source_hides_appearance_sections() {
        let mut app = AppState::test_new();
        app.global_theme_mode = ThemeMode::System;
        app.settings.section = SettingsSection::Theme;
        app.settings.pending_theme_mode = Some(ThemeMode::System);
        app.settings.pending_light_theme_name = Some("System".to_string());
        app.settings.pending_dark_theme_name = Some("System".to_string());

        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_theme(&app, frame, Rect::new(0, 0, 100, 50)))
            .expect("render theme settings");

        let text = buffer_text(terminal.backend().buffer(), 100, 50);
        assert!(text.contains("✓ Terminal"));
        assert!(text.contains("Palettes"));
        assert!(text.contains("Accent"));
        assert!(text.contains("✓ Blue"));
        assert!(text.contains("Magenta"));
        assert!(text.contains("Colors"));
        assert!(!text.contains("Light Appearance"));
        assert!(!text.contains("Dark Appearance"));
    }

    #[test]
    fn theme_settings_system_mode_lists_light_and_dark_selections() {
        let mut app = AppState::test_new();
        app.global_theme_mode = ThemeMode::System;
        app.settings.section = SettingsSection::Theme;
        app.settings.pending_theme_mode = Some(ThemeMode::System);
        app.settings.pending_light_theme_name = Some("solarized-light".to_string());
        app.settings.pending_dark_theme_name = Some("rose-pine".to_string());

        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_theme(&app, frame, Rect::new(0, 0, 100, 50)))
            .expect("render theme settings");

        let text = buffer_text(terminal.backend().buffer(), 100, 50);
        assert!(text.contains("Terminal"));
        assert!(text.contains("✓ Palettes"));
        assert!(text.contains("Appearance"));
        assert!(text.contains("✓ Automatic"));
        assert!(!text.contains("System Terminal"));
        assert!(!text.contains(" mode"));
        assert!(text.contains("Light Appearance"));
        assert!(text.contains("Solarized"));
        assert_no_option_line(&text, "Terminal");

        app.settings.list.selected = theme_names_for_appearance(ThemeAppearance::Light).len();
        app.settings.list.show();
        app.settings.scroll = theme_names_for_appearance(ThemeAppearance::Light).len() + 3;
        terminal
            .draw(|frame| render_settings_theme(&app, frame, Rect::new(0, 0, 100, 50)))
            .expect("render theme settings");

        let text = buffer_text(terminal.backend().buffer(), 100, 50);
        assert!(text.contains("Dark Appearance"));
        assert!(text.contains("Rosé Pine"));
    }

    #[test]
    fn theme_settings_system_scroll_can_reveal_dark_section_while_mode_selected() {
        let mut app = AppState::test_new();
        app.global_theme_mode = ThemeMode::System;
        app.settings.section = SettingsSection::Theme;
        app.settings.pending_theme_mode = Some(ThemeMode::System);
        app.settings.pending_light_theme_name = Some("solarized-light".to_string());
        app.settings.pending_dark_theme_name = Some("rose-pine".to_string());
        app.settings.list.selected = 0;
        app.settings.list.show();
        app.settings.scroll = theme_names_for_appearance(ThemeAppearance::Light).len() + 2;

        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_theme(&app, frame, Rect::new(0, 0, 100, 50)))
            .expect("render theme settings");

        let text = buffer_text(terminal.backend().buffer(), 100, 50);
        assert!(text.contains("Dark Appearance"));
        assert!(text.contains("Rosé Pine"));
    }

    #[test]
    fn theme_settings_marks_pending_values_not_hovered_cursor_values() {
        let mut app = AppState::test_new();
        app.global_theme_mode = ThemeMode::System;
        app.global_light_theme_name = "catppuccin-latte".to_string();
        app.settings.section = SettingsSection::Theme;
        app.settings.pending_theme_mode = Some(ThemeMode::Light);
        app.settings.pending_light_theme_name = Some("solarized-light".to_string());
        app.settings.list.selected = 1;
        app.settings.list.show();

        let backend = TestBackend::new(100, 50);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_theme(&app, frame, Rect::new(0, 0, 100, 50)))
            .expect("render theme settings");

        let text = buffer_text(terminal.backend().buffer(), 100, 50);
        assert!(!text.contains("✓ Automatic"));
        assert!(text.contains("✓ Light"));
        assert!(!text.contains("✓ Catppuccin Latte"));
        assert!(text.contains("✓ Solarized"));
    }

    #[test]
    fn theme_settings_selected_row_highlight_extends_to_row_end() {
        let mut app = AppState::test_new();
        app.global_theme_mode = ThemeMode::System;
        app.settings.section = SettingsSection::Theme;
        app.settings.pending_theme_mode = Some(ThemeMode::Light);
        app.settings.list.selected = 3;
        app.settings.list.show();

        let area = Rect::new(0, 0, 100, 80);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_theme(&app, frame, area))
            .expect("render theme settings");

        let buffer = terminal.backend().buffer();
        let text = buffer_text(buffer, area.width, area.height);
        let (selected_y, selected_x) =
            find_text_cell(&text, "✓ Light").expect("selected light row");
        let selected_row_end = area.x + area.width.saturating_sub(1);
        assert_eq!(
            buffer[(selected_x, selected_y)].style().bg,
            Some(app.palette.accent)
        );
        assert_eq!(
            buffer[(selected_row_end, selected_y)].style().bg,
            Some(app.palette.accent)
        );
    }

    #[test]
    fn terminal_dark_accent_highlight_starts_on_first_option() {
        let mut app = AppState::test_new();
        app.global_theme_mode = ThemeMode::System;
        app.settings.section = SettingsSection::Theme;
        app.settings.pending_theme_mode = Some(ThemeMode::System);
        app.settings.pending_light_theme_name = Some("system".to_string());
        app.settings.pending_dark_theme_name = Some("system".to_string());
        app.settings.list.selected = 2 + TerminalAccent::ALL.len();
        app.settings.list.show();

        let area = Rect::new(0, 0, 100, 50);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_theme(&app, frame, area))
            .expect("render theme settings");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        let dark_heading_y = text
            .lines()
            .position(|line| line.contains("Dark Accent"))
            .expect("dark accent heading") as u16;
        let dark_blue_y = text
            .lines()
            .enumerate()
            .skip(dark_heading_y as usize + 1)
            .find_map(|(y, line)| line.contains("Blue").then_some(y as u16))
            .expect("dark Blue option");
        let selected_row_end = area.x + area.width.saturating_sub(2);

        assert_ne!(
            terminal.backend().buffer()[(selected_row_end, dark_heading_y)]
                .style()
                .bg,
            Some(app.palette.accent)
        );
        assert_eq!(
            terminal.backend().buffer()[(selected_row_end, dark_blue_y)]
                .style()
                .bg,
            Some(app.palette.accent)
        );
    }

    #[test]
    fn theme_settings_selected_row_does_not_shift_text() {
        let mut app = AppState::test_new();
        app.global_theme_mode = ThemeMode::System;
        app.settings.section = SettingsSection::Theme;
        app.settings.pending_theme_mode = Some(ThemeMode::Light);
        app.settings.list.selected = 3;
        app.settings.list.show();

        let area = Rect::new(0, 0, 100, 50);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_theme(&app, frame, area))
            .expect("render theme settings");

        let buffer = terminal.backend().buffer();
        let text = buffer_text(buffer, area.width, area.height);
        let (y, x) = find_text_cell(&text, "✓ Light").expect("selected light row");
        assert_eq!(buffer[(x, y)].symbol(), "✓");
        assert_eq!(buffer[(x.saturating_add(1), y)].symbol(), " ");
        assert_eq!(buffer[(x.saturating_add(2), y)].symbol(), "L");
    }

    #[test]
    fn agent_settings_renders_custom_profile_management() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Agents;

        let area = Rect::new(0, 0, 100, 40);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(text.contains("Agents"));
        assert!(text.contains("New Agent Profile"));
        assert!(text.contains("Saved Profiles"));
        assert!(!text.contains("show shift+←→"));
    }

    #[test]
    fn failed_connection_removal_renders_plain_language_choices() {
        let mut app = AppState::test_new();
        app.ssh_connection_profiles.push(
            crate::persist::ssh_profiles::SshConnectionProfile::new(
                "robotbox",
                "Robotbox",
                "robotbox",
                Some(
                    crate::execution_host::HostPath::new("/home/charlie/projects")
                        .expect("valid host path"),
                ),
            )
            .expect("valid SSH profile"),
        );
        let mut client_view = crate::app::ClientViewState::from_default_client_state(&app);
        client_view.settings.section = SettingsSection::Connections;
        client_view.settings.connection_editor =
            Some(crate::app::state::ConnectionEditorState::detail_profile(
                "robotbox",
                "Robotbox",
                "robotbox",
                "/home/charlie/projects",
            ));
        client_view
            .settings
            .connection_editor
            .as_mut()
            .expect("connection editor")
            .connection_retirement = Some(crate::app::state::ConnectionRetirementState::Failed);
        client_view.settings.list.show();
        client_view.settings.scroll = 10;

        let area = Rect::new(0, 0, 100, 40);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay_for_view(&app, &client_view, frame, area))
            .expect("render connection failure");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(
            text.contains("Full cleanup is unavailable for Robotbox."),
            "{text}"
        );
        assert!(
            text.contains("Processes or worker files on that machine might remain."),
            "{text}"
        );
        assert!(
            text.contains("Remove the saved connection anyway?"),
            "{text}"
        );
        assert!(text.contains("Remove Saved Connection"), "{text}");
        assert!(text.contains("Try Again"), "{text}");
        assert!(text.contains("Cancel"), "{text}");
        assert!(!text.contains("remote host unavailable"), "{text}");
        assert!(!text.contains("inventory"), "{text}");
        assert!(!text.contains("local-only forget"), "{text}");
    }

    #[test]
    fn connection_removal_check_renders_animated_progress() {
        let mut app = AppState::test_new();
        app.ssh_connection_profiles.push(
            crate::persist::ssh_profiles::SshConnectionProfile::new(
                "robotbox",
                "Robotbox",
                "robotbox",
                Some(
                    crate::execution_host::HostPath::new("/home/charlie/projects")
                        .expect("valid host path"),
                ),
            )
            .expect("valid SSH profile"),
        );
        let mut client_view = crate::app::ClientViewState::from_default_client_state(&app);
        client_view.settings.section = SettingsSection::Connections;
        client_view.settings.connection_editor =
            Some(crate::app::state::ConnectionEditorState::detail_profile(
                "robotbox",
                "Robotbox",
                "robotbox",
                "/home/charlie/projects",
            ));
        client_view
            .settings
            .connection_editor
            .as_mut()
            .expect("connection editor")
            .connection_retirement =
            Some(crate::app::state::ConnectionRetirementState::InventoryPending);
        client_view.settings.scroll = 10;

        let area = Rect::new(0, 0, 100, 40);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay_for_view(&app, &client_view, frame, area))
            .expect("render first cleanup progress frame");
        let first_frame = buffer_text(terminal.backend().buffer(), area.width, area.height);

        app.spinner_tick = 8;
        terminal
            .draw(|frame| render_settings_overlay_for_view(&app, &client_view, frame, area))
            .expect("render second cleanup progress frame");
        let second_frame = buffer_text(terminal.backend().buffer(), area.width, area.height);

        assert!(
            first_frame.contains("⠋ Checking Removal Impact..."),
            "{first_frame}"
        );
        assert!(
            second_frame.contains("⠙ Checking Removal Impact..."),
            "{second_frame}"
        );
    }

    #[test]
    fn group_profile_settings_renders_profile_sections_without_filters() {
        let mut app = AppState::test_new();
        let group_idx = app.create_group("Work".to_string());
        app.settings.group_settings_target = Some(group_idx);
        app.settings.section = SettingsSection::GroupProfiles;
        app.settings.agent_profile_kind_filter = Some(crate::agent_profiles::AgentKind::Omp);

        let area = Rect::new(0, 0, 100, 40);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render group profile settings");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(text.contains("Group Settings"));
        assert!(text.contains("Favorites"));
        assert!(text.contains("Available"));
        assert!(!text.contains("show shift+←→"));
    }
    #[test]
    fn agent_profile_editor_renders_compact_form() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Agents;
        app.settings.pending_agent_profile_name = Some("omp mk".to_string());
        app.settings.pending_agent_profile_command = Some("omp-mk".to_string());
        app.settings.pending_agent_profile_kind = Some(crate::agent_profiles::AgentKind::Omp);
        app.integration_recommendations = vec![crate::integration::IntegrationRecommendation {
            target: crate::api::schema::IntegrationTarget::Omp,
            label: "omp",
            command: "omp",
            available: true,
            path: std::path::PathBuf::from("/tmp/gardn-test-omp"),
            state: crate::integration::IntegrationStatusKind::Current,
        }];

        let area = Rect::new(0, 0, 100, 40);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(text.contains("← Back"));
        assert!(text.contains("New Agent Profile"));
        assert!(text.contains("Label shown in menus"));
        assert!(text.contains("Agent Type"));
        assert!(text.contains("Choose an installed integration"));
        assert!(text.contains("Shell command to run"));
        assert!(!text.contains("1. name"));
        assert!(!text.contains("4. actions"));
        app.settings.scroll = 2;
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render scrolled settings overlay");
        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(text.contains("Create Profile"));
        assert!(text.contains("Cancel"));
    }

    #[test]
    fn agent_profile_editor_selected_choice_does_not_shift_text() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Agents;
        app.settings.pending_agent_profile_name = Some("omp mk".to_string());
        app.settings.pending_agent_profile_command = Some("omp-mk".to_string());
        app.settings.pending_agent_profile_kind = Some(crate::agent_profiles::AgentKind::Codex);
        app.integration_recommendations = vec![
            crate::integration::IntegrationRecommendation {
                target: crate::api::schema::IntegrationTarget::Claude,
                label: "claude",
                command: "claude",
                available: true,
                path: std::path::PathBuf::from("/tmp/gardn-test-claude"),
                state: crate::integration::IntegrationStatusKind::Current,
            },
            crate::integration::IntegrationRecommendation {
                target: crate::api::schema::IntegrationTarget::Codex,
                label: "codex",
                command: "codex",
                available: true,
                path: std::path::PathBuf::from("/tmp/gardn-test-codex"),
                state: crate::integration::IntegrationStatusKind::Current,
            },
        ];
        app.settings.list.selected = 2;
        app.settings.list.show();

        let area = Rect::new(0, 0, 100, 40);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let buffer = terminal.backend().buffer();
        let text = buffer_text(buffer, area.width, area.height);
        let (selected_y, selected_x) =
            find_text_cell(&text, "✓ Codex").expect("selected Codex kind");
        let (_, claude_x) = find_text_cell(&text, "  Claude").expect("unselected Claude kind");
        assert_eq!(selected_x, claude_x);
        assert_eq!(buffer[(selected_x, selected_y)].symbol(), "✓");
        assert_eq!(
            buffer[(selected_x, selected_y)].style().bg,
            Some(app.palette.accent)
        );
    }

    #[test]
    fn toast_settings_render_as_value_row() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Toast;
        app.settings.pending_toast_delivery = Some(ToastDelivery::Off);

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        let lines: Vec<&str> = text.lines().collect();
        let header_row = lines
            .iter()
            .position(|line| line.contains("Notification Popups"))
            .expect("notification header row");
        let delivery_row = lines
            .iter()
            .position(|line| line.contains("Toast Delivery") && line.contains("Off"))
            .expect("toast delivery row");
        let description_row = lines
            .iter()
            .position(|line| line.contains("Where notification popups should appear"))
            .expect("toast delivery description row");

        assert_eq!(delivery_row, header_row + 1);
        assert_eq!(description_row, delivery_row + 1);
        assert!(!text.contains("Inside Gardn"));
        assert!(!text.contains("Via Terminal"));
    }

    #[test]
    fn settings_renders_single_escape_close_label() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Theme;

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert_eq!(text.matches("Esc Close").count(), 1);
        assert!(!text.contains("Esc Cancel"));
    }

    #[test]
    fn settings_has_no_footer_save_button() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Theme;

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(!text.contains("↵ save"));
        assert!(!text.contains("↵ apply"));
    }

    #[test]
    fn narrow_client_settings_tabs_render_at_their_hit_areas() {
        let app = AppState::test_new();
        let mut client_view = crate::app::ClientViewState::from_default_client_state(&app);
        client_view.settings.section = SettingsSection::Agents;
        let row = Rect::new(3, 4, 30, 1);
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).expect("test backend");

        terminal
            .draw(|frame| render_settings_tabs_for_view(&client_view, frame, row, &app.palette))
            .expect("render client settings tabs");

        let hit_areas = settings_tab_hit_areas_for_view(&client_view, row);
        assert_eq!(
            hit_areas
                .iter()
                .map(|(section, _)| *section)
                .collect::<Vec<_>>(),
            vec![SettingsSection::Commands, SettingsSection::Agents]
        );
        for (section, rect) in &hit_areas {
            assert_eq!(
                terminal.backend().buffer()[(rect.x + 1, row.y)].symbol(),
                &settings_tab_text_for(&client_view.settings, *section)[..1],
                "tab {section:?} is not rendered at its hit area"
            );
        }

        assert_eq!(
            settings_tab_chevron_at_for_view(&client_view, row, row.x),
            Some(SettingsSection::PaneLabels)
        );
        let right_chevron_x = hit_areas
            .last()
            .map(|(_, rect)| rect.x + rect.width)
            .expect("visible settings tab");
        assert_eq!(
            settings_tab_chevron_at_for_view(&client_view, row, right_chevron_x),
            Some(SettingsSection::Integrations)
        );

        let tab_text = (row.x..row.x + row.width)
            .map(|x| terminal.backend().buffer()[(x, row.y)].symbol())
            .collect::<String>();
        assert!(tab_text.contains("Commands"));
        assert!(tab_text.contains("Agents"));
        assert!(!tab_text.contains("Behavior"));
        assert!(!tab_text.contains("Notifications"));
        assert!(!tab_text.contains("Toasts"));
        assert!(!tab_text.contains("Advanced"));
        assert!(!tab_text.contains("Integrations"));
    }

    #[test]
    fn settings_sidebar_lists_categories_and_active_subsections() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Theme;

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(text.contains("▾ Appearance"), "{text}");
        assert!(text.contains("* Colors"), "{text}");
        assert!(text.contains("Themes"), "{text}");
        assert!(text.contains("Sidebar"), "{text}");
        assert!(text.contains("Panes"), "{text}");
        assert!(text.contains("▸ Notifications"), "{text}");
        assert!(text.contains("▸ Advanced"), "{text}");
    }

    #[test]
    fn behavior_sidebar_lists_selection_subsection() {
        let mut app = AppState::test_new();
        app.settings.sidebar_expanded = Some(SettingsSection::PaneLabels);
        app.settings.section = SettingsSection::PaneLabels;

        let labels = settings_sidebar_entries(&app.settings)
            .into_iter()
            .filter(|entry| entry.section == SettingsSection::PaneLabels)
            .map(|entry| entry.label)
            .collect::<Vec<_>>();

        assert_eq!(
            labels,
            ["Behavior", "General", "Selection", "Terminal", "Sessions"]
        );
    }
    #[test]
    fn settings_sidebar_separates_content_header_from_options() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Theme;

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        let (_, navigation_x) = find_text_cell(&text, "SETTINGS").expect("sidebar heading");
        let (description_y, description_x) = find_text_cell(
            &text,
            "Configure theme, sidebar layout, and pane appearance",
        )
        .expect("settings description");
        assert!(text.contains("Appearance"), "{text}");
        assert!(
            description_x > navigation_x + SETTINGS_SIDEBAR_WIDTH,
            "{text}"
        );
        assert_eq!(
            terminal.backend().buffer()[(description_x, description_y + 1)].symbol(),
            "─"
        );
    }

    #[test]
    fn settings_tabs_render_descriptions_consistently() {
        let expected = [
            (
                SettingsSection::Theme,
                "Appearance",
                "Configure theme, sidebar layout, and pane appearance",
            ),
            (
                SettingsSection::Sound,
                "Notifications",
                "Choose sound and toast notification behavior",
            ),
            (
                SettingsSection::PaneLabels,
                "Behavior",
                "Control workspace prompts and terminal interaction defaults",
            ),
            (
                SettingsSection::Commands,
                "Commands",
                "Edit launch commands; clear one to disable and hide it",
            ),
            (
                SettingsSection::Agents,
                "Agents",
                "Create and manage agent launch profiles",
            ),
            (
                SettingsSection::Integrations,
                "Agent Integrations",
                "Install hooks so agents report state directly",
            ),
            (
                SettingsSection::Connections,
                "Connections",
                "Add SSH hosts and manage their connections",
            ),
            (
                SettingsSection::Experiments,
                "Advanced",
                "Configure advanced or platform-specific behavior",
            ),
            (
                SettingsSection::About,
                "About",
                "Open-source projects and contributors behind Gardn",
            ),
        ];

        assert_eq!(expected.len(), SettingsSection::ALL.len());
        for (&(section, title, description), expected_section) in
            expected.iter().zip(SettingsSection::ALL)
        {
            assert_eq!(section, *expected_section);

            let mut app = AppState::test_new();
            app.settings.section = section;

            let area = Rect::new(0, 0, 100, 30);
            let backend = TestBackend::new(area.width, area.height);
            let mut terminal = Terminal::new(backend).expect("test backend");
            terminal
                .draw(|frame| render_settings_overlay(&app, frame, area))
                .expect("render settings overlay");

            let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
            assert!(text.contains(title), "missing title for {section:?}");
            assert!(
                text.contains(description),
                "missing description for {section:?}"
            );
            let (description_y, description_x) =
                find_text_cell(&text, description).expect("settings description");
            assert_eq!(
                terminal.backend().buffer()[(description_x, description_y + 1)].symbol(),
                "─",
                "missing header divider for {section:?}"
            );
        }
    }

    #[test]
    fn about_settings_render_acknowledgments() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::About;

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render About settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        for expected in ["Copyright (c) 2026 Kit Langton", "MIT License"] {
            assert!(text.contains(expected), "missing About copy: {expected}");
        }
    }

    #[test]
    fn scoped_settings_screens_separate_headers_from_options() {
        for workspace in [false, true] {
            let area = Rect::new(0, 0, 100, 30);
            let mut app = AppState::test_new();
            if workspace {
                app.workspaces = vec![crate::workspace::Workspace::test_new("Space")];
                app.settings.workspace_settings_target = Some(0);
                app.settings.section = SettingsSection::WorkspaceGeneral;
            } else {
                let group_idx = app.create_group("Work".to_string());
                app.settings.group_settings_target = Some(group_idx);
                app.settings.section = SettingsSection::GroupGeneral;
            }
            let mut terminal =
                Terminal::new(TestBackend::new(area.width, area.height)).expect("test backend");
            terminal
                .draw(|frame| render_settings_overlay(&app, frame, area))
                .expect("render scoped settings overlay");
            let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
            let (name_y, name_x) = find_text_cell(&text, "Name").expect("name control");
            assert_eq!(
                terminal.backend().buffer()[(name_x, name_y - 1)].symbol(),
                "─"
            );
        }
    }

    #[test]
    fn layout_settings_render_sidebar_widths() {
        let mut app = AppState::test_new();
        app.default_sidebar_width = 26;
        app.sidebar_min_width = 18;
        app.sidebar_max_width = 36;
        app.settings.section = SettingsSection::Layout;

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(text.contains("Sidebar"));
        assert!(text.contains("Sidebar Arrangement"));
        assert!(text.contains("Auto"));
        assert!(text.contains("Default Sidebar Width"));
        assert!(text.contains("26 cols"));
        assert!(text.contains("Minimum Sidebar Width"));
        assert!(text.contains("18 cols"));
        assert!(text.contains("Maximum Sidebar Width"));
        assert!(text.contains("36 cols"));
    }

    #[test]
    fn behavior_settings_render_workspace_and_terminal_options() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::PaneLabels;

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(text.contains("General"));
        assert!(text.contains("Name New Workspaces"));
        assert!(text.contains("Show Counters"));
        assert!(!text.contains("Pane Border Agent Info"));

        let rows =
            rows_for_section(&app, SettingsSection::PaneLabels).expect("behavior settings rows");
        let selection_header = rows
            .iter()
            .position(|row| matches!(row, SettingsListRow::Header("Selection")))
            .expect("selection settings header");
        app.settings.scroll = visual_row_count(&rows[..selection_header]);
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render selection settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(text.contains("Selection"));
        assert!(text.contains("Copy on Select"));
        assert!(text.contains("Right-click Passthrough"));

        let terminal_header = rows
            .iter()
            .position(|row| matches!(row, SettingsListRow::Header("Terminal")))
            .expect("terminal settings header");
        app.settings.scroll = visual_row_count(&rows[..terminal_header]);
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render scrolled settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(text.contains("Terminal"));
        assert!(text.contains("New Terminal CWD"));
        assert!(text.contains("Mouse Wheel Speed"));
        let sessions_header = rows
            .iter()
            .position(|row| matches!(row, SettingsListRow::Header("Sessions")))
            .expect("sessions settings header");
        app.settings.scroll = visual_row_count(&rows[..sessions_header]);
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render sessions settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(text.contains("Sessions"));
        assert!(text.contains("Resume Agent Sessions"));
    }

    #[test]
    fn appearance_settings_render_window_title() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Theme;
        let rows =
            rows_for_section(&app, SettingsSection::Theme).expect("appearance settings rows");
        let window_header = rows
            .iter()
            .position(|row| matches!(row, SettingsListRow::Header("Window")))
            .expect("window settings header");
        app.settings.scroll = visual_row_count(&rows[..window_header]);

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render window settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(text.contains("Window"));
        assert!(text.contains("Window Title"));
        assert!(text.contains("{hostname}: {workspace}"));
    }

    #[test]
    fn sectioned_settings_selected_text_uses_selected_foreground() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Layout;
        app.settings.list.selected = 0;
        app.settings.list.show();

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        let (selected_y, selected_x) = text
            .lines()
            .enumerate()
            .find_map(|(y, line)| {
                line.find("Default Sidebar Width")
                    .map(|x| (y as u16, x as u16))
            })
            .expect("selected layout row");

        assert_eq!(
            terminal.backend().buffer()[(selected_x, selected_y)]
                .style()
                .fg,
            Some(panel_contrast_fg(&app.palette))
        );
    }

    #[test]
    fn settings_rows_open_without_selected_highlight() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Layout;
        app.settings.list.selected = 0;
        app.settings.list.hide();

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        let (row, col) =
            find_text_cell(&text, "Default Sidebar Width").expect("layout setting row");

        assert_ne!(
            terminal.backend().buffer()[(col, row)].style().fg,
            Some(panel_contrast_fg(&app.palette))
        );
        assert_ne!(
            terminal.backend().buffer()[(col, row)].style().bg,
            Some(app.palette.accent)
        );
    }

    #[test]
    fn selected_value_row_uses_selected_foreground() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Layout;
        app.settings.list.selected = 0;
        app.settings.list.show();

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        let (selected_y, selected_x) =
            find_text_cell(&text, "Default Sidebar Width").expect("selected layout row");

        assert_eq!(
            terminal.backend().buffer()[(selected_x, selected_y)]
                .style()
                .fg,
            Some(panel_contrast_fg(&app.palette))
        );
        assert!(text.contains("26 cols"));
    }

    #[test]
    fn selected_toggle_row_uses_selected_foreground() {
        let mut app = AppState::test_new();
        app.switch_ascii_input_source_in_prefix = false;
        app.settings.section = SettingsSection::Experiments;
        app.settings.list.selected = 0;
        app.settings.list.show();

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        let (selected_y, selected_x) = find_text_cell(
            &text,
            "Switch to ASCII Input Source in Prefix (macOS/Windows)",
        )
        .expect("selected experiment row");

        assert_eq!(
            terminal.backend().buffer()[(selected_x, selected_y)].symbol(),
            "S"
        );
        assert_eq!(
            terminal.backend().buffer()[(selected_x, selected_y)]
                .style()
                .fg,
            Some(panel_contrast_fg(&app.palette))
        );
    }

    #[test]
    fn integration_success_feedback_renders_as_styled_hint_not_log() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Integrations;
        app.integration_recommendations = vec![crate::integration::IntegrationRecommendation {
            target: crate::api::schema::IntegrationTarget::Codex,
            label: "codex",
            command: "codex",
            available: true,
            path: std::path::PathBuf::from("/tmp/gardn-test-codex"),
            state: crate::integration::IntegrationStatusKind::Current,
        }];
        let restart_guidance = "Restart running Codex panes to use the updated hook";
        app.integration_install_messages =
            vec![restart_guidance.to_string(), "installed codex".to_string()];

        let area = Rect::new(0, 0, 120, 32);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let buffer = terminal.backend().buffer();
        let text = buffer_text(buffer, area.width, area.height);
        assert!(
            !text.contains("installed codex"),
            "post-install feedback should not render the old install log:\n{text}"
        );
        assert_eq!(text.matches(restart_guidance).count(), 1, "{text}");
        assert!(text.contains("Move ↑↓"), "{text}");
        assert!(text.contains("Action Space/↵"), "{text}");
        assert!(text.contains("Sidebar Tab"), "{text}");

        let (hint_y, _) = find_text_cell(&text, restart_guidance).expect("restart hint");
        let hint_line = text.lines().nth(hint_y as usize).expect("restart hint row");
        assert!(
            hint_line.contains(restart_guidance) && !hint_line.contains("installed codex"),
            "restart guidance should be a single hint row, got {hint_line:?}"
        );
        let (label_y, label_x) = find_text_cell(&text, " Hint ").expect("hint label");
        assert_eq!(label_y, hint_y);
        assert!(
            hint_y > 0,
            "restart hint should have a blank spacer row above it:\n{text}"
        );
        let popup = crate::ui::widgets::centered_popup_rect(area, 92, 26).expect("popup");
        let inner = Rect::new(
            popup.x + 1,
            popup.y + 1,
            popup.width.saturating_sub(2),
            popup.height.saturating_sub(2),
        );
        let content = settings_sidebar_areas(settings_stack_areas(&app, inner).content).content;
        assert!(
            (content.x..content.x + content.width)
                .all(|x| { buffer[(x, hint_y - 1)].symbol().trim().is_empty() }),
            "restart hint should be visually separated from the integration list"
        );
        let footer_y = find_text_cell(&text, "Move ↑↓").expect("footer controls").0;
        assert!(
            hint_y < footer_y,
            "restart hint should remain above footer controls:\n{text}"
        );
        assert_eq!(
            buffer[(label_x, label_y)].style().bg,
            Some(app.palette.green),
            "restart instruction should have a distinct hint label style"
        );
        let guidance_x = find_text_cell(&text, restart_guidance)
            .expect("restart hint")
            .1;
        assert_eq!(
            buffer[(guidance_x, hint_y)].style().fg,
            Some(app.palette.subtext0),
            "restart instruction should use muted hint text, not the old dim log style"
        );
        assert_ne!(
            buffer[(guidance_x, hint_y)].style().fg,
            Some(app.palette.overlay0),
            "restart instruction should not use the old dim log style"
        );
    }

    #[test]
    fn unavailable_integration_hint_keeps_spacer_and_footer_controls() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Integrations;
        app.settings.list.selected = 1;
        app.integration_recommendations = vec![
            crate::integration::IntegrationRecommendation {
                target: crate::api::schema::IntegrationTarget::Codex,
                label: "codex",
                command: "codex",
                available: true,
                path: std::path::PathBuf::from("/tmp/gardn-test-codex"),
                state: crate::integration::IntegrationStatusKind::Current,
            },
            crate::integration::IntegrationRecommendation {
                target: crate::api::schema::IntegrationTarget::Claude,
                label: "claude",
                command: "claude",
                available: false,
                path: std::path::PathBuf::from("/tmp/gardn-test-claude"),
                state: crate::integration::IntegrationStatusKind::NotInstalled,
            },
        ];

        let area = Rect::new(0, 0, 120, 32);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        let unavailable_hint = "Selected integration is unavailable";
        assert_eq!(text.matches(unavailable_hint).count(), 1, "{text}");
        assert!(text.contains("Move ↑↓"), "{text}");
        assert!(text.contains("Action Space/↵"), "{text}");
        assert!(text.contains("Sidebar Tab"), "{text}");

        let (hint_y, _) = find_text_cell(&text, unavailable_hint).expect("unavailable hint");
        assert!(
            hint_y > 0,
            "unavailable hint should have a blank spacer row above it:\n{text}"
        );
        let buffer = terminal.backend().buffer();
        let popup = crate::ui::widgets::centered_popup_rect(area, 92, 26).expect("popup");
        let inner = Rect::new(
            popup.x + 1,
            popup.y + 1,
            popup.width.saturating_sub(2),
            popup.height.saturating_sub(2),
        );
        let content = settings_sidebar_areas(settings_stack_areas(&app, inner).content).content;
        assert!(
            (content.x..content.x + content.width)
                .all(|x| { buffer[(x, hint_y - 1)].symbol().trim().is_empty() }),
            "unavailable hint should be visually separated from the integration list"
        );
        let footer_y = find_text_cell(&text, "Move ↑↓").expect("footer controls").0;
        assert!(
            hint_y < footer_y,
            "unavailable hint should remain above footer controls:\n{text}"
        );
    }

    #[test]
    fn settings_tabs_do_not_show_integration_badges() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Integrations;
        app.integration_recommendations = vec![crate::integration::IntegrationRecommendation {
            target: crate::api::schema::IntegrationTarget::Omp,
            label: "omp",
            command: "omp",
            available: true,
            path: std::path::PathBuf::from("/tmp/gardn-test-omp"),
            state: crate::integration::IntegrationStatusKind::Outdated,
        }];

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(text.contains("Integrations"));
        assert!(!text.contains("● Integrations"));
    }

    #[test]
    fn integrations_render_selected_ssh_host_and_pending_status() {
        let mut app = AppState::test_new();
        let profile = crate::persist::ssh_profiles::SshConnectionProfile::new(
            "build-box",
            "Build box",
            "dev@build.example",
            None,
        )
        .expect("valid SSH profile");
        app.ssh_connection_profiles.push(profile);
        app.integration_recommendations = vec![crate::integration::IntegrationRecommendation {
            target: crate::api::schema::IntegrationTarget::Omp,
            label: "omp",
            command: "omp",
            available: true,
            path: std::path::PathBuf::from("/tmp/gardn-test-omp"),
            state: crate::integration::IntegrationStatusKind::Current,
        }];
        app.settings.section = SettingsSection::Integrations;
        app.settings.integration_host_profile_id = Some("build-box".to_string());
        app.settings.list.show();

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(text.contains("Integration Host"), "{text}");
        assert!(text.contains("Build box"), "{text}");
        assert!(
            text.contains("Checking integrations on Build box"),
            "{text}"
        );
        assert!(
            text.contains("Press enter to change the integration host"),
            "{text}"
        );
        assert!(!text.contains("Uninstall selected integration"), "{text}");
    }
    #[test]
    fn integrations_selected_row_highlight_extends_to_row_end() {
        let mut app = AppState::test_new();
        app.settings.section = SettingsSection::Integrations;
        app.settings.list.selected = 0;
        app.settings.list.show();
        app.integration_recommendations = vec![crate::integration::IntegrationRecommendation {
            target: crate::api::schema::IntegrationTarget::Omp,
            label: "omp",
            command: "omp",
            available: true,
            path: std::path::PathBuf::from("/tmp/gardn-test-omp"),
            state: crate::integration::IntegrationStatusKind::NotInstalled,
        }];

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let popup = crate::ui::widgets::centered_popup_rect(area, 92, 26).expect("popup");
        let inner = Rect::new(
            popup.x + 1,
            popup.y + 1,
            popup.width.saturating_sub(2),
            popup.height.saturating_sub(2),
        );
        let content = settings_sidebar_areas(settings_stack_areas(&app, inner).content).content;
        let body = settings_section_list_rect(content);
        let [list_area, _] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(2)]).areas::<2>(body);
        let selected_row_end = list_area.x + list_area.width.saturating_sub(1);

        assert_eq!(
            terminal.backend().buffer()[(selected_row_end, list_area.y)]
                .style()
                .bg,
            Some(app.palette.accent)
        );
    }
    #[test]
    fn client_integrations_selected_status_highlight_covers_status_text() {
        let mut app = AppState::test_new();
        app.integration_recommendations = vec![crate::integration::IntegrationRecommendation {
            target: crate::api::schema::IntegrationTarget::Omp,
            label: "pi",
            command: "pi",
            available: true,
            path: std::path::PathBuf::from("/tmp/gardn-test-pi"),
            state: crate::integration::IntegrationStatusKind::Current,
        }];
        let mut client_view = crate::app::ClientViewState::from_default_client_state(&app);
        client_view.settings.section = SettingsSection::Integrations;
        client_view.settings.list.selected = 0;
        client_view.settings.list.show();

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay_for_view(&app, &client_view, frame, area))
            .expect("render client settings overlay");

        let buffer = terminal.backend().buffer();
        let text = buffer_text(buffer, area.width, area.height);
        let (status_y, status_x) = find_text_cell(&text, "Installed").expect("installed status");
        assert_eq!(
            buffer[(status_x, status_y)].style().bg,
            Some(app.palette.accent),
            "selected integration status should share the row highlight"
        );
    }
    #[test]
    fn client_group_settings_selected_choice_marker_uses_group_accent_background() {
        let mut app = AppState::test_new();
        let group_idx = app.create_group("work".to_string());
        app.set_group_accent(group_idx, Some(TerminalAccent::Magenta));
        let palette = app.palette_for_group(group_idx);
        let mut client_view = crate::app::ClientViewState::from_default_client_state(&app);
        client_view.settings.group_settings_target = Some(group_idx);
        client_view.settings.section = SettingsSection::Theme;
        client_view.settings.list.selected = 1 + TerminalAccent::ALL
            .iter()
            .position(|accent| *accent == TerminalAccent::Magenta)
            .expect("magenta terminal accent");
        client_view.settings.list.show();

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay_for_view(&app, &client_view, frame, area))
            .expect("render client group settings overlay");

        let buffer = terminal.backend().buffer();
        let text = buffer_text(buffer, area.width, area.height);
        let selected_label = format!("✓ {}", TerminalAccent::Magenta.display_name());
        let (choice_y, choice_x) =
            find_text_cell(&text, &selected_label).expect("selected magenta choice");
        assert_eq!(
            buffer[(choice_x, choice_y)].style().bg,
            Some(palette.accent),
            "selected group choice marker should share the group accent highlight"
        );
    }

    #[test]
    fn advanced_settings_render_input_and_headless_size() {
        let mut app = AppState::test_new();
        app.switch_ascii_input_source_in_prefix = true;
        app.headless_size = (160, 50);
        app.settings.section = SettingsSection::Experiments;
        app.settings.list.selected = 0;
        app.settings.list.show();

        let area = Rect::new(0, 0, 100, 30);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_settings_overlay(&app, frame, area))
            .expect("render settings overlay");

        let text = buffer_text(terminal.backend().buffer(), area.width, area.height);
        assert!(!text.contains("pane screen history"));
        assert!(text.contains("Input"));
        assert!(text.contains("Switch to ASCII Input Source in Prefix (macOS/Windows)"));
        assert!(text.contains("Server"));
        assert!(text.contains("Headless Terminal Columns"));
        assert!(text.contains("160"));
        assert!(text.contains("Headless Terminal Rows"));
        assert!(text.contains("50"));
    }
    fn assert_no_option_line(text: &str, option: &str) {
        let mut in_appearance_section = false;
        for line in text.lines() {
            let line = line.trim();
            if line == "Light Appearance" || line == "Dark Appearance" {
                in_appearance_section = true;
                continue;
            }
            if in_appearance_section && line.is_empty() {
                in_appearance_section = false;
                continue;
            }
            assert!(
                !in_appearance_section
                    || (line != option
                        && line != format!("{option} ✓")
                        && line != format!("✓ {option}")),
                "unexpected appearance option line {option:?} in:\n{text}"
            );
        }
    }
    fn find_text_cell(text: &str, needle: &str) -> Option<(u16, u16)> {
        text.lines().enumerate().find_map(|(y, line)| {
            let byte_x = line.find(needle)?;
            let cell_x = line[..byte_x].chars().count();
            Some((y as u16, cell_x as u16))
        })
    }

    fn buffer_text(buffer: &Buffer, width: u16, height: u16) -> String {
        let mut text = String::new();
        for y in 0..height {
            for x in 0..width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }
}
