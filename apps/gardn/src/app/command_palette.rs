use crate::{
    app::{
        agent_profile_picker::workspace_agent_profile_ids, state::AgentPanelScope,
        view_state::ClientViewState, AppState,
    },
    layout::NavDirection,
    workspace::DEFAULT_GROUP_ID,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CommandPaletteAction {
    OpenNavigator,
    NewWorkspace,
    RenameWorkspace,
    CloseWorkspace,
    PreviousWorkspace,
    NextWorkspace,
    SwitchWorkspace(usize),
    SwitchTab(usize),
    NewTab,
    RenameTab,
    PreviousTab,
    NextTab,
    CloseTab,
    SplitVertical,
    SplitHorizontal,
    ClosePane,
    RenamePane,
    Fullscreen,
    EditScrollback,
    ResizeMode,
    FocusPane(NavDirection),
    CyclePaneNext,
    CyclePanePrevious,
    OpenGroupMenu,
    ShowAllGroups,
    NewGroup,
    RenameGroup,
    DeleteGroup,
    ToggleGroupFilter,
    PreviousGroup,
    NextGroup,
    SwitchGroup(usize),
    OpenAgentMenu,
    OpenContextMenu,
    SetAgentScope(AgentPanelScope),
    PreviousAgent,
    NextAgent,
    OpenBrowser,
    OpenReview,
    OpenEditor,
    OpenGithub,
    Github(crate::github::screen::GithubAction),
    ToggleSidebar,
    ToggleContextBar,
    ZenMode,
    ToggleRightSidebar,
    OpenGlobalMenu,
    OpenSettings,
    OpenKeybinds,
    ReloadConfig,
    OpenNotificationTarget,
    DetachOrQuit,
    CustomCommand(usize),
    ProjectCommand(String),
    NewAgent,
}
impl CommandPaletteAction {
    pub(crate) fn project_command_kind(&self) -> Option<super::state::ProjectCommandKind> {
        match self {
            Self::OpenBrowser => Some(super::state::ProjectCommandKind::Browser),
            Self::OpenReview => Some(super::state::ProjectCommandKind::Review),
            Self::OpenEditor => Some(super::state::ProjectCommandKind::Editor),
            Self::OpenGithub => Some(super::state::ProjectCommandKind::Github),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandPaletteCommand {
    pub title: String,
    pub group: &'static str,
    pub key_label: Option<String>,
    pub action: CommandPaletteAction,
}

impl CommandPaletteCommand {
    fn new(title: impl Into<String>, group: &'static str, action: CommandPaletteAction) -> Self {
        Self {
            title: title.into(),
            group,
            key_label: None,
            action,
        }
    }

    fn with_key_label(mut self, key_label: Option<String>) -> Self {
        self.key_label = key_label;
        self
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return true;
        }

        let haystack = format!("{} {}", self.title, self.group).to_ascii_lowercase();
        query.split_whitespace().all(|term| haystack.contains(term))
    }
}

fn command_palette_group_order(group: &str) -> usize {
    match group {
        "spaces" => 0,
        "tabs" => 1,
        "panes" => 2,
        "groups" => 3,
        "git" => 4,
        "agents" => 5,
        "layout" => 6,
        "app" => 7,
        "custom" => 8,
        _ => 8,
    }
}

pub(crate) fn command_palette_commands(state: &AppState) -> Vec<CommandPaletteCommand> {
    let mut commands = vec![
        CommandPaletteCommand::new("New Space", "spaces", CommandPaletteAction::NewWorkspace),
        CommandPaletteCommand::new(
            "Rename Selected Space",
            "spaces",
            CommandPaletteAction::RenameWorkspace,
        ),
        CommandPaletteCommand::new(
            "Close Selected Space",
            "spaces",
            CommandPaletteAction::CloseWorkspace,
        ),
        CommandPaletteCommand::new(
            "Previous Space",
            "spaces",
            CommandPaletteAction::PreviousWorkspace,
        ),
        CommandPaletteCommand::new("Next Space", "spaces", CommandPaletteAction::NextWorkspace),
        CommandPaletteCommand::new(
            "Open Workspace Navigator",
            "spaces",
            CommandPaletteAction::OpenNavigator,
        ),
        CommandPaletteCommand::new("New Tab", "tabs", CommandPaletteAction::NewTab),
        CommandPaletteCommand::new("Rename Tab", "tabs", CommandPaletteAction::RenameTab),
        CommandPaletteCommand::new("Previous Tab", "tabs", CommandPaletteAction::PreviousTab),
        CommandPaletteCommand::new("Next Tab", "tabs", CommandPaletteAction::NextTab),
        CommandPaletteCommand::new("Close Tab", "tabs", CommandPaletteAction::CloseTab),
        CommandPaletteCommand::new(
            "Split Pane Vertical",
            "panes",
            CommandPaletteAction::SplitVertical,
        ),
        CommandPaletteCommand::new(
            "Split Pane Horizontal",
            "panes",
            CommandPaletteAction::SplitHorizontal,
        ),
        CommandPaletteCommand::new("Close Pane", "panes", CommandPaletteAction::ClosePane),
        CommandPaletteCommand::new("Rename Pane", "panes", CommandPaletteAction::RenamePane),
        CommandPaletteCommand::new("Zoom Pane", "panes", CommandPaletteAction::Fullscreen),
        CommandPaletteCommand::new(
            "Edit Scrollback",
            "panes",
            CommandPaletteAction::EditScrollback,
        ),
        CommandPaletteCommand::new("Resize Panes", "panes", CommandPaletteAction::ResizeMode),
        CommandPaletteCommand::new(
            "Focus Pane Left",
            "panes",
            CommandPaletteAction::FocusPane(NavDirection::Left),
        ),
        CommandPaletteCommand::new(
            "Focus Pane Down",
            "panes",
            CommandPaletteAction::FocusPane(NavDirection::Down),
        ),
        CommandPaletteCommand::new(
            "Focus Pane Up",
            "panes",
            CommandPaletteAction::FocusPane(NavDirection::Up),
        ),
        CommandPaletteCommand::new(
            "Focus Pane Right",
            "panes",
            CommandPaletteAction::FocusPane(NavDirection::Right),
        ),
        CommandPaletteCommand::new(
            "Cycle Pane Next",
            "panes",
            CommandPaletteAction::CyclePaneNext,
        ),
        CommandPaletteCommand::new(
            "Cycle Pane Previous",
            "panes",
            CommandPaletteAction::CyclePanePrevious,
        ),
        CommandPaletteCommand::new(
            "Open Group Menu",
            "groups",
            CommandPaletteAction::OpenGroupMenu,
        ),
        CommandPaletteCommand::new(
            "Show All Spaces",
            "groups",
            CommandPaletteAction::ShowAllGroups,
        ),
        CommandPaletteCommand::new("New Group", "groups", CommandPaletteAction::NewGroup),
        CommandPaletteCommand::new("Rename Group", "groups", CommandPaletteAction::RenameGroup),
        CommandPaletteCommand::new("Delete Group", "groups", CommandPaletteAction::DeleteGroup),
        CommandPaletteCommand::new(
            "Toggle Current/All Groups",
            "groups",
            CommandPaletteAction::ToggleGroupFilter,
        ),
        CommandPaletteCommand::new(
            "Previous Group",
            "groups",
            CommandPaletteAction::PreviousGroup,
        ),
        CommandPaletteCommand::new("Next Group", "groups", CommandPaletteAction::NextGroup),
        CommandPaletteCommand::new(
            "Open Agent Menu",
            "agents",
            CommandPaletteAction::OpenAgentMenu,
        ),
        CommandPaletteCommand::new(
            "Open Context Menu",
            "agents",
            CommandPaletteAction::OpenContextMenu,
        ),
        CommandPaletteCommand::new(
            "Agents: Space",
            "agents",
            CommandPaletteAction::SetAgentScope(AgentPanelScope::CurrentWorkspace),
        ),
        CommandPaletteCommand::new(
            "Agents: Group",
            "agents",
            CommandPaletteAction::SetAgentScope(AgentPanelScope::CurrentGroup),
        ),
        CommandPaletteCommand::new(
            "Agents: All",
            "agents",
            CommandPaletteAction::SetAgentScope(AgentPanelScope::AllWorkspaces),
        ),
        CommandPaletteCommand::new(
            "Previous Agent",
            "agents",
            CommandPaletteAction::PreviousAgent,
        ),
        CommandPaletteCommand::new("Next Agent", "agents", CommandPaletteAction::NextAgent),
        CommandPaletteCommand::new("Open Browser", "project", CommandPaletteAction::OpenBrowser),
        CommandPaletteCommand::new("Open Review", "project", CommandPaletteAction::OpenReview),
        CommandPaletteCommand::new("Open Editor", "project", CommandPaletteAction::OpenEditor),
        CommandPaletteCommand::new("Open GitHub", "project", CommandPaletteAction::OpenGithub),
        CommandPaletteCommand::new(
            "Toggle Sidebar",
            "layout",
            CommandPaletteAction::ToggleSidebar,
        ),
        CommandPaletteCommand::new(
            "Toggle Context Bar",
            "layout",
            CommandPaletteAction::ToggleContextBar,
        ),
        CommandPaletteCommand::new("Toggle Zen Mode", "layout", CommandPaletteAction::ZenMode),
        CommandPaletteCommand::new(
            "Toggle Right Sidebar",
            "layout",
            CommandPaletteAction::ToggleRightSidebar,
        ),
        CommandPaletteCommand::new(
            "Open Global Menu",
            "app",
            CommandPaletteAction::OpenGlobalMenu,
        ),
        CommandPaletteCommand::new("Open Settings", "app", CommandPaletteAction::OpenSettings),
        CommandPaletteCommand::new("Open Keybinds", "app", CommandPaletteAction::OpenKeybinds),
        CommandPaletteCommand::new("Reload Config", "app", CommandPaletteAction::ReloadConfig),
        CommandPaletteCommand::new(
            "Open Notification Target",
            "app",
            CommandPaletteAction::OpenNotificationTarget,
        ),
        CommandPaletteCommand::new("Detach / Quit", "app", CommandPaletteAction::DetachOrQuit),
    ];

    if let Some(ws) = state.active.and_then(|idx| state.workspaces.get(idx)) {
        commands.extend(ws.tabs.iter().enumerate().map(|(idx, _tab)| {
            CommandPaletteCommand::new(
                format!(
                    "Switch to Tab: {}",
                    ws.tab_display_name(idx)
                        .unwrap_or_else(|| (idx + 1).to_string())
                ),
                "tabs",
                CommandPaletteAction::SwitchTab(idx),
            )
            .with_key_label(indexed_keybind_label(&state.keybinds.switch_tab, idx))
        }));
    }

    commands.extend(
        state
            .visible_workspace_indices()
            .into_iter()
            .enumerate()
            .filter_map(|(shortcut_idx, idx)| {
                state.workspaces.get(idx).map(|workspace| {
                    CommandPaletteCommand::new(
                        format!("Switch to Space: {}", workspace.display_name()),
                        "spaces",
                        CommandPaletteAction::SwitchWorkspace(idx),
                    )
                    .with_key_label(indexed_keybind_label(
                        &state.keybinds.switch_workspace,
                        shortcut_idx,
                    ))
                })
            }),
    );

    commands.extend(state.groups.iter().enumerate().map(|(idx, group)| {
        CommandPaletteCommand::new(
            format!("Switch to Group: {} {}", group.icon, group.name),
            "groups",
            CommandPaletteAction::SwitchGroup(idx),
        )
        .with_key_label(indexed_keybind_label(&state.keybinds.switch_group, idx))
    }));

    if state
        .active
        .is_some_and(|ws_idx| workspace_agent_profile_ids(state, ws_idx).next().is_some())
    {
        commands.push(CommandPaletteCommand::new(
            "New Agent",
            "agents",
            CommandPaletteAction::NewAgent,
        ));
    }

    commands.extend(
        state
            .keybinds
            .custom_commands
            .iter()
            .enumerate()
            .map(|(idx, binding)| {
                CommandPaletteCommand::new(
                    format!("Run Command: {}", binding.command),
                    "custom",
                    CommandPaletteAction::CustomCommand(idx),
                )
            }),
    );
    commands.extend(state.command_catalog.iter().map(|project_command| {
        CommandPaletteCommand::new(
            format!("Run Project Command: {}", project_command.name),
            "project",
            CommandPaletteAction::ProjectCommand(project_command.id.clone()),
        )
    }));

    for command in &mut commands {
        if command.key_label.is_none() {
            command.key_label = command_palette_key_label(state, &command.action);
        }
    }

    commands
}

fn indexed_keybind_label(
    bindings: &[crate::config::IndexedKeybind],
    index: usize,
) -> Option<String> {
    bindings.get(index).map(|binding| binding.label.clone())
}

fn command_palette_key_label(state: &AppState, action: &CommandPaletteAction) -> Option<String> {
    let kb = &state.keybinds;
    let label = |bindings: &crate::config::ActionKeybinds| bindings.label();
    match action {
        CommandPaletteAction::OpenNavigator => label(&kb.workspace_picker),
        CommandPaletteAction::NewWorkspace => label(&kb.new_workspace),
        CommandPaletteAction::RenameWorkspace => label(&kb.rename_workspace),
        CommandPaletteAction::CloseWorkspace => label(&kb.close_workspace),
        CommandPaletteAction::PreviousWorkspace => label(&kb.previous_workspace),
        CommandPaletteAction::NextWorkspace => label(&kb.next_workspace),
        CommandPaletteAction::NewTab => label(&kb.new_tab),
        CommandPaletteAction::SwitchTab(idx) => indexed_keybind_label(&kb.switch_tab, *idx),
        CommandPaletteAction::RenameTab => label(&kb.rename_tab),
        CommandPaletteAction::PreviousTab => label(&kb.previous_tab),
        CommandPaletteAction::NextTab => label(&kb.next_tab),
        CommandPaletteAction::CloseTab => label(&kb.close_tab),
        CommandPaletteAction::SplitVertical => label(&kb.split_vertical),
        CommandPaletteAction::SplitHorizontal => label(&kb.split_horizontal),
        CommandPaletteAction::ClosePane => label(&kb.close_pane),
        CommandPaletteAction::RenamePane => label(&kb.rename_pane),
        CommandPaletteAction::Fullscreen => label(&kb.zoom),
        CommandPaletteAction::EditScrollback => label(&kb.edit_scrollback),
        CommandPaletteAction::ResizeMode => label(&kb.resize_mode),
        CommandPaletteAction::FocusPane(crate::layout::NavDirection::Left) => {
            label(&kb.focus_pane_left).or_else(|| Some("h".into()))
        }
        CommandPaletteAction::FocusPane(crate::layout::NavDirection::Down) => {
            label(&kb.focus_pane_down).or_else(|| Some("j".into()))
        }
        CommandPaletteAction::FocusPane(crate::layout::NavDirection::Up) => {
            label(&kb.focus_pane_up).or_else(|| Some("k".into()))
        }
        CommandPaletteAction::FocusPane(crate::layout::NavDirection::Right) => {
            label(&kb.focus_pane_right).or_else(|| Some("l".into()))
        }
        CommandPaletteAction::CyclePaneNext => label(&kb.cycle_pane_next),
        CommandPaletteAction::CyclePanePrevious => label(&kb.cycle_pane_previous),
        CommandPaletteAction::OpenGroupMenu => label(&kb.open_group_menu),
        CommandPaletteAction::NewGroup => label(&kb.new_group),
        CommandPaletteAction::RenameGroup => label(&kb.rename_group),
        CommandPaletteAction::DeleteGroup => label(&kb.delete_group),
        CommandPaletteAction::ToggleGroupFilter => label(&kb.toggle_group_filter),
        CommandPaletteAction::PreviousGroup => label(&kb.previous_group),
        CommandPaletteAction::NextGroup => label(&kb.next_group),
        CommandPaletteAction::OpenAgentMenu => label(&kb.open_agent_menu),
        CommandPaletteAction::OpenContextMenu => label(&kb.open_context_menu),
        CommandPaletteAction::PreviousAgent => label(&kb.previous_agent),
        CommandPaletteAction::NextAgent => label(&kb.next_agent),
        CommandPaletteAction::OpenBrowser
        | CommandPaletteAction::OpenReview
        | CommandPaletteAction::OpenEditor
        | CommandPaletteAction::OpenGithub
        | CommandPaletteAction::Github(_) => None,
        CommandPaletteAction::ToggleSidebar => label(&kb.toggle_sidebar),
        CommandPaletteAction::ToggleContextBar => label(&kb.toggle_context_bar),
        CommandPaletteAction::ZenMode => label(&kb.zen_mode),
        CommandPaletteAction::ToggleRightSidebar => label(&kb.toggle_right_sidebar),
        CommandPaletteAction::OpenSettings => label(&kb.settings),
        CommandPaletteAction::OpenKeybinds => label(&kb.help),
        CommandPaletteAction::ReloadConfig => label(&kb.reload_config),
        CommandPaletteAction::OpenNotificationTarget => label(&kb.open_notification_target),
        CommandPaletteAction::DetachOrQuit => label(&kb.detach),
        CommandPaletteAction::CustomCommand(idx) => kb
            .custom_commands
            .get(*idx)
            .map(|binding| binding.label.clone()),
        CommandPaletteAction::ProjectCommand(_) => None,
        CommandPaletteAction::SwitchWorkspace(_)
        | CommandPaletteAction::ShowAllGroups
        | CommandPaletteAction::SwitchGroup(_)
        | CommandPaletteAction::NewAgent
        | CommandPaletteAction::SetAgentScope(_)
        | CommandPaletteAction::OpenGlobalMenu => None,
    }
}

pub(crate) fn command_palette_commands_for_view(
    state: &AppState,
    view: &ClientViewState,
) -> Vec<CommandPaletteCommand> {
    let mut commands = command_palette_commands(state);
    if let Some(screen) = &view.github {
        commands.extend(
            screen
                .contextual_actions()
                .into_iter()
                .map(|(action, title)| {
                    CommandPaletteCommand::new(title, "git", CommandPaletteAction::Github(action))
                }),
        );
    }
    commands.retain(|command| {
        !matches!(
            command.action,
            CommandPaletteAction::SwitchWorkspace(_)
                | CommandPaletteAction::SwitchTab(_)
                | CommandPaletteAction::NewAgent
        )
    });
    if let Some(ws) = view
        .active_workspace
        .and_then(|idx| state.workspaces.get(idx))
    {
        commands.extend(ws.tabs.iter().enumerate().map(|(idx, _tab)| {
            CommandPaletteCommand::new(
                format!(
                    "Switch to Tab: {}",
                    ws.tab_display_name(idx)
                        .unwrap_or_else(|| (idx + 1).to_string())
                ),
                "tabs",
                CommandPaletteAction::SwitchTab(idx),
            )
            .with_key_label(indexed_keybind_label(&state.keybinds.switch_tab, idx))
        }));
    }
    if view
        .active_workspace
        .is_some_and(|ws_idx| workspace_agent_profile_ids(state, ws_idx).next().is_some())
    {
        commands.push(CommandPaletteCommand::new(
            "New Agent",
            "agents",
            CommandPaletteAction::NewAgent,
        ));
    }
    let active_group_id = state
        .groups
        .get(view.active_group)
        .map(|group| group.id.as_str())
        .unwrap_or(DEFAULT_GROUP_ID);
    commands.extend(
        state
            .workspaces
            .iter()
            .enumerate()
            .filter(|(_, workspace)| {
                !view.group_filter_enabled || workspace.group_id == active_group_id
            })
            .enumerate()
            .map(|(shortcut_idx, (idx, workspace))| {
                CommandPaletteCommand::new(
                    format!("Switch to Space: {}", workspace.display_name()),
                    "spaces",
                    CommandPaletteAction::SwitchWorkspace(idx),
                )
                .with_key_label(indexed_keybind_label(
                    &state.keybinds.switch_workspace,
                    shortcut_idx,
                ))
            }),
    );
    commands
}

pub(crate) fn command_palette_filtered_commands_for_view(
    state: &AppState,
    view: &ClientViewState,
) -> Vec<CommandPaletteCommand> {
    let mut commands = command_palette_commands_for_view(state, view)
        .into_iter()
        .enumerate()
        .filter(|(_, command)| command.matches(view.command_palette.query.as_str()))
        .collect::<Vec<_>>();

    commands.sort_by_key(|(idx, command)| (command_palette_group_order(command.group), *idx));
    commands.into_iter().map(|(_, command)| command).collect()
}

pub(crate) fn command_palette_filtered_commands(state: &AppState) -> Vec<CommandPaletteCommand> {
    command_palette_filtered_commands_for_query(state, state.command_palette.query.as_str())
}

pub(crate) fn command_palette_filtered_commands_for_query(
    state: &AppState,
    query: &str,
) -> Vec<CommandPaletteCommand> {
    let mut commands = command_palette_commands(state)
        .into_iter()
        .enumerate()
        .filter(|(_, command)| command.matches(query))
        .collect::<Vec<_>>();

    commands.sort_by_key(|(idx, command)| (command_palette_group_order(command.group), *idx));
    commands.into_iter().map(|(_, command)| command).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;

    #[test]
    fn client_palette_lists_workspaces_from_its_group_filter() {
        let mut state = AppState::test_new();
        let client_group = state.create_group("client".to_string());
        let mut client_workspace = Workspace::test_new("client workspace");
        client_workspace.group_id = state.groups[client_group].id.clone();
        state.workspaces = vec![Workspace::test_new("default workspace"), client_workspace];
        state.active_group = 0;
        state.group_filter_enabled = true;

        let mut view = ClientViewState::from_default_client_state(&state);
        view.active_group = client_group;
        view.group_filter_enabled = true;

        let workspace_actions = command_palette_filtered_commands_for_view(&state, &view)
            .into_iter()
            .filter_map(|command| match command.action {
                CommandPaletteAction::SwitchWorkspace(idx) => Some(idx),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(workspace_actions, vec![1]);
    }

    #[test]
    fn palette_includes_workspace_navigator_with_its_keybinding() {
        let state = AppState::test_new();

        let command = command_palette_filtered_commands_for_query(&state, "workspace navigator")
            .into_iter()
            .next()
            .expect("workspace navigator command");

        assert_eq!(command.action, CommandPaletteAction::OpenNavigator);
        assert_eq!(command.key_label, state.keybinds.workspace_picker.label());
    }
}
