use std::io;

use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;

pub(crate) const GARDN_ENV_VAR: &str = "GARDN_ENV";
pub(crate) const GARDN_ENV_VALUE: &str = "1";
const NESTED_GARDN_MESSAGES: [&str; 6] = [
    "inception detected. we need to go deeper... said no one ever.",
    "recursion is a pathway to many abilities some consider to be... unnatural.",
    "you were so preoccupied with whether you could, you didn't stop to think if you should. — dr. malcolm",
    "recursive gardning is disabled. somewhere, a call stack breathes a sigh of relief.",
    "recursive descent denied. there is, in fact, such a thing as too much gardn.",
    "recursion detected. base case not found. aborting.",
];

mod agent_detection_policy;
mod agent_profiles;

mod agent_resume;
mod api;
mod app;
mod browser_theme;
mod build_info;
mod checksum;
mod cli;
mod client;
mod commands;
mod config;
mod detect;
mod events;
mod execution_host;
mod external_tool_theme;
mod fresh_theme;
mod ghostty;
mod github;
mod handoff_runtime;
mod hunk_theme;
mod input;
mod install;
mod integration;
mod ipc;
mod kitty_graphics;
mod layout;
mod logging;
mod metadata_tokens;
mod noninteractive_process;
mod pane;
mod pane_graphics_files;
mod persist;
mod platform;
mod plugin_command;

mod plugin_paths;
mod popup_size;
mod ports;
mod product_announcements;
mod product_env;

mod protocol;
mod pty;
mod raw_input;
mod release_notes;
mod render_signal;

mod remote;
mod selection;
mod server;
mod session;
mod settings_rows;
mod sound;
mod terminal;
mod terminal_effects;
mod terminal_modes;
mod terminal_notify;
mod terminal_theme;
mod ui;
mod update;
mod workspace;

fn init_logging() {
    crate::logging::init_file_logging("gardn.log");
}

const DEFAULT_CONFIG: &str = r##"# Gardn configuration
# place this file at ~/.config/gardn/config.toml

# show first-run notification setup on startup.
# missing also shows onboarding; set false after you've chosen.
# onboarding = true

[theme]
# built-in themes: system, terminal, catppuccin-latte, flexoki-light,
#                 gardn-day, gruvbox-light, kanagawa-lotus, monokai-pro-light,
#                 monokai-pro-light-sun, one-light, rose-pine-dawn,
#                 solarized-light, tokyo-night-day, white, catppuccin,
#                 catppuccin-frappe, catppuccin-macchiato, dracula,
#                 ethereal, everforest, flexoki, gardn-night, gruvbox, hackerman,
#                 kanagawa, last-horizon, lumon, matte-black, miasma,
#                 monokai-classic, monokai-pro, monokai-pro-machine,
#                 monokai-pro-octagon, monokai-pro-ristretto,
#                 monokai-pro-spectrum, nord, one-dark, osaka-jade,
#                 retro-82, rose-pine, solarized, solitude, tokyo-night,
#                 vantablack, vesper
# name = "catppuccin"
# mode = "system"
# light = "system"
# dark = "system"
# terminal_accent = "blue"       # fallback: blue, magenta, cyan, green, yellow, red
# terminal_light_accent = "blue"
# terminal_dark_accent = "blue"


[terminal]
# Executable used for new interactive panes.
# Empty means $SHELL, then /bin/sh.
# default_shell = ""

# Startup mode for new interactive pane shells: "auto", "login", or "non_login".
# "auto" uses login shells on macOS and keeps the current behavior elsewhere.
# shell_mode = "auto"

# CWD policy for new panes, tabs, and workspaces when no explicit --cwd is provided.
# Use "follow" to inherit the source pane/workspace, "home" for $HOME,
# "current" for Gardn's process directory, or a fixed path such as "~/Projects".
# new_cwd = "follow"

[update]
# Check GitHub for new Gardn versions in the background.
# version_check = true

# Check for remote agent-detection manifest updates in the background.
# manifest_check = true

[agent_profiles]
# Optional global order across system and custom agent profiles.
# System ids match integration targets, e.g. system:codex, system:omp.
# order = ["system:codex", "system:omp"]

