//! yap-call-2 — browser calls in Chromium-family browsers, told apart by the
//! front tab's window title. Every case runs the real `call_titles::read_titles`
//! (with a fake AX reader and a collecting log sink) into the real
//! `call_signals::score` / `offer_copy`, so no Mac and no Accessibility grant
//! is needed.

use wilson_voice_lib::call_signals::{
    catalog, offer_copy, score, AxSource, CalendarHint, CallMemory, CallSnapshot, ProcAudio,
    RunningApp, TitleObs, Via,
};
use wilson_voice_lib::call_titles::{
    is_chromium_browser, log_line, read_titles, title_candidates, AX_OFF_LINE, FRONT_TAB_LINE,
    SITE_UNKNOWN_COPY, SITE_UNKNOWN_NAME,
};

const YAP: &str = "com.wilsonguenther.wilson-voice";

fn proc(pid: i32, ppid: i32, bundle: &str, input: bool) -> ProcAudio {
    ProcAudio {
        pid,
        parent_pid: Some(ppid),
        bundle_id: Some(bundle.to_string()),
        running_input: Some(input),
        running_output: Some(true),
    }
}

/// Yap idle, plus `browser` (pid 2000) whose helper (pid 2001) holds the mic
/// when `mic` is true.
fn browser_snap(browser: &str, helper: &str, mic: bool) -> CallSnapshot {
    CallSnapshot {
        synthetic: true,
        own_pid: 900,
        procs: vec![
            ProcAudio {
                pid: 900,
                parent_pid: Some(1),
                bundle_id: Some(YAP.to_string()),
                running_input: Some(false),
                running_output: Some(false),
            },
            proc(2001, 2000, helper, mic),
        ],
        running_apps: vec![RunningApp {
            pid: 2000,
            bundle_id: browser.to_string(),
        }],
        frontmost: Some(browser.to_string()),
        browser_titles: None,
        ax_source: AxSource::Yap,
        calendar: None,
    }
}

fn chrome(mic: bool) -> CallSnapshot {
    browser_snap("com.google.Chrome", "com.google.Chrome.helper", mic)
}

/// Read `titles` as the front-tab titles of every candidate; returns the
/// snapshot with titles filled, the pids read, and the log lines.
fn observe(
    mut snap: CallSnapshot,
    trusted: bool,
    titles: &[&str],
) -> (CallSnapshot, Vec<i32>, Vec<String>) {
    let mut read_pids = Vec::new();
    let mut lines = Vec::new();
    let owned: Vec<String> = titles.iter().map(|t| t.to_string()).collect();
    snap.browser_titles = read_titles(
        &snap,
        catalog(),
        trusted,
        false,
        |pid| {
            read_pids.push(pid);
            owned.clone()
        },
        |line| lines.push(line),
    );
    (snap, read_pids, lines)
}

fn offer_text(snap: &CallSnapshot) -> Option<String> {
    score(snap, catalog(), &CallMemory::default())
        .filter(|c| c.offers())
        .map(|c| offer_copy(&c, catalog()))
}

#[test]
fn call_browser_titles_fixture_titles_match_each_service() {
    // (front-tab title, pattern id, offer text)
    let table: &[(&str, &str, &str)] = &[
        (
            "Meet - abc-defg-hij",
            "meet",
            "Google Meet call — record it?",
        ),
        (
            "meet.google.com/abc-defg-hij",
            "meet",
            "Google Meet call — record it?",
        ),
        ("Zoom Meeting", "zoom_web", "Zoom call — record it?"),
        (
            "Weekly sync | Microsoft Teams",
            "teams_web",
            "Microsoft Teams call — record it?",
        ),
        (
            "teams.microsoft.com",
            "teams_web",
            "Microsoft Teams call — record it?",
        ),
        (
            "Huddle: #eng - Acme - Slack",
            "slack_huddle",
            "Slack call — record it?",
        ),
        (
            "#general | Acme - Discord",
            "discord_web",
            "Discord call — record it?",
        ),
        (
            "Standup room - Whereby",
            "whereby",
            "Whereby call — record it?",
        ),
        (
            "Cisco Webex Meeting",
            "webex_web",
            "Webex call — record it?",
        ),
        ("Around | standup", "around", "Around call — record it?"),
        // Case-insensitive.
        (
            "MEET - ABC-DEFG-HIJ",
            "meet",
            "Google Meet call — record it?",
        ),
    ];
    let browsers = [
        ("com.google.Chrome", "com.google.Chrome.helper"),
        ("com.microsoft.edgemac", "com.microsoft.edgemac.helper"),
        ("com.brave.Browser", "com.brave.Browser.helper"),
        (
            "company.thebrowser.Browser",
            "company.thebrowser.browser.helper",
        ),
    ];
    for (browser, helper) in browsers {
        for (title, id, text) in table {
            let (snap, read, _) = observe(browser_snap(browser, helper, true), true, &[title]);
            assert_eq!(
                read,
                vec![2000],
                "{browser}: the browser holding the mic is read once"
            );
            let obs = snap.browser_titles.as_ref().expect("AX granted");
            assert_eq!(obs.len(), 1);
            assert_eq!(
                obs[0].pattern_id.as_deref(),
                Some(*id),
                "{browser}: {title}"
            );
            assert_eq!(obs[0].len, title.chars().count());
            let c = score(&snap, catalog(), &CallMemory::default()).expect("candidate");
            assert_eq!(c.bundle_id, browser);
            assert_eq!(c.via, Via::HelperParent);
            assert_eq!(
                offer_text(&snap).as_deref(),
                Some(*text),
                "{browser}: {title}"
            );
        }
    }
}

