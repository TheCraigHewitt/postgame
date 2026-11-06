// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "PGAudio",
    platforms: [.macOS("14.4")],
    products: [
        .library(name: "PGAudio", type: .static, targets: ["PGAudio"])
    ],
    targets: [
        .target(name: "PGAudio", path: "Sources/PGAudio")
    ]
)
