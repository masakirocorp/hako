import AppKit
import Sparkle
import SwiftUI
import UserNotifications

@main
struct GardnMenuApp: App {
    @NSApplicationDelegateAdaptor(ExtraAppDelegate.self) private var delegate

    var body: some Scene {
        Settings {
            EmptyView()
        }
        .commands {
            CommandGroup(replacing: .appSettings) {}
            CommandGroup(after: .appInfo) {
                Button("Check for Updates…", action: ExtraAppDelegate.checkForUpdates)
            }
        }
    }
}

@MainActor
final class ExtraAppDelegate: NSObject, NSApplicationDelegate, NSWindowDelegate {
    static let updaterController = SPUStandardUpdaterController(
        startingUpdater: true,
        updaterDelegate: nil,
        userDriverDelegate: nil
    )

    static func checkForUpdates() {
        updaterController.updater.checkForUpdates()
    }
    let store = AgentStore()
    private let statusItem = NSStatusBar.system.statusItem(withLength: 22)
    private lazy var menuPanel = ExtraMenuPanel(store: store, catalog: store.catalog)
    private var settingsWindow: NSWindow?

    func applicationDidFinishLaunching(_ notification: Notification) {
        if PathCli.shouldClaimPath(bundleURL: Bundle.main.bundleURL) {
            Self.terminateOtherCopies()
            PathCli.installBundledCLI()
        } else if Self.otherCopiesRunning() {
            NSApp.terminate(nil)
            return
        }
        NSApp.setActivationPolicy(.accessory)
        UNUserNotificationCenter.current().delegate = self
        AgentNotifications.requestAuthorization()
        menuPanel.attach(statusItem: statusItem)
        statusItem.button?.imagePosition = .imageOnly
        statusItem.button?.action = #selector(togglePopover)
        statusItem.button?.target = self
        store.onNeedsAttentionChange = { [weak self] alert in
            self?.applyIcon(alert)
        }
        store.onDidFocus = { [weak self] in
            self?.menuPanel.hide()
        }
        store.onOpenSettings = { [weak self] in
            self?.openSettings()
        }
        store.start()
        applyIcon(store.needsAttention)
    }

    private static func terminateOtherCopies() {
        let id = Bundle.main.bundleIdentifier ?? "com.masakiro.gardn.menu"
        let pid = ProcessInfo.processInfo.processIdentifier
        for app in NSRunningApplication.runningApplications(withBundleIdentifier: id)
        where app.processIdentifier != pid {
            app.forceTerminate()
        }
    }

    private static func otherCopiesRunning() -> Bool {
        let id = Bundle.main.bundleIdentifier ?? "com.masakiro.gardn.menu"
        let pid = ProcessInfo.processInfo.processIdentifier
        return NSRunningApplication.runningApplications(withBundleIdentifier: id)
            .contains { $0.processIdentifier != pid }
    }


    func applyIcon(_ alert: Bool) {
        statusItem.button?.image = StatusItemImage.make(alert: alert)
    }
    @objc private func togglePopover(_ sender: Any?) {
        if menuPanel.isShown {
            menuPanel.hide()
        } else {
            NSApp.activate(ignoringOtherApps: true)
            store.refresh()
            menuPanel.show()
        }
    }

    func openSettings() {
        menuPanel.hide()
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        let window = settingsWindow ?? makeSettingsWindow()
        settingsWindow = window
        window.makeKeyAndOrderFront(nil)
    }

    private func makeSettingsWindow() -> NSWindow {
        let controller = NSHostingController(
            rootView: ExtraSettingsView(
                store: store,
                catalog: store.catalog,
                checkForUpdates: Self.checkForUpdates
            )
        )
        let window = NSWindow(contentViewController: controller)
        window.title = "Settings"
        window.styleMask = [.titled, .closable, .miniaturizable, .resizable]
        window.titleVisibility = .hidden
        window.toolbarStyle = .unified
        window.setContentSize(NSSize(width: 520, height: 400))
        window.minSize = NSSize(width: 520, height: 400)
        window.isReleasedWhenClosed = false
        window.delegate = self
        window.center()
        return window
    }

    func windowWillClose(_ notification: Notification) {
        guard (notification.object as AnyObject?) === settingsWindow else { return }
        DispatchQueue.main.async {
            NSApp.setActivationPolicy(.accessory)
        }
    }
}

extension ExtraAppDelegate: UNUserNotificationCenterDelegate {
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .list, .sound])
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        let terminalId = response.notification.request.content.userInfo[AgentNotifications.terminalIdKey] as? String
        Task { @MainActor in
            if let terminalId {
                store.focus(terminalId: terminalId)
            }
            completionHandler()
        }
    }
}

enum StatusItemImage {
    static func make(alert: Bool) -> NSImage {
        let size = NSSize(width: 22, height: 22)
        let image = NSImage(size: size, flipped: false) { rect in
            let inset = rect.insetBy(dx: 2.5, dy: 1)
            if alert {
                NSColor.black.withAlphaComponent(0.3).setFill()

                leafFaces(in: inset).fill()
            }
            NSColor.black.setStroke()
            let stroke = leafStroke(in: inset)
            stroke.lineWidth = 1.4
            stroke.lineJoinStyle = .round
            stroke.lineCapStyle = .round
            stroke.stroke()
            return true
        }
        image.isTemplate = true
        return image
    }

    /// Logo leaf, viewBox 70 28 116 164, no land plot.
    private static func map(_ x: CGFloat, _ y: CGFloat, in rect: NSRect) -> NSPoint {
        NSPoint(
            x: rect.minX + (x - 70) / 116 * rect.width,
            y: rect.maxY - (y - 28) / 164 * rect.height
        )
    }

    private static func leafFaces(in rect: NSRect) -> NSBezierPath {
        let path = NSBezierPath()
        path.move(to: map(128, 38, in: rect))
        path.line(to: map(176, 72, in: rect))
        path.line(to: map(128, 112, in: rect))
        path.line(to: map(80, 72, in: rect))
        path.close()
        path.move(to: map(80, 72, in: rect))
        path.line(to: map(80, 140, in: rect))
        path.line(to: map(128, 180, in: rect))
        path.line(to: map(128, 112, in: rect))
        path.close()
        path.move(to: map(176, 72, in: rect))
        path.line(to: map(176, 140, in: rect))
        path.line(to: map(128, 180, in: rect))
        path.line(to: map(128, 112, in: rect))
        path.close()
        return path
    }


    private static func leafStroke(in rect: NSRect) -> NSBezierPath {
        let path = NSBezierPath()
        path.move(to: map(128, 38, in: rect))
        path.line(to: map(176, 72, in: rect))
        path.line(to: map(128, 112, in: rect))
        path.line(to: map(80, 72, in: rect))
        path.close()
        path.move(to: map(80, 72, in: rect))
        path.line(to: map(80, 140, in: rect))
        path.line(to: map(128, 180, in: rect))
        path.line(to: map(176, 140, in: rect))
        path.line(to: map(176, 72, in: rect))
        path.move(to: map(128, 112, in: rect))
        path.line(to: map(128, 180, in: rect))
        return path
    }
}
