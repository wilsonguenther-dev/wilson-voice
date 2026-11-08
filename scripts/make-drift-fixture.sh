#!/bin/sh
# yap-1106-fb-23 — the real-audio drift fixture for
# desktop/src-tauri/tests/diarize_speaker_cap.rs (the #[ignore]d variant).
#
# Input: FOUR licensed speech clips (CC-BY / CC0 / your own recordings; never
# a clip you have no licence for), one speaker each, 16 kHz-capable WAV/FLAC:
#   scripts/make-drift-fixture.sh a.wav b.wav c.wav d.wav out.wav
# Output: one 16 kHz mono WAV where A, B, C, D take turns and D's second half
# is band-passed to a phone line (300–3400 Hz) — the "voice that drifts".
# Then: YAP_DRIFT_FIXTURE_WAV=out.wav cargo test --test diarize_speaker_cap -- --ignored
set -eu
[ "$#" -eq 5 ] || { echo "usage: $0 a.wav b.wav c.wav d.wav out.wav" >&2; exit 2; }
command -v ffmpeg >/dev/null || { echo "ffmpeg is required" >&2; exit 2; }
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
i=0
for f in "$1" "$2" "$3" "$4"; do
  ffmpeg -v error -y -i "$f" -ac 1 -ar 16000 -t 24 "$tmp/s$i.wav"
  i=$((i+1))
done
ffmpeg -v error -y -i "$tmp/s3.wav" -ss 12 -t 12 -af "highpass=f=300,lowpass=f=3400" "$tmp/s3b.wav"
ffmpeg -v error -y -i "$tmp/s3.wav" -t 12 "$tmp/s3a.wav"
list="$tmp/list.txt"; : > "$list"
for half in 0 1; do
  for s in 0 1 2; do
    ffmpeg -v error -y -i "$tmp/s$s.wav" -ss $((half*12)) -t 12 "$tmp/t${s}_$half.wav"
    echo "file '$tmp/t${s}_$half.wav'" >> "$list"
  done
  if [ "$half" -eq 0 ]; then echo "file '$tmp/s3a.wav'" >> "$list"; else echo "file '$tmp/s3b.wav'" >> "$list"; fi
done
ffmpeg -v error -y -f concat -safe 0 -i "$list" -ac 1 -ar 16000 "$5"
echo "wrote $5"
