import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

// usage: imgdiff <expected.png> <actual.png> <diff.png> — prints the share of pixels that differ
// (a channel off by more than 16), writes those pixels in red over a dimmed copy. macOS sizes a
// window a few pixels differently from one launch to the next, so images up to 8 px apart are
// compared where they overlap, from the top left; exit 2 when they are further apart.
func pixels(_ path: String) -> (data: [UInt8], width: Int, height: Int)? {
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else { return nil }
    let (width, height) = (image.width, image.height)
    var data = [UInt8](repeating: 0, count: width * height * 4)
    let context = CGContext(data: &data, width: width, height: height, bitsPerComponent: 8,
                            bytesPerRow: width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
    context.draw(image, in: CGRect(x: 0, y: 0, width: width, height: height))
    return (data, width, height)
}
let args = CommandLine.arguments
guard let expected = pixels(args[1]), var actual = pixels(args[2]) else { exit(1) }
guard abs(expected.width - actual.width) <= 8, abs(expected.height - actual.height) <= 8 else {
    print("size \(expected.width)x\(expected.height) != \(actual.width)x\(actual.height)")
    exit(2)
}
let (width, height) = (min(expected.width, actual.width), min(expected.height, actual.height))
var changed = 0
for y in 0..<height {
  for x in 0..<width {
    let i = (y * actual.width + x) * 4, j = (y * expected.width + x) * 4
    let off = (0..<3).contains { abs(Int(expected.data[j + $0]) - Int(actual.data[i + $0])) > 16 }
    if off {
        changed += 1
        (actual.data[i], actual.data[i + 1], actual.data[i + 2]) = (255, 0, 0)
    } else {
        for c in 0..<3 { actual.data[i + c] /= 3 }
    }
  }
}
let context = CGContext(data: &actual.data, width: actual.width, height: actual.height, bitsPerComponent: 8,
                        bytesPerRow: actual.width * 4, space: CGColorSpace(name: CGColorSpace.sRGB)!,
                        bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
let destination = CGImageDestinationCreateWithURL(URL(fileURLWithPath: args[3]) as CFURL,
                                                  UTType.png.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(destination, context.makeImage()!, nil)
CGImageDestinationFinalize(destination)
print(String(format: "%.5f", Double(changed) / Double(width * height)))
