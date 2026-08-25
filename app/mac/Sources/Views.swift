import SwiftUI

enum Pane: String, CaseIterable { case chat = "Chat", changes = "Changes" }

struct ContentView: View {
    @ObservedObject var core: Core
    @ObservedObject private var state = AppState.shared
    @StateObject private var store = ChannelStore()
    @State private var showChannels = false
    @State private var pane: Pane = .chat
    @State private var query = ""
    @State private var channel: String?
    @State private var draft = ""
    @State private var sendError: String?
    @State private var note: String?
    @State private var pickedInitial = false
    @State private var mentionPick = 0

    /// You post where you are looking. Reading one channel and typing into
    /// another would be a nasty little trap.
    private var postChannel: String { channel ?? core.homeChannel }

    /// What the status pill says. "disconnected" on its own was the whole
    /// problem: it was true of some channel, and read as true of everything.
    private var statusText: String {
        if !core.connected { return "disconnected" }
        let others = core.othersDown
        if others.isEmpty { return core.serverAddr }
        return others.count == 1
            ? "\(core.serverAddr) · #\(others[0]) down"
            : "\(core.serverAddr) · \(others.count) channels down"
    }

    private var visible: [Msg] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        return core.messages.filter { m in
            if let channel, m.channel != channel { return false }
            guard !q.isEmpty else { return true }
            let hay = [m.text, m.from, m.target ?? "", m.action ?? "", m.channel,
                       m.isAI ? "ai" : ""].joined(separator: " ").lowercased()
            return hay.contains(q)
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            header
            if let fatal = core.fatal {
                Banner(text: fatal, tone: .red)
            } else if !core.connected {
                Banner(text: "DISCONNECTED from \(core.serverAddr) on #\(postChannel)"
                       + (core.statusDetail.map { " — \($0)" } ?? "")
                       + " — retrying. Messages sent meanwhile will arrive when it comes back.",
                       tone: .red)
            }
            Divider().overlay(Sol.rule)

            Group {
                if pane == .chat {
                    ChatList(messages: visible, query: query)
                } else {
                    ChangesList(messages: visible.filter(\.isChange), query: query)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)

            if pane == .chat { composer }
        }
        .background(Sol.bg)
        .onAppear { adoptHomeChannel(); core.watching = postChannel }
        .onChange(of: core.settingsLoaded) { _, _ in adoptHomeChannel(); core.watching = postChannel }
        // The light follows the room. Switching channels switches which
        // connection it is reporting on.
        .onChange(of: postChannel) { _, now in core.watching = now }
        .onChange(of: state.requestedChannel) { _, wanted in
            guard let wanted, !wanted.isEmpty else { return }
            channel = wanted
            pickedInitial = true
            state.requestedChannel = nil
        }
        .onDrop(of: [.fileURL], isTargeted: nil) { providers in
            for p in providers {
                _ = p.loadObject(ofClass: URL.self) { url, _ in
                    guard let url, url.isFileURL else { return }
                    Task { @MainActor in send(file: url.path) }
                }
            }
            return true
        }
        .sheet(isPresented: $showChannels) {
            ChannelsView(store: store) { core.restartWatcher() }.environmentObject(core)
        }
    }

