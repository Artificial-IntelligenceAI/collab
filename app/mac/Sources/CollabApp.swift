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
        // fullScreenNone beats fullScreenPrimary: a window carrying both is
        // refused full screen outright, which is why the green button fell back
        // to zoom and the in-app button retried twelve times and gave up.
        // SwiftUI sets it on this window under conditions I have not pinned
        // down; inserting primary without removing it fixed nothing, four
        // times. Take it off first.
        w.collectionBehavior.remove(.fullScreenNone)
        w.collectionBehavior.insert(.fullScreenPrimary)
        w.styleMask.insert(.resizable)
        if NSApp.activationPolicy() != .regular {
            NSApp.setActivationPolicy(.regular)
            NSApp.activate(ignoringOtherApps: true)
        }
    }

    /// True from the moment AppKit accepts a full screen change until its
    /// animation ends. A toggle asked for inside that window is dropped, so a
    /// click landing there would look like a dead button.
    private(set) var inTransition = false
    /// Whether the change just asked for was accepted. This is the difference
    /// between "refused, ask again" and "under way, leave it alone" — which a
    /// fixed delay can only ever guess at.
    private var accepted = false
    private var transitionStarted = Date.distantPast
    fileprivate var lastResizeNote = Date.distantPast

    /// AppKit says when a full screen change begins and ends. Listening is what
    /// replaces guessing how long becoming a regular app takes.
    /// Keeps fullScreenNone off the window, and says when it had to.
    ///
    /// Re-asserting only when the window becomes key is not enough: the green
    /// button never enters this program, so if the flag appears while the
    /// window simply sits there, the button silently becomes a zoom button and
    /// nothing notices. Two seconds is cheap — it is a bit test — and the log
    /// line is the point: it records the moment the flag arrives, which is the
    /// one thing still unknown about this bug after five fixes.
    func guardFullScreenFlag() {
        Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { _ in
            MainActor.assumeIsolated {
                let s = AppState.shared
                guard let w = s.mainWindow() else { return }
                guard w.collectionBehavior.contains(.fullScreenNone) else { return }
                s.note("CLEARED fullScreenNone, it had come back: \(s.state(w))")
                w.collectionBehavior.remove(.fullScreenNone)
                w.collectionBehavior.insert(.fullScreenPrimary)
            }
        }
    }

    func watchFullScreen() {
        let nc = NotificationCenter.default
        for n in [NSWindow.willEnterFullScreenNotification,
                  NSWindow.willExitFullScreenNotification] {
            nc.addObserver(forName: n, object: nil, queue: .main) { _ in
                MainActor.assumeIsolated {
                    let s = AppState.shared
                    s.accepted = true
                    s.inTransition = true
                    s.transitionStarted = Date()
                }
            }
        }
        // A window can be closed and opened again without SwiftUI rebuilding
        // the view behind it, so the promotion done when that view was first
        // made never runs a second time. The app stays an accessory, macOS
        // quietly downgrades the green button from a full screen button to a
        // zoom button, and clicking it only resizes the window. Re-asserting
        // whenever the window comes forward is what makes the second opening
        // behave like the first.
        // The green button never reaches toggleFullScreen — AppKit handles it —
        // so a failure there leaves no trace in the log at all, which is what
        // today's silence showed. A resize is the visible symptom of the button
        // choosing zoom over full screen, so record the state when one happens:
        // if primary is false at that moment, the flag was lost and macOS
        // downgraded the button. Throttled, because live resizing fires
        // continuously.
        nc.addObserver(forName: NSWindow.didResizeNotification, object: nil, queue: .main) { note in
            MainActor.assumeIsolated {
                let s = AppState.shared
                guard let w = note.object as? NSWindow, w.title == "collab" else { return }
                guard Date().timeIntervalSince(s.lastResizeNote) > 1.5 else { return }
                s.lastResizeNote = Date()
                s.note("resized \(s.state(w))")
            }
        }

        for n in [NSWindow.didBecomeKeyNotification, NSWindow.didBecomeMainNotification] {
            nc.addObserver(forName: n, object: nil, queue: .main) { note in
                MainActor.assumeIsolated {
                    guard let w = note.object as? NSWindow, w.title == "collab" else { return }
                    let s = AppState.shared
                    let before = s.state(w)
                    s.makeOrdinary(w)
                    // Only worth a line when it actually changed something.
                    if before != s.state(w) { s.note("promoted on \(note.name.rawValue) was: \(before)") }
                }
            }
        }
        for n in [NSWindow.didEnterFullScreenNotification,
                  NSWindow.didExitFullScreenNotification] {
            nc.addObserver(forName: n, object: nil, queue: .main) { note in
                MainActor.assumeIsolated {
                    let s = AppState.shared
                    s.inTransition = false
                    s.note("ended \(note.name.rawValue) \(s.state(note.object as? NSWindow))")
                }
            }
        }
    }

    /// Full screen has broken and been fixed four times, each time for a
    /// different reason, and each fix verified before it broke again. Guessing
    /// after the fact has not worked, so every attempt writes down what was
    /// actually true at the moment it was made. When it next fails, the answer
    /// is already on disk instead of being reconstructed from memory.
    func note(_ line: String) {
        let path = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent(".collab-fullscreen.log")
        let stamp = ISO8601DateFormatter().string(from: Date())
        guard let data = "\(stamp) \(line)\n".data(using: .utf8) else { return }
        if let h = try? FileHandle(forWritingTo: path) {
            defer { try? h.close() }
            // Keep it small: this is a tail, not an archive.
            if (try? h.seekToEnd()) ?? 0 > 64_000 {
                try? FileHandle(forWritingTo: path).truncate(atOffset: 0)
            }
            try? h.seekToEnd()
            try? h.write(contentsOf: data)
        } else {
            try? data.write(to: path)
        }
    }

    /// Everything that decides whether a window may go full screen.
    func state(_ w: NSWindow?) -> String {
        guard let w else { return "no-window policy=\(NSApp.activationPolicy().rawValue)" }
        let cb = w.collectionBehavior
        return "policy=\(NSApp.activationPolicy().rawValue) primary=\(cb.contains(.fullScreenPrimary)) "
            + "none=\(cb.contains(.fullScreenNone)) cb=\(cb.rawValue) "
            + "isFS=\(w.styleMask.contains(.fullScreen)) resizable=\(w.styleMask.contains(.resizable)) "
            + "style=\(w.styleMask.rawValue) inTransition=\(inTransition) "
            + "frame=\(Int(w.frame.width))x\(Int(w.frame.height)) key=\(w.isKeyWindow) main=\(w.isMainWindow)"
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
        // A second click during the animation is not a second request. AppKit
        // drops it, and acting on it would fight the transition already running.
        // A transition that begins and never reports finishing would leave this
        // true for ever and the button dead with no way back. macOS can cancel
        // one. Anything older than a few seconds is not a transition any more.
        if inTransition && Date().timeIntervalSince(transitionStarted) > 4 {
            inTransition = false
        }
        guard !inTransition else { note("BLOCKED inTransition \(state(w))"); return }
        note("ask  \(state(w))")
        makeOrdinary(w)
        w.makeKeyAndOrderFront(nil)
        note("made \(state(w))")
        ask(w, want: !w.styleMask.contains(.fullScreen), tries: 0)
    }

    /// Asks, then checks whether the ask was taken, and asks again if it was
    /// not. Becoming a regular app has to reach the window server before a
    /// window may go full screen, and how long that takes is not a fixed
    /// number — the half second this replaces was right on an idle machine and
    /// wrong on a busy one, which is the exact shape of "sometimes it works".
    private func ask(_ w: NSWindow, want: Bool, tries: Int) {
        guard tries < 12 else { note("GAVE UP after 12 \(state(w))"); return }
        if tries > 0 { note("retry \(tries) \(state(w))") }
        accepted = false
        w.toggleFullScreen(nil)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
            MainActor.assumeIsolated {
                // Accepted, or already where it was asked to be: either way the
                // job is done and asking again would undo it.
                guard !self.accepted, w.styleMask.contains(.fullScreen) != want else { return }
                self.makeOrdinary(w)
                self.ask(w, want: want, tries: tries + 1)
            }
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
            state.watchFullScreen()
            state.guardFullScreenFlag()
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

    /// Take the watcher with us. Left behind, it holds a pipe whose reader has
    /// gone and lingers until the next message, and every quit leaves another
    /// one waiting. They used to die loudly too — an abort and a crash report
    /// apiece — which is how twenty-five of those came to exist.
    func applicationWillTerminate(_ note: Notification) {
        MainActor.assumeIsolated { AppState.shared.core.stop() }
    }

    /// Going full screen turns this into a regular app, because macOS will not
    /// give full screen to a menu bar one. Closing the window puts it back —
    /// otherwise a single use of full screen would leave a Dock icon behind for
    /// the rest of the session, which is not what a menu bar app is for.
    @objc func windowWillClose(_ note: Notification) {
        guard let w = note.object as? NSWindow, w.title == "collab" else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
            // Not while a full screen change is running: that tears the window
            // down and puts it back, and demoting in the middle drops the app
            // out of full screen for no reason anybody can see.
            let busy = MainActor.assumeIsolated { AppState.shared.inTransition }
            if busy { return }
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
            MainActor.assumeIsolated {
                if let w = v.window {
                    AppState.shared.makeOrdinary(w)
                }
            }
            // Keep saying it after the window turns up rather than stopping at
            // the first success: SwiftUI configures this window itself, and
            // whichever of us goes last wins. Asking once is why the green
            // button sometimes only zoomed.
            attach(to: v, tries: tries - 1)
        }
    }

    func updateNSView(_ nsView: NSView, context: Context) {}
}
