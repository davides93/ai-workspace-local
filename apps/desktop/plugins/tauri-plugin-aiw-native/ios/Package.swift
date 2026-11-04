// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "tauri-plugin-aiw-native",
    platforms: [.iOS(.v15), .macOS(.v13)],
    products: [
        .library(name: "tauri-plugin-aiw-native", type: .static, targets: ["AiwNativePlugin"]),
    ],
    dependencies: [
        .package(name: "Tauri", path: "../.tauri/tauri-api"),
    ],
    targets: [
        .target(name: "AiwNativePlugin", dependencies: [.byName(name: "Tauri")], path: "Sources/AiwNativePlugin"),
    ]
)
