from wilson_voice.paste import copy_text


def test_copy_text_ok():
    assert copy_text("wilson-voice-test-clipboard") is True


def test_copy_empty():
    assert copy_text("") is True