#[test]
fn call_browser_titles_every_pattern_ships_unverified_and_chromium_rows_carry_all_services() {
    let mut chromium = 0;
    for app in &catalog().apps {
        for p in &app.title_patterns {
            assert!(
                !p.verified,
                "{} {}: verified only by a Wilson walk",
                app.bundle_id, p.id
            );
        }
        if is_chromium_browser(app) {
            chromium += 1;
            for id in [
                "meet",
                "zoom_web",
                "teams_web",
                "slack_huddle",
                "discord_web",
                "whereby",
                "webex_web",
                "around",
            ] {
                assert!(
                    app.title_patterns.iter().any(|p| p.id == id),
                    "{} lacks {id}",
                    app.bundle_id
                );
            }
        }
    }
    assert_eq!(chromium, 4, "Chrome, Edge, Brave, Arc");
    assert!(!is_chromium_browser(
        catalog().app("com.apple.Safari").unwrap()
    ));
}

#[test]
fn call_browser_titles_ax_off_is_site_unknown_and_needs_a_calendar_match() {
    let (snap, read, lines) = observe(chrome(true), false, &["Meet - abc-defg-hij"]);
    assert_eq!(snap.browser_titles, None, "AX off = titles None");
    assert!(read.is_empty(), "nothing is read without Accessibility");
    assert!(
        lines.iter().any(|l| l.contains("site=unknown")),
        "{lines:?}"
    );
    // No calendar match: no offer.
    assert_eq!(offer_text(&snap), None);
    // A calendar event with a call link in its window: "site unknown" offer.
    let mut snap = snap;
    snap.calendar = Some(CalendarHint {
        linked_event_in_window: true,
    });
    assert_eq!(offer_text(&snap).as_deref(), Some(SITE_UNKNOWN_COPY));
    assert!(SITE_UNKNOWN_COPY.starts_with(SITE_UNKNOWN_NAME));
    assert!(SITE_UNKNOWN_NAME.contains("site unknown"));
}

#[test]
fn call_browser_titles_youtube_with_mic_open_never_offers() {
    let (mut snap, read, _) = observe(chrome(true), true, &["Lo-fi beats to study to - YouTube"]);
    assert_eq!(read, vec![2000]);
    assert_eq!(
        snap.browser_titles.as_ref().unwrap()[0].pattern_id,
        None,
        "YouTube is no call site"
    );
    // Even frontmost with a calendar match it stays under the threshold.
    snap.calendar = Some(CalendarHint {
        linked_event_in_window: true,
    });
    assert_eq!(offer_text(&snap), None);
}

#[test]
fn call_browser_titles_meet_in_a_background_tab_is_not_offered() {
    // The Meet tab is open but in the BACKGROUND: the window's title is the
    // front tab (Gmail), so that is all Yap can read.
    let (mut snap, _, _) = observe(chrome(true), true, &["Inbox (3) - Gmail"]);
    snap.calendar = Some(CalendarHint {
        linked_event_in_window: true,
    });
    assert_eq!(
        offer_text(&snap),
        None,
        "a background-tab call is the safe failure"
    );
    // Bring the Meet tab to the front: now it is offered.
    let (snap, _, _) = observe(chrome(true), true, &["Meet - abc-defg-hij"]);
    assert_eq!(
        offer_text(&snap).as_deref(),
        Some("Google Meet call — record it?")
    );
    assert!(FRONT_TAB_LINE.contains("front tab"));
}

