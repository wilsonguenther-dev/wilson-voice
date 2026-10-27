#!/usr/bin/env python3
"""compare-reference.py -- score a transcribed call against a reference, in numbers.

Prints ONLY numbers and fixed key names. It never prints a word of any
transcript and never a speaker's name: reference speakers become A, B, C... in
order of first appearance, tracks become t0, t1... This is deliberate. The
databases it reads can hold a real person's meetings, and the output of a
diagnostic is the part that gets pasted into PRs and chat.

Run it with `python3 -I` (isolated: no user site, no PYTHONPATH, nothing from
the current directory). Standard library only.

  compare-reference.py <db-copy> <meeting-id> <reference>
      Score one meeting in a COPY of wilson_voice.db (never the live file: the
      script opens it read-only and refuses a path inside Application Support).

  compare-reference.py --tracks <t0.json> <t1.json> <reference>
      Score two timed-transcript files (the output of
      `wilson-voice --transcribe-file <wav> --timed`, one per track, mic first)
      with no database. This is how the AEC pre-pass is measured on audio on disk.

  compare-reference.py --notes <db-copy> <meeting-id>
      Numbers only for one meeting's notes: the Summary's word 4-gram verbatim
      ratio against the transcript, duplicate lines across sections, the most
      items citing one segment, and owner counts (Me / Them / Someone here /
      blank, plus other).

  compare-reference.py --self-test
      Runs the whole thing on a tiny synthetic pair and exits 0.

The reference is "M:SS - Speaker" blocks: a header line, then the text, blocks
separated by blank lines (H:MM:SS also works).

Output keys (DB and --tracks modes), all numbers:
  offset_seconds            median (hypothesis time - reference time) over the
                            reference blocks whose first word was found
  ref_words, speakers
  t<k>_hyp_words            words in track k
  share_t<k>_<S>            share of speaker S's reference words found in track k
  t<k>_home_speakers        how many speakers have track k as their best track
  t<k>_sub/ins/del/wer      track k scored against the speakers whose home it is
                            (insertions there are the other side leaking in)
  rendered_sub/ins/del/wer  both tracks merged by time, scored against everything
  coverage_words            share of reference words found in the rendered text
  coverage_time             share of the meeting's span covered by segments
  echo_rate                 share of call-track windows (>=3 words) whose word
                            trigrams are >=30% contained in the mic text within
                            +-1.5 s. Same definition as
                            desktop/src-tauri/src/meeting_echo.rs.
"""

import json
import math
import os
import re
import sqlite3
import sys
import tempfile

ECHO_THRESHOLD = 0.30
ECHO_SEARCH_SECONDS = 1.5
VERBATIM_N = 4
HOME_MARGIN = 0.05

# ---------------------------------------------------------------------------
# Words
# ---------------------------------------------------------------------------


def norm_words(text):
    """ASCII alphanumerics and word-internal apostrophes, lowercased."""
    out = []
    cur = []
    for ch in text:
        if ch.isascii() and ch.isalnum():
            cur.append(ch.lower())
        elif ch in ("'", "’") and cur:
            cur.append("'")
        elif cur:
            out.append("".join(cur))
            cur = []
    if cur:
        out.append("".join(cur))
    out = [w.rstrip("'") for w in out]
    return [w for w in out if w]


def ngram_set(words, n):
    if n <= 0 or len(words) < n:
        return set()
    return {" ".join(words[i : i + n]) for i in range(len(words) - n + 1)}


def containment(window_words, reference_words, n):
    own = ngram_set(window_words, n)
    if not own:
        return 0.0
    other = ngram_set(reference_words, n)
    return len(own & other) / len(own)


# ---------------------------------------------------------------------------
# Reference
# ---------------------------------------------------------------------------

HEADER = re.compile(r"^\s*(?:(\d+):)?(\d{1,2}):(\d{2})\s*[-–—]\s*(.+?)\s*$")


def parse_reference(text):
    """-> list of (start_seconds, speaker_letter, [words]) and the speaker order."""
    blocks = []
    letters = {}
    current = None
    for line in text.splitlines():
        m = HEADER.match(line)
        if m:
            hours = int(m.group(1) or 0)
            start = hours * 3600 + int(m.group(2)) * 60 + int(m.group(3))
            name = m.group(4).strip().lower()
            if name not in letters:
                letters[name] = letter(len(letters))
            current = [float(start), letters[name], []]
            blocks.append(current)
        elif current is not None and line.strip():
            current[2].extend(norm_words(line))
    return [(s, who, words) for s, who, words in blocks if words], list(letters.values())


