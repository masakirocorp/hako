use std::collections::{HashMap, HashSet};

static NEXT_CLIENT_VIEW_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

use crate::app::state::{
    AgentProfilePickerState, AppState, CommandPaletteState, ContextMenuState, DragState,
    GitRepoPickerState, GroupPressState, KeybindHelpState, ModalListState, Mode, NavigatorState,
    PaneFocusTarget, ProductAnnouncementState, ReleaseNotesState, RightClickPassthroughGesture,
    SelectionAutoscroll, SettingsState, TabPressState, ViewState, WorkspacePressState,
};
use crate::layout::PaneId;
use crate::terminal::{TerminalId, TerminalRuntimeRegistry};
use ratatui::layout::{Rect, Size};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClientTabControl {
    Unavailable,
    Controlling { epoch: u64 },
    WatchingFree { epoch: u64 },
    WatchingControlled { epoch: u64 },
}

impl Default for ClientTabControl {
    fn default() -> Self {
        Self::Controlling { epoch: 0 }
    }
}

impl ClientTabControl {
    pub(crate) const fn can_mutate_tab(self) -> bool {
        matches!(self, Self::Controlling { .. })
    }

    pub(crate) const fn epoch(self) -> Option<u64> {
        match self {
            Self::Unavailable => None,
            Self::Controlling { epoch }
            | Self::WatchingFree { epoch }
            | Self::WatchingControlled { epoch } => Some(epoch),
        }
    }

