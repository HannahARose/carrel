// swift-tools-version: 6.0
// CarrelProviders: Carrel providers built on Apple's frameworks (docs/providers.md).
// `carrel-provider-apple` is the program Carrel runs; the CarrelAppleProviders library holds
// the providers themselves, so the Mac app can use them in-process too.

import PackageDescription

let package = Package(
    name: "CarrelProviders",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "CarrelAppleProviders", targets: ["CarrelAppleProviders"]),
        .executable(name: "carrel-provider-apple", targets: ["carrel-provider-apple"]),
    ],
    targets: [
        .target(name: "CarrelAppleProviders"),
        .executableTarget(
            name: "carrel-provider-apple",
            dependencies: ["CarrelAppleProviders"]
        ),
        .testTarget(
            name: "CarrelAppleProvidersTests",
            dependencies: ["CarrelAppleProviders"]
        ),
    ]
)
