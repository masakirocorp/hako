use super::{state::AppState, App};
fn is_system_theme(name: &str) -> bool {
    name.eq_ignore_ascii_case("system")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManagedTerminalThemeTarget {
    pub terminal_id: crate::terminal::TerminalId,
    pub resolved_override: Option<crate::terminal_theme::ResolvedTerminalTheme>,
}

impl AppState {
    pub(crate) fn managed_terminal_theme_targets(&self) -> Vec<ManagedTerminalThemeTarget> {
        let mut terminal_ids = std::collections::HashSet::<crate::terminal::TerminalId>::new();
        let mut targets = Vec::new();
        for (ws_idx, workspace) in self.workspaces.iter().enumerate() {
            for terminal_id in workspace
                .tabs
                .iter()
                .flat_map(|tab| tab.panes.values())
                .map(|pane| &pane.attached_terminal_id)
            {
                if !terminal_ids.insert(terminal_id.clone()) {
                    continue;
                }
                let Some(binding) = self
                    .terminals
                    .get(terminal_id)
                    .and_then(|terminal| terminal.terminal_theme_binding)
                else {
                    continue;
                };
                let resolved_override = match binding.source {
                    crate::terminal_theme::TerminalThemeSource::WorkspacePalette
                        if crate::external_tool_theme::is_terminal_passthrough(
                            &self.theme_name,
                        ) =>
                    {
                        None
                    }
                    crate::terminal_theme::TerminalThemeSource::WorkspacePalette => {
                        Some(crate::external_tool_theme::resolved_terminal_theme(
                            &self.palette_for_workspace(ws_idx),
                            self.effective_theme_appearance,
                            self.host_terminal_theme,
                        ))
                    }
                };
                targets.push(ManagedTerminalThemeTarget {
                    terminal_id: terminal_id.clone(),
                    resolved_override,
                });
            }
        }
        targets
    }
}

impl App {
    pub(super) fn query_host_terminal_theme(&self) {
        #[cfg(test)]
        self.host_terminal_theme_query_count
            .set(self.host_terminal_theme_query_count.get() + 1);
        use std::io::Write;

        let _ = std::io::stdout()
            .write_all(crate::terminal_theme::HOST_COLOR_QUERY_SEQUENCE.as_bytes());
        let _ = std::io::stdout().flush();
    }

    pub(crate) async fn refresh_host_terminal_theme_for(&mut self, timeout: std::time::Duration) {
        self.query_host_terminal_theme();

        let deadline = std::time::Instant::now() + timeout;
        let mut idle_deadline: Option<std::time::Instant> = None;

        loop {
            if host_terminal_theme_complete(self.state.host_terminal_theme) {
                break;
            }

            let now = std::time::Instant::now();
            if now >= deadline || idle_deadline.is_some_and(|idle| now >= idle) {
                break;
            }

            let wait_until = idle_deadline.unwrap_or(deadline).min(deadline);
            let Some(rx) = self.input_rx.as_mut() else {
                break;
            };

            match tokio::time::timeout_at(tokio::time::Instant::from_std(wait_until), rx.recv())
                .await
            {
                Ok(Some(event)) => {
                    if self.handle_host_terminal_theme_event(&event) {
                        idle_deadline =
                            Some(std::time::Instant::now() + std::time::Duration::from_millis(80));
                    }
                }
                Ok(None) => {
                    self.input_rx = None;
                    break;
                }
                Err(_) => break,
            }
        }
    }

    fn handle_host_terminal_theme_event(
        &mut self,
        event: &crate::raw_input::RawInputEvent,
    ) -> bool {
        match event {
            crate::raw_input::RawInputEvent::HostDefaultColor { kind, color } => {
                self.update_host_terminal_theme(*kind, *color)
            }
            crate::raw_input::RawInputEvent::HostPaletteColor { index, color } => {
                self.update_host_terminal_palette_color(*index, *color)
            }
            crate::raw_input::RawInputEvent::HostCursorColor { color } => {
                self.update_host_terminal_cursor_color(*color)
            }
            _ => false,
        }
    }

    pub(super) fn update_host_terminal_theme(
        &mut self,
        kind: crate::terminal_theme::DefaultColorKind,
        color: crate::terminal_theme::RgbColor,
    ) -> bool {
        let next_theme = self.state.host_terminal_theme.with_color(kind, color);
        self.set_host_terminal_theme(next_theme)
    }

    pub(super) fn update_host_terminal_palette_color(
        &mut self,
        index: u8,
        color: crate::terminal_theme::RgbColor,
    ) -> bool {
        let next_theme = self
            .state
            .host_terminal_theme
            .with_palette_color(index, color);
        self.set_host_terminal_theme(next_theme)
    }

    pub(super) fn update_host_terminal_cursor_color(
        &mut self,
        color: crate::terminal_theme::RgbColor,
    ) -> bool {
        let next_theme = self.state.host_terminal_theme.with_cursor_color(color);
        self.set_host_terminal_theme(next_theme)
    }

    pub(crate) fn set_host_terminal_theme(
        &mut self,
        theme: crate::terminal_theme::TerminalTheme,
    ) -> bool {
        if theme.is_empty() || theme == self.state.host_terminal_theme {
            return false;
        }
        self.state.host_terminal_theme = theme;
        if self.state.global_theme_mode == crate::config::ThemeMode::System
            || is_system_theme(&self.state.theme_name)
            || is_system_theme(&self.state.global_light_theme_name)
            || is_system_theme(&self.state.global_dark_theme_name)
        {
            self.state.refresh_global_palette();
            self.state.apply_effective_theme();
        }
        self.apply_host_terminal_theme_to_panes();
        true
    }

    pub(crate) fn reconcile_terminal_themes(&mut self) -> bool {
        let targets = self.state.managed_terminal_theme_targets();
        let target_ids = targets
            .iter()
            .map(|target| target.terminal_id.clone())
            .collect::<std::collections::HashSet<_>>();
        let stale_ids = self
            .reconciled_terminal_themes
            .keys()
            .filter(|terminal_id| !target_ids.contains(*terminal_id))
            .cloned()
            .collect::<Vec<_>>();
        let mut changed = false;
        for terminal_id in stale_ids {
            if let Some(runtime) = self.terminal_runtimes.get(&terminal_id) {
                runtime.set_resolved_terminal_theme_override(None);
                changed = true;
            }
            self.reconciled_terminal_themes.remove(&terminal_id);
        }

        for target in targets {
            let Some(runtime) = self.terminal_runtimes.get(&target.terminal_id) else {
                self.reconciled_terminal_themes.remove(&target.terminal_id);
                continue;
            };
            let next = (runtime.instance_key(), target.resolved_override);
            if self.reconciled_terminal_themes.get(&target.terminal_id) == Some(&next) {
                continue;
            }
            runtime.set_resolved_terminal_theme_override(target.resolved_override);
            self.reconciled_terminal_themes
                .insert(target.terminal_id, next);
            changed = true;
        }

        if changed {
            self.render_dirty.request_generic();
            self.render_notify.notify_one();
        }
        changed
    }

    fn apply_host_terminal_theme_to_panes(&self) {
        if self.state.host_terminal_theme.is_empty() {
            return;
        }

        let appearance = self.state.host_terminal_theme.appearance();
        for runtime in self.terminal_runtimes.values() {
            runtime.apply_host_terminal_theme(self.state.host_terminal_theme);
            runtime.apply_host_terminal_appearance(appearance);
        }

        self.render_dirty.request_generic();
        self.render_notify.notify_one();
    }
}

fn host_terminal_theme_complete(theme: crate::terminal_theme::TerminalTheme) -> bool {
    theme.foreground.is_some()
        && theme.background.is_some()
        && theme.palette.iter().all(Option::is_some)
}
#[cfg(test)]
mod tests {
    use crate::app::state::{AppState, Group, Palette};
    use crate::config::{TerminalAccent, ThemeMode};
    use crate::terminal_theme::{TerminalThemeBinding, TerminalThemeSource, ThemeAppearance};
    use crate::workspace::Workspace;

    #[test]
    fn managed_terminal_theme_resolves_from_its_owning_workspace() {
        let mut state = AppState::test_new();
        state.workspaces = vec![Workspace::test_new("first"), Workspace::test_new("second")];
        let mut second_group = Group::default_group();
        second_group.id = "second".to_string();
        second_group.name = "Second".to_string();
        second_group.accent = Some(TerminalAccent::Red);
        state.groups.push(second_group);
        state.workspaces[1].group_id = "second".to_string();
        state.ensure_test_terminals();
        state.palette = Palette::dracula();
        state.global_palette = Palette::dracula();
        state.theme_name = "dracula".to_string();
        state.global_theme_name = "dracula".to_string();
        state.global_theme_mode = ThemeMode::Dark;
        state.effective_theme_appearance = ThemeAppearance::Dark;
        let pane_id = state.workspaces[1].tabs[0].root_pane;
        let terminal_id = state.workspaces[1]
            .terminal_id(pane_id)
            .expect("second workspace terminal")
            .clone();
        state
            .terminals
            .get_mut(&terminal_id)
            .expect("terminal state")
            .terminal_theme_binding = Some(TerminalThemeBinding {
            source: TerminalThemeSource::WorkspacePalette,
        });

        let target = state
            .managed_terminal_theme_targets()
            .into_iter()
            .find(|target| target.terminal_id == terminal_id)
            .expect("managed terminal target");
        let expected = crate::external_tool_theme::resolved_terminal_theme(
            &state.palette_for_workspace(1),
            ThemeAppearance::Dark,
            state.host_terminal_theme,
        );

        assert_eq!(target.resolved_override, Some(expected));
    }

    #[test]
    fn system_preview_clears_managed_override_without_changing_committed_mode() {
        let mut state = AppState::test_new();
        state.workspaces.push(Workspace::test_new("web"));
        state.ensure_test_terminals();
        let pane_id = state.workspaces[0].tabs[0].root_pane;
        let terminal_id = state.workspaces[0]
            .terminal_id(pane_id)
            .expect("workspace terminal")
            .clone();
        state
            .terminals
            .get_mut(&terminal_id)
            .expect("terminal state")
            .terminal_theme_binding = Some(TerminalThemeBinding {
            source: TerminalThemeSource::WorkspacePalette,
        });
        state.global_theme_mode = ThemeMode::Dark;
        assert!(state.preview_theme_with_mode("system", ThemeMode::Light));

        let target = state
            .managed_terminal_theme_targets()
            .into_iter()
            .find(|target| target.terminal_id == terminal_id)
            .expect("managed terminal target");

        assert_eq!(state.global_theme_mode, ThemeMode::Dark);
        assert_eq!(state.effective_theme_appearance, ThemeAppearance::Light);
        assert_eq!(target.resolved_override, None);

        assert!(state.preview_theme_with_mode("terminal", ThemeMode::Light));
        let target = state
            .managed_terminal_theme_targets()
            .into_iter()
            .find(|target| target.terminal_id == terminal_id)
            .expect("managed terminal target");
        assert_eq!(target.resolved_override, None);
    }
}
