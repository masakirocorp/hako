use crate::app::view_state::{CanvasOrigin, ClientTabViewKey, TabCanvasViewport};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

mod agent_profile_picker;
mod command_palette;
mod config_diagnostics;
mod dialogs;
pub(crate) mod git_repo_picker;
pub(crate) mod github;
mod keybind_help;
mod menus;
mod mobile;
mod modal_tabs;
mod navigator;
mod onboarding;
pub(crate) mod panes;
mod release_notes;
mod scrollbar;
mod settings;
mod sidebar;
mod status;
mod tabs;
mod text;
mod widgets;

use self::agent_profile_picker::{
    render_agent_profile_picker_overlay, render_agent_profile_picker_overlay_for_view,
};
use self::command_palette::{
    render_command_palette_overlay, render_command_palette_overlay_for_view,
};
pub(crate) use self::config_diagnostics::{
    config_diagnostics_action_at, config_diagnostics_max_scroll, config_diagnostics_popup_rect,
    ConfigDiagnosticsAction,
};
use self::config_diagnostics::{
    render_config_diagnostics_overlay, render_config_diagnostics_overlay_for_view,
};
use self::dialogs::{
    render_confirm_close_overlay, render_confirm_close_overlay_for_view,
    render_confirm_delete_group_overlay, render_confirm_delete_group_overlay_for_view,
    render_rename_overlay, render_rename_overlay_for_view,
};
use self::git_repo_picker::{
    render_git_repo_picker_overlay, render_git_repo_picker_overlay_for_view,
};
#[cfg(test)]
pub(crate) use self::keybind_help::keybind_help_lines;
use self::keybind_help::{render_keybind_help_overlay, render_keybind_help_overlay_for_view};
use self::menus::{
    render_agent_menu, render_agent_menu_for_view, render_context_menu,
    render_context_menu_for_view, render_copy_mode_overlay, render_copy_mode_overlay_for_view,
    render_global_launcher_menu, render_global_launcher_menu_for_view, render_group_menu,
    render_group_menu_for_view, render_navigate_overlay, render_navigate_overlay_for_view,
    render_prefix_overlay, render_prefix_overlay_for_view, render_resize_overlay,
    render_resize_overlay_for_view,
};
use self::mobile::{
    is_mobile_width, mobile_switcher_max_scroll_for_height,
    mobile_switcher_max_scroll_for_view_height, mobile_toast_banner_rect, render_mobile_header,
    render_mobile_header_for_view, render_mobile_panel, render_mobile_panel_for_view,
    render_mobile_toast_banner, MOBILE_AGENT_PANEL_CHROME_HEIGHT, MOBILE_HEADER_HEIGHT,
};
pub(crate) use self::navigator::{navigator_layout, navigator_popup_rect};
use self::navigator::{render_navigator_overlay, render_navigator_overlay_for_view};
pub(crate) use self::onboarding::onboarding_welcome_continue_rect;
use self::onboarding::render_onboarding_overlay;
pub(crate) use self::panes::popup_pane_rects_for_view;
use self::panes::{
    compute_pane_infos, compute_pane_infos_for_view, render_panes, render_panes_for_view,
    render_popup_pane_for_view, resize_popup_pane_for_view,
};
pub(crate) use self::release_notes::{
    product_announcement_display_lines, release_notes_close_button_rect,
    release_notes_display_lines, release_notes_wrapped_line_count, PRODUCT_ANNOUNCEMENT_MODAL_SIZE,
    RELEASE_NOTES_MODAL_SIZE,
};
use self::release_notes::{
    render_product_announcement_overlay, render_product_announcement_overlay_for_view,
    render_release_notes_overlay, render_release_notes_overlay_for_view,
};
pub(crate) use self::scrollbar::{
    pane_scrollbar_rect, release_notes_scrollbar_rect, scrollbar_offset_from_drag_row,
    scrollbar_offset_from_row, scrollbar_thumb_grab_offset, should_show_scrollbar,
};
use self::settings::{render_settings_overlay, render_settings_overlay_for_view};
#[cfg(test)]
pub(crate) use self::sidebar::collapsed_workspace_rows_rect;
use self::sidebar::{
    render_collapsed_sidebar_hover, render_collapsed_sidebar_hover_for_view, render_right_sidebar,
    render_right_sidebar_for_view, render_sidebar, render_sidebar_collapsed,
    render_sidebar_collapsed_for_view, render_sidebar_for_view,
};
use self::status::{
    copy_feedback_rect, render_copy_feedback, render_toast_notification, toast_notification_rect,
};
use self::tabs::{render_tab_bar, render_tab_bar_for_view};
use self::widgets::fill_rect;
pub(crate) use self::{
    agent_profile_picker::{
        agent_profile_picker_button_rects, agent_profile_picker_inner_rect,
        agent_profile_picker_list_geometry, agent_profile_picker_popup_rect,
        agent_profile_picker_tab_chevron_at, agent_profile_picker_tab_hit_areas,
    },
    command_palette::{
        command_palette_button_rects, command_palette_inner_rect, command_palette_list_geometry,
        command_palette_popup_rect,
    },
};
pub(crate) use self::{
    dialogs::{
        confirm_close_button_rects, confirm_close_popup_rect, group_default_directory_input_rect,
        group_default_directory_input_rect_for_view, group_default_host_rect,
        group_default_host_rect_for_view, group_icon_button_rect, group_icon_button_rect_for_view,
        group_icon_picker_rects, group_icon_picker_rects_at, group_icon_picker_rects_for_view,
        group_icon_picker_row_count, group_name_input_rect, group_name_input_rect_for_view,
        rename_button_rects, rename_modal_size, rename_modal_size_for_view,
    },
    settings::{
        settings_close_button_rect, settings_editor_back_button_rect, settings_section_list_rect,
        settings_sidebar_areas, settings_sidebar_entries, settings_sidebar_hit_areas,
        settings_stack_areas, settings_subsection_anchor, settings_tab_chevron_at,
        settings_tab_hit_areas, SettingsSidebarEntry,
    },
    sidebar::{
        agent_panel_body_rect, agent_panel_empty_row_at, agent_panel_empty_row_at_for_view,
        agent_panel_entries, agent_panel_entries_for_view, agent_panel_entry_at_row,
        agent_panel_entry_at_row_for_view, agent_panel_header_target_at_row,
        agent_panel_header_target_at_row_for_view, agent_panel_scroll_metrics,
        agent_panel_scroll_metrics_for_view, agent_panel_scrollbar_rect, agent_panel_toggle_rect,
        collapsed_agent_panel_entry_at_row, collapsed_agent_panel_entry_at_row_for_view,
        collapsed_agent_panel_header_target_at_row,
        collapsed_agent_panel_header_target_at_row_for_view, collapsed_agent_panel_toggle_rect,
        collapsed_group_header_rect, collapsed_sidebar_launcher_rect,
        collapsed_sidebar_sections_for_split, collapsed_sidebar_toggle_rect,
        collapsed_workspace_at_row, collapsed_workspace_group_header_at_row,
        collapsed_workspace_row_entry_at_for_view, compute_workspace_card_areas,
        compute_workspace_card_areas_in_list, compute_workspace_card_areas_in_list_for_view,
        compute_workspace_group_drop_areas_in_list, compute_workspace_group_empty_areas_in_list,
        compute_workspace_group_empty_areas_in_list_for_view, compute_workspace_group_header_areas,
        compute_workspace_group_header_areas_in_list,
        compute_workspace_group_header_areas_in_list_for_view, expanded_sidebar_sections,
        expanded_sidebar_toggle_rect, global_launcher_rect_for_view, group_selector_rect_for_view,
        left_sidebar_workspace_rect, right_aligned_expanded_sidebar_sections,
        right_aligned_sidebar_section_divider_rect, right_aligned_workspace_list_rect,
        right_sidebar_content_rect, right_sidebar_toggle_rect, sidebar_section_divider_rect,
        workspace_drop_indicator_row, workspace_list_body_rect, workspace_list_entry_count,
        workspace_list_entry_count_for_view, workspace_list_position_for_workspace,
        workspace_list_rect, workspace_list_scroll_metrics, workspace_list_scroll_metrics_for_view,
        workspace_list_scrollbar_rect, workspace_list_scrollbar_rect_for_view, AgentPanelEntry,
        AgentPanelHeaderTarget, CollapsedWorkspaceRowEntry,
    },
};
pub(crate) use self::{
    keybind_help::{keybind_help_layout, keybind_help_scroll_metrics, keybind_help_scrollbar_rect},
    mobile::{
        keep_mobile_switcher_selection_visible, keep_mobile_switcher_selection_visible_for_view,
        mobile_agent_strip_rect, mobile_switcher_areas, mobile_switcher_areas_for_view,
        mobile_switcher_max_scroll, mobile_switcher_max_scroll_for_view,
        mobile_switcher_selected_target, mobile_switcher_selected_target_for_view,
        mobile_switcher_target_at, mobile_switcher_target_at_for_view,
        mobile_switcher_target_count, mobile_switcher_target_count_for_view,
        mobile_switcher_target_index, mobile_switcher_target_index_for_view,
        mobile_switcher_workspace_doc_row, MobileSwitcherTarget,
    },
    panes::pane_is_scrolled_back,
    tabs::compute_tab_bar_view,
    widgets::{
        centered_popup_rect, modal_scroll_metrics, modal_stack_areas, ModalListGeometry,
        ModalListViewport,
    },
};
use crate::app::state::ViewLayout;
use crate::app::{AppState, ClientTabControl, ClientViewState, Mode};
use crate::terminal::TerminalRuntimeRegistry;

const COLLAPSED_WIDTH: u16 = 4; // num + space + dot + separator
const RIGHT_SIDEBAR_MIN_TERMINAL_WIDTH: u16 = 56;
#[allow(dead_code)]
pub(crate) const MIN_SIDEBAR_WIDTH: u16 = 18;
#[allow(dead_code)]
pub(crate) const MAX_SIDEBAR_WIDTH: u16 = 36;
pub(crate) const MIN_RIGHT_SIDEBAR_WIDTH: u16 = 18;
pub(crate) const MAX_RIGHT_SIDEBAR_WIDTH: u16 = 36;

const CONTEXT_BAR_SEPARATOR: &str = " / ";

fn desktop_content_areas(area: Rect, show_context_bar: bool) -> (Rect, Rect) {
    if !show_context_bar || area.height <= 1 {
        return (area, Rect::default());
    }
    let [content, context_bar] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    (content, context_bar)
}

fn count_label(count: usize, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

const WATCHING_CHIP_BADGE: &str = " Watching ";
const FREE_CHIP_BADGE: &str = " Free ";

/// Copy for the per-client tab-control chip that trails the context bar.
/// The chip is client-local chrome: it only exists for the watching states
/// and is omitted entirely for the controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabControlChip {
    badge: &'static str,
    suffix: &'static str,
    free: bool,
}

impl TabControlChip {
    fn label(self) -> String {
        format!("{}{}", self.badge, self.suffix)
    }
}

pub(crate) fn tab_control_chip(tab_control: ClientTabControl) -> Option<TabControlChip> {
    match tab_control {
        ClientTabControl::WatchingControlled { .. } => Some(TabControlChip {
            badge: WATCHING_CHIP_BADGE,
            suffix: " Another Client Controls · Take Over",
            free: false,
        }),
        ClientTabControl::WatchingFree { .. } => Some(TabControlChip {
            badge: FREE_CHIP_BADGE,
            suffix: " Take Control",
            free: true,
        }),
        ClientTabControl::Controlling { .. } | ClientTabControl::Unavailable => None,
    }
}

/// Truncate the desktop chip label for a narrow bar: the hint suffix elides
/// first, then the badge.
fn truncate_tab_control_chip_label(label: &str, max_width: usize) -> String {
    for badge in [WATCHING_CHIP_BADGE, FREE_CHIP_BADGE] {
        if let Some(suffix) = label.strip_prefix(badge) {
            let badge_width = text::display_width(badge);
            if max_width >= badge_width.saturating_add(2) {
                return format!(
                    "{}{}",
                    badge,
                    text::truncate_end(suffix, max_width - badge_width)
                );
            }
            if max_width >= badge_width {
                return badge.to_string();
            }
            return text::truncate_end(label, max_width);
        }
    }
    text::truncate_end(label, max_width)
}

const TAB_CONTROL_ACTIONS: [&str; 2] = ["Take Over", "Take Control"];

fn tab_control_action_span(label: &str) -> Option<(u16, u16)> {
    for action in TAB_CONTROL_ACTIONS {
        if let Some(idx) = label.find(action) {
            let start = text::display_width_u16(&label[..idx]);
            let width = text::display_width_u16(action);
            if width > 0 {
                return Some((start, width));
            }
        }
    }
    None
}

fn tab_control_action_hit_rect(label: &str, rect: Rect) -> Option<Rect> {
    let (start, width) = tab_control_action_span(label)?;
    Some(Rect::new(rect.x.saturating_add(start), rect.y, width, 1))
}

fn context_bar_segment(
    target: crate::app::state::ContextBarTarget,
    label: String,
    rect: Rect,
) -> crate::app::state::ContextBarSegment {
    let hit_rect = (target == crate::app::state::ContextBarTarget::TabControl)
        .then(|| tab_control_action_hit_rect(&label, rect))
        .flatten();
    crate::app::state::ContextBarSegment {
        target,
        label,
        rect,
        hit_rect,
    }
}

fn compute_context_bar(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    active_workspace: Option<usize>,
    active_group: usize,
    active_tab: Option<usize>,
    focused_pane: Option<crate::layout::PaneId>,
    tab_control: ClientTabControl,
    rect: Rect,
) -> crate::app::state::ContextBarView {
    use crate::app::state::{ContextBarTarget, ContextBarView};

    if rect.width < 2 || rect.height == 0 {
        return ContextBarView::default();
    }

    let count_variants = if app.show_counters {
        let group_count = app.groups.len();
        let workspace_count = app.workspaces.len();
        let tab_count = app
            .workspaces
            .iter()
            .map(|workspace| workspace.tabs.len())
            .sum::<usize>();
        [
            format!(
                "{} · {} · {}",
                count_label(group_count, "Group", "Groups"),
                count_label(workspace_count, "Space", "Spaces"),
                count_label(tab_count, "Tab", "Tabs")
            ),
            format!(
                "{} · {}",
                count_label(group_count, "Group", "Groups"),
                count_label(workspace_count, "Space", "Spaces")
            ),
            count_label(group_count, "Group", "Groups"),
            String::new(),
        ]
    } else {
        std::array::from_fn(|_| String::new())
    };

    let workspace = active_workspace.and_then(|index| app.workspaces.get(index));
    let group = workspace
        .and_then(|workspace| app.group_index_by_id(&workspace.group_id))
        .or_else(|| (active_group < app.groups.len()).then_some(active_group))
        .and_then(|index| app.groups.get(index));
    let mut labels = Vec::with_capacity(4);
    if let Some(group) = group {
        labels.push((
            ContextBarTarget::Group,
            // An empty icon would otherwise leave a stray leading column.
            format!("{} {}", group.icon, group.name).trim().to_string(),
        ));
    }
    if let Some(workspace) = workspace {
        labels.push((
            ContextBarTarget::Workspace,
            workspace.display_name_from(&app.terminals, terminal_runtimes),
        ));
        if let Some(tab_idx) = active_tab.filter(|index| *index < workspace.tabs.len()) {
            if let Some(label) = workspace.tab_display_name(tab_idx) {
                labels.push((ContextBarTarget::Tab, label));
            }
        }
        if let Some(tab_idx) = active_tab.filter(|index| *index < workspace.tabs.len()) {
            let tab = &workspace.tabs[tab_idx];
            if tab.panes.len() > 1 {
                if let Some(pane_id) =
                    focused_pane.filter(|pane_id| tab.panes.contains_key(pane_id))
                {
                    let label = workspace
                        .pane_state(pane_id)
                        .and_then(|pane| app.terminals.get(&pane.attached_terminal_id))
                        .and_then(|terminal| terminal.manual_label.clone())
                        .or_else(|| {
                            workspace
                                .pane_display_number(pane_id)
                                .map(|number| format!("Pane {number}"))
                        });
                    if let Some(label) = label {
                        labels.push((ContextBarTarget::Pane, label));
                    }
                }
            }
        }
    }
    // The control chip is always the trailing segment, so the left-drop loop
    // below sacrifices path segments first and keeps the chip longest.
    if let Some(chip) = tab_control_chip(tab_control) {
        labels.push((ContextBarTarget::TabControl, chip.label()));
    }

    let inner_width = rect.width.saturating_sub(2) as usize;
    let separator_width = CONTEXT_BAR_SEPARATOR.len() * labels.len().saturating_sub(1);
    let full_path_width = labels
        .iter()
        .map(|(_, label)| text::display_width(label))
        .sum::<usize>()
        .saturating_add(separator_width);
    let counts = count_variants
        .into_iter()
        .find(|counts| {
            let counts_width = text::display_width(counts);
            let gap = usize::from(!counts.is_empty() && !labels.is_empty()) * 2;
            counts_width
                .saturating_add(gap)
                .saturating_add(full_path_width)
                <= inner_width
        })
        .unwrap_or_default();
    let counts_width = text::display_width(&counts);
    let gap = usize::from(!counts.is_empty() && !labels.is_empty()) * 2;
    let path_available = inner_width.saturating_sub(counts_width.saturating_add(gap));

    while labels.len() > 1
        && labels
            .len()
            .saturating_add(CONTEXT_BAR_SEPARATOR.len() * labels.len().saturating_sub(1))
            > path_available
    {
        labels.remove(0);
    }
    if !labels.is_empty() && path_available > 0 {
        let separators = CONTEXT_BAR_SEPARATOR.len() * labels.len().saturating_sub(1);
        let label_budget = path_available.saturating_sub(separators);
        let mut widths = labels
            .iter()
            .map(|(_, label)| text::display_width(label))
            .collect::<Vec<_>>();
        while widths.iter().sum::<usize>() > label_budget {
            let Some((index, _)) = widths
                .iter()
                .enumerate()
                .filter(|(_, width)| **width > 1)
                .max_by_key(|(_, width)| **width)
            else {
                break;
            };
            widths[index] -= 1;
        }
        for ((target, label), width) in labels.iter_mut().zip(widths) {
            *label = if *target == ContextBarTarget::TabControl {
                truncate_tab_control_chip_label(label, width)
            } else {
                text::truncate_end(label, width)
            };
        }
    } else {
        labels.clear();
    }

    let path_x = rect.x.saturating_add(1);
    let mut cursor = path_x;
    let segments = labels
        .into_iter()
        .enumerate()
        .map(|(index, (target, label))| {
            if index > 0 {
                cursor = cursor.saturating_add(CONTEXT_BAR_SEPARATOR.len() as u16);
            }
            let width = text::display_width_u16(&label);
            let segment = context_bar_segment(target, label, Rect::new(cursor, rect.y, width, 1));
            cursor = cursor.saturating_add(width);
            segment
        })
        .collect();
    let counts_rect = if counts.is_empty() {
        Rect::default()
    } else {
        Rect::new(
            rect.x
                .saturating_add(rect.width)
                .saturating_sub(counts_width as u16)
                .saturating_sub(1),
            rect.y,
            counts_width as u16,
            1,
        )
    };

    ContextBarView {
        rect,
        counts,
        counts_rect,
        segments,
    }
}

