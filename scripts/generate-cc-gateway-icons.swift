#!/usr/bin/env swift

import AppKit
import Foundation

private enum IconError: Error, CustomStringConvertible {
    case usage
    case unreadableSource(String)
    case bitmapCreation(Int)
    case pngEncoding(Int)

    var description: String {
        switch self {
        case .usage:
            return "usage: generate-cc-gateway-icons.swift <source.png> [icons-directory]"
        case let .unreadableSource(path):
            return "cannot read source image: \(path)"
        case let .bitmapCreation(size):
            return "cannot create \(size)x\(size) bitmap"
        case let .pngEncoding(size):
            return "cannot encode \(size)x\(size) PNG"
        }
    }
}

private extension Data {
    mutating func appendLittleEndian<T: FixedWidthInteger>(_ value: T) {
        var encoded = value.littleEndian
        Swift.withUnsafeBytes(of: &encoded) { append(contentsOf: $0) }
    }

    mutating func appendBigEndian<T: FixedWidthInteger>(_ value: T) {
        var encoded = value.bigEndian
        Swift.withUnsafeBytes(of: &encoded) { append(contentsOf: $0) }
    }
}

private func pngData(
    size: Int,
    draw: (NSBitmapImageRep, NSGraphicsContext) -> Void
) throws -> Data {
    guard let bitmap = NSBitmapImageRep(
        bitmapDataPlanes: nil,
        pixelsWide: size,
        pixelsHigh: size,
        bitsPerSample: 8,
        samplesPerPixel: 4,
        hasAlpha: true,
        isPlanar: false,
        colorSpaceName: .deviceRGB,
        bytesPerRow: 0,
        bitsPerPixel: 0
    ) else {
        throw IconError.bitmapCreation(size)
    }

    bitmap.size = NSSize(width: size, height: size)
    guard let context = NSGraphicsContext(bitmapImageRep: bitmap) else {
        throw IconError.bitmapCreation(size)
    }

    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = context
    context.cgContext.clear(CGRect(x: 0, y: 0, width: size, height: size))
    context.imageInterpolation = .high
    draw(bitmap, context)
    context.flushGraphics()
    NSGraphicsContext.restoreGraphicsState()

    guard let data = bitmap.representation(using: .png, properties: [:]) else {
        throw IconError.pngEncoding(size)
    }
    return data
}

private func makeAppIcon(source: NSImage, size: Int) throws -> Data {
    try pngData(size: size) { _, _ in
        let dimension = CGFloat(size)
        let inset = dimension * (18.0 / 1024.0)
        let cornerRadius = dimension * (188.0 / 1024.0)
        let clipRect = NSRect(
            x: inset,
            y: inset,
            width: dimension - (2 * inset),
            height: dimension - (2 * inset)
        )
        NSBezierPath(
            roundedRect: clipRect,
            xRadius: cornerRadius,
            yRadius: cornerRadius
        ).addClip()
        source.draw(
            in: NSRect(x: 0, y: 0, width: dimension, height: dimension),
            from: .zero,
            operation: .copy,
            fraction: 1,
            respectFlipped: true,
            hints: [.interpolation: NSImageInterpolation.high]
        )
    }
}

private func makeTrayTemplate(source: NSImage, size: Int) throws -> Data {
    try pngData(size: size) { bitmap, _ in
        let dimension = CGFloat(size)
        let sourceSize = source.size
        let crop = NSRect(
            x: sourceSize.width * 0.095,
            y: sourceSize.height * 0.235,
            width: sourceSize.width * 0.81,
            height: sourceSize.height * 0.54
        )
        let targetWidth = dimension * 0.90
        let targetHeight = targetWidth * crop.height / crop.width
        source.draw(
            in: NSRect(
                x: (dimension - targetWidth) / 2,
                y: (dimension - targetHeight) / 2,
                width: targetWidth,
                height: targetHeight
            ),
            from: crop,
            operation: .copy,
            fraction: 1,
            respectFlipped: true,
            hints: [.interpolation: NSImageInterpolation.high]
        )

        for y in 0..<size {
            for x in 0..<size {
                guard let sampled = bitmap.colorAt(x: x, y: y)?.usingColorSpace(.deviceRGB) else {
                    continue
                }
                let signal = max(sampled.redComponent, sampled.greenComponent, sampled.blueComponent)
                let alpha = max(0, min(1, (signal - 0.16) / 0.42))
                bitmap.setColor(
                    NSColor(deviceWhite: 0, alpha: alpha),
                    atX: x,
                    y: y
                )
            }
        }
    }
}

