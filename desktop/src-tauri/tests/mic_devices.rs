//! PERM-H acceptance — the ranked microphone preference list, forget-device,
//! the AirPods and clamshell warnings, and a device change mid-take.
//!
//! Everything here runs on a CI runner with no microphone, no grant and no
//! window server: the rules are pure functions over device NAMES, and the
//! capture path's use of them is checked at the source level where it needs a
//! real HAL to run.

use wilson_voice_lib::input_format::{
    FormatChangeAction, FormatEventSource, InputFormat, InputFormatWatch, InputObservation,
};
use wilson_voice_lib::mic_ranking::{
    classify, mid_take_decision, rows, transport, DeviceClass, MicNotice, MicRanking, MidTake,
    NoticeKind, PickReason, WarningLatch,
};
use wilson_voice_lib::{load_settings, write_settings_file, AppSettings};

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

const BUILT_IN: &str = "MacBook Pro Microphone";
const USB: &str = "Shure MV7";
const AIRPODS: &str = "Wilson's AirPods Pro";
const OLD_HEADSET: &str = "Jabra Evolve 65";

/// Strip `//` comments so a source scan reads code, not prose about the code.
fn code_only(src: &str) -> String {
    src.lines()
        .map(|line| match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The body of `fn <name>` in `src`, up to its closing brace at column 0.
fn fn_body<'a>(src: &'a str, name: &str) -> &'a str {
    let start = src
        .find(&format!("fn {name}("))
        .unwrap_or_else(|| panic!("fn {name} not found"));
    let end = src[start..]
        .find("\n}\n")
        .map(|e| start + e)
        .unwrap_or(src.len());
    &src[start..end]
}

#[test]
fn the_highest_ranked_present_device_wins() {
    let ranking = MicRanking::from_settings(&names(&[USB, OLD_HEADSET, BUILT_IN]));

    // Every ranked device attached: rank 0 wins, whatever macOS's default is.
    let all = names(&[BUILT_IN, OLD_HEADSET, USB, AIRPODS]);
    let pick = ranking.pick(&all, Some(AIRPODS)).expect("a device");
    assert_eq!(pick.name, USB);
    assert_eq!(pick.reason, PickReason::Ranked(0));

    // The favourite unplugged: the NEXT ranked one that is present wins — not
    // the system default, which is unranked.
    let no_usb = names(&[BUILT_IN, OLD_HEADSET, AIRPODS]);
    let pick = ranking.pick(&no_usb, Some(AIRPODS)).expect("a device");
    assert_eq!(pick.name, OLD_HEADSET);
    assert_eq!(pick.reason, PickReason::Ranked(1));

    // No ranked device present: macOS's default input, exactly the old rule.
    let only_airpods = names(&[AIRPODS]);
    let pick = ranking
        .pick(&only_airpods, Some(AIRPODS))
        .expect("a device");
    assert_eq!(pick.name, AIRPODS);
    assert_eq!(pick.reason, PickReason::SystemDefault);

    // An empty ranking is the pre-PERM-H app: the system default always wins.
    let empty = MicRanking::default();
    assert_eq!(empty.pick(&all, Some(BUILT_IN)).unwrap().name, BUILT_IN);

    // A default that is not actually attached is never chosen.
    let pick = empty.pick(&names(&[USB]), Some("Gone")).unwrap();
    assert_eq!(
        (pick.name.as_str(), pick.reason),
        (USB, PickReason::FirstPresent)
    );
    assert!(
        empty.pick(&[], Some(BUILT_IN)).is_none(),
        "nothing attached"
    );

    // Selecting moves to the top; a stored list is de-duplicated.
    let mut r = MicRanking::from_settings(&names(&[BUILT_IN, USB, BUILT_IN, "  "]));
    assert_eq!(r.order(), names(&[BUILT_IN, USB]).as_slice());
    r.select(AIRPODS);
    assert_eq!(r.order(), names(&[AIRPODS, BUILT_IN, USB]).as_slice());
    r.reorder(&names(&[USB, AIRPODS, BUILT_IN]));
    assert_eq!(
        r.pick(&all, None).unwrap().name,
        USB,
        "drag-to-reorder decides"
    );

    // The Settings rows say which device wins.
    let listed = rows(&ranking, &all, Some(AIRPODS), |n| classify(n, None));
    let winners: Vec<_> = listed
        .iter()
        .filter(|r| r.wins)
        .map(|r| r.name.as_str())
        .collect();
    assert_eq!(winners, vec![USB]);
}

