# Start from any surface, stop from any other — capture sheet

Generated 2026-11-07T17:38:27.581Z by scripts/start-stop-any-surface-capture.mjs (yap-1107-start-stop-any-surface).

Headless matrix `cargo test --lib take_control`: exit 0. It covers all 16 start x stop pairs, the hotkey-hold row, a stop during warm-up and a double stop (acceptance lines 2 and 4).
Frames are the pill at Retina 2x in headless Chrome (scripts/pill-matrix.mjs): before start (idle), recording (listening), after stop (idle). Acceptance line 3.

| start | stop | frames | result |
|---|---|---|---|
| pill | pill | [before@2x.png](pill-to-pill-1-before@2x.png) · [recording@2x.png](pill-to-pill-2-recording@2x.png) · [after@2x.png](pill-to-pill-3-after@2x.png) | PASS |
| pill | hotkey | [before@2x.png](pill-to-hotkey-1-before@2x.png) · [recording@2x.png](pill-to-hotkey-2-recording@2x.png) · [after@2x.png](pill-to-hotkey-3-after@2x.png) | PASS |
| pill | menu_bar | [before@2x.png](pill-to-menu_bar-1-before@2x.png) · [recording@2x.png](pill-to-menu_bar-2-recording@2x.png) · [after@2x.png](pill-to-menu_bar-3-after@2x.png) | PASS |
| pill | dictate_button | [before@2x.png](pill-to-dictate_button-1-before@2x.png) · [recording@2x.png](pill-to-dictate_button-2-recording@2x.png) · [after@2x.png](pill-to-dictate_button-3-after@2x.png) | PASS |
| hotkey | pill | [before@2x.png](hotkey-to-pill-1-before@2x.png) · [recording@2x.png](hotkey-to-pill-2-recording@2x.png) · [after@2x.png](hotkey-to-pill-3-after@2x.png) | PASS |
| hotkey | hotkey | [before@2x.png](hotkey-to-hotkey-1-before@2x.png) · [recording@2x.png](hotkey-to-hotkey-2-recording@2x.png) · [after@2x.png](hotkey-to-hotkey-3-after@2x.png) | PASS |
| hotkey | menu_bar | [before@2x.png](hotkey-to-menu_bar-1-before@2x.png) · [recording@2x.png](hotkey-to-menu_bar-2-recording@2x.png) · [after@2x.png](hotkey-to-menu_bar-3-after@2x.png) | PASS |
| hotkey | dictate_button | [before@2x.png](hotkey-to-dictate_button-1-before@2x.png) · [recording@2x.png](hotkey-to-dictate_button-2-recording@2x.png) · [after@2x.png](hotkey-to-dictate_button-3-after@2x.png) | PASS |
| menu_bar | pill | [before@2x.png](menu_bar-to-pill-1-before@2x.png) · [recording@2x.png](menu_bar-to-pill-2-recording@2x.png) · [after@2x.png](menu_bar-to-pill-3-after@2x.png) | PASS |
| menu_bar | hotkey | [before@2x.png](menu_bar-to-hotkey-1-before@2x.png) · [recording@2x.png](menu_bar-to-hotkey-2-recording@2x.png) · [after@2x.png](menu_bar-to-hotkey-3-after@2x.png) | PASS |
| menu_bar | menu_bar | [before@2x.png](menu_bar-to-menu_bar-1-before@2x.png) · [recording@2x.png](menu_bar-to-menu_bar-2-recording@2x.png) · [after@2x.png](menu_bar-to-menu_bar-3-after@2x.png) | PASS |
| menu_bar | dictate_button | [before@2x.png](menu_bar-to-dictate_button-1-before@2x.png) · [recording@2x.png](menu_bar-to-dictate_button-2-recording@2x.png) · [after@2x.png](menu_bar-to-dictate_button-3-after@2x.png) | PASS |
| dictate_button | pill | [before@2x.png](dictate_button-to-pill-1-before@2x.png) · [recording@2x.png](dictate_button-to-pill-2-recording@2x.png) · [after@2x.png](dictate_button-to-pill-3-after@2x.png) | PASS |
| dictate_button | hotkey | [before@2x.png](dictate_button-to-hotkey-1-before@2x.png) · [recording@2x.png](dictate_button-to-hotkey-2-recording@2x.png) · [after@2x.png](dictate_button-to-hotkey-3-after@2x.png) | PASS |
| dictate_button | menu_bar | [before@2x.png](dictate_button-to-menu_bar-1-before@2x.png) · [recording@2x.png](dictate_button-to-menu_bar-2-recording@2x.png) · [after@2x.png](dictate_button-to-menu_bar-3-after@2x.png) | PASS |
| dictate_button | dictate_button | [before@2x.png](dictate_button-to-dictate_button-1-before@2x.png) · [recording@2x.png](dictate_button-to-dictate_button-2-recording@2x.png) · [after@2x.png](dictate_button-to-dictate_button-3-after@2x.png) | PASS |
