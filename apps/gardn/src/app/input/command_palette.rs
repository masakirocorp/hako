use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::{
    command_palette::{
        command_palette_filtered_commands, command_palette_filtered_commands_for_view,
        CommandPaletteAction, CommandPaletteCommand,
    },
    state::{AppState, Mode},
    view_state::ClientViewState,
    App,
};

use super::{
    modal::modal_action_from_buttons, modal::ModalAction, ScrollbarClickTarget,
    MODAL_PAGE_SCROLL_ROWS,
};

pub(super) fn open_command_palette(state: &mut AppState) {
    state.command_palette.query.clear();
    state.command_palette.list.select(0);
    state.command_palette.list.hide();
    state.command_palette.scroll = 0;
    state.mode = Mode::CommandPalette;
}

#[cfg(test)]
pub(crate) fn open_command_palette_for_view(view: &mut ClientViewState) {
    view.command_palette.query.clear();
    view.command_palette.list.select(0);
    view.command_palette.list.hide();
    view.command_palette.scroll = 0;
    view.mode = Mode::CommandPalette;
}

pub(crate) fn handle_command_palette_key_for_view(
    state: &AppState,
    view: &mut ClientViewState,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => {
            view.return_to_active_workspace_mode();
        }
        KeyCode::Enter => {}
        KeyCode::Up => {
            move_command_palette_selection_for_view(state, view, false);
        }
        KeyCode::Down => {
            move_command_palette_selection_for_view(state, view, true);
        }
        KeyCode::PageUp => {
            scroll_command_palette_rows_for_view(state, view, -MODAL_PAGE_SCROLL_ROWS)
        }
        KeyCode::PageDown => {
            scroll_command_palette_rows_for_view(state, view, MODAL_PAGE_SCROLL_ROWS)
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            move_command_palette_selection_for_view(state, view, false);
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            move_command_palette_selection_for_view(state, view, true);
        }
        KeyCode::Backspace => {
            view.command_palette.query.pop();
            clamp_command_palette_selection_for_view(state, view);
        }
        KeyCode::Char(c) if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT => {
            view.command_palette.query.push(c);
            clamp_command_palette_selection_for_view(state, view);
        }
        _ => {}
    }
}

pub(crate) fn selected_command_palette_action_for_view(
    state: &AppState,
    view: &ClientViewState,
) -> Option<CommandPaletteAction> {
    command_palette_filtered_commands_for_view(state, view)
        .get(view.command_palette.list.selected)
        .map(|command| command.action.clone())
}

pub(super) fn command_palette_visible_commands(state: &AppState) -> Vec<CommandPaletteCommand> {
    command_palette_filtered_commands(state)
}

impl App {
    pub(crate) fn handle_command_palette_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => leave_command_palette(&mut self.state),
            KeyCode::Enter => self.execute_selected_command_palette_command(),
            KeyCode::Up => {
                move_command_palette_selection(&mut self.state, false);
            }
            KeyCode::Down => {
                move_command_palette_selection(&mut self.state, true);
            }
            KeyCode::PageUp => {
                scroll_command_palette_rows(&mut self.state, -MODAL_PAGE_SCROLL_ROWS)
            }
            KeyCode::PageDown => {
                scroll_command_palette_rows(&mut self.state, MODAL_PAGE_SCROLL_ROWS)
            }
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                move_command_palette_selection(&mut self.state, false);
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                move_command_palette_selection(&mut self.state, true);
            }
            KeyCode::Backspace => {
                self.state.command_palette.query.pop();
                clamp_command_palette_selection(&mut self.state);
            }
            KeyCode::Char(c)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.state.command_palette.query.push(c);
                clamp_command_palette_selection(&mut self.state);
            }
            _ => {}
        }
    }

    pub(super) fn execute_selected_command_palette_command(&mut self) {
        let commands = command_palette_visible_commands(&self.state);
        let Some(command) = commands
            .get(self.state.command_palette.list.selected)
            .cloned()
        else {
            return;
        };
        execute_command_palette_action(self, command.action);
    }
}

impl App {
    pub(super) async fn execute_selected_command_palette_command_interactive(&mut self) {
        let commands = command_palette_visible_commands(&self.state);
        let Some(command) = commands
            .get(self.state.command_palette.list.selected)
            .cloned()
        else {
            return;
        };

        if command.action == CommandPaletteAction::OpenGithub {
            execute_command_palette_action(self, command.action);
            return;
        }
        if let Some(kind) = command.action.project_command_kind() {
            leave_command_palette(&mut self.state);
            self.refresh_host_terminal_theme_for(std::time::Duration::from_millis(500))
                .await;
            let previous_toast = self.state.toast.clone();
            if let Err(err) = self
                .state
                .open_project_command(&mut self.terminal_runtimes, kind)
            {
                self.state.toast = Some(crate::app::state::ToastNotification {
                    kind: crate::app::state::ToastKind::NeedsAttention,
                    title: format!("{} Command Failed", self.state.project_command_role(kind)),
                    context: err,
                    position: None,
                    target: None,
                });
                self.sync_toast_deadline(previous_toast);
            }
            return;
        }

        execute_command_palette_action(self, command.action);
    }
}

