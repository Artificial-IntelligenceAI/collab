import SwiftUI

enum Pane: String, CaseIterable { case chat = "Chat", changes = "Changes" }

struct ContentView: View {
    @ObservedObject var core: Core
    @State private var pane: Pane = .chat
    @State private var query = ""
    @State private var channel: String?
    @State private var draft = ""
    @State private var sendError: String?

    /// You post where you are looking. Reading one channel and typing into
    /// another would be a nasty little trap.
    private var postChannel: String { channel ?? core.homeChannel }

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
                Banner(text: "DISCONNECTED from \(core.serverAddr)"
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
        .onAppear { if channel == nil { channel = core.homeChannel } }
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

            HStack(spacing: 6) {
                Circle()
                    .fill(core.connected ? Sol.green : Sol.red)
                    .frame(width: 8, height: 8)
                Text(core.connected ? core.serverAddr : "disconnected")
                    .font(.system(size: 11))
                    .foregroundStyle(core.connected ? Sol.fgDim : Sol.red)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .background(Sol.bgAlt)
    }

    private var composer: some View {
        VStack(spacing: 0) {
            Divider().overlay(Sol.rule)
            VStack(alignment: .leading, spacing: 5) {
                HStack(alignment: .bottom, spacing: 9) {
                    TextField("Say something on #\(postChannel)…", text: $draft, axis: .vertical)
                        .textFieldStyle(.roundedBorder)
                        .lineLimit(1...5)
                        .onSubmit(send)
                    Button("Send", action: send)
                        .buttonStyle(.borderedProminent)
                        .tint(Sol.blue)
                        .disabled(draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
                Text(sendError ?? "posting as \(core.me) on #\(postChannel)")
                    .font(.system(size: 11))
                    .foregroundStyle(sendError == nil ? Sol.fgDim : Sol.red)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 9)
            .background(Sol.bgAlt)
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
            if m.isChange {
                ActionBadge(action: m.action ?? "edited")
                Text(m.target ?? "").font(.system(size: 12, design: .monospaced)).foregroundStyle(Sol.cyan)
                Text("— " + m.text).foregroundStyle(Sol.fgEm)
            } else {
                Text(m.text).foregroundStyle(Sol.fg).textSelection(.enabled)
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
                                            .font(.system(size: 12, design: .monospaced))
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