def letter(i):
    s = ""
    i += 1
    while i:
        i, r = divmod(i - 1, 26)
        s = chr(ord("A") + r) + s
    return s


# ---------------------------------------------------------------------------
# Alignment (word-level Levenshtein, matches preferred on ties)
# ---------------------------------------------------------------------------

MATCH, SUB, DEL, INS = 0, 1, 2, 3


def align(ref, hyp):
    """-> list of (op, ref_index or None, hyp_index or None) in order.

    `ref` and `hyp` are lists of words. DEL = a reference word with no
    hypothesis word; INS = a hypothesis word with no reference word.
    """
    n, m = len(ref), len(hyp)
    width = m + 1
    back = bytearray((n + 1) * width)
    prev = list(range(m + 1))
    for j in range(1, m + 1):
        back[j] = INS
    for i in range(1, n + 1):
        cur = [i] + [0] * m
        back[i * width] = DEL
        ri = ref[i - 1]
        for j in range(1, m + 1):
            same = ri == hyp[j - 1]
            diag = prev[j - 1] + (0 if same else 1)
            dele = prev[j] + 1
            ins = cur[j - 1] + 1
            best = diag
            op = MATCH if same else SUB
            if dele < best:
                best, op = dele, DEL
            if ins < best:
                best, op = ins, INS
            cur[j] = best
            back[i * width + j] = op
        prev = cur
    ops = []
    i, j = n, m
    while i > 0 or j > 0:
        op = back[i * width + j]
        if op in (MATCH, SUB):
            ops.append((op, i - 1, j - 1))
            i, j = i - 1, j - 1
        elif op == DEL:
            ops.append((DEL, i - 1, None))
            i -= 1
        else:
            ops.append((INS, None, j - 1))
            j -= 1
    ops.reverse()
    return ops


def counts(ops):
    c = {MATCH: 0, SUB: 0, DEL: 0, INS: 0}
    for op, _, _ in ops:
        c[op] += 1
    return c


def wer(c, ref_n):
    if ref_n == 0:
        return float("nan")
    return (c[SUB] + c[DEL] + c[INS]) / ref_n


# ---------------------------------------------------------------------------
# Hypothesis: segments -> timed words
# ---------------------------------------------------------------------------


def timed_words(segments):
    """segments: [(start, end, text)] -> [(word, time)], time interpolated
    evenly across the segment so a 30 s window still places its words."""
    out = []
    for start, end, text in segments:
        words = norm_words(text)
        if not words:
            continue
        span = max(end - start, 0.0)
        for k, w in enumerate(words):
            out.append((w, start + span * (k / len(words))))
    return out


def echo_rate(call_segments, mic_segments):
    judged = echoed = 0
    for start, end, text in call_segments:
        words = norm_words(text)
        if len(words) < 3:
            continue
        judged += 1
        lo, hi = start - ECHO_SEARCH_SECONDS, end + ECHO_SEARCH_SECONDS
        near = []
        for s, e, t in mic_segments:
            if e >= lo and s <= hi:
                near.extend(norm_words(t))
        if containment(words, near, 3) >= ECHO_THRESHOLD:
            echoed += 1
    return echoed / judged if judged else 0.0


def covered_share(segments_by_track):
    spans = sorted((s, e) for segs in segments_by_track.values() for s, e, _ in segs if e > s)
    if not spans:
        return 0.0
    total = max(e for _, e in spans) - min(s for s, _ in spans)
    union = 0.0
    cur_s, cur_e = spans[0]
    for s, e in spans[1:]:
        if s > cur_e:
            union += cur_e - cur_s
            cur_s, cur_e = s, e
        else:
            cur_e = max(cur_e, e)
    union += cur_e - cur_s
    return union / total if total > 0 else 0.0


# ---------------------------------------------------------------------------
# The comparison
# ---------------------------------------------------------------------------


