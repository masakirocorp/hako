# Gardn features

This is the product feature reference for Gardn.

## Workspace model

### Sessions

A session is a persistent Gardn runtime with its own sockets, panes, tabs, workspaces, and saved state.

- **Default session** — `gardn` launches or attaches to the default background session.
- **Named sessions** — `gardn --session <name>` and `gardn session attach <name>` select separate runtime namespaces.
- **Detach / reattach** — clients can detach while panes and agents continue running in the server.
- **Remote attach** — `gardn --remote <target>` attaches to a Gardn server over SSH.
- **Remote bootstrap** — remote attach can detect the remote platform, reuse an existing compatible binary, or install a matching Gardn binary before connecting.
- **Remote server restart flow** — remote attach checks protocol/version compatibility and can prompt to stop or restart an incompatible remote server.
- **SSH keepalive fallback** — remote attach can add private generated SSH keepalive defaults without overriding your own SSH config.
- **Direct terminal attach** — `gardn terminal attach <terminal-id>` and `gardn agent attach <target>` attach directly to a single server-owned terminal.
- **Attach takeover** — direct attach is exclusive by default; `--takeover` can claim a terminal attachment from another client. This terminal-level takeover is separate from normal app-client Tab Control.
- **Multiple clients** — more than one client can connect to a server; each client owns its navigation and sidebar view. Interactive terminal control is assigned per stable tab identity: the first client may claim a free tab, switching to another free tab may claim it, and an occupied tab is view-only until explicit takeover with `prefix+t` or the persistent desktop/mobile **Take control** action. The controller owns the tab's canonical PTY size and interactive input authority; watchers keep navigation, scroll, copy, and search local and see the canonical terminal canvas cropped or padded to their viewport. Watcher focus, resize, and input do not change PTY size or content, so different client sizes do not cause layout shifts until takeover. Controller navigation, disconnect, and direct terminal attach release control without auto-promoting a watcher. Local API and system automation bypass interactive tab ownership. Global foreground remains host focus, theme, keybinding, and notification context, not PTY sizing or input authority.
- **Clipboard bridging** — thin clients forward OSC 52 clipboard writes locally and can bridge local clipboard-image paste into server panes.
- **Live server handoff** — supported updates can move live pane PTYs and session state into a replacement server so running pane processes survive a server swap.
- **Beta Direct Install** — install a GitHub prerelease as `gardn-beta` to dogfood the next stable on the same `~/.config/gardn` session. Stop the running server before switching binaries; a beta client will not attach to a stable server.


### Workspaces

A workspace contains tabs, panes, cwd metadata, and agent state rollups.

- **Workspace creation and focus** — create, focus, rename, close, list, and inspect workspaces from the TUI, CLI, or socket API. Interactive creation derives a workspace name automatically by default; set `ui.prompt_new_workspace_name = true` to ask for a name first.
- **Workspace sidebar** — expanded workspace rows show the workspace name, activity state, and git/cwd summary.
- **Execution host labels** — pane borders, Space rows, Runs On, and host pickers name the coordinator by its hostname (or `ui.coordinator_display_name`) and SSH hosts by connection profile name. Mixed Spaces join those names. The UI does not show Local as a host.
- **Status indicators.** **Appearance > Agent Status > Status Indicators** controls both Space status marks and Agent group headers. Dots is the static default. Symbols uses distinct marks and animates a Braille loader only for Working.
- **Agent sidebar** — agent rows focus their workspace, tab, and pane when clicked and highlight the active agent row for the attached client. Clicking a Done agent keeps that row in Triage until focus leaves the pane or the agent starts working. Row titles use the pane cwd, then append the tab name when that space has multiple tabs and the pane name or number when that tab is split, joined with `/` and no spaces. Text before the last `/` is muted; the final segment stays at full contrast. Unnamed pane numbers are dense within their tab and update with the current pane layout without changing stable API pane IDs.
- **Collapsed sidebar rail** — collapsed sidebars keep group boundaries and agent status categories visible: group rows switch directly to that group's remembered workspace, space rows switch directly to a workspace, and activity counts open filtered agent pickers. Compact agent rows sit under expandable triage, follow up, working, and idle headers. Hovering or keyboard-selecting a compact space row reveals its accented group, full space name, and color-coded status. At the bottom of the rail, the help launcher sits directly above the expand control.
- **Configurable sidebar metadata** — `[ui.sidebar.agents]` and `[ui.sidebar.spaces]` rows accept built-in tokens and `$custom` metadata reported through the socket API; defaults preserve compact workspace and agent labels across expanded, collapsed, and mobile views.
- **New-client sidebar defaults** — every app client starts with all spaces visible. `ui.sidebar.initial_state` and `ui.sidebar.initial_agent_scope` choose its initial expansion and agent scope; defaults are `expanded` and `all`, and one client's runtime changes never seed another client.
- **Workspace navigator** — search and filter groups, workspaces, tabs, and panes by text or state; open it with `prefix+w`, any desktop context-bar segment, or **Open workspace navigator** in the command palette. Group rows use their configured accents, while descendant labels remain neutral and agent-state colors appear only when meaningful. The tree omits redundant singleton levels: tab rows appear only when a workspace has multiple tabs, and pane rows only when their tab is split; hidden singleton-pane metadata remains searchable through its workspace or tab row. The active group opens with every space branch expanded so those conditional tab and pane rows are immediately visible. Group and workspace disclosure arrows expand each branch without leaving the navigator; `Space` toggles the visibly highlighted branch, `E` expands every branch, and `C` collapses the tree to its group roots, while clicking a row name focuses it. The tall, stable modal uses the same title/close header, dividers, detail row, and footer hints as other app modals, so branches do not move pointer targets between clicks.
- **Desktop context bar** - an independent bottom row shows the attached client's active group / workspace / tab path, plus the focused pane name when a tab is split, on the left and optional live topology counts on the right. It is visible by default; set `ui.context_bar = "never"` to hide it persistently, or toggle one client temporarily with `prefix+Down`. Set `ui.show_counters = true` to show right-aligned group, space, tab, pane, and agent section counts across desktop and mobile UI; counters are hidden by default. Every path segment opens the same workspace navigator with the matching group, space, tab, or pane visibly selected; narrow terminals drop counts before shortening path segments.
- **Workspace groups** — group workspaces, filter the sidebar by group, collapse groups, and assign per-group theme accent colors that tint group labels, tabs, menus, and related group UI.
- **Group lifecycle** — create, rename, delete, focus, and switch groups from the TUI, CLI, or socket API; reorder groups by dragging headers in the all-groups sidebar. New groups start with an initial space, creating a group preserves the all-groups view, and a group's context menu creates spaces in that group. Right-clicking the blank area after the final group or space opens a compact creation menu; a new space belongs to the active workspace's group. The expanded sidebar labels the all-groups scope as **groups** and a filtered scope as **spaces**.
- **Group icons** — group creation, rename, and group settings can choose from a curated set of single-cell icons.

