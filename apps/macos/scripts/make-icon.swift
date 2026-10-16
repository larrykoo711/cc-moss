// Render docs/assets/icon.svg into an .iconset folder: every size macOS
// asks for, with the artwork on Apple's icon grid (an 824 pt body on a
// 1024 pt canvas), so it sits next to other apps at the same size.
//
//   swift make-icon.swift <icon.svg> <out.iconset>
import AppKit

let args = CommandLine.arguments
guard args.count == 3, let svg = NSImage(contentsOfFile: args[1]) else {
    FileHandle.standardError.write("usage: make-icon.swift <icon.svg> <out.iconset>\n".data(using: .utf8)!)
    exit(2)
}
let out = URL(fileURLWithPath: args[2])
try FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)

// icon.svg draws its rounded square 480 wide in a 512 viewBox.
let bodyShare = 824.0 / 1024.0
let artShare = bodyShare * 512.0 / 480.0

func png(_ pixels: Int) -> Data {
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: pixels, pixelsHigh: pixels,
        bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    let side = Double(pixels) * artShare
    let origin = (Double(pixels) - side) / 2
    svg.draw(in: NSRect(x: origin, y: origin, width: side, height: side))
    NSGraphicsContext.restoreGraphicsState()
    return rep.representation(using: .png, properties: [:])!
}

for points in [16, 32, 128, 256, 512] {
    try png(points).write(to: out.appendingPathComponent("icon_\(points)x\(points).png"))
    try png(points * 2).write(to: out.appendingPathComponent("icon_\(points)x\(points)@2x.png"))
}
