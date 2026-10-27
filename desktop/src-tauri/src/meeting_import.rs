//! yap-notetaker-onboard — import a meeting someone else transcribed.
//!
//! The Set up Notetaker card's third row. A user who has months of meetings in
//! Otter should not start Yap with an empty Meetings list, so they can hand Yap
//! the files Otter already let them download, and each file becomes a Yap
//! meeting: a `meetings` row with `source = 'imported:otter'`, its turns as
//! `meeting_segments`, searchable through the same `meeting_segments_fts`
//! index every recorded meeting uses (the insert triggers own the index, so an
//! import is searchable by construction, not by a second code path).
//!
//! FILES ONLY. Nothing in this module opens a socket: no Otter API, no Granola
//! API, no OAuth. The frontend reads the file the user picked and hands the
//! TEXT to [`parse`]; this module never even opens a path, which is what keeps
//! "import" from ever growing into "sync".
//!
//! ALL OR NOTHING. A file is parsed completely, in memory, before anything is
//! written. A malformed file returns a plain-language error and writes NO row
//! (see [`ImportError`]); a well-formed one is written by
//! `Database::import_meeting` in ONE transaction, meeting row and segments
//! together, so a crash mid-write cannot leave a meeting with half its words.
//!
//! FORMATS — what is and is not confirmed (recorded again in the PR):
//!   * Otter `.txt` — blocks separated by a blank line, each opening with a
//!     header line `<Speaker name>  <m:ss | h:mm:ss>` (the speaker may be absent
//!     when speaker names were switched off at export). Shape taken from a
//!     third-party guide and Otter's export dialog options; NOT confirmed
//!     against a real Otter account's export in this build.
//!   * Otter `.srt` — standard SubRip cues; Otter prefixes the cue text with
//!     `Speaker name: ` when speaker names are on. Same caveat.
//!   * Otter `.docx` — NOT parsed in this build (it is a zip of XML and the app
//!     carries no zip reader). The card says to export as .txt or .srt.
//!   * Granola — its one official file export is a bulk CSV (Settings →
//!     Profile → Generate CSV, emailed link; title, summary, transcript per
//!     note), described in third-party guides but with NO published column
//!     layout in Granola's own docs, so it is not a stable format to parse.
//!     The card shows Granola as Coming with that reason. [`ImportSource::Granola`]
//!     exists only so the stored `source` spelling is decided in one place.

use crate::meetings::NewMeetingSegment;

/// Which product a file came from. Spelled into `meetings.source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportSource {
    Otter,
    /// Not importable yet — see the module docs. Kept so the `source` string
    /// for a future Granola import is decided here and nowhere else.
    Granola,
}

impl ImportSource {
    pub fn as_source(self) -> &'static str {
        match self {
            ImportSource::Otter => "imported:otter",
            ImportSource::Granola => "imported:granola",
        }
    }
}

/// The file shapes this build can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportFormat {
    OtterTxt,
    OtterSrt,
}

/// Why a file was not imported. Every variant's `Display` is a sentence a user
/// can act on — it is shown verbatim on the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// The extension is not one this build reads (.docx lands here on purpose).
    Unsupported(String),
    /// The file is empty or only whitespace.
    Empty,
    /// The file has the right extension but not the right shape.
    Malformed(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Unsupported(ext) if ext == "docx" => write!(
                f,
                "Word files aren't supported yet. In Otter, export the meeting as .txt or .srt and import that."
            ),
            ImportError::Unsupported(ext) => write!(
                f,
                "Yap can import Otter .txt and .srt files. This one is .{ext}."
            ),
            ImportError::Empty => write!(f, "This file is empty."),
            ImportError::Malformed(why) => write!(
                f,
                "This doesn't look like an Otter export ({why}). Nothing was imported."
            ),
        }
    }
}

