//! YV100 acceptance — **the aggregate-device composition dictionary**, proved
//! as a pure function with zero audio hardware.
//!
//! Why this is worth a test binary of its own: CoreAudio composes an aggregate
//! device from a `CFDictionary` of *string* keys, and it **ignores keys it does
//! not recognise**. So `"tapautostart"` misspelled as `"tapauto"` does not fail
//! — `AudioHardwareCreateAggregateDevice` returns `noErr`, the device is
//! created, the tap never auto-starts, and the symptom is a meeting track that
//! is silent from sample zero, i.e. exactly the symptom of a TCC denial and of
//! OS-4's ghost tap. The three-way collapse this whole item is designed to avoid
//! can be caused by a typo, so the typo is what is tested.
//!
//! `the_declared_keys_are_coreaudios_own` closes the loop the other way: it
//! reads the key strings back out of `objc2-core-audio`'s own constants and
//! asserts they equal the ones the pure builder writes. A pure test over
//! hand-written constants can only prove self-consistency; this makes it prove
//! agreement with the framework.

use wilson_voice_lib::syscapture::{
    aggregate_description, declared_aggregate_key_names, keys, AggregateSpec, DictValue,
};

fn spec() -> AggregateSpec {
    AggregateSpec {
        aggregate_uid: "consulting.drivia.yap.tap.6f1c".to_string(),
        aggregate_name: "Yap meeting capture".to_string(),
        output_uid: "BuiltInSpeakerDevice".to_string(),
        tap_uid: "11111111-2222-3333-4444-555555555555".to_string(),
    }
}

fn get<'a>(dict: &'a [(String, DictValue)], key: &str) -> &'a DictValue {
    &dict
        .iter()
        .find(|(k, _)| k == key)
        .unwrap_or_else(|| panic!("the composition dictionary has no `{key}` key"))
        .1
}

#[test]
fn the_dictionary_is_exactly_the_seven_keys_coreaudio_is_given() {
    let dict = aggregate_description(&spec());
    let names: Vec<&str> = dict.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        names,
        vec![
            keys::AGGREGATE_UID,
            keys::AGGREGATE_NAME,
            keys::IS_PRIVATE,
            keys::MAIN_SUB_DEVICE,
            keys::SUB_DEVICE_LIST,
            keys::TAP_LIST,
            keys::TAP_AUTO_START,
        ],
        "an extra key is dead weight; a missing one is a silent misconfiguration"
    );
}

#[test]
fn the_tap_rides_in_the_tap_list_as_a_sub_tap_dictionary() {
    let dict = aggregate_description(&spec());
    // `taps` is a LIST of DICTIONARIES keyed by `uid`, not a list of bare
    // strings. A bare string there is accepted and silently dropped, which
    // produces an aggregate device with no tap on it at all.
    assert_eq!(
        get(&dict, keys::TAP_LIST),
        &DictValue::List(vec![DictValue::Dict(vec![(
            keys::SUB_TAP_UID.to_string(),
            DictValue::Str("11111111-2222-3333-4444-555555555555".to_string()),
        )])]),
    );
}

#[test]
fn the_default_output_device_is_both_the_main_sub_device_and_the_whole_sub_device_list() {
    let dict = aggregate_description(&spec());
    // The main sub device is what gives the aggregate its clock. If the tapped
    // process renders to a DIFFERENT device than this one, the tap is silent —
    // routine with AirPods, and the reason YV103's device-change guard exists.
    assert_eq!(
        get(&dict, keys::MAIN_SUB_DEVICE),
        &DictValue::Str("BuiltInSpeakerDevice".to_string()),
    );
    assert_eq!(
        get(&dict, keys::SUB_DEVICE_LIST),
        &DictValue::List(vec![DictValue::Dict(vec![(
            keys::SUB_DEVICE_UID.to_string(),
            DictValue::Str("BuiltInSpeakerDevice".to_string()),
        )])]),
    );
}

#[test]
fn the_device_is_private_and_the_tap_auto_starts() {
    let dict = aggregate_description(&spec());
    // Private: the device never appears in another app's device list, and it
    // dies with the process — a crashed Yap must not leave a phantom output
    // device in Audio MIDI Setup.
    assert_eq!(get(&dict, keys::IS_PRIVATE), &DictValue::Bool(true));
    // Auto-start: nothing else in this design ever issues a separate tap start,
    // so `false` here is a permanently silent track that reports no error.
    assert_eq!(get(&dict, keys::TAP_AUTO_START), &DictValue::Bool(true));
}

#[test]
fn the_uids_come_straight_from_the_spec_and_nothing_is_invented() {
    let dict = aggregate_description(&spec());
    assert_eq!(
        get(&dict, keys::AGGREGATE_UID),
        &DictValue::Str("consulting.drivia.yap.tap.6f1c".to_string()),
    );
    assert_eq!(
        get(&dict, keys::AGGREGATE_NAME),
        &DictValue::Str("Yap meeting capture".to_string()),
    );
}

#[test]
fn it_is_a_pure_function_of_the_spec() {
    // Same in, same out, twice — this is the property that lets YV104's rebuild
    // recompose the device from the new output UID without a second code path.
    assert_eq!(
        aggregate_description(&spec()),
        aggregate_description(&spec())
    );
    let mut other = spec();
    other.output_uid = "AppleHDAEngineOutput:1B,0,1,1:0".to_string();
    assert_ne!(
        aggregate_description(&spec()),
        aggregate_description(&other),
        "a new default output device must compose a different device"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn the_declared_keys_are_coreaudios_own() {
    use wilson_voice_lib::syscapture::coreaudio_aggregate_key_names;
    // The anti-typo assertion: our constants against the framework's, name by
    // name. Without this, the tests above only prove the builder agrees with
    // itself.
    assert_eq!(
        declared_aggregate_key_names(),
        coreaudio_aggregate_key_names()
    );
}

#[cfg(not(target_os = "macos"))]
#[test]
fn the_declared_key_list_is_still_complete_off_macos() {
    assert_eq!(declared_aggregate_key_names().len(), 9);
}
