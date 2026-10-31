// swift-tools-version: 6.2
// CarrelMLX: Carrel's provider for open models through MLX on Apple silicon (docs/design/mlx.md).
// `carrel-provider-mlx` is the program Carrel runs; the CarrelMLXProvider library holds the
// provider itself. It's a package of its own because MLX's Metal shaders can only be built with
// `xcodebuild`, not `swift build`, and `carrel-provider-apple` shouldn't need that.
//
// Build: xcodebuild -scheme carrel-provider-mlx -destination 'platform=macOS' \
//            -derivedDataPath .build/xcode -configuration Release build

import PackageDescription

let package = Package(
    name: "CarrelMLX",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "CarrelMLXProvider", targets: ["CarrelMLXProvider"]),
        .executable(name: "carrel-provider-mlx", targets: ["carrel-provider-mlx"]),
    ],
    dependencies: [
        .package(path: "../CarrelProviders"),
        // Pinned exactly; Package.resolved pins what they depend on.
        .package(url: "https://github.com/ml-explore/mlx-swift-lm", exact: "3.32.3"),
        .package(url: "https://github.com/huggingface/swift-transformers", exact: "1.3.5"),
    ],
    targets: [
        .target(
            name: "CarrelMLXProvider",
            dependencies: [
                .product(name: "CarrelAppleProviders", package: "CarrelProviders"),
                .product(name: "MLXLLM", package: "mlx-swift-lm"),
                .product(name: "MLXLMCommon", package: "mlx-swift-lm"),
                .product(name: "MLXGuidedGeneration", package: "mlx-swift-lm"),
                .product(name: "MLXEmbedders", package: "mlx-swift-lm"),
                // Only its tokenizers, loaded from a local folder; Carrel never lets it download.
                .product(name: "Tokenizers", package: "swift-transformers"),
            ]
        ),
        .executableTarget(
            name: "carrel-provider-mlx",
            dependencies: ["CarrelMLXProvider"]
        ),
        .testTarget(
            name: "CarrelMLXProviderTests",
            dependencies: ["CarrelMLXProvider"]
        ),
    ]
)