fn leave_command_palette(state: &mut AppState) {
    state.return_to_active_workspace_mode();
}

pub(crate) fn close_command_palette(state: &mut AppState) {
    leave_command_palette(state);
}

pub(super) fn command_palette_action_button_at(
    state: &AppState,
    col: u16,
    row: u16,
) -> Option<ModalAction> {
    let inner = command_palette_inner_rect(state)?;
    let (run, close) = crate::ui::command_palette_button_rects(inner);
    modal_action_from_buttons(
        col,
        row,
        &[(run, ModalAction::Apply), (close, ModalAction::Close)],
    )
}

fn command_palette_action_button_at_for_view(
    view: &ClientViewState,
    col: u16,
    row: u16,
) -> Option<ModalAction> {
    let inner = crate::ui::command_palette_inner_rect(view.screen_rect())?;
    let (run, close) = crate::ui::command_palette_button_rects(inner);
    modal_action_from_buttons(
        col,
        row,
        &[(run, ModalAction::Apply), (close, ModalAction::Close)],
    )
}

fn clamp_command_palette_selection(state: &mut AppState) {
    let count = command_palette_visible_commands(state).len();
    if count == 0 {
        state.command_palette.list.select(0);
        state.command_palette.scroll = 0;
        return;
    }

    state
        .command_palette
        .list
        .select(state.command_palette.list.selected.min(count - 1));
    ensure_command_palette_selection_visible(state);
}

fn move_command_palette_selection(state: &mut AppState, down: bool) -> bool {
    let count = command_palette_visible_commands(state).len();
    if count == 0 {
        state.command_palette.list.select(0);
        state.command_palette.scroll = 0;
        return false;
    }

    let previous = state.command_palette.list.selected;
    let current = previous.min(count - 1);
    if current != previous {
        state.command_palette.list.select(current);
    }
    if down {
        state.command_palette.list.move_next(count);
    } else {
        state.command_palette.list.move_prev();
    }
    let changed = state.command_palette.list.selected != previous;
    ensure_command_palette_selection_visible(state);
    changed
}

pub(crate) fn scroll_command_palette_rows(state: &mut AppState, delta: i16) {
    let max_scroll = command_palette_max_scroll(state);
    let next = if delta.is_negative() {
        state
            .command_palette
            .scroll
            .saturating_sub(delta.unsigned_abs() as usize)
    } else {
        state
            .command_palette
            .scroll
            .saturating_add(delta as usize)
            .min(max_scroll)
    };
    state.command_palette.scroll = next.min(max_scroll);
}

pub(crate) fn hover_command_palette_selection(state: &mut AppState, col: u16, row: u16) {
    let hovered = command_palette_selection_at(state, col, row);
    state.command_palette.list.hover(hovered);
}

pub(crate) fn select_command_palette_selection(state: &mut AppState, col: u16, row: u16) {
    match command_palette_selection_at(state, col, row) {
        Some(selected) => {
            state.command_palette.list.select(selected);
            ensure_command_palette_selection_visible(state);
        }
        None => state.command_palette.list.hover(None),
    }
}

fn command_palette_selection_at(state: &AppState, col: u16, row: u16) -> Option<usize> {
    let (list, rows) = command_palette_viewport(state)?;
    let row_idx = list.hit_visual_row(col, row)?;
    rows.get(row_idx).copied().flatten()
}

pub(crate) fn command_palette_contains_point(state: &AppState, col: u16, row: u16) -> bool {
    command_palette_popup_rect(state).is_some_and(|popup| {
        col >= popup.x
            && col < popup.x + popup.width
            && row >= popup.y
            && row < popup.y + popup.height
    })
}

pub(crate) fn handle_command_palette_mouse_for_view(
    state: &AppState,
    view: &mut ClientViewState,
    mouse: MouseEvent,
) -> bool {
    if view.mode != Mode::CommandPalette {
        return false;
    }

    match mouse.kind {
        MouseEventKind::Moved => {
            hover_command_palette_selection_for_view(state, view, mouse.column, mouse.row);
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if matches!(
                command_palette_action_button_at_for_view(view, mouse.column, mouse.row),
                Some(ModalAction::Close)
            ) {
                view.return_to_active_workspace_mode();
                view.command_palette.query.clear();
                view.command_palette.list.select(0);
                view.command_palette.scroll = 0;
            } else if command_palette_contains_point_for_view(view, mouse.column, mouse.row) {
                select_command_palette_selection_for_view(state, view, mouse.column, mouse.row);
            } else {
                view.return_to_active_workspace_mode();
                view.command_palette.query.clear();
                view.command_palette.list.select(0);
                view.command_palette.scroll = 0;
            }
        }
        MouseEventKind::ScrollDown => {
            scroll_command_palette_rows_for_view(state, view, super::MODAL_WHEEL_SCROLL_ROWS);
        }
        MouseEventKind::ScrollUp => {
            scroll_command_palette_rows_for_view(state, view, -super::MODAL_WHEEL_SCROLL_ROWS);
        }
        MouseEventKind::Up(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left) => {}
        _ => {}
    }

    true
}

