// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "vibetrail",
    platforms: [
        .macOS(.v14)
    ],
    products: [
        .library(name: "VibeTrailCore", targets: ["VibeTrailCore"]),
        .executable(name: "vibetrail", targets: ["vibetrail"]),
    ],
    dependencies: [
        .package(url: "https://github.com/apple/swift-argument-parser.git", from: "1.5.0"),
    ],
    targets: [
        .target(
            name: "VibeTrailCore"
        ),
        .executableTarget(
            name: "vibetrail",
            dependencies: [
                "VibeTrailCore",
                .product(name: "ArgumentParser", package: "swift-argument-parser"),
            ]
        ),
        .testTarget(
            name: "VibeTrailCoreTests",
            dependencies: ["VibeTrailCore"],
            resources: [
                .copy("Fixtures")
            ]
        ),
    ]
)