- **Move between groups** — move workspaces between groups from the TUI/sidebar group workflows.
- **Public IDs** — CLI and socket API commands target workspaces, tabs, panes, and groups with public IDs; raw pane IDs remain compatibility inputs and are remapped after live handoff where possible.
- **Live cwd labels** — workspace labels can follow active pane cwd unless manually renamed.
- **Git summaries** — workspace summaries roll up added, modified, deleted, and conflicted files across detected repository roots.

### Tabs

A tab belongs to one workspace and contains one or more panes.

- **Tab lifecycle** — create, focus, rename, close, list, and inspect tabs.
- **Tab bar** — click tabs, close hovered tabs with the inline close button, use overflow scrolling, and switch with keybindings. The overflow view follows the active tab after navigation; manually scrolling the tab bar suspends that follow behavior until tab focus changes again.
- **Tab drag reorder** — reorder tabs in the tab bar by dragging, with a drop indicator.
- **Tab-aware state** — workspace and agent UI can include tab context for agents and notifications.
- **Tab control** — each stable tab identity has at most one interactive controller. A watcher can view an occupied tab but must explicitly take control before tab input or canonical resizing can follow that client.

### Panes

A pane is a terminal runtime inside a tab layout.

- **Pane splitting** — split panes vertically or horizontally.
- **Pane move** — move panes into another tab, a new tab, or a new workspace from the CLI or socket API.
- **Pane focus and zoom** — focus by direction, cycle panes, and zoom the focused pane.
- **Pane resize** — the tab controller can resize panes interactively from resize mode or by dragging borders; watchers remain view-only until explicit takeover.
- **Zen mode** — toggle a client-local full-screen terminal view that temporarily hides sidebars, the tab bar, mobile header, and context bar.
- **Pane labels** — set manual pane labels; optionally show agent names or compact name-and-status metadata on pane borders. Integration titles and manual names always take precedence.
- **Pane close** — close panes with confirmation where configured.
- **Scrollback** — scroll panes, edit scrollback in `$EDITOR`, and read visible/recent output through the API.
- **Pane history** — persist recent screen history to `session-history.json` by default.
- **Terminal identity** — panes advertise Gardn's terminal layer instead of leaking the outer terminal identity.
- **Snapshot restore** — saved sessions restore groups, active selections, sidebar sizing and arrangement, tabs, pane layouts, focus, zoom, cwd, labels, and agent session references.
- **Text selection** — mouse dragging leaves pane text highlighted until the next click or keypress. Keyboard copy mode remains available for explicit selection and copying.
- **Automatic selection copy** — `ui.copy_on_select` defaults to `true` and copies a drag selection on mouse-up. Set it to `false` to retain the highlight without writing to the clipboard. Double-click always selects and copies the clicked word.
- **Keyboard protocol encoding** — pane input honors negotiated terminal keyboard protocols, including Kitty CSI u and legacy modified-key sequences.