fn clamp_command_palette_selection_for_view(state: &AppState, view: &mut ClientViewState) {
    let count = command_palette_filtered_commands_for_view(state, view).len();
    if count == 0 {
        view.command_palette.list.select(0);
        view.command_palette.scroll = 0;
        return;
    }

    view.command_palette
        .list
        .select(view.command_palette.list.selected.min(count - 1));
    ensure_command_palette_selection_visible_for_view(state, view);
}

fn move_command_palette_selection_for_view(
    state: &AppState,
    view: &mut ClientViewState,
    down: bool,
) -> bool {
    let count = command_palette_filtered_commands_for_view(state, view).len();
    if count == 0 {
        view.command_palette.list.select(0);
        view.command_palette.scroll = 0;
        return false;
    }

    let previous = view.command_palette.list.selected;
    let current = previous.min(count - 1);
    if current != previous {
        view.command_palette.list.select(current);
    }
    if down {
        view.command_palette.list.move_next(count);
    } else {
        view.command_palette.list.move_prev();
    }
    let changed = view.command_palette.list.selected != previous;
    ensure_command_palette_selection_visible_for_view(state, view);
    changed
}

fn scroll_command_palette_rows_for_view(state: &AppState, view: &mut ClientViewState, delta: i16) {
    let max_scroll = command_palette_max_scroll_for_view(state, view);
    let next = if delta.is_negative() {
        view.command_palette
            .scroll
            .saturating_sub(delta.unsigned_abs() as usize)
    } else {
        view.command_palette
            .scroll
            .saturating_add(delta as usize)
            .min(max_scroll)
    };
    view.command_palette.scroll = next.min(max_scroll);
}

fn hover_command_palette_selection_for_view(
    state: &AppState,
    view: &mut ClientViewState,
    col: u16,
    row: u16,
) {
    let hovered = command_palette_selection_at_for_view(state, view, col, row);
    view.command_palette.list.hover(hovered);
}

fn select_command_palette_selection_for_view(
    state: &AppState,
    view: &mut ClientViewState,
    col: u16,
    row: u16,
) {
    match command_palette_selection_at_for_view(state, view, col, row) {
        Some(selected) => {
            view.command_palette.list.select(selected);
            ensure_command_palette_selection_visible_for_view(state, view);
        }
        None => view.command_palette.list.hover(None),
    }
}

fn command_palette_selection_at_for_view(
    state: &AppState,
    view: &ClientViewState,
    col: u16,
    row: u16,
) -> Option<usize> {
    let (list, rows) = command_palette_viewport_for_view(state, view)?;
    let row_idx = list.hit_visual_row(col, row)?;
    rows.get(row_idx).copied().flatten()
}

fn command_palette_contains_point_for_view(view: &ClientViewState, col: u16, row: u16) -> bool {
    crate::ui::command_palette_popup_rect(view.screen_rect()).is_some_and(|popup| {
        col >= popup.x
            && col < popup.x + popup.width
            && row >= popup.y
            && row < popup.y + popup.height
    })
}

fn command_palette_max_scroll_for_view(state: &AppState, view: &ClientViewState) -> usize {
    command_palette_viewport_for_view(state, view)
        .map(|(list, _)| list.viewport.max_scroll())
        .unwrap_or(0)
}

fn command_palette_viewport_for_view(
    state: &AppState,
    view: &ClientViewState,
) -> Option<(crate::ui::ModalListGeometry, Vec<Option<usize>>)> {
    let rows = command_palette_rows_for_view(state, view)?;
    let list = crate::ui::command_palette_list_geometry(
        view.screen_rect(),
        rows.len(),
        view.command_palette.scroll,
    )?;
    Some((list, rows))
}

fn command_palette_rows_for_view(
    state: &AppState,
    view: &ClientViewState,
) -> Option<Vec<Option<usize>>> {
    let commands = command_palette_filtered_commands_for_view(state, view);
    if commands.is_empty() {
        return None;
    }
    let mut rows = Vec::new();
    let mut last_group = None;
    for (idx, command) in commands.iter().enumerate() {
        if last_group != Some(command.group) {
            if last_group.is_some() {
                rows.push(None);
            }
            rows.push(None);
            last_group = Some(command.group);
        }
        rows.push(Some(idx));
    }

    Some(rows)
}

