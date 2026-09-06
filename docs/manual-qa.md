# Manual QA matrix

Use this guide to select and run the manual checks that Gardn's automated suite cannot prove reliably. It complements `pnpm check`; it does not repeat state, protocol, socket, PTY, or render behavior already covered by automated tests.

Run M01-M08 before tagging a release, then run M09 against the published artifacts. Run affected P1 cases when changing their surface, and run the full P1 set for broad platform, terminal, or lifecycle changes.

## Test record

Record this environment before each run:

- commit SHA and Gardn version
- binary source and checksum
- OS and architecture
- terminal application and version
- shell
- session and config namespace

Record each selected case as `PASS`, `FAIL`, or `BLOCKED`. For failures, preserve the relevant Gardn logs, exact reproduction steps, and a screenshot or short recording when presentation matters. Track each defect separately and link it from the run record.

## Matrix

| ID | Priority | Surface | Required environment | Manual risk |
| --- | --- | --- | --- | --- |
| M01 | P0 | First launch and core TUI | macOS arm64, Ghostty, and one non-Kitty terminal | Visible layout, focus, hit targets, onboarding |
| M02 | P0 | Terminal input and output | Real terminal, IME, mouse-reporting app | Unicode width, paste, keyboard protocol, selection, graphics |
| M03 | P0 | Detach, reattach, named sessions | Two terminal windows | Process continuity and session isolation |
| M04 | P0 | Two live app clients | Wide desktop and narrow/mobile terminals with different dimensions | Per-tab control, size mismatch, watcher isolation, explicit takeover |
| M05 | P0 | Restore and persistence | Rich saved session | Layout, cwd, history, and session identity after restart |
| M06 | P0 | Live handoff and update | Long-running PTY and TCP listener | Process loss, duplicate ownership, stale sockets |
| M07 | P0 | Real agent lifecycle | Grok Build and one established integration | Authentication, lifecycle reporting, parent state, restore |
| M08 | P0 | Remote attach and bootstrap | Reachable Linux SSH host | Transport, bootstrap, compatibility prompt, reconnect |
| M09 | P0 | Release artifacts | macOS arm64, Linux x86_64, Windows x86_64 | Interactive behavior of downloaded binaries |
| M10 | P1 | Host bridges | macOS and Linux where available | Clipboard, URL, toast, notification, and sound helpers |
| M11 | P1 | Mouse, responsive UI, external tools | Wide and narrow terminals | Drag geometry, compact layout, commands, ports |
| M12 | P1 | Sleep, wake, and recovery | macOS laptop and abrupt client loss | Recovery under real OS lifecycle events |

## M01: First launch and core TUI

1. Use an isolated `gardn-dev` configuration or disposable OS user and a named QA session.
2. Launch with no server, complete onboarding, and confirm the first shell is usable.
3. Create two workspaces, a group, three tabs, and a three-pane layout.
4. Navigate the sidebar, tabs, global menu, navigator, command palette, Settings, help, and confirmation dialogs once by keyboard and once by mouse.
5. Resize from wide to approximately `60x20`, then return to wide.

Pass when no control becomes inaccessible or misleading, no stale hover or focus remains, the layout stays coherent, and destructive dialogs identify the correct target.

## M02: Terminal input and output

1. Type and paste ASCII, multiline text, CJK, emoji, combining characters, and an IME-composed phrase into a shell and editor.
2. Exercise arrows, modifiers, function keys, Kitty CSI-u input, and legacy application key modes in an editor or TUI.
3. Produce long scrollback; scroll, search, enter copy mode, drag-select, double-click-select, and paste the copied result.
4. Run a mouse-reporting application and test normal mouse handling plus configured right-click passthrough.
5. Display an OSC 8 hyperlink and a Kitty image where supported.

Pass when input has no dropped or duplicated bytes, character widths remain aligned, modifiers do not stick, selections and scroll position remain stable, and supported links and images render and clear correctly.

## M03: Detach, reattach, and named sessions

1. Start a visible counter and a local HTTP listener in separate panes.
2. Detach, wait for additional counter output, and reattach.
3. Open a second named session and verify its workspaces and processes are isolated.
4. Close one client abruptly, reconnect, and verify both workloads remain alive.

Pass when output advances while detached, pane targets remain usable, the server survives client loss, and no state crosses named-session boundaries.

## M04: Two live app clients