## Agent awareness

Gardn detects and tracks coding agents running inside panes.

### Agent states

- **Blocked** — agent needs user input, approval, or intervention.
- **Working** — agent is actively running.
- **Done** — agent finished work and has not been seen yet.
- **Idle** — agent is done and seen, or otherwise waiting without attention.
- **Unknown** — no supported agent state is currently detectable.

### Detection

Gardn combines foreground-process detection, terminal-screen heuristics, and optional integration reports.

Supported built-in detection includes:

- pi
- Oh My Pi / OMP
- Claude Code
- Codex
- Gemini CLI
- Cursor agent
- Antigravity
- Cline
- OpenCode
- GitHub Copilot CLI
- Kimi
- Kiro
- Droid
- Amp
- Grok CLI
- Hermes agent
- Kilo Code CLI
- Qwen Code
- MastraCode
- Maki


- **Manifest rules** — bundled per-agent TOML manifests define screen, OSC title, and OSC progress matching rules for screen-detectable built-in families, including Qwen Code. MastraCode intentionally has no screen manifest because its hook owns lifecycle state. Screen rules can provide strong visible evidence; OSC-only rules are fallback evidence and do not override hook authority as visible UI.
- **Manifest updates** — Gardn can cache newer remote manifests, reject downgrades or incompatible engine versions, reload local manifests through `gardn server reload-agent-manifests`, and report updated detection rules through the normal toast/update path.
- **Wrapped-process hints** — Gardn-managed profiles automatically set `GARDN_AGENT=<agent>` from the selected supported agent kind, so host-visible wrappers remain detectable on Linux and macOS. Set the hint explicitly only when launching a wrapper manually inside an arbitrary pane. The hint is process-scoped; avoid exporting it globally. Upstream-branded hint names are not accepted.

### Agent UI

- **Activity sidebar** — shows agents in Triage (when nonempty), always-present Follow Up, Working, and Idle. An expanded empty Follow Up section shows a muted `Drop an agent here` row on desktop and mobile. Its full desktop width and the section header accept drops. Follow Up is shared placement, not a lifecycle state: drag an agent row onto the Follow Up header or body to queue it, or right-click any agent row to add it to or remove it from Follow Up. Queued agents keep their real runtime state and waiting age, ordered oldest-added first. Successful unmodified Enter from the human terminal input path clears that pane's placement; typing, paste, failed sends, and API automation do not. Closing a pane or restoring without that target drops stale queue entries. Triage lists oldest meaningful activity first (no activity counts as oldest); Working and Idle stay newest-first.
- **macOS app** — Gardn.app is the macOS install. It owns `~/.local/bin/gardn` by linking it to the bundled CLI. The menu bar surface lists the same Triage, Follow Up, Working, and Idle groups as the Agents sidebar. The header picks which coordinator to observe: this Mac's local sessions, or a saved remote Coordinator Host (`gardn extra connect --remote`). Add a remote server from the app. Settings… in the extra header opens a Settings window for servers and updates. Sparkle owns updates when the running `gardn` is that bundled CLI. Use Check for Updates in Gardn, or install the latest DMG from GitHub. A real standalone `gardn` binary keeps Direct updates even if the app is present. `gardn-beta`, `gardn-dev`, mise, and Nix keep their own update owners. The menu bar leaf fills when Follow Up or Triage needs attention. Click a row to focus that agent in an attached client, or launch the bundled `gardn` / `gardn --remote` if none is attached. Right-click to add or remove Follow Up. The app posts Notification Center alerts when an agent enters Follow Up or Triage; clicking an alert focuses that agent. While the app is running, terminal and system agent toasts defer to it.


- **Agent focus** — focus agents from the activity panel, command surfaces, CLI, or socket API.
- **Agent labels** — manual, detected, and integration-reported labels are surfaced in lists and pane borders.
- **Agent metadata tokens** — pane metadata token patches are exposed consistently through pane/agent API snapshots and rendered without leaking one client's sidebar view into another.
- **State notifications** — background state changes can trigger Gardn toasts, terminal toasts, system toasts, and sounds.
- **Integration authority** — installed hooks either report native session identity for restore or report state directly. Kilo and MastraCode hooks own lifecycle state. Qwen Code and Antigravity CLI hooks report session identity only while their screen manifests own lifecycle state.
- **Pi settled lifecycle** — the Pi integration reports only TUI sessions and keeps an active root agent working through compaction. It reports the root agent idle only after Pi emits `agent_settled` while the root session is actually idle. Stale or non-idle settlement signals do not end active work.
- **Missing integration warning** — if screen detection sees an integration-capable agent such as Codex but no accepted Gardn hook, session, or metadata report arrives for that pane, Gardn shows a pane-targeted toast with the matching `gardn integration install <agent>` command.
- **Host-scoped integration management** — Settings can inspect, install, update, and uninstall agent integrations on the coordinator or a saved SSH Execution Host. Remote operations run through the managed worker in order, and remote hooks report through a restricted authenticated worker endpoint instead of receiving the coordinator's Local API socket.