    private var header: some View {
        HStack(spacing: 12) {
            Picker("", selection: $pane) {
                ForEach(Pane.allCases, id: \.self) { Text($0.rawValue).tag($0) }
            }
            .pickerStyle(.segmented)
            .fixedSize()

            Spacer()

            Picker("", selection: $channel) {
                Text("all channels").tag(String?.none)
                ForEach(core.channels, id: \.self) { Text($0).tag(String?.some($0)) }
            }
            .fixedSize()

            TextField("Search…", text: $query)
                .textFieldStyle(.roundedBorder)
                .frame(width: 200)

            // Making a channel is a person's job, so it lives behind a button
            // and not behind a tool.
            Button { AppState.shared.toggleFullScreen() } label: {
                Image(systemName: "arrow.up.left.and.arrow.down.right")
            }
            .help("Full screen (⌃⌘F)")

            Button { pickFile() } label: {
                Image(systemName: "paperclip")
            }
            .help("Send a file to #\(postChannel) — or drop one on the window")

            Button {
                store.reload()
                showChannels = true
            } label: {
                Image(systemName: "number.square")
            }
            .help("Channels — make one, or join one someone sent you")

            // In the window, not only in the menu bar. An update nobody finds
            // is an update nobody installs, and the menu bar is not where
            // somebody looks for one.
            Button { Updater.checkForUpdates() } label: {
                Image(systemName: "arrow.triangle.2.circlepath")
            }
            .help("Check for updates")

            // The light is about the channel being looked at. Another channel
            // being down is worth saying, but it is not this room's state and
            // it should not colour this room's light.
            HStack(spacing: 6) {
                Circle()
                    .fill(core.connected ? (core.othersDown.isEmpty ? Sol.green : Sol.yellow) : Sol.red)
                    .frame(width: 8, height: 8)
                Text(statusText)
                    .font(.system(size: 11))
                    .foregroundStyle(core.connected ? Sol.fgDim : Sol.red)
                    .help(core.othersDown.isEmpty
                          ? "Connected to \(core.serverAddr) on #\(postChannel)"
                          : "#\(postChannel) is connected. Not connected on "
                            + core.othersDown.map { "#\($0)" }.joined(separator: ", "))
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .background(Sol.bgAlt)
    }

    private var composer: some View {
        VStack(spacing: 0) {
            if !suggestions.isEmpty {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(suggestions.enumerated()), id: \.element.name) { i, s in
                        HStack(spacing: 7) {
                            Text("@" + Self.addressable(s.name))
                                .font(.system(size: 12, weight: .medium))
                                .foregroundStyle(Sol.forName(s.name))
                            if Self.addressable(s.name) != s.name {
                                Text(s.name)
                                    .font(.system(size: 11))
                                    .foregroundStyle(Sol.fgDim)
                            }
                            Text(s.isAI ? "AI" : "Human")
                                .font(.system(size: 9, weight: .bold))
                                .padding(.horizontal, 4).padding(.vertical, 1)
                                .overlay(RoundedRectangle(cornerRadius: 3)
                                    .stroke(s.isAI ? Sol.forName(s.name) : Sol.fgDim, lineWidth: 1))
                                .foregroundStyle(s.isAI ? Sol.forName(s.name) : Sol.fgDim)
                                .opacity(0.7)
                            Spacer()
                            if i == mentionPick {
                                Text("↩").font(.system(size: 10)).foregroundStyle(Sol.fgDim)
                            }
                        }
                        .padding(.horizontal, 10).padding(.vertical, 5)
                        .background(i == mentionPick ? Sol.bg : Color.clear)
                        .contentShape(Rectangle())
                        .onTapGesture { accept(s.name) }
                    }
                }
                .background(RoundedRectangle(cornerRadius: 8).fill(Sol.bgAlt))
                .overlay(RoundedRectangle(cornerRadius: 8).stroke(Sol.rule, lineWidth: 1))
                .padding(.horizontal, 14)
                .padding(.bottom, 6)
                .frame(maxWidth: 320, alignment: .leading)
            }
            Divider().overlay(Sol.rule)
            VStack(alignment: .leading, spacing: 5) {
                HStack(alignment: .bottom, spacing: 9) {
                    TextField("Say something on #\(postChannel)…", text: $draft, axis: .vertical)
                        .textFieldStyle(.roundedBorder)
                        .lineLimit(1...5)
                        .onSubmit { if suggestions.isEmpty { send() } }
                        // While the list is open Return takes the highlighted
                        // name instead of sending, which is what makes it feel
                        // like a suggestion rather than an obstacle.
                        .onKeyPress { press in
                            guard !suggestions.isEmpty else { return .ignored }
                            switch press.key {
                            case .downArrow:
                                mentionPick = min(mentionPick + 1, suggestions.count - 1)
                                return .handled
                            case .upArrow:
                                mentionPick = max(mentionPick - 1, 0)
                                return .handled
                            case .return, .tab:
                                accept(suggestions[min(mentionPick, suggestions.count - 1)].name)
                                return .handled
                            case .escape:
                                draft += " "
                                return .handled
                            default:
                                return .ignored
                            }
                        }
                    Button("Send", action: send)
                        .buttonStyle(.borderedProminent)
                        .tint(Sol.blue)
                        .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
                Text(sendError ?? note ?? "posting as \(core.displayName(on: postChannel)) on #\(postChannel)")
                    .font(.system(size: 11))
                    .foregroundStyle(sendError != nil ? Sol.red : (note != nil ? Sol.green : Sol.fgDim))
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 9)
            .background(Sol.bgAlt)
        }
    }

    /// Waits for the real settings before choosing what to show. Doing this on
    /// appear alone captured the placeholder channel and stuck to it.
    /// The @word being typed right now, if the cursor is still inside one.
    /// Anything with a space in it has stopped being a mention.
    private var mentionQuery: String? {
        guard let at = draft.lastIndex(of: "@") else { return nil }
        let after = draft[draft.index(after: at)...]
        if after.contains(" ") || after.contains("\n") { return nil }
        let before = at == draft.startIndex
            ? true
            : !(draft[draft.index(before: at)].isLetter || draft[draft.index(before: at)].isNumber)
        return before ? String(after) : nil
    }

    private var suggestions: [(name: String, isAI: Bool)] {
        guard let q = mentionQuery else { return [] }
        let all = core.mentionable(on: channel)
        guard !q.isEmpty else { return Array(all.prefix(6)) }
        let qq = q.lowercased()
        return Array(all.filter {
            $0.name.lowercased().hasPrefix(qq) || Self.addressable($0.name).hasPrefix(qq)
        }.prefix(6))
    }

    /// The form a mention has to be written in, mirroring `addressable()` in
    /// core/src/msg.rs. A display name is a person's to choose and may hold
    /// spaces and capitals; the parser stops a mention at the first space, so
    /// "Big Fable" offered verbatim autofills `@Big`, which addresses nobody.
    /// Offering a name that cannot work is worse than offering none.
    static func addressable(_ name: String) -> String {
        name.trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
            .split(whereSeparator: { $0.isWhitespace })
            .joined(separator: "-")
    }

    private func accept(_ name: String) {
        guard let at = draft.lastIndex(of: "@") else { return }
        draft = String(draft[..<at]) + "@" + Self.addressable(name) + " "
        mentionPick = 0
    }

    private func adoptHomeChannel() {
        guard !pickedInitial, core.settingsLoaded else { return }
        channel = core.homeChannel
        pickedInitial = true
    }

    private func pickFile() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = true
        panel.canChooseDirectories = false
        panel.allowsMultipleSelection = false
        panel.prompt = "Send"
        if panel.runModal() == .OK, let url = panel.url {
            send(file: url.path)
        }
    }