# Custom profiles model wrappers or profile-specific commands.
# Commands are parsed into argv; shell pipes, redirects, globbing, and $VAR
# expansion are not applied. Use env rows for profile/config directories.
# [[agent_profiles.custom]]
# id = "omp-mk"
# name = "omp mk"
# kind = "omp"
# command = "omp-mk"
# enabled = true
#
# [agent_profiles.custom.env]
# PI_CONFIG_DIR = "/Users/me/.omp-mk"

[keys]
# Prefix key to enter prefix mode (default: "ctrl+b")
# Examples: "ctrl+b", "f12", "esc", "-"
# Action bindings use explicit syntax: "prefix+n" requires the prefix;
# "ctrl+alt+n" is a direct terminal-mode shortcut.
# Accepted key syntax: plain keys, ctrl/shift/alt/cmd/super modifiers, and special keys like enter/tab/esc/left/right/up/down.
# Named punctuation such as minus, comma, ampersand, plus, and backtick is also accepted.
# Most reliable direct bindings are ctrl+letter, function keys, and explicit modified chords.
# alt+..., cmd/super, and punctuation-with-modifiers may depend on your terminal/tmux setup.
# prefix = "ctrl+b"

# Prefix-mode actions
# help = "prefix+?"
# settings = "prefix+s"
# detach = "prefix+q"
# reload_config = "prefix+shift+r"
# open_notification_target = "prefix+o"
# workspace_picker = "prefix+w"
# new_workspace = "prefix+shift+n"
# rename_workspace = "prefix+shift+w"
# close_workspace = "prefix+shift+d"
# previous_workspace = "" # optional, unset by default
# next_workspace = ""     # optional, unset by default
# open_group_menu = ""    # optional, unset by default
# new_group = ""          # optional, unset by default
# rename_group = ""       # optional, unset by default
# delete_group = ""       # optional, unset by default
# toggle_group_filter = "" # optional, unset by default
# previous_group = ""     # optional, unset by default
# next_group = ""         # optional, unset by default
# switch_group = "prefix+alt+1..0"
# previous_agent = ""     # optional, unset by default
# next_agent = ""         # optional, unset by default
# open_agent_menu = ""    # optional, unset by default
# open_context_menu = "shift+f10"
# command_palette = "prefix+space"
# focus_agent = ""        # optional indexed binding, e.g. "prefix+alt+1..9"
# remote_image_paste = "ctrl+v" # only active in gardn --remote; empty disables raw-key image paste
# new_tab = "prefix+c"
# rename_tab = "prefix+shift+t"
# previous_tab = "prefix+p"
# next_tab = "prefix+n"
# switch_tab = "prefix+1..0"
# switch_workspace = "prefix+shift+1..0"
# close_tab = "prefix+shift+x"
# rename_pane = "prefix+shift+p"
# edit_scrollback = "prefix+e"
# focus_pane_left = "prefix+h"
# focus_pane_down = "prefix+j"
# focus_pane_up = "prefix+k"
# focus_pane_right = "prefix+l"
# cycle_pane_next = "prefix+tab"
# cycle_pane_previous = "prefix+shift+tab"
# split_vertical = "prefix+v"
# split_horizontal = "prefix+minus"
# close_pane = "prefix+x"
# zoom = "prefix+z"       # legacy alias: fullscreen
# resize_mode = "prefix+r"
# toggle_sidebar = "prefix+b"
# toggle_context_bar = "prefix+down"
# zen_mode = "prefix+shift+z"
# toggle_right_sidebar = "" # optional, unset by default

# Navigate-mode movement. These local shortcuts win while navigate mode is open.
# They are independent from focus_pane_*. Do not include prefix+, esc, enter, tab, or unmodified 1..0.
# navigate_workspace_up = "up"
# navigate_workspace_down = "down"
# navigate_pane_left = "h"      # left arrow always focuses the pane to the left
# navigate_pane_down = "j"
# navigate_pane_up = "k"
# navigate_pane_right = "l"     # right arrow always focuses the pane to the right

