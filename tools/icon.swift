// Draws MDView's app icon and writes a .iconset directory.
//
// The icon is code rather than a checked-in drawing so it can be reasoned
// about and adjusted: every size is drawn at its own resolution instead of
// being downsampled from one master, which is what keeps the 16pt version
// from turning to mush.
//
// Design: the Markdown mark -- a rounded rectangle enclosing "M" and a down
// arrow, the notation's own logo -- in the parchment cream this app uses for
// its Solarized Light theme, on a slate squircle. Cream on slate rather than
// the reverse because a Dock is mostly saturated colour, and because the mark
// has to stay legible when it is 16 points across.
//
// Usage: icon <output.iconset>

import Cocoa

let args = CommandLine.arguments
guard args.count >= 2 else {
    FileHandle.standardError.write("usage: icon <output.iconset>\n".data(using: .utf8)!)
    exit(2)
}
let outDir = URL(fileURLWithPath: args[1])

// Everything below is expressed on a 1024pt canvas and scaled per size.
let canvas: CGFloat = 1024

// macOS 26 composes app icons itself: it masks the artwork to the system
// shape and casts the shadow. Artwork that draws its own rounded body and
// leaves transparent margins gets treated as a loose image and placed on a
// default tile, which renders as a squircle inside a squircle. So the art is
// full bleed -- every pixel opaque, no self-drawn corners, no shadow -- and
// the system supplies the silhouette.
//
// The cost is older macOS, which masks nothing and would show a hard square.
// Corners are the only part the system crops, so the mark sits well inside
// the safe area and reads correctly either way.
let markSafeScale: CGFloat = 0.95

func rgb(_ r: Int, _ g: Int, _ b: Int) -> CGColor {
    CGColor(srgbRed: CGFloat(r) / 255, green: CGFloat(g) / 255, blue: CGFloat(b) / 255, alpha: 1)
}

let slateTop = rgb(62, 74, 88)
let slateBottom = rgb(31, 38, 46)
let cream = rgb(253, 246, 227)  // #fdf6e3, the Solarized Light page colour

/// macOS icon corners are a continuous curve, not a circular arc. A rounded
/// rect is close but reads subtly wrong beside real icons, so sample the
/// superellipse Apple's shape actually follows.
func squircle(in rect: CGRect, exponent: CGFloat = 5) -> CGPath {
    let path = CGMutablePath()
    let a = rect.width / 2, b = rect.height / 2
    let cx = rect.midX, cy = rect.midY
    let steps = 720
    for i in 0...steps {
        let t = CGFloat(i) / CGFloat(steps) * 2 * .pi
        let ct = cos(t), st = sin(t)
        // Signed |cos|^(2/n) form of the superellipse.
        let x = cx + a * pow(abs(ct), 2 / exponent) * (ct < 0 ? -1 : 1)
        let y = cy + b * pow(abs(st), 2 / exponent) * (st < 0 ? -1 : 1)
        if i == 0 { path.move(to: CGPoint(x: x, y: y)) } else { path.addLine(to: CGPoint(x: x, y: y)) }
    }
    path.closeSubpath()
    return path
}

