// collab — the Mac app. Lives in the menu bar, because notifications should
// arrive whether or not you have a window open, and a window you have to keep
// open is a window you will close.
import AppKit
import SwiftUI
import UserNotifications

@MainActor
final class AppState: ObservableObject {
    static let shared = AppState()
    let core = Core()
    /// Registered by SwiftUI once it exists; AppKit alone cannot open a
    /// Window scene by id.
    var openMainWindow: (() -> Void)?
    /// Set when something asks for a particular channel — a notification being
    /// clicked, or a collab://open?channel= URL. A popup should land where the
    /// message came from, the way clicking a WhatsApp notification opens that
    /// conversation rather than the app in general.
    @Published var requestedChannel: String?

    /// A menu bar app has no View menu to put "Enter Full Screen" in, and a
    /// window does not offer it unless it is told it may — so it is asked for
    /// here, from the menu bar and from a button in the window itself.
    /// A window can only go full screen if it says it may, and only if the app
    /// is an ordinary one — a menu bar app is refused. So while the window is
    /// open collab is an ordinary app, which is also what makes the green
    /// button behave the way anybody would expect it to.
    func makeOrdinary(_ w: NSWindow) {
        w.collectionBehavior.insert(.fullScreenPrimary)
        w.styleMask.insert(.resizable)
        if NSApp.activationPolicy() != .regular {
            NSApp.setActivationPolicy(.regular)
            NSApp.activate(ignoringOtherApps: true)
        }
    }

    func toggleFullScreen() {
        // macOS will not give native full screen to an accessory app, and this
        // is one so that it lives in the menu bar without a Dock icon. Becoming
        // a regular app for as long as the window is up is the price; it drops
        // back when the window closes.
        guard let w = mainWindow() else {
            openMainWindow?()
            return
        }
        makeOrdinary(w)
        w.makeKeyAndOrderFront(nil)
        // The policy change has to reach the window server before the window
        // will accept going full screen; asking too soon promotes the app and
        // does nothing else, which looks exactly like the feature not working.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            (self.mainWindow() ?? w).toggleFullScreen(nil)
        }
    }

    func mainWindow() -> NSWindow? {
        NSApp.windows.first { $0.canBecomeMain && $0.title == "collab" }
    }

    func showWindow() {
        NSApp.activate(ignoringOtherApps: true)
        if let open = openMainWindow {
            open()
        } else if let w = NSApp.windows.first(where: { $0.canBecomeKey }) {
            w.makeKeyAndOrderFront(nil)
        }
    }
}

final class Delegate: NSObject, NSApplicationDelegate, UNUserNotificationCenterDelegate {
    func applicationDidFinishLaunching(_ note: Notification) {
        NotificationCenter.default.addObserver(
            self, selector: #selector(windowWillClose(_:)),
            name: NSWindow.willCloseNotification, object: nil)

        let center = UNUserNotificationCenter.current()
        center.delegate = self
        center.requestAuthorization(options: [.alert, .sound]) { _, _ in }

        Task { @MainActor in
            let state = AppState.shared
            state.core.onArrival = { [weak state] batch, wasBacklog in
                guard let state else { return }
                // Everything replayed on first connect is history. Announcing
                // it would mean a popup for every message sent while the
                // machine was asleep, which is not a notification but a
                // punishment.
                guard !wasBacklog else { return }
                Notifier.post(batch: batch, me: state.core.me)
            }
            state.core.start()
        }
    }

    /// The CLI cannot post a notification itself — macOS attributes one to the
    /// bundle's main executable, and a second binary inside the same bundle is
    /// refused. So `collab test-notify` asks here instead.
    func application(_ app: NSApplication, open urls: [URL]) {
        for url in urls {
            switch url.host {
            case "test":
                let c = UNMutableNotificationContent()
                c.title = "collab"
                c.subtitle = "test"
                c.body = "If you can see this, notifications work."
                c.sound = .default
                UNUserNotificationCenter.current().add(
                    UNNotificationRequest(identifier: UUID().uuidString, content: c, trigger: nil))
            case "fullscreen":
                Task { @MainActor in AppState.shared.toggleFullScreen() }
            case "open":
                let wanted = URLComponents(url: url, resolvingAgainstBaseURL: false)?
                    .queryItems?.first(where: { $0.name == "channel" })?.value
                Task { @MainActor in
                    if let wanted, !wanted.isEmpty { AppState.shared.requestedChannel = wanted }
                    AppState.shared.showWindow()
                }
            default:
                break
            }
        }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ app: NSApplication) -> Bool {
        false // closing the window is not quitting; the popups carry on
    }

