// collab-notify — posts one real macOS notification and exits.
//
// This exists as a Swift .app rather than part of the Go binary because macOS
// will not deliver a UserNotifications alert from a bare command-line tool: the
// notification is attributed to a bundle, so there has to be a bundle. That is
// also what makes the popup say "collab" and carry collab's icon, instead of
// being attributed to Script Editor the way an osascript one-liner would be.
import Cocoa
import UserNotifications

let argv = Array(CommandLine.arguments.dropFirst())
guard !argv.isEmpty else {
    FileHandle.standardError.write(Data("usage: collab-notify <title> [body] [subtitle]\n".utf8))
    exit(2)
}
let title    = argv[0]
let body     = argv.count > 1 ? argv[1] : ""
let subtitle = argv.count > 2 ? argv[2] : ""

func die(_ code: Int32, _ msg: String) -> Never {
    FileHandle.standardError.write(Data((msg + "\n").utf8))
    exit(code)
}

final class Notifier: NSObject, NSApplicationDelegate {
    func applicationDidFinishLaunching(_ note: Notification) {
        let center = UNUserNotificationCenter.current()
        center.requestAuthorization(options: [.alert, .sound]) { granted, err in
            if let err { die(3, "collab-notify: \(err.localizedDescription)") }
            guard granted else { die(4, "collab-notify: notifications are turned off for collab") }

            let content = UNMutableNotificationContent()
            content.title = title
            if !subtitle.isEmpty { content.subtitle = subtitle }
            content.body = body
            content.sound = .default

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
