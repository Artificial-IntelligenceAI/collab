// Renders collab.icns. Run once via build.sh; needs only Xcode's Swift and
// iconutil, so the icon is generated here rather than checked in as a binary.
import AppKit

let BASE03 = NSColor(srgbRed: 0x00/255.0, green: 0x2b/255.0, blue: 0x36/255.0, alpha: 1)
let BLUE   = NSColor(srgbRed: 0x26/255.0, green: 0x8b/255.0, blue: 0xd2/255.0, alpha: 1)
let GREEN  = NSColor(srgbRed: 0x85/255.0, green: 0x99/255.0, blue: 0x00/255.0, alpha: 1)

// A speech bubble, as two separate paths. They are filled separately and never
// combined into one compound path: a tail wound the opposite way to the bubble
// cancels against it under a non-zero fill and punches a notch through itself.
func bubblePaths(_ r: NSRect, radius: CGFloat, tailLeft: Bool) -> (NSBezierPath, NSBezierPath) {
    let body = NSBezierPath(roundedRect: r, xRadius: radius, yRadius: radius)
    let tail = NSBezierPath()
    let y = r.minY + radius * 0.5
    let x = tailLeft ? r.minX + radius * 1.6 : r.maxX - radius * 1.6
    let d: CGFloat = tailLeft ? -1 : 1
    tail.move(to: NSPoint(x: x - d * radius * 0.75, y: y))
    tail.line(to: NSPoint(x: x + d * radius * 1.15, y: r.minY - radius * 0.95))
    tail.line(to: NSPoint(x: x + d * radius * 0.55, y: y))
    tail.close()
    return (body, tail)
}

func fillBubble(_ r: NSRect, radius: CGFloat, tailLeft: Bool, grow: CGFloat, _ color: NSColor) {
    let (body, tail) = bubblePaths(r, radius: radius, tailLeft: tailLeft)
    if grow != 1 {
        let t = NSAffineTransform()
        t.translateX(by: r.midX, yBy: r.midY)
        t.scale(by: grow)
        t.translateX(by: -r.midX, yBy: -r.midY)
        body.transform(using: t as AffineTransform)
        tail.transform(using: t as AffineTransform)
    }
    color.setFill()
    body.fill()
    tail.fill()
}

func render(_ px: Int) -> Data {
    let s = CGFloat(px)
    let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px,
                               bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
                               isPlanar: false, colorSpaceName: .deviceRGB,
                               bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)

    // the squircle, inset the way macOS app icons are
    let inset = s * 0.085
    let plate = NSRect(x: inset, y: inset, width: s - inset * 2, height: s - inset * 2)
    BASE03.setFill()
    NSBezierPath(roundedRect: plate, xRadius: s * 0.2237, yRadius: s * 0.2237).fill()

    // two bubbles, one per session, overlapping — blue behind, green in front
    let back  = NSRect(x: s * 0.205, y: s * 0.455, width: s * 0.46, height: s * 0.30)
    let front = NSRect(x: s * 0.335, y: s * 0.245, width: s * 0.46, height: s * 0.30)
    fillBubble(back,  radius: s * 0.085, tailLeft: true,  grow: 1, BLUE)

    // the front bubble sits on a rim of the plate colour, so the two stay
    // distinguishable when this is drawn 16 pixels wide
    fillBubble(front, radius: s * 0.085, tailLeft: false, grow: 1.11, BASE03)
    fillBubble(front, radius: s * 0.085, tailLeft: false, grow: 1,    GREEN)

    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

let out = CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "collab.iconset"
try? FileManager.default.createDirectory(atPath: out, withIntermediateDirectories: true)
for (name, px) in [("16x16", 16), ("16x16@2x", 32), ("32x32", 32), ("32x32@2x", 64),
                   ("128x128", 128), ("128x128@2x", 256), ("256x256", 256),
                   ("256x256@2x", 512), ("512x512", 512), ("512x512@2x", 1024)] {
    try! render(px).write(to: URL(fileURLWithPath: "\(out)/icon_\(name).png"))
}
print("wrote \(out)")