1. Attach two real app clients with materially different dimensions, including a wide desktop viewport and a narrow/mobile viewport. Create or identify one stable tab.
2. Have client A claim the free tab, then attach client B to that same tab. Confirm A is the controller and B is a view-only watcher. Switch B to another free tab and confirm it may claim that tab, then return to the tab controlled by A.
3. In B, navigate and focus panes, scroll, search, and use copy mode. Resize B and send focus, mouse, and keyboard input while it watches A's tab. Confirm those actions remain client-local and do not change the controller's PTY size or terminal content; the watcher viewport crops or pads the controller-sized canvas without a layout shift.
4. From the watcher, use `prefix+t` to take control and confirm that B becomes the controller and the canonical PTY resizes to B's dimensions. Repeat takeover from the persistent desktop/mobile **Take control** action and confirm the same explicit transition.
5. Have the controller navigate to another tab. Confirm control is released and the remaining watcher is not auto-promoted; it must explicitly take control before resizing or sending tab input.
6. Disconnect the controller while a watcher remains connected. Confirm the tab is unowned, the watcher remains view-only, and explicit takeover is required.
7. Directly attach to the tab's terminal from another client and confirm terminal-level exclusivity remains in force; only the direct attach takeover flow can replace its attach owner. Exercise a Local API request and a system-automation request with explicit tab/pane ids while the tab is interactively occupied, and confirm both succeed without claiming or changing the interactive controller.
8. Detach each app client independently.

Pass when the first-client/free-tab and explicit-takeover rules are visible, watcher navigation/scroll/copy/search and differing dimensions stay local, no layout shifts occur before takeover, takeover alone changes canonical PTY size, navigation/disconnect release without auto-promotion, direct attach remains terminal-exclusive, and Local API/system automation bypasses interactive Tab Control.

## M05: Restore and persistence

1. Build a session containing groups, custom accents or icons, several workspaces and tabs, split layouts, zoom, cwd changes, labels, scrollback, and a resumable agent session.
2. Record the visible state, stop the server cleanly, and relaunch the same session.
3. Verify layout, active targets, cwd, labels, history, and agent identity.
4. Resume the agent and verify conversation continuity without replayed pane-history noise.

Pass when no workspace is lost, active tabs and panes remain correct, cwd and labels persist, focus remains coherent, and the agent session is neither lost nor duplicated.

## M06: Live handoff and update

1. Run a counter, an interactive shell, and a TCP listener with a recognizable response.
2. Perform the supported handoff or update from the current binary to the candidate binary.
3. During and after handoff, probe the listener, type into the shell, and confirm counter continuity.
4. Reattach a fresh client and inspect status and logs.

Pass when PTYs and the listener survive, every pane has one owner, input remains live, output is not duplicated, and no stale socket or surprise restart appears.

## M07: Real agent lifecycle

1. Install the candidate Grok Build integration through Settings and confirm its status is current.
2. Launch real Grok Build and exercise prompt submission, a tool call, a permission or elicitation block, compaction, a subagent, stop or idle, and session end.
3. Verify Gardn's working, blocked, idle, and release transitions. Verify child completion never idles or releases the parent and the pane is never labeled as another agent.
4. Restart or restore and verify native Grok session continuity.
5. Repeat the core working, blocked, and idle path with an established direct integration such as Claude Code or Codex.
6. Uninstall Grok and verify manifest detection remains a usable fallback and missing-integration guidance is accurate.

Pass when state matches the visible agent, identity remains stable, restore works, and install or uninstall changes only Gardn-owned integration files.

## M08: Remote attach and managed worker lifecycle

1. Attach to a clean Linux host over SSH with no running Gardn server and exercise standalone bootstrap.
2. Create a pane workload, interrupt the SSH connection, and reconnect.
3. Repeat with an older remote Gardn binary to exercise the standalone compatibility and restart prompt.
4. Verify resize, keyboard input, direct terminal attach, and clipboard behavior supported by the client and host pair.
5. Save the same host in **Settings > Connections**. Connect without a manual worker install and verify that Gardn installs the current managed worker.
6. Stop and restart the local coordinator while the remote terminal remains active. Verify that the saved connection reconnects without a manual **Connect** action and that the pane renders the preserved terminal output.
7. Keep a remote terminal active, connect with a newer compatible worker version, and verify that the active runtime is not interrupted. End the runtime and verify that the deferred worker update activates.
8. Reference the connection from two named local sessions. Start removal, verify that inventory lists both sessions and owned bindings, then confirm. Interrupt one removal after approval and restart the server to verify journal recovery.
9. Repeat with the remote host unavailable. Verify that full removal fails closed. Verify that the failure screen identifies the connection, warns that remote processes or files might remain, and offers **Remove Saved Connection**, **Try Again**, and **Cancel**.

