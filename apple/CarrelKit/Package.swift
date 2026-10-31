// swift-tools-version: 6.0
// CarrelKit: carrel-core for Swift, through the UniFFI bindings in carrel-ffi.
// Run scripts/build-xcframework.sh first; it builds CarrelFFI.xcframework, which this package
// links. The generated Swift bindings in Sources/CarrelKit/Generated are committed.

import PackageDescription

let package = Package(
    name: "CarrelKit",
    platforms: [.macOS(.v15), .iOS(.v18)],
    products: [
        .library(name: "CarrelKit", targets: ["CarrelKit"]),
        .executable(name: "CarrelPoC", targets: ["CarrelPoC"]),
    ],
    targets: [
        .binaryTarget(name: "CarrelFFI", path: "CarrelFFI.xcframework"),
        .target(
            name: "CarrelKit",
            dependencies: ["CarrelFFI"],
            // libgit2, built into carrel-core, needs these system libraries.
            linkerSettings: [.linkedLibrary("z"), .linkedLibrary("iconv")]
        ),
        .executableTarget(name: "CarrelPoC", dependencies: ["CarrelKit"]),
        .testTarget(name: "CarrelKitTests", dependencies: ["CarrelKit"]),
    ]
)