    private func send(file path: String) {
        do {
            // Whatever is in the box travels with it — a file arriving with no
            // explanation is a puzzle rather than a message.
            try core.sendFile(path, caption: draft.trimmingCharacters(in: .whitespacesAndNewlines),
                              to: postChannel)
            draft = ""
            sendError = nil
            note = "sent \((path as NSString).lastPathComponent)"
        } catch {
            sendError = "Not sent — \(error.localizedDescription)"
        }
    }

    private func send() {
        let text = draft
        guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        do {
            try core.post(text, to: postChannel)
            draft = ""
            sendError = nil
        } catch {
            // Never let a message that went nowhere look like one that landed.
            sendError = "Not sent — \(error.localizedDescription) Your text is still here."
        }
    }
}

struct Banner: View {
    enum Tone { case red, green }
    let text: String
    let tone: Tone
    var body: some View {
        Text(text)
            .font(.system(size: 12, weight: .medium))
            .foregroundStyle(Sol.onAccent)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 14)
            .padding(.vertical, 7)
            .background(tone == .red ? Sol.red : Sol.green)
    }
}

/// The name, what kind of thing it is, and — when the name does not give it
/// away — which machine. An AI names itself per chat, so "shop" could be either
/// person's Claude, and that is the one question this tool exists to answer.
struct Who: View {
    let name: String
    let isAI: Bool
    let machine: String?

