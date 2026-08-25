import AppKit
import SwiftUI

/// The message box.
///
/// This was a SwiftUI `TextField(axis: .vertical)` until 2026-08-25, and the
/// reason it is no longer one is worth writing down, because the replacement
/// looks like a lot of machinery for a text box.
///
/// A `TextField` bound to a `String` will not tell you where the caret is. So
/// "put a line break in" could only be implemented as "append a line break to
/// the end", and a line break on the end is trimmed off on the way out — for
/// good reason, since a trailing blank line carries nothing. The result was a
/// box that grew a second line and a message that arrived without one, which
/// is worse than not having the key at all: it looked like it worked.
///
/// An `NSTextView` knows where the caret is, so the break goes where the
/// person is typing and survives the trip.
struct Composer: NSViewRepresentable {
    @Binding var text: String
    var placeholder: String
    /// Return with no modifiers. The caller decides whether that sends or
    /// takes the highlighted name from the suggestion list.
    var onReturn: () -> Void
    /// Arrow keys and Tab and Escape, so the suggestion list keeps working —
    /// it is the field's own key handling that has to yield to it.
    var onKey: (KeyStroke) -> Bool
    /// How tall the text wants to be, in points, clamped by the caller.
    var onHeight: (CGFloat) -> Void

    enum KeyStroke { case up, down, tab, escape }

    func makeCoordinator() -> Coordinator { Coordinator(self) }

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSScrollView()
        scroll.drawsBackground = false
        scroll.hasVerticalScroller = false
        scroll.autohidesScrollers = true
        scroll.borderType = .noBorder

        let view = KeyingTextView()
        view.delegate = context.coordinator
        view.owner = context.coordinator
        view.isRichText = false
        view.isEditable = true
        view.allowsUndo = true
        view.drawsBackground = false
        view.textContainerInset = NSSize(width: 4, height: 5)
        view.font = .systemFont(ofSize: 13)
        view.textColor = NSColor(Sol.fg)
        view.insertionPointColor = NSColor(Sol.blue)
        // Every one of these is on by default in an NSTextView and every one
        // is wrong in a message box: "..." for three dots, a capital after a
        // full stop, and — the one that would actually hurt — curly quotes,
        // which stop `backticks` and code from being copied out as typed.
        view.isAutomaticTextReplacementEnabled = false
        view.isAutomaticQuoteSubstitutionEnabled = false
        view.isAutomaticDashSubstitutionEnabled = false
        view.isAutomaticSpellingCorrectionEnabled = false
        view.isContinuousSpellCheckingEnabled = false
        view.textContainer?.widthTracksTextView = true
        view.isVerticallyResizable = true
        view.isHorizontallyResizable = false
        view.autoresizingMask = [.width]

        scroll.documentView = view
        context.coordinator.view = view
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard let view = scroll.documentView as? KeyingTextView else { return }
        context.coordinator.parent = self
        if view.string != text {
            view.string = text
            // Typing into an empty box and having the caret sit at the front
            // is the sort of thing that only happens to somebody else. This
            // is the path where the binding changed underneath us — accepting
            // a suggestion, or clearing after a send — so the end is right.
            view.setSelectedRange(NSRange(location: view.string.count, length: 0))
        }
        view.placeholder = placeholder
        view.needsDisplay = true
        context.coordinator.reportHeight()
    }

    final class Coordinator: NSObject, NSTextViewDelegate {
        var parent: Composer
        weak var view: KeyingTextView?
        init(_ parent: Composer) { self.parent = parent }

        func textDidChange(_ notification: Notification) {
            guard let view = view else { return }
            parent.text = view.string
            reportHeight()
        }

        /// The last height handed out, so an unchanged measurement does not
        /// write state and start another layout pass.
        private var lastHeight: CGFloat = 0

        func reportHeight() {
            guard let view = view,
                  let container = view.textContainer,
                  let manager = view.layoutManager else { return }
            let inset = view.textContainerInset.height * 2
            let height: CGFloat
            if view.string.isEmpty {
                // Do not measure an empty box. Right after a send the text is
                // gone but the old layout is not, so `usedRect` answers with
                // the height of the message that just left — the box jumped to
                // full size and snapped back a moment later, on an empty field.
                height = (view.font?.boundingRectForFont.height ?? 16) + inset
            } else {
                manager.ensureLayout(for: container)
                height = manager.usedRect(for: container).height + inset
            }
            guard abs(height - lastHeight) > 0.5 else { return }
            lastHeight = height
            // Never during the update itself: `updateNSView` calls this, and a
            // state write from inside a view update is applied on some later
            // pass of SwiftUI's choosing, which is the other half of the flicker.
            DispatchQueue.main.async { [parent] in parent.onHeight(height) }
        }

        /// Return sends; Return with Shift or Command makes a line. The second
        /// half is the whole reason this file exists.
        func handle(_ event: NSEvent) -> Bool {
            let mods = event.modifierFlags.intersection(.deviceIndependentFlagsMask)
            switch event.keyCode {
            case 36, 76: // Return, Enter
                if mods.contains(.shift) || mods.contains(.command) {
                    // A line break before anything has been typed carries
                    // nothing — it is trimmed off on the way out — but the box
                    // still grows for it, so an empty composer silently became
                    // two lines tall with nothing in it. Swallow the key
                    // instead: there is no line to break yet.
                    guard let v = view,
                          !v.string.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                    else { return true }
                    v.insertNewline(nil)
                    return true
                }
                parent.onReturn()
                return true
            case 126: return parent.onKey(.up)
            case 125: return parent.onKey(.down)
            case 48:  return parent.onKey(.tab)
            case 53:  return parent.onKey(.escape)
            default:  return false
            }
        }
    }

    /// `keyDown` rather than `doCommandBy:`, because the distinction that
    /// matters here — Return alone versus Return with a modifier — has already
    /// been thrown away by the time a command selector arrives.
    final class KeyingTextView: NSTextView {
        weak var owner: Composer.Coordinator?
        var placeholder: String = ""

        override func keyDown(with event: NSEvent) {
            if owner?.handle(event) == true { return }
            super.keyDown(with: event)
        }

        override func draw(_ dirtyRect: NSRect) {
            super.draw(dirtyRect)
            guard string.isEmpty, !placeholder.isEmpty else { return }
            let style = NSMutableParagraphStyle()
            style.lineBreakMode = .byTruncatingTail
            placeholder.draw(
                at: NSPoint(x: textContainerInset.width + 5, y: textContainerInset.height),
                withAttributes: [
                    .font: font ?? .systemFont(ofSize: 13),
                    .foregroundColor: NSColor(Sol.fgDim).withAlphaComponent(0.7),
                    .paragraphStyle: style,
                ])
        }
    }
}