#[test]
fn forget_removes_it_from_selection() {
    let mut ranking = MicRanking::from_settings(&names(&[OLD_HEADSET, BUILT_IN]));
    let present = names(&[BUILT_IN, OLD_HEADSET]);
    assert_eq!(ranking.pick(&present, None).unwrap().name, OLD_HEADSET);

    assert!(ranking.forget(OLD_HEADSET), "a ranked row is removed");
    assert!(!ranking.forget(OLD_HEADSET), "forgetting twice is a no-op");
    assert_eq!(ranking.rank_of(OLD_HEADSET), None);
    assert_eq!(
        ranking.pick(&present, Some(OLD_HEADSET)).unwrap().name,
        BUILT_IN,
        "the old headset stops winning even while attached and the system default"
    );

    // An unplugged ranked device is still LISTED, so it can be forgotten.
    let ranking = MicRanking::from_settings(&names(&[OLD_HEADSET, BUILT_IN]));
    let listed = rows(&ranking, &names(&[BUILT_IN]), None, |n| classify(n, None));
    let headset = listed
        .iter()
        .find(|r| r.name == OLD_HEADSET)
        .expect("listed");
    assert!(!headset.present && headset.rank == Some(0) && !headset.wins);
}

#[test]
fn the_airpods_warning_fires_once_per_selection() {
    assert_eq!(classify(AIRPODS, None), DeviceClass::Bluetooth);
    assert_eq!(
        classify("WH-1000XM4", Some(transport::BLUETOOTH)),
        DeviceClass::Bluetooth
    );
    assert_eq!(
        classify("Pixel Buds", Some(transport::BLUETOOTH_LE)),
        DeviceClass::Bluetooth
    );
    assert_eq!(classify(BUILT_IN, None), DeviceClass::BuiltIn);
    assert_eq!(
        classify("Anything", Some(transport::BUILT_IN)),
        DeviceClass::BuiltIn
    );
    assert_eq!(classify(USB, None), DeviceClass::Other);
    // The transport type wins over a misleading name.
    assert_eq!(
        classify("AirPods-named USB box", Some(0x75736220)),
        DeviceClass::Other
    );

    let mut latch = WarningLatch::new();
    let first = latch.observe(AIRPODS, DeviceClass::Bluetooth, false);
    assert_eq!(first.len(), 1, "selecting AirPods says so");
    assert_eq!(first[0].kind, NoticeKind::Airpods);
    assert!(
        first[0].body.contains("built-in microphone"),
        "and offers the built-in mic"
    );

    for _ in 0..5 {
        assert!(
            latch
                .observe(AIRPODS, DeviceClass::Bluetooth, false)
                .is_empty(),
            "the same selection (every take start on it) never repeats the warning"
        );
    }

    // A different device, then back: a NEW selection, warned again.
    assert!(latch
        .observe(BUILT_IN, DeviceClass::BuiltIn, false)
        .is_empty());
    assert_eq!(
        latch.observe(AIRPODS, DeviceClass::Bluetooth, false).len(),
        1
    );

    // Non-Bluetooth devices never get the AirPods copy.
    let mut latch = WarningLatch::new();
    assert!(latch.observe(USB, DeviceClass::Other, false).is_empty());
}

#[test]
fn the_clamshell_warning_fires_for_the_built_in_mic_with_the_lid_closed() {
    let mut latch = WarningLatch::new();
    assert!(
        latch
            .observe(BUILT_IN, DeviceClass::BuiltIn, false)
            .is_empty(),
        "lid open"
    );
    let closed = latch.observe(BUILT_IN, DeviceClass::BuiltIn, true);
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].kind, NoticeKind::Clamshell);
    assert!(
        latch
            .observe(BUILT_IN, DeviceClass::BuiltIn, true)
            .is_empty(),
        "said once"
    );
    // Open, close again: worth another word.
    assert!(latch
        .observe(BUILT_IN, DeviceClass::BuiltIn, false)
        .is_empty());
    assert_eq!(latch.observe(BUILT_IN, DeviceClass::BuiltIn, true).len(), 1);
    // A USB mic with the lid closed is fine.
    let mut latch = WarningLatch::new();
    assert!(latch.observe(USB, DeviceClass::Other, true).is_empty());
}

