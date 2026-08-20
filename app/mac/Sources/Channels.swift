// Making and sharing channels — the one thing here that only a person does.
//
// An AI has no tool for this. That is a guardrail rather than a wall: anything
// with a shell on this machine could write the file. It is aimed at the failure
// that actually happens, which is a chat inventing a reasonable-sounding
// channel that matches nothing on the other machine and then talking to an
// empty room. Across machines it is real — without the key there is no way in.
import AppKit
import SwiftUI

struct ChannelInfo: Codable, Identifiable, Hashable {
    var name: String
    var key: String
    var mine: Bool
    var created: String
    var creator: String
    var id: String { name }
}

@MainActor
final class ChannelStore: ObservableObject {
    @Published private(set) var channels: [ChannelInfo] = []
    @Published var error: String?

    private func collab(_ args: [String]) throws -> String {
        guard let bin = Core.binary() else { throw CollabError.noBinary }
        let p = Process()
        p.executableURL = URL(fileURLWithPath: bin)
        p.arguments = args
        let out = Pipe(), err = Pipe()
        p.standardOutput = out
        p.standardError = err
        try p.run()
        let o = out.fileHandleForReading.readDataToEndOfFile()
        let e = err.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        guard p.terminationStatus == 0 else {
            throw CollabError.command(String(decoding: e, as: UTF8.self)
                .trimmingCharacters(in: .whitespacesAndNewlines))
        }
        return String(decoding: o, as: UTF8.self)
    }

    func reload() {
        do {
            let json = try collab(["channels", "-json"])
            channels = (try? JSONDecoder().decode([ChannelInfo].self,
                                                  from: Data(json.utf8))) ?? []
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// Returns the new channel's key, so it can be handed over straight away —
    /// a channel nobody else has the key to is not yet a channel.
    @discardableResult
    func create(_ name: String) -> String? {
        do {
            let out = try collab(["channel", "create", name])
            reload()
            error = nil
            return out.split(separator: "\n").last.map(String.init)
        } catch {
            self.error = error.localizedDescription
            return nil
        }
    }

    func add(name: String, key: String) -> Bool {
        do {
            _ = try collab(["channel", "add", name, key])
            reload()
            error = nil
            return true
        } catch {
            self.error = error.localizedDescription
            return false
        }
    }

    /// Leaving: drops this machine's key and touches nobody else's.
    func forget(_ name: String) {
        do {
            _ = try collab(["channel", "forget", name])
            reload()
        } catch {
            self.error = error.localizedDescription
        }
    }

    /// Closing the room, for everyone. Only where it was made.
    func delete(_ name: String) {
        do {
            _ = try collab(["channel", "delete", name])
            reload()
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }
}

struct ChannelsView: View {
    @ObservedObject var store: ChannelStore
    var onChange: () -> Void
    @Environment(\.dismiss) private var dismiss

    @State private var newName = ""
    @State private var addName = ""
    @State private var addKey = ""
    @State private var justMade: (name: String, key: String)?
    @State private var copied: String?
    @State private var confirming: ChannelInfo?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text("Channels")
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(Sol.fgEm)
                .padding(.horizontal, 16).padding(.top, 14).padding(.bottom, 2)
            Text("A channel is a separate conversation with its own key. Both machines need the same key, or neither sees the other.")
                .font(.system(size: 11)).foregroundStyle(Sol.fgDim)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, 16).padding(.bottom, 10)

            Divider().overlay(Sol.rule)

            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    if store.channels.isEmpty {
                        Text("No channels yet. Make one below, then send its key to the other person.")
                            .italic().foregroundStyle(Sol.fgDim)
                            .padding(.vertical, 8)
                    }
                    ForEach(store.channels) { c in
                        row(c)
                    }
                }
                .padding(.horizontal, 16).padding(.vertical, 12)
            }
            .frame(maxHeight: 200)

