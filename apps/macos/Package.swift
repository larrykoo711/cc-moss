// swift-tools-version:5.10
import PackageDescription

let package = Package(
    name: "cc-moss-app",
    platforms: [.macOS(.v14)],
    products: [
        .executable(name: "CCMoss", targets: ["CCMoss"]),
    ],
    targets: [
        // Everything that can be tested without a screen: talking to the
        // cc-moss CLI, decoding, grouping, wording.
        .target(name: "MossKit"),
        // The menubar app itself: SwiftUI views on top of MossKit.
        .executableTarget(name: "CCMoss", dependencies: ["MossKit"]),
        // Swift Testing, not XCTest: the Command Line Tools ship only the former.
        .testTarget(name: "MossKitTests", dependencies: ["MossKit"]),
    ]
)
