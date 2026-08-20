// Solarized. Light and dark share the accents; only the eight greys swap,
// which is the whole point of the palette.
import AppKit
import SwiftUI

enum Sol {
    static let base03 = NSColor(srgbRed: 0x00 / 255, green: 0x2b / 255, blue: 0x36 / 255, alpha: 1)
    static let base02 = NSColor(srgbRed: 0x07 / 255, green: 0x36 / 255, blue: 0x42 / 255, alpha: 1)
    static let base01 = NSColor(srgbRed: 0x58 / 255, green: 0x6e / 255, blue: 0x75 / 255, alpha: 1)
    static let base00 = NSColor(srgbRed: 0x65 / 255, green: 0x7b / 255, blue: 0x83 / 255, alpha: 1)
    static let base0  = NSColor(srgbRed: 0x83 / 255, green: 0x94 / 255, blue: 0x96 / 255, alpha: 1)
    static let base1  = NSColor(srgbRed: 0x93 / 255, green: 0xa1 / 255, blue: 0xa1 / 255, alpha: 1)
    static let base2  = NSColor(srgbRed: 0xee / 255, green: 0xe8 / 255, blue: 0xd5 / 255, alpha: 1)
    static let base3  = NSColor(srgbRed: 0xfd / 255, green: 0xf6 / 255, blue: 0xe3 / 255, alpha: 1)

    static let yellow  = Color(nsColor: NSColor(srgbRed: 0xb5 / 255, green: 0x89 / 255, blue: 0x00 / 255, alpha: 1))
    static let orange  = Color(nsColor: NSColor(srgbRed: 0xcb / 255, green: 0x4b / 255, blue: 0x16 / 255, alpha: 1))
    static let red     = Color(nsColor: NSColor(srgbRed: 0xdc / 255, green: 0x32 / 255, blue: 0x2f / 255, alpha: 1))
    static let magenta = Color(nsColor: NSColor(srgbRed: 0xd3 / 255, green: 0x36 / 255, blue: 0x82 / 255, alpha: 1))
    static let violet  = Color(nsColor: NSColor(srgbRed: 0x6c / 255, green: 0x71 / 255, blue: 0xc4 / 255, alpha: 1))
    static let blue    = Color(nsColor: NSColor(srgbRed: 0x26 / 255, green: 0x8b / 255, blue: 0xd2 / 255, alpha: 1))
    static let cyan    = Color(nsColor: NSColor(srgbRed: 0x2a / 255, green: 0xa1 / 255, blue: 0x98 / 255, alpha: 1))
    static let green   = Color(nsColor: NSColor(srgbRed: 0x85 / 255, green: 0x99 / 255, blue: 0x00 / 255, alpha: 1))

    /// Follows the OS, without every view having to know which mode it is in.
    private static func dyn(_ light: NSColor, _ dark: NSColor) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.aqua, .darkAqua]) == .darkAqua ? dark : light
        })
    }

    static let bg     = dyn(base3, base03)
    static let bgAlt  = dyn(base2, base02)
    static let fg     = dyn(base00, base0)
    static let fgEm   = dyn(base01, base1)
    static let fgDim  = dyn(base1, base01)
    static let rule   = dyn(base01.withAlphaComponent(0.18), base1.withAlphaComponent(0.16))
    static let onAccent = dyn(base3, base03)

    /// One stable colour per person, so you can tell who is who at a glance.
    /// Someone and their AI share a colour — it is the same household.
    static func forName(_ name: String) -> Color {
        let palette: [Color] = [blue, magenta, cyan, violet, orange, green, yellow]
        var h: UInt32 = 0
        for b in name.utf8 { h = h &* 31 &+ UInt32(b) }
        return palette[Int(h % UInt32(palette.count))]
    }

    static func forAction(_ action: String) -> Color {
        switch action {
        case "added": return green
        case "removed": return red
        case "renamed": return violet
        default: return blue
        }
    }
}