### Agent profiles

- **System profiles** — Gardn exposes one read-only system profile for each supported integration target.
- **Custom profiles** — add or edit profile-specific commands from Settings > Agents. Gardn persists them to `[agent_profiles]`; known-family wrappers automatically receive the selected kind as `GARDN_AGENT`, keep native profile/tooling restore behavior, and cannot override that managed identity through profile environment entries. `custom` unsupported agents are labeled `custom · launch-only`.
- **Group favorites and defaults** — group settings can promote favorite profiles with `ctrl+f` and set a default with `ctrl+d`. Favorites appear before available profiles while both sections keep the global profile order. When a group default is set, `new agent` starts it directly instead of opening the picker.
- **New agent launch** — choose `new agent` from the command palette, space context menu, tab context menu, or the tab `+` dropdown. Gardn starts the group default or only available profile immediately, or opens a favorites-first profile picker when multiple profiles are available.

### Agent session restore

Gardn resumes supported agents into native agent sessions during session restore by default. Set `[session].resume_agents_on_restore = false` to disable it.

- Supported restore sources come from installed integrations that report session references.
- Duplicate session references are deduplicated during a restore pass.
- Native agent restore suppresses pane-history replay so the resumed agent owns its conversation history.
- Restored agents launch as one-shot executable or shell-wrapper commands with their saved environment. OMP restores reconcile safe `.omp` and `.omp-*` session paths with the matching profile wrapper and environment before launch.
- MastraCode restores with `mastracode --thread <id>`. Antigravity CLI restores with `agy --conversation <id>`. Qwen Code and Kilo do not advertise restore support.

## Navigation and interaction

### Prefix mode

Gardn uses a prefix key before most built-in shortcuts. The default prefix is `ctrl+b`.

On macOS, `[experimental].switch_ascii_input_source_in_prefix = true` temporarily switches the host input source to an ASCII-capable layout while prefix mode is active, then restores the previous source when prefix mode exits.

Default prefix actions include:

- workspace navigator
- command palette
- settings
- keybinding help
- new / rename / close workspace
- new / rename / close tab
- tab switching
- pane focus, split, resize, zoom, and close
- sidebar toggle
- detach
- reload config
- open notification target

### Mouse support

Mouse capture is enabled by default.

- Click workspaces, groups, tabs, panes, agents, commands, ports, and modal controls.
- Drag pane borders to resize.
- Drag workspace rows to reorder.
- Scroll lists, panes, modals, and scrollbars. Set `ui.pane_scrollbars = false` to hide pane scrollbars and reclaim the gutter column.
- Right-click where context menus are available. Shift+F10 opens the same menu for the focused pane or agent.
- Configure `ui.right_click_passthrough_modifier` to send modified right-click hold/drag gestures to mouse-reporting pane apps while normal right-click keeps Gardn menus.
- Pane apps that enable SGR pixel mouse (DEC 1016) make Gardn enable host 1016 and convert those pixel reports into pane coordinates. Without cell geometry, Gardn still falls back to cell SGR. Consecutive mouse-move and same-button drag reports keep only the latest position per 16ms frame. Wheel ticks in that same interval are accumulated and all written on flush.
- Select pane text for copy workflows.
- **Mobile layout** — narrow terminals keep the terminal nearly full-height under a compact two-row header: an always-visible agent-status row on top opens the agent list directly, and a breadcrumb below shows the active group, space, tab, and split pane. Each breadcrumb segment opens a compact, bordered dropdown anchored beneath it; group, space, and tab dropdowns include contextual creation, while split actions appear in the pane dropdown when the focused pane has room. Current items are marked, selecting a space, tab, or pane closes the dropdown, Right drills into the highlighted row's children, Left returns to the parent breadcrumb level, Up/Down or `j`/`k` moves within a dropdown, Enter activates the selected row, and Escape closes the dropdown without changing focus.

### Copy mode

Copy mode is client-local: one attached client's cursor, selection, search, and scroll position do not affect another client. It supports directional `/` and `?` search with `n`/`N` repeats, tmux-style word motions, and full- or half-page navigation with the configured prefix.

### Navigator

The navigator is a workspace/tab/pane chooser.

- Search text matches whitespace-separated terms.
- Filter chips select blocked, working, idle, or done targets.
- Workspace rows can expand and collapse.
- Selection accepts a workspace, tab, or pane target.
- Mouse hover moves selection; row clicks accept targets.

### Command palette and command panel

Gardn can discover and run project commands. The command palette is also a general action surface for app, workspace, group, tab, pane, layout, agent-scope, settings, reload, notification, and detach/quit actions.

