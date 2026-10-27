// Draws desktop/core/assets/camera-off.jpg: what the macOS camera stream (for OBS) shows while
// the phone camera is off, like the Linux placeholder ffmpeg draws.
//
//     swiftc -O scripts/make-camera-placeholder.swift -o /tmp/draw && /tmp/draw desktop/core/assets/camera-off.jpg

import CoreGraphics
import CoreText
import Foundation
import ImageIO
import UniformTypeIdentifiers
let w = 1280, h = 720
guard let ctx = CGContext(data: nil, width: w, height: h, bitsPerComponent: 8, bytesPerRow: 0,
                          space: CGColorSpace(name: CGColorSpace.sRGB)!, bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { fatalError("ctx") }
ctx.setFillColor(CGColor(srgbRed: 0x11/255.0, green: 0x11/255.0, blue: 0x11/255.0, alpha: 1))
ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
let font = CTFontCreateUIFontForLanguage(.system, 33, nil)!
let attrs: [NSAttributedString.Key: Any] = [NSAttributedString.Key(kCTFontAttributeName as String): font,
    NSAttributedString.Key(kCTForegroundColorAttributeName as String): CGColor(gray: 1, alpha: 0.55)]
let line = CTLineCreateWithAttributedString(NSAttributedString(string: "Dotlink · phone camera off", attributes: attrs))
let b = CTLineGetBoundsWithOptions(line, .useGlyphPathBounds)
ctx.textPosition = CGPoint(x: (CGFloat(w) - b.width) / 2 - b.minX, y: (CGFloat(h) - b.height) / 2 - b.minY)
CTLineDraw(line, ctx)
let image = ctx.makeImage()!
let url = URL(fileURLWithPath: CommandLine.arguments[1]) as CFURL
let dest = CGImageDestinationCreateWithURL(url, UTType.jpeg.identifier as CFString, 1, nil)!
CGImageDestinationAddImage(dest, image, [kCGImageDestinationLossyCompressionQuality: 0.8] as CFDictionary)
if !CGImageDestinationFinalize(dest) { fatalError("write") }