# Custom commands use the same binding syntax.
# type = "shell" runs detached in the background.
# type = "pane" opens a temporary pane and closes it when the command exits.
# On Windows, command strings run through cmd.exe /d /c.
[commands]
# Commands run in the selected project context.
browser = "terminal-browser"
review = "hunk diff --watch"
editor = "fresh ."

# Legacy indexed shortcut config is still parsed for compatibility.
# Prefer switch_tab, switch_workspace, switch_group, and focus_agent for new configs.
# [keys.indexed]
# tabs = ""       # e.g. "ctrl" makes ctrl+1..9 switch tabs directly
# workspaces = "" # e.g. "ctrl+shift" makes ctrl+shift+1..9 switch workspaces directly
# agents = ""     # e.g. "alt" makes alt+1..9 focus agent rows directly

[ui]
# sidebar width (auto-scaled based on workspace names, this sets the default)
# sidebar_width = 26

# Minimum sidebar width when expanded (columns)
# sidebar_min_width = 18

# Maximum sidebar width when expanded (columns)
# sidebar_max_width = 36

# Terminal width at or below which Gardn uses the mobile single-column layout.
# Increase this for foldables, tablets, or wide phone terminals.
# mobile_width_threshold = 64
# Bottom context bar visibility: "always" or "never".
# context_bar = "always"



# Capture mouse input for Gardn's mouse UI.
# Set false to let the terminal handle normal clicks, such as Cmd-clicking URLs.
# Pane apps such as terminal-browser and btop can still receive mouse when they request it.
# mouse_capture = true

# Automatically copy text selected by mouse drag.
# Set false to keep drag selection visible without copying; double-click still copies a word.
# copy_on_select = true

# Host cursor policy: "auto", "native", or "drawn".
# "auto" draws Gardn's own cursor on native Windows builds and WSL to avoid ConPTY cursor flicker.
# "native" always uses the outer terminal cursor; "drawn" always draws Gardn's cursor as cell content.
# host_cursor = "auto"

# Optional modifier that forwards right-click hold/drag gestures to pane apps instead of opening Gardn's pane menu.
# Empty/off disables this. Shift is intentionally unsupported because terminals commonly reserve Shift+mouse.
# Supported values include "ctrl", "alt", "cmd", "super", "meta", "hyper", and + combinations such as "cmd+alt".
# right_click_passthrough_modifier = ""

# Force a full redraw when the outer terminal regains focus.
# Set false to reduce visible flashing when switching back to Gardn.
# Trade-off: rare host terminal surface corruption may persist until the next full redraw.
# redraw_on_focus_gained = true

# Pane scrollback lines to scroll per mouse wheel notch.
# mouse_scroll_lines = 3

# ask for confirmation before closing a workspace
# confirm_close = true

# ask for a tab name before creating a new tab.
# set false to create tabs immediately with generated names.
# prompt_new_tab_name = true

# ask for a workspace name before interactive creation.
# prompt_new_workspace_name = false

# agent metadata shown in split pane borders when no title or manual name is set.
# pane_border_agent_info = "hidden" # "hidden", "name", or "name_and_status"

# how agent status is rendered in lists: uniform colored dots or distinct symbols.
# status_indicators = "dots" # "dots" or "symbols"

# optional coordinator execution-host display name; empty uses the machine hostname.
# coordinator_display_name = ""

# title oh my gardn writes to the terminal it runs in, which is what window
# managers show in title, tab, and group bars. tokens are {hostname},
# {workspace}, {tab}, {pane}, and {terminal_title}; {{ and }} are literal braces.
# the title renders on the server, so {hostname} names the host the panes run
# on even when attaching from a remote client.
# set to "" to leave the outer terminal title alone.
# window_title = "{hostname}: {workspace}"

# draw borders around split panes.
# pane_borders = true

# draw interactive scrollbars beside terminal panes.
# set false to reclaim the scrollbar column.
# pane_scrollbars = true


# keep split panes visually separated instead of sharing divider borders.
# pane_gaps = true

# hide the tab row when a workspace has exactly one tab.
# hide_tab_bar_when_single_tab = false

