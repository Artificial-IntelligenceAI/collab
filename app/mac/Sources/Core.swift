// Everything the window knows, and the only place it talks to the outside.
//
// The app never speaks the protocol itself: it runs the Rust binary and reads
// what comes back. That keeps one implementation of the wire format and one of
// the encryption, in the place that was built for it — Swift holding a second
// copy of the crypto would be a second thing to get wrong.
import Combine
import Foundation

struct Msg: Codable, Identifiable, Hashable {
    var seq: Int64
    var channel: String
    var from: String
    var at: String
    var kind: String?
    var via: String?
    var text: String
    var action: String?
    var target: String?

    var id: Int64 { seq }
    var isChange: Bool { kind == "change" }
    var isAI: Bool { via == "ai" }
    var who: String { isAI ? "\(from)'s AI" : from }

    var date: Date {
        Msg.parser.date(from: at) ?? Msg.plainParser.date(from: at) ?? .distantPast
    }
    private static let parser: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()
    private static let plainParser: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()

    /// One line, the way the terminal shows it.
    var line: String {
        guard isChange else { return text }
        let t = target ?? ""
        return t.isEmpty ? "[\(action ?? "")] \(text)" : "[\(action ?? "")] \(t) — \(text)"
    }
}

private struct Event: Decodable {
    var type: String
    var msg: Msg?
    var connected: Bool?
    var from: Int64?
    var addr: String?
    var error: String?
}

private struct Settings: Decodable {
    var name: String
    var channel: String
    var addr: String
    var hasKey: Bool
    var notifier: String?
}

@MainActor
final class Core: ObservableObject {
    @Published private(set) var messages: [Msg] = []
    @Published private(set) var connected = false
    @Published private(set) var wasDisconnected = false
    @Published private(set) var statusDetail: String?
    @Published private(set) var me = ""
    @Published private(set) var homeChannel = "general"
    @Published private(set) var serverAddr = ""
    @Published private(set) var hasKey = true
    @Published private(set) var fatal: String?

    /// True until the backlog has finished arriving. Everything the server
    /// replays on connect is history, and history must not raise forty popups.
    private(set) var priming = true

    private var watcher: Process?
    private var buffer = Data()
    private var seen = Set<Int64>()
    var onArrival: (([Msg], Bool) -> Void)?
    private var pending: [Msg] = []
    private var settleTimer: Timer?

    var channels: [String] {
        var set = Set(messages.map(\.channel))
        set.insert(homeChannel)
        return set.filter { !$0.isEmpty }.sorted()
    }

    // MARK: finding the core

    static func binary() -> String? {
        var candidates: [String] = []
        // Beside the .app, first — a bundle and its core should travel together.
        let appDir = Bundle.main.bundleURL.deletingLastPathComponent().path
        candidates.append(appDir + "/collab")
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        candidates += [
            home + "/.local/bin/collab",
            "/usr/local/bin/collab",
            "/opt/homebrew/bin/collab",
        ]
        return candidates.first { FileManager.default.isExecutableFile(atPath: $0) }
    }

