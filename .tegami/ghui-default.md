---
packages:
  gardn: patch
---

# Bring GitHub into Gardn

GitHub now opens in a dedicated native tab named `GitHub` for the invoking client. The tab is durable session membership: it survives client detachment, disconnection, and session restart. Each client's GitHub screen and breadcrumb selection are transient. After reattach or restart, focusing the tab initializes them from the current Space scope. The source tab stays open, and only that client selects the new tab. Explicit GitHub Close or an ordinary tab close removes the dedicated tab and restores the source pane when it still exists. The header shows an interactive `GitHub / account / repository` breadcrumb. Narrow panes use one `Scope ▾` control and keep the adaptive toolbar on its own row. There is no companion to install, pin, launch, or configure.

Each Space provides the saved default scope. Breadcrumb browsing can switch to the Space default, personal queues, discovered organizations, all repositories, or one repository without changing saved settings. Repository narrowing never widens the selected account scope.

GitHub uses one adaptive control row. All five primary tabs remain visible. Refresh and Filter stay in the row when they fit. Queue, pagination, detail navigation, and selected-item commands move into the `…` menu as space decreases. The menu does not repeat visible controls. Controls, clickable rows, diff lines, and scrollbars show hover feedback without changing the selected item or keyboard focus.

The `…` menu stays local to GitHub instead of opening the global command palette. It supports mouse and keyboard selection and scrolls in compact panes. Ctrl+P keeps access to the global palette.

Pull request and issue descriptions now separate the title, status, and secondary metadata from the body. Markdown headings, lists, checklists, emphasis, and code render with distinct styling. Links show readable labels and support mouse or keyboard activation. Empty metadata is hidden, and commit hashes use seven characters.

Native GitHub retains Overview, pull request and issue conversations, comments and edits to your own comments, labels, draft state, and close actions. Diffs support split and unified views, wrapping, whitespace controls, file navigation, and inline range reviews. Safe merge, auto-merge, queue actions, and scoped Actions runs with jobs, steps, and log links remain available. Filters apply to loaded results, and More loads another page.

The native screen removes companion launch and configuration plumbing, Worktrunk review Spaces, and agent handoffs. It does not offer admin merge, branch deletion, outside-scope browsing, or a matching-Space action. Adapted behavior retains Kit Langton's ghui copyright notice and MIT License attribution.

Group GitHub organization fields validate and save on Enter. Space Settings keeps repository scope in GitHub, and Group Settings separates General, Space defaults, Appearance, Agents, and GitHub.
