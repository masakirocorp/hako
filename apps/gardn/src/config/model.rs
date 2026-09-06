use std::{collections::BTreeSet, num::NonZeroUsize};

use crossterm::event::KeyModifiers;
use serde::{de, Deserialize, Deserializer, Serialize};

use super::{
    ActionKeybinds, BindingConfig, CommandKeybindConfig, IndexedKeybind, Keybinds, SidebarConfig,
    SoundConfig, ThemeConfig, DEFAULT_MOBILE_WIDTH_THRESHOLD, DEFAULT_MOUSE_SCROLL_LINES,
    DEFAULT_SCROLLBACK_LIMIT_BYTES,
};

pub const MAX_TOAST_DELAY_SECONDS: u64 = 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ToastDelivery {
    #[default]
    Off,
    Gardn,
    Terminal,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HostCursorModeConfig {
    #[default]
    Auto,
    Native,
    Drawn,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum ToastGardnPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ToastClipboardPosition {
    TopLeft,
    TopCenter,
    TopRight,
    BottomLeft,
    #[default]
    BottomCenter,
    BottomRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AgentPanelScopeConfig {
    Current,
    Group,
    #[default]
    All,
}

impl AgentPanelScopeConfig {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::All => Self::Group,
            Self::Group => Self::Current,
            Self::Current => Self::All,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Group => "Current Group",
            Self::Current => "Current Space",
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Group => "group",
            Self::Current => "current",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SidebarArrangementConfig {
    #[default]
    Auto,
    Separate,
    CombinedLeft,
    CombinedRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SidebarCollapsedModeConfig {
    /// Show a narrow compact rail when the sidebar is collapsed.
    #[default]
    Compact,
    /// Hide the collapsed sidebar completely (zero-width).
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PaneBorderAgentInfoConfig {
    #[default]
    Hidden,
    Name,
    NameAndStatus,
}

impl PaneBorderAgentInfoConfig {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Hidden => Self::Name,
            Self::Name => Self::NameAndStatus,
            Self::NameAndStatus => Self::Hidden,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Hidden => "Hidden",
            Self::Name => "Name",
            Self::NameAndStatus => "Name and Status",
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::Name => "name",
            Self::NameAndStatus => "name_and_status",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StatusIndicatorStyle {
    #[default]
    Dots,
    Symbols,
}

impl StatusIndicatorStyle {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Dots => Self::Symbols,
            Self::Symbols => Self::Dots,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Dots => "Dots",
            Self::Symbols => "Symbols",
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        match self {
            Self::Dots => "dots",
            Self::Symbols => "symbols",
        }
    }
}

impl SidebarArrangementConfig {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Auto => Self::Separate,
            Self::Separate => Self::CombinedLeft,
            Self::CombinedLeft => Self::CombinedRight,
            Self::CombinedRight => Self::Auto,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Separate => "Split",
            Self::CombinedLeft => "Left",
            Self::CombinedRight => "Right",
        }
    }

    pub(crate) fn config_value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Separate => "separate",
            Self::CombinedLeft => "combined_left",
            Self::CombinedRight => "combined_right",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ContextBarVisibilityConfig {
    #[default]
    Always,
    Never,
}

impl ContextBarVisibilityConfig {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::Always => Self::Never,
            Self::Never => Self::Always,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Always => "Always",
            Self::Never => "Never",
        }
    }

    pub(crate) const fn config_value(self) -> &'static str {
        match self {
            Self::Always => "always",
            Self::Never => "never",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RightClickPassthroughModifierConfig(Option<KeyModifiers>);

impl RightClickPassthroughModifierConfig {
    const ACCEPTED: [KeyModifiers; 5] = [
        KeyModifiers::CONTROL,
        KeyModifiers::ALT,
        KeyModifiers::SUPER,
        KeyModifiers::META,
        KeyModifiers::HYPER,
    ];

    pub fn modifiers(self) -> Option<KeyModifiers> {
        self.0
    }

    pub(crate) fn from_modifiers(modifiers: Option<KeyModifiers>) -> Self {
        Self(modifiers.and_then(Self::sanitize))
    }

    pub(crate) fn label(self) -> String {
        match self.0 {
            None => "Off".to_string(),
            Some(modifiers) => format_passthrough_modifiers(modifiers, " + "),
        }
    }

    pub(crate) fn config_value(self) -> String {
        match self.0 {
            None => "off".to_string(),
            Some(modifiers) => format_passthrough_modifiers(modifiers, "+").to_ascii_lowercase(),
        }
    }

    pub(crate) fn next(self) -> Self {
        let values = Self::accepted_values();
        let current = values.iter().position(|value| *value == self).unwrap_or(0);
        values[(current + 1) % values.len()]
    }

    fn accepted_values() -> Vec<Self> {
        let mut values = vec![Self(None)];
        for mask in 1u8..(1 << Self::ACCEPTED.len()) {
            let mut modifiers = KeyModifiers::empty();
            for (index, flag) in Self::ACCEPTED.iter().enumerate() {
                if mask & (1 << index) != 0 {
                    modifiers |= *flag;
                }
            }
            values.push(Self(Some(modifiers)));
        }
        values
    }

    fn sanitize(modifiers: KeyModifiers) -> Option<KeyModifiers> {
        let mut cleaned = KeyModifiers::empty();
        for flag in Self::ACCEPTED {
            if modifiers.contains(flag) {
                cleaned |= flag;
            }
        }
        (!cleaned.is_empty()).then_some(cleaned)
    }
}

fn format_passthrough_modifiers(modifiers: KeyModifiers, sep: &str) -> String {
    let mut parts = Vec::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl");
    }
    if modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt");
    }
    if modifiers.contains(KeyModifiers::SUPER) {
        parts.push(match super::keybinds::super_modifier_label() {
            "cmd" => "Cmd",
            _ => "Super",
        });
    }
    if modifiers.contains(KeyModifiers::META) {
        parts.push("Meta");
    }
    if modifiers.contains(KeyModifiers::HYPER) {
        parts.push("Hyper");
    }
    if parts.is_empty() {
        "Off".to_string()
    } else {
        parts.join(sep)
    }
}

impl<'de> Deserialize<'de> for RightClickPassthroughModifierConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        parse_right_click_passthrough_modifier(&value)
            .map(Self)
            .ok_or_else(|| {
                de::Error::custom(
                    "right_click_passthrough_modifier must be empty, off, none, disabled, ctrl/control, alt/option, cmd/command/super, meta, hyper, or a + separated combination without shift",
                )
            })
    }
}

fn parse_right_click_passthrough_modifier(value: &str) -> Option<Option<KeyModifiers>> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("off")
        || trimmed.eq_ignore_ascii_case("none")
        || trimmed.eq_ignore_ascii_case("disabled")
    {
        return Some(None);
    }

    let mut modifiers = KeyModifiers::empty();
    for token in trimmed.split('+') {
        let token = token.trim().to_ascii_lowercase();
        let modifier = match token.as_str() {
            "ctrl" | "control" => KeyModifiers::CONTROL,
            "alt" | "option" => KeyModifiers::ALT,
            "cmd" | "command" | "super" => KeyModifiers::SUPER,
            "meta" => KeyModifiers::META,
            "hyper" => KeyModifiers::HYPER,
            "shift" => return None,
            _ => return None,
        };
        modifiers |= modifier;
    }

    (!modifiers.is_empty()).then_some(Some(modifiers))
}