/// A fully parsed file, ready for ONE transactional insert.
#[derive(Debug, Clone)]
pub struct ParsedMeeting {
    pub source: ImportSource,
    pub title: String,
    pub segments: Vec<NewMeetingSegment>,
    /// Seconds from the first turn to the end of the last one.
    pub duration_seconds: f64,
    /// Otter `.txt`/`.srt` carry no summary; kept for formats that do.
    pub summary: Option<String>,
}

/// Pick the parser from the file name. Case-insensitive on the extension.
pub fn format_for(file_name: &str) -> Result<ImportFormat, ImportError> {
    let ext = std::path::Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "txt" => Ok(ImportFormat::OtterTxt),
        "srt" => Ok(ImportFormat::OtterSrt),
        other => Err(ImportError::Unsupported(if other.is_empty() {
            "no extension".into()
        } else {
            other.to_string()
        })),
    }
}

/// The meeting title an imported file gets: its name without the extension,
/// trimmed. Falls back to "Imported meeting".
pub fn title_for(file_name: &str) -> String {
    let base = std::path::Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .trim();
    if base.is_empty() {
        "Imported meeting".into()
    } else {
        base.to_string()
    }
}

/// Parse one file's text. Pure: no I/O, no DB. Any `Err` means NOTHING is
/// written for this file.
pub fn parse(file_name: &str, content: &str) -> Result<ParsedMeeting, ImportError> {
    let format = format_for(file_name)?;
    let content = content.trim_start_matches('\u{feff}');
    if content.trim().is_empty() {
        return Err(ImportError::Empty);
    }
    let segments = match format {
        ImportFormat::OtterTxt => parse_otter_txt(content)?,
        ImportFormat::OtterSrt => parse_srt(content)?,
    };
    let first = segments.first().map(|s| s.start_seconds).unwrap_or(0.0);
    let last = segments
        .iter()
        .map(|s| s.end_seconds)
        .fold(0.0_f64, f64::max);
    Ok(ParsedMeeting {
        source: ImportSource::Otter,
        title: title_for(file_name),
        duration_seconds: (last - first).max(0.0),
        segments,
        summary: None,
    })
}

/// `m:ss`, `mm:ss` or `h:mm:ss` → seconds.
fn parse_clock(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let mut total = 0u64;
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || (i > 0 && p.len() > 2) || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let n: u64 = p.parse().ok()?;
        if i > 0 && n >= 60 {
            return None;
        }
        total = total * 60 + n;
    }
    Some(total as f64)
}

/// An Otter `.txt` header line: `Speaker  1:23` or just `1:23`.
fn otter_header(line: &str) -> Option<(Option<String>, f64)> {
    let line = line.trim();
    let (speaker, clock) = match line.rsplit_once(char::is_whitespace) {
        Some((head, tail)) => (head.trim(), tail),
        None => ("", line),
    };
    let at = parse_clock(clock)?;
    let speaker = (!speaker.is_empty()).then(|| speaker.to_string());
    Some((speaker, at))
}

fn with_speaker(speaker: Option<&str>, text: &str) -> String {
    match speaker {
        Some(name) => format!("{name}: {text}"),
        None => text.to_string(),
    }
}