# show right-aligned topology and section counters.
# show_counters = false

# collapsed sidebar mode: "compact" (narrow rail) or "hidden" (zero-width).
# sidebar_collapsed_mode = "compact"


[ui.sidebar]
# Initial view for each newly connected client. Runtime changes remain client-local.
# "expanded" or "collapsed"
initial_state = "expanded"
# "all", "group", or "current" (space)
initial_agent_scope = "all"


# background notification popup delivery
[ui.toast]
# off = disable pop-up notifications
# gardn = show in-app toasts
# terminal = ask the outer terminal to show a desktop notification
# system = ask the OS notification service directly
# delivery = "off"
# delay_seconds = 1

[ui.toast.gardn]
# position = "bottom-right"

[ui.toast.clipboard]
# enabled = true
# position = "bottom-center"

# play sounds when agents change state in background workspaces
[ui.sound]
# enabled = true
# optional custom mp3 sound files. relative paths are resolved from this config file's directory.
# path = "sounds/notification.mp3"   # one mp3 file for all sound notifications
# done_path = "sounds/done.mp3"      # overrides only finished notifications
# request_path = "sounds/request.mp3" # overrides only needs-attention notifications

# per-agent overrides: default | on | off
# by default, droid is muted.
# [ui.sound.agents]
# droid = "off"

[session]
# Resume supported AI-agent panes into their native conversation sessions after
# a Gardn server restart. Requires official integrations that report session refs.
# resume_agents_on_restore = true


[server]
# Virtual terminal size used when no client is attached. Default: 120x40.
# headless_cols = 120
# headless_rows = 40

[remote]
# Whether Gardn manages the ssh config used for the `gardn --remote` bridge.
# When true (default), Gardn runs the bridge ssh through a generated config that
# includes your ~/.ssh/config first and adds ServerAliveInterval/
# ServerAliveCountMax as a fallback, so any keepalive you set yourself still
# wins and idle network/NAT timeouts are less likely to drop the bridge.
# Set false to run plain ssh against your ssh config unchanged.
# manage_ssh_config = true

[experimental]
# Allow launching Gardn from inside a Gardn-managed pane.
# allow_nested = false
# Experimental local Kitty graphics rendering for attached clients.
# Requires a Kitty graphics-compatible outer terminal. Default: true.
# kitty_graphics = true
# Save recent pane screen history across full server restarts.
# pane_history = true
# While prefix mode is active, temporarily switch the host input source to
# an ASCII-capable mode so prefix commands register even when an IME is
# active, then restore the previous input source when prefix mode exits. On
# macOS this selects the ASCII-capable keyboard layout; on Windows it toggles
# a Korean IME between Hangul and English (other IME languages are left
# unchanged). macOS and Windows only; best-effort. Default: false.
# switch_ascii_input_source_in_prefix = false
# Expose the focused pane's cursor to the outer terminal so macOS input
# methods keep tracking the candidate window when TUIs paint their own
# cursor (Claude Code, pi, codex). Trade-off: extra cursor visible for
# apps that hide it without painting a replacement (vim normal mode, etc.).
# reveal_hidden_cursor_for_cjk_ime = false
# Optional allow-list: only reveal for focused panes whose detected agent
# matches one of these names. Empty means apply to any focused pane.
# If the list contains no valid names, the reveal does not apply.
# Accepted: pi, claude, codex, gemini, cursor, cline, opencode, copilot,
# kimi, kiro, droid, amp, grok, hermes, kilo, qwen, qwen-code, mastracode, qodercli, qoder, maki.
# cjk_ime_agents = []
# Cursor shape rendered when reveal_hidden_cursor_for_cjk_ime is true.
# Values: block, steady_block (default), underline, steady_underline, bar, steady_bar.
# cjk_ime_cursor_shape = "steady_block"

[advanced]
# Maximum scrollback buffer size in bytes retained per pane terminal.
# Matches Ghostty's default scrollback-limit behavior.
# scrollback_limit_bytes = 10000000
"##;

// Bundled at build time so the printed skill always matches this binary's release.
const SKILL: &str = include_str!("../../../SKILL.md");