- Commands are scoped from the active workspace or selected workspace while navigating.
- Command rows are grouped by repo and branch context.
- Command status sections include running, failed, unknown, and stopped commands.
- Custom keybindings can launch shell helpers or pane commands.
- **Panel actions** — command rows can run, focus, expand, or stop commands from the right sidebar.
- **Project commands.** **Settings > Commands** configures three independent project launchers: **Browser** (`terminal-browser`), **Review** (`hunk diff --watch`), and **Editor** (`fresh .`). Each field is editable. Leaving one empty hides only that action. These actions appear in the command palette and workspace and `+` menus. Browser and Editor run from the workspace directory. Review uses observed Git repositories.
- **Native GitHub.** GitHub opens a built-in, client-local ratatui screen rather than a terminal command or companion process. It uses the existing authenticated `gh` CLI and Gardn's palette, theme, scrollbars, and settings. There is no separate GitHub command, companion version, configuration, or theme to manage.
- **Space GitHub scope.** **Space Settings > GitHub** offers Automatic, Selected repositories, and Group organization modes. Automatic uses GitHub repositories discovered from the Space's local directories. If it finds none, it uses the Group organization or the signed-in user's queues. Discovery errors remain visible. Selected repositories accepts exact `owner/repository` identities and overrides the Group organization. Press Enter to save the repository list. Selecting a repository within GitHub narrows the view without widening its base scope. Scope changes close an invalidated view. Reopening GitHub applies the current settings.
- **GitHub workflows.** Overview summarizes scoped work. Pull requests and issues include conversations, comments, edits to your own comments, labels, and close actions. Pull requests also support draft state, split or unified diffs, wrapping, whitespace controls, file navigation, inline range reviews, and safe merge, auto-merge, or queue actions. Actions lists scoped workflow runs with jobs, steps, and log links. Filters apply to loaded results. **More** loads another page.
- **GitHub limits and attribution.** Native GitHub does not offer admin merge, branch deletion, outside-scope browsing, Worktrunk review Spaces, agent handoffs, or a matching-Space action. Adapted ghui behavior retains Kit Langton's MIT attribution.
- **Command resets.** Each project command has a reset action. **Reset All Commands** restores the three built-in command values.
- **Command discovery** — Gardn discovers VS Code tasks, package scripts, just recipes, Make targets, and defaults for common Cargo, Go, Java, Python, .NET, PHP, and Ruby projects.
- **Managed reruns** — rerunning a managed command focuses an existing run or restarts a stopped/failed run in the same pane instead of spawning duplicates.

### Activity panels

The right sidebar can show agents, commands, and ports. Port entries include active/stale state, exposure labels, owner context, and click-to-focus behavior when an owner pane is known.
Shared ports can list multiple owner panes when more than one pane/process-tree owner is attributed to the same listener.

### Settings modal

Settings are edited in an in-app modal.

The left sidebar includes:

- Appearance
- Notifications
- Behavior
- Commands
- Connections
- Agents
- Integrations
- Advanced

The general settings modal uses an expandable sidebar. Each category exposes subsection links that jump to the matching content group. The right panel keeps the category heading and description. A horizontal rule separates this introduction from the controls, and blank rows separate logical setting groups and independent fields. Press `tab` to move keyboard focus between the sidebar and the settings controls. Group and Space Settings use compact tabs. Group Settings separates General, Space defaults, Appearance, Agents, and GitHub. Space Settings keeps its name and execution location in General, with repository scope in GitHub. All settings modals support mouse navigation, scrollbars, and a top-right `Esc Close` action. GitHub text fields save on Enter; other controls retain their existing immediate updates. Visible control, menu, navigation, status, and modal labels use title casing. Integrations can select Local or a configured SSH connection before it checks, installs, updates, or uninstalls an integration. Appearance owns theme, sidebar, and pane-label settings. Notifications owns sounds and toasts. Behavior owns prompts and terminal defaults. Commands owns the Git, Diff, IDE, and GitHub project launchers. Agents uses a browse-and-edit workflow for launch profiles, with a separate danger zone for deletion.

Appearance > Panes exposes `Pane Borders`, `Pane Scrollbars`, `Pane Gaps`, `Hide Single-Tab Bar`, and `Pane Border Agent Info`. Appearance > Window edits the outer terminal window-title template. Behavior > Terminal exposes `Default Shell` and `Shell Startup Mode` before new-terminal CWD and mouse-wheel speed. Notifications keeps Sound Alerts, expands Notification Popups with background-alert delay and in-app toast position, and adds Clipboard Feedback for copy confirmation. Advanced > Updates toggles background version and manifest checks after Server. Agents can disable a custom profile without changing its id, env, or order. New profiles start enabled.

The Connections section manages SSH execution hosts and their workers. A saved connection opens to its status and runtime controls. Editing persistent details is a separate action. The new-connection form starts with the SSH target and uses it as the display name when the optional name is empty. Connecting installs or updates the versioned execution worker automatically. A compatible worker with live runtimes stays active until it is unused. Removing a connection first inventories every session and managed worker binding, shows each affected Group, Workspace, pane, pending termination, and owned binding, and requires confirmation. Affected Workspace defaults move to the displayed local home directory. Gardn then fences new work, drains or closes remote panes, rewrites dormant session placement, removes only bindings owned by the connection, and deletes the connection profile only after all sessions are clear. A durable journal keeps a partial removal fenced and resumable after restart. If full cleanup is unavailable, the failure screen states that remote processes or files might remain and offers **Remove Saved Connection**, **Try Again**, and **Cancel**. Testing a saved connection reports the result immediately, including when the host is already connected.