private func write(_ data: Data, to url: URL) throws {
    try FileManager.default.createDirectory(
        at: url.deletingLastPathComponent(),
        withIntermediateDirectories: true
    )
    try data.write(to: url, options: .atomic)
}

private func makeIcns(source: NSImage, output: URL) throws {
    let entries: [(String, Int)] = [
        ("icp4", 16),
        ("icp5", 32),
        ("icp6", 64),
        ("ic07", 128),
        ("ic08", 256),
        ("ic09", 512),
        ("ic10", 1024),
    ]
    let images = try entries.map { try makeAppIcon(source: source, size: $0.1) }
    let totalSize = 8 + zip(entries, images).reduce(0) { partial, pair in
        partial + 8 + pair.1.count
    }
    var result = Data("icns".utf8)
    result.appendBigEndian(UInt32(totalSize))
    for (entry, image) in zip(entries, images) {
        result.append(Data(entry.0.utf8))
        result.appendBigEndian(UInt32(8 + image.count))
        result.append(image)
    }
    try write(result, to: output)
}

private func makeIco(source: NSImage, output: URL) throws {
    let sizes = [16, 24, 32, 48, 64, 128, 256]
    let images = try sizes.map { try makeAppIcon(source: source, size: $0) }
    let directorySize = 6 + (16 * images.count)
    var offset = directorySize
    var result = Data()
    result.appendLittleEndian(UInt16(0))
    result.appendLittleEndian(UInt16(1))
    result.appendLittleEndian(UInt16(images.count))

    for (index, image) in images.enumerated() {
        let size = sizes[index]
        result.append(UInt8(size == 256 ? 0 : size))
        result.append(UInt8(size == 256 ? 0 : size))
        result.append(UInt8(0))
        result.append(UInt8(0))
        result.appendLittleEndian(UInt16(1))
        result.appendLittleEndian(UInt16(32))
        result.appendLittleEndian(UInt32(image.count))
        result.appendLittleEndian(UInt32(offset))
        offset += image.count
    }
    for image in images {
        result.append(image)
    }
    try write(result, to: output)
}

do {
    guard CommandLine.arguments.count >= 2 else { throw IconError.usage }
    let sourcePath = CommandLine.arguments[1]
    let outputPath = CommandLine.arguments.count >= 3
        ? CommandLine.arguments[2]
        : "src-tauri/icons"
    guard let source = NSImage(contentsOfFile: sourcePath) else {
        throw IconError.unreadableSource(sourcePath)
    }

    let output = URL(fileURLWithPath: outputPath, isDirectory: true)
    try FileManager.default.createDirectory(at: output, withIntermediateDirectories: true)

    let pngTargets: [(String, Int)] = [
        ("icon-source.png", 1024),
        ("icon.png", 512),
        ("32x32.png", 32),
        ("64x64.png", 64),
        ("128x128.png", 128),
        ("128x128@2x.png", 256),
    ]
    for (name, size) in pngTargets {
        try write(try makeAppIcon(source: source, size: size), to: output.appendingPathComponent(name))
    }

    try makeIcns(source: source, output: output.appendingPathComponent("icon.icns"))
    try makeIco(source: source, output: output.appendingPathComponent("icon.ico"))

    let trayDirectory = output.appendingPathComponent("tray/macos", isDirectory: true)
    for (name, size) in [
        ("statusTemplate.png", 24),
        ("statusTemplate@2x.png", 48),
        ("statusbar_template_3x.png", 72),
    ] {
        try write(try makeTrayTemplate(source: source, size: size), to: trayDirectory.appendingPathComponent(name))
    }

    print("Generated CC Gateway icon suite in \(output.path)")
} catch {
    fputs("error: \(error)\n", stderr)
    exit(1)
}