Pass when prompts are accurate, transport loss and coordinator restart do not lose workloads, restored remote panes reconnect automatically, updates do not interrupt compatible live runtimes, retirement removes only Gardn-owned state, and an approved partial retirement resumes after restart.

## M09: Downloaded release artifacts

Use downloaded release artifacts rather than local Cargo builds.

1. On macOS arm64, Linux x86_64, and Windows x86_64, verify the filename and checksum, executable launch, `--version`, status, first server start, and interactive shell input.
2. Exercise create, split, detach, and reattach once on each platform.
3. On Windows, use a real ConPTY terminal and verify resize, modified keys, paste, and clean shutdown.
4. Smoke the macOS x86_64 and Linux aarch64 artifacts on native hardware or supported emulation when available.

Pass when the version matches the tag, no runtime dependency is missing, and the core interaction path works on each required platform.

## M10: Host bridges

Exercise OSC 52 text copy, image paste, URL opening, terminal toast, system notification, default and custom sounds, and missing-helper fallback on each applicable OS.

Pass when each enabled bridge reaches the host once, disabled or missing helpers fail safely, and remote panes do not write to the wrong clipboard.

## M11: Mouse, responsive UI, and external tools

1. Drag workspace or group rows, tabs, and pane borders; scroll every list and modal; test context menus and inline close controls.
2. Exercise the compact layout at narrow widths.
3. Discover, rerun, and stop a project command. Focus a real port owner.
4. Verify that **Settings > Commands** contains only Browser, Review, and Editor. Reset them and confirm `terminal-browser`, `hunk diff --watch`, and `fresh .`. Open each from the command palette and workspace menus.
5. With an authenticated `gh` CLI, open GitHub from the command palette and workspace menu. Confirm that it opens a native screen without creating a companion terminal pane. Open GitHub in a second app client and confirm that navigation in one client does not move the other.
6. In a Space with local GitHub checkouts, open GitHub with Automatic scope. Confirm that results stay within the discovered repositories. Narrow to one repository, then remove the narrowing. Confirm that the original scope returns. In a Space without discovered repositories, check the Group organization fallback and, without a Group organization, the signed-in user's queues.
7. Save exact repositories in **Space Settings > GitHub** with Enter. Reopen Settings and verify persistence. Confirm that changing scope closes the invalidated GitHub view. Reopen GitHub and check the saved scope. Repeat with Group organization mode. Confirm that repository narrowing never exposes an outside-scope repository.
8. Open Overview, pull requests, issues, and Actions. Apply a filter and confirm that it filters loaded results rather than claiming a complete server search. Use **More** to load another page. Inspect an Actions run, its jobs and steps, and a log link.
9. In a disposable repository, read a conversation, add a comment, edit your own comment, change labels, and close an issue. Exercise pull request draft state, file navigation, split and unified diffs, wrapping, whitespace controls, and an inline range review. Check safe merge, auto-merge, and queue actions only on disposable pull requests with the required repository settings.
10. Check native GitHub keyboard and mouse navigation, the active Gardn theme, scrollbars, and narrow-terminal rendering. Confirm that no companion installation, version, config, or theme setup is required. Confirm that admin merge, branch deletion, outside-scope browsing, review Space creation, agent handoffs, and a matching-Space action are absent.

Pass when hit areas match their visuals, compact layouts retain required controls, reruns reuse managed command tabs, and port focus selects the owning pane. GitHub must keep client navigation independent, enforce the configured Space scope, paginate explicitly, and apply mutations only to the selected target.

## M12: Sleep, wake, and recovery

1. Leave a counter and listener active, sleep and wake macOS, then reattach.
2. Abruptly kill a client during resize or input, relaunch, and verify stale state converges.
3. Repeat after restarting the terminal application.

Pass when the server and workloads survive, sockets recover, no stuck mouse or input mode remains, and no manual state-file cleanup is required.

## Release gate

A release is manually cleared when:

- M01-M08 pass against the release candidate, including the M08 attach, worker update, and retirement paths against a real Linux SSH host
- M09 passes against the published macOS arm64, Linux x86_64, and Windows x86_64 artifacts
- the published macOS x86_64 and Linux aarch64 artifacts launch and report the correct version on native hardware or supported emulation
- no unresolved failure risks data or process loss, wrong input targeting, unsafe destructive action, unusable rendering, broken restore, or release artifact startup
- every P1 failure has a linked issue and an explicit ship or no-ship decision

After preserving evidence, remove QA sessions, integrations, and remote test state.