    var body: some View {
        HStack(spacing: 4) {
            Text(name).fontWeight(.semibold).foregroundStyle(Sol.forName(name))
            Text(isAI ? "AI" : "Human")
                .font(.system(size: 9, weight: .bold))
                .padding(.horizontal, 4).padding(.vertical, 1)
                .overlay(RoundedRectangle(cornerRadius: 3)
                    .stroke(isAI ? Sol.forName(name) : Sol.fgDim, lineWidth: 1))
                .foregroundStyle(isAI ? Sol.forName(name) : Sol.fgDim)
                .opacity(isAI ? 0.8 : 0.55)
            if let machine {
                Text(machine).font(.system(size: 10)).foregroundStyle(Sol.fgDim)
            }
        }
    }
}

/// A file in the stream. Deliberately a card rather than a line of text: an
/// attachment is a thing you act on, and styling it like prose made the Save
/// button read as part of the sentence.
struct FileChip: View {
    let msg: Msg
    let file: FileRef
    @EnvironmentObject private var core: Core
    @State private var savedTo: String?
    @State private var failed: String?
    @State private var busy = false

    private var icon: String {
        switch (file.name as NSString).pathExtension.lowercased() {
        case "png", "jpg", "jpeg", "gif", "heic", "webp": return "photo"
        case "lua", "rs", "swift", "cs", "go", "js", "ts", "py", "json", "toml": return "curlybraces"
        case "rbxl", "rbxm", "zip", "tar", "gz": return "shippingbox"
        case "md", "txt": return "doc.text"
        default: return "doc"
        }
    }

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: icon)
                .font(.system(size: 15))
                .foregroundStyle(Sol.cyan)
                .frame(width: 20)

            VStack(alignment: .leading, spacing: 1) {
                Text(file.name)
                    .font(Sol.mono(12, weight: .medium))
                    .foregroundStyle(Sol.fgEm)
                Text(subtitle)
                    .font(.system(size: 11))
                    .foregroundStyle(failed == nil ? Sol.fgDim : Sol.red)
                    .lineLimit(2)
            }

            Spacer(minLength: 10)

            Button(action: act) {
                Label(savedTo == nil ? "Save" : "Show", systemImage: savedTo == nil ? "arrow.down.circle" : "checkmark.circle.fill")
                    .font(.system(size: 11, weight: .medium))
            }
            .buttonStyle(.bordered)
            .controlSize(.small)
            .tint(savedTo == nil ? Sol.blue : Sol.green)
            .disabled(busy)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 7)
        .frame(maxWidth: 460, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 8).fill(Sol.bgAlt))
        .overlay(RoundedRectangle(cornerRadius: 8).stroke(Sol.rule, lineWidth: 1))
    }

    private var subtitle: String {
        if let failed { return failed }
        if let savedTo { return "Saved to \((savedTo as NSString).abbreviatingWithTildeInPath)" }
        return file.readable + (msg.text.isEmpty ? "" : " · \(msg.text)")
    }

    /// First press fetches it; afterwards the button shows you where it went,
    /// which is the question you actually have once it has downloaded.
    private func act() {
        if let savedTo {
            NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: savedTo)])
            return
        }
        busy = true
        defer { busy = false }
        do {
            let out = try core.fetchFile(msg)
            savedTo = out.replacingOccurrences(of: "saved ", with: "")
            failed = nil
        } catch {
            failed = "Could not save — \(error.localizedDescription)"
        }
    }
}

struct ActionBadge: View {
    let action: String
    var body: some View {
        Text(action.uppercased())
            .font(.system(size: 10, weight: .bold))
            .foregroundStyle(Sol.onAccent)
            .padding(.horizontal, 6).padding(.vertical, 1)
            .background(RoundedRectangle(cornerRadius: 4).fill(Sol.forAction(action)))
    }
}