fn compute_mobile_breadcrumb(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    active_workspace: Option<usize>,
    active_group: usize,
    active_tab: Option<usize>,
    focused_pane: Option<crate::layout::PaneId>,
    tab_control: ClientTabControl,
    rect: Rect,
) -> crate::app::state::ContextBarView {
    use crate::app::state::{ContextBarSegment, ContextBarTarget, ContextBarView};

    if rect.width < 3 || rect.height == 0 {
        return ContextBarView::default();
    }

    let workspace = active_workspace.and_then(|index| app.workspaces.get(index));
    let group = workspace
        .and_then(|workspace| app.group_index_by_id(&workspace.group_id))
        .or_else(|| (active_group < app.groups.len()).then_some(active_group))
        .and_then(|index| app.groups.get(index));
    let mut labels = Vec::with_capacity(4);
    if let Some(group) = group {
        labels.push((
            ContextBarTarget::Group,
            // An empty icon would otherwise leave a stray leading column.
            format!("{} {}", group.icon, group.name).trim().to_string(),
        ));
    }
    if let Some(workspace) = workspace {
        labels.push((
            ContextBarTarget::Workspace,
            workspace.display_name_from(&app.terminals, terminal_runtimes),
        ));
        if let Some(tab_idx) = active_tab.filter(|index| *index < workspace.tabs.len()) {
            if let Some(label) = workspace.tab_display_name(tab_idx) {
                labels.push((ContextBarTarget::Tab, label));
            }
            let tab = &workspace.tabs[tab_idx];
            if tab.panes.len() > 1 {
                if let Some(pane_id) =
                    focused_pane.filter(|pane_id| tab.panes.contains_key(pane_id))
                {
                    if let Some(label) = workspace
                        .pane_state(pane_id)
                        .and_then(|pane| app.terminals.get(&pane.attached_terminal_id))
                        .and_then(|terminal| terminal.manual_label.clone())
                        .or_else(|| {
                            workspace
                                .pane_display_number(pane_id)
                                .map(|number| format!("Pane {number}"))
                        })
                    {
                        labels.push((ContextBarTarget::Pane, label));
                    }
                }
            }
        }
    }

    const PREFERRED_TAP_WIDTH: usize = 8;
    // Reserve the right edge for the control chip so breadcrumb labels never
    // slide under it; the chip only exists for watching clients and is
    // dropped entirely when the badge cannot fit. Mobile shows the concise
    // badge without the desktop suffix.
    let chip = tab_control_chip(tab_control).and_then(|chip| {
        let max_width = rect.width.saturating_sub(2) as usize;
        (max_width >= text::display_width(chip.badge)).then(|| chip.badge.to_string())
    });
    let chip_reserve = chip
        .as_ref()
        .map(|label| text::display_width(label).saturating_add(1))
        .unwrap_or(0);
    let available = (rect.width.saturating_sub(2) as usize).saturating_sub(chip_reserve);
    // Mirror the desktop left-drop: at tiny widths crumbs that cannot fit
    // even at minimum width ("… ▾") are dropped from the left instead of
    // overdrawing the header row.
    while !labels.is_empty()
        && 3 * labels.len() + CONTEXT_BAR_SEPARATOR.len() * labels.len().saturating_sub(1)
            > available
    {
        labels.remove(0);
    }
    let separator_width = CONTEXT_BAR_SEPARATOR.len() * labels.len().saturating_sub(1);
    let label_budget = available.saturating_sub(separator_width);
    let mut widths = labels
        .iter()
        .map(|(_, label)| {
            text::display_width(label)
                .saturating_add(2)
                .max(PREFERRED_TAP_WIDTH)
        })
        .collect::<Vec<_>>();
    for minimum_width in [PREFERRED_TAP_WIDTH, 3] {
        while widths.iter().sum::<usize>() > label_budget {
            let Some((index, _)) = widths
                .iter()
                .enumerate()
                .filter(|(_, width)| **width > minimum_width)
                .max_by_key(|(_, width)| **width)
            else {
                break;
            };
            widths[index] -= 1;
        }
    }

    let mut cursor = rect.x.saturating_add(1);
    let mut segments: Vec<ContextBarSegment> = labels
        .into_iter()
        .zip(widths)
        .enumerate()
        .map(|(index, ((target, label), width))| {
            if index > 0 {
                cursor = cursor.saturating_add(CONTEXT_BAR_SEPARATOR.len() as u16);
            }
            let text = format!("{} ▾", text::truncate_end(&label, width.saturating_sub(2)));
            let padding = width.saturating_sub(text::display_width(&text));
            let leading_padding = padding / 2;
            let label = format!(
                "{}{}{}",
                " ".repeat(leading_padding),
                text,
                " ".repeat(padding.saturating_sub(leading_padding))
            );
            let segment =
                context_bar_segment(target, label, Rect::new(cursor, rect.y, width as u16, 1));
            cursor = cursor.saturating_add(width as u16);
            segment
        })
        .collect();
    if let Some(chip_label) = chip {
        let width = text::display_width_u16(&chip_label);
        if width > 0 {
            let x = rect
                .x
                .saturating_add(rect.width)
                .saturating_sub(width)
                .saturating_sub(1);
            segments.push(context_bar_segment(
                ContextBarTarget::TabControl,
                chip_label,
                Rect::new(x, rect.y, width, 1),
            ));
        }
    }

    ContextBarView {
        rect,
        counts: String::new(),
        counts_rect: Rect::default(),
        segments,
    }
}

fn render_context_bar(
    app: &AppState,
    context_bar: &crate::app::state::ContextBarView,
    frame: &mut Frame,
) {
    if context_bar.rect == Rect::default() {
        return;
    }
    fill_rect(
        frame,
        context_bar.rect,
        Style::default()
            .fg(app.palette.overlay1)
            .bg(app.palette.surface0),
    );
    if context_bar.counts_rect != Rect::default() {
        frame.render_widget(
            Paragraph::new(context_bar.counts.as_str()).style(
                Style::default()
                    .fg(app.palette.overlay1)
                    .bg(app.palette.surface0),
            ),
            context_bar.counts_rect,
        );
    }
    for (index, segment) in context_bar.segments.iter().enumerate() {
        if index > 0 {
            let separator_x = segment
                .rect
                .x
                .saturating_sub(CONTEXT_BAR_SEPARATOR.len() as u16);
            let previous = context_bar.segments[index - 1].rect;
            // The right-aligned mobile chip leaves a gap before it; only draw
            // a separator between contiguous path segments.
            if separator_x == previous.x.saturating_add(previous.width) {
                let separator = Rect::new(
                    separator_x,
                    segment.rect.y,
                    CONTEXT_BAR_SEPARATOR.len() as u16,
                    1,
                );
                frame.render_widget(
                    Paragraph::new(CONTEXT_BAR_SEPARATOR).style(
                        Style::default()
                            .fg(app.palette.overlay0)
                            .bg(app.palette.surface0),
                    ),
                    separator,
                );
            }
        }
        if segment.target == crate::app::state::ContextBarTarget::TabControl {
            render_tab_control_chip_segment(app, segment, frame);
            continue;
        }
        let style = if index + 1 == context_bar.segments.len() {
            Style::default()
                .fg(app.palette.text)
                .bg(app.palette.surface0)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default()
                .fg(app.palette.overlay1)
                .bg(app.palette.surface0)
                .add_modifier(Modifier::UNDERLINED)
        };
        frame.render_widget(
            Paragraph::new(Line::from(segment.label.as_str())).style(style),
            segment.rect,
        );
    }
}

