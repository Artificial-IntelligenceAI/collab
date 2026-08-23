// Everything the window knows, and the only place it talks to the outside.
//
// The app never speaks the protocol itself: it runs the Rust binary and reads
// what comes back. That keeps one implementation of the wire format and one of
// the encryption, in the place that was built for it — Swift holding a second
// copy of the crypto would be a second thing to get wrong.
import Combine
import Foundation

struct FileRef: Codable, Hashable {
    var name: String
    var size: Int64
    var hash: String

    var readable: String {
        ByteCountFormatter.string(fromByteCount: size, countStyle: .file)
    }
}

struct Msg: Codable, Identifiable, Hashable {
    var seq: Int64
    var channel: String
    var from: String
    var at: String
    var kind: String?
    var via: String?
    /// Which machine it came from. `from` is a display name an AI may choose
    /// for itself, so without this you could not tell whose Claude spoke.
    var host: String?
    var text: String
    var action: String?
    var target: String?
    /// Set when this is a file. The bytes are not here — they are fetched when
    /// somebody actually wants them.
    var file: FileRef?
    /// Who it is aimed at, from the @names in it. Empty means everyone. It
    /// narrows who is told, never who can read it.
    var to: [String]?

    var id: Int64 { seq }
    var isChange: Bool { kind == "change" }
    var isFile: Bool { kind == "file" }

    /// Whether this should interrupt somebody answering to `name`.
    func isFor(_ name: String) -> Bool {
        guard let to, !to.isEmpty else { return true }
        return to.contains(name.lowercased())
    }
    var isAI: Bool { via == "ai" }

    /// "AI" or "Human" — what the badge says.
    var role: String { isAI ? "AI" : "Human" }

    /// Just the name. The badge beside it says AI or Human, so spelling it out
    /// as "tankun's AI" would be saying the same thing twice — that phrasing
    /// exists for the terminal, which has no badge to lean on.
    var who: String {
        guard isAI else { return from }
        // Records written before names existed carry no host: back then `from`
        // was the machine.
        guard let h = host, !h.isEmpty else { return from }
        return (from.isEmpty || from == h) ? h : from
    }

    /// The machine, when the name does not already give it away.
    var machine: String? {
        guard let h = host, !h.isEmpty, !who.contains(h) else { return nil }
        return h
    }

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
        if isFile, let f = file {
            return "[file] \(f.name) (\(f.readable))" + (text.isEmpty ? "" : " — \(text)")
        }
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
    /// Which channel this status is about. The watcher holds one connection per
    /// channel and reports on each separately; without this the reports are
    /// indistinguishable and the last one wins.
    var channel: String?
}

private struct Settings: Decodable {
    var name: String
    var channel: String
    var addr: String
    var channels: [String]
    var notifier: String?
}

@MainActor
final class Core: ObservableObject {
    @Published private(set) var messages: [Msg] = []
    @Published private(set) var connected = false
    @Published private(set) var wasDisconnected = false
    @Published private(set) var statusDetail: String?
    /// Connection state per channel, because there is one connection per channel
    /// and they fail independently. Collapsing them into a single flag meant a
    /// channel nobody was looking at — a stale key, a room left over from
    /// testing — painted the whole window disconnected while every message the
    /// person sent went through normally.
    @Published private(set) var channelUp: [String: Bool] = [:]
    /// The channel the window is showing. Set by the view; the status light
    /// follows it, so switching rooms shows that room's connection.
    @Published var watching: String = "" {
        didSet {
            guard watching != oldValue else { return }
            if let up = channelUp[watching] { connected = up; statusDetail = nil }
        }
    }
    /// Channels that are down other than the one being viewed — worth saying
    /// out loud, because the alternative is a green light and a room that is
    /// quietly receiving nothing.
    var othersDown: [String] {
        channelUp.filter { $0.key != watching && !$0.value }.keys.sorted()
    }
    @Published private(set) var me = ""
    /// What this machine calls itself on each channel. The composer footer has
    /// to read from this rather than from `me`, or it tells you that you are
    /// posting as the machine while the message goes out under another name.
    @Published private(set) var displayNames: [String: String] = [:]
    @Published private(set) var homeChannel = "general"
    @Published private(set) var serverAddr = ""
    @Published private(set) var knownChannels: [String] = []
    /// Until this is true, `homeChannel` is only a placeholder. A view that
    /// snapshots it before the settings arrive keeps the placeholder for ever.
    @Published private(set) var settingsLoaded = false
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

