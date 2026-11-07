// Parakeet emits SentencePiece tokens with per-token times. The app's timed
// path (`asr_engine::TimedTranscript`) wants words and segments, so the helper
// merges them here: a token that starts with the SentencePiece word marker
// (U+2581) or a space opens a new word; a segment ends after a word that ends
// a sentence, or before a pause longer than `segmentGapSeconds`.
import Foundation

public struct TimedToken: Equatable {
    public var text: String
    public var start: Double
    public var end: Double

    public init(text: String, start: Double, end: Double) {
        self.text = text
        self.start = start
        self.end = end
    }
}

/// A pause this long splits a segment even mid-sentence.
public let segmentGapSeconds = 0.8

private let wordMarker: Character = "\u{2581}"

public func mergeTokensToWords(_ tokens: [TimedToken]) -> [WireSpan] {
    var words: [WireSpan] = []
    for token in tokens {
        let opensWord = token.text.first == wordMarker || token.text.first == " "
        let piece = token.text
            .replacingOccurrences(of: String(wordMarker), with: " ")
            .trimmingCharacters(in: .whitespaces)
        if piece.isEmpty {
            continue
        }
        if opensWord || words.isEmpty {
            words.append(WireSpan(startSeconds: token.start, endSeconds: token.end, text: piece))
        } else {
            words[words.count - 1].text += piece
            words[words.count - 1].endSeconds = max(words[words.count - 1].endSeconds, token.end)
        }
    }
    return words
}

public func groupWordsToSegments(_ words: [WireSpan], gap: Double = segmentGapSeconds) -> [WireSpan] {
    var segments: [WireSpan] = []
    var current: WireSpan?
    for word in words {
        if var open = current {
            if word.startSeconds - open.endSeconds > gap {
                segments.append(open)
                current = word
            } else {
                open.text += " " + word.text
                open.endSeconds = max(open.endSeconds, word.endSeconds)
                current = open
            }
        } else {
            current = word
        }
        if let open = current, let last = open.text.last, ".?!".contains(last) {
            segments.append(open)
            current = nil
        }
    }
    if let open = current {
        segments.append(open)
    }
    return segments
}

/// The decode result in the protocol's shape. `timed == false` answers kind
/// "none" with both lists empty, exactly like `yap-asr`.
public func buildTimed(text: String, tokens: [TimedToken]?, timed: Bool) -> WireTimed {
    let clean = text.trimmingCharacters(in: .whitespacesAndNewlines)
    guard timed, let tokens, !tokens.isEmpty else {
        return WireTimed(text: clean, kind: "none", segments: [], words: [])
    }
    let words = mergeTokensToWords(tokens)
    if words.isEmpty {
        return WireTimed(text: clean, kind: "none", segments: [], words: [])
    }
    return WireTimed(text: clean, kind: "word", segments: groupWordsToSegments(words), words: words)
}