After a coordinator restart, restored remote panes reconnect their saved SSH connection and re-adopt the live worker runtime automatically.

### Help and confirmations

Gardn includes a scrollable keybinding help modal generated from current bindings, including custom command bindings. Press `/` to filter by shortcut, action, or group. Esc leaves the filter first, then closes the modal. Destructive actions such as workspace close and group delete use confirmation dialogs that show the affected target.

### Global menu

The global menu exposes settings, keybinding help, config reload, update/release-note actions, and detach from sidebar and mobile menu surfaces.

## Integrations

Gardn ships installable integrations for agents that report semantic state, native session identity, or both over the socket API.

Built-in installable integrations:

- pi
- OMP
- Claude Code
- Codex
- Copilot
- Devin
- Kimi
- Droid
- OpenCode
- Hermes
- Qoder CLI
- Cursor
- Grok Build
- Qwen Code
- Kilo
- MastraCode
- Antigravity CLI

Integration management supports:

- install
- uninstall
- status checks
- outdated-version detection
- in-app integration management
- coordinator and configured SSH execution-host selection

Integration install side effects are agent-specific: pi and OMP install extensions; OpenCode and Kilo install JavaScript plugins; Hermes installs and enables a plugin; and the remaining built-in targets install hooks or settings without replacing foreign configuration.

Kilo and MastraCode integrations report native session identity and lifecycle state directly. Qwen Code and Antigravity CLI report session identity only, so their screen manifests provide blocked, working, and idle state. MastraCode and Antigravity CLI provide verified native restore commands.

Integration management runs on the selected host. SSH integration operations use the managed execution worker. Agent panes on an SSH host send lifecycle reports to a restricted worker-local endpoint. They do not receive the coordinator Local API socket.

Integration path overrides include `PI_CODING_AGENT_DIR`, `PI_CONFIG_DIR`, `CLAUDE_CONFIG_DIR`, `CODEX_HOME`, `COPILOT_HOME`, `DEVIN_CONFIG_DIR`, `GROK_HOME`, `HERMES_HOME`, `KIMI_CODE_HOME`, `QODER_CONFIG_DIR`, `QWEN_HOME`, `ANTIGRAVITY_CLI_CONFIG_DIR`, and `CURSOR_CONFIG_DIR`. OMP install/status checks scan `.omp` and `.omp-*` extension directories.
- On Windows, Qwen Code, Kilo, MastraCode, and Antigravity CLI use their upstream-supported PowerShell or JavaScript integration assets.


## Plugins

Gardn plugin v1 lets local extensions add actions, panes, link handlers, and event hooks through the Gardn socket API and CLI.

Plugin manifests use `gardn-plugin.toml` with `min_gardn_version`, or Herdr v0.8.2 compatibility manifests use `herdr-plugin.toml` with `min_herdr_version`.

Plugins run unsandboxed as the current user. Remote installs show source, build commands, actions, panes, link handlers, and event hooks before install, and require confirmation unless `--yes` is passed.

Installed and linked plugins live in one user-level registry shared by the default and named sessions. Legacy per-session registries migrate into that global registry, and registry entries survive live server handoff.
`gardn plugin install`, `gardn plugin uninstall`, `gardn plugin link`, and `gardn plugin list` can read or update the registry while no server is running. Runtime operations such as actions, hooks, panes, enable/disable, and `plugin unlink` still require the server.
Enabled, platform-compatible `[[startup]]` commands run once after server readiness, including after live-handoff replacement. Refreshing plugin manifests does not replay them.
Plugin panes support overlay, popup, split, tab, and zoomed placement on the coordinator execution host. Popup dimensions accept terminal cells or percentages and are valid only for popup placement. Plugin v1 rejects a pane whose selected Workspace or source pane resolves to an SSH execution host before it creates a pane. Overlay and popup placement use a detached runtime owned by the requesting client. Split, tab, and zoomed placements are normal session panes; their attribution follows pane moves and is removed when tabs, workspaces, layouts, or plugins remove the pane.
Plugin commands receive protected dialect-compatible context variables. Gardn manifests use `GARDN_*`; Herdr v0.8.2 manifests also receive protected `HERDR_*` aliases that plugin-provided env overrides cannot replace.

## External tools

Gardn is a terminal workspace manager, so some features call user-installed tools instead of bundling every backend.