#[test]
fn a_mid_take_input_change_does_not_silently_truncate() {
    let ranking = MicRanking::from_settings(&names(&[USB, BUILT_IN]));

    // The take's device is still there: it STAYS, even when a higher-ranked
    // device turns up (that one wins at the next take start, not mid-sentence).
    assert_eq!(
        mid_take_decision(&ranking, BUILT_IN, &names(&[BUILT_IN, USB]), None),
        MidTake::Stay
    );

    // The take's device vanished: carry the take to the next pick AND tell the
    // user. There is no variant that switches without a notice.
    match mid_take_decision(&ranking, USB, &names(&[BUILT_IN, AIRPODS]), Some(AIRPODS)) {
        MidTake::SwitchTo { to, notice } => {
            assert_eq!(
                to, BUILT_IN,
                "the next ranked device, not the system default"
            );
            assert_eq!(notice, MicNotice::switched(USB, BUILT_IN));
            assert_eq!(notice.kind, NoticeKind::SwitchedMidTake);
            assert!(notice.body.contains("Nothing recorded so far was lost"));
        }
        other => panic!("expected a switch, got {other:?}"),
    }

    // Nothing left to record from: parked (kept), and the user is told.
    match mid_take_decision(&ranking, USB, &[], None) {
        MidTake::Park { notice } => {
            assert_eq!(notice.kind, NoticeKind::ParkedMidTake);
            assert!(notice.body.contains("kept everything"));
        }
        other => panic!("expected a park, got {other:?}"),
    }

    // No ranking: the pre-PERM-H rule (follow macOS's default input, YV92) —
    // but now announced too.
    match mid_take_decision(
        &MicRanking::default(),
        BUILT_IN,
        &names(&[BUILT_IN, USB]),
        Some(USB),
    ) {
        MidTake::SwitchTo { to, notice } => {
            assert_eq!(to, USB);
            assert_eq!(notice.kind, NoticeKind::SwitchedMidTake);
        }
        other => panic!("expected a switch, got {other:?}"),
    }

    // The switch is a SEGMENT BOUNDARY, not an end: the format watch the
    // capture path feeds turns a device-name change into a Reconfigure with a
    // device_change marker at the exact output sample — record.rs carries the
    // same DSP (the take's audio) across that reopen.
    let mut watch = InputFormatWatch::new(USB, InputFormat::new(48_000, 1), 16_000);
    let action = watch.observe(InputObservation {
        device_name: BUILT_IN.into(),
        format: InputFormat::new(48_000, 1),
        host_time: 1_000,
        output_sample_index: 80_000,
        source: FormatEventSource::Watchdog,
    });
    match action {
        FormatChangeAction::Reconfigure { marker, .. } => {
            assert_eq!(marker.from_device, USB);
            assert_eq!(marker.to_device, BUILT_IN);
            assert_eq!(
                marker.output_sample_index, 80_000,
                "the take keeps its first 5 s"
            );
            assert_eq!(marker.segment_index, 1);
        }
        other => panic!("a device change at the same format must still reconfigure, got {other:?}"),
    }

    // And the capture path really routes through the rule: the watchdog asks
    // `mid_take_input`, which announces every switch and park while armed, and
    // the reopen hands the SAME take (Some(reuse)) to the decided device.
    let record = code_only(include_str!("../src/record.rs"));
    let watchdog = fn_body(&record, "watchdog_poll");
    assert!(watchdog.contains("mid_take_input(&live.device_name, live.is_armed())"));
    assert!(watchdog.contains("reopen_after_change(live, cache, Some(marker.to), Some(&name))"));
    let mid = fn_body(&record, "mid_take_input");
    assert!(mid.contains("mic_ranking::mid_take_decision("));
    assert!(mid.contains("mic_ranking::announce(&notice)"));
    assert!(mid.contains("mic_ranking::announce_parked_once(&notice)"));
    let reopen = fn_body(&record, "reopen_after_change");
    assert!(reopen.contains("open_stream_into(cache, Some(reuse), want)"));
    // The Disarm path still stamps a device failure onto the take (YV67) —
    // the parked take is returned with every sample, never truncated silently.
    assert!(record.contains("mark_device_failure(stream.end(), stream.has_failed())"));
}

