// Wire protocol for the yap-engine-ane helper.
//
// This is a field-for-field mirror of desktop/src-tauri/src/asr_protocol.rs
// (protocol 0.1.0), the file `yap-asr` and the app compile from one source.
// Swift cannot `#[path]`-include a Rust file, so the mirror is pinned instead
// by `asr_engine.rs::ane_wire_mirror_matches_the_rust_protocol` (keys) and
// by WireTests.swift (shapes). stdout carries JSON and nothing else.
import Foundation

public let asrProtocolVersion = "0.1.0"

public enum Kind {
    public static let load = "load"
    public static let transcribe = "transcribe"
    public static let cancel = "cancel"
    public static let shutdown = "shutdown"
}

/// The first stdout line of a healthy helper.
public struct AsrReady: Codable, Equatable {
    public var type: String
    public var version: String
    public var pid: UInt32

    public init(pid: UInt32) {
        self.type = "ready"
        self.version = asrProtocolVersion
        self.pid = pid
    }
}

/// One request line. Fields a kind does not use stay nil.
public struct AsrRequest: Codable, Equatable {
    public var id: UInt64
    public var kind: String
    public var modelPath: String?
    public var samplesPath: String?
    public var language: String?
    public var biasPrompt: String?
    public var timed: Bool

    enum CodingKeys: String, CodingKey {
        case id, kind, language, timed
        case modelPath = "model_path"
        case samplesPath = "samples_path"
        case biasPrompt = "bias_prompt"
    }

    public init(id: UInt64, kind: String, modelPath: String? = nil, samplesPath: String? = nil,
                language: String? = nil, biasPrompt: String? = nil, timed: Bool = false) {
        self.id = id
        self.kind = kind
        self.modelPath = modelPath
        self.samplesPath = samplesPath
        self.language = language
        self.biasPrompt = biasPrompt
        self.timed = timed
    }

    // `timed` is `#[serde(default)]` on the Rust side: absent means false.
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(UInt64.self, forKey: .id)
        kind = try c.decode(String.self, forKey: .kind)
        modelPath = try c.decodeIfPresent(String.self, forKey: .modelPath)
        samplesPath = try c.decodeIfPresent(String.self, forKey: .samplesPath)
        language = try c.decodeIfPresent(String.self, forKey: .language)
        biasPrompt = try c.decodeIfPresent(String.self, forKey: .biasPrompt)
        timed = try c.decodeIfPresent(Bool.self, forKey: .timed) ?? false
    }
}

/// What the loaded model can do. Mirrors `WireCaps`.
public struct WireCaps: Codable, Equatable {
    public var arch: String
    public var backend: String
    public var nativeSampleRate: Int32
    public var languages: [String]
    public var maxTimestampKind: String
    public var maxAudioMs: Int64
    public var supportsLanguageDetect: Bool
    public var supportsStreaming: Bool

    enum CodingKeys: String, CodingKey {
        case arch, backend, languages
        case nativeSampleRate = "native_sample_rate"
        case maxTimestampKind = "max_timestamp_kind"
        case maxAudioMs = "max_audio_ms"
        case supportsLanguageDetect = "supports_language_detect"
        case supportsStreaming = "supports_streaming"
    }

    /// Parakeet TDT v3 on the Neural Engine: 16 kHz, per-token times (merged
    /// to words here), no language detection exposed, no streaming in this
    /// helper (the sliding-window manager chunks long audio internally, so
    /// there is no hard length cap: 0 = "no cap", the same as yap-asr).
    public static func parakeetAne() -> WireCaps {
        WireCaps(
            arch: "parakeet-tdt-coreml",
            backend: "ane",
            nativeSampleRate: 16_000,
            languages: ["en"],
            maxTimestampKind: "word",
            maxAudioMs: 0,
            supportsLanguageDetect: false,
            supportsStreaming: false
        )
    }
}

/// One timed row, seconds from the start of the decoded buffer.
public struct WireSpan: Codable, Equatable {
    public var startSeconds: Double
    public var endSeconds: Double
    public var text: String

    enum CodingKeys: String, CodingKey {
        case text
        case startSeconds = "start_seconds"
        case endSeconds = "end_seconds"
    }

    public init(startSeconds: Double, endSeconds: Double, text: String) {
        self.startSeconds = startSeconds
        self.endSeconds = endSeconds
        self.text = text
    }
}

/// A decode result. A plain (untimed) decode answers "none" with both lists empty.
public struct WireTimed: Codable, Equatable {
    public var text: String
    public var kind: String
    public var segments: [WireSpan]
    public var words: [WireSpan]

    public init(text: String, kind: String, segments: [WireSpan], words: [WireSpan]) {
        self.text = text
        self.kind = kind
        self.segments = segments
        self.words = words
    }
}

/// One response line. `ok == false` carries `error`, one plain sentence.
/// Nil fields are omitted, like `skip_serializing_if = "Option::is_none"`.
public struct AsrResponse: Codable, Equatable {
    public var id: UInt64
    public var ok: Bool
    public var error: String?
    public var caps: WireCaps?
    public var timed: WireTimed?
    public var language: String?
    public var freed: UInt32?

    public init(id: UInt64, ok: Bool, error: String? = nil, caps: WireCaps? = nil,
                timed: WireTimed? = nil, language: String? = nil, freed: UInt32? = nil) {
        self.id = id
        self.ok = ok
        self.error = error
        self.caps = caps
        self.timed = timed
        self.language = language
        self.freed = freed
    }

    public static func fail(_ id: UInt64, _ error: String) -> AsrResponse {
        AsrResponse(id: id, ok: false, error: error)
    }
}

/// Decode a samples file's bytes (raw little-endian f32, 16 kHz mono). A
/// trailing partial sample is an error: it means the file was truncated.
public func samplesFromLeBytes(_ data: Data) throws -> [Float] {
    guard data.count % 4 == 0 else {
        throw WireError.truncated(bytes: data.count)
    }
    var out = [Float](repeating: 0, count: data.count / 4)
    data.withUnsafeBytes { raw in
        for i in 0..<out.count {
            let bits = raw.loadUnaligned(fromByteOffset: i * 4, as: UInt32.self)
            out[i] = Float(bitPattern: UInt32(littleEndian: bits))
        }
    }
    return out
}

public enum WireError: Error, Equatable, CustomStringConvertible {
    case truncated(bytes: Int)

    public var description: String {
        switch self {
        case .truncated(let bytes):
            return "samples file is \(bytes) bytes, not a whole number of samples"
        }
    }
}

/// One JSON line, keys sorted so the output is stable and greppable.
public func encodeLine<T: Encodable>(_ value: T) throws -> String {
    let enc = JSONEncoder()
    enc.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
    let data = try enc.encode(value)
    return String(decoding: data, as: UTF8.self)
}