#[derive(Debug, Clone)]
pub struct ToastConfig {
    pub delivery: ToastDelivery,
    pub delay_seconds: u64,
    pub gardn: GardnToastConfig,
    pub clipboard: ClipboardToastConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct GardnToastConfig {
    pub position: ToastGardnPosition,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct ClipboardToastConfig {
    pub enabled: bool,
    pub position: ToastClipboardPosition,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum NewTerminalCwdConfig {
    #[default]
    Follow,
    Home,
    Current,
    Path(String),
}

impl<'de> Deserialize<'de> for NewTerminalCwdConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.trim() {
            "" | "follow" => Ok(Self::Follow),
            "home" => Ok(Self::Home),
            "current" => Ok(Self::Current),
            _ => Ok(Self::Path(value)),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellModeConfig {
    #[default]
    Auto,
    Login,
    NonLogin,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct TerminalConfig {
    /// Executable used for new interactive panes. Empty means SHELL, then /bin/sh.
    pub default_shell: String,
    /// Startup mode for new interactive pane shells.
    pub shell_mode: ShellModeConfig,
    /// CWD policy for new interactive panes, tabs, and workspaces.
    pub new_cwd: NewTerminalCwdConfig,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    /// Resume supported AI-agent panes into their native conversation sessions
    /// when restoring a Gardn session. Default: true.
    pub resume_agents_on_restore: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            resume_agents_on_restore: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct CommandsConfig {
    /// Browser command launched in the selected workspace directory.
    pub browser: String,
    /// Review command launched in the selected repository root.
    pub review: String,
    /// Editor command launched in the selected workspace directory.
    pub editor: String,
}

impl CommandsConfig {
    pub(crate) const DEFAULT_BROWSER: &str = "terminal-browser";
    pub(crate) const DEFAULT_REVIEW: &str = "hunk diff --watch";
    pub(crate) const DEFAULT_EDITOR: &str = "fresh .";
}

impl Default for CommandsConfig {
    fn default() -> Self {
        Self {
            browser: Self::DEFAULT_BROWSER.to_string(),
            review: Self::DEFAULT_REVIEW.to_string(),
            editor: Self::DEFAULT_EDITOR.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    /// Check GitHub for a newer Gardn release in the background. Default: true.
    pub version_check: bool,
    /// Check for remote agent-detection manifest updates in the background. Default: true.
    pub manifest_check: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            version_check: true,
            manifest_check: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConfigReloadStatus {
    Applied,
    Partial,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ConfigReloadReport {
    pub status: ConfigReloadStatus,
    pub diagnostics: Vec<String>,
}

/// Validate `[ui]` sidebar bound configuration.
///
/// Returns `Some((min, max))` when `min <= max`, `None` otherwise. The two
/// values are funneled through this helper before they reach any
/// `u16::clamp(min, max)` call site (`u16::clamp` panics when `min > max`).
pub fn validated_sidebar_bounds(min: u16, max: u16) -> Option<(u16, u16)> {
    if min <= max {
        Some((min, max))
    } else {
        None
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub onboarding: Option<bool>,
    pub theme: ThemeConfig,
    pub terminal: TerminalConfig,
    pub session: SessionConfig,
    pub server: ServerConfig,
    pub keys: KeysConfig,
    pub ui: UiConfig,
    pub advanced: AdvancedConfig,
    pub commands: CommandsConfig,
    pub update: UpdateConfig,
    pub experimental: ExperimentalConfig,
    pub remote: RemoteConfig,
    pub agent_profiles: crate::agent_profiles::AgentProfilesConfig,
}

#[derive(Debug)]
pub struct LoadedConfig {
    pub config: Config,
    pub diagnostics: Vec<String>,
    pub invalid_sections: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct KeysConfig {
    /// Prefix key to enter prefix mode (e.g. "ctrl+b", "f12", "esc").
    pub prefix: String,
    /// Open keybinding help. Default: "prefix+?"
    pub help: BindingConfig,
    /// Open settings. Default: "prefix+s"
    pub settings: BindingConfig,
    /// Create a new workspace. Default: "prefix+shift+n"
    pub new_workspace: BindingConfig,
    /// Rename the selected workspace. Default: "prefix+shift+w"
    pub rename_workspace: BindingConfig,
    /// Close the selected workspace. Default: "prefix+shift+d"
    pub close_workspace: BindingConfig,
    /// Open the workspace navigation surface. Default: "prefix+w"
    pub workspace_picker: BindingConfig,
    /// Open the session navigator. Default: "prefix+g"
    pub goto: BindingConfig,
    /// Move workspace selection up in navigate mode. Default: "up".
    pub navigate_workspace_up: BindingConfig,
    /// Move workspace selection down in navigate mode. Default: "down".
    pub navigate_workspace_down: BindingConfig,
    /// Focus the pane to the left in navigate mode. Default: "h". Left arrow is always an alias.
    pub navigate_pane_left: BindingConfig,
    /// Focus the pane below in navigate mode. Default: "j".
    pub navigate_pane_down: BindingConfig,
    /// Focus the pane above in navigate mode. Default: "k".
    pub navigate_pane_up: BindingConfig,
    /// Focus the pane to the right in navigate mode. Default: "l". Right arrow is always an alias.
    pub navigate_pane_right: BindingConfig,
    /// Detach from server/client mode, or exit --no-session mode. Default: "prefix+q".
    pub detach: BindingConfig,
    /// Reload config.toml in the running app/server. Default: "prefix+shift+r".
    pub reload_config: BindingConfig,
    /// Focus the currently visible notification target. Default: "prefix+o".
    pub open_notification_target: BindingConfig,
    /// Open the command palette. Default: "prefix+space".
    pub command_palette: BindingConfig,
    /// Select the previous workspace. Unset by default.
    pub previous_workspace: BindingConfig,
    /// Select the next workspace. Unset by default.
    pub next_workspace: BindingConfig,
    /// Open the sidebar group menu. Unset by default.
    pub open_group_menu: BindingConfig,
    /// Create a new group. Unset by default.
    pub new_group: BindingConfig,
    /// Rename the active group. Unset by default.
    pub rename_group: BindingConfig,
    /// Delete the active group. Unset by default.
    pub delete_group: BindingConfig,
    /// Toggle current group/all groups filtering. Unset by default.
    pub toggle_group_filter: BindingConfig,
    /// Focus the previous group. Unset by default.
    pub previous_group: BindingConfig,
    /// Focus the next group. Unset by default.
    pub next_group: BindingConfig,
    /// Switch to group 1-10 from prefix mode. Default: "prefix+alt+1..0".
    pub switch_group: BindingConfig,
    /// Focus the previous agent shown in the agent panel. Unset by default.
    pub previous_agent: BindingConfig,
    /// Focus the next agent shown in the agent panel. Unset by default.
    pub next_agent: BindingConfig,
    /// Open the agent scope menu. Unset by default.
    pub open_agent_menu: BindingConfig,
    /// Open the context menu for the focused pane or agent. Default: "shift+f10".
    pub open_context_menu: BindingConfig,
    /// Focus an agent by index 1-9. Unset by default.
    pub focus_agent: BindingConfig,
    /// Local-client shortcut that sends a clipboard image to a remote Gardn session. Default: "ctrl+v".
    pub remote_image_paste: String,
    /// Create a new tab in the active workspace. Default: "prefix+c"
    pub new_tab: BindingConfig,
    /// Request control of the active tab from another client. Default: "prefix+t".
    pub take_control: BindingConfig,
    /// Rename the active tab. Default: "prefix+shift+t".
    pub rename_tab: BindingConfig,
    /// Select the previous tab. Default: "prefix+p".
    pub previous_tab: BindingConfig,
    /// Select the next tab. Default: "prefix+n".
    pub next_tab: BindingConfig,
    /// Switch to tab 1-10. Default: "prefix+1..0".
    pub switch_tab: BindingConfig,
    /// Switch to workspace 1-10 from prefix mode. Default: "prefix+shift+1..0".
    pub switch_workspace: BindingConfig,
    /// Close the active tab. Default: "prefix+shift+x".
    pub close_tab: BindingConfig,
    /// Rename the focused pane. Default: "prefix+shift+p".
    pub rename_pane: BindingConfig,
    /// Open the focused pane scrollback in $EDITOR. Default: "prefix+e".
    pub edit_scrollback: BindingConfig,
    /// Enter keyboard copy mode for the focused pane. Default: "prefix+[".
    pub copy_mode: BindingConfig,
    /// Focus the pane to the left. Default: "prefix+h".
    pub focus_pane_left: BindingConfig,
    /// Toggle the bottom context bar for this client. Default: "prefix+down".
    pub toggle_context_bar: BindingConfig,
    /// Toggle Zen mode for this client. Default: "prefix+shift+z".
    pub zen_mode: BindingConfig,
    /// Focus the pane below. Default: "prefix+j".
    pub focus_pane_down: BindingConfig,
    /// Focus the pane above. Default: "prefix+k".
    pub focus_pane_up: BindingConfig,
    /// Focus the pane to the right. Default: "prefix+l".
    pub focus_pane_right: BindingConfig,
    /// Cycle to the next pane. Default: "prefix+tab".
    pub cycle_pane_next: BindingConfig,
    /// Cycle to the previous pane. Default: "prefix+shift+tab".
    pub cycle_pane_previous: BindingConfig,
    /// Focus the last focused pane across workspaces and tabs. Unset by default.
    pub last_pane: BindingConfig,
    /// Split pane vertically (side by side). Default: "prefix+v"
    pub split_vertical: BindingConfig,
    /// Split pane horizontally (stacked). Default: "prefix+minus"
    pub split_horizontal: BindingConfig,
    /// Close the focused pane. Default: "prefix+x"
    pub close_pane: BindingConfig,
    /// Toggle zoom for the focused pane. Default: "prefix+z"
    #[serde(alias = "fullscreen")]
    pub zoom: BindingConfig,
    /// Enter resize mode. Default: "prefix+r"
    pub resize_mode: BindingConfig,
    /// Toggle sidebar collapse. Default: "prefix+b"
    pub toggle_sidebar: BindingConfig,
    /// Toggle right sidebar collapse. Unset by default.
    pub toggle_right_sidebar: BindingConfig,
    /// Optional indexed shortcuts expanded over number keys 1-9.
    pub indexed: IndexedKeysConfig,
    /// Prefix-mode custom command bindings.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub command: Vec<CommandKeybindConfig>,
    #[serde(skip_serializing)]
    pub(crate) user_fields: BTreeSet<&'static str>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct KeysConfigOverlay {
    #[serde(skip_serializing_if = "Option::is_none")]
    prefix: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    help: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    settings: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_workspace: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rename_workspace: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    close_workspace: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    workspace_picker: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    goto: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    navigate_workspace_up: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    navigate_workspace_down: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    navigate_pane_left: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    navigate_pane_down: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    navigate_pane_up: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    navigate_pane_right: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detach: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reload_config: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    open_notification_target: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command_palette: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_workspace: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_workspace: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    open_group_menu: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_group: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rename_group: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delete_group: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    toggle_group_filter: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_group: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_group: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    switch_group: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_agent: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_agent: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    open_agent_menu: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    open_context_menu: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus_agent: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    remote_image_paste: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_tab: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    take_control: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rename_tab: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    previous_tab: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_tab: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    switch_tab: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    switch_workspace: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    close_tab: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rename_pane: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    edit_scrollback: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    copy_mode: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    toggle_context_bar: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    zen_mode: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus_pane_left: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus_pane_down: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus_pane_up: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus_pane_right: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cycle_pane_next: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cycle_pane_previous: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_pane: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    split_vertical: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    split_horizontal: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    close_pane: Option<BindingConfig>,
    #[serde(alias = "fullscreen", skip_serializing_if = "Option::is_none")]
    zoom: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resize_mode: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    toggle_sidebar: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    toggle_right_sidebar: Option<BindingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    indexed: Option<IndexedKeysConfig>,
    #[serde(skip_serializing)]
    command: Option<Vec<CommandKeybindConfig>>,
}

impl<'de> Deserialize<'de> for KeysConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let input = KeysConfigOverlay::deserialize(deserializer)?;
        let mut keys = KeysConfig::default();

        macro_rules! apply_field {
            ($field:ident) => {
                if let Some(value) = input.$field {
                    keys.$field = value;
                    keys.user_fields.insert(stringify!($field));
                }
            };
        }

        apply_field!(prefix);
        apply_field!(help);
        apply_field!(settings);
        apply_field!(new_workspace);
        apply_field!(rename_workspace);
        apply_field!(close_workspace);
        apply_field!(workspace_picker);
        apply_field!(goto);
        apply_field!(navigate_workspace_up);
        apply_field!(navigate_workspace_down);
        apply_field!(navigate_pane_left);
        apply_field!(navigate_pane_down);
        apply_field!(navigate_pane_up);
        apply_field!(navigate_pane_right);
        apply_field!(detach);
        apply_field!(reload_config);
        apply_field!(open_notification_target);
        apply_field!(command_palette);
        apply_field!(previous_workspace);
        apply_field!(next_workspace);
        apply_field!(open_group_menu);
        apply_field!(new_group);
        apply_field!(rename_group);
        apply_field!(delete_group);
        apply_field!(toggle_group_filter);
        apply_field!(previous_group);
        apply_field!(next_group);
        apply_field!(switch_group);
        apply_field!(previous_agent);
        apply_field!(next_agent);
        apply_field!(toggle_context_bar);
        apply_field!(zen_mode);
        apply_field!(open_agent_menu);
        apply_field!(open_context_menu);
        apply_field!(focus_agent);
        apply_field!(remote_image_paste);
        apply_field!(new_tab);
        apply_field!(take_control);
        apply_field!(rename_tab);
        apply_field!(previous_tab);
        apply_field!(next_tab);
        apply_field!(switch_tab);
        apply_field!(switch_workspace);
        apply_field!(close_tab);
        apply_field!(rename_pane);
        apply_field!(edit_scrollback);
        apply_field!(copy_mode);
        apply_field!(focus_pane_left);
        apply_field!(focus_pane_down);
        apply_field!(focus_pane_up);
        apply_field!(focus_pane_right);
        apply_field!(cycle_pane_next);
        apply_field!(cycle_pane_previous);
        apply_field!(last_pane);
        apply_field!(split_vertical);
        apply_field!(split_horizontal);
        apply_field!(close_pane);
        apply_field!(zoom);
        apply_field!(resize_mode);
        apply_field!(toggle_sidebar);
        apply_field!(toggle_right_sidebar);
        apply_field!(indexed);
        apply_field!(command);

        Ok(keys)
    }
}

impl KeysConfig {
    pub(crate) fn key_field_is_user_configured(&self, field: &str) -> bool {
        self.user_fields.contains(field)
    }

    pub(crate) fn local_profile(&self, keybinds: &Keybinds) -> KeysConfigOverlay {
        let mut profile = KeysConfigOverlay::default();

        macro_rules! copy_effective_action_field {
            ($field:ident, $target:expr) => {
                if self.user_fields.contains(stringify!($field)) {
                    profile.$field = Some(self.$field.clone());
                } else if binding_config_is_effective(&self.$field, &$target) {
                    profile.$field = Some(self.$field.clone());
                } else if binding_config_has_values(&self.$field) {
                    profile.$field = Some(BindingConfig::empty());
                }
            };
        }
        macro_rules! copy_effective_indexed_field {
            ($field:ident, $target:expr) => {
                if let Some(effective) = effective_indexed_config(&self.$field, &$target) {
                    profile.$field = Some(effective);
                } else if binding_config_has_values(&self.$field) {
                    profile.$field = Some(BindingConfig::empty());
                }
            };
        }

        profile.prefix = Some(self.prefix.clone());
        profile.remote_image_paste = Some(self.remote_image_paste.clone());
        copy_effective_action_field!(help, keybinds.help);
        copy_effective_action_field!(settings, keybinds.settings);
        copy_effective_action_field!(new_workspace, keybinds.new_workspace);
        copy_effective_action_field!(rename_workspace, keybinds.rename_workspace);
        copy_effective_action_field!(close_workspace, keybinds.close_workspace);
        copy_effective_action_field!(workspace_picker, keybinds.workspace_picker);
        copy_effective_action_field!(goto, keybinds.goto);
        copy_effective_action_field!(navigate_workspace_up, keybinds.navigate.workspace_up);
        copy_effective_action_field!(navigate_workspace_down, keybinds.navigate.workspace_down);
        copy_effective_action_field!(navigate_pane_left, keybinds.navigate.pane_left);
        copy_effective_action_field!(navigate_pane_down, keybinds.navigate.pane_down);
        copy_effective_action_field!(navigate_pane_up, keybinds.navigate.pane_up);
        copy_effective_action_field!(navigate_pane_right, keybinds.navigate.pane_right);
        copy_effective_action_field!(detach, keybinds.detach);
        copy_effective_action_field!(reload_config, keybinds.reload_config);
        copy_effective_action_field!(open_notification_target, keybinds.open_notification_target);
        copy_effective_action_field!(command_palette, keybinds.command_palette);
        copy_effective_action_field!(previous_workspace, keybinds.previous_workspace);
        copy_effective_action_field!(next_workspace, keybinds.next_workspace);
        copy_effective_action_field!(open_group_menu, keybinds.open_group_menu);
        copy_effective_action_field!(new_group, keybinds.new_group);
        copy_effective_action_field!(rename_group, keybinds.rename_group);
        copy_effective_action_field!(delete_group, keybinds.delete_group);
        copy_effective_action_field!(toggle_group_filter, keybinds.toggle_group_filter);
        copy_effective_action_field!(previous_group, keybinds.previous_group);
        copy_effective_action_field!(next_group, keybinds.next_group);
        copy_effective_indexed_field!(switch_group, keybinds.switch_group);
        copy_effective_action_field!(previous_agent, keybinds.previous_agent);
        copy_effective_action_field!(toggle_context_bar, keybinds.toggle_context_bar);
        copy_effective_action_field!(zen_mode, keybinds.zen_mode);
        copy_effective_action_field!(next_agent, keybinds.next_agent);
        copy_effective_action_field!(open_agent_menu, keybinds.open_agent_menu);
        copy_effective_action_field!(open_context_menu, keybinds.open_context_menu);
        copy_effective_indexed_field!(focus_agent, keybinds.focus_agent);
        copy_effective_action_field!(new_tab, keybinds.new_tab);
        copy_effective_action_field!(take_control, keybinds.take_control);
        copy_effective_action_field!(rename_tab, keybinds.rename_tab);
        copy_effective_action_field!(previous_tab, keybinds.previous_tab);
        copy_effective_action_field!(next_tab, keybinds.next_tab);
        copy_effective_indexed_field!(switch_tab, keybinds.switch_tab);
        copy_effective_indexed_field!(switch_workspace, keybinds.switch_workspace);
        copy_effective_action_field!(close_tab, keybinds.close_tab);
        copy_effective_action_field!(rename_pane, keybinds.rename_pane);
        copy_effective_action_field!(edit_scrollback, keybinds.edit_scrollback);
        copy_effective_action_field!(copy_mode, keybinds.copy_mode);
        copy_effective_action_field!(focus_pane_left, keybinds.focus_pane_left);
        copy_effective_action_field!(focus_pane_down, keybinds.focus_pane_down);
        copy_effective_action_field!(focus_pane_up, keybinds.focus_pane_up);
        copy_effective_action_field!(focus_pane_right, keybinds.focus_pane_right);
        copy_effective_action_field!(cycle_pane_next, keybinds.cycle_pane_next);
        copy_effective_action_field!(cycle_pane_previous, keybinds.cycle_pane_previous);
        copy_effective_action_field!(last_pane, keybinds.last_pane);
        copy_effective_action_field!(split_vertical, keybinds.split_vertical);
        copy_effective_action_field!(split_horizontal, keybinds.split_horizontal);
        copy_effective_action_field!(close_pane, keybinds.close_pane);
        copy_effective_action_field!(zoom, keybinds.zoom);
        copy_effective_action_field!(resize_mode, keybinds.resize_mode);
        copy_effective_action_field!(toggle_sidebar, keybinds.toggle_sidebar);
        copy_effective_action_field!(toggle_right_sidebar, keybinds.toggle_right_sidebar);
        if self.user_fields.contains("indexed") {
            let mut indexed = self.indexed.clone();
            if let Some(effective) =
                effective_legacy_indexed_config(&indexed.tabs, &keybinds.switch_tab)
            {
                indexed.tabs.clear();
                profile.switch_tab = Some(effective);
            }
            if let Some(effective) =
                effective_legacy_indexed_config(&indexed.workspaces, &keybinds.switch_workspace)
            {
                indexed.workspaces.clear();
                profile.switch_workspace = Some(effective);
            }
            if let Some(effective) =
                effective_legacy_indexed_config(&indexed.agents, &keybinds.focus_agent)
            {
                indexed.agents.clear();
                profile.focus_agent = Some(effective);
            }
            profile.indexed = Some(indexed);
        }

        profile
    }
}
fn binding_config_has_values(config: &BindingConfig) -> bool {
    config.has_values()
}

fn binding_config_is_effective(config: &BindingConfig, keybinds: &ActionKeybinds) -> bool {
    !binding_config_has_values(config) || !keybinds.bindings.is_empty()
}

fn effective_indexed_config(
    config: &BindingConfig,
    keybinds: &[IndexedKeybind],
) -> Option<BindingConfig> {
    if !binding_config_has_values(config) {
        return Some(config.clone());
    }

    let expected_labels = config.indexed_labels();
    if expected_labels.is_empty() {
        return None;
    }

    let effective_labels: Vec<String> = expected_labels
        .iter()
        .filter(|expected| {
            keybinds
                .iter()
                .any(|binding| binding.label.as_str() == expected.as_str())
        })
        .cloned()
        .collect();

    if effective_labels.is_empty() {
        None
    } else if effective_labels.len() == expected_labels.len() {
        Some(config.clone())
    } else {
        Some(BindingConfig::Many(effective_labels))
    }
}

fn effective_legacy_indexed_config(
    configured_label: &str,
    keybinds: &[IndexedKeybind],
) -> Option<BindingConfig> {
    let expected_labels = super::keybinds::legacy_indexed_labels(configured_label)?;
    let effective_labels: Vec<String> = expected_labels
        .iter()
        .filter(|expected| {
            keybinds
                .iter()
                .any(|binding| binding.label.as_str() == expected.as_str())
        })
        .cloned()
        .collect();

    if effective_labels.len() == expected_labels.len() {
        None
    } else if effective_labels.is_empty() {
        Some(BindingConfig::empty())
    } else {
        Some(BindingConfig::Many(effective_labels))
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct IndexedKeysConfig {
    /// Modifier combo for tab shortcuts 1-9. Unset by default.
    pub tabs: String,
    /// Modifier combo for workspace shortcuts 1-9. Unset by default.
    pub workspaces: String,
    /// Modifier combo for agent shortcuts 1-9. Unset by default.
    pub agents: String,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct ServerConfig {
    /// Virtual terminal width used when no client is attached. Default: 120.
    pub headless_cols: u16,
    /// Virtual terminal height used when no client is attached. Default: 40.
    pub headless_rows: u16,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            headless_cols: crate::config::DEFAULT_HEADLESS_COLS,
            headless_rows: crate::config::DEFAULT_HEADLESS_ROWS,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub sidebar_width: u16,
    /// Minimum sidebar width (columns) when expanded. Default: 18.
    /// Host cursor policy. Auto draws on Windows and WSL.
    pub host_cursor: HostCursorModeConfig,
    pub sidebar_min_width: u16,
    /// Maximum sidebar width (columns) when expanded. Default: 36.
    pub sidebar_max_width: u16,
    /// Terminal width at or below which Gardn uses the mobile single-column layout. Default: 64.
    pub mobile_width_threshold: u16,
    /// Sidebar arrangement on desktop: auto, separate, combined_left, or combined_right.
    pub sidebar_arrangement: SidebarArrangementConfig,
    /// Bottom context bar visibility.
    pub context_bar: ContextBarVisibilityConfig,
    /// Configurable rows and metadata tokens for spaces and agents.
    pub sidebar: SidebarConfig,
    /// Capture mouse input for Gardn's mouse UI. Default: true.
    pub mouse_capture: bool,
    /// Automatically copy text selected by mouse drag or double-click. When disabled, Ctrl+C or a host-forwarded Cmd+C copies and clears the retained selection. Default: true.
    pub copy_on_select: bool,
    /// Modifier that lets right-click gestures pass through to pane apps. Empty disables it.
    pub right_click_passthrough_modifier: RightClickPassthroughModifierConfig,
    /// Force a full host-terminal redraw when the outer terminal regains focus. Default: true.
    pub redraw_on_focus_gained: bool,
    /// Lines to scroll per mouse wheel notch. Default: 3.
    pub mouse_scroll_lines: Option<NonZeroUsize>,
    /// Ask for confirmation before closing a workspace. Default: true.
    pub confirm_close: bool,
    /// Ask for a tab name before creating a new tab. Default: true.
    pub prompt_new_tab_name: bool,
    /// Ask for a workspace name before interactive creation. Default: false.
    pub prompt_new_workspace_name: bool,
    /// Agent metadata shown in split pane borders when no title or manual name is set.
    pub pane_border_agent_info: PaneBorderAgentInfoConfig,
    /// Status marks for Space rows and Agent group headers. Dots is the default.
    pub status_indicators: StatusIndicatorStyle,
    /// Format for the outer terminal window title. Empty leaves the title alone.
    /// Default: "{hostname}: {workspace}".
    pub window_title: String,
    /// Optional display name for the coordinator's execution host. Empty uses the machine hostname.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub coordinator_display_name: String,
    /// Draw borders around split panes. Default: true.
    pub pane_borders: bool,
    /// Draw interactive scrollbars beside terminal panes. Default: true.
    pub pane_scrollbars: bool,
    /// Keep split panes visually separated instead of sharing divider borders. Default: true.
    pub pane_gaps: bool,
    /// Hide the tab row when the active workspace has exactly one tab. Default: false.
    pub hide_tab_bar_when_single_tab: bool,
    /// Show right-aligned topology and section counters. Default: false.
    pub show_counters: bool,
    /// How to render the collapsed sidebar. Default: "compact".
    pub sidebar_collapsed_mode: SidebarCollapsedModeConfig,
    /// Optional visual toast notifications for background workspace events.
    pub toast: ToastConfig,
    /// Play sounds when agents change state in background workspaces.
    pub sound: SoundConfig,
}

/// Cursor shape (DECSCUSR) used for the forced IME anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImeCursorShape {
    Block,
    #[default]
    SteadyBlock,
    Underline,
    SteadyUnderline,
    Bar,
    SteadyBar,
}

impl ImeCursorShape {
    /// Convert to DECSCUSR parameter (1–6).
    pub fn to_decscusr(self) -> u8 {
        match self {
            Self::Block => 1,
            Self::SteadyBlock => 2,
            Self::Underline => 3,
            Self::SteadyUnderline => 4,
            Self::Bar => 5,
            Self::SteadyBar => 6,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct AdvancedConfig {
    /// Maximum scrollback buffer size in bytes retained per pane terminal. Default: 10000000.
    #[serde(alias = "scrollback_lines")]
    pub scrollback_limit_bytes: usize,
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct RemoteConfig {
    /// Add a keepalive fallback under the user's ssh config for the `--remote`
    /// bridge. Set false to run plain ssh unchanged. Default: true.
    pub manage_ssh_config: bool,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            manage_ssh_config: true,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct ExperimentalConfig {
    /// Allow launching Gardn inside an existing Gardn pane. Default: false.
    pub allow_nested: bool,
    /// Experimental local Kitty graphics rendering for attached clients. Default: true.
    pub kitty_graphics: bool,
    /// Persist pane screen history to session-history.json. Default: true.
    pub pane_history: bool,
    /// Expose the focused pane's cursor anchor to the outer terminal even when
    /// the pane requested `?25l`, so macOS native input methods keep tracking
    /// the candidate window when TUIs paint their own cursor (Claude Code, pi,
    /// codex, etc.). Default: false.
    ///
    /// When the pane reports no cursor position, falls back to the pane's
    /// top-left so a stable IME anchor is always available.
    ///
    /// Trade-off when enabled: an extra hardware cursor will be visible in the
    /// outer terminal for apps that hide the cursor without painting a
    /// replacement (vim normal mode, etc.). See #149.
    pub reveal_hidden_cursor_for_cjk_ime: bool,
    /// Restrict `reveal_hidden_cursor_for_cjk_ime` to focused panes whose
    /// detected agent matches one of these names (case-insensitive). Empty
    /// list means apply to any focused pane. Unknown agent names are ignored;
    /// if the list contains no valid names, the reveal does not apply.
    /// Accepted names: pi, claude, codex, gemini, cursor, cline, opencode,
    /// copilot, devin, kimi, kiro, droid, amp, grok, hermes, kilo, qwen, qwen-code, mastracode, qodercli, qoder, maki.
    /// Default: empty.
    pub cjk_ime_agents: Vec<String>,
    /// Cursor shape rendered for the IME anchor when
    /// `reveal_hidden_cursor_for_cjk_ime` is enabled. Default: "steady_block".
    pub cjk_ime_cursor_shape: ImeCursorShape,
    /// While prefix mode is active, temporarily switch the host input source
    /// to an ASCII-capable mode so prefix commands are read as ASCII even when
    /// an IME is active, then restore the previous input source when prefix
    /// mode exits. On macOS this selects the ASCII-capable keyboard layout; on
    /// Windows it switches the IME to English (ASCII) input. Windows support is
    /// currently limited to the Korean IME; with an IME for any other language,
    /// the input source is left unchanged. macOS and Windows only; a no-op
    /// elsewhere and a best-effort no-op if the switch fails.
    /// Default: false.
    pub switch_ascii_input_source_in_prefix: bool,
}

impl Default for ExperimentalConfig {
    fn default() -> Self {
        Self {
            allow_nested: false,
            kitty_graphics: true,
            pane_history: true,
            reveal_hidden_cursor_for_cjk_ime: false,
            cjk_ime_agents: Vec::new(),
            cjk_ime_cursor_shape: ImeCursorShape::default(),
            switch_ascii_input_source_in_prefix: false,
        }
    }
}

impl Default for KeysConfig {
    fn default() -> Self {
        Self {
            prefix: "ctrl+b".into(),
            help: BindingConfig::one("prefix+?"),
            settings: BindingConfig::one("prefix+s"),
            new_workspace: BindingConfig::one("prefix+shift+n"),
            rename_workspace: BindingConfig::one("prefix+shift+w"),
            close_workspace: BindingConfig::one("prefix+shift+d"),
            workspace_picker: BindingConfig::one("prefix+w"),
            goto: BindingConfig::one("prefix+g"),
            navigate_workspace_up: BindingConfig::one("up"),
            navigate_workspace_down: BindingConfig::one("down"),
            navigate_pane_left: BindingConfig::one("h"),
            navigate_pane_down: BindingConfig::one("j"),
            navigate_pane_up: BindingConfig::one("k"),
            navigate_pane_right: BindingConfig::one("l"),
            detach: BindingConfig::one("prefix+q"),
            reload_config: BindingConfig::one("prefix+shift+r"),
            open_notification_target: BindingConfig::one("prefix+o"),
            command_palette: BindingConfig::one("prefix+space"),
            previous_workspace: BindingConfig::empty(),
            next_workspace: BindingConfig::empty(),
            open_group_menu: BindingConfig::empty(),
            new_group: BindingConfig::empty(),
            rename_group: BindingConfig::empty(),
            delete_group: BindingConfig::empty(),
            toggle_group_filter: BindingConfig::empty(),
            previous_group: BindingConfig::empty(),
            next_group: BindingConfig::empty(),
            switch_group: BindingConfig::one("prefix+alt+1..0"),
            toggle_context_bar: BindingConfig::one("prefix+down"),
            zen_mode: BindingConfig::one("prefix+shift+z"),
            previous_agent: BindingConfig::empty(),
            next_agent: BindingConfig::empty(),
            open_agent_menu: BindingConfig::empty(),
            open_context_menu: BindingConfig::one("shift+f10"),
            focus_agent: BindingConfig::empty(),
            remote_image_paste: "ctrl+v".into(),
            new_tab: BindingConfig::one("prefix+c"),
            take_control: BindingConfig::one("prefix+t"),
            rename_tab: BindingConfig::one("prefix+shift+t"),
            previous_tab: BindingConfig::one("prefix+p"),
            next_tab: BindingConfig::one("prefix+n"),
            switch_tab: BindingConfig::one("prefix+1..0"),
            switch_workspace: BindingConfig::one("prefix+shift+1..0"),
            close_tab: BindingConfig::one("prefix+shift+x"),
            rename_pane: BindingConfig::one("prefix+shift+p"),
            edit_scrollback: BindingConfig::one("prefix+e"),
            copy_mode: BindingConfig::one("prefix+["),
            focus_pane_left: BindingConfig::one("prefix+h"),
            focus_pane_down: BindingConfig::one("prefix+j"),
            focus_pane_up: BindingConfig::one("prefix+k"),
            focus_pane_right: BindingConfig::one("prefix+l"),
            cycle_pane_next: BindingConfig::one("prefix+tab"),
            cycle_pane_previous: BindingConfig::one("prefix+shift+tab"),
            last_pane: BindingConfig::empty(),
            split_vertical: BindingConfig::one("prefix+v"),
            split_horizontal: BindingConfig::one("prefix+minus"),
            close_pane: BindingConfig::one("prefix+x"),
            zoom: BindingConfig::one("prefix+z"),
            resize_mode: BindingConfig::one("prefix+r"),
            toggle_sidebar: BindingConfig::one("prefix+b"),
            toggle_right_sidebar: BindingConfig::empty(),
            indexed: IndexedKeysConfig::default(),
            command: Vec::new(),
            user_fields: BTreeSet::new(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            sidebar_width: 26,
            sidebar_min_width: 18,
            sidebar_max_width: 36,
            mobile_width_threshold: DEFAULT_MOBILE_WIDTH_THRESHOLD,
            sidebar_arrangement: SidebarArrangementConfig::Auto,
            context_bar: ContextBarVisibilityConfig::Always,
            sidebar: SidebarConfig::default(),
            mouse_capture: true,
            copy_on_select: true,
            host_cursor: HostCursorModeConfig::Auto,
            right_click_passthrough_modifier: RightClickPassthroughModifierConfig::default(),
            redraw_on_focus_gained: true,
            mouse_scroll_lines: None,
            confirm_close: true,
            prompt_new_tab_name: true,
            prompt_new_workspace_name: false,
            coordinator_display_name: String::new(),
            pane_border_agent_info: PaneBorderAgentInfoConfig::default(),
            status_indicators: StatusIndicatorStyle::default(),
            window_title: super::window_title::default_window_title(),
            pane_borders: true,
            pane_scrollbars: true,
            pane_gaps: true,
            hide_tab_bar_when_single_tab: false,
            show_counters: false,
            sidebar_collapsed_mode: SidebarCollapsedModeConfig::default(),
            toast: ToastConfig::default(),
            sound: SoundConfig::default(),
        }
    }
}

impl UiConfig {
    pub fn mouse_scroll_lines(&self) -> usize {
        self.mouse_scroll_lines
            .map(NonZeroUsize::get)
            .unwrap_or(DEFAULT_MOUSE_SCROLL_LINES)
    }

    pub fn right_click_passthrough_modifiers(&self) -> Option<KeyModifiers> {
        self.right_click_passthrough_modifier.modifiers()
    }
}

impl Default for ToastConfig {
    fn default() -> Self {
        Self {
            delivery: ToastDelivery::Off,
            delay_seconds: 1,
            gardn: GardnToastConfig::default(),
            clipboard: ClipboardToastConfig::default(),
        }
    }
}

impl Default for GardnToastConfig {
    fn default() -> Self {
        Self {
            position: ToastGardnPosition::BottomRight,
        }
    }
}

impl Default for ClipboardToastConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            position: ToastClipboardPosition::BottomCenter,
        }
    }
}

impl<'de> Deserialize<'de> for ToastConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct RawToastConfig {
            delivery: Option<ToastDelivery>,
            enabled: Option<bool>,
            delay_seconds: Option<u64>,
            gardn: GardnToastConfig,
            clipboard: ClipboardToastConfig,
        }

        let raw = RawToastConfig::deserialize(deserializer)?;
        let legacy_delivery = match raw.enabled {
            Some(true) => ToastDelivery::Gardn,
            Some(false) | None => ToastDelivery::Off,
        };
        let delivery = raw.delivery.unwrap_or(legacy_delivery);
        let default = Self::default();
        let delay_seconds = raw.delay_seconds.unwrap_or(default.delay_seconds);
        if delay_seconds > MAX_TOAST_DELAY_SECONDS {
            return Err(de::Error::custom(format!(
                "ui.toast.delay_seconds must be between 0 and {MAX_TOAST_DELAY_SECONDS}"
            )));
        }
        Ok(Self {
            delivery,
            delay_seconds,
            gardn: raw.gardn,
            clipboard: raw.clipboard,
        })
    }
}

impl Default for AdvancedConfig {
    fn default() -> Self {
        Self {
            scrollback_limit_bytes: DEFAULT_SCROLLBACK_LIMIT_BYTES,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_default_shell_defaults_empty_and_parses() {
        let default_config = Config::default();
        assert!(default_config.terminal.default_shell.is_empty());
        assert_eq!(default_config.terminal.shell_mode, ShellModeConfig::Auto);

        let toml = r#"
[terminal]
default_shell = "nu"
shell_mode = "non_login"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.terminal.default_shell, "nu");
        assert_eq!(config.terminal.shell_mode, ShellModeConfig::NonLogin);
    }

    #[test]
    fn project_commands_parse_as_distinct_roles() {
        let config: Config = toml::from_str(
            r#"
[commands]
git = "legacy-browser"
diff = "legacy-review"
ide = "legacy-editor"
browser = "custom-browser"
review = "custom-review"
editor = "helix ."
"#,
        )
        .unwrap();
        assert_eq!(config.commands.browser, "custom-browser");
        assert_eq!(config.commands.review, "custom-review");
        assert_eq!(config.commands.editor, "helix .");
    }

    #[test]
    fn legacy_command_keys_do_not_override_curated_defaults() {
        let config: Config = toml::from_str(
            r#"
[commands]
git = "legacy-browser"
diff = "legacy-review"
ide = "legacy-editor"
"#,
        )
        .unwrap();

        assert_eq!(config.commands, CommandsConfig::default());
    }

    #[test]
    fn terminal_new_cwd_defaults_follow_and_parses() {
        let default_config = Config::default();
        assert_eq!(
            default_config.terminal.new_cwd,
            NewTerminalCwdConfig::Follow
        );

        let config: Config = toml::from_str(
            r#"
[terminal]
new_cwd = "home"
"#,
        )
        .unwrap();
        assert_eq!(config.terminal.new_cwd, NewTerminalCwdConfig::Home);

        let config: Config = toml::from_str(
            r#"
[terminal]
new_cwd = "~/Projects"
"#,
        )
        .unwrap();
        assert_eq!(
            config.terminal.new_cwd,
            NewTerminalCwdConfig::Path("~/Projects".into())
        );
    }

    #[test]
    fn background_update_checks_default_on_and_parse_off() {
        let default_config = Config::default();
        assert!(default_config.update.version_check);
        assert!(default_config.update.manifest_check);

        let config: Config = toml::from_str(
            r#"
[update]
version_check = false
manifest_check = false
"#,
        )
        .unwrap();
        assert!(!config.update.version_check);
        assert!(!config.update.manifest_check);
    }

    #[test]
    fn resume_agents_on_restore_defaults_on_and_parses() {
        let default_config = Config::default();
        assert!(default_config.session.resume_agents_on_restore);

        let toml = r#"
[session]
resume_agents_on_restore = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.session.resume_agents_on_restore);
    }

    #[test]
    fn initial_sidebar_config_defaults_and_parses() {
        let defaults = Config::default();
        assert_eq!(
            defaults.ui.sidebar.initial_state,
            crate::config::SidebarInitialStateConfig::Expanded
        );
        assert_eq!(
            defaults.ui.sidebar.initial_agent_scope,
            AgentPanelScopeConfig::All
        );

        let config: Config = toml::from_str(
            r#"
[ui.sidebar]
initial_state = "collapsed"
initial_agent_scope = "group"
"#,
        )
        .unwrap();
        assert_eq!(
            config.ui.sidebar.initial_state,
            crate::config::SidebarInitialStateConfig::Collapsed
        );
        assert_eq!(
            config.ui.sidebar.initial_agent_scope,
            AgentPanelScopeConfig::Group
        );
    }

    #[test]
    fn pane_appearance_defaults_and_parse() {
        let default_config = Config::default();
        assert_eq!(
            default_config.ui.pane_border_agent_info,
            PaneBorderAgentInfoConfig::Hidden
        );
        assert!(default_config.ui.pane_borders);
        assert!(default_config.ui.pane_scrollbars);
        assert!(default_config.ui.pane_gaps);
        assert!(!default_config.ui.hide_tab_bar_when_single_tab);
        assert!(!default_config.ui.show_counters);

        let toml = r#"
[ui]
pane_border_agent_info = "name_and_status"
pane_borders = false
pane_scrollbars = false
pane_gaps = true
hide_tab_bar_when_single_tab = true
show_counters = true
sidebar_collapsed_mode = "hidden"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            config.ui.pane_border_agent_info,
            PaneBorderAgentInfoConfig::NameAndStatus
        );
        assert!(!config.ui.pane_borders);
        assert!(!config.ui.pane_scrollbars);
        assert!(config.ui.pane_gaps);
        assert!(config.ui.hide_tab_bar_when_single_tab);
        assert!(config.ui.show_counters);
        assert_eq!(
            config.ui.sidebar_collapsed_mode,
            SidebarCollapsedModeConfig::Hidden
        );
    }

    #[test]
    fn prompt_new_tab_name_defaults_on_and_parses() {
        let default_config = Config::default();
        assert!(default_config.ui.prompt_new_tab_name);

        let toml = r#"
[ui]
prompt_new_tab_name = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.ui.prompt_new_tab_name);
    }

    #[test]
    fn prompt_new_workspace_name_defaults_off_and_parses() {
        let default_config = Config::default();
        assert!(!default_config.ui.prompt_new_workspace_name);

        let toml = r#"
[ui]
prompt_new_workspace_name = true
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.ui.prompt_new_workspace_name);
    }

    #[test]
    fn status_indicators_default_dots_and_parse() {
        let default_config = Config::default();
        assert_eq!(
            default_config.ui.status_indicators,
            StatusIndicatorStyle::Dots
        );

        let toml = r#"
[ui]
status_indicators = "symbols"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.status_indicators, StatusIndicatorStyle::Symbols);
    }

    #[test]
    fn window_title_defaults_and_parses() {
        let default_config = Config::default();
        assert_eq!(default_config.ui.window_title, "{hostname}: {workspace}");

        let toml = r#"
[ui]
window_title = "{workspace}/{tab}"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.window_title, "{workspace}/{tab}");

        let toml = r#"
[ui]
window_title = ""
"#;
        let config: Config = toml::from_str(toml).unwrap();

        assert_eq!(config.ui.window_title, "");
    }
    #[test]
    fn coordinator_display_name_defaults_empty_and_parses() {
        assert_eq!(Config::default().ui.coordinator_display_name, "");

        let config: Config = toml::from_str(
            r#"
[ui]
coordinator_display_name = "build coordinator"
"#,
        )
        .unwrap();
        assert_eq!(config.ui.coordinator_display_name, "build coordinator");
    }

    #[test]
    fn reveal_hidden_cursor_for_cjk_ime_default_off_and_parse() {
        let default_config = Config::default();
        assert!(!default_config.experimental.reveal_hidden_cursor_for_cjk_ime);

        let toml = r#"
[experimental]
reveal_hidden_cursor_for_cjk_ime = true
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.experimental.reveal_hidden_cursor_for_cjk_ime);
    }

    #[test]
    fn switch_ascii_input_source_in_prefix_default_off_and_parse() {
        let default_config = Config::default();
        assert!(
            !default_config
                .experimental
                .switch_ascii_input_source_in_prefix
        );

        let toml = r#"
[experimental]
switch_ascii_input_source_in_prefix = true
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.experimental.switch_ascii_input_source_in_prefix);
    }

    #[test]
    fn cjk_ime_cursor_shape_default_steady_block_and_parse() {
        let default_config = Config::default();
        assert_eq!(
            default_config.experimental.cjk_ime_cursor_shape,
            ImeCursorShape::SteadyBlock
        );

        let toml = r#"
[experimental]
cjk_ime_cursor_shape = "bar"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            config.experimental.cjk_ime_cursor_shape,
            ImeCursorShape::Bar
        );
    }

    #[test]
    fn agent_profiles_config_parses_custom_profiles() {
        let toml = r#"
[agent_profiles]
order = ["user:omp-mk", "system:codex"]

[[agent_profiles.custom]]
id = "omp-mk"
name = "omp mk"
kind = "omp"
command = "omp-mk --profile main"
enabled = true

[agent_profiles.custom.env]
PI_CONFIG_DIR = "/Users/test/.omp-mk"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.agent_profiles.order, ["user:omp-mk", "system:codex"]);
        assert_eq!(
            config.agent_profiles.custom[0].kind,
            crate::agent_profiles::AgentKind::Omp
        );
        assert_eq!(
            config.agent_profiles.custom[0].command,
            "omp-mk --profile main"
        );
        assert_eq!(
            config.agent_profiles.custom[0].env["PI_CONFIG_DIR"],
            "/Users/test/.omp-mk"
        );
    }

    #[test]
    fn cjk_ime_agents_default_empty_and_parse() {
        let default_config = Config::default();
        assert!(default_config.experimental.cjk_ime_agents.is_empty());

        let toml = r#"
[experimental]
cjk_ime_agents = ["claude", "codex"]
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            config.experimental.cjk_ime_agents,
            vec!["claude".to_string(), "codex".to_string()]
        );
    }

    #[test]
    fn sidebar_bounds_default_and_parse() {
        let default_config = Config::default();
        assert_eq!(default_config.ui.sidebar_min_width, 18);
        assert_eq!(default_config.ui.sidebar_max_width, 36);
        assert_eq!(
            default_config.ui.context_bar,
            ContextBarVisibilityConfig::Always
        );

        assert_eq!(
            default_config.ui.mobile_width_threshold,
            DEFAULT_MOBILE_WIDTH_THRESHOLD
        );
        let toml = r#"
[ui]
sidebar_min_width = 12
sidebar_max_width = 80
mobile_width_threshold = 96
context_bar = "never"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.sidebar_min_width, 12);
        assert_eq!(config.ui.sidebar_max_width, 80);
        assert_eq!(config.ui.mobile_width_threshold, 96);
        assert_eq!(config.ui.context_bar, ContextBarVisibilityConfig::Never);
    }

    #[test]
    fn validated_sidebar_bounds_rejects_inverted() {
        assert_eq!(validated_sidebar_bounds(18, 36), Some((18, 36)));
        assert_eq!(validated_sidebar_bounds(20, 20), Some((20, 20)));
        assert_eq!(validated_sidebar_bounds(0, u16::MAX), Some((0, u16::MAX)));
        assert_eq!(validated_sidebar_bounds(50, 30), None);
        assert_eq!(validated_sidebar_bounds(u16::MAX, 0), None);
    }

    #[test]
    fn mouse_capture_default_on_and_parse() {
        let default_config = Config::default();
        assert!(default_config.ui.mouse_capture);

        let toml = r#"
[ui]
mouse_capture = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.ui.mouse_capture);
    }
    #[test]
    fn right_click_passthrough_modifier_defaults_off_and_parses() {
        let default_config = Config::default();
        assert_eq!(default_config.ui.right_click_passthrough_modifiers(), None);

        for value in ["", "off", "none", "disabled"] {
            let toml = format!(
                r#"
[ui]
right_click_passthrough_modifier = "{value}"
"#
            );
            let config: Config = toml::from_str(&toml).unwrap();
            assert_eq!(
                config.ui.right_click_passthrough_modifiers(),
                None,
                "value {value:?} should disable passthrough"
            );
        }

        for (value, expected) in [
            ("ctrl", KeyModifiers::CONTROL),
            ("control", KeyModifiers::CONTROL),
            ("alt", KeyModifiers::ALT),
            ("option", KeyModifiers::ALT),
            ("cmd", KeyModifiers::SUPER),
            ("command", KeyModifiers::SUPER),
            ("super", KeyModifiers::SUPER),
            ("meta", KeyModifiers::META),
            ("hyper", KeyModifiers::HYPER),
        ] {
            let toml = format!(
                r#"
[ui]
right_click_passthrough_modifier = "{value}"
"#
            );
            let config: Config = toml::from_str(&toml).unwrap();
            assert_eq!(
                config.ui.right_click_passthrough_modifiers(),
                Some(expected),
                "value {value:?} should parse"
            );
        }

        let toml = r#"
[ui]
right_click_passthrough_modifier = "cmd+alt"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            config.ui.right_click_passthrough_modifiers(),
            Some(KeyModifiers::SUPER | KeyModifiers::ALT)
        );
    }

