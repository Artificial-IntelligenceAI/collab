// The picture behind the two icons in the disk image window. Generated rather
// than checked in, for the same reason the app icon is: this file is the source.
import AppKit

let W = 660.0, H = 420.0
let base3 = NSColor(srgbRed: 0xfd/255.0, green: 0xf6/255.0, blue: 0xe3/255.0, alpha: 1)
let base2 = NSColor(srgbRed: 0xee/255.0, green: 0xe8/255.0, blue: 0xd5/255.0, alpha: 1)
let base01 = NSColor(srgbRed: 0x58/255.0, green: 0x6e/255.0, blue: 0x75/255.0, alpha: 1)
let base1 = NSColor(srgbRed: 0x93/255.0, green: 0xa1/255.0, blue: 0xa1/255.0, alpha: 1)
let blue = NSColor(srgbRed: 0x26/255.0, green: 0x8b/255.0, blue: 0xd2/255.0, alpha: 1)

let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(W), pixelsHigh: Int(H),
                           bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                           colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)

base3.setFill()
NSRect(x: 0, y: 0, width: W, height: H).fill()

// A band behind the two icons, so they read as one gesture rather than two things
base2.setFill()
NSBezierPath(roundedRect: NSRect(x: 40, y: 150, width: W - 80, height: 170),
             xRadius: 14, yRadius: 14).fill()

// The arrow: from the app towards the Applications folder
let arrow = NSBezierPath()
arrow.move(to: NSPoint(x: 268, y: 236))
arrow.line(to: NSPoint(x: 386, y: 236))
arrow.lineWidth = 6
arrow.lineCapStyle = .round
blue.withAlphaComponent(0.55).setStroke()
arrow.stroke()
let head = NSBezierPath()
head.move(to: NSPoint(x: 408, y: 236))
head.line(to: NSPoint(x: 380, y: 252))
head.line(to: NSPoint(x: 380, y: 220))
head.close()
blue.withAlphaComponent(0.55).setFill()
head.fill()

func text(_ s: String, _ y: Double, size: Double, color: NSColor, weight: NSFont.Weight = .regular) {
    let f = NSFont.systemFont(ofSize: size, weight: weight)
    let a: [NSAttributedString.Key: Any] = [.font: f, .foregroundColor: color]
    let str = NSAttributedString(string: s, attributes: a)
    str.draw(at: NSPoint(x: (W - str.size().width) / 2, y: y))
}

text("Drag collab into Applications", H - 78, size: 21, color: base01, weight: .semibold)
text("Everything it needs travels inside the app.", H - 106, size: 13, color: base1)
text("Open it afterwards and it will ask you two questions.", 74, size: 12, color: base1)

NSGraphicsContext.restoreGraphicsState()
let out = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "dmg-background.png"
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: out))
print("wrote \(out)")
