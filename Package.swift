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
        .executable(name: "lwctl", targets: ["lwctl"])
    ],
    targets: [
        .target(name: "LiveWallCore"),
        .target(name: "LiveWallEngine"),
        .target(name: "LiveWallMedia"),
        .target(name: "LiveWallTools"),
        .target(name: "LiveWallIPC"),
        .executableTarget(
            name: "LiveWall",
            dependencies: ["LiveWallCore", "LiveWallEngine", "LiveWallMedia", "LiveWallTools", "LiveWallIPC"]
        ),
        .executableTarget(name: "lwctl", dependencies: ["LiveWallIPC"]),
        .testTarget(name: "LiveWallCoreTests", dependencies: ["LiveWallCore"]),
        .testTarget(name: "LiveWallEngineTests", dependencies: ["LiveWallEngine"]),
        .testTarget(name: "LiveWallMediaTests", dependencies: ["LiveWallMedia"]),
        .testTarget(name: "LiveWallToolsTests", dependencies: ["LiveWallTools"]),
        .testTarget(name: "LiveWallIPTests", dependencies: ["LiveWallIPC"])
    ],
    swiftLanguageModes: [.v6]
)