    #[test]
    fn right_click_passthrough_modifier_rejects_shift() {
        for value in ["shift", "shift+ctrl", "ctrl+", "ctrl++alt", "banana"] {
            let toml = format!(
                r#"
[ui]
right_click_passthrough_modifier = "{value}"
"#
            );
            let err = toml::from_str::<Config>(&toml)
                .expect_err(&format!("value {value:?} should be rejected"))
                .to_string();
            assert!(
                err.contains("right_click_passthrough_modifier must be")
                    && err.contains("without shift"),
                "value {value:?} failed with the wrong error: {err}"
            );
        }
    }

    #[test]
    fn right_click_passthrough_modifier_preserves_and_cycles_accepted_combinations() {
        let combined = RightClickPassthroughModifierConfig::from_modifiers(Some(
            KeyModifiers::SUPER | KeyModifiers::ALT,
        ));
        assert_eq!(
            combined.modifiers(),
            Some(KeyModifiers::SUPER | KeyModifiers::ALT)
        );
        let super_label = crate::config::keybinds::super_modifier_label();
        let display_super_label = if super_label == "cmd" { "Cmd" } else { "Super" };
        assert_eq!(combined.label(), format!("Alt + {display_super_label}"));
        assert_eq!(combined.config_value(), format!("alt+{super_label}"));
        let parsed: Config = toml::from_str(&format!(
            "[ui]\nright_click_passthrough_modifier = {:?}\n",
            combined.config_value()
        ))
        .unwrap();
        assert_eq!(
            parsed.ui.right_click_passthrough_modifiers(),
            combined.modifiers()
        );

        let mut seen = std::collections::BTreeSet::new();
        let mut current = RightClickPassthroughModifierConfig::default();
        for _ in 0..32 {
            seen.insert(current.config_value());
            current = current.next();
        }
        assert_eq!(seen.len(), 32);
        assert!(seen.contains("off"));
        assert!(seen.contains("ctrl"));
        assert!(seen.contains(&format!("alt+{super_label}")));
        assert!(seen.contains(&format!("ctrl+alt+{super_label}+meta+hyper")));
        assert_eq!(current, RightClickPassthroughModifierConfig::default());
    }