| Tool | Used for | Requirement |
| --- | --- | --- |
| `git` | Git status, repository discovery, and Git-aware project commands. | Required for Git-aware features. |
| Configured project commands | Browser, review, and editor actions from project contexts. Configure `[commands].browser`, `[commands].review`, and `[commands].editor`; defaults are `terminal-browser`, `hunk diff --watch`, and `fresh .`. | Each tool is optional and required only when its configured action is used. |
| `gh` | Authentication, reads, and mutations for the built-in native GitHub screen. | Required for GitHub workflows. Uses existing `gh` authentication. No ghui companion install is required. |
| Agent CLIs such as `pi`, `omp`, `claude`, `codex`, `grok`, `opencode`, `hermes`, `copilot`, `kimi`, `droid`, `qodercli`, and `cursor-agent` | Launching agent panes and installing/updating matching Gardn integrations. | Required only for the agent/profile the user launches or integrates. |
| `python3` | Installed hook scripts for agent integrations. | Required for hook-based state/session reports; hooks exit quietly when it is missing. |
| `curl` | Update checks, release downloads, manifest refreshes, and remote bootstrap downloads. | Required for those networked update/bootstrap features. |
| `ssh` | Remote attach, remote install, and remote client bridge. | Required for remote features. |
| `lsof` | Local TCP listener discovery for the ports panel. | Optional; missing or failing probes produce no port observations. |
| macOS `pbcopy`, `pbpaste`, `open`, `/usr/bin/osascript`, optional `terminal-notifier`, and optional `mdfind` | Clipboard, URL opening, and system notifications on macOS. | Platform helpers; Gardn falls back where possible. |
| Linux `xdg-open`, `notify-send`, `wl-copy`, `wl-paste`, `xclip`, and `xsel` | URL opening, system notifications, and clipboard/image paste on Linux. | Optional per feature and display server; missing helpers disable the matching bridge/fallback. |
| macOS `afplay` | Custom sound notification playback. | Required only for custom notification sound playback on macOS. |

## CLI and socket API

Gardn exposes the same runtime model through the CLI and local Unix socket API.

### CLI areas

- **`gardn status`** — show client/server status and protocol compatibility.
- **Protocol guard** — operational CLI commands verify the server wire-protocol version before dispatch and return a request-correlated JSON error with update/restart guidance on mismatch; status checks and live handoff remain available for diagnosis and recovery.
- **`gardn session`** — list, attach, stop, and delete named sessions.
- **`gardn workspace`** — manage workspaces.
- **`gardn tab`** — manage tabs.
- **`gardn pane`** — manage panes, read output, send input, report agent state, and run commands.
- **`gardn agent`** — list, inspect, focus, read, send encoded keys to, prompt, wait for, attach to, rename, and start agents.
- **`gardn agent explain`** — inspect why an agent pane is classified as idle, working, blocked, unknown, or skipped by manifest detection.
- **`gardn wait`** — wait for output matches or agent status changes.
- **`gardn integration`** — install, uninstall, and inspect agent integrations.
- **`gardn group`** — list, create, focus/switch, rename, and delete workspace groups.
- **`gardn config reset-keys`** — remove custom keybindings while preserving the rest of the config.
- **`gardn update`** — self-update supported binary installs; `--handoff` can preserve live panes while moving running sessions to the updated server.
- **`gardn server`** — run the headless server, stop it, reload config, or trigger a live handoff.
- **`gardn api`** — print or write the generated public API schema and request a live session snapshot.
- **Launch flags** — `--no-session`, `--default-config`, `--skill`, and `--remote-keybindings <local|server>` control startup, skill output, and remote behavior.
- **JSON output** — status and session commands expose machine-readable output where supported.
- **Read modes** — pane and agent reads support visible, recent, recent-unwrapped, ANSI, raw, and bounded line output.
- **Wait matching** — output waits support substring or regex matching, raw matching, timeouts, and agent-status waits.
- **Automation reads** — pane and agent output can be consumed as rendered visible text, recent scrollback, ANSI, or raw output for agent feedback loops.

### Socket API

The socket API supports typed request/response calls and event subscriptions. It is the local JSON control plane; interactive render streaming and terminal attach use the separate client wire-protocol socket.

The public website combines authored transport, lifecycle, trust, compatibility, workflow, and error guidance with deterministic shape reference generated from a specified `gardn` binary. Published schema JSON is immutable at `/api/<product-version>/schema.json`; the `/api/latest/schema.json` alias is reserved for release deployment. Generated Local API material excludes the separate internal client wire and handoff protocols.

API-visible domains include:

- server control
- workspaces
- tabs
- panes
- agents, client-local agent views, and prompt/wait automation
- integrations
- output reads
- output waits
- event subscriptions
- session snapshots
- terminal observe and control streams
- pane scroll state
- workspace groups
- integration authority reports
- protocol and capability ping

## Appearance and notifications

### Themes

Gardn supports terminal-derived colors and built-in palettes.