fn should_block_nested(config: &config::Config) -> bool {
    should_block_nested_for_env(config, std::env::var(GARDN_ENV_VAR).ok().as_deref())
}

fn should_block_nested_for_env(config: &config::Config, gardn_env: Option<&str>) -> bool {
    !config.experimental.allow_nested && gardn_env == Some(GARDN_ENV_VALUE)
}

fn random_nested_message() -> &'static str {
    use std::time::{SystemTime, UNIX_EPOCH};

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as usize)
        .unwrap_or(0);
    let index = (nanos ^ (std::process::id() as usize)) % NESTED_GARDN_MESSAGES.len();
    NESTED_GARDN_MESSAGES[index]
}

fn exit_if_nested_disabled(config: &config::Config) {
    if should_block_nested(config) {
        eprintln!("\x1b[1merror:\x1b[0m nested Gardn is disabled by default.");
        eprintln!("see configuration if you want to enable it.");
        eprintln!();
        eprintln!("\x1b[2m\"{}\"\x1b[0m", random_nested_message());
        std::process::exit(1);
    }
}

fn args_as_utf8<I>(args: I) -> Result<Vec<String>, String>
where
    I: IntoIterator<Item = std::ffi::OsString>,
{
    args.into_iter()
        .enumerate()
        .map(|(index, arg)| {
            arg.into_string()
                .map_err(|_| format!("argument {index} is not valid UTF-8"))
        })
        .collect()
}