def analyze(blocks, speakers, segments_by_track):
    """-> list of (key, number). `segments_by_track`: {track: [(start, end, text)]}."""
    out = []
    ref_words = []
    ref_speaker = []
    ref_block = []
    for b, (_, who, words) in enumerate(blocks):
        for w in words:
            ref_words.append(w)
            ref_speaker.append(who)
            ref_block.append(b)
    out.append(("ref_words", len(ref_words)))
    out.append(("speakers", len(speakers)))
    tracks = sorted(segments_by_track)

    hyp = {t: timed_words(segments_by_track[t]) for t in tracks}
    share = {}
    for t in tracks:
        out.append((f"t{t}_hyp_words", len(hyp[t])))
        ops = align(ref_words, [w for w, _ in hyp[t]])
        found = {s: 0 for s in speakers}
        for op, i, _ in ops:
            if op == MATCH:
                found[ref_speaker[i]] += 1
        for s in speakers:
            total = sum(1 for x in ref_speaker if x == s)
            share[(t, s)] = found[s] / total if total else 0.0
            out.append((f"share_t{t}_{s}", share[(t, s)]))

    # A speaker's home is the track holding most of their words. A tie goes to
    # the HIGHER track, within HOME_MARGIN: a far-side voice leaking into the mic
    # can match its own system-track share (the word alignment is not exact, so
    # "match" means within a few points) but never really beat it, so among
    # tracks that close the original is the system track.
    home = {}
    for s in speakers:
        top = max((share[(t, s)] for t in tracks), default=0.0)
        near_top = [t for t in tracks if share[(t, s)] >= top - HOME_MARGIN]
        home[s] = max(near_top) if near_top and top > 0 else None
    for t in tracks:
        mine = [i for i, s in enumerate(ref_speaker) if home.get(s) == t]
        out.append((f"t{t}_home_speakers", sum(1 for s in speakers if home.get(s) == t)))
        sub_ref = [ref_words[i] for i in mine]
        c = counts(align(sub_ref, [w for w, _ in hyp[t]]))
        out.append((f"t{t}_sub", c[SUB]))
        out.append((f"t{t}_ins", c[INS]))
        out.append((f"t{t}_del", c[DEL]))
        out.append((f"t{t}_wer", wer(c, len(sub_ref))))

    merged = sorted((p for t in tracks for p in hyp[t]), key=lambda p: p[1])
    ops = align(ref_words, [w for w, _ in merged])
    c = counts(ops)
    out.append(("rendered_sub", c[SUB]))
    out.append(("rendered_ins", c[INS]))
    out.append(("rendered_del", c[DEL]))
    out.append(("rendered_wer", wer(c, len(ref_words))))
    out.append(("coverage_words", c[MATCH] / len(ref_words) if ref_words else 0.0))
    out.append(("coverage_time", covered_share(segments_by_track)))

    # offset: for each block, the first reference word that matched gives
    # (where the hypothesis put it) - (where the reference says the block starts).
    first = {}
    for op, i, j in ops:
        if op == MATCH and ref_block[i] not in first:
            first[ref_block[i]] = merged[j][1] - blocks[ref_block[i]][0]
    deltas = sorted(first.values())
    if deltas:
        mid = len(deltas) // 2
        med = deltas[mid] if len(deltas) % 2 else (deltas[mid - 1] + deltas[mid]) / 2
        out.append(("offset_seconds", med))
    else:
        out.append(("offset_seconds", float("nan")))

    if len(tracks) >= 2:
        mic = tracks[0]
        call = [t for t in tracks if t != mic][-1]
        out.append(("echo_rate", echo_rate(segments_by_track[call], segments_by_track[mic])))
    else:
        out.append(("echo_rate", float("nan")))
    return out


def fmt(value):
    if isinstance(value, float):
        return "nan" if math.isnan(value) else f"{value:.4f}"
    return str(value)


def emit(rows, stream):
    for key, value in rows:
        stream.write(f"{key}={fmt(value)}\n")


# ---------------------------------------------------------------------------
# Inputs
# ---------------------------------------------------------------------------


def read_text(path):
    with open(path, "r", encoding="utf-8") as f:
        return f.read()


def open_copy(path):
    real = os.path.realpath(path)
    if "Application Support" in real:
        raise SystemExit("refusing a database inside Application Support: work on a COPY")
    if not os.path.isfile(real):
        raise SystemExit("database copy not found")
    return sqlite3.connect(f"file:{real}?mode=ro", uri=True)