    /// Names that can be mentioned on a channel: everyone who has spoken there,
    /// yourself excluded — you do not need telling about your own messages.
    /// Each carries whether it was a person or an AI, so the list says what it
    /// is offering rather than just a word.
    func mentionable(on channel: String?) -> [(name: String, isAI: Bool)] {
        var seen: [String: Bool] = [:]
        for m in messages where channel == nil || m.channel == channel {
            if m.from.isEmpty || m.from.caseInsensitiveCompare(me) == .orderedSame { continue }
            seen[m.from] = m.isAI
        }
        return seen.map { (name: $0.key, isAI: $0.value) }
            .sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
    }

    /// Whether this machine is the one running the server. A channel only
    /// exists as far as the server is concerned, so a channel made anywhere
    /// else reaches nobody until the server holds its key too.
    var isServer: Bool {
        let host = serverAddr.split(separator: ":").first.map(String.init) ?? ""
        return host.isEmpty || host == "localhost" || host == "127.0.0.1"
    }

    /// Every channel worth offering: the ones this machine holds keys for, plus
    /// any seen in messages. A channel with nothing in it yet still exists.
    var channels: [String] {
        var set = Set(messages.map(\.channel))
        set.formUnion(knownChannels)
        set.insert(homeChannel)
        return set.filter { !$0.isEmpty }.sorted()
    }

    // MARK: finding the core

    static func binary() -> String? {
        var candidates: [String] = []
        // Inside the bundle first: an app dragged to Applications carries its
        // own core, so there is nothing else to install for it to work.
        if let inside = Bundle.main.resourceURL?.appendingPathComponent("collab").path {
            candidates.append(inside)
        }
        // Then beside the .app, which is where a build tree keeps it.
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

    /// The name to show for a channel: the one chosen there, or the machine's.
    func displayName(on channel: String) -> String {
        let d = displayNames[channel] ?? ""
        return d.isEmpty ? me : d
    }

    func loadDisplayNames() {
        guard let json = try? run(["channels", "-json"]),
              let data = json.data(using: .utf8),
              let list = try? JSONSerialization.jsonObject(with: data) as? [[String: Any]]
        else { return }
        var out: [String: String] = [:]
        for c in list {
            if let n = c["name"] as? String { out[n] = (c["display"] as? String) ?? "" }
        }
        displayNames = out
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
        loadDisplayNames()
        serverAddr = s.addr
        knownChannels = s.channels
        settingsLoaded = true
        fatal = s.channels.isEmpty
            ? "No channels yet. Make one with the # button above, then send its key to the other person."
            : nil
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
        // The app is not a chat and must not inherit one's identity. Launched
        // from a terminal that had a session id, it would take that chat's name
        // and start treating that chat's messages as its own echo.
        env["CLAUDE_CODE_SESSION_ID"] = nil
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

    /// A watcher holds one connection per channel, opened when it started, so a
    /// channel made after that is invisible to it until it is started again.
    func restartWatcher() {
        stop()
        seen.removeAll()
        messages.removeAll()
        buffer.removeAll()
        priming = true
        loadSettings()
        startWatching()
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
            if let ch = ev.channel, !ch.isEmpty {
                channelUp[ch] = up
                // Only the channel being looked at drives the light. A report
                // about a different room is recorded, not displayed.
                if ch == watching { connected = up; statusDetail = ev.error }
            } else {
                connected = up
                statusDetail = ev.error
            }
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

    /// Sending is a person's act here; the caption is whatever is in the box.
    func sendFile(_ path: String, caption: String, to channel: String) throws {
        _ = try run(["send", path, "-m", caption, "-c", channel], channel: channel)
    }

    /// Downloads by hash and returns where it landed. Nothing is written unless
    /// what arrived matches the hash.
    @discardableResult
    func fetchFile(_ m: Msg) throws -> String {
        guard let f = m.file else { throw CollabError.command("that message has no file") }
        let out = try run(["get", f.hash, "-c", m.channel], channel: m.channel)
        return out.trimmingCharacters(in: .whitespacesAndNewlines)
    }

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