/// Otter `.txt`: blank-line separated blocks, each opening with a header line.
///
/// A file whose first non-empty line is not a header is refused outright —
/// that is the "a text file, but not an Otter export" case, and importing it as
/// one giant untimed turn would be a silently wrong meeting.
fn parse_otter_txt(content: &str) -> Result<Vec<NewMeetingSegment>, ImportError> {
    struct Turn {
        speaker: Option<String>,
        at: f64,
        text: Vec<String>,
    }
    let mut turns: Vec<Turn> = Vec::new();
    let mut expect_header = true;
    for (n, raw) in content.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() {
            expect_header = true;
            continue;
        }
        if expect_header {
            if let Some((speaker, at)) = otter_header(line) {
                turns.push(Turn {
                    speaker,
                    at,
                    text: Vec::new(),
                });
                expect_header = false;
                continue;
            }
            if turns.is_empty() {
                return Err(ImportError::Malformed(format!(
                    "line {} has no speaker timestamp",
                    n + 1
                )));
            }
        }
        // A paragraph break inside one speaker's turn: keep it with that turn.
        expect_header = false;
        turns
            .last_mut()
            .expect("turns is non-empty here")
            .text
            .push(line.to_string());
    }
    if turns.is_empty() {
        return Err(ImportError::Malformed("no speaker turns found".into()));
    }
    let mut segments = Vec::with_capacity(turns.len());
    for i in 0..turns.len() {
        let t = &turns[i];
        let text = t.text.join(" ");
        if text.trim().is_empty() {
            continue;
        }
        if i > 0 && t.at < turns[i - 1].at {
            return Err(ImportError::Malformed(format!(
                "timestamps go backwards at turn {}",
                i + 1
            )));
        }
        // A turn ends where the next one starts; the last turn gets a nominal
        // length from its word count (~2.5 words/s) so its end is not its start.
        let end = turns
            .get(i + 1)
            .map(|next| next.at)
            .unwrap_or_else(|| t.at + (text.split_whitespace().count() as f64 / 2.5).max(1.0));
        segments.push(NewMeetingSegment::new(
            t.at,
            end.max(t.at),
            with_speaker(t.speaker.as_deref(), &text),
        ));
    }
    if segments.is_empty() {
        return Err(ImportError::Malformed("every turn is empty".into()));
    }
    Ok(segments)
}

/// `00:01:02,345` → seconds.
fn parse_srt_time(s: &str) -> Option<f64> {
    let (hms, ms) = s.trim().split_once([',', '.'])?;
    if ms.len() != 3 || !ms.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let parts: Vec<&str> = hms.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let mut total = 0f64;
    for p in &parts {
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        total = total * 60.0 + p.parse::<f64>().ok()?;
    }
    Some(total + ms.parse::<f64>().ok()? / 1000.0)
}

/// `Speaker name: words` → (Some(name), words). Conservative: a "name" longer
/// than 40 chars or containing sentence punctuation is treated as text.
fn split_speaker(text: &str) -> (Option<&str>, &str) {
    if let Some((head, tail)) = text.split_once(": ") {
        let head = head.trim();
        if !head.is_empty()
            && head.len() <= 40
            && !head.contains(['.', '?', '!', ','])
            && !tail.trim().is_empty()
        {
            return (Some(head), tail.trim());
        }
    }
    (None, text)
}