def db_segments(conn, meeting_id):
    cols = [r[1] for r in conn.execute("PRAGMA table_info(meeting_segments)")]
    track = "track" if "track" in cols else "0"
    rows = conn.execute(
        f"SELECT {track}, start_seconds, end_seconds, text FROM meeting_segments "
        "WHERE meeting_id = ? ORDER BY start_seconds",
        (meeting_id,),
    ).fetchall()
    if not rows:
        raise SystemExit("no segments for that meeting id")
    by_track = {}
    for t, s, e, text in rows:
        # Track 0 is the microphone; anything else came from the process tap.
        by_track.setdefault(0 if t == 0 else 1, []).append((float(s), float(e), text or ""))
    return by_track


def timed_json_segments(path):
    with open(path, "r", encoding="utf-8") as f:
        doc = json.load(f)

    def span(row):
        s = row.get("startSeconds", row.get("start_seconds", 0.0))
        e = row.get("endSeconds", row.get("end_seconds", s))
        return float(s), float(e), row.get("text", "")

    segments = [span(r) for r in doc.get("segments", [])]
    if segments:
        return segments
    # Words only: group into windows at pauses longer than half a second.
    words = [span(r) for r in doc.get("words", [])]
    windows = []
    for s, e, t in words:
        if windows and s - windows[-1][1] <= 0.5:
            ws, _, wt = windows[-1]
            windows[-1] = (ws, e, f"{wt} {t}")
        else:
            windows.append((s, e, t))
    if not windows and doc.get("text"):
        windows = [(0.0, 0.0, doc["text"])]
    return windows


# ---------------------------------------------------------------------------
# Notes
# ---------------------------------------------------------------------------

ITEM = re.compile(r"^\s*-\s+(?P<text>.*?)\s*\((?:(?P<who>[^,()]*?),\s*)?(?P<at>(?:\d+:)?\d{1,2}:\d{2})\)\s*$")
SECTION = re.compile(r"^\s*#{2,4}\s+(.*?)\s*$")


def at_seconds(at):
    parts = [int(p) for p in at.split(":")]
    total = 0
    for p in parts:
        total = total * 60 + p
    return float(total)


def notes_numbers(summary, transcript_words, segments):
    """-> rows. `segments` is [(start, end)] used to map a citation to a segment."""
    narrative = []
    items = []  # (section, text_words, owner, seconds)
    section = None
    for line in summary.splitlines():
        h = SECTION.match(line)
        if h:
            section = h.group(1).strip().lower()
            continue
        m = ITEM.match(line)
        if m and section is not None:
            items.append(
                (section, norm_words(m.group("text")), (m.group("who") or "").strip(), at_seconds(m.group("at")))
            )
        elif section is None and line.strip():
            narrative.append(line)

    rows = []
    summary_words = norm_words("\n".join(narrative))
    rows.append(("summary_words", len(summary_words)))
    rows.append(("summary_verbatim_ratio", containment(summary_words, transcript_words, VERBATIM_N)))

    seen = {}
    duplicate = 0
    for section, words, _, _ in items:
        key = " ".join(words)
        if not key:
            continue
        if key in seen:
            duplicate += 1
        seen[key] = section
    rows.append(("items", len(items)))
    rows.append(("duplicate_lines", duplicate))

    def segment_of(sec):
        for k, (s, e) in enumerate(segments):
            if s <= sec < e:
                return k
        if segments:
            return min(range(len(segments)), key=lambda k: abs(segments[k][0] - sec))
        return sec

    per = {}
    for _, _, _, sec in items:
        key = segment_of(sec)
        per[key] = per.get(key, 0) + 1
    rows.append(("max_items_per_cited_segment", max(per.values()) if per else 0))

    owners = {"me": 0, "them": 0, "someone_here": 0, "blank": 0, "other": 0}
    for _, _, who, _ in items:
        w = who.strip().lower()
        if not w:
            owners["blank"] += 1
        elif w == "me":
            owners["me"] += 1
        elif w == "them":
            owners["them"] += 1
        elif w == "someone here":
            owners["someone_here"] += 1
        else:
            owners["other"] += 1
    for k, v in owners.items():
        rows.append((f"owner_{k}", v))
    return rows


def run_notes(db, meeting_id, stream):
    conn = open_copy(db)
    try:
        row = conn.execute("SELECT summary FROM meetings WHERE id = ?", (meeting_id,)).fetchone()
        if row is None:
            raise SystemExit("no such meeting id")
        segs = conn.execute(
            "SELECT start_seconds, end_seconds, text FROM meeting_segments WHERE meeting_id = ? "
            "ORDER BY start_seconds",
            (meeting_id,),
        ).fetchall()
    finally:
        conn.close()
    transcript = norm_words(" ".join(t or "" for _, _, t in segs))
    emit(notes_numbers(row[0] or "", transcript, [(float(s), float(e)) for s, e, _ in segs]), stream)


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------

