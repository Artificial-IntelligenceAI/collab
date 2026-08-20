// The update button, and the dialog in front of it.
//
// The dialog is not what makes this safe. It asks you to approve something you
// cannot inspect, and you would click through it for somebody else's build as
// readily as for your own. What makes it safe is that `collab update` refuses
// anything not signed by the key compiled into it — the dialog is only there so
// that installing is a thing you chose, at a moment you chose it.
import AppKit
import Foundation

@MainActor
enum Updater {
    private struct Check: Decodable {
        var ok: Bool
        var current: String?
        var available: String?
        var newer: Bool?
        var notes: String?
        var error: String?
        var installed: [String]?
    }

    private static func collab(_ args: [String]) -> Check? {
        guard let bin = Core.binary() else { return nil }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: bin)
        p.arguments = args
        let out = Pipe()
        p.standardOutput = out
        p.standardError = Pipe()
        try? p.run()
        let data = out.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        // The tail is the JSON; anything before it is human-facing chatter.
        guard let line = String(decoding: data, as: UTF8.self)
            .split(separator: "\n").last(where: { $0.hasPrefix("{") })
        else { return nil }
        return try? JSONDecoder().decode(Check.self, from: Data(line.utf8))
    }

    private static func alert(_ title: String, _ body: String,
                              confirm: String? = nil) -> Bool {
        NSApp.activate(ignoringOtherApps: true)
        let a = NSAlert()
        a.messageText = title
        a.informativeText = body
        if let confirm {
            a.addButton(withTitle: confirm)
            a.addButton(withTitle: "Cancel")
        } else {
            a.addButton(withTitle: "OK")
        }
        return a.runModal() == .alertFirstButtonReturn
    }

    static func checkForUpdates() {
        guard let c = collab(["update", "-json"]) else {
            _ = alert("Could not check", "The collab command did not answer.")
            return
        }
        guard c.ok else {
            _ = alert("Could not check", c.error ?? "Unknown problem.")
            return
        }
        let current = c.current ?? "?"
        let available = c.available ?? "?"
        guard c.newer == true else {
            _ = alert("Up to date", "You are running \(current), which is the latest signed release.")
            return
        }

        let notes = (c.notes?.isEmpty == false) ? "\n\n\(c.notes!)" : ""
        let go = alert(
            "Update to \(available)?",
            "You are running \(current). The release is signed and the signature has already "
            + "been checked — anything not signed with collab's release key is refused before "
            + "you are asked.\n\nThis replaces the collab command and this app. Nothing is sent "
            + "anywhere."
            + notes,
            confirm: "Update")
        guard go else { return }

        guard let done = collab(["update", "-yes", "-json"]), done.ok else {
            _ = alert("Update failed",
                      collab(["update", "-json"])?.error ?? "Nothing was installed.")
            return
        }
        let restart = alert(
            "Updated to \(available)",
            "Replaced:\n" + (done.installed ?? []).map { "  \($0)" }.joined(separator: "\n")
            + "\n\nThe server and this app need restarting to run the new version.",
            confirm: "Restart now")
        if restart { restartEverything() }
    }

    /// launchd owns both halves, so asking it to kick them is the honest way to
    /// restart: whatever it starts at login is what starts now.
    private static func restartEverything() {
        let uid = getuid()
        for label in ["com.tankun.collab", "com.tankun.collab.app"] {
            let p = Process()
            p.executableURL = URL(fileURLWithPath: "/bin/launchctl")
            p.arguments = ["kickstart", "-k", "gui/\(uid)/\(label)"]
            try? p.run()
            p.waitUntilExit()
        }
        NSApp.terminate(nil)
    }
}