fn render_tab_control_chip_segment(
    app: &AppState,
    segment: &crate::app::state::ContextBarSegment,
    frame: &mut Frame,
) {
    if segment.rect.width == 0 || segment.rect.height == 0 {
        return;
    }
    let p = &app.palette;
    let (badge, suffix, free) =
        if let Some(suffix) = segment.label.strip_prefix(WATCHING_CHIP_BADGE) {
            (WATCHING_CHIP_BADGE, suffix, false)
        } else if let Some(suffix) = segment.label.strip_prefix(FREE_CHIP_BADGE) {
            (FREE_CHIP_BADGE, suffix, true)
        } else {
            // Extreme truncation cut into the badge itself; render what remains.
            (segment.label.as_str(), "", false)
        };
    let badge_bg = if free { p.teal } else { p.overlay0 };
    let (hint, action) = match tab_control_action_span(suffix) {
        Some((start, width)) => {
            let start = start as usize;
            let hint: String = suffix.chars().take(start).collect();
            let action: String = suffix.chars().skip(start).take(width as usize).collect();
            (hint, action)
        }
        None => (suffix.to_string(), String::new()),
    };
    let mut spans = Vec::new();
    let status_style = if action.is_empty() {
        Style::default()
            .fg(widgets::panel_contrast_fg(p))
            .bg(badge_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(p.overlay1).bg(p.surface0)
    };
    spans.push(Span::styled(badge.to_string(), status_style));
    if !hint.is_empty() {
        spans.push(Span::styled(
            hint,
            Style::default().fg(p.overlay0).bg(p.surface0),
        ));
    }
    if !action.is_empty() {
        spans.push(Span::styled(
            action,
            Style::default()
                .fg(widgets::panel_contrast_fg(p))
                .bg(badge_bg)
                .add_modifier(Modifier::BOLD),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), segment.rect);
}

const SPINNERS: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub(super) fn spinner_frame(tick: u32) -> &'static str {
    SPINNERS[(tick as usize / crate::app::ANIMATION_TICK_STEP as usize) % SPINNERS.len()]
}

/// Compute view geometry and reconcile pane sizes.
/// Called before render to separate mutation from drawing.
#[cfg_attr(not(test), allow(dead_code))]
pub fn compute_view(app: &mut AppState, area: Rect) {
    let terminal_runtimes = TerminalRuntimeRegistry::new();
    compute_view_with_runtime_registry(app, &terminal_runtimes, area);
}

pub fn compute_view_with_runtime_registry(
    app: &mut AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
) {
    compute_view_internal(
        app,
        terminal_runtimes,
        area,
        true,
        crate::kitty_graphics::HostCellSize::default(),
    );
}

pub fn compute_view_with_cell_size(
    app: &mut AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
    cell_size: crate::kitty_graphics::HostCellSize,
) {
    compute_view_internal(app, terminal_runtimes, area, true, cell_size);
}

/// Compute view geometry for a client-sized render without resizing pane runtimes.
///
/// This is used by the headless server when a client needs its own frame size
/// while pane runtimes remain sized by the controlling tab's explicit compute.
pub(crate) fn compute_view_without_resizing_panes(
    app: &mut AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
) {
    compute_view_internal(
        app,
        terminal_runtimes,
        area,
        false,
        crate::kitty_graphics::HostCellSize::default(),
    );
}

pub(crate) fn compute_view_for_client_with_cell_size(
    app: &AppState,
    client_view: &mut ClientViewState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
    cell_size: crate::kitty_graphics::HostCellSize,
) {
    compute_view_for_client_internal(app, client_view, terminal_runtimes, area, true, cell_size);
}

pub(crate) fn compute_view_for_client_without_resizing_panes(
    app: &AppState,
    client_view: &mut ClientViewState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
) {
    compute_view_for_client_internal(
        app,
        client_view,
        terminal_runtimes,
        area,
        false,
        crate::kitty_graphics::HostCellSize::default(),
    );
}

fn client_tab_canvas_view(
    app: &AppState,
    client_view: &mut ClientViewState,
    terminal_area: Rect,
    resize_panes: bool,
) -> (Rect, bool) {
    let (canvas_width, canvas_height, resize_pane_runtimes) = if client_view.can_mutate_tab() {
        (terminal_area.width, terminal_area.height, resize_panes)
    } else {
        let (width, height) = client_view
            .tab_canvas_size
            .unwrap_or((terminal_area.width, terminal_area.height));
        (width, height, false)
    };
    let canvas_size = ratatui::layout::Size::new(canvas_width, canvas_height);
    let canvas_area = Rect::new(0, 0, canvas_width, canvas_height);
    let Some(ws_idx) = client_view.active_workspace else {
        client_view.tab_canvas_view = None;
        return (canvas_area, resize_pane_runtimes);
    };
    let Some(workspace) = app.workspaces.get(ws_idx) else {
        client_view.tab_canvas_view = None;
        return (canvas_area, resize_pane_runtimes);
    };
    let Some(tab_idx) = client_view.active_tab_index_for_workspace(app, ws_idx) else {
        client_view.tab_canvas_view = None;
        return (canvas_area, resize_pane_runtimes);
    };
    let Some(tab) = workspace.tabs.get(tab_idx) else {
        client_view.tab_canvas_view = None;
        return (canvas_area, resize_pane_runtimes);
    };
    let key = ClientTabViewKey::new(&workspace.id, tab.number);
    let origin = if client_view.can_mutate_tab() {
        CanvasOrigin::default()
    } else {
        client_view.tab_canvas_origin(&key)
    };
    let mut canvas_view = TabCanvasViewport::new(canvas_size, terminal_area, origin);
    let focused_pane = client_view
        .focused_pane_for_tab(&workspace.id, tab.number)
        .filter(|pane_id| tab.panes.contains_key(pane_id))
        .unwrap_or_else(|| tab.layout.focused());
    let focused_rect = if client_view.tab_is_zoomed(&workspace.id, tab.number) {
        Some(canvas_area)
    } else {
        tab.layout
            .panes(canvas_area)
            .into_iter()
            .find(|info| info.id == focused_pane)
            .map(|info| info.rect)
    };
    if let Some(focused_rect) = focused_rect {
        let revealed_origin = canvas_view.reveal_focused(canvas_view.origin, focused_rect);
        if revealed_origin != canvas_view.origin {
            canvas_view = TabCanvasViewport::new(canvas_size, terminal_area, revealed_origin);
        }
    }
    client_view.set_tab_canvas_view(key, canvas_view);
    (canvas_area, resize_pane_runtimes)
}

fn hide_tab_bar_when_single_tab(app: &AppState) -> bool {
    app.settings
        .pending_hide_tab_bar_when_single_tab
        .unwrap_or(app.hide_tab_bar_when_single_tab)
}

fn tab_bar_layout(
    hide_when_single: bool,
    zen_mode: bool,
    tab_count: usize,
    main_area: Rect,
) -> (Rect, Rect) {
    let show =
        !zen_mode && main_area.height > 1 && tab_count > 0 && !(hide_when_single && tab_count <= 1);
    if show {
        let [tab_bar_rect, terminal_area] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(main_area);
        (tab_bar_rect, terminal_area)
    } else {
        (Rect::default(), main_area)
    }
}

fn compute_view_for_client_internal(
    app: &AppState,
    client_view: &mut ClientViewState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
    resize_panes: bool,
    cell_size: crate::kitty_graphics::HostCellSize,
) {
    if let Some(screen) = client_view.github.as_mut() {
        screen.compute(area);
    }
    if is_mobile_width(area, app.mobile_width_threshold) {
        compute_mobile_view_for_client(
            app,
            client_view,
            terminal_runtimes,
            area,
            resize_panes,
            cell_size,
        );
        return;
    }
    let show_context_bar = !client_view.zen_mode
        && app.context_bar_is_visible(client_view.context_bar_visibility_override);
    let (content_area, context_bar_rect) = desktop_content_areas(area, show_context_bar);

    let sidebar_w = if client_view.sidebar_collapsed {
        COLLAPSED_WIDTH
    } else {
        client_view
            .sidebar_width
            .clamp(app.sidebar_min_width, app.sidebar_max_width)
    };
    let right_sidebar_w = if client_view.right_sidebar_collapsed {
        COLLAPSED_WIDTH
    } else {
        client_view
            .right_sidebar_width
            .clamp(MIN_RIGHT_SIDEBAR_WIDTH, MAX_RIGHT_SIDEBAR_WIDTH)
    };

    let auto_separate = area.width
        >= sidebar_w
            .saturating_add(right_sidebar_w)
            .saturating_add(RIGHT_SIDEBAR_MIN_TERMINAL_WIDTH);
    let separate_sidebars = match app.sidebar_arrangement {
        crate::config::SidebarArrangementConfig::Auto => auto_separate,
        crate::config::SidebarArrangementConfig::Separate => true,
        crate::config::SidebarArrangementConfig::CombinedLeft
        | crate::config::SidebarArrangementConfig::CombinedRight => false,
    };
    let combined_right =
        app.sidebar_arrangement == crate::config::SidebarArrangementConfig::CombinedRight;
    let (sidebar_area, main_area, right_sidebar_area) = if client_view.zen_mode {
        (Rect::default(), content_area, Rect::default())
    } else if separate_sidebars {
        let [sidebar_area, main_area, right_sidebar_area] = Layout::horizontal([
            Constraint::Length(sidebar_w),
            Constraint::Min(1),
            Constraint::Length(right_sidebar_w),
        ])
        .areas(content_area);
        (sidebar_area, main_area, right_sidebar_area)
    } else if combined_right {
        let [main_area, sidebar_area] =
            Layout::horizontal([Constraint::Min(1), Constraint::Length(sidebar_w)])
                .areas(content_area);
        (sidebar_area, main_area, Rect::default())
    } else {
        let [sidebar_area, main_area] =
            Layout::horizontal([Constraint::Length(sidebar_w), Constraint::Min(1)])
                .areas(content_area);
        (sidebar_area, main_area, Rect::default())
    };

    let active_workspace = client_view
        .active_workspace
        .and_then(|idx| app.workspaces.get(idx));
    let (tab_bar_rect, terminal_area) = tab_bar_layout(
        hide_tab_bar_when_single_tab(app),
        client_view.zen_mode,
        active_workspace
            .map(|workspace| workspace.tabs.len())
            .unwrap_or(0),
        main_area,
    );

    client_view.workspace_scroll = client_view
        .workspace_scroll
        .min(workspace_list_entry_count_for_view(app, client_view).saturating_sub(1));
    if !client_view.zen_mode {
        if right_sidebar_area != Rect::default() && !client_view.right_sidebar_collapsed {
            let max_agent_scroll = agent_panel_scroll_metrics_for_view(
                app,
                terminal_runtimes,
                client_view,
                right_sidebar_content_rect(right_sidebar_area),
                false,
            )
            .max_offset_from_bottom;
            client_view.agent_panel_scroll = client_view.agent_panel_scroll.min(max_agent_scroll);
        } else if right_sidebar_area == Rect::default() && !client_view.sidebar_collapsed {
            let (_, agent_area) = if combined_right {
                right_aligned_expanded_sidebar_sections(
                    sidebar_area,
                    client_view.sidebar_section_split,
                )
            } else {
                expanded_sidebar_sections(sidebar_area, client_view.sidebar_section_split)
            };
            let max_agent_scroll = agent_panel_scroll_metrics_for_view(
                app,
                terminal_runtimes,
                client_view,
                agent_area,
                true,
            )
            .max_offset_from_bottom;
            client_view.agent_panel_scroll = client_view.agent_panel_scroll.min(max_agent_scroll);
        } else {
            client_view.agent_panel_scroll = 0;
        }
    }

    let (workspace_card_areas, workspace_group_header_areas, workspace_group_empty_areas) =
        if client_view.zen_mode || client_view.sidebar_collapsed {
            (Vec::new(), Vec::new(), Vec::new())
        } else if right_sidebar_area != Rect::default() {
            let ws_area = left_sidebar_workspace_rect(sidebar_area);
            (
                compute_workspace_card_areas_in_list_for_view(app, client_view, ws_area),
                compute_workspace_group_header_areas_in_list_for_view(app, client_view, ws_area),
                compute_workspace_group_empty_areas_in_list_for_view(app, client_view, ws_area),
            )
        } else if combined_right {
            let ws_area =
                right_aligned_workspace_list_rect(sidebar_area, client_view.sidebar_section_split);
            (
                compute_workspace_card_areas_in_list_for_view(app, client_view, ws_area),
                compute_workspace_group_header_areas_in_list_for_view(app, client_view, ws_area),
                compute_workspace_group_empty_areas_in_list_for_view(app, client_view, ws_area),
            )
        } else {
            let ws_area = workspace_list_rect(sidebar_area, client_view.sidebar_section_split);
            (
                compute_workspace_card_areas_in_list_for_view(app, client_view, ws_area),
                compute_workspace_group_header_areas_in_list_for_view(app, client_view, ws_area),
                compute_workspace_group_empty_areas_in_list_for_view(app, client_view, ws_area),
            )
        };

    let tab_bar_view = client_view
        .active_workspace
        .and_then(|idx| app.workspaces.get(idx))
        .map(|workspace| {
            compute_tab_bar_view(
                workspace,
                tab_bar_rect,
                client_view.tab_scroll,
                client_view.tab_scroll_follow_active,
                app.mouse_capture,
                client_view.hovered_tab,
            )
        })
        .unwrap_or_default();
    client_view.tab_scroll = tab_bar_view.scroll;

    let (pane_area, resize_pane_runtimes) =
        client_tab_canvas_view(app, client_view, terminal_area, resize_panes);
    let split_borders = client_view
        .active_workspace
        .and_then(|idx| {
            let workspace = app.workspaces.get(idx)?;
            let tab_idx = client_view.active_tab_index_for_workspace(app, idx)?;
            workspace.tabs.get(tab_idx)
        })
        .map(|tab| tab.layout.splits(pane_area))
        .unwrap_or_default();
    let pane_infos = compute_pane_infos_for_view(
        app,
        client_view,
        terminal_runtimes,
        pane_area,
        resize_pane_runtimes,
        cell_size,
    );
    if resize_panes {
        resize_popup_pane_for_view(app, client_view, terminal_runtimes, area, cell_size);
    }

    let toast_hit_area = app
        .toast
        .as_ref()
        .map(|toast| {
            toast_notification_rect(
                area,
                toast,
                toast.position.unwrap_or(app.toast_config.gardn.position),
            )
        })
        .unwrap_or_default();

    let active_tab = client_view
        .active_workspace
        .and_then(|ws_idx| client_view.active_tab_index_for_workspace(app, ws_idx));
    let focused_pane = client_view
        .active_workspace
        .and_then(|ws_idx| client_view.focused_pane_for_workspace(app, ws_idx))
        .map(|(_, pane_id)| pane_id);
    let context_bar = compute_context_bar(
        app,
        terminal_runtimes,
        client_view.active_workspace,
        client_view.active_group,
        active_tab,
        focused_pane,
        client_view.tab_control,
        context_bar_rect,
    );

    client_view.computed = crate::app::ViewState {
        layout: ViewLayout::Desktop,
        sidebar_rect: sidebar_area,
        right_sidebar_rect: right_sidebar_area,
        workspace_card_areas,
        workspace_group_header_areas,
        workspace_group_empty_areas,
        tab_bar_rect,
        tab_hit_areas: tab_bar_view.tab_hit_areas,
        tab_close_hit_areas: tab_bar_view.tab_close_hit_areas,
        tab_scroll_left_hit_area: tab_bar_view.scroll_left_hit_area,
        tab_scroll_right_hit_area: tab_bar_view.scroll_right_hit_area,
        new_tab_hit_area: tab_bar_view.new_tab_hit_area,
        context_bar,
        terminal_area,
        mobile_header_rect: Rect::default(),
        toast_hit_area,
        pane_infos,
        split_borders,
    };
}

fn compute_view_internal(
    app: &mut AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
    resize_panes: bool,
    cell_size: crate::kitty_graphics::HostCellSize,
) {
    if is_mobile_width(area, app.mobile_width_threshold) {
        compute_mobile_view(app, terminal_runtimes, area, resize_panes, cell_size);
        return;
    }
    let show_context_bar =
        !app.zen_mode && app.context_bar_is_visible(app.context_bar_visibility_override);
    let (content_area, context_bar_rect) = desktop_content_areas(area, show_context_bar);

    let sidebar_w = if app.sidebar_collapsed {
        COLLAPSED_WIDTH
    } else {
        app.sidebar_width
            .clamp(app.sidebar_min_width, app.sidebar_max_width)
    };
    let right_sidebar_w = if app.right_sidebar_collapsed {
        COLLAPSED_WIDTH
    } else {
        app.right_sidebar_width
            .clamp(MIN_RIGHT_SIDEBAR_WIDTH, MAX_RIGHT_SIDEBAR_WIDTH)
    };

    let auto_separate = area.width
        >= sidebar_w
            .saturating_add(right_sidebar_w)
            .saturating_add(RIGHT_SIDEBAR_MIN_TERMINAL_WIDTH);
    let separate_sidebars = match app.sidebar_arrangement {
        crate::config::SidebarArrangementConfig::Auto => auto_separate,
        crate::config::SidebarArrangementConfig::Separate => true,
        crate::config::SidebarArrangementConfig::CombinedLeft
        | crate::config::SidebarArrangementConfig::CombinedRight => false,
    };
    let combined_right =
        app.sidebar_arrangement == crate::config::SidebarArrangementConfig::CombinedRight;
    let (sidebar_area, main_area, right_sidebar_area) = if app.zen_mode {
        (Rect::default(), content_area, Rect::default())
    } else if separate_sidebars {
        let [sidebar_area, main_area, right_sidebar_area] = Layout::horizontal([
            Constraint::Length(sidebar_w),
            Constraint::Min(1),
            Constraint::Length(right_sidebar_w),
        ])
        .areas(content_area);
        (sidebar_area, main_area, right_sidebar_area)
    } else if combined_right {
        let [main_area, sidebar_area] =
            Layout::horizontal([Constraint::Min(1), Constraint::Length(sidebar_w)])
                .areas(content_area);
        (sidebar_area, main_area, Rect::default())
    } else {
        let [sidebar_area, main_area] =
            Layout::horizontal([Constraint::Length(sidebar_w), Constraint::Min(1)])
                .areas(content_area);
        (sidebar_area, main_area, Rect::default())
    };

    let (tab_bar_rect, terminal_area) = tab_bar_layout(
        hide_tab_bar_when_single_tab(app),
        app.zen_mode,
        app.active
            .and_then(|idx| app.workspaces.get(idx))
            .map(|workspace| workspace.tabs.len())
            .unwrap_or(0),
        main_area,
    );

    app.workspace_scroll = app
        .workspace_scroll
        .min(workspace_list_entry_count(app).saturating_sub(1));
    if !app.zen_mode {
        if right_sidebar_area != Rect::default() && !app.right_sidebar_collapsed {
            let max_agent_scroll = agent_panel_scroll_metrics(
                app,
                right_sidebar_content_rect(right_sidebar_area),
                false,
            )
            .max_offset_from_bottom;
            app.agent_panel_scroll = app.agent_panel_scroll.min(max_agent_scroll);
        } else if right_sidebar_area == Rect::default() && !app.sidebar_collapsed {
            let (_, agent_area) = if combined_right {
                right_aligned_expanded_sidebar_sections(sidebar_area, app.sidebar_section_split)
            } else {
                expanded_sidebar_sections(sidebar_area, app.sidebar_section_split)
            };
            let max_agent_scroll =
                agent_panel_scroll_metrics(app, agent_area, true).max_offset_from_bottom;
            app.agent_panel_scroll = app.agent_panel_scroll.min(max_agent_scroll);
        } else {
            app.agent_panel_scroll = 0;
        }
    }

    let (workspace_card_areas, workspace_group_header_areas, workspace_group_empty_areas) =
        if app.zen_mode || app.sidebar_collapsed {
            (Vec::new(), Vec::new(), Vec::new())
        } else if right_sidebar_area != Rect::default() {
            let ws_area = left_sidebar_workspace_rect(sidebar_area);
            (
                compute_workspace_card_areas_in_list(app, ws_area),
                compute_workspace_group_header_areas_in_list(app, ws_area),
                compute_workspace_group_empty_areas_in_list(app, ws_area),
            )
        } else if combined_right {
            let ws_area =
                right_aligned_workspace_list_rect(sidebar_area, app.sidebar_section_split);
            (
                compute_workspace_card_areas_in_list(app, ws_area),
                compute_workspace_group_header_areas_in_list(app, ws_area),
                compute_workspace_group_empty_areas_in_list(app, ws_area),
            )
        } else {
            let ws_area = workspace_list_rect(sidebar_area, app.sidebar_section_split);
            (
                compute_workspace_card_areas_in_list(app, ws_area),
                compute_workspace_group_header_areas_in_list(app, ws_area),
                compute_workspace_group_empty_areas_in_list(app, ws_area),
            )
        };

    let tab_bar_view = app
        .active
        .and_then(|i| app.workspaces.get(i))
        .map(|ws| {
            compute_tab_bar_view(
                ws,
                tab_bar_rect,
                app.tab_scroll,
                app.tab_scroll_follow_active,
                app.mouse_capture,
                app.hovered_tab,
            )
        })
        .unwrap_or_default();
    app.tab_scroll = tab_bar_view.scroll;

    let split_borders = app
        .active
        .and_then(|i| app.workspaces.get(i))
        .and_then(|ws| ws.active_tab())
        .map(|tab| tab.layout.splits(terminal_area))
        .unwrap_or_default();

    let pane_infos = compute_pane_infos(
        app,
        terminal_runtimes,
        terminal_area,
        resize_panes,
        cell_size,
    );

    let toast_hit_area = app
        .toast
        .as_ref()
        .map(|toast| {
            toast_notification_rect(
                area,
                toast,
                toast.position.unwrap_or(app.toast_config.gardn.position),
            )
        })
        .unwrap_or_default();

    let active_tab = app
        .active
        .and_then(|ws_idx| app.workspaces.get(ws_idx))
        .map(|workspace| workspace.active_tab_index());
    let focused_pane = app
        .active
        .and_then(|ws_idx| app.workspaces.get(ws_idx))
        .and_then(crate::workspace::Workspace::focused_pane_id);
    let context_bar = compute_context_bar(
        app,
        terminal_runtimes,
        app.active,
        app.active_group,
        active_tab,
        focused_pane,
        ClientTabControl::default(),
        context_bar_rect,
    );

    app.view = crate::app::ViewState {
        layout: ViewLayout::Desktop,
        sidebar_rect: sidebar_area,
        right_sidebar_rect: right_sidebar_area,
        workspace_card_areas,
        workspace_group_header_areas,
        workspace_group_empty_areas,
        tab_bar_rect,
        tab_hit_areas: tab_bar_view.tab_hit_areas,
        tab_close_hit_areas: tab_bar_view.tab_close_hit_areas,
        tab_scroll_left_hit_area: tab_bar_view.scroll_left_hit_area,
        tab_scroll_right_hit_area: tab_bar_view.scroll_right_hit_area,
        new_tab_hit_area: tab_bar_view.new_tab_hit_area,
        context_bar,
        terminal_area,
        mobile_header_rect: Rect::default(),
        toast_hit_area,
        pane_infos,
        split_borders,
    };
    app.sync_copy_mode_search_geometry();
}

fn compute_mobile_view(
    app: &mut AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
    resize_panes: bool,
    cell_size: crate::kitty_graphics::HostCellSize,
) {
    let header_h = if app.zen_mode {
        0
    } else {
        area.height.min(MOBILE_HEADER_HEIGHT)
    };
    let (header_rect, terminal_area) = if header_h == 0 {
        (Rect::default(), area)
    } else if area.height > header_h {
        let [header_rect, terminal_area] =
            Layout::vertical([Constraint::Length(header_h), Constraint::Min(1)]).areas(area);
        (header_rect, terminal_area)
    } else {
        (area, Rect::default())
    };

    if app.mode == Mode::Navigate {
        let chrome_height = if app.mobile_agents_expanded {
            MOBILE_AGENT_PANEL_CHROME_HEIGHT
        } else {
            MOBILE_HEADER_HEIGHT.saturating_add(2)
        };
        let switcher_viewport_h = area.height.saturating_sub(chrome_height);
        let max_scroll = mobile_switcher_max_scroll_for_height(app, switcher_viewport_h);
        app.mobile_switcher_scroll = app.mobile_switcher_scroll.min(max_scroll);
    }

    let split_borders = app
        .active
        .and_then(|i| app.workspaces.get(i))
        .and_then(|ws| ws.active_tab())
        .map(|tab| tab.layout.splits(terminal_area))
        .unwrap_or_default();

    let pane_infos = compute_pane_infos(
        app,
        terminal_runtimes,
        terminal_area,
        resize_panes,
        cell_size,
    );
    let breadcrumb_rect = Rect::new(
        header_rect.x,
        header_rect.y.saturating_add(1),
        header_rect.width,
        1,
    );
    let active_tab = app
        .active
        .and_then(|ws_idx| app.workspaces.get(ws_idx))
        .map(crate::workspace::Workspace::active_tab_index);
    let focused_pane = app
        .active
        .and_then(|ws_idx| app.workspaces.get(ws_idx))
        .and_then(crate::workspace::Workspace::focused_pane_id);
    let breadcrumb = compute_mobile_breadcrumb(
        app,
        terminal_runtimes,
        app.active,
        app.active_group,
        active_tab,
        focused_pane,
        ClientTabControl::default(),
        breadcrumb_rect,
    );

    let toast_hit_area = app
        .toast
        .as_ref()
        .map(|_| mobile_toast_banner_rect(area))
        .unwrap_or_default();

    app.view = crate::app::ViewState {
        layout: ViewLayout::Mobile,
        sidebar_rect: Rect::default(),
        right_sidebar_rect: Rect::default(),
        workspace_card_areas: Vec::new(),
        workspace_group_header_areas: Vec::new(),
        workspace_group_empty_areas: Vec::new(),
        tab_bar_rect: Rect::default(),
        tab_hit_areas: Vec::new(),
        tab_close_hit_areas: Vec::new(),
        tab_scroll_left_hit_area: Rect::default(),
        tab_scroll_right_hit_area: Rect::default(),
        new_tab_hit_area: Rect::default(),
        context_bar: breadcrumb,
        terminal_area,
        mobile_header_rect: header_rect,
        toast_hit_area,
        pane_infos,
        split_borders,
    };
    app.sync_copy_mode_search_geometry();
}

fn compute_mobile_view_for_client(
    app: &AppState,
    client_view: &mut ClientViewState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
    resize_panes: bool,
    cell_size: crate::kitty_graphics::HostCellSize,
) {
    let header_h = if client_view.zen_mode {
        0
    } else {
        area.height.min(MOBILE_HEADER_HEIGHT)
    };
    let (header_rect, terminal_area) = if header_h == 0 {
        (Rect::default(), area)
    } else if area.height > header_h {
        let [header_rect, terminal_area] =
            Layout::vertical([Constraint::Length(header_h), Constraint::Min(1)]).areas(area);
        (header_rect, terminal_area)
    } else {
        (area, Rect::default())
    };

    if client_view.mode == Mode::Navigate {
        let chrome_height = if client_view.mobile_agents_expanded {
            MOBILE_AGENT_PANEL_CHROME_HEIGHT
        } else {
            MOBILE_HEADER_HEIGHT.saturating_add(2)
        };
        let switcher_viewport_h = area.height.saturating_sub(chrome_height);
        let max_scroll = mobile_switcher_max_scroll_for_view_height(
            app,
            terminal_runtimes,
            client_view,
            switcher_viewport_h,
        );
        client_view.mobile_switcher_scroll = client_view.mobile_switcher_scroll.min(max_scroll);
    }

    let (pane_area, resize_pane_runtimes) =
        client_tab_canvas_view(app, client_view, terminal_area, resize_panes);
    let split_borders = client_view
        .active_workspace
        .and_then(|idx| {
            let workspace = app.workspaces.get(idx)?;
            let tab_idx = client_view.active_tab_index_for_workspace(app, idx)?;
            workspace.tabs.get(tab_idx)
        })
        .map(|tab| tab.layout.splits(pane_area))
        .unwrap_or_default();

    let pane_infos = compute_pane_infos_for_view(
        app,
        client_view,
        terminal_runtimes,
        pane_area,
        resize_pane_runtimes,
        cell_size,
    );
    let breadcrumb_rect = Rect::new(
        header_rect.x,
        header_rect.y.saturating_add(1),
        header_rect.width,
        1,
    );
    let active_tab = client_view
        .active_workspace
        .and_then(|ws_idx| client_view.active_tab_index_for_workspace(app, ws_idx));
    let focused_pane = client_view
        .active_workspace
        .and_then(|ws_idx| client_view.focused_pane_for_workspace(app, ws_idx))
        .map(|(_, pane_id)| pane_id);
    let breadcrumb = compute_mobile_breadcrumb(
        app,
        terminal_runtimes,
        client_view.active_workspace,
        client_view.active_group,
        active_tab,
        focused_pane,
        client_view.tab_control,
        breadcrumb_rect,
    );

    let toast_hit_area = app
        .toast
        .as_ref()
        .map(|_| mobile_toast_banner_rect(area))
        .unwrap_or_default();

    client_view.computed = crate::app::ViewState {
        layout: ViewLayout::Mobile,
        sidebar_rect: Rect::default(),
        right_sidebar_rect: Rect::default(),
        workspace_card_areas: Vec::new(),
        workspace_group_header_areas: Vec::new(),
        workspace_group_empty_areas: Vec::new(),
        tab_bar_rect: Rect::default(),
        tab_hit_areas: Vec::new(),
        tab_close_hit_areas: Vec::new(),
        tab_scroll_left_hit_area: Rect::default(),
        tab_scroll_right_hit_area: Rect::default(),
        new_tab_hit_area: Rect::default(),
        context_bar: breadcrumb,
        terminal_area,
        mobile_header_rect: header_rect,
        toast_hit_area,
        pane_infos,
        split_borders,
    };
}

/// Render the UI — reads AppState but does not mutate it.
#[cfg_attr(not(test), allow(dead_code))]
pub fn render(app: &AppState, frame: &mut Frame) {
    let terminal_runtimes = TerminalRuntimeRegistry::new();
    render_with_runtime_registry(app, &terminal_runtimes, frame);
}

pub fn render_with_runtime_registry(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
) {
    fill_rect(
        frame,
        frame.area(),
        Style::default().bg(app.palette.panel_bg),
    );
    let sidebar_area = app.view.sidebar_rect;
    let right_sidebar_area = app.view.right_sidebar_rect;
    let tab_bar_area = app.view.tab_bar_rect;
    let terminal_area = app.view.terminal_area;

    if app.view.layout == ViewLayout::Mobile {
        if !app.zen_mode {
            render_mobile_header(app, terminal_runtimes, frame, app.view.mobile_header_rect);
        }
    } else if !app.zen_mode {
        if app.sidebar_collapsed {
            render_sidebar_collapsed(app, frame, sidebar_area);
        } else {
            render_sidebar(app, terminal_runtimes, frame, sidebar_area);
        }
    }
    if !app.zen_mode && app.view.layout != ViewLayout::Mobile {
        render_tab_bar(app, frame, tab_bar_area);
    }
    render_panes(app, terminal_runtimes, frame, terminal_area);
    if right_sidebar_area != Rect::default() {
        render_right_sidebar(app, terminal_runtimes, frame, right_sidebar_area);
    }
    if !app.zen_mode
        && (app.sidebar_collapsed || app.right_sidebar_collapsed)
        && app.view.layout != ViewLayout::Mobile
    {
        render_collapsed_sidebar_hover(app, frame);
    }
    render_context_bar(app, &app.view.context_bar, frame);

    match app.mode {
        Mode::Onboarding => render_onboarding_overlay(app, frame, frame.area()),
        Mode::ReleaseNotes => render_release_notes_overlay(app, frame, frame.area()),
        Mode::ProductAnnouncement => render_product_announcement_overlay(app, frame, frame.area()),
        Mode::Navigate if app.view.layout == ViewLayout::Mobile => {
            render_mobile_panel(app, terminal_runtimes, frame, frame.area())
        }
        Mode::Navigate => render_navigate_overlay(app, frame, terminal_area),
        Mode::Prefix => render_prefix_overlay(app, frame, terminal_area),
        Mode::Copy => render_copy_mode_overlay(app, frame, terminal_area),
        Mode::Resize => render_resize_overlay(app, frame, terminal_area),
        Mode::ConfirmClose => {
            render_confirm_close_overlay(app, terminal_runtimes, frame, terminal_area)
        }
        Mode::ConfirmDeleteGroup => render_confirm_delete_group_overlay(app, frame, terminal_area),
        Mode::ContextMenu => render_context_menu(app, frame),
        Mode::Settings => render_settings_overlay(app, frame, frame.area()),
        Mode::RenameWorkspace | Mode::RenameGroup | Mode::RenameTab | Mode::RenamePane => {
            render_rename_overlay(app, frame, frame.area())
        }
        Mode::GlobalMenu => render_global_launcher_menu(app, frame),
        Mode::GroupMenu => render_group_menu(app, frame),
        Mode::AgentMenu => render_agent_menu(app, frame),
        Mode::KeybindHelp => render_keybind_help_overlay(app, frame),
        Mode::Navigator => render_navigator_overlay(app, frame),
        Mode::CommandPalette => render_command_palette_overlay(app, frame),
        Mode::AgentProfilePicker => render_agent_profile_picker_overlay(app, frame),
        Mode::GitRepoPicker => render_git_repo_picker_overlay(app, frame),
        Mode::ConfigDiagnostics => render_config_diagnostics_overlay(app, frame),
        Mode::Terminal | Mode::Github => {}
    }
    // Notifications remain legible above interactive overlays.
    render_notifications(app, frame, terminal_area);
}

pub(crate) fn render_loop_debug(frame: &mut Frame, line: &str, bg: Color, fg: Color) {
    let area = frame.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    let width = (line.chars().count() as u16).min(area.width).max(1);
    let x = area.x + area.width.saturating_sub(width);
    let y = area.y + area.height.saturating_sub(1);
    frame.render_widget(
        Paragraph::new(line).style(Style::default().fg(fg).bg(bg)),
        Rect::new(x, y, width, 1),
    );
}

pub fn render_with_runtime_registry_for_view(
    app: &AppState,
    client_view: &ClientViewState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
) {
    fill_rect(
        frame,
        frame.area(),
        Style::default().bg(app.palette.panel_bg),
    );
    let sidebar_area = client_view.computed.sidebar_rect;
    let right_sidebar_area = client_view.computed.right_sidebar_rect;
    let tab_bar_area = client_view.computed.tab_bar_rect;
    let terminal_area = client_view.computed.terminal_area;

    if client_view.computed.layout == ViewLayout::Mobile {
        if !client_view.zen_mode {
            render_mobile_header_for_view(
                app,
                terminal_runtimes,
                client_view,
                frame,
                client_view.computed.mobile_header_rect,
            );
        }
    } else if !client_view.zen_mode {
        if client_view.sidebar_collapsed {
            render_sidebar_collapsed_for_view(
                app,
                terminal_runtimes,
                client_view,
                frame,
                sidebar_area,
            );
        } else {
            render_sidebar_for_view(app, terminal_runtimes, client_view, frame, sidebar_area);
        }
    }
    if !client_view.zen_mode && client_view.computed.layout != ViewLayout::Mobile {
        render_tab_bar_for_view(app, client_view, frame, tab_bar_area);
    }
    render_panes_for_view(app, client_view, terminal_runtimes, frame, terminal_area);
    if client_view.tab_control.is_watching() {
        panes::wash_rect(frame, tab_bar_area, &app.palette);
        panes::wash_rect(frame, terminal_area, &app.palette);
    }
    if right_sidebar_area != Rect::default() {
        render_right_sidebar_for_view(
            app,
            terminal_runtimes,
            client_view,
            frame,
            right_sidebar_area,
        );
    }
    if !client_view.zen_mode
        && (client_view.sidebar_collapsed || client_view.right_sidebar_collapsed)
        && client_view.computed.layout != ViewLayout::Mobile
    {
        render_collapsed_sidebar_hover_for_view(app, client_view, frame);
    }
    render_context_bar(app, &client_view.computed.context_bar, frame);
    if matches!(client_view.mode, Mode::Github | Mode::CommandPalette) {
        if let Some(screen) = &client_view.github {
            github::render(screen, &app.palette, frame);
        }
    }

    match client_view.mode {
        Mode::Onboarding => render_onboarding_overlay(app, frame, frame.area()),
        Mode::ReleaseNotes => {
            render_release_notes_overlay_for_view(app, client_view, frame, frame.area())
        }
        Mode::ProductAnnouncement => {
            render_product_announcement_overlay_for_view(app, client_view, frame, frame.area())
        }
        Mode::Navigate if client_view.computed.layout == ViewLayout::Mobile => {
            render_mobile_panel_for_view(app, terminal_runtimes, client_view, frame, frame.area())
        }
        Mode::Navigate => render_navigate_overlay_for_view(app, client_view, frame, terminal_area),
        Mode::Prefix => render_prefix_overlay_for_view(app, client_view, frame, terminal_area),
        Mode::Copy => render_copy_mode_overlay_for_view(app, client_view, frame, terminal_area),
        Mode::Resize => render_resize_overlay_for_view(app, client_view, frame, terminal_area),
        Mode::ConfirmClose => render_confirm_close_overlay_for_view(
            app,
            client_view,
            terminal_runtimes,
            frame,
            terminal_area,
        ),
        Mode::ConfirmDeleteGroup => {
            render_confirm_delete_group_overlay_for_view(app, client_view, frame, terminal_area)
        }
        Mode::ContextMenu => render_context_menu_for_view(app, client_view, frame),
        Mode::Settings => render_settings_overlay_for_view(app, client_view, frame, frame.area()),
        Mode::RenameWorkspace | Mode::RenameGroup | Mode::RenameTab | Mode::RenamePane => {
            render_rename_overlay_for_view(app, client_view, frame, frame.area())
        }
        Mode::GlobalMenu => render_global_launcher_menu_for_view(app, client_view, frame),
        Mode::GroupMenu => render_group_menu_for_view(app, client_view, frame),
        Mode::AgentMenu => render_agent_menu_for_view(app, client_view, frame),
        Mode::KeybindHelp => render_keybind_help_overlay_for_view(app, client_view, frame),
        Mode::Navigator => {
            render_navigator_overlay_for_view(app, client_view, terminal_runtimes, frame)
        }
        Mode::CommandPalette => render_command_palette_overlay_for_view(app, client_view, frame),
        Mode::AgentProfilePicker => {
            render_agent_profile_picker_overlay_for_view(app, client_view, frame)
        }
        Mode::GitRepoPicker => render_git_repo_picker_overlay_for_view(app, client_view, frame),
        Mode::ConfigDiagnostics => {
            render_config_diagnostics_overlay_for_view(app, client_view, frame)
        }
        Mode::Terminal | Mode::Github => {}
    }
    render_notifications_for_view(app, client_view, frame, terminal_area);
    if client_view.popup_pane.is_some() {
        render_popup_pane_for_view(app, client_view, terminal_runtimes, frame, frame.area());
    }
    if client_view.authentication_prompt.is_some() {
        dialogs::render_authentication_overlay_for_view(app, client_view, frame, frame.area());
    }
}

fn render_notifications(app: &AppState, frame: &mut Frame, terminal_area: Rect) {
    let mut copy_feedback_offset = 0;
    let mut toast_rect = None;
    if let Some(toast) = &app.toast {
        if app.view.layout == ViewLayout::Mobile {
            render_mobile_toast_banner(frame, frame.area(), toast, &app.palette);
            toast_rect = Some(mobile_toast_banner_rect(frame.area()));
        } else {
            let position = toast.position.unwrap_or(app.toast_config.gardn.position);
            render_toast_notification(frame, frame.area(), toast, position, &app.palette);
            toast_rect = Some(toast_notification_rect(frame.area(), toast, position));
        }
    }
    if let Some(feedback) = &app.copy_feedback {
        let area = if app.view.layout == ViewLayout::Mobile {
            frame.area()
        } else {
            terminal_area
        };
        if let Some(toast_rect) = toast_rect {
            copy_feedback_offset = copy_feedback_offset_for_toast(
                area,
                feedback,
                copy_feedback_offset,
                app.toast_config.clipboard.position,
                toast_rect,
            );
        }
        render_copy_feedback(
            frame,
            area,
            feedback,
            copy_feedback_offset,
            app.toast_config.clipboard.position,
            &app.palette,
        );
    }
}

fn render_notifications_for_view(
    app: &AppState,
    client_view: &ClientViewState,
    frame: &mut Frame,
    terminal_area: Rect,
) {
    let mut copy_feedback_offset = 0;
    let mut toast_rect = None;
    if let Some(toast) = &app.toast {
        if client_view.computed.layout == ViewLayout::Mobile {
            render_mobile_toast_banner(frame, frame.area(), toast, &app.palette);
            toast_rect = Some(mobile_toast_banner_rect(frame.area()));
        } else {
            let position = toast.position.unwrap_or(app.toast_config.gardn.position);
            render_toast_notification(frame, frame.area(), toast, position, &app.palette);
            toast_rect = Some(toast_notification_rect(frame.area(), toast, position));
        }
    }
    if let Some(feedback) = &app.copy_feedback {
        let area = if client_view.computed.layout == ViewLayout::Mobile {
            frame.area()
        } else {
            terminal_area
        };
        if let Some(toast_rect) = toast_rect {
            copy_feedback_offset = copy_feedback_offset_for_toast(
                area,
                feedback,
                copy_feedback_offset,
                app.toast_config.clipboard.position,
                toast_rect,
            );
        }
        render_copy_feedback(
            frame,
            area,
            feedback,
            copy_feedback_offset,
            app.toast_config.clipboard.position,
            &app.palette,
        );
    }
}

fn copy_feedback_offset_for_toast(
    area: Rect,
    feedback: &crate::app::state::CopyFeedback,
    base_offset: u16,
    position: crate::config::ToastClipboardPosition,
    toast_rect: Rect,
) -> u16 {
    let feedback_rect = copy_feedback_rect(area, feedback, base_offset, position);
    if rects_overlap(feedback_rect, toast_rect) {
        base_offset.saturating_add(toast_rect.height)
    } else {
        base_offset
    }
}

fn rects_overlap(a: Rect, b: Rect) -> bool {
    a.x < b.x.saturating_add(b.width)
        && b.x < a.x.saturating_add(a.width)
        && a.y < b.y.saturating_add(b.height)
        && b.y < a.y.saturating_add(a.height)
}

fn dim_background(frame: &mut Frame, area: Rect) {
    let buf = frame.buffer_mut();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, y)];
            cell.set_style(cell.style().add_modifier(Modifier::DIM));
        }
    }
}