pub(super) fn command_palette_scrollbar_target_at(
    state: &AppState,
    col: u16,
    row: u16,
) -> Option<ScrollbarClickTarget> {
    let metrics = command_palette_scroll_metrics(state)?;
    let track = command_palette_scrollbar_track(state)?;
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

pub(super) fn command_palette_offset_for_drag_row(
    state: &AppState,
    row: u16,
    grab_row_offset: u16,
) -> Option<usize> {
    let metrics = command_palette_scroll_metrics(state)?;
    let track = command_palette_scrollbar_track(state)?;
    Some(crate::ui::scrollbar_offset_from_drag_row(
        metrics,
        track,
        row,
        grab_row_offset,
    ))
}

pub(super) fn set_command_palette_offset_from_bottom(
    state: &mut AppState,
    offset_from_bottom: usize,
) {
    let Some((list, _)) = command_palette_viewport(state) else {
        state.command_palette.scroll = 0;
        return;
    };
    state.command_palette.scroll = list
        .viewport
        .scroll_from_offset_from_bottom(offset_from_bottom);
}

fn command_palette_scroll_metrics(state: &AppState) -> Option<crate::pane::ScrollMetrics> {
    let (list, _) = command_palette_viewport(state)?;
    Some(list.metrics())
}

fn command_palette_max_scroll(state: &AppState) -> usize {
    command_palette_viewport(state)
        .map(|(list, _)| list.viewport.max_scroll())
        .unwrap_or(0)
}

fn command_palette_scrollbar_track(state: &AppState) -> Option<Rect> {
    let (list, _) = command_palette_viewport(state)?;
    list.scroll_area.track
}

fn ensure_command_palette_selection_visible_for_view(state: &AppState, view: &mut ClientViewState) {
    let Some((list, rows)) = command_palette_viewport_for_view(state, view) else {
        view.command_palette.scroll = 0;
        return;
    };

    let Some(selected_row) = rows
        .iter()
        .position(|row| *row == Some(view.command_palette.list.selected))
    else {
        view.command_palette.scroll = list.viewport.scroll();
        return;
    };

    let first_section_row = selected_row
        .checked_sub(1)
        .filter(|idx| rows.get(*idx).is_some_and(Option::is_none));
    view.command_palette.scroll = list
        .viewport
        .ensure_visible(selected_row, first_section_row);
}

fn ensure_command_palette_selection_visible(state: &mut AppState) {
    let Some((list, rows)) = command_palette_viewport(state) else {
        state.command_palette.scroll = 0;
        return;
    };

    let Some(selected_row) = rows
        .iter()
        .position(|row| *row == Some(state.command_palette.list.selected))
    else {
        state.command_palette.scroll = list.viewport.scroll();
        return;
    };

    let first_section_row = selected_row
        .checked_sub(1)
        .filter(|idx| rows.get(*idx).is_some_and(Option::is_none));
    state.command_palette.scroll = list
        .viewport
        .ensure_visible(selected_row, first_section_row);
}

fn command_palette_viewport(
    state: &AppState,
) -> Option<(crate::ui::ModalListGeometry, Vec<Option<usize>>)> {
    let rows = command_palette_rows_for_input(state)?;
    let list = crate::ui::command_palette_list_geometry(
        state.screen_rect(),
        rows.len(),
        state.command_palette.scroll,
    )?;
    Some((list, rows))
}

fn command_palette_rows_for_input(state: &AppState) -> Option<Vec<Option<usize>>> {
    let commands = command_palette_visible_commands(state);
    if commands.is_empty() {
        return None;
    }
    let mut rows = Vec::new();
    let mut last_group = None;
    for (idx, command) in commands.iter().enumerate() {
        if last_group != Some(command.group) {
            if last_group.is_some() {
                rows.push(None);
            }
            rows.push(None);
            last_group = Some(command.group);
        }
        rows.push(Some(idx));
    }

    Some(rows)
}

fn command_palette_inner_rect(state: &AppState) -> Option<Rect> {
    crate::ui::command_palette_inner_rect(state.screen_rect())
}

fn command_palette_popup_rect(state: &AppState) -> Option<Rect> {
    crate::ui::command_palette_popup_rect(state.screen_rect())
}

fn open_new_agent_from_palette(app: &mut App) {
    let Some(ws_idx) = app.state.active else {
        return;
    };
    super::agent_profile_picker::open_new_agent_picker_for_workspace(&mut app.state, ws_idx);
}

pub(crate) fn execute_command_palette_action(app: &mut App, action: CommandPaletteAction) {
    match action {
        CommandPaletteAction::OpenNavigator => {
            app.state.open_navigator();
            return;
        }
        CommandPaletteAction::NewWorkspace => {
            app.execute_tui_navigate_action(
                super::navigate::NavigateAction::NewWorkspace,
                super::navigate::ActionContext::Navigate,
            );
        }
        CommandPaletteAction::RenameWorkspace => {
            let selected = app.state.selected;
            if app.state.workspace_in_active_group(selected) {
                super::modal::open_rename_workspace(
                    &mut app.state,
                    &app.terminal_runtimes,
                    selected,
                );
                return;
            }
        }
        CommandPaletteAction::CloseWorkspace => {
            app.execute_tui_navigate_action(
                super::navigate::NavigateAction::CloseWorkspace,
                super::navigate::ActionContext::Navigate,
            );
        }
        CommandPaletteAction::PreviousWorkspace => app.execute_tui_navigate_action(
            super::navigate::NavigateAction::PreviousWorkspace,
            super::navigate::ActionContext::Navigate,
        ),
        CommandPaletteAction::NextWorkspace => app.execute_tui_navigate_action(
            super::navigate::NavigateAction::NextWorkspace,
            super::navigate::ActionContext::Navigate,
        ),
        CommandPaletteAction::SwitchWorkspace(idx) => app.execute_tui_navigate_action(
            super::navigate::NavigateAction::SwitchWorkspace(idx),
            super::navigate::ActionContext::Navigate,
        ),
        CommandPaletteAction::NewTab => {
            super::modal::open_new_tab_dialog(&mut app.state);
            return;
        }
        CommandPaletteAction::NewAgent => {
            open_new_agent_from_palette(app);
            return;
        }
        CommandPaletteAction::SwitchTab(idx) => app.execute_tui_navigate_action(
            super::navigate::NavigateAction::SwitchTab(idx),
            super::navigate::ActionContext::Navigate,
        ),
        CommandPaletteAction::RenameTab => {
            super::modal::open_rename_active_tab(&mut app.state, false);
            return;
        }
        CommandPaletteAction::PreviousTab => app.execute_tui_navigate_action(
            super::navigate::NavigateAction::PreviousTab,
            super::navigate::ActionContext::Navigate,
        ),
        CommandPaletteAction::NextTab => app.execute_tui_navigate_action(
            super::navigate::NavigateAction::NextTab,
            super::navigate::ActionContext::Navigate,
        ),
        CommandPaletteAction::CloseTab => app.execute_tui_navigate_action(
            super::navigate::NavigateAction::CloseTab,
            super::navigate::ActionContext::Navigate,
        ),
        CommandPaletteAction::SplitVertical => app.execute_tui_navigate_action(
            super::navigate::NavigateAction::SplitVertical,
            super::navigate::ActionContext::Navigate,
        ),
        CommandPaletteAction::SplitHorizontal => app.execute_tui_navigate_action(
            super::navigate::NavigateAction::SplitHorizontal,
            super::navigate::ActionContext::Navigate,
        ),
        CommandPaletteAction::ClosePane => app.execute_tui_navigate_action(
            super::navigate::NavigateAction::ClosePane,
            super::navigate::ActionContext::Navigate,
        ),
        CommandPaletteAction::RenamePane => {
            if let Some(pane_id) = app
                .state
                .active
                .and_then(|ws_idx| app.state.workspaces.get(ws_idx))
                .and_then(|ws| ws.focused_pane_id())
            {
                super::modal::open_rename_pane(&mut app.state, pane_id);
                return;
            }
        }
        CommandPaletteAction::Fullscreen => app.execute_tui_navigate_action(
            super::navigate::NavigateAction::Zoom,
            super::navigate::ActionContext::Navigate,
        ),
        CommandPaletteAction::EditScrollback => {
            app.launch_focused_scrollback_editor();
            return;
        }
        CommandPaletteAction::ResizeMode => {
            app.state.mode = Mode::Resize;
            return;
        }
        CommandPaletteAction::FocusPane(direction) => {
            let action = match direction {
                crate::layout::NavDirection::Left => super::navigate::NavigateAction::FocusPaneLeft,
                crate::layout::NavDirection::Down => super::navigate::NavigateAction::FocusPaneDown,
                crate::layout::NavDirection::Up => super::navigate::NavigateAction::FocusPaneUp,
                crate::layout::NavDirection::Right => {
                    super::navigate::NavigateAction::FocusPaneRight
                }
            };
            app.execute_tui_navigate_action(action, super::navigate::ActionContext::Navigate);
        }
        CommandPaletteAction::CyclePaneNext => app.execute_tui_navigate_action(
            super::navigate::NavigateAction::CyclePaneNext,
            super::navigate::ActionContext::Navigate,
        ),
        CommandPaletteAction::CyclePanePrevious => app.execute_tui_navigate_action(
            super::navigate::NavigateAction::CyclePanePrevious,
            super::navigate::ActionContext::Navigate,
        ),
        CommandPaletteAction::OpenGroupMenu => {
            super::modal::open_group_menu(&mut app.state);
            return;
        }
        CommandPaletteAction::ShowAllGroups => app.state.show_all_groups(),
        CommandPaletteAction::NewGroup => {
            super::modal::open_new_group_dialog(&mut app.state);
            return;
        }
        CommandPaletteAction::RenameGroup => {
            super::modal::open_rename_group(&mut app.state);
            return;
        }
        CommandPaletteAction::DeleteGroup => {
            let active_group = app.state.active_group;
            super::modal::open_confirm_delete_group(&mut app.state, active_group);
            return;
        }
        CommandPaletteAction::ToggleGroupFilter => app.state.toggle_group_filter(),
        CommandPaletteAction::PreviousGroup => app.state.previous_group(),
        CommandPaletteAction::NextGroup => app.state.next_group(),
        CommandPaletteAction::SwitchGroup(idx) => app.state.switch_group(idx),
        CommandPaletteAction::OpenAgentMenu => {
            super::modal::open_agent_menu(&mut app.state);
            return;
        }
        CommandPaletteAction::OpenContextMenu => {
            super::modal::open_context_menu_for_focus(&mut app.state);
            return;
        }
        CommandPaletteAction::SetAgentScope(scope) => {
            app.state.agent_panel_scope = scope;
            app.state.agent_panel_scroll = 0;
            app.state.mark_session_dirty();
        }
        CommandPaletteAction::PreviousAgent => app.state.previous_agent(),
        CommandPaletteAction::NextAgent => app.state.next_agent(),
        CommandPaletteAction::OpenBrowser => {
            app.state.request_open_project_command =
                Some(crate::app::state::ProjectCommandKind::Browser);
        }
        CommandPaletteAction::OpenReview => {
            app.state.request_open_project_command =
                Some(crate::app::state::ProjectCommandKind::Review);
        }
        CommandPaletteAction::OpenEditor => {
            app.state.request_open_project_command =
                Some(crate::app::state::ProjectCommandKind::Editor);
        }
        CommandPaletteAction::OpenGithub => {
            app.with_default_github_view(|app, view| app.open_github_for_view(view));
            return;
        }
        CommandPaletteAction::Github(action) => {
            app.with_default_github_view(|app, view| {
                app.execute_client_view_command_palette_action(
                    view,
                    CommandPaletteAction::Github(action),
                )
            });
            return;
        }
        CommandPaletteAction::ToggleSidebar => {
            app.state.sidebar_collapsed = !app.state.sidebar_collapsed;
            app.state.mark_session_dirty();
        }
        CommandPaletteAction::ToggleContextBar => {
            let visible = app
                .state
                .context_bar_is_visible(app.state.context_bar_visibility_override);
            app.state.context_bar_visibility_override = Some(!visible);
        }
        CommandPaletteAction::ZenMode => app.state.zen_mode = !app.state.zen_mode,
        CommandPaletteAction::ToggleRightSidebar => {
            if app.state.view.right_sidebar_rect != ratatui::layout::Rect::default() {
                app.state.right_sidebar_collapsed = !app.state.right_sidebar_collapsed;
                app.state.mark_session_dirty();
            }
        }
        CommandPaletteAction::OpenGlobalMenu => {
            super::modal::open_global_menu(&mut app.state);
            return;
        }
        CommandPaletteAction::OpenSettings => {
            super::settings::open_settings(&mut app.state);
            return;
        }
        CommandPaletteAction::OpenKeybinds => {
            super::modal::open_keybind_help(&mut app.state);
            return;
        }
        CommandPaletteAction::ReloadConfig => app.state.request_reload_config = true,
        CommandPaletteAction::OpenNotificationTarget => {
            app.state.focus_toast_target();
            if app.state.mode != Mode::CommandPalette {
                return;
            }
        }
        CommandPaletteAction::DetachOrQuit => super::modal::request_detach(&mut app.state),
        CommandPaletteAction::ProjectCommand(command_id) => {
            leave_command_palette(&mut app.state);
            let previous_toast = app.state.toast.clone();
            if let Err(error) = app.run_project_command_on_resolved_host(&command_id) {
                app.state.toast = Some(crate::app::state::ToastNotification {
                    kind: crate::app::state::ToastKind::NeedsAttention,
                    title: "Project Command Failed".to_string(),
                    context: error,
                    position: None,
                    target: None,
                });
                app.sync_toast_deadline(previous_toast);
            }
            return;
        }
        CommandPaletteAction::CustomCommand(idx) => {
            let Some(binding) = app.state.keybinds.custom_commands.get(idx).cloned() else {
                return;
            };
            app.launch_custom_command(binding, super::navigate::ActionContext::Navigate);
            return;
        }
    }

    leave_command_palette(&mut app.state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, workspace::Workspace};

    fn app_with_space() -> App {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &Config::default(),
            true,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.state.workspaces = vec![Workspace::test_new("test")];
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::CommandPalette;
        app
    }

    fn rendered_text_point(app: &App, text: &str, width: u16, height: u16) -> (u16, u16) {
        let backend = ratatui::backend::TestBackend::new(width, height);
        let mut terminal = ratatui::Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| crate::ui::render(&app.state, frame))
            .expect("render command palette");
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

    #[test]
    fn command_palette_new_agent_opens_agent_profile_picker_when_multiple_profiles_exist() {
        let mut app = app_with_space();
        app.state.command_palette.query = "new agent".to_string();

        app.state.integration_recommendations = vec![
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
                available: true,
                path: std::path::PathBuf::from("/tmp/gardn-test-claude"),
                state: crate::integration::IntegrationStatusKind::Current,
            },
        ];
        app.handle_command_palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert_eq!(app.state.mode, Mode::AgentProfilePicker);
        assert!(app.state.agent_profile_picker.query.is_empty());
        assert_eq!(app.state.agent_profile_picker.ws_idx, 0);
    }

    #[test]
    fn command_palette_click_uses_rendered_mobile_geometry() {
        let mut app = app_with_space();
        crate::ui::compute_view(&mut app.state, ratatui::layout::Rect::new(0, 0, 119, 24));
        app.state.command_palette.query = "new tab".to_string();
        app.state.command_palette.list.select(0);

        let backend = ratatui::backend::TestBackend::new(119, 24);
        let mut terminal = ratatui::Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| crate::ui::render(&app.state, frame))
            .expect("render command palette");

        let buffer = terminal.backend().buffer();
        let (new_tab_x, new_tab_y) = (0..24)
            .find_map(|y| {
                (0..112).find_map(|x| {
                    ["N", "e", "w", " ", "T", "a", "b"]
                        .iter()
                        .enumerate()
                        .all(|(idx, ch)| buffer[(x + idx as u16, y)].symbol() == *ch)
                        .then_some((x, y))
                })
            })
            .expect("new tab command");
        let new_tab_idx = command_palette_visible_commands(&app.state)
            .iter()
            .position(|command| command.action == CommandPaletteAction::NewTab)
            .expect("new tab command index");

        app.handle_mouse(super::super::mouse(
            crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            new_tab_x,
            new_tab_y,
        ));

        assert_eq!(app.state.command_palette.list.selected, new_tab_idx);
    }

    #[test]
    fn command_palette_filters_commands_by_query() {
        let mut app = app_with_space();
        app.state.command_palette.query = "right side".to_string();

        let commands = command_palette_visible_commands(&app.state);

        assert!(commands
            .iter()
            .any(|command| command.title == "Toggle Right Sidebar"));
        assert!(commands.iter().all(|command| {
            command.title.to_ascii_lowercase().contains("right")
                || command.group.to_ascii_lowercase().contains("right")
        }));
    }

    #[test]
    fn command_palette_enter_opens_workspace_navigator() {
        let mut app = app_with_space();
        app.state.command_palette.query = "workspace navigator".to_string();

        app.handle_command_palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert_eq!(app.state.mode, Mode::Navigator);
        assert!(app.state.navigator.list.visible().is_some());
    }

    #[test]
    fn command_palette_enter_executes_selected_command() {
        let mut app = app_with_space();
        app.state.command_palette.query = "new tab".to_string();

        app.handle_command_palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert_eq!(app.state.mode, Mode::RenameTab);
        assert!(app.state.creating_new_tab);
    }

    #[test]
    fn command_palette_enter_switches_to_selected_tab() {
        let mut app = app_with_space();
        app.state.workspaces[0].test_add_tab(Some("logs"));
        app.state.command_palette.query = "switch to tab logs".to_string();

        app.handle_command_palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert_eq!(app.state.workspaces[0].active_tab_index(), 1);
        assert_eq!(app.state.mode, Mode::Terminal);
    }

    #[test]
    fn command_palette_enter_requests_review_and_closes_palette() {
        let mut app = app_with_space();
        app.state.command_palette.query = "open review".to_string();

        app.handle_command_palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert_eq!(
            app.state.request_open_project_command,
            Some(crate::app::state::ProjectCommandKind::Review)
        );
        assert_eq!(app.state.mode, Mode::Terminal);
    }

    #[test]
    fn command_palette_close_last_space_deletes_space_and_shows_empty_group() {
        let mut app = app_with_space();
        app.state.confirm_close = false;
        app.state.command_palette.query = "close selected space".to_string();

        app.handle_command_palette_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::empty()));

        assert_eq!(app.state.mode, Mode::Navigate);
        assert!(app.state.workspaces.is_empty());
        assert_eq!(app.state.active, None);
    }

    #[test]
    fn command_palette_selection_clamps() {
        let mut app = app_with_space();

        app.handle_command_palette_key(KeyEvent::new(KeyCode::Up, KeyModifiers::empty()));
        assert_eq!(app.state.command_palette.list.selected, 0);

        app.handle_command_palette_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        assert_eq!(app.state.command_palette.list.selected, 1);
    }

    #[test]
    fn command_palette_scrolls_when_down_key_moves_selection_past_rendered_rows() {
        let mut app = app_with_space();
        app.state.view.sidebar_rect = ratatui::layout::Rect::new(0, 0, 26, 20);
        app.state.view.terminal_area = ratatui::layout::Rect::new(26, 0, 80, 20);

        for _ in 0..7 {
            app.handle_command_palette_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        }

        let visible_commands = command_palette_visible_commands(&app.state);
        let selected_title = visible_commands
            .get(app.state.command_palette.list.selected)
            .map(|command| command.title.as_str())
            .expect("selected command");
        assert_eq!(app.state.command_palette.list.selected, 7);
        assert!(app.state.command_palette.scroll > 0);
        rendered_text_point(&app, selected_title, 106, 20);
    }

    #[test]
    fn command_palette_page_keys_scroll_rows_without_changing_selection() {
        let mut app = app_with_space();
        app.state.view.sidebar_rect = ratatui::layout::Rect::new(0, 0, 26, 20);
        app.state.view.terminal_area = ratatui::layout::Rect::new(26, 0, 80, 20);

        app.handle_command_palette_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::empty()));
        assert_eq!(app.state.command_palette.list.selected, 0);
        assert_eq!(
            app.state.command_palette.scroll,
            MODAL_PAGE_SCROLL_ROWS as usize
        );

        app.handle_command_palette_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::empty()));
        assert_eq!(app.state.command_palette.list.selected, 0);
        assert_eq!(app.state.command_palette.scroll, 0);
    }

    #[test]
    fn command_palette_keeps_section_header_reachable_at_top() {
        let mut app = app_with_space();
        app.state.view.sidebar_rect = ratatui::layout::Rect::new(0, 0, 26, 20);
        app.state.view.terminal_area = ratatui::layout::Rect::new(26, 0, 80, 20);

        for _ in 0..12 {
            app.handle_command_palette_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        }
        assert!(app.state.command_palette.scroll > 0);

        for _ in 0..12 {
            app.handle_command_palette_key(KeyEvent::new(KeyCode::Up, KeyModifiers::empty()));
        }

        assert_eq!(app.state.command_palette.list.selected, 0);
        assert_eq!(app.state.command_palette.scroll, 0);
    }

    #[test]
    fn command_palette_hover_ignores_rendered_scrollbar_column() {
        let mut app = app_with_space();
        app.state.view.sidebar_rect = ratatui::layout::Rect::new(0, 0, 26, 20);
        app.state.view.terminal_area = ratatui::layout::Rect::new(26, 0, 80, 20);
        app.handle_command_palette_key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::empty()));

        let scrollbar = rendered_text_point(&app, "▐", 106, 20);
        app.handle_mouse(super::super::mouse(
            crossterm::event::MouseEventKind::Moved,
            scrollbar.0,
            scrollbar.1,
        ));

        assert_eq!(app.state.command_palette.list.selected, 0);
        assert_eq!(app.state.command_palette.list.visible(), None);
    }

    #[test]
    fn command_palette_pointer_exit_hides_highlight_and_keyboard_resumes_anchor() {
        let mut app = app_with_space();
        app.state.view.sidebar_rect = ratatui::layout::Rect::new(0, 0, 26, 20);
        app.state.view.terminal_area = ratatui::layout::Rect::new(26, 0, 80, 20);

        let (list, rows) = command_palette_viewport(&app.state).expect("palette rows");
        assert_eq!(app.state.command_palette.list.visible(), None);
        let row_idx = rows
            .iter()
            .position(|row| row.is_some_and(|index| index > 0))
            .expect("second actionable row");
        let hovered = rows[row_idx].expect("action index");
        app.handle_mouse(super::super::mouse(
            crossterm::event::MouseEventKind::Moved,
            list.rect.x.saturating_add(2),
            list.rect.y.saturating_add(row_idx as u16),
        ));

        assert_eq!(app.state.command_palette.list.selected, 0);
        assert_eq!(app.state.command_palette.list.visible(), Some(hovered));

        app.handle_mouse(super::super::mouse(
            crossterm::event::MouseEventKind::Moved,
            0,
            0,
        ));
        assert_eq!(app.state.command_palette.list.visible(), None);

        app.handle_command_palette_key(KeyEvent::new(KeyCode::Down, KeyModifiers::empty()));
        assert_eq!(app.state.command_palette.list.selected, 1);
        assert_eq!(app.state.command_palette.list.visible(), Some(1));
    }

    #[test]
    fn command_palette_commands_include_keybind_labels() {
        let app = app_with_space();

        let commands = command_palette_visible_commands(&app.state);

        assert!(commands.iter().any(|command| {
            command.title == "New Space"
                && command.key_label.as_deref()
                    == app.state.keybinds.new_workspace.label().as_deref()
        }));
    }

    #[test]
    fn command_palette_commands_include_indexed_navigation_labels() {
        let mut app = app_with_space();
        app.state.workspaces[0].test_add_tab(Some("logs"));

        let commands = command_palette_visible_commands(&app.state);

        assert!(commands.iter().any(|command| {
            command.title == "Switch to Tab: logs"
                && command.action == CommandPaletteAction::SwitchTab(1)
                && command.key_label.as_deref() == Some("prefix+2")
        }));
        assert!(commands.iter().any(|command| {
            command.title == "Switch to Space: test"
                && command.action == CommandPaletteAction::SwitchWorkspace(0)
                && command.key_label.as_deref() == Some("prefix+shift+1")
        }));
        assert!(commands.iter().any(|command| {
            matches!(command.action, CommandPaletteAction::SwitchGroup(0))
                && command.key_label.as_deref() == Some("prefix+alt+1")
        }));
    }

    #[test]
    fn command_palette_exposes_scrollback_and_configured_cycle_hints() {
        let app = app_with_space();

        let commands = command_palette_visible_commands(&app.state);
        assert!(commands.iter().any(|command| {
            command.title == "Edit Scrollback"
                && command.group == "panes"
                && command.key_label.as_deref()
                    == app.state.keybinds.edit_scrollback.label().as_deref()
        }));
        assert!(commands.iter().any(|command| {
            command.title == "Cycle Pane Next"
                && command.key_label.as_deref()
                    == app.state.keybinds.cycle_pane_next.label().as_deref()
        }));
        assert!(commands.iter().any(|command| {
            command.title == "Cycle Pane Previous"
                && command.key_label.as_deref()
                    == app.state.keybinds.cycle_pane_previous.label().as_deref()
        }));
    }

    #[test]
    fn command_palette_includes_all_project_command_launchers() {
        let app = app_with_space();

        let commands = command_palette_visible_commands(&app.state);

        for (title, action) in [
            ("Open Browser", CommandPaletteAction::OpenBrowser),
            ("Open Review", CommandPaletteAction::OpenReview),
            ("Open Editor", CommandPaletteAction::OpenEditor),
            ("Open GitHub", CommandPaletteAction::OpenGithub),
        ] {
            assert!(commands.iter().any(|command| {
                command.title == title && command.group == "project" && command.action == action
            }));
        }
    }
}
