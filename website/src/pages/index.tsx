import { Link } from "fumapress/client";
import { canonicalUrl } from "../site-url";
import {
  CTASection,
  FeatureGrid,
  Footer,
  Hero,
  PlatformCard,
  SessionShot,
  Workflow,
} from "../marketing";

export default function HomePage() {
  return (
    <>
      <title>Gardn | Terminal workspace management for AI coding agents</title>
      <meta
        name="description"
        content="Run AI coding agents, shells, and project context in persistent terminal workspaces that survive disconnects."
      />
      <meta property="og:title" content="Gardn" />
      <meta
        property="og:description"
        content="Terminal workspace management for AI coding agents."
      />
      <meta name="twitter:title" content="Gardn" />
      <meta
        name="twitter:description"
        content="Run AI coding agents, shells, and project context in persistent terminal workspaces."
      />
      <link rel="canonical" href={canonicalUrl("/")} />
      <meta property="og:url" content={canonicalUrl("/")} />

      <main className="gardn-page">
        <Hero
          eyebrow="Terminal workspace manager"
          title="Keep the terminal work. Lose the terminal sprawl."
          actions={
            <>
              <Link className="gardn-action" data-primary="true" href="/docs">
                Read the documentation
              </Link>
              <Link className="gardn-action" href="/docs/getting-started/install">
                Install from source
              </Link>
            </>
          }
          status={
            <>
              <span className="gardn-status gardn-status--live">pre-public</span>
              <span>
                Verified release downloads are not yet available. Check the{" "}
                <Link href="/download">download status</Link> page.
              </span>
            </>
          }
        >
          <p>
            Gardn organizes AI coding agents, shells, and project context into persistent terminal
            spaces. Workspaces, tabs, and panes live inside a session server that survives client
            disconnects, so you can detach, reattach from another terminal, and pick up where you
            left off without losing the live processes behind each pane.
          </p>
        </Hero>

        <SessionShot
          title="The session owns your work; clients are just views"
          caption={
            <>
              Spaces, groups, tabs, pane layouts, runtimes, and agent state belong to the session. A
              client only renders its own view. Read the{" "}
              <Link href="/docs/concepts">product concepts</Link> for the full vocabulary.
            </>
          }
        />

        <SessionShot
          headingId="groups-shot-title"
          src="/groups.png"
          srcDark="/groups-night.png"
          video="/groups.mp4"
          videoDark="/groups-night.mp4"
          title="Filter the session by group"
          alt="The Groups menu open over a Gardn session, listing All plus product, ops, and commerce."
          caption="Groups keep product, ops, and commerce in one session without mixing their spaces."
        />

        <SessionShot
          headingId="follow-up-shot-title"
          src="/follow-up.png"
          srcDark="/follow-up-night.png"
          video="/follow-up.mp4"
          videoDark="/follow-up-night.mp4"
          title="Manage follow-up from an agent row"
          alt="The Remove from Follow Up menu on the claude agent in Follow Up, with triage, working, and idle lists in the sidebar."
          caption="Triage, Follow Up, and Working are first-class. Right-click an agent row to add or remove Follow Up without leaving the current space."
        />

        <Workflow
          title="From launch to reattach"
          steps={[
            {
              title: "Launch Gardn",
              copy: (
                <>
                  Run <span className="gardn-command">gardn</span> to start or attach to the default
                  session. First-launch onboarding walks through the basics.
                </>
              ),
              href: "/docs/getting-started/quick-start",
              label: "Complete the quick start",
            },
            {
              title: "Create a space",
              copy: (
                <>
                  Press <span className="gardn-command">ctrl+b</span>, then{" "}
                  <span className="gardn-command">shift+n</span> to create a workspace. Spaces keep
                  related tabs, panes, and context together.
                </>
              ),
              href: "/docs/guides/workspaces-and-navigation",
              label: "Organize workspaces",
            },
            {
              title: "Start an agent",
              copy: (
                <>
                  Open the command palette with <span className="gardn-command">ctrl+b</span>,{" "}
                  <span className="gardn-command">space</span>, then choose{" "}
                  <span className="gardn-command">new agent</span> to launch a supported profile.
                </>
              ),
              href: "/docs/guides/plugins-and-integrations",
              label: "Manage integrations",
            },
            {
              title: "Open project tools",
              copy: (
                <>
                  Open Browser, Review, and Editor in managed tool tabs. Choose{" "}
                  <span className="gardn-command">open GitHub</span> for the native GitHub screen,
                  using your existing <span className="gardn-command">gh</span> authentication and
                  the Space&apos;s repository scope.
                </>
              ),
              href: "/docs/guides/workspaces-and-navigation#open-curated-project-tools",
              label: "Use curated project tools",
            },
            {
              title: "Split, tab, and resize",
              copy: (
                <>
                  Split panes with <span className="gardn-command">ctrl+b</span>,{" "}
                  <span className="gardn-command">v</span>, add tabs with{" "}
                  <span className="gardn-command">ctrl+b</span>,{" "}
                  <span className="gardn-command">c</span>, and drag borders to resize.
                </>
              ),
              href: "/docs/guides/workspaces-and-navigation",
              label: "Learn navigation",
            },
            {
              title: "Detach and reconnect",
              copy: (
                <>
                  Press <span className="gardn-command">ctrl+b</span>,{" "}
                  <span className="gardn-command">q</span> to detach. The session server keeps your
                  panes running. Run <span className="gardn-command">gardn</span> again to
                  reconnect.
                </>
              ),
              href: "/docs/guides/updates-and-handoff",
              label: "Understand handoff",
            },
          ]}
        />

        <FeatureGrid
          title="What the workspace does"
          features={[
            {
              title: "Spaces and groups",
              copy: (
                <>
                  Organize work into persistent spaces, group them in the sidebar, and collapse or
                  filter by group without moving context out of the session.
                </>
              ),
              href: "/docs/guides/workspaces-and-navigation",
            },
            {
              title: "Curated project tools",
              copy: (
                <>
                  Open Browser, Review, and Editor in managed tabs. Use native GitHub for pull
                  requests, issues, and Actions scoped to the Space&apos;s repositories or Group
                  organization, with Gardn&apos;s theme and existing gh authentication.
                </>
              ),
              href: "/docs/guides/workspaces-and-navigation#open-curated-project-tools",
            },
            {
              title: "Agent awareness",
              copy: (
                <>
                  Detects coding agents running inside panes and surfaces states such as working,
                  blocked, done, and idle. Integrations can report native session identity for
                  restore.
                </>
              ),
              href: "/docs/concepts",
            },
            {
              title: "Remote attach",
              copy: (
                <>
                  Attach to a session over SSH with{" "}
                  <span className="gardn-command">gardn --remote</span>. The remote host can
                  bootstrap a matching binary before the client connects.
                </>
              ),
              href: "/docs/guides/remote",
            },
            {
              title: "Plugins and integrations",
              copy: (
                <>
                  Install built-in agent hooks, link local plugins, or install reviewed GitHub
                  plugins. Plugins run unsandboxed as your user, so review before confirming.
                </>
              ),
              href: "/docs/guides/plugins-and-integrations",
            },
            {
              title: "Live updates and handoff",
              copy: (
                <>
                  On compatible Unix servers,{" "}
                  <span className="gardn-command">gardn update --handoff</span> moves live pane PTYs
                  into a newly installed server without stopping running processes.
                </>
              ),
              href: "/docs/guides/updates-and-handoff",
            },
          ]}
        />

        <PlatformCard
          title="Pre-public status"
          rows={[
            {
              platform: "macOS",
              architectures: "x86_64, aarch64",
              role: "Local client and remote host",
            },
            {
              platform: "Linux",
              architectures: "x86_64, aarch64",
              role: "Local client and remote host",
            },
            {
              platform: "Windows",
              architectures: "x86_64",
              role: "Local client only; not a remote host",
            },
            {
              platform: "WSL",
              architectures: "x86_64, aarch64",
              role: "Follows the Linux path",
            },
          ]}
          actions={
            <>
              <Link
                className="gardn-action"
                data-primary="true"
                href="/docs/getting-started/install"
              >
                Install from source
              </Link>
              <Link className="gardn-action" href="/download">
                Download status
              </Link>
              <Link className="gardn-action" href="/docs/reference/platforms">
                Platform reference
              </Link>
            </>
          }
        >
          <p>
            Gardn is pre-public. Verified release downloads are not yet available, so install from
            the source checkout or the Nix flake. The local client runs on macOS, Linux, and
            Windows; the remote bridge is limited to Unix local clients and Linux or macOS remote
            hosts.
          </p>
        </PlatformCard>

        <CTASection
          title="Start with the docs or the source"
          actions={
            <>
              <Link className="gardn-action" data-primary="true" href="/docs">
                Read the documentation
              </Link>
              <Link className="gardn-action" href="/docs/getting-started/quick-start">
                Open the quick start
              </Link>
              <a
                className="gardn-action"
                href="https://github.com/masakirocorp/gardn"
                rel="noopener noreferrer"
                target="_blank"
              >
                View source on GitHub
              </a>
            </>
          }
        >
          <p>
            The documentation is verified against the current source and local API schema. The
            source repository is the best place to follow development, inspect the release process,
            or install from the latest tag.
          </p>
        </CTASection>

        <Footer>
          <p>Gardn is a terminal workspace manager for AI coding agents.</p>
        </Footer>
      </main>
    </>
  );
}
