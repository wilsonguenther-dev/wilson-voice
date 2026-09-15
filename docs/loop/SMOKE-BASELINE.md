# The windowed smoke baseline

`scripts/smoke-windowed.sh` (Y0-E) is the only instrument in this plan that can
see "looks broken". The merge gate is eight headless commands — tsc, vitest,
vite, two cargo builds, two cargo test runs — and not one of them can tell a
laid-out view from a blank one. This file records what the instrument says
about the tree it shipped on, because **a gate whose first run is green has
never demonstrated that it can say no.**

Run recorded here: `2026-09-15T03-20-04-517Z`
Captures: 14 (7 Nav views x 2 window sizes) — **10 carry findings.**

## The five structural conditions

A capture FAILS on any of these. None of them is pixel equality against a
golden image, on purpose: a legitimate restyle must never have to relitigate a
PNG, so every condition is a fact about the DOM or the layout box tree.

| id | condition |
|---|---|
| `no-text` | the view's content region renders no text at all |
| `spinner-stuck` | a spinner is still present after 10 s |
| `doc-hscroll` | the document has a horizontal scrollbar |
| `out-of-bounds` | an element's box falls outside the window bounds |
| `silent-empty` | the view renders zero rows AND carries no `data-empty-state` |

## The baseline — today's tree

| view | size | rows | chars | findings |
|---|---|---|---|---|
| home | 980x700 | 0 | 155 | silent-empty |
| permissions | 980x700 | 6 | 1167 | - |
| meetings | 980x700 | 0 | 188 | silent-empty |
| insights | 980x700 | 6 | 496 | - |
| dictionary | 980x700 | 0 | 145 | silent-empty |
| scratchpad | 980x700 | 0 | 20 | silent-empty |
| settings | 980x700 | 0 | 784 | silent-empty |
| home | 720x520 | 0 | 155 | silent-empty |
| permissions | 720x520 | 6 | 1167 | - |
| meetings | 720x520 | 0 | 188 | silent-empty |
| insights | 720x520 | 6 | 496 | - |
| dictionary | 720x520 | 0 | 145 | silent-empty |
| scratchpad | 720x520 | 0 | 20 | silent-empty |
| settings | 720x520 | 0 | 784 | silent-empty |

### What the red actually says

**`silent-empty` on 10 of 14 captures.** `data-empty-state` does not appear
anywhere in `desktop/src` today — `git grep -c data-empty-state -- desktop/src`
returns nothing. So five of the seven views (home, meetings, dictionary,
scratchpad, settings) render with zero rows and no machine-readable statement
that emptiness is the intended state. For a human that is the difference
between "you have no meetings yet" and a screen that failed to load, and right
now the markup cannot tell those apart either.

Fixing this is the UI lane's job, one `data-empty-state` at a time. **Do not
weaken the detector to make this green.** The two views that pass —
`permissions` and `insights` — pass because they render real rows, which is
exactly the distinction the condition is meant to draw.

**`scratchpad` renders 20 characters at both sizes.** It clears `no-text` and
nothing else fires, but 20 characters is a heading and little else. It is worth
a human look during the Y5 lane; the instrument reports it rather than judging
it, because "too little text" is a taste call and these conditions are not.

## Non-vacuity

A detector nobody has ever seen fire is a decoration.
`./scripts/smoke-windowed.sh --self-test-must-fail` injects one synthetic view
that violates all five conditions at once and requires the detector to report
every one of them:

```
self-test: synthetic broken view injected (no text / stuck spinner / 4000px row / zero rows / no [data-empty-state])
  detector fired: no-text — content region has zero characters of rendered text
  detector fired: spinner-stuck — 1 spinner/skeleton element(s) still visible after 10000 ms: div.spinner
  detector fired: doc-hscroll — documentElement.scrollWidth=4000 > clientWidth=980
  detector fired: out-of-bounds — box(es) outside the 980x700 window: div [0,0 4000x20]
  detector fired: silent-empty — zero rows rendered and no [data-empty-state] element — the view is empty without saying so
SELF_TEST=satisfied
```

The mode is named `must-fail` because the run it performs contains a deliberate
break, so a **satisfied** self-test still exits non-zero (`1`). A detector that
stays blind exits `9` and prints `SELF_TEST=vacuous`. Both are non-zero; only
one of them is good news, and the last line says which.

This was not theatre. On its first run the self-test caught `doc-hscroll`
never firing — the synthetic overflow element was `position:absolute`, so it
never contributed to `documentElement.scrollWidth`. The fixture was wrong, not
the detector. That is the bug class this mode exists to catch.

## What is driven, precisely

The smoke loads the **same `desktop/dist` bundle the packaged app loads**
(`tauri.conf.json` `frontendDist` = `"../dist"`), served by `vite preview`,
inside headless Chrome at exact viewport sizes, over the Chrome DevTools
Protocol. It is **not** the WKWebView: WKWebView has no scriptable inspector on
macOS — Safari Web Inspector cannot be driven from a script — so there is no way
to ask the native shell's webview for a layout box.

`window.__TAURI_INTERNALS__` is therefore shimmed, and its answers come from
`scripts/smoke-fixtures.json`. Only commands whose shape is **certain** live
there; everything else rejects, which leaves each component on its own
well-formed initial state — the unfed shape these conditions are meant to
judge. Seeding a guessed shape is worse than rejecting: seeding `get_settings`
with `{}` crashed the whole app with `Cannot read properties of undefined
(reading 'map')` and produced a wall of fake `no-text` findings on every view.

`get_settings`, `get_status`, `get_insights` and `get_permissions` must be
present, because `refreshAll()` awaits them in one `Promise.all` and App.tsx
gates the entire UI on the result (`if (bootError && !settings) return <boot
error screen>`). Their fixtures were generated mechanically from the
`AppSettings`, `AppStatus`, `Insights` and `PermissionReport` interfaces in
`desktop/src/App.tsx`, not hand-guessed.

**When those interfaces gain a required field, this fixture goes stale.** The
driver detects that itself: it recognises the boot-error screen and aborts with
`FIXTURE STALE`, naming the missing command, rather than reporting a wall of
convincing structural findings about an app that never rendered. Verified — the
guard is what produced `FIXTURE STALE: ... no fixture for get_status` while this
harness was being built.

### What the harness cannot see

- **Native window chrome.** Title bar, traffic lights, window minimum size and
  offscreen placement are the shell's, not the bundle's.
- **The floating pill.** `float.html` is a second Vite entry in its own native
  window. Y7-B keeps that, along with the new pill phases and dock positions,
  once Y5 has landed them.
- **Anything gated on a real backend response.** A view that only renders rows
  after a successful `invoke` shows its unfed shape here. That is deliberate,
  and it is the shape `data-empty-state` is supposed to describe.

## Modes

| command | does |
|---|---|
| `npm run smoke:windowed` (in `desktop/`) | build `dist`, serve it, walk 7 views x 2 sizes, capture, judge |
| `./scripts/smoke-windowed.sh --check-only` | re-assert the last capture; **exits non-zero when no run exists** |
| `./scripts/smoke-windowed.sh --self-test-must-fail` | prove the detector is non-vacuous |

`docs/smoke-shots/` is gitignored: a PNG is evidence of one run on one machine,
not source. That is why `--check-only` fails on a fresh checkout, which is the
correct answer — it is a pre-flight convenience, never a substitute for a
capture.
