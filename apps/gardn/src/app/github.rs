use super::{state, App, ClientViewState, Mode};
use crate::events::AppEvent;

impl App {
    pub(super) fn handle_github_mouse_for_view(
        &mut self,
        view: &mut ClientViewState,
        mouse: crossterm::event::MouseEvent,
    ) -> bool {
        let inside = view
            .github_pane_rect(&self.state)
            .contains((mouse.column, mouse.row).into());
        if !inside && matches!(mouse.kind, crossterm::event::MouseEventKind::Up(_)) {
            if let Some(screen) = view.github.as_mut() {
                screen.handle_mouse(mouse);
            }
        }
        if !matches!(view.mode, Mode::Terminal | Mode::Github)
            || view.popup_pane.is_some()
            || view.context_menu.is_some()
            || view.github.is_none()
            || !inside
        {
            return false;
        }
        if matches!(mouse.kind, crossterm::event::MouseEventKind::Down(_)) {
            if let (Some(ws_idx), Some(pane_id)) = (view.active_workspace, view.github_pane_id) {
                if let Some(tab_idx) = view.active_tab_index_for_workspace(&self.state, ws_idx) {
                    view.focus_pane_in_workspace(&self.state, ws_idx, tab_idx, pane_id);
                    view.mode = Mode::Github;
                }
            }
        }
        let effects = view
            .github
            .as_mut()
            .map(|screen| screen.handle_mouse(mouse))
            .unwrap_or_default();
        self.apply_github_effects(view, effects);
        true
    }

    pub(crate) fn open_default_github(&mut self, workspace: Option<usize>) {
        self.with_default_github_view(|app, view| {
            view.active_workspace = workspace.or(app.state.active);
            app.open_github_for_view(view);
        });
    }

    pub(crate) fn close_github_for_view(&mut self, view: &mut ClientViewState) {
        if let Some(mut screen) = view.github.take() {
            for id in screen
                .pending_requests()
                .into_iter()
                .chain(screen.cancel_requests())
            {
                self.github_runtime.cancel(id);
            }
        }
        view.github_workspace_id = None;
        view.github_pane_id = None;
        view.github_scope_settings = None;
        if matches!(view.mode, Mode::Github | Mode::CommandPalette) {
            view.return_to_active_workspace_mode();
        }
    }

    pub(crate) fn open_github_for_view(&mut self, view: &mut ClientViewState) {
        let Some(ws_idx) = view.active_workspace else {
            return;
        };
        let Some((_, pane_id)) = view.focused_pane_for_workspace(&self.state, ws_idx) else {
            return;
        };
        self.close_github_for_view(view);
        match self
            .state
            .resolved_github_scope(&self.terminal_runtimes, ws_idx)
        {
            Ok(scope) => {
                let workspace = &self.state.workspaces[ws_idx];
                view.github_workspace_id = Some(workspace.id.clone());
                view.github_pane_id = Some(pane_id);
                view.github_scope_settings = Some((
                    workspace.github_scope.clone(),
                    self.state
                        .groups
                        .iter()
                        .find(|group| group.id == workspace.group_id)
                        .and_then(|group| group.github_organization.clone()),
                ));
                view.github = Some(crate::github::screen::GithubScreen::new(scope));
                view.mode = Mode::Github;
                self.pump_github_for_view(view);
            }
            Err(error) => {
                self.state.toast = Some(state::ToastNotification {
                    kind: state::ToastKind::NeedsAttention,
                    title: "Cannot open GitHub".into(),
                    context: error,
                    position: None,
                    target: None,
                });
            }
        }
    }

    pub(crate) fn poll_github(&mut self) -> bool {
        self.github_runtime.tick()
    }

    pub(crate) fn github_has_pending(&self) -> bool {
        self.github_runtime.has_pending()
    }

