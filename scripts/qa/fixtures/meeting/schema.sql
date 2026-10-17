CREATE TABLE meetings (id TEXT PRIMARY KEY, title TEXT, started_at TEXT, ended_at TEXT, duration_seconds REAL, state TEXT, error TEXT, summary TEXT);
CREATE TABLE meeting_segments (id TEXT, meeting_id TEXT, start_seconds REAL, end_seconds REAL, text TEXT, track INTEGER);