#[test]
fn call_browser_titles_browser_without_the_mic_is_never_read() {
    // Chrome holds no mic: no candidate, no AX read at all.
    let (snap, read, lines) = observe(chrome(false), true, &["Meet - abc-defg-hij"]);
    assert!(
        read.is_empty(),
        "titles are read only for a browser holding the mic"
    );
    assert!(lines.is_empty());
    assert_eq!(snap.browser_titles, Some(Vec::new()));
    assert_eq!(offer_text(&snap), None);
    // Yap's own input never makes a browser a candidate.
    let mut s = chrome(false);
    s.procs[0].running_input = Some(true);
    assert!(title_candidates(&s, catalog()).is_empty());
}

#[test]
fn call_browser_titles_safari_webkit_gpu_input_never_offers() {
    let mut snap = CallSnapshot {
        synthetic: true,
        own_pid: 900,
        procs: vec![proc(3001, 1, "com.apple.WebKit.GPU", true)],
        running_apps: vec![RunningApp {
            pid: 3000,
            bundle_id: "com.apple.Safari".to_string(),
        }],
        frontmost: Some("com.apple.Safari".to_string()),
        browser_titles: None,
        ax_source: AxSource::Yap,
        calendar: Some(CalendarHint {
            linked_event_in_window: true,
        }),
    };
    assert!(
        title_candidates(&snap, catalog()).is_empty(),
        "Safari is never read"
    );
    assert_eq!(offer_text(&snap), None, "AX off");
    // Even a Safari title match (from an older probe) is deferred.
    snap.browser_titles = Some(vec![TitleObs {
        bundle_id: "com.apple.Safari".to_string(),
        pattern_id: Some("meet".to_string()),
        len: 19,
        raw_title: None,
    }]);
    assert_eq!(score(&snap, catalog(), &CallMemory::default()), None);
}

#[test]
fn call_browser_titles_a_title_is_never_written_to_the_log_or_the_db() {
    let secret = "Meet - Q4 layoffs with Dana Whitfield";
    let (snap, _, lines) = observe(chrome(true), true, &[secret, "Payroll export - Sheets"]);
    // The log sink: pattern id + length only.
    assert!(!lines.is_empty());
    for l in &lines {
        assert!(!l.contains("layoffs"), "title text in the log: {l}");
        assert!(!l.contains("Dana"), "title text in the log: {l}");
        assert!(!l.contains("Payroll"), "title text in the log: {l}");
    }
    let obs = snap.browser_titles.as_ref().unwrap();
    assert_eq!(
        log_line(&obs[0]),
        format!(
            "call_titles observed bundle=com.google.Chrome pattern=meet len={}",
            secret.chars().count()
        )
    );
    assert!(lines.contains(&log_line(&obs[1])));
    assert!(lines.iter().any(|l| l.contains("pattern=none")));
    // What could reach the DB or a fixture: the snapshot and the candidate,
    // serialized. No text, no raw_title key.
    assert!(obs.iter().all(|o| o.raw_title.is_none()));
    let json = serde_json::to_string(&snap).unwrap();
    assert!(
        !json.contains("layoffs") && !json.contains("Payroll"),
        "{json}"
    );
    assert!(!json.contains("raw_title"));
    let c = score(&snap, catalog(), &CallMemory::default()).unwrap();
    let cj = serde_json::to_string(&c).unwrap();
    assert!(!cj.contains("layoffs"), "{cj}");
    // And the module's own log macros never format a title.
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/call_titles.rs"),
    )
    .unwrap();
    for line in src.lines().filter(|l| l.contains("log::")) {
        let rest = line.replace("call_titles", "");
        assert!(
            !rest.contains("title"),
            "a log line formats a title: {line}"
        );
    }
}

#[test]
fn call_browser_titles_reads_titles_only_never_urls_or_window_lists() {
    let src = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/call_titles.rs"),
    )
    .unwrap();
    for banned in ["CGWindowListCopyWindowInfo", "NSAppleScript", "AXURL"] {
        assert!(!src.contains(banned), "call_titles.rs uses {banned}");
    }
    assert!(src.contains("\"AXWindows\"") && src.contains("\"AXTitle\""));
    assert!(src.contains(AX_OFF_LINE));
    // Never prompts for Accessibility from here.
    assert!(
        !src.contains("request_accessibility") && !src.contains("AXIsProcessTrustedWithOptions")
    );
}
