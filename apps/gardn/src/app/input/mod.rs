//! Input handling — translates crossterm key/mouse events into state mutations.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::app::PaneClickState;
use crate::input::TerminalKey;
use ratatui::layout::Direction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollbarClickTarget {
    Thumb { grab_row_offset: u16 },
    Track { offset_from_bottom: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(test)]
enum WheelRouting {
    HostScroll,
    MouseReport,
    AlternateScroll,
}

pub(super) const WORKSPACE_DRAG_THRESHOLD: u16 = 1;
pub(super) const TAB_DRAG_THRESHOLD: u16 = 1;
pub(super) const AGENT_DRAG_THRESHOLD: u16 = 1;
pub(super) const MODAL_WHEEL_SCROLL_ROWS: i16 = 3;
pub(super) const MODAL_PAGE_SCROLL_ROWS: i16 = 8;

fn modified_url_click_modifier() -> KeyModifiers {
    KeyModifiers::CONTROL
}

pub(crate) fn rendering_client_may_open_url(url: &str, execution_host_is_local: bool) -> bool {
    if execution_host_is_local {
        return true;
    }
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("file:") {
        return false;
    }
    let Some((scheme, rest)) = lower.split_once("://") else {
        return true;
    };
    if !matches!(scheme, "http" | "https") {
        return true;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let host = if let Some(bracketed) = host_port.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or_default()
    } else {
        host_port.split(':').next().unwrap_or_default()
    };
    !(host == "localhost"
        || host.ends_with(".localhost")
        || host == "::1"
        || host == "0.0.0.0"
        || host == "127.0.0.1"
        || host.starts_with("127."))
}

pub(crate) fn next_group_execution_host(
    profiles: &[crate::persist::ssh_profiles::SshConnectionProfile],
    current: &crate::execution_host::ExecutionHostId,
) -> crate::execution_host::ExecutionHostId {
    let mut hosts = vec![crate::execution_host::ExecutionHostId::local()];
    hosts.extend(profiles.iter().map(|profile| profile.execution_host_id()));
    let next = hosts
        .iter()
        .position(|host| host == current)
        .map_or(0, |index| (index + 1) % hosts.len());
    hosts
        .get(next)
        .cloned()
        .unwrap_or_else(crate::execution_host::ExecutionHostId::local)
}

pub(crate) fn apply_group_host_cycle(
    profiles: &[crate::persist::ssh_profiles::SshConnectionProfile],
    host: &mut crate::execution_host::ExecutionHostId,
    directory: &mut String,
) {
    *host = next_group_execution_host(profiles, host);
    if !directory.trim().is_empty() || host.is_local() {
        return;
    }
    *directory = profiles
        .iter()
        .find(|profile| profile.execution_host_id() == *host)
        .and_then(|profile| profile.suggested_directory())
        .map(|path| path.as_path().display().to_string())
        .unwrap_or_else(|| ".".to_string());
}

pub(crate) fn group_default_location_for(
    profiles: &[crate::persist::ssh_profiles::SshConnectionProfile],
    host: &crate::execution_host::ExecutionHostId,
    directory: &str,
) -> Option<crate::execution_host::ResourceLocation> {
    let trimmed = directory.trim();
    let path = if trimmed.is_empty() {
        if host.is_local() {
            return None;
        }
        profiles
            .iter()
            .find(|profile| &profile.execution_host_id() == host)
            .and_then(|profile| profile.suggested_directory().cloned())
            .or_else(|| crate::execution_host::HostPath::new(".").ok())?
    } else {
        crate::execution_host::HostPath::new(trimmed).ok()?
    };
    Some(crate::execution_host::ResourceLocation::new(
        host.clone(),
        path,
    ))
}

#[cfg(test)]
#[test]
fn modified_url_click_modifier_matches_terminal_mouse_reporting() {
    assert_eq!(modified_url_click_modifier(), KeyModifiers::CONTROL);
}

#[cfg(test)]
#[test]
fn remote_execution_urls_never_become_rendering_client_local_targets() {
    for url in [
        "file:///srv/private/report.html",
        "http://localhost:3000",
        "https://api.localhost/status",
        "http://127.0.0.1:8080",
        "http://[::1]:9000",
    ] {
        assert!(!rendering_client_may_open_url(url, false), "{url}");
        assert!(rendering_client_may_open_url(url, true), "{url}");
    }
    assert!(rendering_client_may_open_url(
        "https://example.com/report",
        false
    ));
}

#[cfg(test)]
#[test]
fn group_default_location_keeps_ssh_host_without_typed_directory() {
    let profile = crate::persist::ssh_profiles::SshConnectionProfile::new(
        "workbox",
        "Work box",
        "alice@workbox",
        Some(crate::execution_host::HostPath::new("/srv/work").expect("valid path")),
    )
    .expect("valid profile");
    let host = profile.execution_host_id();
    let location = group_default_location_for(std::slice::from_ref(&profile), &host, " ")
        .expect("remote default");
    assert_eq!(location.execution_host_id, host);
    assert_eq!(location.path.as_path(), std::path::Path::new("/srv/work"));
    assert!(group_default_location_for(
        std::slice::from_ref(&profile),
        &crate::execution_host::ExecutionHostId::local(),
        " "
    )
    .is_none());
}

#[cfg(test)]
#[test]
fn group_host_cycle_fills_suggested_directory_when_empty() {
    let profile = crate::persist::ssh_profiles::SshConnectionProfile::new(
        "workbox",
        "Work box",
        "alice@workbox",
        Some(crate::execution_host::HostPath::new("/srv/work").expect("valid path")),
    )
    .expect("valid profile");
    let expected_host = profile.execution_host_id();
    let mut host = crate::execution_host::ExecutionHostId::local();
    let mut directory = String::new();
    apply_group_host_cycle(std::slice::from_ref(&profile), &mut host, &mut directory);
    assert_eq!(host, expected_host);
    assert_eq!(directory, "/srv/work");
}

pub(super) mod agent_profile_picker;
mod clipboard;
mod command_palette;
mod copy_mode;
mod lease;
mod modal;
mod mouse;
mod navigate;
mod overlays;
mod selection;
mod settings;
mod sidebar;
mod terminal;

pub(crate) use self::{
    command_palette::{
        handle_command_palette_key_for_view, handle_command_palette_mouse_for_view,
        selected_command_palette_action_for_view,
    },
    modal::{
        apply_keybind_help_key, confirm_close_accept, confirm_close_cancel,
        confirm_delete_group_accept, confirm_delete_group_cancel, context_menu_state_for_pane,
        global_menu_actions, handle_agent_menu_key, handle_config_diagnostics_key,
        handle_confirm_close_key, handle_confirm_delete_group_key, handle_context_menu_key,
        handle_global_menu_key, handle_group_menu_key, handle_keybind_help_key,
        handle_navigator_key, handle_rename_key, handle_resize_key, insert_keybind_help_query_text,
        insert_navigator_search_text, modal_action_from_buttons,
        open_new_workspace_dialog_at_location, request_detach, GlobalMenuAction,
        KeybindHelpKeyResult, ModalAction,
    },
    navigate::{
        command_for_key, indexed_navigation_action, non_indexed_action_for_key,
        terminal_direct_indexed_navigation_action, terminal_direct_non_indexed_navigation_action,
        ActionContext, BindingDispatch, NavigateAction,
    },
    settings::{
        close_agent_profile_editor_for_view, open_settings_at, paste_settings_text_for_view,
        prepare_general_settings_state, prepare_group_settings_state,
        prepare_workspace_settings_state, update_settings_mouse_for_view,
        update_settings_state_for_view, SettingsAction,
    },
    sidebar::{AgentMenuAction, GroupDropTarget, GroupMenuAction, WorkspaceDropTarget},
};

#[cfg(test)]
pub(crate) use self::command_palette::open_command_palette_for_view;
pub(crate) use self::lease::{
    ConsumedInputLease, InputLeaseKey, InputLeaseTable, InputSourceId, RepeatPlan,
    TerminalInputContext, LOCAL_INPUT_SOURCE,
};
use self::modal::{modal_action_from_key, ONBOARDING_WELCOME_ACTIONS, RELEASE_NOTES_ACTIONS};
pub(crate) use self::terminal::TerminalKeyTarget;
use super::state::{AppState, DragState, DragTarget, Mode};
use super::App;

// ---------------------------------------------------------------------------
// Key handling
// ---------------------------------------------------------------------------

impl App {
    pub(super) async fn handle_key(&mut self, key: TerminalKey) -> Option<TerminalKeyTarget> {
        if self.state.mode == Mode::Github
            || (self.state.mode == Mode::CommandPalette
                && self.default_client_view.github.is_some())
        {
            self.with_default_github_view(|app, view| app.handle_client_view_modal_key(view, key));
            return None;
        }
        if self.default_client_view.popup_pane.is_some() {
            if key.as_key_event().code == KeyCode::Esc {
                let mut view = self.default_client_view.clone_reconciled(&self.state);
                self.close_popup_pane_for_view(&mut view);
                self.default_client_view = view;
            } else {
                let _ = self.send_popup_key_for_view(&self.default_client_view, key);
            }
            return None;
        }
        let key_event = key.as_key_event();
        if modal_paste_target_active(&self.state) && is_modal_paste_shortcut(&key_event) {
            if let Some(text) = crate::platform::read_clipboard_text() {
                self.paste_into_active_text_input(&text);
            }
            return None;
        }

        if self.state.mode == Mode::Terminal {
            return self.handle_terminal_key(key).await;
        }

        match self.state.mode {
            Mode::Github => {}
            Mode::Prefix => self.handle_prefix_key(key),
            Mode::Navigate => self.handle_navigate_key(key),
            Mode::Copy => self.handle_copy_mode_key(key),
            Mode::Onboarding => self.handle_onboarding_key(key_event),
            Mode::ReleaseNotes => self.handle_release_notes_key(key_event),
            Mode::ProductAnnouncement => self.handle_product_announcement_key(key_event),
            Mode::RenameWorkspace | Mode::RenameGroup | Mode::RenameTab | Mode::RenamePane => {
                self.handle_rename_key_via_runtime(key_event)
            }
            Mode::Resize => handle_resize_key(&mut self.state, key),
            Mode::ConfirmClose => handle_confirm_close_key(&mut self.state, key_event),
            Mode::ConfirmDeleteGroup => handle_confirm_delete_group_key(&mut self.state, key_event),
            Mode::ContextMenu => {
                handle_context_menu_key(&mut self.state, &mut self.terminal_runtimes, key_event);
            }
            Mode::Settings => self.handle_settings_key(key_event),
            Mode::GlobalMenu => handle_global_menu_key(&mut self.state, key_event),
            Mode::GroupMenu => handle_group_menu_key(&mut self.state, key_event),
            Mode::AgentMenu => handle_agent_menu_key(&mut self.state, key_event),
            Mode::KeybindHelp => handle_keybind_help_key(&mut self.state, key),
            Mode::ConfigDiagnostics => handle_config_diagnostics_key(&mut self.state, key_event),
            Mode::Navigator => handle_navigator_key(&mut self.state, key_event),
            Mode::CommandPalette if key_event.code == KeyCode::Enter => {
                self.execute_selected_command_palette_command_interactive()
                    .await
            }
            Mode::CommandPalette => self.handle_command_palette_key(key_event),
            Mode::AgentProfilePicker => self.handle_agent_profile_picker_key(key_event),
            Mode::GitRepoPicker => self.handle_git_repo_picker_key(key_event),
            Mode::Terminal => unreachable!(),
        }
        None
    }

    pub(crate) fn handle_text_commit_headless(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.default_client_view.popup_pane.is_some() {
            let _ = self.send_popup_text_for_view(&self.default_client_view, text);
            return;
        }
        if self.state.mode != Mode::Terminal {
            self.paste_into_active_text_input(text);
            return;
        }

        self.state.clear_selection();
        self.selection_autoscroll_deadline = None;
        self.state.update_dismissed = true;
        if let Some(ws_idx) = self.state.active {
            if let Some(runtime) = self
                .state
                .focused_runtime_in_workspace(&self.terminal_runtimes, ws_idx)
            {
                let _ = runtime.try_send_bytes(bytes::Bytes::copy_from_slice(text.as_bytes()));
            }
        }
    }

    pub(super) async fn handle_text_commit(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        if self.default_client_view.popup_pane.is_some() {
            let _ = self.send_popup_text_for_view(&self.default_client_view, &text);
            return;
        }
        if self.state.mode != Mode::Terminal {
            self.paste_into_active_text_input(&text);
            return;
        }

        self.state.clear_selection();
        self.selection_autoscroll_deadline = None;
        self.state.update_dismissed = true;
        if let Some(ws_idx) = self.state.active {
            if let Some(runtime) = self
                .state
                .focused_runtime_in_workspace(&self.terminal_runtimes, ws_idx)
            {
                let _ = runtime.send_bytes(bytes::Bytes::from(text)).await;
            }
        }
    }

    pub(super) async fn handle_paste(&mut self, text: String) {
        if self.state.mode != Mode::Terminal {
            self.paste_into_active_text_input(&text);
            return;
        }

        if let Some(ws_idx) = self.state.active {
            if let Some(rt) = self
                .state
                .focused_runtime_in_workspace(&self.terminal_runtimes, ws_idx)
            {
                let _ = rt.send_paste(text).await;
            }
        }
    }

    pub(crate) fn paste_into_active_text_input(&mut self, text: &str) -> bool {
        if self.state.mode == Mode::Github {
            if let Some(screen) = self.default_client_view.github.as_mut() {
                screen.paste(text);
                return true;
            }
        }
        match self.state.mode {
            Mode::RenameWorkspace | Mode::RenameGroup | Mode::RenameTab | Mode::RenamePane => {
                if self.state.name_input_replace_on_type
                    && !(self.state.mode == Mode::RenameGroup
                        && self.state.creating_new_group
                        && self.state.group_modal_selected_field == 2)
                {
                    self.state.name_input.clear();
                    self.state.name_input_replace_on_type = false;
                }
                if self.state.mode == Mode::RenameGroup
                    && self.state.creating_new_group
                    && self.state.group_modal_selected_field == 2
                {
                    self.state.group_default_directory_input.push_str(text);
                } else {
                    self.state.name_input.push_str(text);
                }
                true
            }
            Mode::Navigator => {
                if !self.state.navigator.search_focused {
                    return false;
                }
                insert_navigator_search_text(&mut self.state, text);
                true
            }
            Mode::Copy => {
                let Some(prompt) = self
                    .state
                    .copy_mode
                    .as_mut()
                    .and_then(|copy_mode| copy_mode.search.prompt.as_mut())
                else {
                    return false;
                };
                prompt
                    .query
                    .extend(text.chars().filter(|ch| !ch.is_control()));
                true
            }
            Mode::KeybindHelp => insert_keybind_help_query_text(&mut self.state.keybind_help, text),
            Mode::Settings => {
                let Some(action) = settings::paste_settings_text(&mut self.state, text) else {
                    return false;
                };
                if let Some(action) = action {
                    self.apply_settings_action(action);
                }
                true
            }
            _ => false,
        }
    }

    pub(crate) fn handle_onboarding_key(&mut self, key: KeyEvent) {
        if let Some(ModalAction::Continue) = modal_action_from_key(&key, ONBOARDING_WELCOME_ACTIONS)
        {
            self.open_settings_from_onboarding();
        }
    }

    pub(crate) fn handle_release_notes_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.scroll_release_notes(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_release_notes(1),
            KeyCode::PageUp => self.scroll_release_notes(-MODAL_PAGE_SCROLL_ROWS),
            KeyCode::PageDown => self.scroll_release_notes(MODAL_PAGE_SCROLL_ROWS),
            KeyCode::Home => {
                if let Some(notes) = &mut self.state.release_notes {
                    notes.scroll = 0;
                }
            }
            KeyCode::End => {
                let max_scroll = self.state.release_notes_max_scroll();
                if let Some(notes) = &mut self.state.release_notes {
                    notes.scroll = max_scroll;
                }
            }
            _ => {
                if let Some(ModalAction::Close) = modal_action_from_key(&key, RELEASE_NOTES_ACTIONS)
                {
                    self.dismiss_release_notes();
                }
            }
        }
    }

    pub(crate) fn handle_product_announcement_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.scroll_product_announcement(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_product_announcement(1),
            KeyCode::PageUp => self.scroll_product_announcement(-8),
            KeyCode::PageDown => self.scroll_product_announcement(8),
            KeyCode::Home => {
                if let Some(announcement) = &mut self.state.product_announcement {
                    announcement.scroll = 0;
                }
            }
            KeyCode::End => {
                let max_scroll = self.state.product_announcement_max_scroll();
                if let Some(announcement) = &mut self.state.product_announcement {
                    announcement.scroll = max_scroll;
                }
            }
            _ => {
                if let Some(ModalAction::Close) = modal_action_from_key(&key, RELEASE_NOTES_ACTIONS)
                {
                    self.dismiss_product_announcement();
                }
            }
        }
    }

    pub(super) fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.state.mode == Mode::Github
            || (self.state.mode == Mode::CommandPalette
                && self.default_client_view.github.is_some())
        {
            self.with_default_github_view(|app, view| app.handle_mouse_for_view(view, mouse));
            return;
        }
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.pending_url_click = false;
            }
            MouseEventKind::Drag(MouseButton::Left) if self.pending_url_click => {
                return;
            }
            MouseEventKind::Up(MouseButton::Left) if self.pending_url_click => {
                self.pending_url_click = false;
                return;
            }
            _ => {}
        }
        if self.default_client_view.popup_pane.is_some() {
            let mut view = self.default_client_view.clone_reconciled(&self.state);
            self.handle_mouse_for_view(&mut view, mouse);
            self.default_client_view = view;
            return;
        }
        if self.handle_overlay_mouse(mouse) {
            return;
        }
        if self.handle_context_bar_mouse(mouse) {
            return;
        }

        if self.state.mode == Mode::CommandPalette {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    match command_palette::command_palette_action_button_at(
                        &self.state,
                        mouse.column,
                        mouse.row,
                    ) {
                        Some(ModalAction::Apply) => {
                            self.execute_selected_command_palette_command();
                            return;
                        }
                        Some(ModalAction::Close) => {
                            command_palette::close_command_palette(&mut self.state);
                            return;
                        }
                        _ => {}
                    }

                    if let Some(target) = command_palette::command_palette_scrollbar_target_at(
                        &self.state,
                        mouse.column,
                        mouse.row,
                    ) {
                        match target {
                            ScrollbarClickTarget::Thumb { grab_row_offset } => {
                                self.state.drag = Some(DragState {
                                    target: DragTarget::CommandPaletteScrollbar { grab_row_offset },
                                });
                            }
                            ScrollbarClickTarget::Track { offset_from_bottom } => {
                                command_palette::set_command_palette_offset_from_bottom(
                                    &mut self.state,
                                    offset_from_bottom,
                                );
                            }
                        }
                        return;
                    }

                    if command_palette::command_palette_contains_point(
                        &self.state,
                        mouse.column,
                        mouse.row,
                    ) {
                        command_palette::select_command_palette_selection(
                            &mut self.state,
                            mouse.column,
                            mouse.row,
                        );
                    } else {
                        self.state.drag = None;
                        command_palette::close_command_palette(&mut self.state);
                    }
                    return;
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    if let Some(DragState {
                        target: DragTarget::CommandPaletteScrollbar { grab_row_offset },
                    }) = &self.state.drag
                    {
                        if let Some(offset_from_bottom) =
                            command_palette::command_palette_offset_for_drag_row(
                                &self.state,
                                mouse.row,
                                *grab_row_offset,
                            )
                        {
                            command_palette::set_command_palette_offset_from_bottom(
                                &mut self.state,
                                offset_from_bottom,
                            );
                        }
                    }
                    return;
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.state.drag = None;
                    return;
                }
                MouseEventKind::ScrollDown => {
                    command_palette::scroll_command_palette_rows(
                        &mut self.state,
                        MODAL_WHEEL_SCROLL_ROWS,
                    );
                    return;
                }
                MouseEventKind::ScrollUp => {
                    command_palette::scroll_command_palette_rows(
                        &mut self.state,
                        -MODAL_WHEEL_SCROLL_ROWS,
                    );
                    return;
                }
                MouseEventKind::Moved => {
                    command_palette::hover_command_palette_selection(
                        &mut self.state,
                        mouse.column,
                        mouse.row,
                    );
                    return;
                }
                _ => {}
            }
        }

        if self.state.mode == Mode::GitRepoPicker {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(idx) = crate::ui::git_repo_picker::git_repo_picker_index_at(
                        &self.state,
                        mouse.column,
                        mouse.row,
                    ) {
                        self.state.git_repo_picker.list.select(idx);
                        self.handle_git_repo_picker_key(KeyEvent::new(
                            KeyCode::Enter,
                            KeyModifiers::empty(),
                        ));
                    } else {
                        self.state.return_to_active_workspace_mode();
                    }
                    return;
                }
                MouseEventKind::ScrollDown => {
                    let visible_repos =
                        crate::ui::git_repo_picker::git_repo_picker_list_geometry(&self.state)
                            .map(|list| (list.scroll_area.body.height as usize).div_ceil(2).max(1))
                            .unwrap_or(1);
                    let max_scroll = self
                        .state
                        .git_repo_picker
                        .roots
                        .len()
                        .saturating_sub(visible_repos);
                    self.state.git_repo_picker.scroll = self
                        .state
                        .git_repo_picker
                        .scroll
                        .saturating_add((MODAL_WHEEL_SCROLL_ROWS as usize).div_ceil(2))
                        .min(max_scroll);
                    return;
                }
                MouseEventKind::ScrollUp => {
                    self.state.git_repo_picker.scroll = self
                        .state
                        .git_repo_picker
                        .scroll
                        .saturating_sub((MODAL_WHEEL_SCROLL_ROWS as usize).div_ceil(2));
                    return;
                }
                MouseEventKind::Moved => {
                    self.state.git_repo_picker.list.hover(
                        crate::ui::git_repo_picker::git_repo_picker_index_at(
                            &self.state,
                            mouse.column,
                            mouse.row,
                        ),
                    );
                    return;
                }
                _ => {}
            }
        }

        if self.state.mode == Mode::AgentProfilePicker {
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    match agent_profile_picker::agent_profile_picker_action_button_at(
                        &self.state,
                        mouse.column,
                        mouse.row,
                    ) {
                        Some(ModalAction::Apply) => {
                            self.handle_agent_profile_picker_key(KeyEvent::new(
                                KeyCode::Enter,
                                KeyModifiers::empty(),
                            ));
                            return;
                        }
                        Some(ModalAction::Close) => {
                            agent_profile_picker::close_agent_profile_picker(&mut self.state);
                            return;
                        }
                        _ => {}
                    }

                    if agent_profile_picker::select_agent_profile_picker_tab_at(
                        &mut self.state,
                        mouse.column,
                        mouse.row,
                    ) {
                        return;
                    }

                    if let Some(target) =
                        agent_profile_picker::agent_profile_picker_scrollbar_target_at(
                            &self.state,
                            mouse.column,
                            mouse.row,
                        )
                    {
                        match target {
                            ScrollbarClickTarget::Thumb { grab_row_offset } => {
                                self.state.drag = Some(DragState {
                                    target: DragTarget::AgentProfilePickerScrollbar {
                                        grab_row_offset,
                                    },
                                });
                            }
                            ScrollbarClickTarget::Track { offset_from_bottom } => {
                                agent_profile_picker::set_agent_profile_picker_offset_from_bottom(
                                    &mut self.state,
                                    offset_from_bottom,
                                );
                            }
                        }
                        return;
                    }

                    if agent_profile_picker::agent_profile_picker_contains_point(
                        &self.state,
                        mouse.column,
                        mouse.row,
                    ) {
                        agent_profile_picker::select_agent_profile_picker_selection(
                            &mut self.state,
                            mouse.column,
                            mouse.row,
                        );
                    } else {
                        self.state.drag = None;
                        agent_profile_picker::close_agent_profile_picker(&mut self.state);
                    }
                    return;
                }
                MouseEventKind::Drag(MouseButton::Left) => {
                    if let Some(DragState {
                        target: DragTarget::AgentProfilePickerScrollbar { grab_row_offset },
                    }) = &self.state.drag
                    {
                        if let Some(offset_from_bottom) =
                            agent_profile_picker::agent_profile_picker_offset_for_drag_row(
                                &self.state,
                                mouse.row,
                                *grab_row_offset,
                            )
                        {
                            agent_profile_picker::set_agent_profile_picker_offset_from_bottom(
                                &mut self.state,
                                offset_from_bottom,
                            );
                        }
                    }
                    return;
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    self.state.drag = None;
                    return;
                }
                MouseEventKind::ScrollDown => {
                    agent_profile_picker::scroll_agent_profile_picker_rows(
                        &mut self.state,
                        MODAL_WHEEL_SCROLL_ROWS,
                    );
                    return;
                }
                MouseEventKind::ScrollUp => {
                    agent_profile_picker::scroll_agent_profile_picker_rows(
                        &mut self.state,
                        -MODAL_WHEEL_SCROLL_ROWS,
                    );
                    return;
                }
                MouseEventKind::Moved => {
                    agent_profile_picker::hover_agent_profile_picker_selection(
                        &mut self.state,
                        mouse.column,
                        mouse.row,
                    );
                    return;
                }
                _ => {}
            }
        }

        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && self.state.on_sidebar_divider(mouse.column, mouse.row)
        {
            let now = std::time::Instant::now();
            let is_double_click = self
                .last_sidebar_divider_click
                .is_some_and(|last| now.duration_since(last) <= super::SIDEBAR_DOUBLE_CLICK_WINDOW);
            self.last_sidebar_divider_click = Some(now);

            if is_double_click {
                self.state.sidebar_width = self.state.default_sidebar_width;
                self.state.sidebar_width_source =
                    crate::app::state::SidebarWidthSource::ConfigDefault;
                self.state.sidebar_width_auto = false;
                self.state.mark_session_dirty();
                self.state.drag = None;
                return;
            }
        }

        if self.handle_modified_url_click(mouse) {
            return;
        }

        let handled_pane_double_click = self.handle_pane_double_click(mouse);

        let previous_settings_section = self.state.settings.section;
        if !handled_pane_double_click {
            if let Some(action) = self.state.handle_mouse(&mut self.terminal_runtimes, mouse) {
                let screen = self.state.screen_rect();
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
                        self.save_sidebar_widths(
                            sidebar_width,
                            sidebar_min_width,
                            sidebar_max_width,
                        );
                        self.save_sidebar_arrangement(sidebar_arrangement);
                        self.save_context_bar_visibility(context_bar_visibility);
                        self.save_sidebar_initial_view(
                            sidebar_initial_state,
                            sidebar_initial_agent_scope,
                        );
                        self.save_toast_delivery(toast_delivery);
                        self.save_pane_border_agent_info(pane_border_agent_info);
                        self.save_status_indicators(status_indicators);
                        crate::ui::compute_view_with_runtime_registry(
                            &mut self.state,
                            &self.terminal_runtimes,
                            screen,
                        );
                    }
                    SettingsAction::SaveGroupAccent { group_idx, accent } => {
                        self.state.set_group_accent(group_idx, accent);
                        self.query_host_terminal_theme();
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

                    SettingsAction::SaveGroupDefaultLocation {
                        group_idx,
                        default_location,
                    } => {
                        self.state
                            .set_group_default_location(group_idx, default_location);
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
                    SettingsAction::DeleteGroup(group_idx) => {
                        modal::open_confirm_delete_group(&mut self.state, group_idx);
                    }
                    SettingsAction::SaveSwitchAsciiInputSourceInPrefix(enabled) => {
                        self.save_switch_ascii_input_source_in_prefix(enabled)
                    }
                    action @ (SettingsAction::SaveKittyGraphics(_)
                    | SettingsAction::SaveResumeAgentsOnRestore(_)
                    | SettingsAction::SaveWindowTitle(_)
                    | SettingsAction::SaveHeadlessSize { .. }
                    | SettingsAction::SaveDefaultShell(_)
                    | SettingsAction::SaveShellMode(_)
                    | SettingsAction::SaveVersionCheck(_)
                    | SettingsAction::SaveManifestCheck(_)
                    | SettingsAction::SaveToastDelay(_)
                    | SettingsAction::SaveToastGardnPosition(_)
                    | SettingsAction::SaveClipboardToastEnabled(_)
                    | SettingsAction::SaveClipboardToastPosition(_)) => {
                        self.apply_settings_action(action)
                    }
                    action @ (SettingsAction::CycleIntegrationHost
                    | SettingsAction::InstallIntegration(_)
                    | SettingsAction::UninstallIntegration(_)) => {
                        self.apply_settings_action(action)
                    }
                    action @ SettingsAction::SaveAgentProfile(_) => {
                        self.apply_settings_action(action)
                    }
                    SettingsAction::DeleteAgentProfile(profile_id) => {
                        self.delete_agent_profile(&profile_id)
                    }
                    SettingsAction::SaveSshConnectionProfile(profile) => {
                        self.save_ssh_connection_profile(profile)
                    }
                    action @ (SettingsAction::PreviewSshConnectionRetirement(_)
                    | SettingsAction::ConfirmSshConnectionRetirement { .. }
                    | SettingsAction::ConfirmLocalConnectionForget { .. }) => {
                        self.apply_settings_action(action)
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
                    action @ SettingsAction::LaunchSshWorkspace { .. } => {
                        self.apply_settings_action(action)
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
                    action @ (SettingsAction::RequestForgetRemoteTermination { .. }
                    | SettingsAction::ConfirmForgetRemoteTermination { .. }) => {
                        self.apply_settings_action(action)
                    }
                }
            }
        }
        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            && self
                .state
                .selection
                .as_ref()
                .is_none_or(crate::selection::Selection::is_in_progress)
        {
            self.selection_highlight_clear_deadline = None;
        }

        if previous_settings_section != crate::app::state::SettingsSection::Integrations
            && self.state.settings.section == crate::app::state::SettingsSection::Integrations
        {
            self.apply_integration_operation(
                crate::integration::host::HostIntegrationOperation::Inspect,
            );
        }
        self.dispatch_pending_clipboard_write();

        // Sync autoscroll deadline with state (mouse handler may have
        // set or cleared selection_autoscroll during handle_mouse).
        if self.state.selection_autoscroll.is_none() {
            self.selection_autoscroll_deadline = None;
        } else if self.selection_autoscroll_deadline.is_none() {
            self.selection_autoscroll_deadline =
                Some(std::time::Instant::now() + super::SELECTION_AUTOSCROLL_INTERVAL);
        }
    }

    fn handle_context_bar_mouse(&mut self, mouse: MouseEvent) -> bool {
        if self.state.mode != Mode::Terminal
            || !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
        {
            return false;
        }
        let Some(target) = self
            .state
            .view
            .context_bar
            .target_at(mouse.column, mouse.row)
        else {
            return false;
        };

        let Some(ws_idx) = self.state.active else {
            return true;
        };
        let navigator_target = match target {
            crate::app::state::ContextBarTarget::Group => {
                crate::app::state::NavigatorTarget::Group {
                    group_idx: self.state.active_group,
                }
            }
            crate::app::state::ContextBarTarget::Workspace => {
                crate::app::state::NavigatorTarget::Workspace { ws_idx }
            }
            crate::app::state::ContextBarTarget::Tab => {
                let Some(tab_idx) = self
                    .state
                    .workspaces
                    .get(ws_idx)
                    .map(|workspace| workspace.active_tab_index())
                else {
                    return true;
                };
                crate::app::state::NavigatorTarget::Tab { ws_idx, tab_idx }
            }
            crate::app::state::ContextBarTarget::TabControl => return true,
            crate::app::state::ContextBarTarget::Pane => {
                let Some(workspace) = self.state.workspaces.get(ws_idx) else {
                    return true;
                };
                let tab_idx = workspace.active_tab_index();
                let Some(pane_id) = workspace.focused_pane_id() else {
                    return true;
                };
                crate::app::state::NavigatorTarget::Pane {
                    ws_idx,
                    tab_idx,
                    pane_id,
                }
            }
        };
        if self.state.view.layout == crate::app::state::ViewLayout::Mobile {
            let (level, selected_target) = match navigator_target {
                crate::app::state::NavigatorTarget::Group { group_idx } => (
                    crate::app::state::MobileSwitcherLevel::Groups,
                    crate::ui::MobileSwitcherTarget::Group(group_idx),
                ),
                crate::app::state::NavigatorTarget::Workspace { ws_idx } => (
                    crate::app::state::MobileSwitcherLevel::Workspaces {
                        group_idx: self.state.active_group,
                    },
                    crate::ui::MobileSwitcherTarget::Workspace(ws_idx),
                ),
                crate::app::state::NavigatorTarget::Tab { ws_idx, tab_idx } => (
                    crate::app::state::MobileSwitcherLevel::Tabs { ws_idx },
                    crate::ui::MobileSwitcherTarget::Tab { ws_idx, tab_idx },
                ),
                crate::app::state::NavigatorTarget::Pane {
                    ws_idx,
                    tab_idx,
                    pane_id,
                } => (
                    crate::app::state::MobileSwitcherLevel::Panes { ws_idx, tab_idx },
                    crate::ui::MobileSwitcherTarget::Pane {
                        ws_idx,
                        tab_idx,
                        pane_id,
                    },
                ),
            };
            self.state.mobile_agents_expanded = false;
            self.state.mobile_switcher_level = level;
            self.state.mobile_switcher_scroll = 0;
            self.state.mobile_switcher_selected =
                crate::ui::mobile_switcher_target_index(&self.state, selected_target);
            self.state.mode = Mode::Navigate;
            crate::ui::keep_mobile_switcher_selection_visible(&mut self.state);
            return true;
        }
        self.state.open_navigator();
        let selected = self
            .state
            .navigator_rows_from(&self.terminal_runtimes)
            .iter()
            .position(|row| row.target == navigator_target)
            .unwrap_or(self.state.navigator.list.selected);
        self.state.navigator.list.select(selected);
        self.state.navigator.scroll = selected;
        true
    }

    fn handle_modified_url_click(&mut self, mouse: MouseEvent) -> bool {
        if self.state.mode != Mode::Terminal
            || !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
            || !mouse.modifiers.contains(modified_url_click_modifier())
        {
            return false;
        }

        let Some(info) = self.state.pane_at(mouse.column, mouse.row).cloned() else {
            return false;
        };
        let viewport_row = mouse.row.saturating_sub(info.inner_rect.y);
        let col = mouse.column.saturating_sub(info.inner_rect.x);
        let Some(url) =
            self.state
                .url_at_pane_cell(&self.terminal_runtimes, info.id, viewport_row, col)
        else {
            return false;
        };

        self.last_pane_click = None;
        self.pending_url_click = true;
        match self.invoke_plugin_link_handler_for_url(&url, info.id) {
            Ok(true) => return true,
            Ok(false) => {}
            Err(err) => {
                tracing::warn!(err = %err, url = %url, "failed to invoke plugin link handler");
            }
        }
        if let Err(err) = self.event_tx.try_send(crate::events::AppEvent::OpenUrl {
            pane_id: info.id,
            url: url.clone(),
        }) {
            tracing::warn!(err = %err, url = %url, "failed to queue pane URL open");
        }
        true
    }

    fn handle_pane_double_click(&mut self, mouse: MouseEvent) -> bool {
        // A pane press stops being a double-click candidate once it becomes
        // a drag or completes as a real text selection.
        match mouse.kind {
            MouseEventKind::Drag(MouseButton::Left) => {
                self.last_pane_click = None;
                return false;
            }
            MouseEventKind::Up(MouseButton::Left)
                if self
                    .state
                    .selection
                    .as_ref()
                    .is_some_and(|selection| selection.is_visible()) =>
            {
                self.last_pane_click = None;
                return false;
            }
            _ => {}
        }

        // Only terminal-pane left-clicks can start this gesture; other clicks
        // should keep their existing mouse behavior and clear stale candidates.
        let Some(click) = self.pane_click_candidate(mouse) else {
            return false;
        };

        // Require the second click to land near the first click in the same pane
        // and within the double-click window so adjacent interactions do not select a word.
        if !self.take_pane_double_click(click) {
            return false;
        }

        self.select_double_clicked_word(click)
    }

    fn pane_click_candidate(&mut self, mouse: MouseEvent) -> Option<PaneClickState> {
        if !matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
            return None;
        }

        if !mouse.modifiers.is_empty() {
            self.last_pane_click = None;
            return None;
        }

        if self.state.mode != Mode::Terminal {
            self.last_pane_click = None;
            return None;
        }

        let Some(info) = self.state.pane_at(mouse.column, mouse.row).cloned() else {
            self.last_pane_click = None;
            return None;
        };

        Some(PaneClickState {
            pane_id: info.id,
            viewport_row: mouse.row - info.inner_rect.y,
            col: mouse.column - info.inner_rect.x,
            at: std::time::Instant::now(),
        })
    }

    fn take_pane_double_click(&mut self, click: PaneClickState) -> bool {
        if !self
            .last_pane_click
            .is_some_and(|last| last.is_double_click_for(click))
        {
            self.last_pane_click = Some(click);
            return false;
        }

        self.last_pane_click = None;
        true
    }
    fn select_double_clicked_word(&mut self, click: PaneClickState) -> bool {
        let selected = self.state.select_word_at_pane_cell(
            &self.terminal_runtimes,
            click.pane_id,
            click.viewport_row,
            click.col,
        );
        if selected {
            self.selection_highlight_clear_deadline = self
                .state
                .copy_on_select
                .then(|| std::time::Instant::now() + super::PANE_COPY_HIGHLIGHT_DURATION);
        }
        selected
    }

    pub(crate) fn handle_git_repo_picker_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.state.return_to_active_workspace_mode(),
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Err(err) = self
                    .state
                    .open_selected_project_command(&mut self.terminal_runtimes)
                {
                    self.state.toast = Some(crate::app::state::ToastNotification {
                        kind: crate::app::state::ToastKind::NeedsAttention,
                        title: "Git Diff Command Failed".to_string(),
                        context: err,
                        position: None,
                        target: None,
                    });
                }
                self.state.return_to_active_workspace_mode();
            }
            KeyCode::Up => {
                self.state.git_repo_picker.list.move_prev();
                if self.state.git_repo_picker.list.selected < self.state.git_repo_picker.scroll {
                    self.state.git_repo_picker.scroll = self.state.git_repo_picker.list.selected;
                }
            }
            KeyCode::Down => {
                let count = self.state.git_repo_picker.roots.len();
                if count == 0 {
                    self.state.git_repo_picker.list.select(0);
                } else {
                    self.state.git_repo_picker.list.move_next(count);
                }
                let visible_repos = 5;
                if self.state.git_repo_picker.list.selected
                    >= self.state.git_repo_picker.scroll + visible_repos
                {
                    self.state.git_repo_picker.scroll =
                        self.state.git_repo_picker.list.selected + 1 - visible_repos;
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn is_modal_paste_shortcut(key: &KeyEvent) -> bool {
    if !matches!(key.code, KeyCode::Char('v' | 'V')) {
        return false;
    }

    #[cfg(target_os = "macos")]
    {
        key.modifiers.contains(KeyModifiers::SUPER) || key.modifiers.contains(KeyModifiers::CONTROL)
    }

    #[cfg(not(target_os = "macos"))]
    {
        key.modifiers.contains(KeyModifiers::CONTROL)
    }
}

pub(crate) fn modal_paste_target_active(state: &AppState) -> bool {
    match state.mode {
        Mode::RenameWorkspace | Mode::RenameGroup | Mode::RenameTab | Mode::RenamePane => true,
        Mode::Navigator => state.navigator.search_focused,
        Mode::KeybindHelp => state.keybind_help.search_focused,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Mouse handling
// ---------------------------------------------------------------------------

// Note: split_pane needs runtime (event_tx for PTY spawn), so it lives on App
impl AppState {
    pub(crate) fn split_pane(
        &mut self,
        terminal_runtimes: &mut crate::terminal::TerminalRuntimeRegistry,
        direction: Direction,
    ) {
        // Actual PTY spawning happens in Workspace::split_focused
        // which needs events channel — this is called from navigate_key
        // where we don't have async context, so the workspace handles it
        let (rows, cols) = self.estimate_pane_size();
        let new_rows = (rows / 2).max(4);
        let new_cols = (cols / 2).max(10);

        let follow_cwd = self
            .active
            .and_then(|i| self.workspaces.get(i))
            .and_then(|ws| {
                let tab = ws.active_tab()?;
                tab.cwd_for_pane(tab.layout.focused(), &self.terminals, terminal_runtimes)
            });
        let cwd = Some(super::creation::resolve_new_terminal_cwd(
            &self.new_terminal_cwd,
            follow_cwd,
        ));

        let previous_focus = self.current_pane_focus_target();
        let Some(ws_idx) = self.active else {
            return;
        };
        let Some(ws) = self.workspaces.get_mut(ws_idx) else {
            return;
        };
        let Ok(new_pane) = ws.split_focused(
            direction,
            new_rows,
            new_cols,
            cwd,
            self.pane_scrollback_limit_bytes,
            self.host_terminal_theme,
            crate::pane::PaneShellConfig::new(&self.default_shell, self.shell_mode),
            Vec::new(),
        ) else {
            return;
        };
        let new_id = new_pane.pane_id;
        terminal_runtimes.insert(new_pane.terminal.id.clone(), new_pane.runtime);
        self.terminals
            .insert(new_pane.terminal.id.clone(), new_pane.terminal);
        self.remove_alias_shadowed_by_new_pane(new_id);
        self.record_pane_focus_change(previous_focus, ws_idx, new_id);
        self.mark_session_dirty();
        self.mode = Mode::Terminal;
    }
}

#[cfg(test)]
fn state_with_workspaces(names: &[&str]) -> AppState {
    let mut state = AppState::test_new();
    state.workspaces = names
        .iter()
        .map(|name| crate::workspace::Workspace::test_new(name))
        .collect();
    if !state.workspaces.is_empty() {
        state.active = Some(0);
        state.selected = 0;
        state.mode = Mode::Navigate;
    }
    state
}

#[cfg(test)]
fn app_for_mouse_test() -> App {
    let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut app = App::new(
        &crate::config::Config::default(),
        true,
        None,
        api_rx,
        crate::api::EventHub::default(),
    );
    app.state.mode = Mode::Terminal;
    app.state.sidebar_arrangement = crate::config::SidebarArrangementConfig::CombinedLeft;
    app.state.update_available = None;
    app.state.latest_release_notes_available = false;
    app.state.toast_config.delay_seconds = 0;
    app.state.view.sidebar_rect = ratatui::layout::Rect::new(0, 0, 26, 20);
    app.state.view.terminal_area = ratatui::layout::Rect::new(26, 0, 80, 20);
    app
}

#[cfg(test)]
fn mouse(
    kind: crossterm::event::MouseEventKind,
    col: u16,
    row: u16,
) -> crossterm::event::MouseEvent {
    crossterm::event::MouseEvent {
        kind,
        column: col,
        row,
        modifiers: crossterm::event::KeyModifiers::empty(),
    }
}

#[cfg(test)]
fn numbered_lines_bytes(count: usize) -> Vec<u8> {
    (0..count)
        .map(|i| format!("{i:06}\r\n"))
        .collect::<String>()
        .into_bytes()
}

#[cfg(test)]
fn capture_snapshot(state: &AppState) -> crate::persist::SessionSnapshot {
    let terminal_runtimes = crate::terminal::TerminalRuntimeRegistry::new();
    crate::persist::capture(
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
    )
}

#[cfg(test)]
fn root_layout_ratio(snapshot: &crate::persist::SessionSnapshot) -> Option<f32> {
    match &snapshot.workspaces.first()?.tabs.first()?.layout {
        crate::persist::LayoutSnapshot::Split { ratio, .. } => Some(*ratio),
        crate::persist::LayoutSnapshot::Pane(_) => None,
    }
}

#[cfg(test)]
fn unique_temp_path(name: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("gardn-{name}-{}-{nanos}", std::process::id()))
}

#[cfg(test)]
fn wait_for_file(path: &std::path::Path) -> String {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while std::time::Instant::now() < deadline {
        if let Ok(content) = std::fs::read_to_string(path) {
            if !content.is_empty() {
                return content;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("timed out waiting for {}", path.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> App {
        App::new(
            &crate::config::Config::default(),
            true,
            None,
            tokio::sync::mpsc::unbounded_channel().1,
            crate::api::EventHub::default(),
        )
    }

    #[tokio::test]
    async fn paste_routes_to_rename_modal_input() {
        let mut app = test_app();
        app.state.workspaces = vec![crate::workspace::Workspace::test_new("test")];
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.mode = Mode::RenameTab;
        app.state.name_input = "2".into();
        app.state.name_input_replace_on_type = true;

        app.handle_paste("feature/logs".into()).await;

        assert_eq!(app.state.name_input, "feature/logs");
        assert!(!app.state.name_input_replace_on_type);
    }

    #[tokio::test]
    async fn paste_routes_to_keybind_help_query_only_when_searching() {
        let mut app = test_app();
        app.state.mode = Mode::KeybindHelp;
        app.handle_paste("ignored".into()).await;
        assert!(app.state.keybind_help.query.is_empty());

        app.state.keybind_help.search_focused = true;
        app.state.keybind_help.scroll = 3;
        app.handle_paste("work\nspace".into()).await;

        assert_eq!(app.state.keybind_help.query, "workspace");
        assert_eq!(app.state.keybind_help.scroll, 0);
    }

    #[tokio::test]
    async fn text_commit_slash_focuses_keybind_help_search() {
        let mut app = test_app();
        app.state.mode = Mode::KeybindHelp;

        app.handle_text_commit("/".into()).await;

        assert!(app.state.keybind_help.search_focused);
        assert!(app.state.keybind_help.query.is_empty());
    }

    #[test]
    fn modal_paste_shortcut_matches_platform_primary_v() {
        #[cfg(target_os = "macos")]
        let modifiers = KeyModifiers::SUPER;
        #[cfg(not(target_os = "macos"))]
        let modifiers = KeyModifiers::CONTROL;

        assert!(is_modal_paste_shortcut(&KeyEvent::new(
            KeyCode::Char('v'),
            modifiers
        )));
        assert!(is_modal_paste_shortcut(&KeyEvent::new(
            KeyCode::Char('V'),
            modifiers | KeyModifiers::SHIFT
        )));
        assert!(!is_modal_paste_shortcut(&KeyEvent::new(
            KeyCode::Char('v'),
            KeyModifiers::ALT
        )));
    }

    #[test]
    fn modal_paste_target_is_active_only_for_text_inputs() {
        let mut state = AppState::test_new();

        state.mode = Mode::RenameTab;
        assert!(modal_paste_target_active(&state));

        state.mode = Mode::Navigator;
        state.navigator.search_focused = false;
        assert!(!modal_paste_target_active(&state));
        state.navigator.search_focused = true;
        assert!(modal_paste_target_active(&state));

        state.mode = Mode::KeybindHelp;
        state.keybind_help.search_focused = false;
        assert!(!modal_paste_target_active(&state));
        state.keybind_help.search_focused = true;
        assert!(modal_paste_target_active(&state));

        state.mode = Mode::ConfirmClose;
        assert!(!modal_paste_target_active(&state));
    }
}
