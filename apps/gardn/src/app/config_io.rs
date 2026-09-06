use super::App;

impl App {
    pub(super) fn update_config_file<F>(&mut self, error_context: &str, update: F) -> bool
    where
        F: FnOnce(&str) -> String,
    {
        #[cfg(test)]
        if std::env::var_os(crate::config::CONFIG_PATH_ENV_VAR).is_none() {
            return false;
        }

        let path = crate::config::config_path();
        if let Some(parent) = path.parent() {
            if let Err(err) = std::fs::create_dir_all(parent) {
                crate::logging::config_write_failed(&path, error_context, &err.to_string());
                self.state.config_diagnostic =
                    Some(format!("failed to save {error_context}: {err}"));
                self.config_diagnostic_deadline =
                    Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
                return false;
            }
        }

        let content = std::fs::read_to_string(&path).unwrap_or_default();
        let new_content = update(&content);
        if let Err(err) = std::fs::write(&path, new_content) {
            crate::logging::config_write_failed(&path, error_context, &err.to_string());
            self.state.config_diagnostic = Some(format!("failed to save {error_context}: {err}"));
            self.config_diagnostic_deadline =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
            return false;
        }

        true
    }

    pub(super) fn mark_onboarding_complete(&mut self) {
        self.update_config_file("onboarding setting", |content| {
            crate::config::upsert_top_level_bool(content, "onboarding", false)
        });
    }

