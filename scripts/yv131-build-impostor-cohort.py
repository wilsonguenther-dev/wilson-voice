#!/usr/bin/env python3
"""YV131 — build the shipped AS-norm impostor cohort, and measure whether it helps.

This script is the ONLY producer of `desktop/src-tauri/assets/yv131-impostor-cohort.bin`
and of `docs/yap23-asnorm-measurement.json`. It is checked in so that both are
reproducible rather than blobs somebody once generated and nobody can regenerate.

WHAT IT PRODUCES
  1. desktop/src-tauri/assets/yv131-impostor-cohort.bin
     `count * dim` little-endian f32 — L2-normalized CAM++ embeddings, one per
     (speaker, condition), nothing else. No header: everything describing these
     bytes lives in the manifest beside them, the same split `models.rs` already
     uses (catalog.json describes, the payload is payload).
  2. desktop/src-tauri/assets/yv131-impostor-cohort.json
     The provenance manifest — model digest, extractor version, corpus, speaker
     ids, conditions, dim, count, and the sha256 of (1). Compiled in via
     `include_str!` and checked against (1) by `tests/as_norm_cohort_is_provenanced.rs`.
  3. docs/yap23-asnorm-measurement.json
     Per-trial raw and AS-norm scores for the held-out arm, consumed by
     `tests/as_norm_cross_condition_measured.rs`, which recomputes both EERs
     through YV120's `enrollment_eer` rather than trusting a number printed here.

WHY THESE VOICES
  An AS-norm cohort has two requirements: the voices are definitely not any
  enrolled speaker, and they are embedded by the same model the matcher uses.
  LibriSpeech satisfies the first (a published research corpus read from
  public-domain LibriVox recordings, with no connection to any Yap user) and is
  license-clear: CC-BY-4.0, the same license as the CAM++ weights this repo
  already vendors. Only 512-float embeddings are shipped — no audio is
  redistributed, and an embedding is not invertible to one.

  A SYNTHETIC cohort was built first, from macOS `say` system voices, and
  REJECTED on measurement. CAM++ cannot separate `say` voices at all: over five
  held-out English system voices, clean same-condition EER was 35.6%, against
  2.2% for real LibriSpeech speakers under the identical harness. Two different
  `say` voices score cosine 0.64 where two different humans score 0.51, and
  Apple's Siri-era voices (Eddy/Reed/Rocko/Grandpa/Grandma/Shelley) embed at
  0.91-0.997 to each other across their US and UK variants — variations on one
  synthetic speaker, not different speakers. A cohort has to model "some
  stranger", and TTS output does not land where strangers land.

USAGE
  python3 scripts/yv131-build-impostor-cohort.py \\
      --model /path/to/wespeaker_en_voxceleb_CAM++.onnx \\
      --librispeech /path/to/LibriSpeech

  Needs `sherpa-onnx==1.13.4`, `numpy` and `soundfile`. 1.13.4 is not a nearby
  version — it is the exact pin `desktop/yap-diarize/Cargo.toml` carries, so
  these embeddings come out of the same feature extractor and the same inference
  code the shipped sidecar runs. A cohort embedded by a different pipeline than
  the one that embeds the live utterance is not a cohort, it is noise in the
  same shape.

  LibriSpeech subsets (openslr.org/12): test-clean for the cohort, dev-clean to
  choose the design, dev-other to report it. The three are disjoint speaker
  sets, which is the point — a cohort sharing speakers with the targets is not
  measuring what it claims to, and a design chosen and reported on one split is
  not measured, it is fitted.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import pathlib

import numpy as np

# The pinned CAM++ digest, copied from desktop/src-tauri/src/catalog.json. The
# script re-verifies rather than trusting the path it was handed: an embedding
# space is defined by the weights, so the wrong file silently produces a cohort
# in no relation at all to the one the app will use.
CAMPP_SHA256 = "c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef"
SHERPA_PIN = "1.13.4"

SR = 16000

# 30 s per segment, matching YV126's cluster floor — the shortest thing this
# pipeline ever asks an identity question about. It is also the point past which
# CAM++ is stable here: measured on LibriSpeech dev-clean, a 30 s segment
# reproduces its own speaker at cosine 0.97, while 15 s segments swing between
# 0.57 and 0.99 on the SAME audio depending only on where the window ends.
# Tuning a normalization on top of embeddings that unstable would be tuning the
# window, not the voice.
SEGMENT_SECONDS = 30.0
TEST_SEGMENTS = 2

COHORT_SUBSET = "test-clean"
TUNE_SUBSET = "dev-clean"
REPORT_SUBSET = "dev-other"

# The cohort carries each speaker under two conditions, so AS-norm's adaptive
# top-K has something condition-appropriate to select — the spec's "normalize by
# the impostor-score distribution for that specific recording condition".
COHORT_CONDITIONS = ("clean", "bandlimited")

# Chosen on dev-clean, reported on dev-other. NOT a decision threshold: K is how
# many cohort scores enter the mean/std, and no accept/reject comparison in the
# shipped code uses a number tuned here.
TOP_K = 20

# The channel this item reports against. B1 is the case Wilson asked about — the
# same person on a headset and then on a laptop microphone. B2-B4 are in the
# ladder to show where the effect shrinks, rather than to imply it does not.
REPORT_CHANNEL = "B1 headset->laptop, mild"


# ---------------------------------------------------------------------------
# Channels
# ---------------------------------------------------------------------------


def biquad(x, b, a):
    """Direct-form-I biquad. numpy only — no scipy dependency for six lines."""
    b0, b1, b2 = (c / a[0] for c in b)
    a1, a2 = a[1] / a[0], a[2] / a[0]
    y = np.zeros_like(x)
    x1 = x2 = y1 = y2 = 0.0
    for i, xn in enumerate(x):
        yn = b0 * xn + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        y[i] = yn
        x2, x1 = x1, xn
        y2, y1 = y1, yn
    return y


def rbj_lowpass(x, f0, q=0.707):
    w0 = 2 * math.pi * f0 / SR
    alpha = math.sin(w0) / (2 * q)
    cw = math.cos(w0)
    return biquad(x, ((1 - cw) / 2, 1 - cw, (1 - cw) / 2), (1 + alpha, -2 * cw, 1 - alpha))


def rbj_highpass(x, f0, q=0.707):
    w0 = 2 * math.pi * f0 / SR
    alpha = math.sin(w0) / (2 * q)
    cw = math.cos(w0)
    return biquad(x, ((1 + cw) / 2, -(1 + cw), (1 + cw) / 2), (1 + alpha, -2 * cw, 1 - alpha))


def reflect(x, taps):
    o = x.copy()
    for delay_ms, gain in taps:
        n = int(delay_ms * SR / 1000)
        o[n:] += gain * x[:-n]
    return o.astype(np.float32)


def noisy(x, sd, rng):
    y = x + rng.normal(0.0, sd, len(x)).astype(np.float32)
    peak = float(np.max(np.abs(y)))
    return (y * (0.99 / peak) if peak > 0.99 else y).astype(np.float32)


# A ladder, not one condition. "Cross-device" is not a single thing: it runs
# from a headset swap that barely changes the spectrum to a far-field room that
# removes most of it, and a claim about AS-norm is only meaningful with the
# severity attached. These are SIMULATED channels applied to real speech — no
# AirPods were recorded, and nothing below depends on pretending otherwise.
CHANNELS = {
    "A clean (control)": lambda x, r: x,
    "B1 headset->laptop, mild": lambda x, r: noisy(rbj_lowpass(x, 7000.0) * 0.8, 0.0005, r),
    "B2 across-desk, moderate": lambda x, r: noisy(
        reflect(rbj_lowpass(x, 5000.0), [(17.0, 0.18), (29.0, 0.10)]) * 0.6, 0.0010, r),
    "B3 far-field room": lambda x, r: noisy(
        reflect(rbj_lowpass(rbj_highpass(x, 120.0), 4000.0),
                [(17.0, 0.32), (29.0, 0.19), (43.0, 0.11)]) * 0.45, 0.0015, r),
    "B4 telephone band": lambda x, r: noisy(
        reflect(rbj_lowpass(rbj_highpass(x, 300.0), 3400.0),
                [(17.0, 0.32), (29.0, 0.19), (43.0, 0.11)]) * 0.45, 0.0015, r),
}


# ---------------------------------------------------------------------------
# Embeddings
# ---------------------------------------------------------------------------


class Embedder:
    def __init__(self, model: pathlib.Path):
        import sherpa_onnx

        if sherpa_onnx.__version__ != SHERPA_PIN:  # pragma: no cover - env guard
            raise SystemExit(
                f"sherpa-onnx {sherpa_onnx.__version__} is installed; this cohort must be built "
                f"with the pinned {SHERPA_PIN} that desktop/yap-diarize/Cargo.toml uses"
            )
        self.version = sherpa_onnx.__version__
        self._ex = sherpa_onnx.SpeakerEmbeddingExtractor(
            sherpa_onnx.SpeakerEmbeddingExtractorConfig(
                model=str(model), num_threads=2, debug=False, provider="cpu"
            )
        )
        self.dim = self._ex.dim

    def embed(self, samples):
        stream = self._ex.create_stream()
        stream.accept_waveform(sample_rate=SR, waveform=samples)
        stream.input_finished()
        if not self._ex.is_ready(stream):
            raise SystemExit("segment too short for CAM++ to embed")
        return np.array(self._ex.compute(stream), dtype=np.float32)


def l2(v):
    n = float(np.linalg.norm(v))
    if n <= 0.0:
        raise SystemExit("zero-norm embedding")
    return (v / n).astype(np.float32)


# ---------------------------------------------------------------------------
# AS-norm — the same formulation speaker_asnorm.rs implements
# ---------------------------------------------------------------------------


def as_norm(raw, enroll_cohort_scores, test_cohort_scores, top_k):
    """Symmetric adaptive score normalization (Matejka et al., Interspeech 2017).

    Each side is normalized by the mean/std of its top-K cohort scores, and the
    two z-scores are averaged. `tests/as_norm_matches_the_generator.rs` holds the
    Rust implementation to hand-worked cases of this same formula, so the scores
    measured here and the scores the app computes cannot drift apart without a
    test going red.
    """
    def side(scores):
        top = np.sort(scores)[::-1][:top_k]
        mu, sd = float(np.mean(top)), float(np.std(top))
        return 0.0 if sd < 1e-6 else (raw - mu) / sd

    return 0.5 * (side(enroll_cohort_scores) + side(test_cohort_scores))


def eer(genuine, impostor):
    """Equal error rate, decision rule `accept if score >= threshold`.

    Mirrors YV120's `enrollment_eer` so this printout and the Rust gate agree.
    The Rust one is authoritative — it is the one a test fails on.
    """
    obs = sorted(set(list(genuine) + list(impostor)))
    cands = [obs[0] - 1e-6]
    for a, b in zip(obs, obs[1:]):
        cands += [a, (a + b) / 2]
    cands += [obs[-1], obs[-1] + 1e-6]
    best, best_gap = 1.0, float("inf")
    for th in cands:
        far = sum(1 for s in impostor if s >= th) / len(impostor)
        frr = sum(1 for s in genuine if s < th) / len(genuine)
        if abs(far - frr) < best_gap - 1e-12:
            best_gap, best = abs(far - frr), (far + frr) / 2
    return best


# ---------------------------------------------------------------------------


def speakers(base, subset):
    return sorted(int(d.name) for d in (base / subset).iterdir() if d.is_dir())


def segment(base, subset, spk, seconds, skip=0):
    import soundfile as sf

    files = sorted((base / subset / str(spk)).rglob("*.flac"))
    out, total, i = [], 0.0, skip
    while total < seconds and i < len(files):
        x, sr = sf.read(files[i], dtype="float32")
        if sr != SR:
            raise SystemExit(f"{files[i]}: expected {SR} Hz")
        out.append(x)
        total += len(x) / SR
        i += 1
    if total < seconds - 0.5:
        return None, i
    return np.ascontiguousarray(np.concatenate(out)[: int(seconds * SR)]), i


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--model", required=True, type=pathlib.Path)
    ap.add_argument("--librispeech", required=True, type=pathlib.Path)
    ap.add_argument("--repo", type=pathlib.Path, default=pathlib.Path(__file__).resolve().parents[1])
    args = ap.parse_args()

    digest = hashlib.sha256(args.model.read_bytes()).hexdigest()
    if digest != CAMPP_SHA256:
        raise SystemExit(f"model digest {digest} is not the pinned CAM++ from catalog.json")

    ex = Embedder(args.model)
    print(f"CAM++ loaded, dim = {ex.dim}, sherpa-onnx {ex.version}")
    rng = np.random.default_rng(20260816)

    # --- cohort -----------------------------------------------------------
    cohort_ids, cohort_rows = [], []
    for spk in speakers(args.librispeech, COHORT_SUBSET):
        aud, _ = segment(args.librispeech, COHORT_SUBSET, spk, SEGMENT_SECONDS)
        if aud is None:
            continue
        for cond in COHORT_CONDITIONS:
            wav = aud if cond == "clean" else rbj_lowpass(rbj_highpass(aud, 300.0), 3400.0)
            cohort_ids.append({"speaker": spk, "condition": cond})
            cohort_rows.append(l2(ex.embed(wav)))
    cohort_m = np.stack(cohort_rows)
    n_spk = len(cohort_rows) // len(COHORT_CONDITIONS)
    print(f"cohort: {len(cohort_rows)} entries ({n_spk} speakers x {len(COHORT_CONDITIONS)} conditions)")

    # A cohort with duplicate members has fewer members than it claims, and the
    # duplicates are exactly what adaptive top-K would select. Distinct
    # LibriSpeech speaker ids make same-speaker duplication impossible by
    # construction; this asserts it rather than assuming it, and would catch a
    # corpus laid out differently than expected.
    worst, worst_pair = 0.0, None
    for i in range(len(cohort_rows)):
        for j in range(i + 1, len(cohort_rows)):
            if cohort_ids[i]["speaker"] == cohort_ids[j]["speaker"]:
                continue  # one voice under two conditions, on purpose
            s = float(np.dot(cohort_rows[i], cohort_rows[j]))
            if s > worst:
                worst, worst_pair = s, (cohort_ids[i], cohort_ids[j])
    print(f"  max cross-speaker cohort similarity = {worst:.4f}")
    if worst > 0.97:
        raise SystemExit(f"two cohort speakers are {worst:.4f} similar: {worst_pair}")

    payload = cohort_m.astype("<f4").tobytes()
    assets = args.repo / "desktop/src-tauri/assets"
    assets.mkdir(parents=True, exist_ok=True)
    (assets / "yv131-impostor-cohort.bin").write_bytes(payload)
    manifest = {
        "asset": "yv131-impostor-cohort.bin",
        "sha256": hashlib.sha256(payload).hexdigest(),
        "bytes": len(payload),
        "count": len(cohort_rows),
        "dim": ex.dim,
        "layout": "count * dim little-endian f32, row-major, each row L2-normalized",
        "top_k": TOP_K,
        "embedder": {
            "model_id": "wespeaker-en-voxceleb-campplus",
            "model_sha256": CAMPP_SHA256,
            "extractor": f"sherpa-onnx {ex.version} SpeakerEmbeddingExtractor",
        },
        "source": {
            "corpus": "LibriSpeech ASR corpus",
            "subset": COHORT_SUBSET,
            "url": "https://www.openslr.org/12",
            "license": "CC-BY-4.0",
            "attribution": "LibriSpeech (c) 2014 Vassil Panayotov, CC-BY-4.0",
            "note": "512-float embeddings only; no audio is redistributed and an embedding is not invertible to one.",
            "segment_seconds": SEGMENT_SECONDS,
        },
        "conditions": list(COHORT_CONDITIONS),
        "entries": cohort_ids,
        "max_cross_speaker_similarity": round(worst, 6),
        "generator": "scripts/yv131-build-impostor-cohort.py",
    }
    (assets / "yv131-impostor-cohort.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(f"wrote cohort asset: {len(payload)} bytes")

    # --- measurement ------------------------------------------------------
    def build_split(subset):
        enroll, tests = {}, {}
        for spk in speakers(args.librispeech, subset):
            aud, nxt = segment(args.librispeech, subset, spk, SEGMENT_SECONDS)
            if aud is None:
                continue
            raws = []
            for _ in range(TEST_SEGMENTS):
                seg, nxt = segment(args.librispeech, subset, spk, SEGMENT_SECONDS, skip=nxt)
                if seg is None:
                    break
                raws.append(seg)
            if not raws:
                continue
            enroll[spk] = l2(ex.embed(aud))
            tests[spk] = {c: [l2(ex.embed(fn(g, rng))) for g in raws] for c, fn in CHANNELS.items()}
        return enroll, tests

    ladder, trials_out = {}, None
    for subset in (TUNE_SUBSET, REPORT_SUBSET):
        enroll, tests = build_split(subset)
        ids = sorted(enroll)
        print(f"{subset}: {len(ids)} enrolled speakers")
        ce = {s: cohort_m @ enroll[s] for s in ids}
        for chan in CHANNELS:
            raw_g, raw_i, as_g, as_i = [], [], [], []
            for truth in ids:
                for t in tests[truth][chan]:
                    ct = cohort_m @ t
                    for cand in ids:
                        raw = float(np.dot(enroll[cand], t))
                        adj = as_norm(raw, ce[cand], ct, TOP_K)
                        (raw_g if cand == truth else raw_i).append(raw)
                        (as_g if cand == truth else as_i).append(adj)
            ladder[f"{subset}|{chan}"] = {
                "raw_eer": eer(raw_g, raw_i),
                "as_norm_eer": eer(as_g, as_i),
                "genuine": len(raw_g),
                "impostor": len(raw_i),
            }
            if subset == REPORT_SUBSET and chan == REPORT_CHANNEL:
                trials_out = {
                    "raw_genuine": raw_g, "raw_impostor": raw_i,
                    "as_norm_genuine": as_g, "as_norm_impostor": as_i,
                }

    print(f"\n{'split|channel':44s} {'raw EER':>9s} {'AS-norm EER':>12s}")
    for k, v in ladder.items():
        print(f"{k:44s} {v['raw_eer']*100:8.2f}% {v['as_norm_eer']*100:11.2f}%")

    out = {
        "note": (
            "Measured by scripts/yv131-build-impostor-cohort.py on real CAM++ embeddings of "
            "LibriSpeech speech. Channels B1-B4 are SIMULATED and defined in that script's "
            "CHANNELS table; no AirPods and no human volunteer were recorded. The design "
            f"(symmetric AS-norm, K={TOP_K}, {'+'.join(COHORT_CONDITIONS)} cohort) was chosen on "
            f"{TUNE_SUBSET} and is reported on {REPORT_SUBSET}, which was not consulted until the "
            "design was frozen."
        ),
        "top_k": TOP_K,
        "cohort_entries": len(cohort_rows),
        "dim": ex.dim,
        "tune_subset": TUNE_SUBSET,
        "report_subset": REPORT_SUBSET,
        "report_channel": REPORT_CHANNEL,
        "ladder": ladder,
        "report_trials": trials_out,
    }
    (args.repo / "docs/yap23-asnorm-measurement.json").write_text(json.dumps(out) + "\n")
    print("wrote docs/yap23-asnorm-measurement.json")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
