import XCTest
@testable import YapEngineANECore

final class WireTests: XCTestCase {
    func testReadyLineMatchesTheRustShape() throws {
        let line = try encodeLine(AsrReady(pid: 7))
        XCTAssertEqual(line, #"{"pid":7,"type":"ready","version":"0.1.0"}"#)
    }

    func testRequestsFromTheAppParse() throws {
        let load = try JSONDecoder().decode(
            AsrRequest.self, from: Data(#"{"id":1,"kind":"load","model_path":"/m/parakeet-tdt-0.6b-v3-coreml"}"#.utf8))
        XCTAssertEqual(load.kind, Kind.load)
        XCTAssertEqual(load.modelPath, "/m/parakeet-tdt-0.6b-v3-coreml")
        XCTAssertFalse(load.timed, "timed is serde(default) on the Rust side")

        let cancel = try JSONDecoder().decode(
            AsrRequest.self, from: Data(#"{"id":3,"kind":"cancel","timed":false}"#.utf8))
        XCTAssertEqual(cancel, AsrRequest(id: 3, kind: Kind.cancel))

        let tr = try JSONDecoder().decode(
            AsrRequest.self,
            from: Data(#"{"id":2,"kind":"transcribe","samples_path":"/t/x.f32","language":"en","timed":true}"#.utf8))
        XCTAssertEqual(tr.samplesPath, "/t/x.f32")
        XCTAssertEqual(tr.language, "en")
        XCTAssertNil(tr.biasPrompt)
        XCTAssertTrue(tr.timed)
    }

    func testResponsesOmitNilFieldsAndUseSnakeCase() throws {
        XCTAssertEqual(try encodeLine(AsrResponse(id: 4, ok: true, freed: 1)), #"{"freed":1,"id":4,"ok":true}"#)
        XCTAssertEqual(try encodeLine(AsrResponse.fail(9, "nope")), #"{"error":"nope","id":9,"ok":false}"#)
        let caps = try encodeLine(AsrResponse(id: 1, ok: true, caps: .parakeetAne()))
        for key in ["\"native_sample_rate\":16000", "\"max_timestamp_kind\":\"word\"", "\"max_audio_ms\":0",
                    "\"supports_language_detect\":false", "\"supports_streaming\":false", "\"backend\":\"ane\""] {
            XCTAssertTrue(caps.contains(key), "\(key) missing from \(caps)")
        }
        let timed = try encodeLine(WireSpan(startSeconds: 0.5, endSeconds: 1.25, text: "hi"))
        XCTAssertEqual(timed, #"{"end_seconds":1.25,"start_seconds":0.5,"text":"hi"}"#)
    }

    func testSamplesRoundTripAndRejectTruncation() throws {
        let values: [Float] = [0, -1, 0.5, 1.25]
        var data = Data()
        for v in values {
            withUnsafeBytes(of: v.bitPattern.littleEndian) { data.append(contentsOf: $0) }
        }
        XCTAssertEqual(try samplesFromLeBytes(data), values)
        XCTAssertThrowsError(try samplesFromLeBytes(data.prefix(5)))
    }
}

final class WordTests: XCTestCase {
    func testTokensMergeIntoWordsOnTheMarker() {
        let tokens = [
            TimedToken(text: "\u{2581}hel", start: 0.0, end: 0.2),
            TimedToken(text: "lo", start: 0.2, end: 0.4),
            TimedToken(text: "\u{2581}world.", start: 0.5, end: 0.9),
            TimedToken(text: " next", start: 2.0, end: 2.3),
        ]
        let words = mergeTokensToWords(tokens)
        XCTAssertEqual(words.map(\.text), ["hello", "world.", "next"])
        XCTAssertEqual(words[0].startSeconds, 0.0)
        XCTAssertEqual(words[0].endSeconds, 0.4)
        let segments = groupWordsToSegments(words)
        XCTAssertEqual(segments.map(\.text), ["hello world.", "next"])
    }

    func testLongPauseSplitsASegmentMidSentence() {
        let words = [
            WireSpan(startSeconds: 0, endSeconds: 0.3, text: "one"),
            WireSpan(startSeconds: 1.5, endSeconds: 1.8, text: "two"),
        ]
        XCTAssertEqual(groupWordsToSegments(words).count, 2)
    }

    func testUntimedDecodeAnswersNoneWithEmptyLists() {
        let t = buildTimed(text: " hi ", tokens: [TimedToken(text: "\u{2581}hi", start: 0, end: 1)], timed: false)
        XCTAssertEqual(t, WireTimed(text: "hi", kind: "none", segments: [], words: []))
        XCTAssertEqual(buildTimed(text: "hi", tokens: nil, timed: true).kind, "none")
        XCTAssertEqual(buildTimed(text: "hi", tokens: [TimedToken(text: "\u{2581}hi", start: 0, end: 1)], timed: true).kind, "word")
    }
}