    pub(crate) const fn is_watching(self) -> bool {
        matches!(
            self,
            Self::WatchingFree { .. } | Self::WatchingControlled { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ClientTabViewKey {
    pub(crate) workspace_id: String,
    pub(crate) tab_number: usize,
}

impl ClientTabViewKey {
    pub(crate) fn new(workspace_id: &str, tab_number: usize) -> Self {
        Self {
            workspace_id: workspace_id.to_string(),
            tab_number,
        }
    }
}

/// Client-scoped effect produced by app logic and applied to matching views.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ClientViewEffect {
    /// Clear one pending focus marker if it still matches exactly.
    ClearPendingFocus {
        client_view_id: u64,
        marker: crate::api::PendingFocusMarker,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CanvasOrigin {
    pub(crate) col: u16,
    pub(crate) row: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProjectedRect {
    pub(crate) source: Rect,
    pub(crate) destination: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TabCanvasViewport {
    pub(crate) canvas_size: Size,
    pub(crate) viewport: Rect,
    pub(crate) origin: CanvasOrigin,
}

impl TabCanvasViewport {
    pub(crate) fn new(canvas_size: Size, viewport: Rect, origin: CanvasOrigin) -> Self {
        let origin = CanvasOrigin {
            col: clamp_origin(origin.col, canvas_size.width, viewport.width),
            row: clamp_origin(origin.row, canvas_size.height, viewport.height),
        };
        Self {
            canvas_size,
            viewport,
            origin,
        }
    }

    pub(crate) fn source_rect(self) -> Rect {
        Rect::new(
            self.origin.col,
            self.origin.row,
            self.canvas_size
                .width
                .saturating_sub(self.origin.col)
                .min(self.viewport.width),
            self.canvas_size
                .height
                .saturating_sub(self.origin.row)
                .min(self.viewport.height),
        )
    }

    pub(crate) fn project_rect(self, canonical: Rect) -> Option<ProjectedRect> {
        let source = intersect_rect(canonical, self.source_rect())?;
        let destination = Rect::new(
            add_u16(self.viewport.x, source.x.saturating_sub(self.origin.col)),
            add_u16(self.viewport.y, source.y.saturating_sub(self.origin.row)),
            source.width,
            source.height,
        );
        Some(ProjectedRect {
            source,
            destination,
        })
    }

    pub(crate) fn destination_rect(self) -> Rect {
        let source = self.source_rect();
        Rect::new(
            self.viewport.x,
            self.viewport.y,
            source.width,
            source.height,
        )
    }

    pub(crate) fn screen_to_canvas(self, col: u16, row: u16) -> Option<(u16, u16)> {
        if !contains(self.destination_rect(), col, row) {
            return None;
        }
        Some((
            add_u16(self.origin.col, col.saturating_sub(self.viewport.x)),
            add_u16(self.origin.row, row.saturating_sub(self.viewport.y)),
        ))
    }

    pub(crate) fn canvas_to_screen(self, col: u16, row: u16) -> Option<(u16, u16)> {
        let source = self.source_rect();
        if !contains(source, col, row) {
            return None;
        }
        Some((
            add_u16(self.viewport.x, col.saturating_sub(self.origin.col)),
            add_u16(self.viewport.y, row.saturating_sub(self.origin.row)),
        ))
    }

    pub(crate) fn reveal_focused(self, current: CanvasOrigin, pane: Rect) -> CanvasOrigin {
        Self::new(
            self.canvas_size,
            self.viewport,
            CanvasOrigin {
                col: reveal_axis(
                    current.col,
                    self.canvas_size.width,
                    self.viewport.width,
                    pane.x,
                    pane.width,
                ),
                row: reveal_axis(
                    current.row,
                    self.canvas_size.height,
                    self.viewport.height,
                    pane.y,
                    pane.height,
                ),
            },
        )
        .origin
    }
}

fn add_u16(lhs: u16, rhs: u16) -> u16 {
    (u32::from(lhs) + u32::from(rhs)).min(u32::from(u16::MAX)) as u16
}

fn contains(rect: Rect, col: u16, row: u16) -> bool {
    u32::from(col) >= u32::from(rect.x)
        && u32::from(col) < u32::from(rect.x) + u32::from(rect.width)
        && u32::from(row) >= u32::from(rect.y)
        && u32::from(row) < u32::from(rect.y) + u32::from(rect.height)
}

fn intersect_rect(first: Rect, second: Rect) -> Option<Rect> {
    let left = u32::from(first.x).max(u32::from(second.x));
    let top = u32::from(first.y).max(u32::from(second.y));
    let right = (u32::from(first.x) + u32::from(first.width))
        .min(u32::from(second.x) + u32::from(second.width));
    let bottom = (u32::from(first.y) + u32::from(first.height))
        .min(u32::from(second.y) + u32::from(second.height));
    (left < right && top < bottom).then(|| {
        Rect::new(
            left as u16,
            top as u16,
            (right - left) as u16,
            (bottom - top) as u16,
        )
    })
}

fn clamp_origin(origin: u16, canvas: u16, viewport: u16) -> u16 {
    origin.min(u32::from(canvas.saturating_sub(viewport.min(canvas))) as u16)
}

fn reveal_axis(current: u16, canvas: u16, viewport: u16, pane_start: u16, pane_len: u16) -> u16 {
    let max_origin = u32::from(canvas.saturating_sub(viewport.min(canvas)));
    let pane_start = u32::from(pane_start).min(u32::from(canvas));
    let pane_end = (pane_start + u32::from(pane_len)).min(u32::from(canvas));
    if pane_start >= pane_end {
        return clamp_origin(current, canvas, viewport);
    }
    let viewport_len = u32::from(viewport.min(canvas));
    let (min_origin, max_allowed) = if pane_end - pane_start <= viewport_len {
        (pane_end.saturating_sub(viewport_len), pane_start)
    } else {
        (pane_start, pane_end.saturating_sub(viewport_len))
    };
    let min_origin = min_origin.min(max_origin);
    let max_allowed = max_allowed.min(max_origin).max(min_origin);
    u32::from(current)
        .clamp(min_origin, max_allowed)
        .min(u32::from(u16::MAX)) as u16
}

#[derive(Clone)]
struct ClientOverlayReturnState {
    tab: ClientTabViewKey,
    focused_pane: PaneId,
    zoomed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TerminalViewportOffset {
    pub(crate) offset_from_bottom: usize,
    pub(crate) max_offset_from_bottom: usize,
}

impl TerminalViewportOffset {
    fn from_metrics(metrics: crate::pane::ScrollMetrics) -> Self {
        Self {
            offset_from_bottom: metrics.offset_from_bottom,
            max_offset_from_bottom: metrics.max_offset_from_bottom,
        }
    }

    fn for_metrics(self, metrics: crate::pane::ScrollMetrics) -> usize {
        if self.offset_from_bottom == 0 {
            return 0;
        }
        self.offset_from_bottom
            .saturating_add(
                metrics
                    .max_offset_from_bottom
                    .saturating_sub(self.max_offset_from_bottom),
            )
            .min(metrics.max_offset_from_bottom)
    }
}

/// Per-normal-app-client view/navigation state.
/// looking at. Shared session structures remain in `AppState`; callers must
/// explicitly run view-sensitive work through the client's state instead of
/// implicitly reading whichever client last touched the server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ClientAuthenticationPrompt {
    pub(crate) challenge_id: u64,
    pub(crate) execution_host_id: crate::execution_host::ExecutionHostId,
    pub(crate) prompt: String,
    pub(crate) response: String,
    pub(crate) host_key_confirmation: bool,
}

#[derive(Clone)]
pub(crate) struct ClientViewState {
    id: u64,
    pub(crate) tab_control: ClientTabControl,
    pub(crate) tab_control_request: Option<u64>,
    pub(crate) tab_canvas_size: Option<(u16, u16)>,
    pub(crate) tab_canvas_origins: HashMap<ClientTabViewKey, CanvasOrigin>,
    pub(crate) tab_canvas_view: Option<TabCanvasViewport>,
    pub(crate) active_workspace: Option<usize>,
    pub(crate) selected_workspace: usize,
    active_workspace_id: Option<String>,
    selected_workspace_id: Option<String>,
    /// Ordered workspace ids from the last reconcile. Used to distinguish
    /// an explicit client focus change from a shared workspace-list remap.
    workspace_ids: Option<Vec<String>>,
    pub(crate) active_group: usize,
    pub(crate) group_filter_enabled: bool,
    pub(crate) agent_panel_scope: crate::app::state::AgentPanelScope,
    pub(crate) agent_view_override: Option<crate::api::schema::AgentViewSetParams>,
    pub(crate) workspace_scroll: usize,
    pub(crate) agent_panel_scroll: usize,
    pub(crate) tab_scroll: usize,
    pub(crate) tab_scroll_follow_active: bool,
    pub(crate) hovered_tab: Option<usize>,
    pub(crate) collapsed_sidebar_hover: Option<crate::app::state::CollapsedSidebarHover>,
    pub(crate) mobile_switcher_scroll: usize,
    pub(crate) mobile_switcher_level: crate::app::state::MobileSwitcherLevel,
    pub(crate) mobile_switcher_selected: usize,
    pub(crate) mobile_agents_expanded: bool,
    pub(crate) sidebar_width: u16,
    pub(crate) sidebar_width_source: crate::app::state::SidebarWidthSource,
    pub(crate) sidebar_collapsed: bool,
    pub(crate) right_sidebar_collapsed: bool,
    pub(crate) context_bar_visibility_override: Option<bool>,
    pub(crate) zen_mode: bool,
    pub(crate) right_sidebar_width: u16,
    pub(crate) sidebar_section_split: f32,
    pub(crate) activity_agents_expanded: bool,
    pub(crate) activity_commands_expanded: bool,
    pub(crate) activity_ports_expanded: bool,
    pub(crate) collapsed_agent_sections: Vec<String>,
    pub(crate) collapsed_command_groups: Vec<String>,
    pub(crate) collapsed_command_status_groups: Vec<String>,
    pub(crate) collapsed_workspace_groups: Vec<String>,
    pub(crate) mode: Mode,
    pub(crate) active_tabs: HashMap<String, usize>,
    pub(crate) pending_active_tabs: HashMap<String, usize>,
    /// Workspace id to focus once the shared workspace appears (deferred remote create).
    pub(crate) pending_active_workspace: Option<String>,
    /// Deferred remote split focus: apply once the preallocated pane appears.
    pub(crate) pending_focused_panes: HashMap<ClientTabViewKey, PaneId>,
    /// Deferred remote popup focus: apply once the popup record exists.
    pub(crate) pending_popup_pane: Option<PaneId>,
    pub(crate) focused_panes: HashMap<ClientTabViewKey, PaneId>,
    pub(crate) zoomed_tabs: HashSet<ClientTabViewKey>,
    /// Client-local popup presentation. The detached runtime remains shared.
    pub(crate) popup_pane: Option<PaneId>,
    overlay_return_states: HashMap<PaneId, ClientOverlayReturnState>,
    pub(crate) terminal_offsets_from_bottom: HashMap<TerminalId, TerminalViewportOffset>,
    pub(crate) input_leases: crate::app::input::InputLeaseTable,
    pub(crate) settings: SettingsState,
    pub(crate) command_palette: CommandPaletteState,
    pub(crate) navigator: NavigatorState,
    pub(crate) agent_profile_picker: AgentProfilePickerState,
    pub(crate) git_repo_picker: GitRepoPickerState,
    pub(crate) github: Option<crate::github::screen::GithubScreen>,
    pub(crate) github_workspace_id: Option<String>,
    pub(crate) github_scope_settings: Option<(
        crate::github::GithubRepositoryScope,
        Option<crate::app::state::GithubOrganization>,
    )>,
    pub(crate) context_menu: Option<ContextMenuState>,
    pub(crate) selection: Option<crate::selection::Selection>,
    pub(crate) selection_autoscroll: Option<SelectionAutoscroll>,
    pub(crate) last_pane_click: Option<crate::app::PaneClickState>,
    pub(crate) pending_url_click: bool,
    pub(crate) selection_highlight_clear_deadline: Option<std::time::Instant>,
    pub(crate) copy_mode: Option<crate::app::state::CopyModeState>,
    pub(crate) drag: Option<DragState>,
    pub(crate) workspace_press: Option<WorkspacePressState>,
    pub(crate) group_press: Option<GroupPressState>,
    pub(crate) tab_press: Option<TabPressState>,
    pub(crate) agent_press: Option<crate::app::state::AgentPressState>,
    pub(crate) previous_pane_focus: Option<PaneFocusTarget>,
    pub(crate) right_click_passthrough: Option<RightClickPassthroughGesture>,
    pub(crate) keybind_help: KeybindHelpState,
    pub(crate) config_diagnostics_scroll: u16,
    pub(crate) global_menu: ModalListState,
    pub(crate) group_menu: ModalListState,
    pub(crate) agent_menu: ModalListState,
    pub(crate) creating_new_tab: bool,
    pub(crate) creating_new_group: bool,
    pub(crate) group_icon_input: String,
    pub(crate) group_default_directory_input: String,
    pub(crate) group_default_execution_host_id: crate::execution_host::ExecutionHostId,
    pub(crate) group_modal_selected_field: usize,
    pub(crate) group_icon_picker_open: bool,
    pub(crate) rename_group_target: Option<usize>,
    pub(crate) requested_new_tab_name: Option<String>,
    pub(crate) pending_workspace_create_location: Option<crate::execution_host::ResourceLocation>,
    pub(crate) pending_workspace_create_group: Option<usize>,
    pub(crate) rename_pane_target: Option<PaneId>,
    pub(crate) confirm_delete_group: Option<usize>,
    pub(crate) name_input: String,
    pub(crate) name_input_replace_on_type: bool,
    pub(crate) release_notes: Option<ReleaseNotesState>,
    pub(crate) product_announcement: Option<ProductAnnouncementState>,
    pub(crate) computed: ViewState,
    /// Runtime-only SSH authentication prompt owned by this client view.
    pub(crate) authentication_prompt: Option<ClientAuthenticationPrompt>,
}

impl ClientViewState {
    pub(crate) fn from_default_client_state(state: &AppState) -> Self {
        let mut view = Self {
            id: NEXT_CLIENT_VIEW_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            active_workspace: state.active,
            tab_control: ClientTabControl::default(),
            tab_control_request: None,
            tab_canvas_size: None,
            tab_canvas_origins: HashMap::new(),
            tab_canvas_view: None,
            selected_workspace: state.selected,
            active_workspace_id: state
                .active
                .and_then(|idx| state.workspaces.get(idx))
                .map(|workspace| workspace.id.clone()),
            selected_workspace_id: state
                .workspaces
                .get(state.selected)
                .map(|workspace| workspace.id.clone()),
            workspace_ids: Some(
                state
                    .workspaces
                    .iter()
                    .map(|workspace| workspace.id.clone())
                    .collect(),
            ),
            active_group: state.active_group,
            group_filter_enabled: state.group_filter_enabled,
            agent_panel_scope: state.agent_panel_scope,
            agent_view_override: None,
            workspace_scroll: state.workspace_scroll,
            agent_panel_scroll: state.agent_panel_scroll,
            tab_scroll: state.tab_scroll,
            tab_scroll_follow_active: state.tab_scroll_follow_active,
            hovered_tab: state.hovered_tab,
            collapsed_sidebar_hover: state.collapsed_sidebar_hover.clone(),
            mobile_switcher_scroll: state.mobile_switcher_scroll,
            mobile_switcher_level: state.mobile_switcher_level,
            mobile_switcher_selected: state.mobile_switcher_selected,
            mobile_agents_expanded: state.mobile_agents_expanded,
            sidebar_width: state.sidebar_width,
            sidebar_width_source: state.sidebar_width_source,
            sidebar_collapsed: state.sidebar_collapsed,
            right_sidebar_collapsed: state.right_sidebar_collapsed,
            context_bar_visibility_override: None,
            zen_mode: false,
            right_sidebar_width: state.right_sidebar_width,
            sidebar_section_split: state.sidebar_section_split,
            activity_agents_expanded: state.activity_agents_expanded,
            activity_commands_expanded: state.activity_commands_expanded,
            activity_ports_expanded: state.activity_ports_expanded,
            collapsed_agent_sections: state.collapsed_agent_sections.clone(),
            collapsed_command_groups: state.collapsed_command_groups.clone(),
            collapsed_command_status_groups: state.collapsed_command_status_groups.clone(),
            collapsed_workspace_groups: state.collapsed_workspace_groups.clone(),
            mode: state.mode,
            active_tabs: HashMap::new(),
            pending_active_tabs: HashMap::new(),
            pending_active_workspace: None,
            pending_focused_panes: HashMap::new(),
            pending_popup_pane: None,
            popup_pane: None,
            focused_panes: HashMap::new(),
            zoomed_tabs: HashSet::new(),
            overlay_return_states: HashMap::new(),
            terminal_offsets_from_bottom: HashMap::new(),
            settings: state.settings.clone(),
            command_palette: state.command_palette.clone(),
            navigator: state.navigator.clone(),
            agent_profile_picker: state.agent_profile_picker.clone(),
            git_repo_picker: state.git_repo_picker.clone(),
            github: None,
            github_workspace_id: None,
            github_scope_settings: None,
            context_menu: state.context_menu.clone(),
            selection: state.selection.clone(),
            selection_autoscroll: state.selection_autoscroll.clone(),
            last_pane_click: None,
            pending_url_click: false,
            selection_highlight_clear_deadline: None,
            copy_mode: state.copy_mode.clone(),
            drag: state.drag.clone(),
            workspace_press: state.workspace_press.clone(),
            group_press: state.group_press.clone(),
            tab_press: state.tab_press.clone(),
            agent_press: None,
            previous_pane_focus: state.previous_pane_focus.clone(),
            right_click_passthrough: state.right_click_passthrough.clone(),
            keybind_help: state.keybind_help.clone(),
            config_diagnostics_scroll: state.config_diagnostics_scroll,
            global_menu: state.global_menu,
            group_menu: state.group_menu,
            agent_menu: state.agent_menu,
            creating_new_tab: state.creating_new_tab,
            creating_new_group: state.creating_new_group,
            group_icon_input: state.group_icon_input.clone(),
            group_default_directory_input: state.group_default_directory_input.clone(),
            group_default_execution_host_id: state.group_default_execution_host_id.clone(),
            group_modal_selected_field: state.group_modal_selected_field,
            group_icon_picker_open: state.group_icon_picker_open,
            rename_group_target: state.rename_group_target,
            requested_new_tab_name: state.requested_new_tab_name.clone(),
            pending_workspace_create_location: state.pending_workspace_create_location.clone(),
            pending_workspace_create_group: None,
            rename_pane_target: state.rename_pane_target,
            confirm_delete_group: state.confirm_delete_group,
            name_input: state.name_input.clone(),
            name_input_replace_on_type: state.name_input_replace_on_type,
            release_notes: state.release_notes.clone(),
            product_announcement: state.product_announcement.clone(),
            authentication_prompt: None,
            computed: state.view.clone(),
            input_leases: crate::app::input::InputLeaseTable::default(),
        };
        view.reconcile(state);
        view
    }

    pub(crate) fn clone_reconciled(&self, state: &AppState) -> Self {
        let mut view = self.clone();
        view.reconcile(state);
        view
    }

    pub(crate) fn can_mutate_tab(&self) -> bool {
        self.tab_control.can_mutate_tab()
    }

    pub(crate) fn request_tab_control(&mut self) -> Option<u64> {
        if self.tab_control.is_watching() && self.tab_control_request.is_none() {
            self.tab_control_request = self.tab_control.epoch();
        }
        self.tab_control_request
    }

    pub(crate) fn take_tab_control_request(&mut self) -> Option<u64> {
        self.tab_control_request.take()
    }

    pub(crate) fn set_tab_control(&mut self, tab_control: ClientTabControl) {
        let lost_control = self.tab_control.can_mutate_tab() && !tab_control.can_mutate_tab();
        self.tab_control = tab_control;
        self.tab_control_request = None;
        if matches!(tab_control, ClientTabControl::Unavailable) {
            self.tab_canvas_size = None;
            self.tab_canvas_view = None;
        }
        if lost_control {
            self.input_leases.clear();
        }
    }

    pub(crate) fn tab_canvas_origin(&self, key: &ClientTabViewKey) -> CanvasOrigin {
        self.tab_canvas_origins
            .get(key)
            .copied()
            .unwrap_or_default()
    }

    pub(crate) fn set_tab_canvas_view(
        &mut self,
        key: ClientTabViewKey,
        viewport: TabCanvasViewport,
    ) {
        self.tab_canvas_origins.insert(key, viewport.origin);
        self.tab_canvas_view = Some(viewport);
    }
    pub(crate) fn for_new_client(state: &AppState) -> Self {
        let mut view = Self::from_default_client_state(state);
        let sidebar_collapsed = matches!(
            state.sidebar_config.initial_state,
            crate::config::SidebarInitialStateConfig::Collapsed
        );
        view.group_filter_enabled = false;
        view.agent_panel_scope =
            super::agent_panel_scope_from_config(state.sidebar_config.initial_agent_scope);
        view.sidebar_collapsed = sidebar_collapsed;
        view.right_sidebar_collapsed = sidebar_collapsed;
        view.agent_panel_scroll = 0;
        view.reconcile(state);
        view
    }

    pub(crate) fn reconcile(&mut self, state: &AppState) {
        if let Some(pane_id) = self.pending_popup_pane {
            if let Some(popup) = state.popup_panes.get(&pane_id) {
                if popup.owner.is_none_or(|owner| owner == self.id) {
                    self.popup_pane = Some(pane_id);
                    self.mode = Mode::Terminal;
                    self.pending_popup_pane = None;
                }
            }
        }
        if let Some(pane_id) = self.popup_pane {
            let visible = state
                .popup_panes
                .get(&pane_id)
                .is_some_and(|popup| popup.owner.is_none_or(|owner| owner == self.id));
            if !visible {
                self.popup_pane = None;
                if self.mode == Mode::Terminal {
                    self.mode = if self.active_workspace.is_some() {
                        Mode::Terminal
                    } else {
                        Mode::Navigate
                    };
                }
            }
        }
        // Connection editor drafts (including install/forget substate) remain owned by
        // this client view. Shared host status is reconciled separately.

        if state.groups.is_empty() {
            self.active_group = 0;
            self.group_filter_enabled = false;
        } else {
            self.active_group = self.active_group.min(state.groups.len() - 1);
        }

        if state.workspaces.is_empty() {
            self.active_workspace = None;
            self.selected_workspace = 0;
            self.active_workspace_id = None;
            self.selected_workspace_id = None;
            self.workspace_ids = None;
            self.active_tabs.clear();
            self.pending_active_tabs.clear();
            // Keep pending_active_workspace / pending_focused_panes / pending_popup_pane:
            // deferred remote creates may still complete.
            self.pending_focused_panes.clear();
            self.focused_panes.clear();
            self.zoomed_tabs.clear();
            self.overlay_return_states.clear();
            self.terminal_offsets_from_bottom.clear();
            self.set_tab_control(ClientTabControl::Unavailable);
            self.tab_canvas_origins.clear();
            self.tab_canvas_view = None;
            return;
        }

        let active_group = self.active_group;
        let group_filter_enabled = self.group_filter_enabled;
        let visible_workspace = |idx: usize| {
            if !group_filter_enabled {
                return state.workspaces.get(idx).is_some();
            }

            let active_group_id = state
                .groups
                .get(active_group)
                .map(|group| group.id.as_str())
                .unwrap_or(crate::workspace::DEFAULT_GROUP_ID);
            state
                .workspaces
                .get(idx)
                .is_some_and(|workspace| workspace.group_id == active_group_id)
        };
        let first_visible_workspace = || {
            state
                .workspaces
                .iter()
                .enumerate()
                .find_map(|(idx, _)| visible_workspace(idx).then_some(idx))
        };

        let mut applied_pending_workspace = false;
        if let Some(pending_workspace_id) = self.pending_active_workspace.clone() {
            if let Some(ws_idx) = state
                .workspaces
                .iter()
                .position(|workspace| workspace.id == pending_workspace_id)
            {
                self.active_workspace = Some(ws_idx);
                self.selected_workspace = ws_idx;
                if let Some(group_idx) = state
                    .groups
                    .iter()
                    .position(|group| group.id == state.workspaces[ws_idx].group_id)
                {
                    self.active_group = group_idx;
                }
                self.mode = Mode::Terminal;
                self.pending_active_workspace = None;
                applied_pending_workspace = true;
            }
        }

        let workspace_list_unchanged = self.workspace_ids.as_ref().is_some_and(|previous| {
            previous.len() == state.workspaces.len()
                && previous
                    .iter()
                    .zip(state.workspaces.iter())
                    .all(|(id, workspace)| id == &workspace.id)
        });
        if !workspace_list_unchanged && !applied_pending_workspace {
            if let Some(id) = self.active_workspace_id.clone() {
                if let Some(idx) = state
                    .workspaces
                    .iter()
                    .position(|workspace| workspace.id == id)
                {
                    if visible_workspace(idx) {
                        self.active_workspace = Some(idx);
                    }
                }
            }
            if let Some(id) = self.selected_workspace_id.clone() {
                if let Some(idx) = state
                    .workspaces
                    .iter()
                    .position(|workspace| workspace.id == id)
                {
                    if visible_workspace(idx) {
                        self.selected_workspace = idx;
                    }
                }
            }
        }

        if !self
            .active_workspace
            .is_some_and(|idx| idx < state.workspaces.len() && visible_workspace(idx))
        {
            // Do not steal focus while a deferred remote workspace create is still pending.
            if self.pending_active_workspace.is_none() {
                self.active_workspace = if self.group_filter_enabled {
                    first_visible_workspace()
                } else {
                    state
                        .active
                        .filter(|idx| *idx < state.workspaces.len())
                        .or_else(first_visible_workspace)
                };
            }
        }
        if self.selected_workspace >= state.workspaces.len()
            || !visible_workspace(self.selected_workspace)
        {
            self.selected_workspace = self
                .active_workspace
                .or_else(first_visible_workspace)
                .unwrap_or(0);
        }
        self.active_workspace_id = self
            .active_workspace
            .and_then(|idx| state.workspaces.get(idx))
            .map(|workspace| workspace.id.clone());
        self.selected_workspace_id = state
            .workspaces
            .get(self.selected_workspace)
            .map(|workspace| workspace.id.clone());
        self.workspace_ids = Some(
            state
                .workspaces
                .iter()
                .map(|workspace| workspace.id.clone())
                .collect(),
        );

        let valid_workspace_ids: HashSet<&str> = state
            .workspaces
            .iter()
            .map(|workspace| workspace.id.as_str())
            .collect();
        let live_tab_keys: HashSet<ClientTabViewKey> = state
            .workspaces
            .iter()
            .flat_map(|workspace| {
                workspace
                    .tabs
                    .iter()
                    .map(|tab| ClientTabViewKey::new(&workspace.id, tab.number))
            })
            .collect();
        self.tab_canvas_origins
            .retain(|key, _| live_tab_keys.contains(key));
        self.active_tabs
            .retain(|workspace_id, _| valid_workspace_ids.contains(workspace_id.as_str()));
        self.pending_active_tabs
            .retain(|workspace_id, _| valid_workspace_ids.contains(workspace_id.as_str()));
        self.focused_panes.retain(|key, pane_id| {
            valid_workspace_ids.contains(key.workspace_id.as_str())
                && state
                    .client_overlay_owners
                    .get(pane_id)
                    .is_none_or(|owner| *owner == self.id)
        });
        self.zoomed_tabs
            .retain(|key| valid_workspace_ids.contains(key.workspace_id.as_str()));

        loop {
            let missing_overlay = self
                .overlay_return_states
                .keys()
                .copied()
                .find(|overlay_pane| {
                    !state.workspaces.iter().any(|workspace| {
                        workspace
                            .tabs
                            .iter()
                            .any(|tab| tab.panes.contains_key(overlay_pane))
                    })
                });
            let Some(overlay_pane) = missing_overlay else {
                break;
            };
            let Some(return_state) = self.overlay_return_states.remove(&overlay_pane) else {
                continue;
            };

            let mut promoted = false;
            for child_return in self.overlay_return_states.values_mut() {
                if child_return.focused_pane == overlay_pane {
                    child_return.focused_pane = return_state.focused_pane;
                    child_return.zoomed = return_state.zoomed;
                    promoted = true;
                }
            }
            if promoted {
                continue;
            }

            let Some((ws_idx, workspace)) = state
                .workspaces
                .iter()
                .enumerate()
                .find(|(_, workspace)| workspace.id == return_state.tab.workspace_id)
            else {
                continue;
            };
            let Some((tab_idx, tab)) = workspace
                .tabs
                .iter()
                .enumerate()
                .find(|(_, tab)| tab.number == return_state.tab.tab_number)
            else {
                continue;
            };
            if !tab.panes.contains_key(&return_state.focused_pane) {
                continue;
            }

            self.active_workspace = Some(ws_idx);
            self.selected_workspace = ws_idx;
            self.active_tabs.insert(workspace.id.clone(), tab_idx);
            self.focused_panes
                .insert(return_state.tab.clone(), return_state.focused_pane);
            if return_state.zoomed {
                self.zoomed_tabs.insert(return_state.tab.clone());
            } else {
                self.zoomed_tabs.remove(&return_state.tab);
            }
        }
        self.overlay_return_states.retain(|_, return_state| {
            let Some(workspace) = state
                .workspaces
                .iter()
                .find(|workspace| workspace.id == return_state.tab.workspace_id)
            else {
                return false;
            };
            workspace
                .tabs
                .iter()
                .find(|tab| tab.number == return_state.tab.tab_number)
                .is_some_and(|tab| tab.panes.contains_key(&return_state.focused_pane))
        });

        for workspace in &state.workspaces {
            if workspace.tabs.is_empty() {
                self.active_tabs.remove(&workspace.id);
                self.pending_active_tabs.remove(&workspace.id);
                self.focused_panes
                    .retain(|key, _| key.workspace_id != workspace.id);
                self.pending_focused_panes
                    .retain(|key, _| key.workspace_id != workspace.id);
                self.zoomed_tabs
                    .retain(|key| key.workspace_id != workspace.id);
                continue;
            }

            let pending_active_tab = self.pending_active_tabs.get(&workspace.id).copied();
            let active_tab = if let Some(tab_idx) = pending_active_tab {
                if tab_idx < workspace.tabs.len() {
                    self.pending_active_tabs.remove(&workspace.id);
                    tab_idx
                } else {
                    self.active_tabs
                        .get(&workspace.id)
                        .copied()
                        .filter(|idx| *idx < workspace.tabs.len())
                        .unwrap_or_else(|| workspace.active_tab.min(workspace.tabs.len() - 1))
                }
            } else {
                self.active_tabs
                    .get(&workspace.id)
                    .copied()
                    .filter(|idx| *idx < workspace.tabs.len())
                    .unwrap_or_else(|| workspace.active_tab.min(workspace.tabs.len() - 1))
            };
            self.active_tabs.insert(workspace.id.clone(), active_tab);
            for tab in &workspace.tabs {
                let tab_number = tab.number;
                let tab_key = ClientTabViewKey::new(&workspace.id, tab_number);
                if let Some(pending_pane) = self.pending_focused_panes.get(&tab_key).copied() {
                    if tab.panes.contains_key(&pending_pane) {
                        self.focused_panes.insert(tab_key.clone(), pending_pane);
                        self.pending_focused_panes.remove(&tab_key);
                    }
                }
                if !tab.panes.contains_key(
                    self.focused_panes
                        .get(&tab_key)
                        .unwrap_or(&tab.layout.focused()),
                ) {
                    self.focused_panes
                        .insert(tab_key.clone(), tab.layout.focused());
                } else {
                    self.focused_panes
                        .entry(tab_key.clone())
                        .or_insert_with(|| tab.layout.focused());
                }

                if tab.zoomed {
                    self.zoomed_tabs.insert(tab_key);
                }
            }

            self.focused_panes.retain(|key, _| {
                key.workspace_id != workspace.id
                    || workspace
                        .tabs
                        .iter()
                        .any(|tab| tab.number == key.tab_number)
            });
            self.pending_focused_panes.retain(|key, _| {
                key.workspace_id != workspace.id
                    || workspace
                        .tabs
                        .iter()
                        .any(|tab| tab.number == key.tab_number)
            });
            self.zoomed_tabs.retain(|key| {
                key.workspace_id != workspace.id
                    || workspace
                        .tabs
                        .iter()
                        .any(|tab| tab.number == key.tab_number)
            });
        }
        if self.current_tab_key(state).is_none() {
            self.set_tab_control(ClientTabControl::Unavailable);
        }
    }

    pub(crate) fn active_tab_for_workspace(&self, workspace_id: &str) -> Option<usize> {
        self.active_tabs.get(workspace_id).copied()
    }

    pub(crate) fn focused_pane_for_tab(
        &self,
        workspace_id: &str,
        tab_number: usize,
    ) -> Option<PaneId> {
        self.focused_panes
            .get(&ClientTabViewKey::new(workspace_id, tab_number))
            .copied()
    }

    pub(crate) fn active_tab_index_for_workspace(
        &self,
        state: &AppState,
        ws_idx: usize,
    ) -> Option<usize> {
        let workspace = state.workspaces.get(ws_idx)?;
        self.active_tab_for_workspace(&workspace.id)
            .filter(|idx| *idx < workspace.tabs.len())
    }

    pub(crate) fn current_tab_key(&self, state: &AppState) -> Option<ClientTabViewKey> {
        let ws_idx = self.active_workspace?;
        let workspace = state.workspaces.get(ws_idx)?;
        let tab_idx = self.active_tab_index_for_workspace(state, ws_idx)?;
        let tab = workspace.tabs.get(tab_idx)?;
        Some(ClientTabViewKey::new(&workspace.id, tab.number))
    }

    pub(crate) fn focused_pane_for_workspace(
        &self,
        state: &AppState,
        ws_idx: usize,
    ) -> Option<(usize, PaneId)> {
        let workspace = state.workspaces.get(ws_idx)?;
        let tab_idx = self.active_tab_index_for_workspace(state, ws_idx)?;
        let tab = workspace.tabs.get(tab_idx)?;
        let pane_id = self.focused_pane_for_tab(&workspace.id, tab.number)?;
        workspace
            .tabs
            .get(tab_idx)?
            .panes
            .contains_key(&pane_id)
            .then_some((tab_idx, pane_id))
    }

    pub(crate) fn current_pane_focus_target(&self, state: &AppState) -> Option<PaneFocusTarget> {
        let ws_idx = self.active_workspace?;
        let workspace = state.workspaces.get(ws_idx)?;
        let (_, pane_id) = self.focused_pane_for_workspace(state, ws_idx)?;
        Some(PaneFocusTarget {
            workspace_id: workspace.id.clone(),
            pane_id,
        })
    }

    pub(crate) fn focus_pane_in_workspace(
        &mut self,
        state: &AppState,
        ws_idx: usize,
        tab_idx: usize,
        pane_id: PaneId,
    ) -> bool {
        let Some(workspace) = state.workspaces.get(ws_idx) else {
            return false;
        };
        let Some(tab) = workspace.tabs.get(tab_idx) else {
            return false;
        };
        if !tab.panes.contains_key(&pane_id) {
            return false;
        }
        if state
            .client_overlay_owners
            .get(&pane_id)
            .is_some_and(|owner| *owner != self.id)
        {
            return false;
        }

        let previous = self.current_pane_focus_target(state);
        let target = PaneFocusTarget {
            workspace_id: workspace.id.clone(),
            pane_id,
        };
        if previous.as_ref() == Some(&target) {
            return false;
        }

        let workspace_changed = self.active_workspace != Some(ws_idx);
        self.active_workspace = Some(ws_idx);
        self.selected_workspace = ws_idx;
        if let Some(group_idx) = state
            .groups
            .iter()
            .position(|group| group.id == workspace.group_id)
        {
            self.active_group = group_idx;
        }
        self.active_tabs.insert(workspace.id.clone(), tab_idx);
        self.focused_panes
            .insert(ClientTabViewKey::new(&workspace.id, tab.number), pane_id);
        if self.mode != Mode::Navigate {
            self.mode = Mode::Terminal;
        }
        self.selection = None;
        self.selection_autoscroll = None;
        self.tab_scroll_follow_active = true;
        if workspace_changed
            && matches!(
                self.agent_panel_scope,
                crate::app::state::AgentPanelScope::CurrentWorkspace
            )
        {
            self.agent_panel_scroll = 0;
        }
        self.previous_pane_focus = previous;
        true
    }

    /// Stamp requester-local deferred focus for a remote split that has not committed yet.
    pub(crate) fn mark_pending_remote_split_focus(
        &mut self,
        state: &AppState,
        ws_idx: usize,
        tab_idx: usize,
        pane_id: PaneId,
    ) {
        let Some(workspace) = state.workspaces.get(ws_idx) else {
            return;
        };
        let Some(tab) = workspace.tabs.get(tab_idx) else {
            return;
        };
        self.active_workspace = Some(ws_idx);
        self.selected_workspace = ws_idx;
        if let Some(group_idx) = state
            .groups
            .iter()
            .position(|group| group.id == workspace.group_id)
        {
            self.active_group = group_idx;
        }
        self.active_tabs.insert(workspace.id.clone(), tab_idx);
        self.pending_focused_panes
            .insert(ClientTabViewKey::new(&workspace.id, tab.number), pane_id);
        self.mode = Mode::Terminal;
    }

    /// Clear one deferred focus marker only when it still matches exactly.
    /// Newer/replacement markers and other clients' state are left untouched.
    pub(crate) fn clear_pending_focus_marker_if_matches(
        &mut self,
        marker: &crate::api::PendingFocusMarker,
    ) -> bool {
        match marker {
            crate::api::PendingFocusMarker::Workspace { workspace_id } => {
                if self.pending_active_workspace.as_deref() == Some(workspace_id.as_str()) {
                    self.pending_active_workspace = None;
                    return true;
                }
            }
            crate::api::PendingFocusMarker::Tab {
                workspace_id,
                tab_idx,
            } => {
                if self.pending_active_tabs.get(workspace_id) == Some(tab_idx) {
                    self.pending_active_tabs.remove(workspace_id);
                    return true;
                }
            }
            crate::api::PendingFocusMarker::Pane {
                workspace_id,
                tab_number,
                pane_id,
            } => {
                let key = ClientTabViewKey::new(workspace_id, *tab_number);
                if self.pending_focused_panes.get(&key) == Some(pane_id) {
                    self.pending_focused_panes.remove(&key);
                    return true;
                }
            }
        }
        false
    }

    /// Apply a client-scoped effect when this view is the target.
    pub(crate) fn apply_client_view_effect(&mut self, effect: &ClientViewEffect) -> bool {
        match effect {
            ClientViewEffect::ClearPendingFocus {
                client_view_id,
                marker,
            } => {
                if self.id != *client_view_id {
                    return false;
                }
                self.clear_pending_focus_marker_if_matches(marker)
            }
        }
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// Build a temporary encode/projection clone that reports as `client_view_id`.
    /// Used when finishing deferred remote creates for a non-default requester.
    pub(crate) fn clone_for_encode_as(&self, client_view_id: u64) -> Self {
        let mut view = self.clone();
        view.id = client_view_id;
        view
    }

    pub(crate) fn focus_client_overlay(
        &mut self,
        state: &AppState,
        ws_idx: usize,
        tab_idx: usize,
        overlay_pane: PaneId,
    ) -> bool {
        let Some(workspace) = state.workspaces.get(ws_idx) else {
            return false;
        };
        let Some(tab) = workspace.tabs.get(tab_idx) else {
            return false;
        };
        if !tab.panes.contains_key(&overlay_pane) {
            return false;
        }

        let tab_key = ClientTabViewKey::new(&workspace.id, tab.number);
        let focused_pane = self
            .focused_panes
            .get(&tab_key)
            .copied()
            .unwrap_or_else(|| tab.layout.focused());
        if !tab.panes.contains_key(&focused_pane) {
            return false;
        }
        self.overlay_return_states.insert(
            overlay_pane,
            ClientOverlayReturnState {
                tab: tab_key.clone(),
                focused_pane,
                zoomed: self.zoomed_tabs.contains(&tab_key),
            },
        );
        self.focus_pane_in_workspace(state, ws_idx, tab_idx, overlay_pane);
        self.zoomed_tabs.insert(tab_key);
        true
    }

    pub(crate) fn tab_is_zoomed(&self, workspace_id: &str, tab_number: usize) -> bool {
        self.zoomed_tabs
            .contains(&ClientTabViewKey::new(workspace_id, tab_number))
    }

    pub(crate) fn set_tab_zoomed(&mut self, workspace_id: &str, tab_number: usize, zoomed: bool) {
        let key = ClientTabViewKey::new(workspace_id, tab_number);
        if zoomed {
            self.zoomed_tabs.insert(key);
        } else {
            self.zoomed_tabs.remove(&key);
        }
    }

    pub(crate) fn screen_rect(&self) -> ratatui::layout::Rect {
        let sidebar = self.computed.sidebar_rect;
        let right_sidebar = self.computed.right_sidebar_rect;
        let terminal = self.computed.terminal_area;
        let mobile_header = self.computed.mobile_header_rect;
        let context_bar = self.computed.context_bar.rect;
        let x = sidebar
            .x
            .min(right_sidebar.x)
            .min(terminal.x)
            .min(mobile_header.x)
            .min(context_bar.x);
        let y = sidebar
            .y
            .min(right_sidebar.y)
            .min(terminal.y)
            .min(mobile_header.y)
            .min(context_bar.y);
        let right = (sidebar.x + sidebar.width)
            .max(right_sidebar.x + right_sidebar.width)
            .max(terminal.x + terminal.width)
            .max(mobile_header.x + mobile_header.width)
            .max(context_bar.x + context_bar.width);
        let bottom = (sidebar.y + sidebar.height)
            .max(right_sidebar.y + right_sidebar.height)
            .max(terminal.y + terminal.height)
            .max(mobile_header.y + mobile_header.height)
            .max(context_bar.y + context_bar.height);
        ratatui::layout::Rect::new(x, y, right.saturating_sub(x), bottom.saturating_sub(y))
    }

    pub(crate) fn clear_due_selection_highlight(&mut self, now: std::time::Instant) -> bool {
        if self
            .selection_highlight_clear_deadline
            .is_none_or(|deadline| now < deadline)
        {
            return false;
        }

        self.selection_highlight_clear_deadline = None;
        if self
            .selection
            .as_ref()
            .is_some_and(|selection| !selection.is_in_progress())
        {
            self.selection = None;
            self.selection_autoscroll = None;
            return true;
        }
        false
    }

    pub(crate) fn return_to_active_workspace_mode(&mut self) {
        if self.github.is_some() {
            self.mode = Mode::Github;
            return;
        }
        self.mode = if self.active_workspace.is_some() {
            Mode::Terminal
        } else {
            Mode::Navigate
        };
    }
}

#[cfg(test)]
pub(crate) fn capture_terminal_offset_from_runtimes(
    terminal_id: &TerminalId,
    runtimes: &TerminalRuntimeRegistry,
    view: &mut ClientViewState,
) {
    let Some(metrics) = runtimes
        .get(terminal_id)
        .and_then(|runtime| runtime.scroll_metrics())
    else {
        return;
    };
    view.terminal_offsets_from_bottom.insert(
        terminal_id.clone(),
        TerminalViewportOffset::from_metrics(metrics),
    );
}

pub(crate) fn set_terminal_offset_from_bottom(
    terminal_id: &TerminalId,
    metrics: crate::pane::ScrollMetrics,
    offset_from_bottom: usize,
    view: &mut ClientViewState,
) {
    view.terminal_offsets_from_bottom.insert(
        terminal_id.clone(),
        TerminalViewportOffset {
            offset_from_bottom: offset_from_bottom.min(metrics.max_offset_from_bottom),
            max_offset_from_bottom: metrics.max_offset_from_bottom,
        },
    );
}

pub(crate) fn terminal_offset_from_bottom(
    terminal_id: &TerminalId,
    metrics: crate::pane::ScrollMetrics,
    view: &ClientViewState,
) -> usize {
    view.terminal_offsets_from_bottom
        .get(terminal_id)
        .copied()
        .map(|offset| offset.for_metrics(metrics))
        .unwrap_or(metrics.offset_from_bottom)
}

pub(crate) fn capture_terminal_offsets_from_runtimes(
    live_terminal_ids: &[TerminalId],
    runtimes: &TerminalRuntimeRegistry,
    view: &mut ClientViewState,
) {
    let live_terminal_ids = live_terminal_ids.iter().collect::<HashSet<_>>();
    for terminal_id in &live_terminal_ids {
        let Some(metrics) = runtimes
            .get(terminal_id)
            .and_then(|runtime| runtime.scroll_metrics())
        else {
            continue;
        };
        view.terminal_offsets_from_bottom.insert(
            (*terminal_id).clone(),
            TerminalViewportOffset::from_metrics(metrics),
        );
    }
    view.terminal_offsets_from_bottom
        .retain(|terminal_id, _| live_terminal_ids.contains(terminal_id));
}

pub(crate) fn apply_terminal_offsets_to_runtimes(
    live_terminal_ids: &[TerminalId],
    runtimes: &TerminalRuntimeRegistry,
    view: &ClientViewState,
) {
    for terminal_id in live_terminal_ids {
        let Some(offset) = view.terminal_offsets_from_bottom.get(terminal_id) else {
            continue;
        };
        let Some(runtime) = runtimes.get(terminal_id) else {
            continue;
        };
        let Some(metrics) = runtime.scroll_metrics() else {
            continue;
        };
        runtime.set_scroll_offset_from_bottom(offset.for_metrics(metrics));
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;

    #[test]
    fn projection_preserves_nonzero_screen_origin_and_inverse() {
        let viewport = TabCanvasViewport::new(
            Size::new(120, 50),
            Rect::new(7, 3, 40, 20),
            CanvasOrigin { col: 30, row: 10 },
        );
        let projected = viewport
            .project_rect(Rect::new(35, 15, 10, 5))
            .expect("rect should intersect source");
        assert_eq!(projected.source, Rect::new(35, 15, 10, 5));
        assert_eq!(projected.destination, Rect::new(12, 8, 10, 5));
        assert_eq!(viewport.screen_to_canvas(12, 8), Some((35, 15)));
        assert_eq!(viewport.canvas_to_screen(35, 15), Some((12, 8)));
        assert_eq!(viewport.screen_to_canvas(7, 3), Some((30, 10)));
    }

    #[test]
    fn projection_leaves_padding_unmapped_when_canvas_is_smaller() {
        let viewport = TabCanvasViewport::new(
            Size::new(20, 10),
            Rect::new(5, 4, 40, 20),
            CanvasOrigin::default(),
        );
        assert_eq!(
            viewport.project_rect(Rect::new(0, 0, 20, 10)),
            Some(ProjectedRect {
                source: Rect::new(0, 0, 20, 10),
                destination: Rect::new(5, 4, 20, 10),
            })
        );
        assert_eq!(viewport.screen_to_canvas(24, 4), Some((19, 0)));
        assert_eq!(viewport.screen_to_canvas(25, 4), None);
        assert_eq!(viewport.screen_to_canvas(5, 13), Some((0, 9)));
        assert_eq!(viewport.screen_to_canvas(5, 14), None);
    }

    #[test]
    fn projection_and_reveal_are_safe_at_u16_edges() {
        let viewport = TabCanvasViewport::new(
            Size::new(u16::MAX, u16::MAX),
            Rect::new(u16::MAX - 3, u16::MAX - 3, 3, 3),
            CanvasOrigin {
                col: u16::MAX,
                row: u16::MAX,
            },
        );
        assert_eq!(
            viewport.origin,
            CanvasOrigin {
                col: u16::MAX - 3,
                row: u16::MAX - 3
            }
        );
        let projected = viewport
            .project_rect(Rect::new(u16::MAX - 3, u16::MAX - 3, 3, 3))
            .expect("edge rect should intersect");
        assert_eq!(projected.destination, viewport.viewport);
        assert_eq!(
            viewport.reveal_focused(
                CanvasOrigin::default(),
                Rect::new(u16::MAX - 10, u16::MAX - 10, 10, 10),
            ),
            CanvasOrigin {
                col: u16::MAX - 10,
                row: u16::MAX - 10,
            }
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;

    #[test]
    fn default_view_matches_current_empty_app_state() {
        let state = AppState::test_new();

        let view = ClientViewState::from_default_client_state(&state);

        assert_eq!(view.active_workspace, None);
        assert_eq!(view.selected_workspace, 0);
        assert_eq!(view.active_group, 0);
        assert!(view.group_filter_enabled);
        assert_eq!(
            view.agent_panel_scope,
            crate::app::state::AgentPanelScope::CurrentWorkspace
        );
        assert_eq!(view.agent_panel_scroll, 0);
        assert_eq!(view.mode, Mode::Navigate);
        assert!(view.active_tabs.is_empty());
        assert!(view.focused_panes.is_empty());
    }

    #[test]
    fn default_view_captures_workspace_tab_focus_and_zoom() {
        let mut state = AppState::test_new();
        state.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        state.active = Some(1);
        state.selected = 1;
        state.mode = Mode::Terminal;
        state.workspaces[0].tabs[0].zoomed = true;

        let first_workspace_id = state.workspaces[0].id.clone();
        let second_workspace_id = state.workspaces[1].id.clone();
        let first_focused = state.workspaces[0].tabs[0].layout.focused();
        let second_focused = state.workspaces[1].tabs[0].layout.focused();

        let view = ClientViewState::from_default_client_state(&state);

        assert_eq!(view.active_workspace, Some(1));
        assert_eq!(view.selected_workspace, 1);
        assert_eq!(view.mode, Mode::Terminal);
        assert_eq!(view.active_tab_for_workspace(&first_workspace_id), Some(0));
        assert_eq!(view.active_tab_for_workspace(&second_workspace_id), Some(0));
        assert_eq!(
            view.focused_pane_for_tab(&first_workspace_id, 1),
            Some(first_focused)
        );
        assert_eq!(
            view.focused_pane_for_tab(&second_workspace_id, 1),
            Some(second_focused)
        );
        assert!(view.tab_is_zoomed(&first_workspace_id, 1));
        assert!(!view.tab_is_zoomed(&second_workspace_id, 1));
    }

    #[test]
    fn reconcile_discards_deleted_workspaces_and_clamps_selection() {
        let mut state = AppState::test_new();
        state.workspaces = vec![Workspace::test_new("one"), Workspace::test_new("two")];
        state.active = Some(0);
        state.selected = 0;
        let removed_workspace_id = state.workspaces[1].id.clone();
        let removed_pane = state.workspaces[1].tabs[0].layout.focused();

        let mut view = ClientViewState::from_default_client_state(&state);
        view.active_workspace = Some(9);
        view.selected_workspace = 9;
        view.active_tabs.insert(removed_workspace_id.clone(), 7);
        view.focused_panes.insert(
            ClientTabViewKey::new(&removed_workspace_id, 1),
            removed_pane,
        );
        view.zoomed_tabs
            .insert(ClientTabViewKey::new(&removed_workspace_id, 1));

        state.workspaces.pop();
        view.reconcile(&state);

        assert_eq!(view.active_workspace, Some(0));
        assert_eq!(view.selected_workspace, 0);
        assert!(!view.active_tabs.contains_key(&removed_workspace_id));
        assert!(view
            .focused_panes
            .keys()
            .all(|key| key.workspace_id != removed_workspace_id));
        assert!(view
            .zoomed_tabs
            .iter()
            .all(|key| key.workspace_id != removed_workspace_id));
    }

    #[test]
    fn reconcile_remaps_focus_by_workspace_id_when_an_earlier_workspace_closes() {
        let mut state = AppState::test_new();
        state.workspaces = vec![
            Workspace::test_new("a"),
            Workspace::test_new("b"),
            Workspace::test_new("c"),
        ];
        state.active = Some(1);
        state.selected = 1;
        let focused_id = state.workspaces[1].id.clone();

        let mut view = ClientViewState::from_default_client_state(&state);
        view.active_workspace = Some(1);
        view.selected_workspace = 1;
        view.mode = Mode::Navigate;
        view.reconcile(&state);

        state.workspaces.remove(0);
        view.reconcile(&state);

        assert_eq!(view.active_workspace, Some(0));
        assert_eq!(view.selected_workspace, 0);
        assert_eq!(
            state.workspaces[view.active_workspace.unwrap()].id,
            focused_id
        );
        assert_eq!(view.mode, Mode::Navigate);
    }

    #[test]
    fn reconcile_preserves_pending_future_tab_focus_until_tab_exists() {
        let mut state = AppState::test_new();
        state.workspaces = vec![Workspace::test_new("shell")];
        state.workspaces[0].active_tab = 0;
        let workspace_id = state.workspaces[0].id.clone();

        let mut view = ClientViewState::from_default_client_state(&state);
        view.pending_active_tabs.insert(workspace_id.clone(), 1);
        view.reconcile(&state);

        assert_eq!(view.active_tab_for_workspace(&workspace_id), Some(0));
        assert_eq!(view.pending_active_tabs.get(&workspace_id), Some(&1));

        state.workspaces[0].test_add_tab(Some("diff"));
        state.workspaces[0].active_tab = 1;
        view.reconcile(&state);

        assert_eq!(view.active_tab_for_workspace(&workspace_id), Some(1));
        assert!(!view.pending_active_tabs.contains_key(&workspace_id));
    }

    #[test]
    fn reconcile_keeps_empty_filtered_group_without_active_workspace() {
        let mut state = AppState::test_new();
        let mut workspace_group = crate::app::state::Group::default_group();
        workspace_group.id = "with-space".to_string();
        let mut empty_group = crate::app::state::Group::default_group();
        empty_group.id = "empty".to_string();
        state.groups = vec![workspace_group.clone(), empty_group];
        state.workspaces = vec![Workspace::test_new("one")];
        state.workspaces[0].group_id = workspace_group.id;
        state.active = Some(0);
        state.selected = 0;
        state.active_group = 0;
        state.group_filter_enabled = true;

        let mut view = ClientViewState::from_default_client_state(&state);
        view.active_group = 1;
        view.active_workspace = None;
        view.selected_workspace = 0;
        view.reconcile(&state);

        assert_eq!(view.active_group, 1);
        assert_eq!(view.active_workspace, None);
        assert_eq!(view.selected_workspace, 0);
    }

    #[tokio::test]
    async fn terminal_scroll_offset_state_is_client_local() {
        let mut state = AppState::test_new();
        state.workspaces = vec![Workspace::test_new("terminal")];
        state.active = Some(0);
        let pane_id = state.workspaces[0].focused_pane_id().expect("focused pane");
        let terminal_id = state.workspaces[0]
            .pane_state(pane_id)
            .and_then(|pane| pane.terminal_id_cloned())
            .expect("terminal id");
        let mut runtimes = TerminalRuntimeRegistry::new();
        runtimes.insert(
            terminal_id.clone(),
            crate::terminal::TerminalRuntime::test_with_scrollback_bytes(
                12,
                3,
                10_000,
                b"one\ntwo\nthree\nfour\nfive\nsix\n",
            ),
        );
        let live_terminal_ids = vec![terminal_id.clone()];

        let mut first_client = ClientViewState::from_default_client_state(&state);
        let mut second_client = ClientViewState::from_default_client_state(&state);
        capture_terminal_offsets_from_runtimes(&live_terminal_ids, &runtimes, &mut first_client);
        capture_terminal_offsets_from_runtimes(&live_terminal_ids, &runtimes, &mut second_client);
        assert_eq!(
            second_client
                .terminal_offsets_from_bottom
                .get(&terminal_id)
                .map(|offset| offset.offset_from_bottom),
            Some(0)
        );

        runtimes.get(&terminal_id).expect("runtime").scroll_up(2);
        capture_terminal_offsets_from_runtimes(&live_terminal_ids, &runtimes, &mut first_client);
        let first_offset = first_client
            .terminal_offsets_from_bottom
            .get(&terminal_id)
            .copied()
            .expect("first client terminal offset");
        assert!(first_offset.offset_from_bottom > 0);

        apply_terminal_offsets_to_runtimes(&live_terminal_ids, &runtimes, &second_client);
        assert_eq!(
            runtimes
                .get(&terminal_id)
                .and_then(|runtime| runtime.scroll_metrics())
                .map(|metrics| metrics.offset_from_bottom),
            Some(0)
        );

        apply_terminal_offsets_to_runtimes(&live_terminal_ids, &runtimes, &first_client);
        assert_eq!(
            runtimes
                .get(&terminal_id)
                .and_then(|runtime| runtime.scroll_metrics())
                .map(|metrics| metrics.offset_from_bottom),
            Some(first_offset.offset_from_bottom)
        );
    }

    #[tokio::test]
    async fn scrolled_terminal_client_view_stays_anchored_when_output_grows() {
        let mut state = AppState::test_new();
        state.workspaces = vec![Workspace::test_new("terminal")];
        state.active = Some(0);
        let pane_id = state.workspaces[0].focused_pane_id().expect("focused pane");
        let terminal_id = state.workspaces[0]
            .pane_state(pane_id)
            .and_then(|pane| pane.terminal_id_cloned())
            .expect("terminal id");
        let live_terminal_ids = vec![terminal_id.clone()];
        let mut runtimes = TerminalRuntimeRegistry::new();
        runtimes.insert(
            terminal_id.clone(),
            crate::terminal::TerminalRuntime::test_with_scrollback_bytes(
                80,
                3,
                10_000,
                b"000000\r\n000001\r\n000002\r\n000003\r\n000004",
            ),
        );
        let runtime = runtimes.get(&terminal_id).expect("initial runtime");
        runtime.scroll_up(1);
        let visible_before = runtime.visible_text();
        let mut client = ClientViewState::from_default_client_state(&state);
        capture_terminal_offsets_from_runtimes(&live_terminal_ids, &runtimes, &mut client);

        runtimes.insert(
            terminal_id.clone(),
            crate::terminal::TerminalRuntime::test_with_scrollback_bytes(
                80,
                3,
                10_000,
                b"000000\r\n000001\r\n000002\r\n000003\r\n000004\r\n000005",
            ),
        );
        apply_terminal_offsets_to_runtimes(&live_terminal_ids, &runtimes, &client);
        let runtime = runtimes.get(&terminal_id).expect("streamed runtime");

        assert_eq!(
            runtime
                .scroll_metrics()
                .map(|metrics| metrics.offset_from_bottom),
            Some(2)
        );
        assert_eq!(runtime.visible_text(), visible_before);
    }
    #[test]
    fn canvas_origins_follow_stable_tab_numbers_after_reorder() {
        let mut state = AppState::test_new();
        let mut workspace = Workspace::test_new("stable-tabs");
        let second_idx = workspace.test_add_tab(Some("second"));
        let first_number = workspace.tabs[0].number;
        let second_number = workspace.tabs[second_idx].number;
        let workspace_id = workspace.id.clone();
        state.workspaces = vec![workspace];
        state.active = Some(0);
        state.selected = 0;
        state.mode = Mode::Terminal;

        let mut view = ClientViewState::from_default_client_state(&state);
        let second_key = ClientTabViewKey::new(&workspace_id, second_number);
        view.tab_canvas_origins
            .insert(second_key.clone(), CanvasOrigin { col: 17, row: 9 });
        view.tab_canvas_origins.insert(
            ClientTabViewKey::new(&workspace_id, 99),
            CanvasOrigin { col: 1, row: 1 },
        );

        state.workspaces[0].tabs.swap(0, second_idx);
        view.reconcile(&state);

        assert_eq!(
            view.tab_canvas_origins.get(&second_key).copied(),
            Some(CanvasOrigin { col: 17, row: 9 })
        );
        assert!(!view
            .tab_canvas_origins
            .contains_key(&ClientTabViewKey::new(&workspace_id, 99)));
        assert!(view
            .tab_canvas_origins
            .keys()
            .all(|key| key.tab_number == first_number || key.tab_number == second_number));
    }
}

#[cfg(test)]
mod tab_control_tests {
    use super::{ClientTabControl, ClientViewState};
    use crate::app::state::AppState;

    #[test]
    fn watching_projection_exposes_epoch_and_queues_one_shot_request() {
        let mut view = ClientViewState::from_default_client_state(&AppState::test_new());
        view.set_tab_control(ClientTabControl::WatchingFree { epoch: 7 });

        assert!(!view.can_mutate_tab());
        assert_eq!(view.tab_control.epoch(), Some(7));
        assert_eq!(view.request_tab_control(), Some(7));
        assert_eq!(view.request_tab_control(), Some(7));
        assert_eq!(view.take_tab_control_request(), Some(7));
        assert_eq!(view.take_tab_control_request(), None);
    }

    #[test]
    fn losing_control_clears_forwarded_terminal_keys() {
        let mut state = AppState::test_new();
        state.workspaces = vec![crate::workspace::Workspace::test_new("control")];
        let mut view = ClientViewState::from_default_client_state(&state);
        let pane_id = state.workspaces[0].tabs[0].layout.focused();
        let key = crate::input::TerminalKey::new(
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::empty(),
        );
        view.input_leases.insert_forwarded(
            crate::app::input::InputLeaseKey::new(view.id(), &key),
            crate::app::input::TerminalKeyTarget {
                workspace_id: state.workspaces[0].id.clone(),
                pane_id,
            },
            key,
        );

        view.set_tab_control(ClientTabControl::WatchingControlled { epoch: 1 });

        assert!(view.input_leases.is_empty());
    }

    #[test]
    fn unavailable_projection_has_no_epoch_or_control_request() {
        let mut view = ClientViewState::from_default_client_state(&AppState::test_new());
        view.set_tab_control(ClientTabControl::Unavailable);

        assert!(!view.can_mutate_tab());
        assert_eq!(view.tab_control.epoch(), None);
        assert_eq!(view.request_tab_control(), None);
    }
}