    pub(crate) fn pump_github_for_view(&mut self, view: &mut ClientViewState) -> bool {
        if view.github.is_none() {
            return false;
        }
        let workspace = view.github_workspace_id.as_ref().and_then(|id| {
            self.state
                .workspaces
                .iter()
                .position(|workspace| &workspace.id == id)
        });
        let valid = workspace.is_some_and(|index| {
            let workspace = &self.state.workspaces[index];
            let organization = self
                .state
                .groups
                .iter()
                .find(|group| group.id == workspace.group_id)
                .and_then(|group| group.github_organization.as_ref());
            view.github_pane_id.is_some_and(|pane_id| {
                workspace
                    .tabs
                    .iter()
                    .any(|tab| tab.panes.contains_key(&pane_id))
            }) && view.github_scope_settings.as_ref().is_some_and(
                |(scope, previous_organization)| {
                    scope == &workspace.github_scope
                        && previous_organization.as_ref() == organization
                },
            )
        });
        if !valid {
            self.close_github_for_view(view);
            return true;
        }
        let Some(screen) = view.github.as_mut() else {
            return false;
        };
        for id in screen.cancel_requests() {
            self.github_runtime.cancel(id);
        }
        let mut changed = false;
        for id in screen.pending_requests() {
            if let Some(result) = self.github_runtime.take(id) {
                screen.apply(id, result);
                changed = true;
            }
        }
        let force_refresh = screen.take_force_refresh();
        for request in screen.drain_requests() {
            let id = if force_refresh {
                self.github_runtime
                    .submit_refresh(screen.scope.clone(), request)
            } else {
                self.github_runtime.submit(screen.scope.clone(), request)
            };
            screen.track_request(id);
            changed = true;
        }
        changed
    }

    pub(super) fn apply_github_effects(
        &mut self,
        view: &mut ClientViewState,
        effects: Vec<crate::github::screen::GithubEffect>,
    ) {
        for effect in effects {
            match effect {
                crate::github::screen::GithubEffect::Close => self.close_github_for_view(view),
                crate::github::screen::GithubEffect::OpenPalette => {
                    view.command_palette.query.clear();
                    view.command_palette.list.select(0);
                    view.command_palette.scroll = 0;
                    view.mode = Mode::CommandPalette;
                }
                crate::github::screen::GithubEffect::OpenUrl(url) => {
                    if let Err(error) = self.event_tx.try_send(AppEvent::ClientOpenUrl {
                        view_id: view.id(),
                        url,
                    }) {
                        tracing::warn!(%error, "failed to queue GitHub browser action");
                    }
                }
                crate::github::screen::GithubEffect::Copy(text) => {
                    if let Err(error) = self.event_tx.try_send(AppEvent::ClientClipboardWrite {
                        view_id: view.id(),
                        content: text.into_bytes(),
                    }) {
                        tracing::warn!(%error, "failed to queue GitHub clipboard action");
                    }
                }
                crate::github::screen::GithubEffect::OpenEditor => {
                    if let Some(ws_idx) = view.github_workspace_id.as_ref().and_then(|id| {
                        self.state
                            .workspaces
                            .iter()
                            .position(|workspace| &workspace.id == id)
                    }) {
                        self.close_github_for_view(view);
                        view.active_workspace = Some(ws_idx);
                        self.execute_client_view_command_palette_action(
                            view,
                            crate::app::command_palette::CommandPaletteAction::OpenEditor,
                        );
                    }
                }
            }
        }
        self.pump_github_for_view(view);
    }

    pub(super) fn with_default_github_view<R>(
        &mut self,
        action: impl FnOnce(&mut Self, &mut ClientViewState) -> R,
    ) -> R {
        let replacement = ClientViewState::from_default_client_state(&self.state);
        let mut view = std::mem::replace(&mut self.default_client_view, replacement);
        view.active_tabs = self
            .state
            .workspaces
            .iter()
            .map(|workspace| (workspace.id.clone(), workspace.active_tab))
            .collect();
        view.focused_panes = self
            .state
            .workspaces
            .iter()
            .flat_map(|workspace| {
                workspace.tabs.iter().map(|tab| {
                    (
                        super::view_state::ClientTabViewKey::new(&workspace.id, tab.number),
                        tab.layout.focused(),
                    )
                })
            })
            .collect();
        view.tab_canvas_view = None;
        view.active_workspace = self.state.active;
        view.mode = self.state.mode;
        view.computed = self.state.view.clone();
        view.command_palette = self.state.command_palette.clone();
        view.sync_github_mode(&self.state);
        view.compute_github(&self.state);
        let result = action(self, &mut view);
        self.state.mode = view.mode;
        self.state.command_palette = view.command_palette.clone();
        self.default_client_view = view;
        result
    }
}