    /// Going full screen turns this into a regular app, because macOS will not
    /// give full screen to a menu bar one. Closing the window puts it back —
    /// otherwise a single use of full screen would leave a Dock icon behind for
    /// the rest of the session, which is not what a menu bar app is for.
    @objc func windowWillClose(_ note: Notification) {
        guard let w = note.object as? NSWindow, w.title == "collab" else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
            if NSApp.windows.first(where: { $0.title == "collab" && $0.isVisible }) == nil {
                NSApp.setActivationPolicy(.accessory)
            }
        }
    }

    /// A click should land where the message came from — the way clicking a
    /// WhatsApp notification opens that conversation, not the app in general.
    func userNotificationCenter(_ center: UNUserNotificationCenter,
                                didReceive response: UNNotificationResponse,
                                withCompletionHandler done: @escaping () -> Void) {
        let wanted = response.notification.request.content.userInfo["channel"] as? String
        Task { @MainActor in
            if let wanted, !wanted.isEmpty { AppState.shared.requestedChannel = wanted }
            AppState.shared.showWindow()
            done()
        }
    }

    /// Show it even while the app is frontmost — the window may be on another
    /// Space, or behind Roblox Studio.
    func userNotificationCenter(_ center: UNUserNotificationCenter,
                                willPresent notification: UNNotification,
                                withCompletionHandler done: @escaping (UNNotificationPresentationOptions) -> Void) {
        done([.banner, .sound])
    }
}

enum Notifier {
    static func post(batch: [Msg], me: String) {
        // Your own words do not need announcing back to you — but your own
        // AI's do. A name belongs to a machine, so without the second test
        // this would also silence the assistant sitting next to you.
        // Your own words are not announced back at you, and neither is a
        // message plainly addressed to somebody else — it is still in the
        // window, it just does not interrupt.
        let worth = batch.filter {
            ($0.isAI || $0.from.caseInsensitiveCompare(me) != .orderedSame) && $0.isFor(me)
        }
        guard !worth.isEmpty else { return }

        let content = UNMutableNotificationContent()
        content.sound = .default

        if worth.count == 1 {
            let m = worth[0]
            // The machine rides along in the title: a chat that named itself
            // "shop" says nothing about whose Claude it is.
            content.title = m.machine.map { "\(m.who) (\($0))" } ?? m.who
            content.subtitle = m.isChange ? "\(m.action ?? "") · \(m.target ?? "")" : "#\(m.channel)"
            content.body = m.text
        } else {
            var senders: [String] = []
            for m in worth {
                let label = m.machine.map { "\(m.who) (\($0))" } ?? m.who
                if !senders.contains(label) { senders.append(label) }
            }
            let changes = worth.filter(\.isChange).count
            let last = worth[worth.count - 1]
            content.title = senders.count > 2 ? "collab" : senders.joined(separator: " & ")
            content.subtitle = changes > 0
                ? "\(worth.count) new on #\(last.channel) · \(changes) change\(changes == 1 ? "" : "s")"
                : "\(worth.count) new on #\(last.channel)"
            content.body = "\(last.who): \(last.line)"
        }
        content.userInfo = ["channel": worth[worth.count - 1].channel]

        UNUserNotificationCenter.current().add(
            UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil))
    }
}

@main
struct CollabApp: App {
    @NSApplicationDelegateAdaptor(Delegate.self) var delegate
    @ObservedObject private var state = AppState.shared

    var body: some Scene {
        Window("collab", id: "main") {
            ContentView(core: state.core)
                .environmentObject(state.core)
                .frame(minWidth: 720, minHeight: 420)
                .background(WindowCapability())
        }
        .defaultSize(width: 1000, height: 640)

        MenuBarExtra {
            MenuContent(core: state.core)
        } label: {
            MenuLabel(core: state.core)
        }
    }
}

/// The label exists from launch, which makes it the one reliable place to
/// borrow SwiftUI's window-opening action for AppKit to use later.
struct MenuLabel: View {
    @ObservedObject var core: Core
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Image(systemName: core.connected
              ? "bubble.left.and.bubble.right"
              : "bubble.left.and.exclamationmark.bubble.right")
            .onAppear { AppState.shared.openMainWindow = { openWindow(id: "main") } }
    }
}

struct MenuContent: View {
    @ObservedObject var core: Core

    var body: some View {
        Button("Open collab") { AppState.shared.showWindow() }
            .keyboardShortcut("o")
        Button("Full Screen") { AppState.shared.toggleFullScreen() }
            .keyboardShortcut("f", modifiers: [.control, .command])
        Button("Check for Updates…") { Updater.checkForUpdates() }
        Divider()
        if let fatal = core.fatal {
            Text(fatal)
        } else {
            Text(core.connected
                 ? "Connected · \(core.serverAddr)"
                 : "DISCONNECTED from \(core.serverAddr) — retrying")
            Text("\(core.me) on #\(core.homeChannel)")
        }
        Divider()
        Button("Quit collab") { NSApp.terminate(nil) }
            .keyboardShortcut("q")
    }
}


/// Reaches the NSWindow behind the SwiftUI scene to say it may go full screen.
/// Without fullScreenPrimary the green button only zooms, and in an app with no
/// menu bar there is nothing else offering it.
struct WindowCapability: NSViewRepresentable {
    func makeNSView(context: Context) -> NSView {
        let v = NSView()
        // The window does not exist yet when this is made, and asking once and
        // giving up is why the green button only ever zoomed. Keep looking
        // briefly, then set it.
        attach(to: v, tries: 20)
        return v
    }

    private func attach(to v: NSView, tries: Int) {
        guard tries > 0 else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) {
            guard let w = v.window else {
                attach(to: v, tries: tries - 1)
                return
            }
            AppState.shared.makeOrdinary(w)
        }
    }

    func updateNSView(_ nsView: NSView, context: Context) {}
}