            Divider().overlay(Sol.rule)
            maker
        }
        .frame(width: 470)
        .background(Sol.bg)
        .alert("Delete #\(confirming?.name ?? "")?",
               isPresented: Binding(get: { confirming != nil },
                                    set: { if !$0 { confirming = nil } })) {
            Button("Delete", role: .destructive) {
                if let c = confirming { store.delete(c.name); onChange() }
                confirming = nil
            }
            Button("Cancel", role: .cancel) { confirming = nil }
        } message: {
            Text("Its messages are deleted from the server and its key is dropped here. "
                 + "Anyone still holding the key will no longer be able to connect. This cannot be undone.")
        }
    }

    private func row(_ c: ChannelInfo) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack {
                Text("#\(c.name)").fontWeight(.semibold).foregroundStyle(Sol.forName(c.name))
                Text(c.mine ? "made here" : "joined")
                    .font(.system(size: 10)).foregroundStyle(Sol.fgDim)
                Spacer()
                Button(copied == c.name ? "Copied" : "Copy key") { copy(c.key, tag: c.name) }
                    .buttonStyle(.borderless).font(.system(size: 11))
                // Made here: closing it is real, and irreversible, so it asks.
                // Given to you: you can only put your own copy down.
                if c.mine {
                    Button("Delete") { confirming = c }
                        .buttonStyle(.borderless).font(.system(size: 11))
                        .foregroundStyle(Sol.red)
                        .help("Closes #\(c.name) for everyone and deletes its messages")
                } else {
                    Button("Leave") { store.forget(c.name); onChange() }
                        .buttonStyle(.borderless).font(.system(size: 11))
                        .help("Drops your key. Only \(c.creator.isEmpty ? "whoever made it" : c.creator) can delete it")
                }
            }
            Text(c.key)
                .font(.system(size: 10, design: .monospaced))
                .foregroundStyle(Sol.fgDim).textSelection(.enabled).lineLimit(1)
        }
    }

    private var maker: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let made = justMade {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Made #\(made.name). Send this key to the other person — until they have it, nobody else can see the channel.")
                        .font(.system(size: 11)).foregroundStyle(Sol.fgEm)
                        .fixedSize(horizontal: false, vertical: true)
                    HStack {
                        Text(made.key).font(.system(size: 10, design: .monospaced))
                            .textSelection(.enabled).lineLimit(1)
                        Spacer()
                        Button(copied == made.name ? "Copied" : "Copy") { copy(made.key, tag: made.name) }
                            .font(.system(size: 11))
                    }
                }
                .padding(9)
                .background(RoundedRectangle(cornerRadius: 6).fill(Sol.bgAlt))
            }

            HStack {
                TextField("New channel name", text: $newName)
                    .textFieldStyle(.roundedBorder)
                    .onSubmit(make)
                Button("Create", action: make)
                    .buttonStyle(.borderedProminent).tint(Sol.blue)
                    .disabled(newName.trimmingCharacters(in: .whitespaces).isEmpty)
            }

            HStack {
                TextField("Name someone sent you", text: $addName)
                    .textFieldStyle(.roundedBorder).frame(width: 150)
                TextField("…and its key", text: $addKey)
                    .textFieldStyle(.roundedBorder)
                Button("Join") {
                    if store.add(name: addName, key: addKey) {
                        addName = ""; addKey = ""; onChange()
                    }
                }
                .disabled(addName.trimmingCharacters(in: .whitespaces).isEmpty
                          || addKey.trimmingCharacters(in: .whitespaces).isEmpty)
            }

            if let e = store.error {
                Text(e).font(.system(size: 11)).foregroundStyle(Sol.red)
                    .fixedSize(horizontal: false, vertical: true)
            }

            HStack {
                Spacer()
                Button("Done") { dismiss() }.keyboardShortcut(.defaultAction)
            }
        }
        .padding(16)
    }

    private func make() {
        let name = newName.trimmingCharacters(in: .whitespaces)
        guard !name.isEmpty, let key = store.create(name) else { return }
        justMade = (store.channels.last(where: { $0.key == key })?.name ?? name, key)
        newName = ""
        onChange()
    }

    private func copy(_ s: String, tag: String) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(s, forType: .string)
        copied = tag
    }
}
