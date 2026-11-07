// swift-tools-version: 6.0
//
// yap-engine-ane — SP-10 spike (yap-1106-sp-10-ane-engine-spike).
//
// A second speech helper that runs Parakeet on the Apple Neural Engine through
// Core ML, behind the SAME newline-delimited JSON protocol as `yap-asr`
// (desktop/src-tauri/src/asr_protocol.rs). The app never picks it by default:
// `asr_engine::EngineKind::DEFAULT` stays `Gguf`; this binary is reached only
// with YAP_ASR_ENGINE=ane (the bench) or, after the SP-10 decision rule, as
// the governor's "constrained" engine.
//
// FluidAudio is Apache-2.0 and pinned EXACTLY (no range): a new release is a
// reviewed bump, never a silent resolve. The Parakeet weights it loads are
// CC-BY-4.0 (NVIDIA); both licenses are recorded in
// desktop/src-tauri/src/catalog.json under `ane_engines`.
import PackageDescription

let package = Package(
    name: "yap-engine-ane",
    platforms: [
        // FluidAudio's floor. The app itself still runs on 12.0; the helper is
        // only spawned when it is selected, and it refuses older systems with
        // one sentence on its error line (see main.swift).
        .macOS(.v14)
    ],
    products: [
        .executable(name: "yap-engine-ane", targets: ["yap-engine-ane"])
    ],
    dependencies: [
        .package(url: "https://github.com/FluidInference/FluidAudio.git", exact: "0.18.2")
    ],
    targets: [
        // Pure protocol + text logic: no FluidAudio, no Core ML, so its tests
        // run anywhere `swift test` does.
        .target(name: "YapEngineANECore", path: "Sources/YapEngineANECore"),
        .executableTarget(
            name: "yap-engine-ane",
            dependencies: [
                "YapEngineANECore",
                .product(name: "FluidAudio", package: "FluidAudio"),
            ],
            path: "Sources/yap-engine-ane"
        ),
        .testTarget(
            name: "YapEngineANECoreTests",
            dependencies: ["YapEngineANECore"],
            path: "Tests/YapEngineANECoreTests"
        ),
    ],
    // Swift 5 language mode: the helper is one stdin thread + one decode task,
    // guarded by a lock; strict Swift 6 sendability checking adds nothing here
    // but compile risk on the CI toolchain.
    swiftLanguageModes: [.v5]
)