    pub(super) fn save_theme(
        &mut self,
        light: &str,
        dark: &str,
        mode: crate::config::ThemeMode,
        terminal_light_accent: crate::config::TerminalAccent,
        terminal_dark_accent: crate::config::TerminalAccent,
    ) {
        self.state.global_light_theme_name = light.to_string();
        self.state.global_dark_theme_name = dark.to_string();
        self.state.global_theme_mode = mode;
        self.state.global_terminal_light_accent = terminal_light_accent;
        self.state.global_terminal_dark_accent = terminal_dark_accent;
        self.state.refresh_global_palette();
        self.state.apply_effective_theme();
        self.state.settings.pending_light_theme_name = Some(light.to_string());
        self.state.settings.pending_dark_theme_name = Some(dark.to_string());
        self.state.settings.pending_theme_mode = Some(mode);
        self.state.settings.pending_terminal_light_accent = Some(terminal_light_accent);
        self.state.settings.pending_terminal_dark_accent = Some(terminal_dark_accent);
        if self.update_config_file("theme", |content| {
            let content = crate::config::remove_section_key(content, "theme", "name");
            let content = crate::config::upsert_section_value(
                &content,
                "theme",
                "light",
                &format!("\"{light}\""),
            );
            let content = crate::config::upsert_section_value(
                &content,
                "theme",
                "dark",
                &format!("\"{dark}\""),
            );
            let content = crate::config::upsert_section_value(
                &content,
                "theme",
                "mode",
                &format!("\"{}\"", mode.as_str()),
            );
            let content = crate::config::upsert_section_value(
                &content,
                "theme",
                "terminal_accent",
                &format!("\"{}\"", terminal_dark_accent.as_str()),
            );
            let content = crate::config::upsert_section_value(
                &content,
                "theme",
                "terminal_light_accent",
                &format!("\"{}\"", terminal_light_accent.as_str()),
            );
            crate::config::upsert_section_value(
                &content,
                "theme",
                "terminal_dark_accent",
                &format!("\"{}\"", terminal_dark_accent.as_str()),
            )
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_sound(&mut self, enabled: bool) {
        self.state.sound.enabled = enabled;
        self.state.settings.pending_sound_enabled = Some(enabled);
        if self.update_config_file("sound setting", |content| {
            crate::config::upsert_section_bool(content, "ui.sound", "enabled", enabled)
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_new_terminal_cwd(&mut self, policy: &crate::config::NewTerminalCwdConfig) {
        self.state.new_terminal_cwd = policy.clone();
        self.state.settings.pending_new_terminal_cwd = Some(policy.clone());
        let value = match policy {
            crate::config::NewTerminalCwdConfig::Follow => "\"follow\"".to_string(),
            crate::config::NewTerminalCwdConfig::Home => "\"home\"".to_string(),
            crate::config::NewTerminalCwdConfig::Current => "\"current\"".to_string(),
            crate::config::NewTerminalCwdConfig::Path(path) => format!("{path:?}"),
        };
        if self.update_config_file("new terminal cwd", |content| {
            crate::config::upsert_section_value(content, "terminal", "new_cwd", &value)
        }) {
            self.apply_config_from_disk(false);
        }
    }
    pub(super) fn save_mouse_scroll_lines(&mut self, lines: usize) {
        let lines = lines.max(1);
        self.state.mouse_scroll_lines = lines;
        self.state.settings.pending_mouse_scroll_lines = Some(lines);
        if self.update_config_file("mouse scroll lines", |content| {
            crate::config::upsert_section_value(
                content,
                "ui",
                "mouse_scroll_lines",
                &lines.to_string(),
            )
        }) {
            self.apply_config_from_disk(false);
        }
    }
    pub(super) fn save_resume_agents_on_restore(&mut self, enabled: bool) {
        self.state.resume_agents_on_restore = enabled;
        self.state.settings.pending_resume_agents_on_restore = Some(enabled);
        if self.update_config_file("agent session restore", |content| {
            crate::config::upsert_section_bool(
                content,
                "session",
                "resume_agents_on_restore",
                enabled,
            )
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_window_title(&mut self, template: &str) {
        self.state.window_title_template = template.to_string();
        self.state.settings.pending_window_title = Some(template.to_string());
        let value = toml::Value::String(template.to_string()).to_string();
        if self.update_config_file("window title", |content| {
            crate::config::upsert_section_value(content, "ui", "window_title", &value)
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_headless_size(&mut self, cols: u16, rows: u16) {
        if cols == 0 || rows == 0 {
            return;
        }
        self.state.headless_size = (cols, rows);
        self.state.settings.pending_headless_cols = Some(cols.to_string());
        self.state.settings.pending_headless_rows = Some(rows.to_string());
        if self.update_config_file("headless terminal size", |content| {
            let content = crate::config::upsert_section_value(
                content,
                "server",
                "headless_cols",
                &cols.to_string(),
            );
            crate::config::upsert_section_value(
                &content,
                "server",
                "headless_rows",
                &rows.to_string(),
            )
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_commands(&mut self, browser: &str, review: &str, editor: &str) {
        let commands = crate::config::CommandsConfig {
            browser: browser.trim().to_string(),
            review: review.trim().to_string(),
            editor: editor.trim().to_string(),
        };
        self.state.browser_command.clone_from(&commands.browser);
        self.state.review_command.clone_from(&commands.review);
        self.state.editor_command.clone_from(&commands.editor);
        self.state.settings.pending_browser_command = Some(commands.browser.clone());
        self.state.settings.pending_review_command = Some(commands.review.clone());
        self.state.settings.pending_editor_command = Some(commands.editor.clone());
        if self.update_config_file("project commands", |content| {
            let content = crate::config::remove_section_key(content, "commands", "git");
            let content = crate::config::remove_section_key(&content, "commands", "diff");
            let content = crate::config::remove_section_key(&content, "commands", "ide");
            let content = crate::config::remove_section_key(&content, "commands", "github");
            let content = crate::config::upsert_section_value(
                &content,
                "commands",
                "browser",
                &toml::Value::String(commands.browser.clone()).to_string(),
            );
            let content = crate::config::upsert_section_value(
                &content,
                "commands",
                "review",
                &toml::Value::String(commands.review.clone()).to_string(),
            );
            crate::config::upsert_section_value(
                &content,
                "commands",
                "editor",
                &toml::Value::String(commands.editor.clone()).to_string(),
            )
        }) {
            self.apply_config_from_disk(false);
        }
    }
    pub(super) fn save_sidebar_widths(&mut self, width: u16, min: u16, max: u16) {
        let (min, max) = crate::config::validated_sidebar_bounds(min, max)
            .unwrap_or((self.state.sidebar_min_width, self.state.sidebar_max_width));
        let width = width.clamp(min, max);
        self.state.default_sidebar_width = width;
        if self.state.sidebar_width_source == crate::app::state::SidebarWidthSource::ConfigDefault {
            self.state.sidebar_width = width;
        }
        self.state.sidebar_min_width = min;
        self.state.sidebar_max_width = max;
        self.state.sidebar_width = self.state.sidebar_width.clamp(min, max);
        self.state.settings.pending_sidebar_width = Some(width);
        self.state.settings.pending_sidebar_min_width = Some(min);
        self.state.settings.pending_sidebar_max_width = Some(max);
        if self.update_config_file("sidebar widths", |content| {
            let content = crate::config::upsert_section_value(
                content,
                "ui",
                "sidebar_width",
                &width.to_string(),
            );
            let content = crate::config::upsert_section_value(
                &content,
                "ui",
                "sidebar_min_width",
                &min.to_string(),
            );
            crate::config::upsert_section_value(
                &content,
                "ui",
                "sidebar_max_width",
                &max.to_string(),
            )
        }) {
            self.apply_config_from_disk(false);
        }
    }
    pub(super) fn save_sidebar_arrangement(
        &mut self,
        arrangement: crate::config::SidebarArrangementConfig,
    ) {
        self.state.sidebar_arrangement = arrangement;
        self.state.settings.pending_sidebar_arrangement = Some(arrangement);
        if self.update_config_file("sidebar arrangement", |content| {
            crate::config::upsert_section_value(
                content,
                "ui",
                "sidebar_arrangement",
                &format!("{:?}", arrangement.config_value()),
            )
        }) {
            self.apply_config_from_disk(false);
        }
    }
    pub(super) fn save_context_bar_visibility(
        &mut self,
        visibility: crate::config::ContextBarVisibilityConfig,
    ) {
        self.state.context_bar_visibility = visibility;
        self.state.settings.pending_context_bar_visibility = Some(visibility);
        if self.update_config_file("context bar visibility", |content| {
            crate::config::upsert_section_value(
                content,
                "ui",
                "context_bar",
                &format!("{:?}", visibility.config_value()),
            )
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_sidebar_initial_view(
        &mut self,
        initial_state: crate::config::SidebarInitialStateConfig,
        initial_agent_scope: crate::config::AgentPanelScopeConfig,
    ) {
        self.state.sidebar_config.initial_state = initial_state;
        self.state.sidebar_config.initial_agent_scope = initial_agent_scope;
        self.state.settings.pending_sidebar_initial_state = Some(initial_state);
        self.state.settings.pending_sidebar_initial_agent_scope = Some(initial_agent_scope);
        if self.update_config_file("initial sidebar view", |content| {
            let content = crate::config::upsert_section_value(
                content,
                "ui.sidebar",
                "initial_state",
                &format!("{:?}", initial_state.config_value()),
            );
            crate::config::upsert_section_value(
                &content,
                "ui.sidebar",
                "initial_agent_scope",
                &format!("{:?}", initial_agent_scope.config_value()),
            )
        }) {
            self.apply_config_from_disk(false);
        }
    }
    pub(super) fn save_toast_delivery(&mut self, delivery: crate::config::ToastDelivery) {
        self.state.toast_config.delivery = delivery;
        self.state.settings.pending_toast_delivery = Some(delivery);
        let value = match delivery {
            crate::config::ToastDelivery::Off => "\"off\"",
            crate::config::ToastDelivery::Gardn => "\"gardn\"",
            crate::config::ToastDelivery::Terminal => "\"terminal\"",
            crate::config::ToastDelivery::System => "\"system\"",
        };
        if self.update_config_file("toast setting", |content| {
            let content =
                crate::config::upsert_section_value(content, "ui.toast", "delivery", value);
            crate::config::remove_section_key(&content, "ui.toast", "enabled")
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_default_shell(&mut self, shell: &str) {
        self.state.settings.pending_default_shell = Some(shell.to_string());
        let value = toml::Value::String(shell.to_string()).to_string();
        if self.update_config_file("default shell", |content| {
            crate::config::upsert_section_value(content, "terminal", "default_shell", &value)
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_shell_mode(&mut self, mode: crate::config::ShellModeConfig) {
        self.state.settings.pending_shell_mode = Some(mode);
        let value = match mode {
            crate::config::ShellModeConfig::Auto => "\"auto\"",
            crate::config::ShellModeConfig::Login => "\"login\"",
            crate::config::ShellModeConfig::NonLogin => "\"non_login\"",
        };
        if self.update_config_file("shell mode", |content| {
            crate::config::upsert_section_value(content, "terminal", "shell_mode", value)
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_version_check(&mut self, enabled: bool) {
        self.state.settings.pending_version_check = Some(enabled);
        if self.update_config_file("version check", |content| {
            crate::config::upsert_section_bool(content, "update", "version_check", enabled)
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_manifest_check(&mut self, enabled: bool) {
        self.state.settings.pending_manifest_check = Some(enabled);
        if self.update_config_file("manifest check", |content| {
            crate::config::upsert_section_bool(content, "update", "manifest_check", enabled)
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_toast_delay(&mut self, seconds: u64) {
        if seconds > crate::config::MAX_TOAST_DELAY_SECONDS {
            return;
        }
        self.state.settings.pending_toast_delay = Some(seconds.to_string());
        if self.update_config_file("toast delay", |content| {
            crate::config::upsert_section_value(
                content,
                "ui.toast",
                "delay_seconds",
                &seconds.to_string(),
            )
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_toast_gardn_position(
        &mut self,
        position: crate::config::ToastGardnPosition,
    ) {
        self.state.settings.pending_toast_gardn_position = Some(position);
        let value = match position {
            crate::config::ToastGardnPosition::TopLeft => "\"top-left\"",
            crate::config::ToastGardnPosition::TopRight => "\"top-right\"",
            crate::config::ToastGardnPosition::BottomLeft => "\"bottom-left\"",
            crate::config::ToastGardnPosition::BottomRight => "\"bottom-right\"",
        };
        if self.update_config_file("in-app toast position", |content| {
            crate::config::upsert_section_value(content, "ui.toast.gardn", "position", value)
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_clipboard_toast_enabled(&mut self, enabled: bool) {
        self.state.settings.pending_clipboard_toast_enabled = Some(enabled);
        if self.update_config_file("clipboard toast", |content| {
            crate::config::upsert_section_bool(content, "ui.toast.clipboard", "enabled", enabled)
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_clipboard_toast_position(
        &mut self,
        position: crate::config::ToastClipboardPosition,
    ) {
        self.state.settings.pending_clipboard_toast_position = Some(position);
        let value = match position {
            crate::config::ToastClipboardPosition::TopLeft => "\"top-left\"",
            crate::config::ToastClipboardPosition::TopCenter => "\"top-center\"",
            crate::config::ToastClipboardPosition::TopRight => "\"top-right\"",
            crate::config::ToastClipboardPosition::BottomLeft => "\"bottom-left\"",
            crate::config::ToastClipboardPosition::BottomCenter => "\"bottom-center\"",
            crate::config::ToastClipboardPosition::BottomRight => "\"bottom-right\"",
        };
        if self.update_config_file("clipboard toast position", |content| {
            crate::config::upsert_section_value(content, "ui.toast.clipboard", "position", value)
        }) {
            self.apply_config_from_disk(false);
        }
    }
    pub(super) fn save_confirm_close(&mut self, enabled: bool) {
        self.state.confirm_close = enabled;
        self.state.settings.pending_confirm_close = Some(enabled);
        if self.update_config_file("close confirmation", |content| {
            crate::config::upsert_section_bool(content, "ui", "confirm_close", enabled)
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_prompt_new_tab_name(&mut self, enabled: bool) {
        self.state.prompt_new_tab_name = enabled;
        self.state.settings.pending_prompt_new_tab_name = Some(enabled);
        if self.update_config_file("new tab name prompt", |content| {
            crate::config::upsert_section_bool(content, "ui", "prompt_new_tab_name", enabled)
        }) {
            self.apply_config_from_disk(false);
        }
    }
    pub(super) fn save_show_counters(&mut self, enabled: bool) {
        self.state.show_counters = enabled;
        self.state.settings.pending_show_counters = Some(enabled);
        if self.update_config_file("counter visibility", |content| {
            crate::config::upsert_section_bool(content, "ui", "show_counters", enabled)
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_pane_appearance(
        &mut self,
        pane_borders: bool,
        pane_scrollbars: bool,
        pane_gaps: bool,
        hide_tab_bar_when_single_tab: bool,
    ) {
        self.state.pane_borders = pane_borders;
        self.state.pane_scrollbars = pane_scrollbars;
        self.state.pane_gaps = pane_gaps;
        self.state.hide_tab_bar_when_single_tab = hide_tab_bar_when_single_tab;
        self.state.settings.pending_pane_borders = Some(pane_borders);
        self.state.settings.pending_pane_scrollbars = Some(pane_scrollbars);
        self.state.settings.pending_pane_gaps = Some(pane_gaps);
        self.state.settings.pending_hide_tab_bar_when_single_tab =
            Some(hide_tab_bar_when_single_tab);
        if self.update_config_file("pane appearance", |content| {
            let content =
                crate::config::upsert_section_bool(content, "ui", "pane_borders", pane_borders);
            let content = crate::config::upsert_section_bool(
                &content,
                "ui",
                "pane_scrollbars",
                pane_scrollbars,
            );
            let content =
                crate::config::upsert_section_bool(&content, "ui", "pane_gaps", pane_gaps);
            crate::config::upsert_section_bool(
                &content,
                "ui",
                "hide_tab_bar_when_single_tab",
                hide_tab_bar_when_single_tab,
            )
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_behavior_selection(
        &mut self,
        copy_on_select: bool,
        prompt_new_workspace_name: bool,
        right_click_passthrough_modifier: crate::config::RightClickPassthroughModifierConfig,
    ) {
        self.state.copy_on_select = copy_on_select;
        self.state.prompt_new_workspace_name = prompt_new_workspace_name;
        self.state.right_click_passthrough_modifiers = right_click_passthrough_modifier.modifiers();
        self.state.settings.pending_copy_on_select = Some(copy_on_select);
        self.state.settings.pending_prompt_new_workspace_name = Some(prompt_new_workspace_name);
        self.state.settings.pending_right_click_passthrough_modifier =
            Some(right_click_passthrough_modifier);
        if self.update_config_file("selection behavior", |content| {
            let content =
                crate::config::upsert_section_bool(content, "ui", "copy_on_select", copy_on_select);
            let content = crate::config::upsert_section_bool(
                &content,
                "ui",
                "prompt_new_workspace_name",
                prompt_new_workspace_name,
            );
            crate::config::upsert_section_value(
                &content,
                "ui",
                "right_click_passthrough_modifier",
                &format!("{:?}", right_click_passthrough_modifier.config_value()),
            )
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_pane_border_agent_info(
        &mut self,
        level: crate::config::PaneBorderAgentInfoConfig,
    ) {
        self.state.pane_border_agent_info = level;
        self.state.settings.pending_pane_border_agent_info = Some(level);
        if self.update_config_file("pane border agent info", |content| {
            let content = crate::config::upsert_section_value(
                content,
                "ui",
                "pane_border_agent_info",
                &format!("{:?}", level.config_value()),
            );
            crate::config::remove_section_key(&content, "ui", "show_agent_labels_on_pane_borders")
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_status_indicators(&mut self, style: crate::config::StatusIndicatorStyle) {
        self.state.status_indicators = style;
        self.state.settings.pending_status_indicators = Some(style);
        if self.update_config_file("status indicator style", |content| {
            crate::config::upsert_section_value(
                content,
                "ui",
                "status_indicators",
                &format!("{:?}", style.config_value()),
            )
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_switch_ascii_input_source_in_prefix(&mut self, enabled: bool) {
        self.state.switch_ascii_input_source_in_prefix = enabled;
        self.state
            .settings
            .pending_switch_ascii_input_source_in_prefix = Some(enabled);
        if self.update_config_file("prefix ascii input source", |content| {
            crate::config::upsert_section_bool(
                content,
                "experimental",
                "switch_ascii_input_source_in_prefix",
                enabled,
            )
        }) {
            self.apply_config_from_disk(false);
        }
    }
    pub(super) fn save_kitty_graphics(&mut self, enabled: bool) {
        self.state.kitty_graphics_enabled = enabled;
        if self.update_config_file("Kitty graphics", |content| {
            crate::config::upsert_section_bool(content, "experimental", "kitty_graphics", enabled)
        }) {
            self.apply_config_from_disk(false);
        }
    }

    pub(super) fn save_agent_profile(
        &mut self,
        profile: crate::agent_profiles::UserAgentProfileConfig,
    ) -> bool {
        let mut config = self.current_agent_profiles_config();
        let profile_id = format!("user:{}", profile.id.trim_start_matches("user:"));
        if let Some(existing) = config.custom.iter_mut().find(|existing| {
            format!("user:{}", existing.id.trim_start_matches("user:")) == profile_id
        }) {
            *existing = profile;
        } else {
            config.custom.push(profile);
        }
        if !config.order.iter().any(|id| id == &profile_id) {
            config.order.push(profile_id);
        }
        self.save_agent_profiles_config(config)
    }

    pub(super) fn delete_agent_profile(&mut self, profile_id: &str) {
        let mut config = self.current_agent_profiles_config();
        config.custom.retain(|profile| {
            format!("user:{}", profile.id.trim_start_matches("user:")) != profile_id
        });
        config.order.retain(|id| id != profile_id);
        if !self.save_agent_profiles_config(config) {
            return;
        }
        for group in &mut self.state.groups {
            group
                .favorite_agent_profile_ids
                .retain(|id| id != profile_id);
            if group.default_agent_profile_id.as_deref() == Some(profile_id) {
                group.default_agent_profile_id = None;
            }
        }
        self.state.mark_session_dirty();
        self.state.settings.pending_agent_profile_id = None;
        self.state.settings.pending_agent_profile_name = None;
        self.state.settings.pending_agent_profile_kind =
            Some(crate::agent_profiles::AgentKind::Omp);
        self.state.settings.pending_agent_profile_command = None;
        self.state.settings.pending_agent_profile_enabled = None;
        self.state.settings.list.selected = 0;
        self.state.settings.scroll = 0;
    }

    fn current_agent_profiles_config(&self) -> crate::agent_profiles::AgentProfilesConfig {
        let custom = self
            .state
            .agent_profiles
            .profiles()
            .iter()
            .filter(|profile| !profile.is_system())
            .map(|profile| crate::agent_profiles::UserAgentProfileConfig {
                id: profile.id.trim_start_matches("user:").to_string(),
                name: profile.name.clone(),
                kind: profile.kind,
                command: profile.command.clone(),
                env: profile.env.iter().cloned().collect(),
                enabled: profile.enabled,
            })
            .collect();
        let order = self
            .state
            .agent_profiles
            .profiles()
            .iter()
            .map(|profile| profile.id.clone())
            .collect();
        crate::agent_profiles::AgentProfilesConfig { order, custom }
    }

    fn save_agent_profiles_config(
        &mut self,
        config: crate::agent_profiles::AgentProfilesConfig,
    ) -> bool {
        if self.update_config_file("agent profiles", |content| {
            write_agent_profiles_section(content, &config)
        }) {
            self.apply_config_from_disk(false);
            true
        } else {
            false
        }
    }
}

fn write_agent_profiles_section(
    content: &str,
    config: &crate::agent_profiles::AgentProfilesConfig,
) -> String {
    let mut out = String::new();
    let mut skipping = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(header) = toml_header_name(trimmed) {
            skipping = header == "agent_profiles" || header.starts_with("agent_profiles.");
        }
        if !skipping {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if !out.trim().is_empty() {
        out.push('\n');
    }
    out.push_str("[agent_profiles]\n");
    if !config.order.is_empty() {
        out.push_str("order = [");
        for (idx, id) in config.order.iter().enumerate() {
            if idx > 0 {
                out.push_str(", ");
            }
            out.push('"');
            out.push_str(&escape_toml_string(id));
            out.push('"');
        }
        out.push_str("]\n");
    }
    for profile in &config.custom {
        out.push_str("\n[[agent_profiles.custom]]\n");
        out.push_str("id = \"");
        out.push_str(&escape_toml_string(&profile.id));
        out.push_str("\"\nname = \"");
        out.push_str(&escape_toml_string(&profile.name));
        out.push_str("\"\nkind = \"");
        out.push_str(profile.kind.as_str());
        out.push_str("\"\ncommand = \"");
        out.push_str(&escape_toml_string(&profile.command));
        out.push_str("\"\n");
        if !profile.enabled {
            out.push_str("enabled = false\n");
        }
        if !profile.env.is_empty() {
            out.push_str("\n[agent_profiles.custom.env]\n");
            for (key, value) in &profile.env {
                out.push_str(key);
                out.push_str(" = \"");
                out.push_str(&escape_toml_string(value));
                out.push_str("\"\n");
            }
        }
    }
    out
}

fn toml_header_name(trimmed: &str) -> Option<&str> {
    trimmed
        .strip_prefix("[[")
        .and_then(|value| value.strip_suffix("]]"))
        .or_else(|| {
            trimmed
                .strip_prefix('[')
                .and_then(|value| value.strip_suffix(']'))
        })
}

fn escape_toml_string(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> App {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        App::new(
            &crate::config::Config::default(),
            true,
            None,
            api_rx,
            crate::api::EventHub::default(),
        )
    }

    #[test]
    fn delete_agent_profile_closes_editor_and_updates_catalog() {
        let _lock = match crate::config::test_config_env_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let path = std::env::temp_dir().join(format!(
            "gardn-delete-agent-profile-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _config_path =
            crate::config::TestEnvVar::set(crate::config::CONFIG_PATH_ENV_VAR, &path);
        let mut app = test_app();
        app.state.agent_profiles = crate::agent_profiles::AgentProfileCatalog::from_config(
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
        app.state.settings.pending_agent_profile_id = Some("user:omp-mk".to_string());
        app.state.settings.pending_agent_profile_name = Some("omp mk".to_string());
        app.state.settings.pending_agent_profile_command = Some("omp-mk".to_string());
        app.state.settings.list.selected = 12;

        app.delete_agent_profile("user:omp-mk");

        assert!(app.state.agent_profiles.get("user:omp-mk").is_none());
        assert_eq!(app.state.settings.pending_agent_profile_id, None);
        assert_eq!(app.state.settings.pending_agent_profile_name, None);
        assert_eq!(app.state.settings.pending_agent_profile_command, None);
        assert_eq!(app.state.settings.list.selected, 0);
        assert!(app.state.session_dirty);
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn save_kitty_graphics_persists_experimental_setting() {
        let _lock = match crate::config::test_config_env_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let path = std::env::temp_dir().join(format!(
            "gardn-kitty-graphics-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _config_path =
            crate::config::TestEnvVar::set(crate::config::CONFIG_PATH_ENV_VAR, &path);
        std::fs::write(&path, "[experimental]\npane_history = true\n").unwrap();
        let mut app = test_app();

        app.save_kitty_graphics(true);

        assert!(app.state.kitty_graphics_enabled);
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .contains("kitty_graphics = true"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn save_commands_persists_valid_toml_and_runtime_state() {
        let _lock = match crate::config::test_config_env_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let path = std::env::temp_dir().join(format!(
            "gardn-commands-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _config_path =
            crate::config::TestEnvVar::set(crate::config::CONFIG_PATH_ENV_VAR, &path);
        std::fs::write(
            &path,
            "[commands]\ngit = \"gitui\"\ndiff = \"difft\"\nide = \"hx .\"\n",
        )
        .unwrap();
        let mut app = test_app();

        app.save_commands(
            "  terminal-browser  ",
            r#"  hunk diff --watch --theme auto  "#,
            "  fresh .  ",
        );

        let content = std::fs::read_to_string(&path).unwrap();
        let config: crate::config::Config = toml::from_str(&content).unwrap();
        assert_eq!(config.commands.browser, "terminal-browser");
        assert_eq!(config.commands.review, "hunk diff --watch --theme auto");
        assert_eq!(config.commands.editor, "fresh .");
        assert!(!content.contains("\ngit ="));
        assert!(!content.contains("\ndiff ="));
        assert!(!content.contains("\nide ="));
        assert_eq!(app.state.browser_command, config.commands.browser);
        assert_eq!(app.state.review_command, config.commands.review);
        assert_eq!(app.state.editor_command, config.commands.editor);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn save_commands_persists_an_empty_value_as_disabled() {
        let _lock = match crate::config::test_config_env_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let path = std::env::temp_dir().join(format!(
            "gardn-disabled-command-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _config_path =
            crate::config::TestEnvVar::set(crate::config::CONFIG_PATH_ENV_VAR, &path);
        let mut app = test_app();

        app.save_commands("terminal-browser", "", "fresh .");

        let content = std::fs::read_to_string(&path).unwrap();
        let config: crate::config::Config = toml::from_str(&content).unwrap();
        assert_eq!(config.commands.review, "");
        assert_eq!(app.state.review_command, "");
        assert!(!app
            .state
            .project_command_configured(crate::app::state::ProjectCommandKind::Review));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn save_stable_settings_persist_owned_keys() {
        let _lock = match crate::config::test_config_env_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let path = std::env::temp_dir().join(format!(
            "gardn-stable-settings-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _config_path =
            crate::config::TestEnvVar::set(crate::config::CONFIG_PATH_ENV_VAR, &path);
        std::fs::write(&path, "[ui]\ncopy_on_select = true\n").unwrap();
        let mut app = test_app();

        app.save_default_shell("/bin/zsh");
        app.save_shell_mode(crate::config::ShellModeConfig::NonLogin);
        app.save_version_check(false);
        app.save_manifest_check(false);
        app.save_toast_delay(2);
        app.save_toast_gardn_position(crate::config::ToastGardnPosition::TopLeft);
        app.save_clipboard_toast_enabled(false);
        app.save_clipboard_toast_position(crate::config::ToastClipboardPosition::TopCenter);

        let content = std::fs::read_to_string(&path).unwrap();
        let config: crate::config::Config = toml::from_str(&content).unwrap();
        assert_eq!(config.terminal.default_shell, "/bin/zsh");
        assert_eq!(
            config.terminal.shell_mode,
            crate::config::ShellModeConfig::NonLogin
        );
        assert!(!config.update.version_check);
        assert!(!config.update.manifest_check);
        assert_eq!(config.ui.toast.delay_seconds, 2);
        assert_eq!(
            config.ui.toast.gardn.position,
            crate::config::ToastGardnPosition::TopLeft
        );
        assert!(!config.ui.toast.clipboard.enabled);
        assert_eq!(
            config.ui.toast.clipboard.position,
            crate::config::ToastClipboardPosition::TopCenter
        );
        assert!(config.ui.copy_on_select);
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn failed_stable_setting_writes_preserve_live_state() {
        let _lock = match crate::config::test_config_env_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let path = std::env::temp_dir().join(format!(
            "gardn-unwritable-settings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        let _config_path =
            crate::config::TestEnvVar::set(crate::config::CONFIG_PATH_ENV_VAR, &path);
        let mut app = test_app();
        let original_delay = app.state.toast_config.delay_seconds;
        let original_gardn_position = app.state.toast_config.gardn.position;
        let original_clipboard_enabled = app.state.toast_config.clipboard.enabled;
        let original_clipboard_position = app.state.toast_config.clipboard.position;

        app.save_default_shell("/bin/zsh");
        app.save_shell_mode(crate::config::ShellModeConfig::NonLogin);
        app.save_version_check(false);
        app.save_manifest_check(false);
        app.save_toast_delay(2);
        app.save_toast_gardn_position(crate::config::ToastGardnPosition::TopLeft);
        app.save_clipboard_toast_enabled(false);
        app.save_clipboard_toast_position(crate::config::ToastClipboardPosition::TopCenter);

        assert!(app.state.default_shell.is_empty());
        assert_eq!(app.state.shell_mode, crate::config::ShellModeConfig::Auto);
        assert!(app.state.update_version_check);
        assert!(app.state.update_manifest_check);
        assert_eq!(app.state.toast_config.delay_seconds, original_delay);
        assert_eq!(
            app.state.toast_config.gardn.position,
            original_gardn_position
        );
        assert_eq!(
            app.state.toast_config.clipboard.enabled,
            original_clipboard_enabled
        );
        assert_eq!(
            app.state.toast_config.clipboard.position,
            original_clipboard_position
        );
        assert_eq!(
            app.state.settings.pending_default_shell.as_deref(),
            Some("/bin/zsh")
        );
        assert_eq!(app.state.settings.pending_version_check, Some(false));
        assert_eq!(app.state.settings.pending_toast_delay.as_deref(), Some("2"));
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn failed_agent_profile_write_preserves_catalog() {
        let _lock = match crate::config::test_config_env_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let path = std::env::temp_dir().join(format!(
            "gardn-unwritable-agent-profile-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        let _config_path =
            crate::config::TestEnvVar::set(crate::config::CONFIG_PATH_ENV_VAR, &path);
        let mut app = test_app();
        app.state.agent_profiles = crate::agent_profiles::AgentProfileCatalog::from_config(
            &crate::agent_profiles::AgentProfilesConfig {
                order: vec!["user:quiet".to_string()],
                custom: vec![crate::agent_profiles::UserAgentProfileConfig {
                    id: "quiet".to_string(),
                    name: "quiet".to_string(),
                    kind: crate::agent_profiles::AgentKind::Custom,
                    command: "true".to_string(),
                    env: std::collections::BTreeMap::new(),
                    enabled: true,
                }],
            },
        );

        assert!(
            !app.save_agent_profile(crate::agent_profiles::UserAgentProfileConfig {
                id: "quiet".to_string(),
                name: "quiet".to_string(),
                kind: crate::agent_profiles::AgentKind::Custom,
                command: "true".to_string(),
                env: std::collections::BTreeMap::new(),
                enabled: false,
            })
        );
        assert!(app
            .state
            .agent_profiles
            .get("user:quiet")
            .is_some_and(|profile| profile.enabled));
        let _ = std::fs::remove_dir_all(path);
    }
    #[test]
    fn save_enum_settings_persist_valid_toml() {
        let _lock = match crate::config::test_config_env_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let path = std::env::temp_dir().join(format!(
            "gardn-enum-settings-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _config_path =
            crate::config::TestEnvVar::set(crate::config::CONFIG_PATH_ENV_VAR, &path);
        let mut app = test_app();

        app.save_status_indicators(crate::config::StatusIndicatorStyle::Symbols);
        app.save_context_bar_visibility(crate::config::ContextBarVisibilityConfig::Never);
        app.save_sidebar_initial_view(
            crate::config::SidebarInitialStateConfig::Collapsed,
            crate::config::AgentPanelScopeConfig::Group,
        );

        let content = std::fs::read_to_string(&path).unwrap();
        let config: crate::config::Config = toml::from_str(&content).unwrap();
        assert_eq!(
            config.ui.status_indicators,
            crate::config::StatusIndicatorStyle::Symbols
        );
        assert_eq!(
            config.ui.context_bar,
            crate::config::ContextBarVisibilityConfig::Never
        );
        assert_eq!(
            config.ui.sidebar.initial_state,
            crate::config::SidebarInitialStateConfig::Collapsed
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn save_pane_appearance_and_behavior_selection_persist_valid_toml() {
        let _lock = match crate::config::test_config_env_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let path = std::env::temp_dir().join(format!(
            "gardn-pane-behavior-{}-{}.toml",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _config_path =
            crate::config::TestEnvVar::set(crate::config::CONFIG_PATH_ENV_VAR, &path);
        let mut app = test_app();
        let modifier = crate::config::RightClickPassthroughModifierConfig::from_modifiers(Some(
            crossterm::event::KeyModifiers::SUPER | crossterm::event::KeyModifiers::ALT,
        ));

        app.save_pane_appearance(false, false, true, true);
        app.save_behavior_selection(false, true, modifier);

        let content = std::fs::read_to_string(&path).unwrap();
        let config: crate::config::Config = toml::from_str(&content).unwrap();
        assert!(!config.ui.pane_borders);
        assert!(!config.ui.pane_scrollbars);
        assert!(config.ui.pane_gaps);
        assert!(config.ui.hide_tab_bar_when_single_tab);
        assert!(!config.ui.copy_on_select);
        assert!(config.ui.prompt_new_workspace_name);
        assert_eq!(
            config.ui.right_click_passthrough_modifiers(),
            Some(crossterm::event::KeyModifiers::SUPER | crossterm::event::KeyModifiers::ALT)
        );
        assert!(!app.state.pane_borders);
        assert!(!app.state.pane_scrollbars);
        assert!(app.state.pane_gaps);
        assert!(app.state.hide_tab_bar_when_single_tab);
        assert!(!app.state.copy_on_select);
        assert!(app.state.prompt_new_workspace_name);
        assert_eq!(
            app.state.right_click_passthrough_modifiers,
            Some(crossterm::event::KeyModifiers::SUPER | crossterm::event::KeyModifiers::ALT)
        );
        let _ = std::fs::remove_file(path);
    }
}
