---
packages:
  gardn: patch
---

# Bring GitHub into Gardn

GitHub now opens as a built-in, client-local ratatui screen using your existing `gh` authentication. It uses Gardn's palette, theme, scrollbars, and settings. There is no companion to install, pin, launch, or configure. Settings > Commands now contains only Browser (`terminal-browser`), Review (`hunk diff --watch`), and Editor (`fresh .`).

Each Space can use discovered GitHub repositories, exact selected repositories, or its Group organization. Automatic scope falls back to the Group organization or personal queues when no repositories are discovered. Repository narrowing never widens the base scope. Scope changes close an invalidated view. Reopen GitHub to apply current settings.

Native GitHub retains Overview, pull request and issue conversations, comments and edits to your own comments, labels, draft state, and close actions. Diffs support split and unified views, wrapping, whitespace controls, file navigation, and inline range reviews. Safe merge, auto-merge, queue actions, and scoped Actions runs with jobs, steps, and log links remain available. Filters apply to loaded results, and More loads another page.

The native screen removes companion launch and configuration plumbing, Worktrunk review Spaces, and agent handoffs. It does not offer admin merge, branch deletion, outside-scope browsing, or a matching-Space action. Adapted behavior retains Kit Langton's ghui copyright notice and MIT License attribution.

Group GitHub organization fields validate and save on Enter. Space Settings keeps repository scope in GitHub, and Group Settings separates General, Space defaults, Appearance, Agents, and GitHub.