    #[test]
    fn redraw_on_focus_gained_default_on_and_parse() {
        let default_config = Config::default();
        assert!(default_config.ui.redraw_on_focus_gained);

        let toml = r#"
[ui]
redraw_on_focus_gained = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.ui.redraw_on_focus_gained);
    }

    #[test]
    fn copy_on_select_defaults_on_and_parses() {
        let default_config = Config::default();
        assert!(default_config.ui.copy_on_select);

        let toml = r#"
[ui]
copy_on_select = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.ui.copy_on_select);
    }

    #[test]
    fn mouse_scroll_lines_defaults_to_three_and_parses() {
        let default_config = Config::default();
        assert_eq!(
            default_config.ui.mouse_scroll_lines(),
            DEFAULT_MOUSE_SCROLL_LINES
        );

        let toml = r#"
[ui]
mouse_scroll_lines = 1
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.mouse_scroll_lines(), 1);
    }

    #[test]
    fn mouse_scroll_lines_rejects_zero() {
        let toml = r#"
[ui]
mouse_scroll_lines = 0
"#;
        let err = toml::from_str::<Config>(toml).expect_err("zero scroll lines should reject");
        let message = err.to_string();
        assert!(
            message.contains("invalid value")
                && message.contains('0')
                && (message.contains("nonzero") || message.contains("non-zero")),
            "zero scroll lines failed with the wrong error: {message}"
        );
    }

    #[test]
    fn toast_config_parses() {
        let toml = r#"
[ui.toast]
delivery = "terminal"
delay_seconds = 2

[ui.toast.gardn]
position = "top-left"

[ui.toast.clipboard]
enabled = false
position = "top-center"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.toast.delivery, ToastDelivery::Terminal);
        assert_eq!(config.ui.toast.delay_seconds, 2);
        assert_eq!(config.ui.toast.gardn.position, ToastGardnPosition::TopLeft);
        assert!(!config.ui.toast.clipboard.enabled);
        assert_eq!(
            config.ui.toast.clipboard.position,
            ToastClipboardPosition::TopCenter
        );
    }

    #[test]
    fn toast_config_defaults_preserve_existing_behavior_with_delay() {
        let config = Config::default();
        assert_eq!(config.ui.toast.delivery, ToastDelivery::Off);
        assert_eq!(config.ui.toast.delay_seconds, 1);
        assert_eq!(
            config.ui.toast.gardn.position,
            ToastGardnPosition::BottomRight
        );
        assert!(config.ui.toast.clipboard.enabled);
        assert_eq!(
            config.ui.toast.clipboard.position,
            ToastClipboardPosition::BottomCenter
        );
    }

    #[test]
    fn toast_config_parses_system_delivery() {
        let toml = r#"
[ui.toast]
delivery = "system"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.toast.delivery, ToastDelivery::System);
    }

    #[test]
    fn toast_config_legacy_enabled_true_maps_to_gardn() {
        let toml = r#"
[ui.toast]
enabled = true
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.toast.delivery, ToastDelivery::Gardn);
    }

    #[test]
    fn toast_config_legacy_enabled_false_maps_to_off() {
        let toml = r#"
[ui.toast]
enabled = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.toast.delivery, ToastDelivery::Off);
    }

    #[test]
    fn toast_config_delivery_wins_over_legacy_enabled() {
        let toml = r#"
[ui.toast]
enabled = true
delivery = "terminal"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.ui.toast.delivery, ToastDelivery::Terminal);
    }

    #[test]
    fn toast_config_rejects_unbounded_delay() {
        let toml = format!(
            r#"
[ui.toast]
delay_seconds = {}
"#,
            MAX_TOAST_DELAY_SECONDS + 1
        );

        let error = toml::from_str::<Config>(&toml).unwrap_err().to_string();

        assert!(error.contains("ui.toast.delay_seconds must be between 0 and 3600"));
    }

    #[test]
    fn remote_manage_ssh_config_defaults_on_and_parses() {
        let default_config = Config::default();
        assert!(default_config.remote.manage_ssh_config);

        let toml = r#"
[remote]
manage_ssh_config = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.remote.manage_ssh_config);
    }

    #[test]
    fn missing_onboarding_shows_setup() {
        let config = Config::default();
        assert!(config.should_show_onboarding());
    }

    #[test]
    fn onboarding_false_skips_setup() {
        let config: Config = toml::from_str("onboarding = false").unwrap();
        assert!(!config.should_show_onboarding());
    }

    #[test]
    fn server_headless_size_defaults_and_parses() {
        let default_config = Config::default();
        assert_eq!(
            default_config.server.headless_cols,
            crate::config::DEFAULT_HEADLESS_COLS
        );
        assert_eq!(
            default_config.server.headless_rows,
            crate::config::DEFAULT_HEADLESS_ROWS
        );

        let config: Config = toml::from_str(
            r#"[server]
headless_cols = 160
headless_rows = 50
"#,
        )
        .unwrap();
        assert_eq!(config.server.headless_cols, 160);
        assert_eq!(config.server.headless_rows, 50);

        let invalid: Config = toml::from_str(
            r#"[server]
headless_cols = 0
headless_rows = 50
"#,
        )
        .unwrap();
        assert!(invalid.invalid_headless_size_diagnostic().is_some());
        assert_eq!(
            invalid.headless_size(),
            (
                crate::config::DEFAULT_HEADLESS_COLS,
                crate::config::DEFAULT_HEADLESS_ROWS
            )
        );
    }

    #[test]
    fn advanced_defaults_include_scrollback_limit_bytes() {
        let config = Config::default();
        assert_eq!(
            config.advanced.scrollback_limit_bytes,
            DEFAULT_SCROLLBACK_LIMIT_BYTES
        );
    }

    #[test]
    fn pane_history_persistence_defaults_on_and_parses_off() {
        assert!(Config::default().experimental.pane_history);

        let toml = r#"
[experimental]
pane_history = false
"#;
        let config: Config = toml::from_str(toml).unwrap();

        assert!(!config.experimental.pane_history);
    }

    #[test]
    fn kitty_graphics_default_on_and_parse_off() {
        let config = Config::default();
        assert!(config.experimental.kitty_graphics);

        let toml = r#"
[experimental]
kitty_graphics = false
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(!config.experimental.kitty_graphics);
    }

    #[test]
    fn experimental_config_parses() {
        let toml = r#"
[experimental]
allow_nested = true
kitty_graphics = true
pane_history = true
switch_ascii_input_source_in_prefix = true
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.experimental.allow_nested);
        assert!(config.experimental.kitty_graphics);
        assert!(config.experimental.pane_history);
        assert!(config.experimental.switch_ascii_input_source_in_prefix);
    }

    #[test]
    fn advanced_config_parses() {
        let toml = r#"
[advanced]
scrollback_limit_bytes = 12345
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.advanced.scrollback_limit_bytes, 12345);
    }

    #[test]
    fn advanced_legacy_scrollback_lines_alias_parses() {
        let toml = r#"
[advanced]
scrollback_lines = 12345
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.advanced.scrollback_limit_bytes, 12345);
    }
}