func drawIcon(into ctx: CGContext, size: CGFloat) {
    let scale = size / canvas
    ctx.saveGState()
    ctx.scaleBy(x: scale, y: scale)
    // Draw in top-down coordinates; easier to reason about than Quartz's.
    ctx.translateBy(x: 0, y: canvas)
    ctx.scaleBy(x: 1, y: -1)

    // Full-bleed gradient; the system crops it to the icon silhouette.
    let body = CGRect(x: 0, y: 0, width: canvas, height: canvas)
    ctx.saveGState()
    ctx.clip(to: body)
    let space = CGColorSpaceCreateDeviceRGB()
    if let gradient = CGGradient(
        colorsSpace: space, colors: [slateTop, slateBottom] as CFArray, locations: [0, 1])
    {
        ctx.drawLinearGradient(
            gradient, start: CGPoint(x: 0, y: body.minY), end: CGPoint(x: 0, y: body.maxY),
            options: [])
    }
    ctx.restoreGState()

    // Keep the mark inside the area the system mask never touches.
    ctx.translateBy(x: 512, y: 512)
    ctx.scaleBy(x: markSafeScale, y: markSafeScale)
    ctx.translateBy(x: -512, y: -512)

    // --- the Markdown mark -------------------------------------------------
    // Below 128pt the enclosing rectangle stops being a shape and becomes a
    // smudge around an unreadable glyph, so the small sizes drop it and give
    // the M and the arrow the whole face instead. This is the reason each
    // size is drawn rather than downsampled from one master.
    let showBox = size >= 128
    if !showBox {
        ctx.translateBy(x: 512, y: 512)
        ctx.scaleBy(x: 1.5, y: 1.5)
        ctx.translateBy(x: -512, y: -512)
    }

    ctx.setStrokeColor(cream)
    ctx.setFillColor(cream)
    ctx.setLineJoin(.round)
    ctx.setLineCap(.round)

    // Enclosing rounded rectangle.
    if showBox {
        let markBox = CGRect(x: 232, y: 332, width: 560, height: 360)
        ctx.addPath(CGPath(roundedRect: markBox, cornerWidth: 66, cornerHeight: 66, transform: nil))
        ctx.setLineWidth(44)
        ctx.strokePath()
    }

    // "M"
    ctx.setLineWidth(52)
    ctx.move(to: CGPoint(x: 322, y: 606))
    ctx.addLine(to: CGPoint(x: 322, y: 418))
    ctx.addLine(to: CGPoint(x: 416, y: 512))
    ctx.addLine(to: CGPoint(x: 510, y: 418))
    ctx.addLine(to: CGPoint(x: 510, y: 606))
    ctx.strokePath()

    // Down arrow: stem plus a solid head, the way the real mark draws it.
    ctx.setLineWidth(52)
    ctx.move(to: CGPoint(x: 645, y: 418))
    ctx.addLine(to: CGPoint(x: 645, y: 512))
    ctx.strokePath()
    ctx.move(to: CGPoint(x: 557, y: 494))
    ctx.addLine(to: CGPoint(x: 733, y: 494))
    ctx.addLine(to: CGPoint(x: 645, y: 616))
    ctx.closePath()
    ctx.fillPath()

    ctx.restoreGState()
}

func writePNG(size: Int, to url: URL) {
    guard
        let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0),
        let ctx = NSGraphicsContext(bitmapImageRep: rep)
    else {
        FileHandle.standardError.write("could not make a \(size)pt bitmap\n".data(using: .utf8)!)
        exit(1)
    }
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = ctx
    drawIcon(into: ctx.cgContext, size: CGFloat(size))
    NSGraphicsContext.restoreGraphicsState()

    guard let png = rep.representation(using: .png, properties: [:]) else {
        FileHandle.standardError.write("could not encode \(size)pt\n".data(using: .utf8)!)
        exit(1)
    }
    do { try png.write(to: url) } catch {
        FileHandle.standardError.write("could not write \(url.path): \(error)\n".data(using: .utf8)!)
        exit(1)
    }
}

try? FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)

// The set `iconutil` expects. Each entry is drawn at its own size rather than
// scaled down from one master.
let sizes: [(name: String, px: Int)] = [
    ("icon_16x16", 16), ("icon_16x16@2x", 32),
    ("icon_32x32", 32), ("icon_32x32@2x", 64),
    ("icon_128x128", 128), ("icon_128x128@2x", 256),
    ("icon_256x256", 256), ("icon_256x256@2x", 512),
    ("icon_512x512", 512), ("icon_512x512@2x", 1024),
]
for (name, px) in sizes {
    writePNG(size: px, to: outDir.appendingPathComponent("\(name).png"))
}
print(outDir.path)