fn main() -> io::Result<()> {
    let raw_args: Vec<String> = match args_as_utf8(std::env::args_os()) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!("run 'gardn --help' for usage");
            std::process::exit(2);
        }
    };

    if std::env::var_os(execution_host::auth::ASKPASS_ROLE_ENV).is_some() {
        return execution_host::auth::run_ssh_askpass(&raw_args[1..]);
    }
    let args = match session::configure_from_args(&raw_args) {
        Ok(args) => args,
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!("run 'gardn --help' for usage");
            std::process::exit(2);
        }
    };
    let (args, remote_launch) = match remote::extract_remote_args(&args) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("error: {err}");
            eprintln!("run 'gardn --help' for usage");
            std::process::exit(2);
        }
    };

    if remote_launch.is_some()
        && args.get(1).is_some()
        && !args.iter().any(|a| {
            matches!(
                a.as_str(),
                "--help" | "-h" | "--version" | "-V" | "--default-config" | "--skill"
            )
        })
    {
        eprintln!("error: --remote can only be used with the default launch command");
        eprintln!("run 'gardn --help' for usage");
        std::process::exit(2);
    }

    match cli::maybe_run(&args) {
        Ok(cli::CommandOutcome::Handled(code)) => std::process::exit(code),
        Ok(cli::CommandOutcome::NotCli) => {}
        Err(error) => {
            if cli::protocol_mismatch_was_reported(&error) {
                std::process::exit(1);
            }
            if let Some(response) = cli::server_not_running_reported_response(&error) {
                eprintln!(
                    "{}",
                    serde_json::to_string(response).map_err(io::Error::other)?
                );
                std::process::exit(1);
            }
            return Err(error);
        }
    }

    // subcommands and flags (no tui, no logging needed)
    if args.get(1).map(|s| s.as_str()) == Some("execution-worker") {
        return execution_host::worker::run_from_args(&args[2..]);
    }

    if args.get(1).map(|s| s.as_str()) == Some("remote-client-bridge") {
        return remote::run_remote_client_bridge();
    }

    if args.get(1).map(|s| s.as_str()) == Some("remote-api-bridge") {
        return remote::run_remote_api_bridge();
    }

    if args.get(1).map(|s| s.as_str()) == Some("server") {
        return server::headless::run_server();
    }

    // Hidden client mode: connect to an existing server's client socket.
    if args.get(1).map(|s| s.as_str()) == Some("client") {
        let loaded_config = config::Config::load();
        exit_if_nested_disabled(&loaded_config.config);
        return client::run_client();
    }

    if args.get(1).map(|s| s.as_str()) == Some("update") {
        let options = match update::parse_self_update_args(&args[2..]) {
            Ok(options) => options,
            Err(err) if err.starts_with("usage:") => {
                eprintln!("{err}");
                std::process::exit(0);
            }
            Err(err) => {
                eprintln!("{err}");
                eprintln!("usage: gardn update [--handoff]");
                std::process::exit(2);
            }
        };
        match update::self_update(options) {
            Ok(_) => return Ok(()),
            Err(e) => {
                if e.starts_with("self-update is disabled") {
                    eprintln!("{e}");
                } else {
                    eprintln!("update failed: {e}");
                }
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("gardn — terminal workspace manager for ai coding agents");
        println!();
        println!("usage: gardn [options]");
        println!("       gardn --session <name> [options]");
        println!("       gardn --remote <ssh-target> [--session <name>]");
        println!("       gardn session attach <name>");
        println!("       gardn update [--handoff]");
        println!("       gardn server stop");
        println!("       gardn server reload-config");
        println!("       gardn api <subcommand> ...");
        println!("       gardn config <subcommand> ...");
        println!("       gardn workspace <subcommand> ...");
        println!("       gardn tab <subcommand> ...");
        println!("       gardn agent <subcommand> ...");
        println!("       gardn pane <subcommand> ...");
        println!("       gardn wait <subcommand> ...");
        println!("       gardn session <subcommand> ...");
        println!("       gardn integration <subcommand> ...");
        println!();
        println!("common commands:");
        for (command, description) in [
            ("gardn", "launch or attach to the persistent session"),
            (
                "gardn status [server|client]",
                "show local client and running server status",
            ),
            ("gardn update", "download and install the latest version"),
            (
                "gardn server stop",
                "stop the running server via the api socket",
            ),
            (
                "gardn server reload-config",
                "reload config.toml in the running server",
            ),
            (
                "gardn config reset-keys",
                "Back up config.toml and remove custom keybindings",
            ),
            (
                "gardn workspace <subcommand>",
                "workspace helpers over the socket api",
            ),
            ("gardn tab <subcommand>", "tab helpers over the socket api"),
            (
                "gardn agent <subcommand>",
                "Agent/terminal helpers over the socket API",
            ),
            (
                "gardn pane <subcommand>",
                "pane control helpers over the socket api",
            ),
            (
                "gardn wait <subcommand>",
                "blocking wait helpers over the socket api",
            ),
            (
                "gardn session <subcommand>",
                "manage named persistent sessions",
            ),
            (
                "gardn integration <subcommand>",
                "manage built-in agent integrations",
            ),
            (
                "gardn api <subcommand>",
                "inspect socket API metadata and live runtime state",
            ),
        ] {
            println!("  {command:<32} {description}");
        }
        println!();
        println!("advanced commands:");
        println!("  {:<32} run as headless server", "gardn server");
        println!();
        println!("options:");
        println!("  --no-session        run monolithically (no server/client, escape hatch)");
        println!("  --session <name>    use or create a named persistent session");
        println!("  --remote <target>   attach through ssh to a remote Gardn server");
        println!("  --remote-keybindings <local|server>");
        println!("                      keybindings for --remote app attach (default: local)");
        println!("  --handoff           opt into live handoff for update or remote attach");
        println!("  --default-config    print default configuration and exit");
        println!("  --skill             print the agent skill file and exit");
        println!("  --version, -V       print version and exit");
        println!("  --help, -h          show this help");
        println!();
        println!("config: {}", config::config_path().display());
        println!("logs:   {}", logging::help_log_paths_summary());
        println!("env:    GARDN_CONFIG_PATH overrides config file path");
        println!("home:   https://gardn.dev");
        println!("skill:  gardn --skill prints agent instructions for driving gardn from a pane");
        return Ok(());
    }

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("gardn {}", crate::build_info::version());
        return Ok(());
    }

    if args.iter().any(|a| a == "--default-config") {
        print!("{DEFAULT_CONFIG}");
        return Ok(());
    }

    if args.iter().any(|a| a == "--skill") {
        print!("{SKILL}");
        return Ok(());
    }

    // Reject unknown flags
    let known_flags = [
        "--no-session",
        "--session",
        "--remote",
        "--remote-keybindings",
        "--version",
        "-V",
        "--default-config",
        "--skill",
        "--help",
        "-h",
    ];
    for arg in &args[1..] {
        let arg_name = arg.split_once('=').map(|(name, _)| name).unwrap_or(arg);
        if arg.starts_with('-') && !known_flags.contains(&arg_name) {
            eprintln!("unknown option: {arg}");
            eprintln!("run 'gardn --help' for usage");
            std::process::exit(1);
        }
        if !arg.starts_with('-')
            && ![
                "server",
                "client",
                "remote-client-bridge",
                "remote-api-bridge",
                "update",
                "status",
                "config",
                "workspace",
                "pane",
                "wait",
                "session",
                "integration",
            ]
            .contains(&arg.as_str())
        {
            eprintln!("unknown command: {arg}");
            eprintln!("run 'gardn --help' for usage");
            std::process::exit(1);
        }
    }

    if let Some(remote_launch) = remote_launch {
        let remote_target = remote_launch.target.clone();
        if let Err(err) = remote::run_remote(remote_launch) {
            eprintln!("error: {err}");
            remote::print_remote_error_hint(&err, &remote_target);
            std::process::exit(1);
        }
        return Ok(());
    }

    let loaded_config = config::Config::load();
    exit_if_nested_disabled(&loaded_config.config);

    let no_session = args.iter().any(|a| a == "--no-session");

    // Auto-detect launch: when --no-session is NOT set, use server/client mode.
    // Check if a server is running, spawn one if needed, then attach as client.
    if !no_session {
        if let Err(err) = server::autodetect::auto_detect_launch() {
            eprintln!("gardn: {err}");
            std::process::exit(1);
        }
        return Ok(());
    }

    // --- Monolithic mode (--no-session escape hatch) ---
    // This is the pre-mission single-process behavior.

    init_logging();

    let (api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
    let event_hub = api::EventHub::default();
    let _api_server = match api::start_server(api_tx, event_hub.clone()) {
        Ok(server) => server,
        Err(err) if err.kind() == io::ErrorKind::AddrInUse => {
            eprintln!("error: Gardn is already running");
            eprintln!("socket: {}", api::socket_path().display());
            std::process::exit(1);
        }
        Err(err) => return Err(err),
    };

    let modify_other_keys_mode = crate::input::host_modify_other_keys_mode();

    let original_hook = std::panic::take_hook();
    let panic_resets_modify_other_keys = modify_other_keys_mode.is_some();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!("PANIC: {info}");
        if panic_resets_modify_other_keys {
            let _ = std::io::Write::write_all(&mut io::stdout(), b"\x1b[>4;0m");
        }
        if crate::kitty_graphics::is_enabled() {
            let _ = crate::kitty_graphics::clear_all_host_graphics();
        }
        let _ = execute!(
            io::stdout(),
            PopKeyboardEnhancementFlags,
            DisableFocusChange,
            DisableBracketedPaste,
            DisableMouseCapture
        );
        let _ = crate::terminal_modes::clear_host_mouse_reporting(&mut io::stdout());
        ratatui::restore();
        original_hook(info);
    }));

    let config = &loaded_config.config;
    let config_diagnostics =
        (!loaded_config.diagnostics.is_empty()).then(|| loaded_config.diagnostics.clone());
    logging::startup("app");

    // Background update check (non-blocking, best-effort)
    // only checks for newer versions and notifies the tui.
    // Skipped in --no-session mode (testing).

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to create tokio runtime");

    let result = rt.block_on(async {
        let mut terminal = ratatui::init();
        crate::terminal_modes::clear_host_mouse_reporting(&mut io::stdout())?;
        if config.ui.mouse_capture {
            execute!(io::stdout(), EnableMouseCapture)?;
        } else {
            execute!(io::stdout(), DisableMouseCapture)?;
        }
        execute!(
            io::stdout(),
            EnableBracketedPaste,
            EnableFocusChange,
            PushKeyboardEnhancementFlags(crate::input::ime_compatible_keyboard_enhancement_flags())
        )?;

        // Some hosts do not honor Kitty keyboard enhancement pushes for
        // Shift+Enter. Enable xterm modifyOtherKeys only on hosts where we
        // know it is needed and parseable, so modified Enter stays distinct.
        if let Some(mode) = modify_other_keys_mode {
            use std::io::Write;
            std::io::stdout().write_all(mode.set_sequence())?;
            std::io::stdout().flush()?;
        }

        let mut app = app::App::new(
            config,
            true, // no_session — monolithic mode never saves/restores sessions
            config_diagnostics,
            api_rx,
            event_hub,
        );
        let result = app.run(&mut terminal).await;

        // Reset modifyOtherKeys if we enabled it.
        if modify_other_keys_mode.is_some() {
            use std::io::Write;
            std::io::stdout().write_all(b"\x1b[>4;0m")?;
            std::io::stdout().flush()?;
        }

        if crate::kitty_graphics::is_enabled() {
            crate::kitty_graphics::clear_all_host_graphics()?;
        }
        execute!(
            io::stdout(),
            PopKeyboardEnhancementFlags,
            DisableFocusChange,
            DisableBracketedPaste,
            DisableMouseCapture
        )?;
        crate::terminal_modes::clear_host_mouse_reporting(&mut io::stdout())?;
        ratatui::restore();

        // Drop app (and all workspaces/panes) before runtime shuts down
        drop(app);

        result
    });

    // Shut down runtime immediately — kills lingering PTY reader/writer tasks
    rt.shutdown_timeout(std::time::Duration::from_millis(100));

    logging::shutdown("app");
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_parses_configured_commands() {
        let config: config::Config = toml::from_str(DEFAULT_CONFIG).unwrap();

        assert_eq!(config.commands, config::CommandsConfig::default());
    }

    #[test]
    fn nested_gardn_blocks_when_env_is_set() {
        let config = config::Config::default();
        assert!(should_block_nested_for_env(&config, Some(GARDN_ENV_VALUE)));
    }

    #[test]
    fn nested_gardn_does_not_block_when_allowed() {
        let config: config::Config =
            toml::from_str("[experimental]\nallow_nested = true\n").unwrap();
        assert!(!should_block_nested_for_env(&config, Some(GARDN_ENV_VALUE)));
    }

    #[test]
    fn nested_gardn_does_not_block_without_env() {
        let config = config::Config::default();
        assert!(!should_block_nested_for_env(&config, None));
    }

    #[test]
    fn random_nested_message_comes_from_known_set() {
        let message = random_nested_message();
        assert!(NESTED_GARDN_MESSAGES.contains(&message));
    }

    #[test]
    fn nested_message_strings_no_longer_repeat_gardn_prefix() {
        assert!(NESTED_GARDN_MESSAGES
            .iter()
            .all(|message| !message.starts_with("gardn:")));
    }

    #[cfg(unix)]
    fn invalid_utf8_arg() -> std::ffi::OsString {
        use std::os::unix::ffi::OsStringExt;
        std::ffi::OsString::from_vec(vec![0xff])
    }

    #[cfg(windows)]
    fn invalid_utf8_arg() -> std::ffi::OsString {
        use std::os::windows::ffi::OsStringExt;
        std::ffi::OsString::from_wide(&[0xd800])
    }

    #[test]
    fn args_as_utf8_passes_through_valid_arguments() {
        let args = ["gardn", "pane", "get", "pane-1"].map(std::ffi::OsString::from);
        assert_eq!(
            args_as_utf8(args).unwrap(),
            ["gardn", "pane", "get", "pane-1"]
        );
    }

    #[test]
    fn args_as_utf8_reports_the_offending_argument_instead_of_panicking() {
        let args = vec![
            std::ffi::OsString::from("gardn"),
            std::ffi::OsString::from("pane"),
            invalid_utf8_arg(),
        ];
        assert_eq!(
            args_as_utf8(args).unwrap_err(),
            "argument 2 is not valid UTF-8"
        );
    }
}
