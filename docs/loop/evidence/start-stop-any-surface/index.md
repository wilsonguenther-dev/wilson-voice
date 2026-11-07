# Start from any surface, stop from any other — capture sheet

PENDING INTEGRATOR. This file is regenerated (with the 48 frames) by:

    node scripts/start-stop-any-surface-capture.mjs

which runs `cargo test --lib take_control` (the 4x4 matrix, the hotkey-hold row,
stop during warm-up, double stop) and photographs the pill at 2x before start,
while recording and after stop, for each of the 16 start x stop pairs.

| start | stop | frames | result |
|---|---|---|---|
| pill | pill | pending | PENDING |
| pill | hotkey | pending | PENDING |
| pill | menu_bar | pending | PENDING |
| pill | dictate_button | pending | PENDING |
| hotkey | pill | pending | PENDING |
| hotkey | hotkey | pending | PENDING |
| hotkey | menu_bar | pending | PENDING |
| hotkey | dictate_button | pending | PENDING |
| menu_bar | pill | pending | PENDING |
| menu_bar | hotkey | pending | PENDING |
| menu_bar | menu_bar | pending | PENDING |
| menu_bar | dictate_button | pending | PENDING |
| dictate_button | pill | pending | PENDING |
| dictate_button | hotkey | pending | PENDING |
| dictate_button | menu_bar | pending | PENDING |
| dictate_button | dictate_button | pending | PENDING |
