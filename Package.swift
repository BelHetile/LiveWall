// swift-tools-version: 6.4
import PackageDescription

let package = Package(
    name: "LiveWall",
    platforms: [
        .macOS(.v15)
    ],
    products: [
        .library(name: "LiveWallCore", targets: ["LiveWallCore"]),
        .library(name: "LiveWallEngine", targets: ["LiveWallEngine"]),
        .library(name: "LiveWallMedia", targets: ["LiveWallMedia"]),
        .library(name: "LiveWallTools", targets: ["LiveWallTools"]),
        .library(name: "LiveWallIPC", targets: ["LiveWallIPC"]),
        .executable(name: "LiveWall", targets: ["LiveWall"]),
        .executable(name: "lwctl", targets: ["lwctl"]),
        .executable(name: "LiveWallPlaybackProbe", targets: ["LiveWallPlaybackProbe"]),
        .executable(name: "LiveWallStillProbe", targets: ["LiveWallStillProbe"])
    ],
    targets: [
        .target(name: "LiveWallCore"),
        .target(name: "LiveWallEngine", dependencies: ["LiveWallCore"]),
        .target(name: "LiveWallMedia"),
        .target(name: "LiveWallTools"),
        .target(name: "LiveWallIPC"),
        .executableTarget(
            name: "LiveWall",
            dependencies: ["LiveWallCore", "LiveWallEngine", "LiveWallMedia", "LiveWallTools", "LiveWallIPC"]
        ),
        .executableTarget(name: "lwctl", dependencies: ["LiveWallIPC"]),
        .executableTarget(name: "LiveWallPlaybackProbe", dependencies: ["LiveWallEngine"]),
        .executableTarget(name: "LiveWallStillProbe", dependencies: ["LiveWallEngine", "LiveWallCore"]),
        .testTarget(name: "LiveWallCoreTests", dependencies: ["LiveWallCore"]),
        .testTarget(name: "LiveWallEngineTests", dependencies: ["LiveWallEngine", "LiveWallCore"]),
        .testTarget(name: "LiveWallMediaTests", dependencies: ["LiveWallMedia"]),
        .testTarget(name: "LiveWallToolsTests", dependencies: ["LiveWallTools"]),
        .testTarget(name: "LiveWallIPTests", dependencies: ["LiveWallIPC"])
    ],
    swiftLanguageModes: [.v6]
)