struct ChatList: View {
    let messages: [Msg]
    let query: String
    @EnvironmentObject private var core: Core

    var body: some View {
        if messages.isEmpty {
            Empty(query: query, what: "Nothing on this channel yet.")
        } else {
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 0) {
                        ForEach(Array(messages.enumerated()), id: \.element.seq) { i, m in
                            if i == 0 || !sameDay(messages[i - 1].date, m.date) {
                                DaySeparator(date: m.date)
                            }
                            row(m)
                        }
                        Color.clear.frame(height: 1).id("bottom")
                    }
                    .padding(.vertical, 8)
                }
                .onChange(of: messages.count) { _, _ in
                    withAnimation { proxy.scrollTo("bottom", anchor: .bottom) }
                }
                .onAppear { proxy.scrollTo("bottom", anchor: .bottom) }
            }
        }
    }

    private func row(_ m: Msg) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(m.date, format: .dateTime.hour().minute())
                .font(.system(size: 11)).monospacedDigit()
                .foregroundStyle(Sol.fgDim)
                .frame(width: 54, alignment: .trailing)
            Who(name: m.who, isAI: m.isAI, machine: m.machine)
            if m.isFile, let f = m.file {
                FileChip(msg: m, file: f).padding(.vertical, 2)
            } else if m.isChange {
                ActionBadge(action: m.action ?? "edited")
                Text(m.target ?? "").font(Sol.mono(12)).foregroundStyle(Sol.cyan)
                Text("— " + m.text).foregroundStyle(Sol.fgEm)
            } else {
                MessageBody(text: m.text, me: core.me)
            }
            Spacer(minLength: 0)
        }
        .font(.system(size: 13))
        .padding(.horizontal, 16).padding(.vertical, 2)
    }

    private func sameDay(_ a: Date, _ b: Date) -> Bool {
        Calendar.current.isDate(a, inSameDayAs: b)
    }
}

/// A message body, split into prose and fenced blocks.
///
/// Until today no message could contain a newline: four places replaced them
/// with spaces before anything was stored, so every diagram and table anyone
/// posted arrived as one flowed line. Now that the text survives, a block has
/// to be drawn as a block — alignment is the entire content of a diagram, and
/// prose wrapping destroys it while leaving every character present, which is
/// why nobody noticed for three days.
struct MessageBody: View {
    let text: String
    let me: String

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            ForEach(Array(Self.split(text).enumerated()), id: \.offset) { _, part in
                if part.code {
                    // Horizontal scroll rather than wrapping: a wrapped diagram
                    // is not a smaller diagram, it is a wrong one.
                    ScrollView(.horizontal, showsIndicators: false) {
                        Text(Self.evenlySpaced(part.text, size: 12))
                            .font(Sol.mono(12))
                            .foregroundStyle(Sol.fgEm)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: true, vertical: false)
                            .padding(.horizontal, 9).padding(.vertical, 6)
                    }
                    .background(RoundedRectangle(cornerRadius: 6).fill(Sol.bgAlt))
                    .overlay(RoundedRectangle(cornerRadius: 6).stroke(Sol.rule, lineWidth: 1))
                } else {
                    Text(mentionMarkup(part.text, me: me))
                        .foregroundStyle(Sol.fg)
                        .textSelection(.enabled)
                }
            }
        }
    }

    struct Part { let text: String; let code: Bool }

    /// A block where everything advances by a whole number of cells.
    ///
    /// A monospace table lines up because every character advances the same
    /// distance. An emoji drawn at its own aspect ratio advances by whatever it
    /// happens to be, so a row containing one is wider than its neighbours and
    /// the columns shear — every character present, the table wrong.
    ///
    /// Which clusters need fixing is measured rather than guessed from Unicode
    /// properties: anything that does not already advance like a cell is pinned
    /// to two, which is what a monospace context means by an emoji's width.
    /// Tracking adds to a run's advance, so the correction is the difference.
    static func evenlySpaced(_ text: String, size: CGFloat) -> AttributedString {
        let cell = size * 0.6              // JetBrains Mono advances 600/1000 em
        var out = AttributedString()
        for cluster in text {
            var piece = AttributedString(String(cluster))
            if cluster != "\n" {
                let w = Self.advance(String(cluster), size: size)
                if abs(w - cell) > 0.5 {   // not a plain monospace cell
                    piece.tracking = cell * 2 - w
                }
            }
            out.append(piece)
        }
        return out
    }

    private static var advanceCache: [String: CGFloat] = [:]

    /// What one cluster actually advances, asked of the text system rather than
    /// assumed. Cached: a block is measured character by character, and the
    /// same handful of emoji recur.
    static func advance(_ s: String, size: CGFloat) -> CGFloat {
        let key = "\(s)|\(size)"
        if let c = advanceCache[key] { return c }
        let font = NSFont(name: "JetBrains Mono", size: size)
            ?? NSFont.monospacedSystemFont(ofSize: size, weight: .regular)
        let w = NSAttributedString(string: s, attributes: [.font: font]).size().width
        advanceCache[key] = w
        return w
    }

    /// Splits on ``` fences. An unclosed fence is left as prose rather than
    /// swallowing the rest of the message — a half-typed block should look
    /// wrong, not make everything after it disappear into a box.
    static func split(_ text: String) -> [Part] {
        let lines = text.components(separatedBy: "\n")
        guard lines.contains(where: { $0.trimmingCharacters(in: .whitespaces).hasPrefix("```") })
        else { return [Part(text: text, code: false)] }

        var parts: [Part] = []
        var buf: [String] = []
        var inCode = false
        func flush(_ code: Bool) {
            let joined = buf.joined(separator: "\n")
            if !joined.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                parts.append(Part(text: joined, code: code))
            }
            buf = []
        }
        for line in lines {
            if line.trimmingCharacters(in: .whitespaces).hasPrefix("```") {
                flush(inCode)
                inCode.toggle()
                continue
            }
            buf.append(line)
        }
        // An unclosed fence: whatever is left is prose, not a block.
        flush(inCode && !buf.isEmpty ? false : inCode)
        return parts
    }
}