    private func run(_ args: [String], channel: String? = nil) throws -> String {
        guard let bin = Core.binary() else { throw CollabError.noBinary }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: bin)
        p.arguments = args
        var env = ProcessInfo.processInfo.environment
        env["COLLAB_NOTIFY"] = "0" // this app raises its own; two would double up
        if let channel { env["COLLAB_CHANNEL"] = channel }
        p.environment = env
        let out = Pipe(), err = Pipe()
        p.standardOutput = out
        p.standardError = err
        try p.run()
        let data = out.fileHandleForReading.readDataToEndOfFile()
        let errData = err.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        if p.terminationStatus != 0 {
            throw CollabError.command(String(decoding: errData, as: UTF8.self)
                .trimmingCharacters(in: .whitespacesAndNewlines))
        }
        return String(decoding: data, as: UTF8.self)
    }

    // MARK: lifecycle

    func start() {
        guard Core.binary() != nil else {
            fatal = "Cannot find the collab command. Install it with ./install.sh, or put it next to Collab.app."
            return
        }
        loadSettings()
        startWatching()
    }

    private func loadSettings() {
        guard let json = try? run(["who", "-json"]),
              let data = json.data(using: .utf8),
              let s = try? JSONDecoder().decode(Settings.self, from: data)
        else { return }
        me = s.name
        homeChannel = s.channel
        serverAddr = s.addr
        hasKey = s.hasKey
        if !s.hasKey {
            fatal = "No shared key is set. Run `collab key -new` in a terminal, then copy the line it prints to the other machine."
        }
    }

    private func startWatching() {
        guard let bin = Core.binary() else { return }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: bin)
        // -all: every channel, the views filter. -since 0: the whole history.
        // -no-save: never touch ~/.collab-seen, which the Monitor's watcher owns.
        p.arguments = ["watch", "-json", "-all", "-since", "0", "-no-save"]
        var env = ProcessInfo.processInfo.environment
        env["COLLAB_NOTIFY"] = "0"
        p.environment = env

        let pipe = Pipe()
        p.standardOutput = pipe
        p.standardError = FileHandle.nullDevice
        pipe.fileHandleForReading.readabilityHandler = { [weak self] handle in
            let chunk = handle.availableData
            guard !chunk.isEmpty else { return }
            Task { @MainActor in self?.consume(chunk) }
        }
        p.terminationHandler = { [weak self] _ in
            Task { @MainActor in
                self?.connected = false
                self?.statusDetail = "the collab watcher stopped"
            }
        }
        watcher = p
        try? p.run()
    }

    func stop() {
        watcher?.terminate()
        watcher = nil
    }

    // MARK: the stream

    private func consume(_ chunk: Data) {
        buffer.append(chunk)
        while let nl = buffer.firstIndex(of: 0x0a) {
            let line = buffer[buffer.startIndex..<nl]
            buffer = buffer[buffer.index(after: nl)...]
            guard !line.isEmpty,
                  let ev = try? JSONDecoder().decode(Event.self, from: Data(line))
            else { continue }
            handle(ev)
        }
    }

    private func handle(_ ev: Event) {
        switch ev.type {
        case "status":
            let up = ev.connected ?? false
            if !up { wasDisconnected = true }
            connected = up
            statusDetail = ev.error
            if let addr = ev.addr { serverAddr = addr }
        case "msg":
            guard let m = ev.msg, !seen.contains(m.seq) else { return }
            seen.insert(m.seq)
            let at = messages.firstIndex { $0.seq > m.seq }
            messages.insert(m, at: at ?? messages.endIndex)
            pending.append(m)
            scheduleSettle()
        default:
            break
        }
    }

    /// Arrivals are collected until the channel goes quiet. A machine waking
    /// from sleep gets everything it missed at once, and forty popups in a row
    /// is not a notification, it is a punishment.
    private func scheduleSettle() {
        settleTimer?.invalidate()
        settleTimer = Timer.scheduledTimer(withTimeInterval: 0.7, repeats: false) { [weak self] _ in
            Task { @MainActor in self?.settle() }
        }
    }

    private func settle() {
        let batch = pending
        pending = []
        let wasPriming = priming
        priming = false
        guard !batch.isEmpty else { return }
        onArrival?(batch, wasPriming)
    }

    // MARK: sending

    func post(_ text: String, to channel: String) throws {
        let t = text.trimmingCharacters(in: .whitespacesAndNewlines).replacingOccurrences(of: "\n", with: " ")
        guard !t.isEmpty else { return }
        _ = try run(["post", t], channel: channel)
    }
}

enum CollabError: LocalizedError {
    case noBinary
    case command(String)

    var errorDescription: String? {
        switch self {
        case .noBinary: return "Cannot find the collab command."
        case .command(let s): return s.isEmpty ? "The collab command failed." : s
        }
    }
}
