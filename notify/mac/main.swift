// collab-notify — posts one real macOS notification and exits.
//
// This exists as a Swift .app rather than part of the Go binary because macOS
// will not deliver a UserNotifications alert from a bare command-line tool: the
// notification is attributed to a bundle, so there has to be a bundle. That is
// also what makes the popup say "collab" and carry collab's icon, instead of
// being attributed to Script Editor the way an osascript one-liner would be.
import Cocoa
import UserNotifications

// With arguments, this posts a notification and exits. With none, it was
// relaunched by macOS because somebody clicked one — the notification that was
// clicked arrives at the delegate a moment later, carrying, in its userInfo,
// where the window is and what to run to open it.
let argv = Array(CommandLine.arguments.dropFirst())
let title     = argv.count > 0 ? argv[0] : ""
let body      = argv.count > 1 ? argv[1] : ""
let subtitle  = argv.count > 2 ? argv[2] : ""
let windowURL = argv.count > 3 ? argv[3] : ""
let collabBin = argv.count > 4 ? argv[4] : ""
let channel   = argv.count > 5 ? argv[5] : ""
let myName    = argv.count > 6 ? argv[6] : ""

func die(_ code: Int32, _ msg: String) -> Never {
    FileHandle.standardError.write(Data((msg + "\n").utf8))
    exit(code)
}

// Opening the window: prefer running `collab gui`, which shows the existing
// window if one is already up and starts one if not. Opening the URL directly
// would give a connection-refused page whenever the window was closed.
func openWindow(collab: String, url: String, channel: String, name: String) {
    // A window started from a click inherits none of the shell's COLLAB_
    // settings, so they are handed over explicitly — otherwise the window
    // opens on the default channel and posts to the wrong place.
    if !collab.isEmpty, FileManager.default.isExecutableFile(atPath: collab) {
        var args = ["gui"]
        if !channel.isEmpty { args += ["-channel", channel] }
        if !name.isEmpty    { args += ["-name", name] }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: collab)
        p.arguments = args
        if (try? p.run()) != nil { return }
    }
    var target = url
    if !channel.isEmpty, let esc = channel.addingPercentEncoding(withAllowedCharacters: .urlQueryAllowed) {
        target += "/?channel=" + esc
    }
    if let u = URL(string: target) { NSWorkspace.shared.open(u) }
}

final class Notifier: NSObject, NSApplicationDelegate, UNUserNotificationCenterDelegate {
    func userNotificationCenter(_ center: UNUserNotificationCenter,
                                didReceive response: UNNotificationResponse,
                                withCompletionHandler done: @escaping () -> Void) {
        let info = response.notification.request.content.userInfo
        openWindow(collab:  info["collab"]  as? String ?? collabBin,
                   url:     info["url"]     as? String ?? windowURL,
                   channel: info["channel"] as? String ?? channel,
                   name:    info["name"]    as? String ?? myName)
        done()
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) { exit(0) }
    }

    func applicationDidFinishLaunching(_ note: Notification) {
        let center = UNUserNotificationCenter.current()
        center.delegate = self   // must be in place before a click can arrive

        guard !argv.isEmpty else {
            // Relaunched by a click; the delegate call is on its way. Do not
            // linger for ever if it never comes.
            DispatchQueue.main.asyncAfter(deadline: .now() + 5) { exit(0) }
            return
        }

        center.requestAuthorization(options: [.alert, .sound]) { granted, err in
            if let err { die(3, "collab-notify: \(err.localizedDescription)") }
            guard granted else { die(4, "collab-notify: notifications are turned off for collab") }

            let content = UNMutableNotificationContent()
            content.title = title
            if !subtitle.isEmpty { content.subtitle = subtitle }
            content.body = body
            content.sound = .default
            content.userInfo = ["url": windowURL, "collab": collabBin,
                                "channel": channel, "name": myName]

            center.add(UNNotificationRequest(identifier: UUID().uuidString,
                                             content: content, trigger: nil)) { err in
                if let err { die(5, "collab-notify: \(err.localizedDescription)") }
                // Let the notification daemon take delivery before we go.
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { exit(0) }
            }
        }
    }
}

let app = NSApplication.shared
app.setActivationPolicy(.accessory)   // no Dock icon, no menu bar
let delegate = Notifier()
app.delegate = delegate
app.run()