- **Theme source** — terminal colors or theme palettes.
- **Appearance mode** — system, light, or dark.
- **Light and dark palette selection** — choose separate palettes when system mode is enabled.
- **Live system sync** — in system mode, Gardn follows foreground host-terminal light/dark color changes while it is running and refreshes pane terminal defaults.
- **Nested terminal palette** — pane applications that query ANSI colors receive the active host palette. Application-defined palette colors keep precedence until the application resets them.
- **Group settings** — rename groups, choose a group icon, set the default location and directory for new spaces, set the GitHub organization inherited by spaces, assign per-group theme accent colors, choose favorite/default agent profiles, or inherit the global accent. An SSH group default can be saved without a directory; Gardn uses the connection's suggested directory.
  Press Enter to save the GitHub organization. Editing keeps the current organization unchanged until the new value is valid and saved.

- **Accent color** — choose highlight, border, and navigation accent from the built-in theme palette or, when following terminal colors, from the six terminal ANSI accents (with separate light and dark choices).

### Sound and toasts

- **Toast delivery** — off, Gardn, terminal, or system.
- **Sound notifications** — request and done sounds for background agent activity.
- **Per-agent sounds** — agent-specific sound overrides.
- **Validation** — invalid or missing sound files fall back to defaults and emit diagnostics.
- **Terminal toast backends** — terminal toasts use supported terminal notification protocols, including tmux passthrough where available.
- **Custom sound files** — request/done sounds can use MP3 files resolved relative to the config file.
- **Sound disable switch** — `GARDN_DISABLE_SOUND` disables playback.

## Configuration

Configuration file: `~/.config/gardn/config.toml`.
Gardn treats `config.toml` as a stable hand-editable configuration surface. Settings modal changes rewrite their owned keys or sections, preserve unrelated sections, and reload the file into the running app after successful writes.

Runtime reload is section-scoped for live sections: valid sections apply, invalid sections keep the previous live settings and emit diagnostics through the app/server reload path.
- **Offline validation** — `gardn config check` validates `config.toml`, prints diagnostics, and exits without starting or attaching to a session.
- **Configuration status** — startup and reload diagnostics raise one transient toast, then remain available from the bottom-left `Config Issue` status and its diagnostics modal until a successful reload clears them.


Configurable areas include:

- onboarding
- theme
- terminal shell and new-terminal cwd policy
- session restore
- keybindings
- indexed shortcuts
- custom command keybindings
  Shell actions and temporary pane actions run through the platform command interpreter: `/bin/sh` on Unix and `cmd.exe` on Windows.
- multiple bindings per action
- prefix-mode and direct key chords
- sidebar size, initial state, and mouse behavior
- close and naming prompts
- initial agent panel scope
- pane border agent metadata
- toast and sound settings
- scrollback limit
- experimental features

## Updates and release notes

Direct installs use GitHub Releases for update checks, release metadata, and binary downloads on Linux, macOS, and Windows. mise and Nix-managed installs are routed to their package manager instead of self-update. On macOS, Gardn.app owns updates through Sparkle when it is installed. The standalone `gardn update` command refuses for the stable Direct CLI (`~/.local/bin/gardn`) in that case. `gardn-beta`, `gardn-dev`, mise, and Nix keep their own update owners.

- When a newer release is available, the sidebar `?` control stays on `? Update Ready` until you install it.
- `gardn update` downloads and swaps supported direct binary installs.
- mise and Nix-managed installs are blocked from self-update and should use their package manager.
- Live handoff can preserve running pane processes during updates when both the old and new server support the handoff protocol.
- Windows direct updates use the stable `gardn-windows-x86_64.exe` release asset; Gardn does not use a preview channel.
- In-app release notes can be shown after an update.
- Post-update checks can report outdated integrations.
- Product announcements can be shown separately from release notes and tracked as seen per version.
- First-run onboarding introduces the core workflow in-app.
- Update-ready dialogs can show release notes and the install command before the update is applied.

## Fork maintenance

Gardn tracks commits from `ogulcancelik/herdr` with an explicit port ledger.

- **Upstream port ledger** — `upstream-port-map.json` records each upstream commit as ported, superseded, skipped, or pending.
- **Ledger check** — `just upstream-status` reports upstream status and fails when commits are unclassified or still pending.
- **Sync guard integration** — upstream-sync reports include the ledger status so product-specific skips and superseded changes stay visible.
- **Gardn-owned surfaces** — docs, website, release, and repository-process commits can be skipped with explicit reasons instead of silently reintroducing upstream identity.

## Experimental features

Experimental options currently include:

- nested Gardn sessions
- local Kitty graphics rendering for attached clients. New installs default it on. A zoomed or single-pane focused terminal blits a full-grid virtual image without walking every cell. Local uploads larger than 8KiB use a Kitty temp file instead of base64 pixel bytes. Toggle it from **Settings → Experiments → Terminal Graphics**. Reconnect Gardn to apply.
- CJK IME hidden-cursor anchoring
- agent-scoped CJK IME anchoring
- configurable CJK IME anchor cursor shape
- macOS prefix-mode ASCII input-source switching
