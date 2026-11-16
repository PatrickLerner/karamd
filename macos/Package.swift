// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "KaramdBarCore",
    platforms: [.macOS(.v14)],
    products: [.library(name: "KaramdBarCore", targets: ["KaramdBarCore"])],
    targets: [
        .target(name: "KaramdBarCore"),
        .testTarget(name: "KaramdBarCoreTests", dependencies: ["KaramdBarCore"], exclude: ["Fixtures"]),
    ]
)