_SELF_REFERENCE = """0:00 - Alice Example
the hall has the second weekend open for the workshops

0:06 - Bob Example
parking is fine there is a lot behind the building

0:12 - Alice Example
then the second weekend looks better to me
"""

_SELF_SUMMARY = """The hall has the second weekend open for the workshops and parking is fine.

### Action items

- Send the budget outline (Me, 0:03)
- Call the caterer (Them, 0:09)
- Order the banners (0:09)

### Decisions

- Send the budget outline (Me, 0:03)
"""


def self_test():
    failures = []

    def check(name, ok):
        if not ok:
            failures.append(name)

    blocks, speakers = parse_reference(_SELF_REFERENCE)
    check("parse blocks", len(blocks) == 3 and blocks[1][0] == 6.0)
    check("parse speakers", speakers == ["A", "B"])

    # Track 0 (mic) hears speaker A and, late and complete, speaker B's words (the
    # leak). Track 1 (system) is speaker B alone.
    t0 = [
        (0.0, 5.0, "The hall has the second weekend open for the workshops."),
        (6.1, 11.0, "Parking is fine, there is a lot behind the building."),
        (12.0, 16.0, "Then the second weekend looks better to me."),
    ]
    t1 = [(6.0, 11.0, "Parking is fine there is a lot behind the building")]
    rows = dict(analyze(blocks, speakers, {0: t0, 1: t1}))
    check("share A in t0", rows["share_t0_A"] == 1.0)
    check("share B leaks into t0", rows["share_t0_B"] == 1.0)
    check("share B in t1", rows["share_t1_B"] == 1.0)
    check("share A not in t1", rows["share_t1_A"] == 0.0)
    check("homes", rows["t0_home_speakers"] == 1 and rows["t1_home_speakers"] == 1)
    check("leak is insertions in t0", rows["t0_ins"] == 10 and rows["t0_del"] == 0 and rows["t0_sub"] == 0)
    check("t1 is clean", rows["t1_wer"] == 0.0)
    check("echo detected", rows["echo_rate"] == 1.0)
    check("rendered doubles B", rows["rendered_ins"] == 10 and rows["rendered_del"] == 0)
    check("coverage", rows["coverage_words"] == 1.0)
    check("offset small", abs(rows["offset_seconds"]) < 1.0)

    # Without the leak there is no echo and no insertion.
    clean0 = [t0[0], t0[2]]
    clean = dict(analyze(blocks, speakers, {0: clean0, 1: t1}))
    check("no leak, no echo", clean["echo_rate"] == 0.0)
    check("no leak, no insertions", clean["t0_ins"] == 0 and clean["rendered_ins"] == 0)
    check("no leak, B stays out of t0", clean["share_t0_B"] == 0.0)

    # A shifted hypothesis moves the offset by the shift.
    shifted = [(s + 2.0, e + 2.0, t) for s, e, t in clean0]
    shifted1 = [(s + 2.0, e + 2.0, t) for s, e, t in t1]
    off = dict(analyze(blocks, speakers, {0: shifted, 1: shifted1}))["offset_seconds"]
    check("offset tracks a 2 s shift", abs(off - 2.0) < 0.6)

    # One track only: no echo number.
    solo = dict(analyze(blocks, speakers, {0: t0}))
    check("one track has no echo rate", math.isnan(solo["echo_rate"]))

    # Notes.
    narrative_words = norm_words("The hall has the second weekend open for the workshops")
    transcript = norm_words(" ".join(t for _, _, t in t0))
    nrows = dict(notes_numbers(_SELF_SUMMARY, transcript, [(0.0, 5.0), (6.0, 11.0), (12.0, 16.0)]))
    check("verbatim ratio", 0.0 < nrows["summary_verbatim_ratio"] < 1.0)
    check("verbatim ratio of a paste", containment(narrative_words, narrative_words + ["x"], 4) == 1.0)
    check("duplicate across sections", nrows["duplicate_lines"] == 1)
    check("max items per segment", nrows["max_items_per_cited_segment"] == 2)
    check("owner counts", (nrows["owner_me"], nrows["owner_them"], nrows["owner_blank"]) == (2, 1, 1))

    # DB mode and notes mode, end to end, through a real sqlite file.
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "copy.db")
        conn = sqlite3.connect(path)
        conn.executescript(
            "CREATE TABLE meetings (id TEXT PRIMARY KEY, summary TEXT);"
            "CREATE TABLE meeting_segments (id INTEGER PRIMARY KEY, meeting_id TEXT, "
            "start_seconds REAL, end_seconds REAL, text TEXT, track INTEGER NOT NULL DEFAULT 0);"
        )
        conn.execute("INSERT INTO meetings VALUES ('m1', ?)", (_SELF_SUMMARY,))
        for track, segs in ((0, t0), (1, t1)):
            for s, e, t in segs:
                conn.execute(
                    "INSERT INTO meeting_segments (meeting_id, start_seconds, end_seconds, text, track) "
                    "VALUES ('m1', ?, ?, ?, ?)",
                    (s, e, t, track),
                )
        conn.commit()
        conn.close()
        ref_path = os.path.join(tmp, "reference.txt")
        with open(ref_path, "w", encoding="utf-8") as f:
            f.write(_SELF_REFERENCE)
        t0_path, t1_path = os.path.join(tmp, "t0.json"), os.path.join(tmp, "t1.json")
        for p, segs in ((t0_path, t0), (t1_path, t1)):
            with open(p, "w", encoding="utf-8") as f:
                json.dump(
                    {"text": "", "segments": [
                        {"startSeconds": s, "endSeconds": e, "text": t} for s, e, t in segs]},
                    f,
                )

        class Sink:
            def __init__(self):
                self.text = ""

            def write(self, s):
                self.text += s

        for argv in (
            [path, "m1", ref_path],
            ["--tracks", t0_path, t1_path, ref_path],
            ["--notes", path, "m1"],
        ):
            sink = Sink()
            try:
                main(argv, sink)
            except SystemExit as e:
                check(f"mode {argv[0]} exits cleanly ({e})", False)
                continue
            check(f"mode {argv[0]} prints something", bool(sink.text.strip()))
            for line in sink.text.splitlines():
                key, _, value = line.partition("=")
                check(f"numeric line {key}", bool(re.fullmatch(r"[A-Za-z0-9_]+", key)) and _is_number(value))
            # No transcript text and no names in the output.
            lowered = sink.text.lower()
            for secret in ("alice", "bob", "weekend", "parking", "caterer", "hall"):
                check(f"output never prints {secret}", secret not in lowered)
        # A path inside Application Support is refused.
        try:
            open_copy("/Users/x/Library/Application Support/x/wilson_voice.db")
            check("refuses the live data dir", False)
        except SystemExit:
            pass

    if failures:
        sys.stderr.write("self-test FAILED: " + ", ".join(failures) + "\n")
        return 1
    sys.stdout.write("self-test ok\n")
    return 0


