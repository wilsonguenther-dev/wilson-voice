"""Polish + intent unit tests (fast, offline)."""
from wilson_voice.polish import extract_intent, polish


def test_polish_empty():
    assert polish("") == ""
    assert polish("   ") == ""


def test_polish_strips_um():
    assert "um" not in polish("um hello there").lower().split()


def test_polish_strips_uh():
    t = polish("I uh think so")
    assert "uh" not in t.lower().split()


def test_polish_you_know():
    t = polish("this is, you know, fine")
    assert "you know" not in t.lower()


def test_polish_collapses_space():
    assert "  " not in polish("hello    world")


def test_polish_capitalizes():
    t = polish("hello world")
    assert t[0].isupper()


def test_polish_github():
    assert "GitHub" in polish("push to git hub now")


def test_polish_typescript():
    assert "TypeScript" in polish("rewrite in type script")


def test_polish_claude():
    assert "Claude" in polish("ask claude to fix it")


def test_polish_codex():
    assert "Codex" in polish("open codex please")


def test_polish_nextjs():
    assert "Next.js" in polish("deploy next j s app")


def test_polish_supabase():
    assert "Supabase" in polish("query super base")


def test_polish_preserves_code_words():
    t = polish("Run npm run build")
    assert "npm" in t.lower() or "Npm" in t or "npm" in t


def test_intent_request():
    assert extract_intent("can you fix the button") == "request"


def test_intent_please():
    assert extract_intent("please deploy to production") == "request"


def test_intent_question():
    assert extract_intent("what is the status of the job") == "question"


def test_intent_how():
    assert extract_intent("how does this work") == "question"


def test_intent_action_fix():
    assert extract_intent("fix the login page") == "action"


def test_intent_action_build():
    assert extract_intent("build the whisper app") == "action"


def test_intent_action_commit():
    assert extract_intent("commit and push") == "action"


def test_intent_constraint():
    assert extract_intent("never contact josh") == "constraint"


def test_intent_statement():
    assert extract_intent("The server is running fine.") == "statement"


def test_intent_empty():
    assert extract_intent("") == ""


def test_polish_double_punct():
    t = polish("hello , world")
    assert " ," not in t
