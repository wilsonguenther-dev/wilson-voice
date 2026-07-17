"""Offline transcript polish + intent extraction (no network)."""
from __future__ import annotations

import re

_FILLERS = [
    r"\b(um|uh|erm|hmm|ah+|eh+)\b",
    r"\byou know\b",
    r"\bi mean\b",
    r"\bkind of\b",
    r"\bsort of\b",
    r"\bbasically\b",
    r"\bliterally\b",
]


def polish(text: str) -> str:
    if not text:
        return ""
    t = text.strip()
    for pat in _FILLERS:
        t = re.sub(pat, "", t, flags=re.IGNORECASE)
    # Collapse whitespace / fix spaces before punct
    t = re.sub(r"\s{2,}", " ", t)
    t = re.sub(r"\s+([,.!?;:])", r"\1", t)
    t = re.sub(r"([.!?])\s*([a-z])", lambda m: m.group(1) + " " + m.group(2).upper(), t)
    if t and t[0].islower():
        t = t[0].upper() + t[1:]
    # Common ASR fixes for coding dictation
    replacements = {
        r"\bgit hub\b": "GitHub",
        r"\bvs code\b": "VS Code",
        r"\btype script\b": "TypeScript",
        r"\bjava script\b": "JavaScript",
        r"\bnext j s\b": "Next.js",
        r"\bsuper base\b": "Supabase",
        r"\bvercel\b": "Vercel",
        r"\bcodex\b": "Codex",
        r"\bclaude\b": "Claude",
        r"\bgrok\b": "Grok",
    }
    for pat, rep in replacements.items():
        t = re.sub(pat, rep, t, flags=re.IGNORECASE)
    return t.strip()


def extract_intent(text: str) -> str:
    t = (text or "").strip()
    if not t:
        return ""
    low = t.lower()
    if low.startswith(("can you", "could you", "please", "i need you to", "i want you to")):
        return "request"
    if low.startswith(("what", "why", "how", "when", "where", "who", "which", "is ", "are ")):
        return "question"
    if any(low.startswith(x) for x in ("fix", "build", "create", "make", "deploy", "ship", "open", "run", "test", "commit", "push")):
        return "action"
    if low.startswith(("don't", "do not", "never", "stop", "avoid")):
        return "constraint"
    return "statement"