def _is_number(value):
    try:
        float(value)
        return True
    except ValueError:
        return False


# ---------------------------------------------------------------------------


def main(argv, stream):
    if argv and argv[0] == "--self-test":
        raise SystemExit(self_test())
    if argv and argv[0] == "--notes":
        if len(argv) != 3:
            raise SystemExit("usage: --notes <db-copy> <meeting-id>")
        run_notes(argv[1], argv[2], stream)
        return
    if argv and argv[0] == "--tracks":
        if len(argv) != 4:
            raise SystemExit("usage: --tracks <t0.json> <t1.json> <reference>")
        blocks, speakers = parse_reference(read_text(argv[3]))
        by_track = {0: timed_json_segments(argv[1]), 1: timed_json_segments(argv[2])}
        emit(analyze(blocks, speakers, by_track), stream)
        return
    if len(argv) != 3 or argv[0].startswith("--"):
        raise SystemExit(
            "usage: compare-reference.py <db-copy> <meeting-id> <reference> | "
            "--tracks <t0.json> <t1.json> <reference> | --notes <db-copy> <meeting-id> | --self-test"
        )
    blocks, speakers = parse_reference(read_text(argv[2]))
    conn = open_copy(argv[0])
    try:
        by_track = db_segments(conn, argv[1])
    finally:
        conn.close()
    emit(analyze(blocks, speakers, by_track), stream)


if __name__ == "__main__":
    main(sys.argv[1:], sys.stdout)