/// Floating overlay for navigate mode — appears at bottom of terminal area.
fn _build_hints(items: &[(&str, &str)], key_style: Style, dim_style: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    spans.push(Span::raw(" "));
    for (i, (k, desc)) in items.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled("  ", dim_style));
        }
        spans.push(Span::styled(k.to_string(), key_style));
        spans.push(Span::styled(format!(" {desc}"), dim_style));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::keybind_help::{keybind_help_groups, keybind_help_lines};
    use super::scrollbar::scrollbar_thumb;
    use super::*;
    use crate::{
        app::state::ViewLayout,
        layout::PaneInfo,
        workspace::{GitWorkSummary, Workspace},
    };
    use ratatui::{backend::TestBackend, style::Color, Terminal};

    fn canonical_watcher_fixture() -> (
        crate::app::state::AppState,
        ClientViewState,
        ClientViewState,
        TerminalRuntimeRegistry,
    ) {
        let mut app = crate::app::state::AppState::test_new();
        let mut workspace = Workspace::test_new("canonical");
        let root = workspace.tabs[0].root_pane;
        let split = workspace.test_split(ratatui::layout::Direction::Horizontal);
        workspace.tabs[0].layout.focus_pane(root);
        workspace.tabs[0].runtimes.insert(
            root,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(80, 30, b"root"),
        );
        workspace.tabs[0].runtimes.insert(
            split,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(80, 30, b"split"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;
        app.sidebar_collapsed = true;
        app.right_sidebar_collapsed = true;
        app.context_bar_visibility = crate::config::ContextBarVisibilityConfig::Never;

        let mut small = ClientViewState::from_default_client_state(&app);
        small.set_tab_control(ClientTabControl::WatchingControlled { epoch: 1 });
        small.tab_canvas_size = Some((80, 30));
        let large = small.clone();
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        (app, small, large, terminal_runtimes)
    }

    #[test]
    fn settings_modal_keeps_toast_in_foreground() {
        let mut app = crate::app::state::AppState::test_new();
        app.toast = Some(crate::app::state::ToastNotification {
            kind: crate::app::state::ToastKind::NeedsAttention,
            title: "SSH connection test".to_string(),
            context: "execution worker setup is required".to_string(),
            position: Some(crate::config::ToastGardnPosition::BottomLeft),
            target: None,
        });
        let area = Rect::new(0, 0, 100, 30);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let mut client_view = ClientViewState::from_default_client_state(&app);
        client_view.mode = Mode::Settings;
        compute_view_for_client_without_resizing_panes(
            &app,
            &mut client_view,
            &terminal_runtimes,
            area,
        );
        let mut terminal =
            Terminal::new(TestBackend::new(area.width, area.height)).expect("test terminal");

        terminal
            .draw(|frame| {
                render_with_runtime_registry_for_view(&app, &client_view, &terminal_runtimes, frame)
            })
            .expect("render attached-client settings with toast");

        let toast = app.toast.as_ref().expect("toast");
        let toast_area =
            toast_notification_rect(area, toast, crate::config::ToastGardnPosition::BottomLeft);
        assert!(
            !terminal.backend().buffer()[(toast_area.x, toast_area.y)]
                .style()
                .add_modifier
                .contains(Modifier::DIM),
            "toast must remain legible above the settings modal"
        );
    }

    fn pane_geometry(view: &ClientViewState) -> Vec<(crate::layout::PaneId, Rect, Rect, bool)> {
        view.computed
            .pane_infos
            .iter()
            .map(|info| (info.id, info.rect, info.inner_rect, info.is_focused))
            .collect()
    }

    fn assert_same_split_geometry(first: &ClientViewState, second: &ClientViewState) {
        assert_eq!(
            first.computed.split_borders.len(),
            second.computed.split_borders.len()
        );
        for (first, second) in first
            .computed
            .split_borders
            .iter()
            .zip(&second.computed.split_borders)
        {
            assert_eq!(first.pos, second.pos);
            assert_eq!(first.direction, second.direction);
            assert_eq!(first.area, second.area);
            assert_eq!(first.path, second.path);
        }
    }

    #[test]
    fn workspace_creation_prompt_renders_new_workspace_title_and_suggestion() {
        let mut app = crate::app::state::AppState::test_new();
        app.mode = Mode::RenameWorkspace;
        app.pending_workspace_create_location =
            Some(crate::execution_host::ResourceLocation::local("/tmp/project").unwrap());
        app.name_input = "project".into();

        let area = Rect::new(0, 0, 80, 20);
        compute_view(&mut app, area);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let screen = (0..area.height)
            .map(|row| buffer_row_text(terminal.backend().buffer(), area, row))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(screen.contains("New Workspace"), "{screen}");
        assert!(screen.contains("project"), "{screen}");
        assert!(screen.contains("Runs On test-host"), "{screen}");
        let buffer = terminal.backend().buffer();
        let mut caption_row = None;
        let mut button_row = None;
        for row in 0..area.height {
            let text = buffer_row_text(buffer, area, row);
            if text.contains("Runs On") {
                caption_row = Some(row);
            }
            if text.contains("Save") && text.contains("Clear") {
                button_row = Some(row);
            }
        }
        assert_ne!(
            caption_row.expect("Runs On caption"),
            button_row.expect("Save/Clear"),
            "{screen}"
        );
    }

    #[tokio::test]
    async fn focused_pane_cursor_wins_during_terminal_render() {
        let mut app = crate::app::state::AppState::test_new();
        let mut ws = Workspace::test_new("test");
        let first_pane = ws.tabs[0].root_pane;
        let second_pane = ws.test_split(ratatui::layout::Direction::Horizontal);

        ws.insert_test_runtime(
            first_pane,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(20, 5, b"left"),
        );
        ws.insert_test_runtime(
            second_pane,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(20, 5, b"r\r\nb"),
        );
        ws.tabs[0].layout.focus_pane(first_pane);

        app.workspaces = vec![ws];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;

        compute_view(&mut app, Rect::new(0, 0, 80, 20));
        let focused = app
            .view
            .pane_infos
            .iter()
            .find(|info| info.id == first_pane)
            .expect("focused pane info");

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();

        terminal
            .backend_mut()
            .assert_cursor_position((focused.inner_rect.x + 4, focused.inner_rect.y));
    }

    #[test]
    fn mobile_width_uses_header_and_full_width_terminal() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one")];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;

        compute_view(&mut app, Rect::new(0, 0, 44, 20));

        assert_eq!(app.view.layout, ViewLayout::Mobile);
        assert_eq!(app.view.sidebar_rect, Rect::default());
        assert_eq!(app.view.tab_bar_rect, Rect::default());
        assert_eq!(app.view.mobile_header_rect, Rect::new(0, 0, 44, 2));
        assert_eq!(app.view.terminal_area, Rect::new(0, 2, 44, 18));
    }

    #[test]
    fn mobile_breadcrumbs_keep_short_labels_tappable() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one")];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;

        compute_view(&mut app, Rect::new(0, 0, 44, 20));

        let tab = app
            .view
            .context_bar
            .segments
            .iter()
            .find(|segment| segment.target == crate::app::state::ContextBarTarget::Tab)
            .expect("tab breadcrumb");
        assert!(tab.rect.width >= 8, "tab target: {:?}", tab.rect);
        assert_eq!(
            app.view
                .context_bar
                .target_at(tab.rect.x + tab.rect.width - 1, tab.rect.y),
            Some(crate::app::state::ContextBarTarget::Tab)
        );
    }

    #[tokio::test]
    async fn desktop_compute_resizes_selected_tab_without_touching_background_tab() {
        let mut app = crate::app::state::AppState::test_new();
        let mut workspace = Workspace::test_new("one");
        let background_tab = workspace.test_add_tab(Some("background"));
        let active_pane = workspace.tabs[0].root_pane;
        let background_pane = workspace.tabs[background_tab].root_pane;
        workspace.tabs[0].runtimes.insert(
            active_pane,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(11, 7, b""),
        );
        workspace.tabs[background_tab].runtimes.insert(
            background_pane,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(37, 19, b""),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;

        let background_before =
            app.workspaces[0].tabs[background_tab].runtimes[&background_pane].current_size();
        compute_view(&mut app, Rect::new(0, 0, 100, 24));

        let active_info = app.view.pane_infos.first().expect("active pane info");
        assert_eq!(
            app.workspaces[0].tabs[0].runtimes[&active_pane].current_size(),
            (active_info.inner_rect.height, active_info.inner_rect.width)
        );
        assert_eq!(
            app.workspaces[0].tabs[background_tab].runtimes[&background_pane].current_size(),
            background_before
        );
    }

    #[tokio::test]
    async fn mobile_compute_resizes_selected_tab_without_touching_background_tab() {
        let mut app = crate::app::state::AppState::test_new();
        let mut workspace = Workspace::test_new("one");
        let background_tab = workspace.test_add_tab(Some("background"));
        let active_pane = workspace.tabs[0].root_pane;
        let background_pane = workspace.tabs[background_tab].root_pane;
        workspace.tabs[0].runtimes.insert(
            active_pane,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(11, 7, b""),
        );
        workspace.tabs[background_tab].runtimes.insert(
            background_pane,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(37, 19, b""),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;

        let background_before =
            app.workspaces[0].tabs[background_tab].runtimes[&background_pane].current_size();
        compute_view(&mut app, Rect::new(0, 0, 44, 24));

        let active_info = app.view.pane_infos.first().expect("active pane info");
        assert_eq!(
            app.workspaces[0].tabs[0].runtimes[&active_pane].current_size(),
            (active_info.inner_rect.height, active_info.inner_rect.width)
        );
        assert_eq!(
            app.workspaces[0].tabs[background_tab].runtimes[&background_pane].current_size(),
            background_before
        );
    }

    #[test]
    fn configured_mobile_width_threshold_controls_layout_switch() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one")];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;

        compute_view(&mut app, Rect::new(0, 0, 80, 20));
        assert_eq!(app.view.layout, ViewLayout::Desktop);

        app.mobile_width_threshold = 90;
        compute_view(&mut app, Rect::new(0, 0, 80, 20));
        assert_eq!(app.view.layout, ViewLayout::Mobile);
        assert_eq!(app.view.mobile_header_rect, Rect::new(0, 0, 80, 2));
        assert_eq!(app.view.terminal_area, Rect::new(0, 2, 80, 18));
    }

    #[tokio::test]
    async fn zen_mode_gives_the_terminal_the_full_desktop_and_mobile_viewport() {
        let mut app = crate::app::state::AppState::test_new();
        app.context_bar_visibility = crate::config::ContextBarVisibilityConfig::Always;
        let mut workspace = Workspace::test_new("one");
        workspace.custom_name = Some("CHROME-WORKSPACE".into());
        workspace.tabs[0].custom_name = Some("CHROME-TAB".into());
        let root = workspace.tabs[0].root_pane;
        workspace.insert_test_runtime(
            root,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(100, 20, b"ZEN-CONTENT"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;
        app.zen_mode = true;

        let area = Rect::new(0, 0, 100, 20);
        compute_view(&mut app, area);

        assert_eq!(app.view.layout, ViewLayout::Desktop);
        assert_eq!(app.view.sidebar_rect, Rect::default());
        assert_eq!(app.view.right_sidebar_rect, Rect::default());
        assert_eq!(app.view.tab_bar_rect, Rect::default());
        assert_eq!(app.view.context_bar.rect, Rect::default());
        assert_eq!(app.view.terminal_area, area);

        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let rendered = (area.y..area.y + area.height)
            .map(|row| buffer_row_text(terminal.backend().buffer(), area, row))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(rendered.contains("ZEN-CONTENT"));
        assert!(!rendered.contains("CHROME-WORKSPACE"));
        assert!(!rendered.contains("CHROME-TAB"));

        app.mobile_width_threshold = 120;
        compute_view(&mut app, area);

        assert_eq!(app.view.layout, ViewLayout::Mobile);
        assert_eq!(app.view.mobile_header_rect, Rect::default());
        assert_eq!(app.view.terminal_area, area);
    }

    #[test]
    fn desktop_context_bar_renders_topology_and_active_path() {
        let mut app = crate::app::state::AppState::test_new();
        app.context_bar_visibility = crate::config::ContextBarVisibilityConfig::Always;
        app.groups[0].name = "studio".into();
        let mut workspace = Workspace::test_new("ignored");
        workspace.custom_name = Some("website".into());
        workspace.tabs[0].custom_name = Some("release".into());
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;

        compute_view(&mut app, Rect::new(0, 0, 100, 20));
        assert!(app.view.context_bar.counts.is_empty());
        assert_eq!(app.view.context_bar.counts_rect, Rect::default());

        app.show_counters = true;
        compute_view(&mut app, Rect::new(0, 0, 100, 20));

        assert_eq!(app.view.context_bar.rect, Rect::new(0, 19, 100, 1));
        assert_eq!(
            app.view.terminal_area.y + app.view.terminal_area.height,
            app.view.context_bar.rect.y
        );

        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal.draw(|frame| render(&app, frame)).expect("render");
        let line = (0..100)
            .map(|x| terminal.backend().buffer()[(x, 19)].symbol())
            .collect::<String>();

        assert!(line.contains("1 Group · 1 Space · 1 Tab"), "{line:?}");
        assert!(line.contains("studio / website / release"), "{line:?}");
        let path_start = line
            .find("studio / website / release")
            .expect("active path");
        let counts_start = line.find("1 Group · 1 Space · 1 Tab").expect("counts");
        assert!(path_start < counts_start, "{line:?}");
    }

    #[test]
    fn context_bar_identifies_the_focused_pane_in_a_split_tab() {
        let mut app = crate::app::state::AppState::test_new();
        app.groups[0].name = "studio".into();
        let mut workspace = Workspace::test_new("website");
        workspace.tabs[0].custom_name = Some("release".into());
        let focused_pane = workspace.test_split(ratatui::layout::Direction::Horizontal);
        app.workspaces = vec![workspace];
        app.ensure_test_terminals();
        let terminal_id = app.workspaces[0].tabs[0].panes[&focused_pane]
            .attached_terminal_id
            .clone();
        app.terminals
            .get_mut(&terminal_id)
            .expect("focused terminal")
            .manual_label = Some("deploy".into());
        app.active = Some(0);
        app.selected = 0;

        compute_view(&mut app, Rect::new(0, 0, 100, 20));

        let path = app
            .view
            .context_bar
            .segments
            .iter()
            .map(|segment| segment.label.as_str())
            .collect::<Vec<_>>()
            .join(" / ");
        assert!(path.ends_with("release / deploy"), "{path:?}");
        assert_eq!(
            app.view
                .context_bar
                .segments
                .last()
                .map(|segment| segment.target),
            Some(crate::app::state::ContextBarTarget::Pane)
        );
    }

    #[test]
    fn context_bar_renumbers_unnamed_panes_after_one_closes() {
        let mut app = crate::app::state::AppState::test_new();
        let mut workspace = Workspace::test_new("website");
        let closed_pane = workspace.test_split(ratatui::layout::Direction::Horizontal);
        let focused_pane = workspace.test_split(ratatui::layout::Direction::Horizontal);
        assert!(!workspace.close_pane(closed_pane));
        workspace.tabs[0].layout.focus_pane(focused_pane);
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.selected = 0;

        compute_view(&mut app, Rect::new(0, 0, 100, 20));

        assert_eq!(
            app.view
                .context_bar
                .segments
                .last()
                .map(|segment| segment.label.as_str()),
            Some("Pane 2")
        );
    }

    #[test]
    fn context_bar_uses_each_clients_active_workspace() {
        let mut app = crate::app::state::AppState::test_new();
        app.context_bar_visibility = crate::config::ContextBarVisibilityConfig::Always;
        let mut first = Workspace::test_new("ignored");
        first.custom_name = Some("frontend".into());
        first.tabs[0].custom_name = Some("dev".into());
        let mut second = Workspace::test_new("ignored");
        second.custom_name = Some("backend".into());
        second.tabs[0].custom_name = Some("logs".into());
        app.workspaces = vec![first, second];
        app.active = Some(0);
        app.selected = 0;

        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let mut first_client = ClientViewState::from_default_client_state(&app);
        let mut second_client = ClientViewState::from_default_client_state(&app);
        second_client.active_workspace = Some(1);
        second_client.selected_workspace = 1;

        compute_view_for_client_without_resizing_panes(
            &app,
            &mut first_client,
            &terminal_runtimes,
            Rect::new(0, 0, 100, 20),
        );
        compute_view_for_client_without_resizing_panes(
            &app,
            &mut second_client,
            &terminal_runtimes,
            Rect::new(0, 0, 100, 20),
        );

        let first_path = first_client
            .computed
            .context_bar
            .segments
            .iter()
            .map(|segment| segment.label.as_str())
            .collect::<Vec<_>>()
            .join(" / ");
        let second_path = second_client
            .computed
            .context_bar
            .segments
            .iter()
            .map(|segment| segment.label.as_str())
            .collect::<Vec<_>>()
            .join(" / ");

        assert!(first_path.contains("frontend"), "{first_path:?}");
        assert!(!first_path.contains("backend"), "{first_path:?}");
        assert!(second_path.contains("backend"), "{second_path:?}");
        assert!(!second_path.contains("frontend"), "{second_path:?}");
        assert_eq!(app.active, Some(0));
    }

    #[test]
    fn narrow_desktop_context_bar_keeps_path_inside_its_row() {
        let mut app = crate::app::state::AppState::test_new();
        app.context_bar_visibility = crate::config::ContextBarVisibilityConfig::Always;
        app.mobile_width_threshold = 0;
        app.groups[0].name = "engineering-platform".into();
        let mut workspace = Workspace::test_new("ignored");
        workspace.custom_name = Some("customer-operations".into());
        workspace.tabs[0].custom_name = Some("production-observability".into());
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.selected = 0;

        compute_view(&mut app, Rect::new(0, 0, 38, 12));

        let context = &app.view.context_bar;
        assert_eq!(context.rect, Rect::new(0, 11, 38, 1));
        assert_eq!(
            context.segments.last().map(|segment| segment.target),
            Some(crate::app::state::ContextBarTarget::Tab)
        );
        assert!(context.segments.iter().all(|segment| {
            segment.rect.width > 0
                && segment.rect.x + segment.rect.width <= context.rect.x + context.rect.width
        }));
    }

    #[test]
    fn watching_client_context_bar_shows_control_chip() {
        let mut app = crate::app::state::AppState::test_new();
        app.context_bar_visibility = crate::config::ContextBarVisibilityConfig::Always;
        app.mobile_width_threshold = 0;
        let mut workspace = Workspace::test_new("ignored");
        workspace.custom_name = Some("website".into());
        workspace.tabs[0].custom_name = Some("release".into());
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.selected = 0;

        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let mut watcher = ClientViewState::from_default_client_state(&app);
        watcher.set_tab_control(ClientTabControl::WatchingControlled { epoch: 7 });
        let mut controller = ClientViewState::from_default_client_state(&app);
        compute_view_for_client_without_resizing_panes(
            &app,
            &mut watcher,
            &terminal_runtimes,
            Rect::new(0, 0, 100, 20),
        );
        compute_view_for_client_without_resizing_panes(
            &app,
            &mut controller,
            &terminal_runtimes,
            Rect::new(0, 0, 100, 20),
        );

        let bar = &watcher.computed.context_bar;
        let chip = bar.segments.last().expect("watching chip segment");
        assert_eq!(chip.target, crate::app::state::ContextBarTarget::TabControl);
        assert!(chip.label.contains("Watching"), "{chip:?}");
        assert!(chip.label.contains("Another Client Controls"), "{chip:?}");
        assert!(chip.rect.width > 0, "{chip:?}");
        assert!(
            chip.rect.x + chip.rect.width <= bar.rect.x + bar.rect.width,
            "{chip:?}"
        );
        assert_eq!(bar.target_at(chip.rect.x, chip.rect.y), None);
        let take_over = chip
            .label
            .find("Take Over")
            .expect("desktop chip keeps the take-over action");
        let take_over_x = chip.rect.x + take_over as u16;
        assert_eq!(
            bar.target_at(take_over_x, chip.rect.y),
            Some(crate::app::state::ContextBarTarget::TabControl)
        );

        // The controller's bar carries no chip and none of the watcher copy.
        let controller_bar = &controller.computed.context_bar;
        assert!(controller_bar
            .segments
            .iter()
            .all(|segment| segment.target != crate::app::state::ContextBarTarget::TabControl));
        let controller_labels = controller_bar
            .segments
            .iter()
            .map(|segment| segment.label.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            !controller_labels.contains("Watching"),
            "{controller_labels:?}"
        );
        assert!(!controller_labels.contains("Free"), "{controller_labels:?}");

        // The rendered frame shows the badge in the mode-badge idiom.
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render_with_runtime_registry_for_view(&app, &watcher, &terminal_runtimes, frame)
            })
            .expect("render");
        let line = buffer_row_text(terminal.backend().buffer(), Rect::new(0, 0, 100, 20), 19);
        assert!(line.contains("Watching"), "{line:?}");
        assert!(line.contains("Another Client Controls"), "{line:?}");
        let badge_x = line.find("Watching").expect("badge text") as u16;
        assert_ne!(
            terminal.backend().buffer()[(badge_x, 19)].style().bg,
            Some(app.palette.overlay0),
            "Watching is status, not the action chip"
        );
        let take_x = line.find("Take Over").expect("take over text") as u16;
        assert_eq!(
            terminal.backend().buffer()[(take_x, 19)].style().bg,
            Some(app.palette.overlay0),
            "Take Over should carry the action badge"
        );
    }

    #[test]
    fn free_client_context_bar_shows_take_control_chip() {
        let mut app = crate::app::state::AppState::test_new();
        app.context_bar_visibility = crate::config::ContextBarVisibilityConfig::Always;
        app.mobile_width_threshold = 0;
        let mut workspace = Workspace::test_new("ignored");
        workspace.custom_name = Some("website".into());
        workspace.tabs[0].custom_name = Some("release".into());
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.selected = 0;

        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let mut watcher = ClientViewState::from_default_client_state(&app);
        watcher.set_tab_control(ClientTabControl::WatchingFree { epoch: 2 });
        compute_view_for_client_without_resizing_panes(
            &app,
            &mut watcher,
            &terminal_runtimes,
            Rect::new(0, 0, 100, 20),
        );

        let bar = &watcher.computed.context_bar;
        let chip = bar.segments.last().expect("free chip segment");
        assert_eq!(chip.target, crate::app::state::ContextBarTarget::TabControl);
        assert!(chip.label.contains("Free"), "{chip:?}");
        assert!(chip.label.contains("Take Control"), "{chip:?}");

        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render_with_runtime_registry_for_view(&app, &watcher, &terminal_runtimes, frame)
            })
            .expect("render");
        let line = buffer_row_text(terminal.backend().buffer(), Rect::new(0, 0, 100, 20), 19);
        assert!(line.contains("Free"), "{line:?}");
        assert!(line.contains("Take Control"), "{line:?}");
        let badge_x = line.find("Free").expect("badge text") as u16;
        assert_ne!(
            terminal.backend().buffer()[(badge_x, 19)].style().bg,
            Some(app.palette.teal),
            "Free is status, not the action chip"
        );
        let take_x = line.find("Take Control").expect("take control text") as u16;
        assert_eq!(
            terminal.backend().buffer()[(take_x, 19)].style().bg,
            Some(app.palette.teal),
            "Take Control should use the positive teal background"
        );
    }

    #[tokio::test]
    async fn watching_desktop_uses_canonical_tab_canvas_across_viewports() {
        let (mut app, mut small, mut large, terminal_runtimes) = canonical_watcher_fixture();
        app.mobile_width_threshold = 0;
        let small_area = Rect::new(0, 0, 60, 20);
        let large_area = Rect::new(0, 0, 120, 40);

        compute_view_for_client_without_resizing_panes(
            &app,
            &mut small,
            &terminal_runtimes,
            small_area,
        );
        compute_view_for_client_without_resizing_panes(
            &app,
            &mut large,
            &terminal_runtimes,
            large_area,
        );

        assert_ne!(small.computed.terminal_area, large.computed.terminal_area);
        assert_eq!(pane_geometry(&small), pane_geometry(&large));
        assert_same_split_geometry(&small, &large);
        assert_eq!(
            small.tab_canvas_view.map(|view| view.viewport),
            Some(small.computed.terminal_area)
        );
        assert_eq!(
            large.tab_canvas_view.map(|view| view.viewport),
            Some(large.computed.terminal_area)
        );
        assert!(small
            .computed
            .pane_infos
            .iter()
            .all(|info| info.rect.x < 80));
        assert!(small
            .computed
            .pane_infos
            .iter()
            .all(|info| info.rect.y < 30));

        let mut small_terminal =
            Terminal::new(TestBackend::new(small_area.width, small_area.height)).unwrap();
        small_terminal
            .draw(|frame| {
                render_with_runtime_registry_for_view(&app, &small, &terminal_runtimes, frame)
            })
            .expect("canonical desktop canvas is clipped to the observer frame");
        let mut large_terminal =
            Terminal::new(TestBackend::new(large_area.width, large_area.height)).unwrap();
        large_terminal
            .draw(|frame| {
                render_with_runtime_registry_for_view(&app, &large, &terminal_runtimes, frame)
            })
            .expect("canonical desktop canvas renders in a larger observer frame");
    }

    #[tokio::test]
    async fn watching_mobile_uses_canonical_tab_canvas_across_viewports() {
        let (mut app, mut small, mut large, terminal_runtimes) = canonical_watcher_fixture();
        app.mobile_width_threshold = 100;
        let small_area = Rect::new(0, 0, 40, 12);
        let large_area = Rect::new(0, 0, 80, 35);

        compute_view_for_client_without_resizing_panes(
            &app,
            &mut small,
            &terminal_runtimes,
            small_area,
        );
        compute_view_for_client_without_resizing_panes(
            &app,
            &mut large,
            &terminal_runtimes,
            large_area,
        );

        assert_ne!(small.computed.terminal_area, large.computed.terminal_area);
        assert_eq!(pane_geometry(&small), pane_geometry(&large));
        assert_same_split_geometry(&small, &large);
        assert_eq!(
            small.tab_canvas_view.map(|view| view.viewport),
            Some(small.computed.terminal_area)
        );
        assert_eq!(
            large.tab_canvas_view.map(|view| view.viewport),
            Some(large.computed.terminal_area)
        );
        assert!(small
            .computed
            .pane_infos
            .iter()
            .all(|info| info.rect.x < 80));
        assert!(small
            .computed
            .pane_infos
            .iter()
            .all(|info| info.rect.y < 30));

        let mut small_terminal =
            Terminal::new(TestBackend::new(small_area.width, small_area.height)).unwrap();
        small_terminal
            .draw(|frame| {
                render_with_runtime_registry_for_view(&app, &small, &terminal_runtimes, frame)
            })
            .expect("canonical mobile canvas is clipped to the observer frame");
        let mut large_terminal =
            Terminal::new(TestBackend::new(large_area.width, large_area.height)).unwrap();
        large_terminal
            .draw(|frame| {
                render_with_runtime_registry_for_view(&app, &large, &terminal_runtimes, frame)
            })
            .expect("canonical mobile canvas renders in a larger observer frame");
    }

    #[test]
    fn narrow_desktop_context_bar_keeps_control_chip_as_trailing_segment() {
        let mut app = crate::app::state::AppState::test_new();
        app.context_bar_visibility = crate::config::ContextBarVisibilityConfig::Always;
        app.mobile_width_threshold = 0;
        app.groups[0].icon = String::new();
        app.groups[0].name = "eng".into();
        let mut workspace = Workspace::test_new("ignored");
        workspace.custom_name = Some("web".into());
        workspace.tabs[0].custom_name = Some("prod".into());
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.selected = 0;

        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let mut watcher = ClientViewState::from_default_client_state(&app);
        watcher.set_tab_control(ClientTabControl::WatchingControlled { epoch: 1 });

        compute_view_for_client_without_resizing_panes(
            &app,
            &mut watcher,
            &terminal_runtimes,
            Rect::new(0, 0, 40, 12),
        );
        let bar = &watcher.computed.context_bar;
        let chip = bar.segments.last().expect("chip at 40 cols");
        assert_eq!(chip.target, crate::app::state::ContextBarTarget::TabControl);
        assert!(chip.label.contains("Watching"), "{chip:?}");
        assert!(chip.label.contains("Another"), "{chip:?}");

        // The hint suffix truncates first; the bare badge remains.
        compute_view_for_client_without_resizing_panes(
            &app,
            &mut watcher,
            &terminal_runtimes,
            Rect::new(0, 0, 32, 12),
        );
        let bar = &watcher.computed.context_bar;
        let chip = bar.segments.last().expect("chip at 32 cols");
        assert_eq!(chip.target, crate::app::state::ContextBarTarget::TabControl);
        assert_eq!(chip.label, " Watching ", "{chip:?}");

        // Even at extreme widths the chip is never dropped; only truncated.
        compute_view_for_client_without_resizing_panes(
            &app,
            &mut watcher,
            &terminal_runtimes,
            Rect::new(0, 0, 24, 12),
        );
        let bar = &watcher.computed.context_bar;
        let chip = bar.segments.last().expect("chip at 24 cols");
        assert_eq!(chip.target, crate::app::state::ContextBarTarget::TabControl);
        assert!(bar.segments.iter().all(|segment| {
            segment.rect.width > 0
                && segment.rect.x + segment.rect.width <= bar.rect.x + bar.rect.width
        }));
    }

    #[test]
    fn context_bar_visibility_is_independent_of_sidebar_state() {
        let mut app = crate::app::state::AppState::test_new();
        app.mobile_width_threshold = 0;
        app.workspaces = vec![Workspace::test_new("one")];
        app.active = Some(0);
        app.selected = 0;
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let area = Rect::new(0, 0, 80, 20);

        let mut expanded_client = ClientViewState::from_default_client_state(&app);
        let mut collapsed_client = ClientViewState::from_default_client_state(&app);
        collapsed_client.sidebar_collapsed = true;
        compute_view_for_client_without_resizing_panes(
            &app,
            &mut expanded_client,
            &terminal_runtimes,
            area,
        );
        compute_view_for_client_without_resizing_panes(
            &app,
            &mut collapsed_client,
            &terminal_runtimes,
            area,
        );

        assert_eq!(
            expanded_client.computed.context_bar.rect,
            Rect::new(0, 19, 80, 1)
        );
        assert_eq!(
            collapsed_client.computed.context_bar.rect,
            Rect::new(0, 19, 80, 1)
        );

        app.context_bar_visibility = crate::config::ContextBarVisibilityConfig::Never;
        compute_view_for_client_without_resizing_panes(
            &app,
            &mut collapsed_client,
            &terminal_runtimes,
            area,
        );
        assert_eq!(collapsed_client.computed.context_bar.rect, Rect::default());
    }

    #[test]
    fn desktop_layout_reserves_context_row_without_outer_inset() {
        let mut app = crate::app::state::AppState::test_new();
        app.context_bar_visibility = crate::config::ContextBarVisibilityConfig::Always;
        app.workspaces = vec![Workspace::test_new("one")];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;

        let area = Rect::new(0, 0, 80, 20);
        compute_view(&mut app, area);

        assert_eq!(app.view.layout, ViewLayout::Desktop);
        assert_eq!(app.view.sidebar_rect.x, area.x);
        assert_eq!(app.view.sidebar_rect.y, area.y);
        assert_eq!(
            app.view.sidebar_rect.y + app.view.sidebar_rect.height,
            app.view.context_bar.rect.y
        );
        assert_eq!(
            app.view.terminal_area.x + app.view.terminal_area.width,
            area.width
        );
        assert_eq!(
            app.view.terminal_area.y + app.view.terminal_area.height,
            app.view.context_bar.rect.y
        );
        assert_eq!(
            app.view.context_bar.rect.y + app.view.context_bar.rect.height,
            area.height
        );
    }

    #[tokio::test]
    async fn desktop_theme_background_paints_chrome_and_pane_defaults() {
        let mut app = crate::app::state::AppState::test_new();
        app.palette.panel_bg = Color::Rgb(1, 2, 3);
        let mut ws = Workspace::test_new("test");
        let root = ws.tabs[0].root_pane;
        ws.tabs[0].runtimes.insert(
            root,
            crate::terminal::TerminalRuntime::test_with_screen_bytes(20, 5, b""),
        );
        app.workspaces = vec![ws];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;

        compute_view(&mut app, Rect::new(0, 0, 80, 20));
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let buffer = terminal.backend().buffer();

        assert_eq!(buffer[(0, 0)].style().bg, Some(app.palette.panel_bg));
        assert_eq!(
            buffer[(app.view.sidebar_rect.x, app.view.sidebar_rect.y)]
                .style()
                .bg,
            Some(app.palette.panel_bg)
        );
        assert_eq!(
            buffer[(app.view.terminal_area.x, app.view.terminal_area.y)]
                .style()
                .bg,
            Some(app.palette.panel_bg)
        );
    }

    #[test]
    fn config_diagnostic_does_not_render_over_terminal() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one")];
        app.active = Some(0);
        app.selected = 0;
        app.config_diagnostic = Some(
            "unsafe direct keybinding: keys.new_workspace = \"n\"\nunsafe direct keybinding: keys.new_tab = \"c\""
                .into(),
        );

        let area = Rect::new(0, 0, 80, 20);
        compute_view(&mut app, area);
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();

        let buffer = terminal.backend().buffer();
        let text = (0..area.height)
            .map(|row| buffer_row_text(buffer, Rect::new(0, row, area.width, 1), row))
            .collect::<String>();
        assert!(!text.contains("config warning"));
        assert!(!text.contains("unsafe direct keybinding"));
    }
    #[test]
    fn hide_tab_bar_when_single_tab_collapses_the_tab_row() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one")];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;
        app.hide_tab_bar_when_single_tab = true;

        compute_view(&mut app, Rect::new(0, 0, 106, 20));

        assert_eq!(app.view.tab_bar_rect, Rect::default());
        assert!(app.view.terminal_area.height > 1);
    }

    #[test]
    fn hide_tab_bar_when_single_tab_keeps_the_row_for_multiple_tabs() {
        let mut app = crate::app::state::AppState::test_new();
        let mut workspace = Workspace::test_new("one");
        workspace.test_add_tab(Some("two"));
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;
        app.hide_tab_bar_when_single_tab = true;

        compute_view(&mut app, Rect::new(0, 0, 106, 20));

        assert_eq!(app.view.tab_bar_rect.height, 1);
    }

    #[test]
    fn hide_tab_bar_when_single_tab_off_keeps_the_row() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one")];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;
        app.hide_tab_bar_when_single_tab = false;

        compute_view(&mut app, Rect::new(0, 0, 106, 20));

        assert_eq!(app.view.tab_bar_rect.height, 1);
    }

    #[test]
    fn compute_view_clamps_sidebar_width_to_configured_max() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one")];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;
        app.sidebar_max_width = 30;
        app.sidebar_width = 999;

        compute_view(&mut app, Rect::new(0, 0, 100, 20));

        assert_eq!(app.view.sidebar_rect.width, 30);
    }

    #[test]
    fn compute_view_clamps_sidebar_width_to_configured_min() {
        let mut app = crate::app::state::AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one")];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;
        app.sidebar_min_width = 22;
        app.sidebar_width = 5;

        compute_view(&mut app, Rect::new(0, 0, 100, 20));

        assert_eq!(app.view.sidebar_rect.width, 22);
    }

    #[test]
    fn combined_right_sidebar_keeps_workspace_list_above_agent_panel() {
        let mut app = crate::app::state::AppState::test_new();
        app.sidebar_arrangement = crate::config::SidebarArrangementConfig::CombinedRight;
        app.workspaces = vec![Workspace::test_new("one")];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;

        compute_view(&mut app, Rect::new(0, 0, 100, 20));

        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let sidebar = app.view.sidebar_rect;
        let content = Rect::new(
            sidebar.x + 1,
            sidebar.y,
            sidebar.width.saturating_sub(1),
            sidebar.height,
        );
        let row_containing = |needle: &str| {
            (content.y..content.y + content.height)
                .find(|row| buffer_row_text(buffer, content, *row).contains(needle))
                .expect(needle)
        };
        let workspace_row = row_containing("one");
        let agents_row = row_containing("Agents");

        assert_eq!(app.view.right_sidebar_rect, Rect::default());
        assert!(sidebar.x > 0);
        assert_eq!(buffer[(sidebar.x, sidebar.y)].symbol(), "│");
        assert!(workspace_row < agents_row);
    }

    #[test]
    fn collapsed_sidebar_keeps_active_workspace_highlight_in_terminal_mode() {
        let mut app = crate::app::state::AppState::test_new();
        app.sidebar_collapsed = true;
        app.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        app.active = Some(1);
        app.selected = 0;
        app.mode = Mode::Terminal;

        compute_view(&mut app, Rect::new(0, 0, 80, 20));

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let buffer = terminal.backend().buffer();

        let rows = collapsed_workspace_rows_rect(app.view.sidebar_rect, true);
        let active_row = rows.y + 1;
        let active_style = buffer[(rows.x, active_row)].style();

        assert_eq!(active_style.bg, Some(app.palette.surface_dim));
    }

    #[test]
    fn collapsed_sidebar_empty_state_keeps_agents_label() {
        let mut app = crate::app::state::AppState::test_new();
        app.sidebar_collapsed = true;
        app.workspaces.clear();
        app.active = None;
        app.mode = Mode::Terminal;

        compute_view(&mut app, Rect::new(0, 0, 80, 20));

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let text = (app.view.sidebar_rect.y
            ..app.view.sidebar_rect.y + app.view.sidebar_rect.height)
            .map(|row| buffer_row_text(buffer, app.view.sidebar_rect, row))
            .collect::<Vec<_>>()
            .join("\n");

        assert!(text.contains("f:s"));
        assert!(!text.contains("agt"));
    }

    #[test]
    fn expanded_sidebar_workspace_rows_hide_clean_work_summary() {
        let mut app = crate::app::state::AppState::test_new();
        let mut ws = Workspace::test_new("one");
        ws.cached_git_work_summary = Some(GitWorkSummary {
            repo_count: 1,
            ..GitWorkSummary::default()
        });

        app.workspaces = vec![ws];
        app.ensure_test_terminals();
        app.selected = 0;
        app.mode = Mode::Navigate;

        compute_view(&mut app, Rect::new(0, 0, 80, 20));

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let buffer = terminal.backend().buffer();

        let card = app.view.workspace_card_areas[0].rect;
        let line1 = buffer_row_text(buffer, card, card.y);
        let line2 = buffer_row_text(buffer, card, card.y + 1);

        assert!(line1.starts_with("  · one"));
        assert!(!line1.contains("1 one"));
        assert_eq!(line2, "    test-host");
    }

    #[test]
    fn expanded_sidebar_work_summary_colors_stats_by_kind() {
        let mut app = crate::app::state::AppState::test_new();
        let mut ws = Workspace::test_new("one");
        ws.cached_git_work_summary = Some(GitWorkSummary {
            repo_count: 1,
            added: 2,
            modified: 1,
            deleted: 1,
            ..GitWorkSummary::default()
        });

        app.workspaces = vec![ws];
        compute_view(&mut app, Rect::new(0, 0, 80, 20));

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let card = app.view.workspace_card_areas[0].rect;
        let row = card.y + 1;

        let line = buffer_row_text(buffer, card, row);
        assert_eq!(line, "    test-host · +2 ~1 -1");
        let plus = line.find('+').expect("added count");
        let tilde = plus + line[plus..].find('~').expect("modified count");
        let minus = plus + line[plus..].find('-').expect("deleted count");
        let plus = u16::try_from(plus).expect("column");
        let tilde = u16::try_from(tilde).expect("column");
        let minus = u16::try_from(minus).expect("column");
        assert_eq!(
            buffer[(card.x + plus, row)].style().fg,
            Some(app.palette.green)
        );
        assert_eq!(
            buffer[(card.x + tilde, row)].style().fg,
            Some(app.palette.yellow)
        );
        assert_eq!(
            buffer[(card.x + minus, row)].style().fg,
            Some(app.palette.red)
        );
    }

    #[test]
    fn tab_bar_dims_auto_named_tabs_and_emphasizes_custom_tabs() {
        let mut app = crate::app::state::AppState::test_new();
        let mut ws = Workspace::test_new("test");
        let custom_tab = ws.test_add_tab(Some("logs"));
        ws.switch_tab(custom_tab);

        app.workspaces = vec![ws];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;

        compute_view(&mut app, Rect::new(0, 0, 80, 20));

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let buffer = terminal.backend().buffer();

        let auto_rect = app.view.tab_hit_areas[0];
        let custom_rect = app.view.tab_hit_areas[1];
        let auto_style = buffer[(auto_rect.x + 1, auto_rect.y)].style();
        let custom_style = buffer[(custom_rect.x + 1, custom_rect.y)].style();

        assert_eq!(auto_style.fg, Some(app.palette.overlay0));
        assert!(auto_style.add_modifier.contains(Modifier::DIM));
        assert_eq!(custom_style.fg, Some(app.palette.panel_bg));
        assert!(custom_style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn tab_bar_uses_surface_dim_when_panel_background_resets() {
        let mut app = crate::app::state::AppState::test_new();
        let mut ws = Workspace::test_new("test");
        let custom_tab = ws.test_add_tab(Some("logs"));
        ws.switch_tab(custom_tab);

        app.palette.panel_bg = Color::Reset;
        app.workspaces = vec![ws];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;

        compute_view(&mut app, Rect::new(0, 0, 80, 20));

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let buffer = terminal.backend().buffer();

        let custom_rect = app.view.tab_hit_areas[1];
        let custom_style = buffer[(custom_rect.x + 1, custom_rect.y)].style();

        assert_eq!(custom_style.bg, Some(app.palette.accent));
        assert_eq!(custom_style.fg, Some(app.palette.surface_dim));
        assert!(custom_style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn tab_bar_shows_close_icon_only_for_hovered_tab() {
        let mut app = crate::app::state::AppState::test_new();
        let mut ws = Workspace::test_new("test");
        ws.test_add_tab(Some("logs"));

        app.workspaces = vec![ws];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;

        compute_view(&mut app, Rect::new(0, 0, 80, 20));
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let tab_row = app.view.tab_bar_rect.y;
        assert!(!buffer_row_text(buffer, app.view.tab_bar_rect, tab_row).contains('×'));
        assert!(app
            .view
            .tab_close_hit_areas
            .iter()
            .all(|rect| rect.width == 0));

        app.hovered_tab = Some(1);
        compute_view(&mut app, Rect::new(0, 0, 80, 20));
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let close_rect = app.view.tab_close_hit_areas[1];

        assert_eq!(close_rect.width, 1);
        assert_eq!(buffer[(close_rect.x, close_rect.y)].symbol(), "✕");
        assert_eq!(
            buffer[(
                app.view.tab_hit_areas[0].x + app.view.tab_hit_areas[0].width - 1,
                tab_row
            )]
                .symbol(),
            " "
        );
    }

    #[test]
    fn hovered_truncated_tab_keeps_close_icon_visible() {
        let mut app = crate::app::state::AppState::test_new();
        let mut ws = Workspace::test_new("test");
        ws.tabs[0].set_custom_name("very-long-tab-name-0".into());
        for idx in 1..14 {
            ws.test_add_tab(Some(&format!("very-long-tab-name-{idx}")));
        }

        app.workspaces = vec![ws];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;
        app.mouse_capture = true;
        app.tab_scroll_follow_active = false;

        let (area, truncated_idx) = (44..=80)
            .find_map(|width| {
                compute_view(&mut app, Rect::new(0, 0, width, 20));
                let candidates = app
                    .view
                    .tab_hit_areas
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, rect)| (rect.width > 3 && rect.width < 8).then_some(idx))
                    .collect::<Vec<_>>();
                for idx in candidates {
                    app.hovered_tab = Some(idx);
                    let area = Rect::new(0, 0, width, 20);
                    compute_view(&mut app, area);
                    if app.view.tab_close_hit_areas[idx].width > 0 {
                        return Some((area, idx));
                    }
                }
                app.hovered_tab = None;
                None
            })
            .expect("naturally truncated visible tab with close affordance");

        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| render(&app, frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let tab_rect = app.view.tab_hit_areas[truncated_idx];
        let rendered_symbols = (tab_rect.x..tab_rect.x + tab_rect.width)
            .map(|x| buffer[(x, tab_rect.y)].symbol().to_string())
            .collect::<Vec<_>>();
        let close_rect = app.view.tab_close_hit_areas[truncated_idx];
        let close_symbol = if close_rect.width > 0 {
            buffer[(close_rect.x, close_rect.y)].symbol().to_string()
        } else {
            String::new()
        };

        assert_eq!(
            close_symbol, "✕",
            "hovered truncated tab should keep the close icon visible: tab={rendered_symbols:?}, tab_rect={tab_rect:?}, close_rect={close_rect:?}"
        );
        let rendered = rendered_symbols.join("");
        assert!(
            !rendered.contains("very-long-tab-name"),
            "hovered truncated tab should not let the full label crowd out the close icon: {rendered_symbols:?}"
        );
    }

    #[test]
    fn new_tab_button_tracks_rightmost_tab_when_tabs_fit() {
        let mut app = crate::app::state::AppState::test_new();
        let mut ws = Workspace::test_new("test");
        ws.test_add_tab(Some("logs"));

        app.workspaces = vec![ws];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;

        compute_view(&mut app, Rect::new(0, 0, 80, 20));

        let last_visible = app
            .view
            .tab_hit_areas
            .iter()
            .rev()
            .find(|rect| rect.width > 0)
            .copied()
            .expect("last visible tab");

        assert_eq!(
            app.view.new_tab_hit_area.x,
            last_visible.x + last_visible.width
        );
    }

    #[test]
    fn tab_bar_shows_scroll_controls_when_tabs_overflow() {
        let mut app = crate::app::state::AppState::test_new();
        let mut ws = Workspace::test_new("test");
        for name in ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta"] {
            ws.test_add_tab(Some(name));
        }

        app.workspaces = vec![ws];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;
        app.tab_scroll_follow_active = false;
        app.tab_scroll = 2;

        compute_view(&mut app, Rect::new(0, 0, 65, 20));

        assert!(app.view.tab_scroll_left_hit_area.width > 0);
        assert!(app.view.tab_scroll_right_hit_area.width > 0);
        assert_eq!(app.view.tab_hit_areas[0].width, 0);
        assert_eq!(app.view.tab_hit_areas[1].width, 0);
        assert!(app.view.tab_hit_areas[2].width > 0);
        assert!(app.view.new_tab_hit_area.width > 0);

        let last_visible = app
            .view
            .tab_hit_areas
            .iter()
            .rev()
            .find(|rect| rect.width > 0)
            .copied()
            .expect("last visible tab");

        assert_eq!(
            app.view.tab_scroll_right_hit_area.x,
            last_visible.x + last_visible.width
        );
        assert_eq!(
            app.view.new_tab_hit_area.x,
            app.view.tab_scroll_right_hit_area.x + app.view.tab_scroll_right_hit_area.width
        );
    }

    #[test]
    fn tab_bar_clamps_manual_scroll_at_last_visible_tab() {
        let mut app = crate::app::state::AppState::test_new();
        let mut ws = Workspace::test_new("test");
        for name in [
            "one", "two", "three", "four", "five", "six", "seven", "eight",
        ] {
            ws.test_add_tab(Some(name));
        }

        app.workspaces = vec![ws];
        app.active = Some(0);
        app.selected = 0;
        app.mode = Mode::Terminal;
        app.tab_scroll_follow_active = false;
        app.tab_scroll = usize::MAX;

        compute_view(&mut app, Rect::new(0, 0, 65, 20));

        let last_idx = app.workspaces[0].tabs.len() - 1;
        assert!(app.view.tab_hit_areas[last_idx].width > 0);
        let clamped_scroll = app.tab_scroll;

        app.scroll_tabs_right();

        assert_eq!(app.tab_scroll, clamped_scroll);
        assert!(app.view.tab_hit_areas[last_idx].width > 0);
    }

    #[test]
    fn pane_scrollbar_rect_uses_reserved_rightmost_column() {
        let info = PaneInfo {
            id: crate::layout::PaneId::from_raw(1),
            rect: Rect::new(0, 0, 12, 8),
            inner_rect: Rect::new(1, 1, 9, 6),
            scrollbar_rect: Some(Rect::new(10, 1, 1, 6)),
            is_focused: true,
        };

        assert_eq!(pane_scrollbar_rect(&info), Some(Rect::new(10, 1, 1, 6)));
    }

    #[tokio::test]
    async fn compute_view_reserves_terminal_column_when_pane_scrollbar_is_visible() {
        let mut app = crate::app::state::AppState::test_new();
        let mut ws = Workspace::test_new("test");
        let pane_id = ws.tabs[0].root_pane;
        ws.insert_test_runtime(
            pane_id,
            crate::terminal::TerminalRuntime::test_with_scrollback_bytes(
                12,
                4,
                4096,
                b"000000000000\r\n111111111111\r\n222222222222\r\n333333333333\r\n444444444444\r\n",
            ),
        );

        app.workspaces = vec![ws];
        app.active = Some(0);
        app.selected = 0;

        compute_view(&mut app, Rect::new(0, 0, 40, 12));

        let info = app.view.pane_infos.first().expect("pane info");
        assert_eq!(info.inner_rect.width + 1, app.view.terminal_area.width);
        assert_eq!(
            info.scrollbar_rect,
            Some(Rect::new(
                info.inner_rect.x + info.inner_rect.width,
                info.inner_rect.y,
                1,
                info.inner_rect.height,
            ))
        );
    }

    #[tokio::test]
    async fn compute_view_reclaims_terminal_column_when_pane_scrollbars_disabled() {
        let mut app = crate::app::state::AppState::test_new();
        let mut ws = Workspace::test_new("test");
        let pane_id = ws.tabs[0].root_pane;
        ws.insert_test_runtime(
            pane_id,
            crate::terminal::TerminalRuntime::test_with_scrollback_bytes(
                12,
                4,
                4096,
                b"000000000000\r\n111111111111\r\n222222222222\r\n333333333333\r\n444444444444\r\n",
            ),
        );

        app.workspaces = vec![ws];
        app.active = Some(0);
        app.selected = 0;
        app.pane_scrollbars = false;

        compute_view(&mut app, Rect::new(0, 0, 40, 12));

        let info = app.view.pane_infos.first().expect("pane info");
        assert_eq!(info.inner_rect.width, app.view.terminal_area.width);
        assert_eq!(info.scrollbar_rect, None);
    }

    #[test]
    fn scrollbar_stays_hidden_without_scrollback() {
        let metrics = crate::pane::ScrollMetrics {
            offset_from_bottom: 0,
            max_offset_from_bottom: 0,
            viewport_rows: 5,
        };

        assert!(!should_show_scrollbar(metrics));
    }

    #[test]
    fn scrollbar_shows_with_scrollback() {
        let metrics = crate::pane::ScrollMetrics {
            offset_from_bottom: 0,
            max_offset_from_bottom: 20,
            viewport_rows: 5,
        };

        assert!(should_show_scrollbar(metrics));
    }

    #[test]
    fn modal_scroll_metrics_converts_top_scroll_to_offset_from_bottom() {
        let metrics = modal_scroll_metrics(20, 5, 3);

        assert_eq!(metrics.viewport_rows, 5);
        assert_eq!(metrics.max_offset_from_bottom, 15);
        assert_eq!(metrics.offset_from_bottom, 12);
        assert_eq!(widgets::modal_scroll_from_offset_from_bottom(20, 5, 12), 3);
    }

    #[test]
    fn modal_list_viewport_clamps_scroll_and_visible_range() {
        let viewport = ModalListViewport::new(20, 5, 99);

        assert_eq!(viewport.scroll(), 15);
        assert_eq!(viewport.max_scroll(), 15);
        assert_eq!(viewport.visible_range(), 15..20);
    }

    #[test]
    fn modal_list_viewport_keeps_selected_row_visible_with_context() {
        let viewport = ModalListViewport::new(20, 5, 6);

        assert_eq!(viewport.ensure_visible(6, Some(5)), 5);
        assert_eq!(viewport.ensure_visible(11, None), 7);
    }

    #[test]
    fn modal_list_viewport_hit_testing_rejects_scrollbar_column() {
        let viewport = ModalListViewport::new(20, 5, 3);
        let area = Rect::new(10, 4, 10, 5);

        assert_eq!(viewport.hit_visual_row(area, 11, 4), Some(3));
        assert_eq!(viewport.hit_visual_row(area, 11, 8), Some(7));
        assert_eq!(viewport.hit_visual_row(area, 19, 4), None);
        assert_eq!(viewport.hit_visual_row(area, 11, 9), None);
    }

    #[test]
    fn scrollbar_thumb_reaches_bottom_when_scrolled_to_bottom() {
        let metrics = crate::pane::ScrollMetrics {
            offset_from_bottom: 0,
            max_offset_from_bottom: 20,
            viewport_rows: 5,
        };
        let track = Rect::new(9, 4, 1, 5);

        let thumb = scrollbar_thumb(metrics, track).expect("thumb");
        assert_eq!(thumb.top + thumb.len, track.y + track.height);
    }

    #[test]
    fn scrollbar_thumb_grab_ignores_track_before_thumb() {
        let metrics = crate::pane::ScrollMetrics {
            offset_from_bottom: 0,
            max_offset_from_bottom: 20,
            viewport_rows: 5,
        };
        let track = Rect::new(9, 4, 1, 5);

        assert_eq!(scrollbar_thumb_grab_offset(metrics, track, track.y), None);
    }

    #[test]
    fn scrollbar_offset_mapping_hits_top_middle_and_bottom() {
        let metrics = crate::pane::ScrollMetrics {
            offset_from_bottom: 0,
            max_offset_from_bottom: 20,
            viewport_rows: 5,
        };
        let track = Rect::new(9, 4, 1, 5);

        assert_eq!(scrollbar_offset_from_row(metrics, track, 4), 20);
        assert_eq!(scrollbar_offset_from_row(metrics, track, 6), 10);
        assert_eq!(scrollbar_offset_from_row(metrics, track, 8), 0);
    }

    #[test]
    fn dragging_from_current_thumb_row_preserves_offset() {
        let metrics = crate::pane::ScrollMetrics {
            offset_from_bottom: 7,
            max_offset_from_bottom: 20,
            viewport_rows: 5,
        };
        let track = Rect::new(9, 4, 1, 8);
        let thumb = scrollbar_thumb(metrics, track).expect("thumb");
        let row = thumb.top + thumb.len / 2;
        let grab = scrollbar_thumb_grab_offset(metrics, track, row).expect("grab");

        assert_eq!(scrollbar_offset_from_drag_row(metrics, track, row, grab), 7);
    }

    fn buffer_row_text(buffer: &ratatui::buffer::Buffer, area: Rect, row: u16) -> String {
        (area.x..area.x + area.width)
            .map(|x| buffer[(x, row)].symbol())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    #[test]

    fn prefix_mode_renders_prefix_indicator() {
        let mut app = crate::app::state::AppState::test_new();
        app.mode = Mode::Prefix;
        app.view.terminal_area = ratatui::layout::Rect::new(0, 0, 80, 4);
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 4))
            .expect("test terminal");

        terminal
            .draw(|frame| render_prefix_overlay(&app, frame, app.view.terminal_area))
            .expect("draw prefix overlay");

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("PREFIX"));
        assert!(rendered.contains("Esc"));
        assert!(rendered.contains("Space"));
        assert!(rendered.contains("Commands"));
        assert!(rendered.contains("w"));
        assert!(rendered.contains("Spaces"));
        assert!(rendered.contains("?"));
        assert!(rendered.contains("Keys"), "{rendered}");
        assert!(!rendered.contains("Detach"));
    }

    #[test]
    fn prefix_mode_renders_indexed_navigation_hints_when_wide_enough() {
        let mut app = crate::app::state::AppState::test_new();
        app.mode = Mode::Prefix;
        app.view.terminal_area = ratatui::layout::Rect::new(0, 0, 160, 4);
        let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(160, 4))
            .expect("test terminal");

        terminal
            .draw(|frame| render_prefix_overlay(&app, frame, app.view.terminal_area))
            .expect("draw prefix overlay");

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("1..0"));
        assert!(rendered.contains("Tabs"));
        assert!(rendered.contains("shift+1..0"), "{rendered}");
        assert!(rendered.contains("Spaces"), "{rendered}");
        assert!(rendered.contains("alt+1..0"), "{rendered}");
        assert!(rendered.contains("Groups"));
    }

    #[test]
    fn keybind_help_shows_defaults_and_unset_optional_actions() {
        let app = crate::app::state::AppState::test_new();
        let groups = keybind_help_groups(&app);

        let global = groups
            .iter()
            .find(|(name, _)| *name == "Global")
            .expect("global group")
            .1
            .clone();
        let workspace_tab = groups
            .iter()
            .find(|(name, _)| *name == "Workspaces / Tabs")
            .expect("workspace tab group")
            .1
            .clone();
        let group_keys = groups
            .iter()
            .find(|(name, _)| *name == "Groups")
            .expect("groups group")
            .1
            .clone();
        let agents = groups
            .iter()
            .find(|(name, _)| *name == "Agents")
            .expect("agents group")
            .1
            .clone();
        let panes = groups
            .iter()
            .find(|(name, _)| *name == "Panes")
            .expect("panes group")
            .1
            .clone();

        assert!(global
            .iter()
            .any(|(key, label)| key == "prefix+space" && label.as_ref() == "Command Palette"));
        assert!(agents
            .iter()
            .any(|(key, label)| key == "Unset" && label.as_ref() == "Open Agent Menu"));
        assert!(panes
            .iter()
            .any(|(key, label)| key == "Unset" && label.as_ref() == "Toggle Right Sidebar"));
        assert!(workspace_tab
            .iter()
            .any(|(key, label)| key == "Unset" && label.as_ref() == "Previous Workspace"));
        assert!(workspace_tab
            .iter()
            .any(|(key, label)| key == "Unset" && label.as_ref() == "Next Workspace"));
        assert!(workspace_tab
            .iter()
            .any(|(key, label)| key == "Unset" && label.as_ref() == "Previous Agent"));
        assert!(workspace_tab
            .iter()
            .any(|(key, label)| key == "Unset" && label.as_ref() == "Next Agent"));
        assert!(workspace_tab
            .iter()
            .any(|(key, label)| key == "Unset" && label.as_ref() == "Focus Agent 1-9"));
        assert!(workspace_tab.iter().any(|(key, label)| {
            key == "prefix+shift+1..0" && label.as_ref() == "Switch Space 1-10"
        }));
        assert!(workspace_tab
            .iter()
            .any(|(key, label)| key == "prefix+1..0" && label.as_ref() == "Switch Tab 1-10"));
        assert!(group_keys.iter().any(|(key, label)| {
            key == "prefix+alt+1..0" && label.as_ref() == "Switch Group 1-10"
        }));
        assert!(panes
            .iter()
            .any(|(key, label)| key == "prefix+h" && label.as_ref() == "Focus Pane Left"));
        assert!(panes
            .iter()
            .any(|(key, label)| key == "prefix+j" && label.as_ref() == "Focus Pane Down"));
        assert!(panes
            .iter()
            .any(|(key, label)| key == "prefix+k" && label.as_ref() == "Focus Pane Up"));
        assert!(panes
            .iter()
            .any(|(key, label)| key == "prefix+l" && label.as_ref() == "Focus Pane Right"));
    }

    #[test]
    fn keybind_help_lines_follow_modal_option_hierarchy() {
        let app = crate::app::state::AppState::test_new();
        let option_width = 70;
        let lines = keybind_help_lines(&app, option_width, "");

        let (command_width, command_line) = lines
            .iter()
            .find(|(_, line)| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
                    .contains("Command Palette")
            })
            .expect("command palette row");
        let command_text = command_line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(*command_width, option_width);
        assert!(command_text.starts_with("  Command Palette"));
        assert!(command_text.ends_with("prefix+space"));
        assert_eq!(command_line.spans[0].style.fg, Some(app.palette.text));
        let command_shortcut = command_line.spans.last().expect("command shortcut");
        assert_eq!(command_shortcut.style.fg, Some(app.palette.mauve));
        assert!(command_shortcut
            .style
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD));

        let (_, unset_line) = lines
            .iter()
            .find(|(_, line)| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
                    .contains("Open Agent Menu")
            })
            .expect("unset action row");
        let unset_shortcut = unset_line.spans.last().expect("unset shortcut");
        assert_eq!(unset_shortcut.content.as_ref(), "Unset");
        assert_eq!(unset_shortcut.style.fg, Some(app.palette.overlay0));
        assert!(!unset_shortcut
            .style
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD));

        assert_ne!(lines.last().map(|(width, _)| *width), Some(0));
        assert_eq!(
            lines.iter().filter(|(width, _)| *width == 0).count(),
            keybind_help_groups(&app).len().saturating_sub(1)
        );
    }

    #[test]
    fn keybind_help_renders_actions_left_and_shortcuts_right() {
        let mut app = crate::app::state::AppState::test_new();
        app.mode = Mode::KeybindHelp;
        let area = Rect::new(0, 0, 100, 30);
        compute_view(&mut app, area);

        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal.draw(|frame| render(&app, frame)).expect("render");
        let buffer = terminal.backend().buffer();
        let rows = (0..area.height)
            .map(|row| {
                (
                    row,
                    buffer_row_text(buffer, Rect::new(0, row, area.width, 1), row),
                )
            })
            .collect::<Vec<_>>();

        let (_, prefix_text) = rows
            .iter()
            .find(|(_, text)| text.contains("Prefix Mode"))
            .expect("prefix mode row");
        let (command_row, command_text) = rows
            .iter()
            .find(|(_, text)| text.contains("Command Palette"))
            .expect("command palette row");
        let prefix_shortcut = crate::config::format_key_combo((app.prefix_code, app.prefix_mods));
        let command_shortcut = "prefix+space";
        let prefix_end =
            prefix_text.find(&prefix_shortcut).expect("prefix shortcut") + prefix_shortcut.len();
        let command_end = command_text
            .find(command_shortcut)
            .expect("command shortcut")
            + command_shortcut.len();

        assert!(
            prefix_text.find("Prefix Mode").expect("prefix action")
                < prefix_text.find(&prefix_shortcut).expect("prefix shortcut")
        );
        assert!(
            command_text
                .find("Command Palette")
                .expect("command action")
                < command_text
                    .find(command_shortcut)
                    .expect("command shortcut")
        );
        assert_eq!(prefix_end, command_end);

        let action_col = command_text
            .find("Command Palette")
            .expect("command action") as u16;
        let shortcut_col = command_text
            .find(command_shortcut)
            .expect("command shortcut") as u16;
        assert_eq!(
            buffer[(action_col, *command_row)].style().fg,
            Some(app.palette.text)
        );
        assert_eq!(
            buffer[(shortcut_col, *command_row)].style().fg,
            Some(app.palette.mauve)
        );
        assert!(buffer[(shortcut_col, *command_row)]
            .style()
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD));
    }

    #[test]
    fn keybind_help_shows_custom_command_descriptions() {
        let mut app = crate::app::state::AppState::test_new();
        app.keybinds.custom_commands = vec![
            crate::config::CustomCommandKeybind {
                bindings: crate::config::ActionKeybinds::prefix("alt+g"),
                label: "prefix+alt+g".to_string(),
                command: "terminal-browser".to_string(),
                action: crate::config::CustomCommandAction::Pane,
                description: Some("open terminal-browser".to_string()),
            },
            crate::config::CustomCommandKeybind {
                bindings: crate::config::ActionKeybinds::prefix("alt+h"),
                label: "prefix+alt+h".to_string(),
                command: "echo hello".to_string(),
                action: crate::config::CustomCommandAction::Shell,
                description: None,
            },
        ];

        let groups = keybind_help_groups(&app);
        let custom = groups
            .iter()
            .find(|(name, _)| *name == "Custom")
            .expect("custom group")
            .1
            .clone();
        assert!(
            custom
                .iter()
                .any(|(key, label)| key == "prefix+alt+g"
                    && label.as_ref() == "open terminal-browser")
        );
        assert!(custom
            .iter()
            .any(|(key, label)| key == "prefix+alt+h" && label.as_ref() == "Custom Command"));

        let rendered_help = keybind_help_lines(&app, 70, "")
            .into_iter()
            .flat_map(|(_, line)| line.spans)
            .map(|span| span.content.into_owned())
            .collect::<Vec<_>>()
            .join("");
        assert!(rendered_help.contains("open terminal-browser"));
        assert!(rendered_help.contains("Custom Command"));
    }
}
