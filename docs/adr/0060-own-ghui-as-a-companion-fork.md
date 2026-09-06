---
status: superseded
---

# Own ghui as a separately released companion fork

## Superseded by native GitHub

Gardn now owns GitHub as a built-in, client-local ratatui screen using the existing authenticated `gh` CLI. The companion release pin, launcher, command setting, and separate configuration and theme contract no longer apply. Browser, Review, and Editor remain the three configured project commands.

The native screen retains Space-scoped Overview, pull requests, issues, diffs and reviews, safe merge flows, and Actions. Automatic scope uses discovered repositories, then the Group organization or personal queues when no repositories are discovered. Selected repositories uses exact repository identities. Group organization uses the configured organization. Repository narrowing never widens the base scope. Scope changes close an invalidated view, and reopening applies current settings.

Native GitHub excludes admin merge, branch deletion, outside-scope browsing, Worktrunk review Spaces, and agent handoffs. A matching-Space action is not implemented. Adapted ghui behavior retains Kit Langton's MIT attribution.

## Historical decision

The text below records the superseded companion decision. It is not current installation, launch, or handoff guidance.

Gardn uses `masakirocorp/ghui` as its curated GitHub interface. The fork remains a separate MIT-licensed work. Gardn does not copy ghui source into the Gardn repository or link it into the AGPL binary.

Gardn pins each supported ghui integration to one immutable fork release and source commit. The curated launcher rejects a different ghui version instead of silently losing launch-scoped behavior. A user can configure a different `[commands].github` command to opt out of the curated integration.

The fork owns Gardn-specific launch inputs. These include terminal theme selection, visible scrollbars, Space identity, repository scope, and verified local checkout paths. Each Space stores an Automatic, Selected repositories, or Group organization intent. Gardn resolves that intent at launch. The resolved scope filters HOME collections and remains fixed until ghui exits. Selecting a repository narrows the view without replacing that base scope. Explicit outside-scope views remain available.

Masakiro publishes ghui from its own repository and release workflow. Release assets retain Kit Langton's copyright notice and the MIT License. Gardn documents the pinned release and exposes the same acknowledgment in **Settings > About**. A Masakiro Homebrew tap can package the immutable release assets without transferring update ownership to Gardn.

Upstream changes enter the fork through explicit merges from `kitlangton/ghui`. Gardn treats upstream as input, not authority. The fork can diverge when Gardn needs a product invariant that upstream does not accept or schedule.

## Historical rationale

Gardn is mouse-first. ghui has the interaction model that fits that product direction, but upstream does not currently provide Group-level organization scope or all required launch-only presentation controls. An unpinned executable can ignore those inputs and expose unscoped data. A reviewed fork release makes the behavior and license boundary explicit.

Bundling ghui into Gardn would couple two release cadences and mix optional third-party source into the Gardn build. A separately released companion keeps the dependency replaceable while Masakiro still owns the exact behavior that the curated launcher requires.

Repository context belongs to a Space rather than its current pane. A shell directory change must not retarget a running GitHub view. Explicit repository selection takes precedence over the Group organization. Automatic discovery uses the Group organization only when it finds no GitHub repositories. Discovery failures must remain visible rather than silently broadening the scope.

Review handoffs use Worktrunk to create a separate checkout at the selected pull request commit. They never switch the original Space's checkout. Agent handoffs require an explicit target and send context through the existing Gardn CLI readiness gate. The companion receives an explicit binary and socket for the launching session, with no ambient-session fallback.

## Historical consequences

A Gardn release that changes the ghui contract must first publish and verify a compatible fork release. The Gardn pin, installation guidance, tests, documentation, and acknowledgment must change together.

Fork releases require upstream synchronization, platform assets, checksums, and license retention. Gardn must fail closed when the required fork version is unavailable.

The default `ghui` command remains optional. Browser, review, editor, and custom GitHub commands keep their independent installation and update paths.