/// SubRip: `index`, `start --> end`, one or more text lines, blank line.
fn parse_srt(content: &str) -> Result<Vec<NewMeetingSegment>, ImportError> {
    let normalized = content.replace("\r\n", "\n");
    let mut segments = Vec::new();
    for (i, block) in normalized
        .split("\n\n")
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .enumerate()
    {
        let mut lines = block.lines();
        let first = lines.next().unwrap_or("").trim();
        // The cue index is optional in the wild; accept a block that opens on
        // its timing line.
        let timing = if first.contains("-->") {
            first
        } else if first.bytes().all(|b| b.is_ascii_digit()) {
            lines.next().unwrap_or("").trim()
        } else {
            return Err(ImportError::Malformed(format!(
                "cue {} does not start with a number",
                i + 1
            )));
        };
        let Some((a, b)) = timing.split_once("-->") else {
            return Err(ImportError::Malformed(format!(
                "cue {} has no start --> end line",
                i + 1
            )));
        };
        let (Some(start), Some(end)) = (parse_srt_time(a), parse_srt_time(b)) else {
            return Err(ImportError::Malformed(format!(
                "cue {} has an unreadable time",
                i + 1
            )));
        };
        if end < start {
            return Err(ImportError::Malformed(format!(
                "cue {} ends before it starts",
                i + 1
            )));
        }
        let text = lines
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() {
            continue;
        }
        let (speaker, words) = split_speaker(&text);
        segments.push(NewMeetingSegment::new(
            start,
            end,
            with_speaker(speaker, words),
        ));
    }
    if segments.is_empty() {
        return Err(ImportError::Malformed("no subtitle cues found".into()));
    }
    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TXT: &str = include_str!("../tests/fixtures/otter/sample.txt");
    const SRT: &str = include_str!("../tests/fixtures/otter/sample.srt");
    const BAD_SRT: &str = include_str!("../tests/fixtures/otter/malformed.srt");
    const BAD_TXT: &str = include_str!("../tests/fixtures/otter/malformed.txt");

    #[test]
    fn meeting_import_otter_txt_fixture_becomes_segments() {
        let m = parse("Pricing sync.txt", TXT).expect("fixture parses");
        assert_eq!(m.source.as_source(), "imported:otter");
        assert_eq!(m.title, "Pricing sync");
        assert_eq!(m.segments.len(), 3);
        assert_eq!(m.segments[0].start_seconds, 0.0);
        assert_eq!(m.segments[0].end_seconds, 7.0);
        assert_eq!(
            m.segments[0].text,
            "Wilson Guenther: Thanks for making time. Let us walk the pricing page first."
        );
        // A two-line turn stays ONE segment.
        assert_eq!(
            m.segments[1].text,
            "Jeisil Hernandez: Sure. The annual plan needs a clearer discount line. And the trial copy is too long."
        );
        // h:mm:ss headers.
        assert_eq!(m.segments[2].start_seconds, 3725.0);
        assert!(m.segments[2].end_seconds > m.segments[2].start_seconds);
        assert!(m.duration_seconds > 3725.0);
    }

    #[test]
    fn meeting_import_otter_srt_fixture_becomes_segments() {
        let m = parse("call.SRT", SRT).expect("fixture parses");
        assert_eq!(m.segments.len(), 3);
        assert_eq!(m.segments[1].start_seconds, 4.5);
        assert_eq!(m.segments[1].end_seconds, 9.25);
        assert_eq!(
            m.segments[1].text,
            "Jeisil Hernandez: Sure. The annual plan needs a clearer discount line."
        );
        // A cue with no speaker prefix keeps its words as-is.
        assert_eq!(m.segments[2].text, "Agreed, I will draft both tonight.");
        assert_eq!(m.duration_seconds, 12.0);
    }

    #[test]
    fn meeting_import_malformed_srt_is_a_plain_error() {
        let err = parse("broken.srt", BAD_SRT).unwrap_err();
        assert!(matches!(err, ImportError::Malformed(_)), "{err:?}");
        let msg = err.to_string();
        assert!(msg.contains("Nothing was imported"), "{msg}");
    }

    #[test]
    fn meeting_import_malformed_txt_is_a_plain_error() {
        let err = parse("notes.txt", BAD_TXT).unwrap_err();
        assert!(matches!(err, ImportError::Malformed(_)), "{err:?}");
    }

    #[test]
    fn meeting_import_refuses_docx_and_unknown_extensions() {
        assert_eq!(
            parse("call.docx", "PK...").unwrap_err(),
            ImportError::Unsupported("docx".into())
        );
        assert!(parse("call.docx", "")
            .unwrap_err()
            .to_string()
            .contains(".txt or .srt"));
        assert!(matches!(
            parse("call.pdf", "x"),
            Err(ImportError::Unsupported(_))
        ));
        assert_eq!(parse("empty.txt", "  \n").unwrap_err(), ImportError::Empty);
    }

    #[test]
    fn meeting_import_clock_and_srt_time_edges() {
        assert_eq!(parse_clock("0:00"), Some(0.0));
        assert_eq!(parse_clock("12:34"), Some(754.0));
        assert_eq!(parse_clock("1:02:03"), Some(3723.0));
        assert_eq!(parse_clock("1:60"), None);
        assert_eq!(parse_clock("abc"), None);
        assert_eq!(parse_srt_time("00:01:02,345"), Some(62.345));
        assert_eq!(parse_srt_time("00:01:02"), None);
    }
}
