// Renders one of mdview's generated pages in a WKWebView and writes a PNG.
//
// Why this exists: the app's entire UI is HTML in a web view, and its layout
// bugs are the kind that are invisible in the markup and obvious on screen --
// a control painted under a sticky bar, a selected state whose fill matches
// its background, a dropdown clipped by an ancestor's `overflow`. Reading the
// stylesheet catches some of that; looking at the result catches the rest.
//
// It renders through the same WebKit the app embeds, so what you see here is
// what the app draws, minus the window chrome and the AppKit message bridge.
// Anything driven by that bridge (live reload, persistence, the native menu)
// still has to be checked in the running app.
//
// Usage:
//   shot <page.html> <out.png> [width] [height] [javascript]
//
// The JavaScript runs after the load finishes and before the snapshot, which
// is how you reach states the page does not start in -- opening the sidebar,
// opening the theme menu, dispatching a hover. Prefer `make shot`, which
// generates the page for a markdown file and passes sensible defaults.

import Cocoa
import WebKit

let args = CommandLine.arguments
guard args.count >= 3 else {
    FileHandle.standardError.write(
        "usage: shot <page.html> <out.png> [width] [height] [javascript]\n"
            .data(using: .utf8)!)
    exit(2)
}

let src = URL(fileURLWithPath: args[1])
let out = URL(fileURLWithPath: args[2])
let width = args.count > 3 ? Double(args[3]) ?? 900 : 900
let height = args.count > 4 ? Double(args[4]) ?? 700 : 700
let script = args.count > 5 ? args[5] : ""

guard FileManager.default.fileExists(atPath: src.path) else {
    FileHandle.standardError.write("no such page: \(src.path)\n".data(using: .utf8)!)
    exit(2)
}

let app = NSApplication.shared
// .accessory keeps the snapshot from stealing focus from whatever you are doing.
app.setActivationPolicy(.accessory)

final class Snapshotter: NSObject, WKNavigationDelegate {
    var out: URL!
    var script: String = ""

    func webView(_ view: WKWebView, didFinish navigation: WKNavigation!) {
        view.evaluateJavaScript(script.isEmpty ? "0" : script) { _, error in
            if let error = error {
                FileHandle.standardError.write(
                    "javascript failed: \(error)\n".data(using: .utf8)!)
                exit(1)
            }
            // The page bundles KaTeX and Mermaid and renders them on load; give
            // that a beat to settle or diagrams land in the shot half-drawn.
            DispatchQueue.main.asyncAfter(deadline: .now() + 1.2) {
                view.takeSnapshot(with: WKSnapshotConfiguration()) { image, error in
                    guard let image = image,
                        let tiff = image.tiffRepresentation,
                        let rep = NSBitmapImageRep(data: tiff),
                        let png = rep.representation(using: .png, properties: [:])
                    else {
                        FileHandle.standardError.write(
                            "snapshot failed: \(String(describing: error))\n".data(using: .utf8)!)
                        exit(1)
                    }
                    do { try png.write(to: self.out) } catch {
                        FileHandle.standardError.write(
                            "could not write \(self.out.path): \(error)\n".data(using: .utf8)!)
                        exit(1)
                    }
                    print(self.out.path)
                    exit(0)
                }
            }
        }
    }

    func webView(
        _ view: WKWebView, didFail navigation: WKNavigation!, withError error: Error
    ) {
        FileHandle.standardError.write("load failed: \(error)\n".data(using: .utf8)!)
        exit(1)
    }
}

let delegate = Snapshotter()
delegate.out = out
delegate.script = script

let view = WKWebView(frame: NSRect(x: 0, y: 0, width: width, height: height))
view.navigationDelegate = delegate

// The view must be in a window and on screen, or WebKit never paints and the
// snapshot comes back blank.
let window = NSWindow(
    contentRect: NSRect(x: 0, y: 0, width: width, height: height),
    styleMask: [.titled], backing: .buffered, defer: false)
window.contentView = view
window.orderFrontRegardless()

view.loadFileURL(src, allowingReadAccessTo: src.deletingLastPathComponent())

// Do not hang forever if a page never finishes loading.
DispatchQueue.main.asyncAfter(deadline: .now() + 30) {
    FileHandle.standardError.write("timed out waiting for the page\n".data(using: .utf8)!)
    exit(1)
}

app.run()