#[test]
fn presence_and_authorization_stay_separate() {
    // mic_ranking answers HARDWARE questions only. It never asks TCC.
    let ranking_src = code_only(include_str!("../src/mic_ranking.rs"));
    for forbidden in [
        "authorization_status",
        "mic_auth",
        "request_access",
        "AVCaptureDevice",
        "microphone_ready",
        "MicAuth",
    ] {
        assert!(
            !ranking_src.contains(forbidden),
            "mic_ranking.rs must not consult TCC ({forbidden}); PERM-A split presence from permission"
        );
    }

    // And PERM-A's two questions are still two functions: presence never
    // reads TCC, authorization never reads the device list or the ranking.
    let auth_src = code_only(include_str!("../src/mic_auth.rs"));
    let presence = fn_body(&auth_src, "input_device_present");
    assert!(!presence.contains("authorization_status"));
    assert!(
        !auth_src.contains("mic_ranking"),
        "authorization must not depend on rankings"
    );

    // A row says `present`, and nothing about permission: an attached mic Yap
    // is not allowed to use is still present.
    let ranking = MicRanking::from_settings(&names(&[USB]));
    let listed = rows(&ranking, &names(&[USB]), Some(USB), |n| classify(n, None));
    let json = serde_json::to_value(&listed[0]).expect("serialize");
    let keys: Vec<&str> = json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert!(keys.contains(&"present"));
    for key in &keys {
        let k = key.to_lowercase();
        assert!(
            !k.contains("auth") && !k.contains("permission") && !k.contains("granted"),
            "device rows must not carry a permission field: {key}"
        );
    }
}

#[test]
fn ranking_persists_across_a_restart() {
    let dir = std::env::temp_dir().join(format!(
        "yap-perm-h-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("settings.json");

    let mut ranking = MicRanking::default();
    ranking.select(BUILT_IN);
    ranking.select(USB);
    ranking.select(OLD_HEADSET);
    ranking.forget(OLD_HEADSET);
    let settings = AppSettings {
        mic_ranking: ranking.order().to_vec(),
        ..AppSettings::default()
    };
    write_settings_file(&path, &settings).expect("write");

    // "Relaunch": a fresh read of the file, a fresh ranking built from it.
    let reloaded = MicRanking::from_settings(&load_settings(&path).mic_ranking);
    assert_eq!(reloaded, ranking);
    assert_eq!(reloaded.order(), names(&[USB, BUILT_IN]).as_slice());
    assert_eq!(
        reloaded
            .pick(&names(&[BUILT_IN, USB, OLD_HEADSET]), None)
            .unwrap()
            .name,
        USB
    );

    // A store written before PERM-H has no field: an empty ranking, never an error.
    std::fs::write(&path, r#"{"language":"en"}"#).unwrap();
    assert!(load_settings(&path).mic_ranking.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_four_commands_are_registered() {
    // Reachability: a #[tauri::command] missing from generate_handler! fails
    // at runtime with everything green.
    let lib = include_str!("../src/lib.rs");
    let start = lib.find("tauri::generate_handler![").expect("handler list");
    let handler = &lib[start..start + lib[start..].find("])").unwrap()];
    for cmd in [
        "mic_devices",
        "mic_rank_select",
        "mic_rank_reorder",
        "mic_rank_forget",
    ] {
        assert!(
            handler.lines().any(|l| l.trim() == format!("{cmd},")),
            "{cmd} must be in generate_handler!"
        );
    }
}

/// A command-level probe of the REAL hardware reads on the machine running it:
/// the cpal device list, CoreAudio transport types and IOKit's lid state.
/// Ignored by default (CI has no audio devices); run it with
/// `cargo test --features custom-protocol --test mic_devices -- --ignored --nocapture`.
#[test]
#[ignore = "reads this machine's audio hardware"]
fn live_hardware_probe() {
    let view = wilson_voice_lib::mic_ranking::devices_view();
    println!("lid_closed={:?}", view.lid_closed);
    for row in &view.rows {
        println!(
            "row name={:?} present={} rank={:?} class={:?} default={} wins={}",
            row.name, row.present, row.rank, row.class, row.system_default, row.wins
        );
    }
    for (name, t) in wilson_voice_lib::mic_ranking::sys_transport_types_for_probe() {
        println!(
            "hal name={name:?} transport={:?}",
            String::from_utf8_lossy(&t.to_be_bytes())
        );
    }
}