/// Colours the @names in a message, and makes the one aimed at you stand out —
/// a mention you have to hunt for is not much of a mention.
///
/// Also renders the inline markdown everyone writes anyway. Bold and italic are
/// decoration and it costs nothing to show them; **backticks are not**. On this
/// channel a backticked name is how you write *about* somebody without
/// addressing them, so the one piece of syntax the conversation depends on was
/// the one arriving as punctuation.
///
/// Which is why code runs are excluded from mention colouring below. A live
/// `@name` reaches someone and a backticked one does not, and the window now
/// says which is which instead of drawing them identically.
func mentionMarkup(_ text: String, me: String) -> AttributedString {
    var out = (try? AttributedString(
        markdown: text,
        options: .init(
            // Inline only: bold, italic, code. No headers, lists or block
            // quotes — a chat line is not a document, and a stray "# " at the
            // start of a sentence should stay a "# ".
            interpretedSyntax: .inlineOnlyPreservingWhitespace,
            failurePolicy: .returnPartiallyParsedIfPossible)))
        ?? AttributedString(text)

    for run in out.runs where run.inlinePresentationIntent?.contains(.code) == true {
        out[run.range].font = .system(size: 12, design: .monospaced)
        out[run.range].foregroundColor = Sol.cyan
    }

    let plain = String(out.characters)
    for word in plain.split(whereSeparator: { $0 == " " || $0 == "\n" }) {
        guard word.hasPrefix("@"), word.count > 1 else { continue }
        let name = word.dropFirst().lowercased()
            .trimmingCharacters(in: CharacterSet.alphanumerics.union(CharacterSet(charactersIn: "-_./")).inverted)
        guard let r = out.range(of: String(word)) else { continue }
        if out[r].inlinePresentationIntent?.contains(.code) == true { continue }
        let mine = !me.isEmpty && name == me.lowercased()
        out[r].foregroundColor = mine ? Sol.onAccent : Sol.blue
        out[r].font = .system(size: 13, weight: .semibold)
        if mine { out[r].backgroundColor = Sol.blue }
    }
    return out
}

struct DaySeparator: View {
    let date: Date
    var body: some View {
        HStack(spacing: 10) {
            Rectangle().fill(Sol.rule).frame(height: 1)
            Text(label).font(.system(size: 11)).textCase(.uppercase)
                .tracking(0.7).foregroundStyle(Sol.fgDim).fixedSize()
            Rectangle().fill(Sol.rule).frame(height: 1)
        }
        .padding(.horizontal, 16).padding(.top, 14).padding(.bottom, 6)
    }
    private var label: String {
        if Calendar.current.isDateInToday(date) { return "Today" }
        if Calendar.current.isDateInYesterday(date) { return "Yesterday" }
        return date.formatted(.dateTime.weekday(.abbreviated).month(.abbreviated).day())
    }
}

/// A git log for a project that cannot use git. Consecutive changes by one
/// person close together in time are one entry, the way git log groups edits
/// into a commit.
struct ChangesList: View {
    let messages: [Msg]
    let query: String
    private let gap: TimeInterval = 15 * 60

    private struct Group: Identifiable {
        var id: Int64 { items[0].seq }
        var who: String
        var isAI: Bool
        var machine: String?
        var items: [Msg]
    }

    private var groups: [Group] {
        var out: [Group] = []
        for m in messages {
            if var last = out.last, last.who == m.who, last.isAI == m.isAI,
               m.date.timeIntervalSince(last.items[last.items.count - 1].date) <= gap {
                last.items.append(m)
                out[out.count - 1] = last
            } else {
                out.append(Group(who: m.who, isAI: m.isAI, machine: m.machine, items: [m]))
            }
        }
        return out.reversed() // newest first, like git log
    }

    var body: some View {
        if messages.isEmpty {
            Empty(query: query,
                  what: "No changes recorded yet.\nThey appear here when a session records one.")
        } else {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 16) {
                    ForEach(groups) { g in
                        HStack(alignment: .top, spacing: 12) {
                            Circle().stroke(Sol.blue, lineWidth: 2)
                                .frame(width: 10, height: 10).padding(.top, 5)
                            VStack(alignment: .leading, spacing: 4) {
                                HStack(spacing: 10) {
                                    Who(name: g.who, isAI: g.isAI, machine: g.machine)
                                    Text(span(g)).font(.system(size: 12)).foregroundStyle(Sol.fgDim)
                                }
                                ForEach(g.items) { m in
                                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                                        ActionBadge(action: m.action ?? "edited")
                                            .frame(width: 68, alignment: .center)
                                        Text(m.target ?? "")
                                            .font(Sol.mono(12))
                                            .foregroundStyle(Sol.cyan)
                                        Text(m.text).foregroundStyle(Sol.fg)
                                        Spacer(minLength: 0)
                                    }
                                    .font(.system(size: 13))
                                }
                            }
                        }
                        .padding(.horizontal, 16)
                    }
                }
                .padding(.vertical, 14)
            }
        }
    }

    private func span(_ g: Group) -> String {
        let first = g.items[0].date, last = g.items[g.items.count - 1].date
        let day = Calendar.current.isDateInToday(first) ? "Today"
            : first.formatted(.dateTime.month(.abbreviated).day())
        let a = first.formatted(.dateTime.hour().minute())
        let b = last.formatted(.dateTime.hour().minute())
        let time = a == b ? a : "\(a)–\(b)"
        let n = g.items.count
        return "\(day) \(time) · \(n) change\(n == 1 ? "" : "s") · #\(g.items[0].seq)"
    }
}

struct Empty: View {
    let query: String
    let what: String
    var body: some View {
        VStack {
            Spacer()
            Text(query.trimmingCharacters(in: .whitespaces).isEmpty
                 ? what : "Nothing matches “\(query)”.")
                .multilineTextAlignment(.center)
                .italic()
                .foregroundStyle(Sol.fgDim)
            Spacer()
        }
        .frame(maxWidth: .infinity)
    }
}
