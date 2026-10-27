# Mutation ledger — every test this loop added is proven non-vacuous (Y7-C)

A test that asserts a symbol exists, in a file that always contains it, proves
nothing; a grep proving absence with the wrong pattern or the wrong scope proves
less than nothing. This ledger is the mechanical answer: **one row per test file
in `desktop/src-tauri/tests/`**, each naming a **single-line mutation of SOURCE**
(never of the test) and the **one named test** that must go red under it.

`scripts/assert-tests-are-non-vacuous.sh` reads this table.

* **Default run (fast, no cargo — the per-item gate).** Fails unless every test
  file has a row, every row's mutation still applies to exactly one line of the
  current tree, and every row carries an `executed: red` record bound to the
  sha256 of its test file. A row whose mutation was never executed is a failed
  row; a row whose test changed since its red run is STALE and fails too. It
  also runs the **scopeless-grep census** over `scripts/loop/items/*.mjs`.
* **`--execute` (the full sweep, once per part — `YAP_SMOKE_MUTATIONS=1
  ./scripts/loop-smoke.sh`).** On a clean tree: runs every named test unmutated
  (must be green), then per row applies the mutation, runs `cargo test
  --features custom-protocol --test <file> -- --exact <test>`, requires that
  the **named test** reports `FAILED` (a compile error or "the whole suite went
  red" is not proof), restores the file, and requires `git diff --exit-code`.
  The observed transition is written into the `executed` column by the script —
  never by hand. `--only <file.rs>` re-runs selected rows.

Rows marked `exempt:` are counted and printed on every run. `ignored-only` is
verified mechanically (every `#[test]` in the file is `#[ignore]`d). The
model-backed rows (`asr_capabilities_probe`, `diarize_embed_smoke`,
`sherpa_load_smoke`) can only go red on a machine that has the models under the
state root; on a machine without them those tests return early and the sweep
reports them green — which is the correct, loud answer.

Mutations the earlier items named and where they live now: `shipped_defaults`
(auto_paste true → false), `formatting_fixtures` (cleanup_level back to
"light"), `mic_auth_status` (an AVFoundation discriminant moved), `mic_gate`
(the microphone check deleted from `start_recording`), `trial_state_machine`
(the max-seen-wall-clock floor dropped), `dictation_chunked` (the seam dedupe
disabled), `dictation_capture_memory` (the resident ceiling), `polish_long_form`
(`MAX_POLISH_WORDS` no longer exists; the chunk ceiling is mutated instead),
`updater_endpoint` (the endpoint list reduced to one), `lifecycle` (the hide
hint gate). `no_outbound_on_the_dictation_path` (a URL literal added to
`record.rs`) is carried by `matrix_phase_offline`, the test that scans
`record.rs` for network surface today. `paste_target_e2e`,
`history_at_volume` and `lifecycle_e2e` are named by later items but have no
test file in the tree yet — the default run fails the day one lands without a
row here.

<!-- ledger:start -->
| # | test file | named test | source file | anchor | from | to | executed |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | `aggregate_rebuild_debounce.rs` | `the_debounce_window_is_at_least_the_500ms_os3_requires` | `src/input_format.rs` | `pub const AGGREGATE_REBUILD_DEBOUNCE_MS: u64 = 500;` | `= 500;` | `= 400;` | not yet executed |
| 2 | `aggregate_rebuild_idempotent_teardown.rs` | `releasing_the_guard_twice_is_safe_and_the_watch_still_works_afterwards` | `src/input_format.rs` | `pub fn is_aggregate_work_in_flight(&self) -> bool {` | `-> bool {` | `-> bool { return true;` | not yet executed |
| 3 | `app_aware_formatting.rs` | `auto_resolves_real_bundle_identifiers_to_the_right_mode` | `src/dictation.rs` | `        _ => mode_for_app(app_name),` | `mode_for_app(app_name)` | `DictationMode::Plain` | not yet executed |
| 4 | `asr_capabilities_probe.rs` | `the_capabilities_probe_prints_max_timestamp_kind_and_max_audio_ms` | `src/asr_engine.rs` | `max_timestamp_kind: {}\nmax_audio_ms: {}` | `max_timestamp_kind: {}` | `max_timestamp: {}` | not yet executed |
| 5 | `biquad_lowpass_response.rs` | `the_cutoff_sits_below_the_target_nyquist_and_degenerate_filters_are_none` | `src/resample.rs` | `pub const ANTI_ALIAS_CUTOFF_HZ: f32 = 7_200.0;` | `= 7_200.0;` | `= 7_200.0 + 1.0;` | not yet executed |
| 6 | `calendar_permission_readonly.rs` | `info_plist_carries_the_calendar_usage_string` | `Info.plist` | `<key>NSCalendarsFullAccessUsageDescription</key>` | `NSCalendarsFullAccessUsageDescription` | `NSCalendarsUsageDescription` | not yet executed |
| 7 | `cancel_long_take.rs` | `cancel_reason_is_distinguishable_from_preempted_and_abandoned` | `src/transcription.rs` | `pub const CANCELLED_BY_USER: &str` | `= "transcription cancelled by the user";` | `= "";` | not yet executed |
| 8 | `capture_engine_is_installed.rs` | `the_record_button_cannot_stay_dead_once_the_capture_module_lands` | `src/lib.rs` | `            meeting_control::install_capture_engine(Arc::new(` | `install_capture_engine(` | `install_capture_engine (` | not yet executed |
| 9 | `capture_journal_recovery.rs` | `a_synthetic_low_disk_value_refuses_with_the_exact_free_space_number` | `src/meeting.rs` | `pub const MEETING_HARD_CAP: Duration = Duration::from_secs(3 * 60 * 60);` | `= Duration::from_secs(3 * 60 * 60);` | `= Duration::from_secs((3 * 60 * 60) + 1);` | not yet executed |
| 10 | `carbon_exclusive_probe.rs` | `probe_result_maps_exists_err_and_others` | `src/carbon_probe.rs` | `"another app or macOS already uses it, eventHotKeyExistsErr {status}"` | `another app or macOS` | `another application` | not yet executed |
| 11 | `consent_sheet_shown_once.rs` | `consent_sheet_shown_once` | `src/db.rs` | `MeetingConsent::from_ack(self.setting_get(meetings::CONSENT_NOTICE_KEY))` | `self.setting_get(meetings::CONSENT_NOTICE_KEY)` | `None` | not yet executed |
| 12 | `data_dir_override.rs` | `smoke_mode_registers_no_global_hotkey_and_never_pastes` | `src/smoke.rs` | `    !is_armed() && (!release_armed() \|\| INSERT_PERMIT.load(Ordering::SeqCst))` | `!is_armed() && ` | `(empty)` | not yet executed |
| 13 | `db_migration_idempotent.rs` | `a_newer_database_is_left_alone` | `src/db.rs` | `    pub fn schema_version(&self) -> Result<i64, String> {` | `-> Result<i64, String> {` | `-> Result<i64, String> { #[allow(unreachable_code)] return Ok(7);` | not yet executed |
| 14 | `diarization_metrics.rs` | `jer_matches_hand_worked_example` | `src/diarize_metrics.rs` | `let union = union_duration(&ref_spans, &hyp_spans);` | `union_duration(&ref_spans, &hyp_spans);` | `union_duration(&ref_spans, &hyp_spans) * 2.0;` | not yet executed |
| 15 | `diarize_catalog_shape.rs` | `the_catalog_never_states_an_embedding_dimension` | `src/catalog.json` | `"24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488"` | `"24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488"` | `"24615ee884c897d9d2ba09bb4d30da6bb1b15e685065962db5b02e76e4996488", "embedding_dim": 192` | not yet executed |
| 16 | `diarize_cluster_distance_threshold.rs` | `one_pair_groups_below_the_threshold_and_splits_above_it` | `src/diarize.rs` | `        if height <= cut + 1e-9 {` | `height <= cut + 1e-9` | `height <= cut * 2.0 + 1e-9` | not yet executed |
| 17 | `diarize_cluster_rank_and_floor.rs` | `the_floors_are_the_ones_the_finding_named` | `src/diarize.rs` | `pub const CHIP_FLOOR_TURNS: usize = 3;` | `= 3;` | `= 2;` | not yet executed |
| 18 | `diarize_embed_smoke.rs` | `camplus_embeddings_track_the_speaker_across_utterances` | `src/diarize_protocol.rs` | `self.ok.then_some(self.embedding_dim).flatten()` | `self.ok.then_some(self.embedding_dim).flatten()` | `None` | not yet executed |
| 19 | `diarize_full_clustering_vs_binary_fallback.rs` | `binary_mode_never_invents_a_speaker_who_was_not_there` | `src/diarize.rs` | `pub const EVERYONE_ELSE_CLUSTER: i64 = 1;` | `= 1;` | `= 1 + 1;` | not yet executed |
| 20 | `diarize_model_install_e2e.rs` | `(none)` | `` | `` | `` | `(empty)` | exempt: ignored-only — every #[test] in the file is #[ignore]d; no default run can go red |
| 21 | `diarize_sidecar_handshake.rs` | `a_missing_model_is_refused_cleanly_and_the_child_survives_it` | `src/diarize.rs` | `/// answering "this is not the instructor" about a turn nothing was measured on` | `nothing was measured` | `nothing wa~s measured` | not yet executed |
| 22 | `diarize_sidecar_pool.rs` | `a_success_carrying_no_segments_is_a_protocol_failure_not_a_silent_meeting` | `src/diarize.rs` | `            .into_segments()` | `.into_segments()` | `.into_segments().or(Some(Vec::new()))` | not yet executed |
| 23 | `diarize_wire_unit_discipline.rs` | `the_threshold_is_never_converted_between_the_api_and_sherpa` | `../yap-diarize/src/main.rs` | `        threshold: clustering_distance_threshold,` | `clustering_distance_threshold,` | `1.0 - clustering_distance_threshold,` | not yet executed |
| 24 | `dictation_capture_memory.rs` | `resident_bytes_are_bounded_across_a_synthetic_twenty_minute_take` | `src/record.rs` | `pub const MAX_RESIDENT_CAPTURE_BYTES: usize = 4 * 1024 * 1024;` | `4 * 1024 * 1024;` | `4 * 1024;` | not yet executed |
| 25 | `dictation_chunked.rs` | `seams_do_not_duplicate_or_drop_words` | `src/meeting_asr.rs` | `            for trim in 0..=MAX_TAIL_TRIM {` | `0..=MAX_TAIL_TRIM` | `0..=0` | not yet executed |
| 26 | `dictation_pinned_model.rs` | `a_meeting_job_never_gets_a_long_dictation_decoded_on_its_pinned_model` | `src/transcription.rs` | `        self.inner.live_recording.load(Ordering::Acquire)` | `self.inner.live_recording.load(Ordering::Acquire)` | `false` | not yet executed |
| 27 | `entitlements.rs` | `microphone_entitlement_is_granted` | `Entitlements.plist` | `<key>com.apple.security.device.audio-input</key>` | `audio-input` | `audio-input-disabled` | not yet executed |
| 28 | `extract_tar_bz2.rs` | `rejects_a_parent_dir_escape_entry` | `src/models.rs` | `            Component::CurDir => {}` | `Component::CurDir =>` | `Component::CurDir \| Component::ParentDir =>` | not yet executed |
| 29 | `formatting_diff.rs` | `an_empty_rewrite_is_a_skip_not_an_acceptance` | `src/dictation.rs` | `Ok(_) => trace.llm_skip_reason = Some("v1_empty"),` | `Some("v1_empty")` | `None` | not yet executed |
| 30 | `formatting_fixtures.rs` | `fixtures_cover_every_rule_and_measured_failure` | `src/lib.rs` | `            cleanup_level: "medium".into(),` | `"medium"` | `"light"` | not yet executed |
| 31 | `formatting_settings_persist.rs` | `field_count_forces_this_test_to_be_updated` | `src/lib.rs` | `    pub auto_paste: bool,` | `pub auto_paste` | `#[serde(skip)] pub auto_paste` | not yet executed |
| 32 | `gate_corpus.rs` | `gate_corpus_never_discards_real_speech_class` | `src/dictation.rs` | `    let index = first_degenerate_token(&tokens)?;` | `first_degenerate_token(&tokens)?` | `first_degenerate_token(&tokens).unwrap_or(0)` | not yet executed |
| 33 | `hotkey_no_tcc.rs` | `probe_denied_but_tap_delivering_stays_armed` | `src/hotkeys.rs` | `        FnTapStatus::Delivering \| FnTapStatus::ArmedNoDelivery` | `FnTapStatus::Delivering \| ` | `(empty)` | not yet executed |
| 34 | `hotkey_stop.rs` | `the_stop_path_does_not_depend_on_the_frontmost_app` | `src/ptt_macos.rs` | `pub const POLL_INTERVAL_MS: u64 = 30;` | `= 30;` | `= 31;` | not yet executed |
| 35 | `input_format_change_handler.rs` | `the_listener_edge_is_taken_once_and_names_its_selector` | `src/input_format.rs` | `    match LAST_EVENT.swap(0, Ordering::AcqRel) {` | `LAST_EVENT.swap(0, Ordering::AcqRel)` | `LAST_EVENT.load(Ordering::Acquire)` | not yet executed |
| 36 | `insertion_matrix.rs` | `ax_only_when_post_event_denied` | `src/paste.rs` | `    } else if probe.accessibility.usable() {` | `if probe.accessibility` | `if false && probe.accessibility` | not yet executed |
| 37 | `license_gate.rs` | `license_module_holds_no_private_key_material` | `src/license.rs` | `pub const DB_KEY_CLOCK_FLOOR: &str = "license.max_seen_wall_ms";` | `"license.max_seen_wall_ms"` | `"license.max_seen_wall_ms.sign()"` | not yet executed |
| 38 | `lifecycle.rs` | `a_claim_without_a_hide_shows_nothing_and_writes_nothing` | `src/lifecycle.rs` | `.swap(false, std::sync::atomic::Ordering::SeqCst)` | `Ordering::SeqCst)` | `Ordering::SeqCst) && false` | not yet executed |
| 39 | `long_take_budget.rs` | `no_new_polling_timer_was_introduced` | `src/transcription.rs` | `pub const NO_ENGINE_LOADED: &str = "no ASR model is loaded";` | `"no ASR model is loaded";` | `"no ASR model is loaded"; pub const POLL_TICK: std::time::Duration = std::time::Duration::from_millis(16);` | not yet executed |
| 40 | `long_take_kill_mid_decode.rs` | `an_orderly_end_of_the_decode_clears_its_chunk_text` | `src/lib.rs` | `            if let Err(e) = self.db.clear_take_chunks(id) {` | `self.db.clear_take_chunks(id)` | `Ok::<usize, String>(id.len())` | not yet executed |
| 41 | `long_take_recovery.rs` | `a_completed_take_leaves_nothing_recoverable` | `src/db.rs` | `    pub fn clear_take_chunks(&self, take_id: &str) -> Result<usize, String> {` | `-> Result<usize, String> {` | `-> Result<usize, String> { #[allow(unreachable_code)] return Ok(0);` | not yet executed |
| 42 | `matrix_coverage.rs` | `the_matrix_still_covers_every_failure_22a_22b_and_yap23_own` | `src/meeting_matrix.rs` | `        id: "13",` | `"13"` | `"13x"` | not yet executed |
| 43 | `matrix_new_asr_chunk_timeout.rs` | `a_chunk_that_exceeds_the_transcribe_timeout_fails_alone` | `src/transcription.rs` | `"transcription timed out after {}s",` | `timed out after` | `stalled after` | not yet executed |
| 44 | `matrix_new_quit_mid_processing.rs` | `a_corrupt_ledger_starts_over_instead_of_failing` | `src/meeting_asr.rs` | `log::warn!("meeting {meeting_id}: unreadable ASR progress ({e}) — starting over");` | `log::warn!(` | `return Err(format!("{e}")); #[allow(unreachable_code)] log::warn!(` | not yet executed |
| 45 | `matrix_phase_offline.rs` | `the_whole_meeting_path_has_no_network_surface` | `src/record.rs` | `pub const TARGET_RATE: u32 = 16_000;` | `16_000;` | `16_000; pub const SUPPORT_URL: &str = "https://example.invalid/yap";` | not yet executed |
| 46 | `matrix_row12_macos_144_gate.rs` | `the_requirement_is_declared_in_exactly_one_place` | `src/meeting_asr.rs` | `let requirement = os_version_gate::SYSTEM_AUDIO_REQUIREMENT;` | `os_version_gate::SYSTEM_AUDIO_REQUIREMENT` | `"System audio needs macOS 14.4"` | not yet executed |
| 47 | `matrix_row13_diarize_model_missing.rs` | `the_affordance_reports_itself_unavailable_with_the_combined_size` | `src/meeting_matrix.rs` | `"Download {megabytes} MB to enable speaker detection.` | `{megabytes} MB` | `{megabytes}MB` | not yet executed |
| 48 | `matrix_row14_output_device_change.rs` | `the_published_cell_names_the_owner_of_the_missing_wiring` | `src/meeting_matrix.rs` | `wiring_pr: Some("#123 (YV100)"),` | `#123 (YV100)` | `#123 (~YV100)` | not yet executed |
| 49 | `matrix_row15_single_instance.rs` | `the_plugin_is_a_pinned_dependency` | `Cargo.toml` | `tauri-plugin-single-instance = "2.4.3"` | `"2.4.3"` | `"2.4"` | not yet executed |
| 50 | `matrix_row16_sleep_wake.rs` | `row16_is_no_longer_policy_only` | `src/meeting_matrix.rs` | `"**Policy only, NOT WIRED** — `cargo test --test {}` covers the decision; **nothing calls `{absent_call_site}`**, so the app does not do this yet",` | `NOT WIRED` | `NOT W~IRED` | not yet executed |
| 51 | `matrix_row17_meeting_cap.rs` | `the_published_thresholds_are_the_ones_the_watchdog_enforces` | `src/meeting.rs` | `pub const MEETING_CAP_WARN_AT: Duration` | `45 * 60` | `40 * 60` | not yet executed |
| 52 | `matrix_row1_tap_permission_denied.rs` | `the_published_cell_names_the_decision_the_app_now_makes` | `src/meeting_matrix.rs` | `"**Policy only, NOT WIRED** — `cargo test --test {}` covers the decision; **nothing calls `{absent_call_site}`**, so the app does not do this yet",` | `NOT WIRED` | `NOT W~IRED` | not yet executed |
| 53 | `matrix_row2_tap_revoked_mid_meeting.rs` | `the_published_cell_names_the_environment_that_made_this_row_reachable` | `src/meeting_matrix.rs` | `            subject: "TappedEnv",` | `"TappedEnv"` | `"TapEnv"` | not yet executed |
| 54 | `matrix_row3_mic_denied_system_only.rs` | `a_denied_tap_with_a_working_mic_is_22as_ordinary_meeting_badged` | `src/meeting_matrix.rs` | `"System audio is not being recorded — this Mac cannot capture it. \` | `cannot capture` | `cannot ~capture` | not yet executed |
| 55 | `matrix_row4_disk_preflight.rs` | `a_start_that_fits_is_allowed` | `src/meeting.rs` | `pub const MEETING_HARD_CAP: Duration = Duration::from_secs(3 * 60 * 60);` | `= Duration::from_secs(3 * 60 * 60);` | `= Duration::from_secs((3 * 60 * 60) + 1);` | not yet executed |
| 56 | `matrix_row5_journal_backpressure.rs` | `the_dictation_journal_keeps_the_same_never_block_rule` | `src/meeting_matrix.rs` | `behavior: "Bounded queue, `try_send`, drops counted, the audio callback never blocks",` | `try_send` | `try_~send` | not yet executed |
| 57 | `matrix_row6_orphan_recovery.rs` | `an_orphan_marker_and_spill_become_a_playable_wav_marked_partial` | `src/meeting.rs` | `const MEETING_MARKER_EXT: &str = "meeting.in_progress.json";` | `meeting.in_progress.json` | `meeting.in_p~rogress.json` | not yet executed |
| 58 | `matrix_row7_diarize_sidecar_wedge.rs` | `the_give_up_logs_as_a_sidecar_failure_and_says_the_transcript_survived` | `src/meeting_matrix.rs` | `reason=sidecar detail={tag} — the transcript is intact and` | `the transcript is intact` | `the transcript is kept` | not yet executed |
| 59 | `matrix_row8_diarize_garbage_clusters.rs` | `a_crashed_sidecar_and_a_noisy_one_are_distinguishable_in_the_log` | `src/meeting_matrix.rs` | `"{DIARIZATION_FAILED} reason=clusters raw_clusters={raw_clusters} — the diarizer \` | `reason=clusters` | `reason=c~lusters` | not yet executed |
| 60 | `max_session.rs` | `meeting_cap_and_dictation_cap_are_separate_constants` | `src/record.rs` | `pub const MAX_SESSION_SECONDS: u64 = 30 * 60;` | `30 * 60` | `31 * 60` | not yet executed |
| 61 | `meeting_asr_pinned_model_decodes.rs` | `a_pinned_decode_refuses_another_resident_model_and_leaves_it_in_place` | `src/transcription.rs` | `pub const NO_ENGINE_LOADED: &str = "no ASR model is loaded";` | `= "no ASR model is loaded";` | `= "no ASR model is loaded_mut";` | not yet executed |
| 62 | `meeting_audio_retention.rs` | `the_retention_window_is_seven_days` | `src/meetings.rs` | `pub const AUDIO_RETENTION_DAYS: i64 = 7;` | `= 7;` | `= 8;` | not yet executed |
| 63 | `meeting_availability_144_gate.rs` | `system_audio_specifically_reports_requires_macos_14_4_below_the_floor` | `src/meeting_asr.rs` | `    if capture == MeetingCapture::MicPlusSystemAudio {` | `MeetingCapture::MicPlusSystemAudio {` | `MeetingCapture::MicOnly {` | not yet executed |
| 64 | `meeting_capture_memory.rs` | `a_forty_eight_kilohertz_source_does_not_retain_its_resampler_state` | `src/meeting.rs` | `        let delivered_now = delivered + frames as u64;` | `delivered + frames as u64;` | `delivered + frames as u64; std::mem::forget(vec![0u8; 1 << 16]);` | not yet executed |
| 65 | `meeting_capture_rt_safety.rs` | `the_capture_callback_body_allocates_nothing` | `src/meeting.rs` | `        let taken_now = taken + (accepted / channels) as u64;` | `(accepted / channels) as u64;` | `(accepted / channels) as u64; std::hint::black_box(Vec::<u8>::with_capacity(64));` | not yet executed |
| 66 | `meeting_cluster_attribution.rs` | `attribution_never_touches_a_track_the_pass_did_not_cluster` | `src/meetings.rs` | `pub const MIC_SPEAKER_LABEL: &str = "Me";` | `= "Me";` | `= "Me_mut";` | not yet executed |
| 67 | `meeting_delete_cascade.rs` | `delete_removes_rows_wavs_and_fts_entries` | `src/db.rs` | `.query_row("SELECT COUNT(*) FROM meeting_segments", [], \|r\| r.get(0))` | `SELECT COUNT(*) FROM meeting_segments` | `SELECT COUNT(*) FRO~M meeting_segments` | not yet executed |
| 68 | `meeting_diagnostics_row.rs` | `a_completed_meeting_carries_its_thermal_and_battery_readings` | `src/meeting_energy.rs` | `pub const VERSION: u32 = 1;` | `= 1;` | `= 1 + 1;` | not yet executed |
| 69 | `meeting_dictation_fanout.rs` | `a_dictation_with_no_meeting_running_is_an_ordinary_dictation` | `src/meeting.rs` | `    if meeting_capture_active() {` | `if meeting_capture_active()` | `if !meeting_capture_active()` | not yet executed |
| 70 | `meeting_dictation_preempts_transcription.rs` | `the_interactive_claim_is_raised_for_a_dictation_and_dropped_after_it` | `src/transcription.rs` | `self.inner.interactive_waiting.load(Ordering::Acquire) != 0` | `!= 0` | `!= 0 && false` | not yet executed |
| 71 | `meeting_english_only_gate.rs` | `a_non_english_spoken_language_disables_the_notetaker_with_a_message` | `src/meeting_asr.rs` | `Your 'Language I speak' is set to {language}` | `'Language I speak'` | `'Spoken language'` | not yet executed |
| 72 | `meeting_eval.rs` | `overlap_token_budget_matches_the_chunk_geometry` | `src/meeting_asr.rs` | `pub const OVERLAP_TOKEN_BUDGET: usize = 6;` | `= 6;` | `= 6 + 1;` | not yet executed |
| 73 | `meeting_event_contract.rs` | `the_frontend_listens_to_the_name_the_backend_emits` | `src/meetings.rs` | `pub const MEETING_EVENT: &str = "meeting";` | `"meeting"` | `"meetings"` | not yet executed |
| 74 | `meeting_fts_search.rs` | `fts_search_finds_a_phrase_from_a_segment` | `src/db.rs` | `WHERE meeting_segments_fts MATCH ?3` | `MATCH ?3` | `MATCH ?3 AND 0` | not yet executed |
| 75 | `meeting_import.rs` | `meeting_import_huge_srt_cue_is_refused_not_a_panic` | `src/meeting_import.rs` | `Nothing was imported."` | `Nothing was imported` | `Nothing was saved` | not yet executed |
| 76 | `meeting_kind_branch.rs` | `the_label_follows_the_same_branch_the_target_does` | `src/meetings.rs` | `pub const MIC_SPEAKER_LABEL: &str = "Me";` | `= "Me";` | `= "Me_mut";` | not yet executed |
| 77 | `meeting_kind_default_on_skip.rs` | `a_row_created_without_a_kind_is_unknown` | `src/meetings.rs` | `pub const DEFAULT_MEETING_KIND: &str = "unknown";` | `= "unknown";` | `= "unknown_mut";` | not yet executed |
| 78 | `meeting_manual_start_stop.rs` | `the_shipped_tick_is_one_hertz` | `src/meeting_control.rs` | `pub const TICK_INTERVAL: Duration = Duration::from_secs(1);` | `from_secs(1)` | `from_millis(1500)` | not yet executed |
| 79 | `meeting_markdown_export.rs` | `exporting_a_deleted_meeting_is_an_error_not_a_panic` | `src/db.rs` | `.ok_or_else(\|\| format!("no meeting {id}"))?;` | `"no meeting {id}"` | `"missing {id}"` | not yet executed |
| 80 | `meeting_markdown_export_two_track.rs` | `a_mic_only_export_is_unchanged_from_22a` | `src/meetings.rs` | `out.push_str("## Transcript\n\n");` | `## Transcript` | `## Tran~script` | not yet executed |
| 81 | `meeting_no_automute.rs` | `the_policy_itself_is_explicit` | `src/meeting.rs` | `    matches!(context, TakeContext::Dictation)` | `TakeContext::Dictation)` | `TakeContext::Dictation \| TakeContext::Meeting)` | not yet executed |
| 82 | `meeting_no_model_resident.rs` | `the_meeting_queue_is_deeper_than_a_dictation_s_but_still_bounded` | `src/meeting.rs` | `pub const MEETING_QUEUE_DEPTH: usize = 512;` | `= 512;` | `= 64;` | not yet executed |
| 83 | `meeting_pipeline_residual.rs` | `lib_rs_pins_the_model_spawns_the_pipeline_and_flips_live_recording` | `src/lib.rs` | `start_delay: meeting_pipeline::QUEUE_START_DELAY,` | `start_delay: meeting_pipeline::QUEUE_START_DELAY` | `start_delay: (meeting_pipeline::QUEUE_START_DELAY)` | not yet executed |
| 84 | `meeting_pipeline_wired.rs` | `an_empty_transcript_names_failures_never_no_speech` | `src/meeting_pipeline.rs` | `pub const NO_SPEECH_MESSAGE: &str = "Yap found no speech in this meeting's audio.";` | `= "Yap found no speech in this meeting's audio.";` | `= "Yap found no speech in this meeting's audio._mut";` | not yet executed |
| 85 | `meeting_power_assertion.rs` | `the_reason_the_user_sees_names_the_app_and_the_activity` | `src/power.rs` | `    format!("Yap is recording {what}")` | `"Yap is recording {what}"` | `"Recording {what}"` | not yet executed |
| 86 | `meeting_retention_keeps_untranscribed_audio.rs` | `retention_is_state_aware` | `src/db.rs` | `                       AND state = 'complete'` | `AND state = 'complete'` | `AND state != 'recording'` | not yet executed |
| 87 | `meeting_stats_rollup.rs` | `activation_is_none_until_both_ends_exist` | `src/db.rs` | `stats.days_to_first_meeting = first_dictation.map(parse_dt)` | `first_dictation.map(parse_dt)` | `first_dictation.filter(\|_\| false).map(parse_dt)` | not yet executed |
| 88 | `meeting_survives_sleep.rs` | `will_sleep_mid_recording_finalizes_partial_then_acknowledges` | `src/meeting_matrix.rs` | `pub const SLEEP_STOP_NOTE: &str = "Stopped when the Mac went to sleep.";` | `= "Stopped when the Mac went to sleep.";` | `= "Stopped when the Mac went to sleep._mut";` | not yet executed |
| 89 | `meeting_track_b_wiring.rs` | `meeting_pre_14_4_stays_mic_only` | `src/syscapture.rs` | `    if !gate.is_available() {` | `!gate.is_available()` | `false` | not yet executed |
| 90 | `meeting_transcript_render_single_track_unchanged.rs` | `an_empty_mic_only_meeting_still_exports_the_honest_line` | `src/meetings.rs` | `out.push_str("_No transcript segments._\n");` | `_No transcript segments._` | `_No segments._` | not yet executed |
| 91 | `meeting_transcript_render_two_track.rs` | `two_track_detection_is_about_the_segments_not_the_meeting_row` | `src/meetings.rs` | `        .any(\|s\| s.track != MIC_TRACK && !one_line(&s.text).is_empty())` | `s.track != MIC_TRACK` | `s.track == MIC_TRACK` | not yet executed |
| 92 | `mic_auth_status.rs` | `mic_auth_discriminants_match_avfoundation` | `src/mic_auth.rs` | `    Authorized = 3,` | `= 3,` | `= 4,` | not yet executed |
| 93 | `mic_gate.rs` | `microphone_is_checked_before_license` | `src/lib.rs` | `    if !microphone_allows_new_dictation(app, state) {` | `!microphone_allows_new_dictation(app, state)` | `false` | not yet executed |
| 94 | `paragraphing.rs` | `pause_at_a_sentence_boundary_breaks` | `src/dictation.rs` | `pub const PARAGRAPH_PAUSE_SECONDS: f64 = 0.9;` | `= 0.9;` | `= 2.0;` | not yet executed |
| 95 | `perm_repair.rs` | `phase_and_manual_copy_name_the_running_product` | `src/perm_truth.rs` | `        _ => "Yap".to_string(),` | `"Yap"` | `"Wilson Voice"` | not yet executed |
| 96 | `perm_truth.rs` | `the_banner_poll_is_five_seconds_and_activation_is_a_watch_reason` | `src/permissions.rs` | `pub const WATCH_INTERVAL: Duration = Duration::from_secs(5);` | `from_secs(5)` | `from_secs(6)` | not yet executed |
| 97 | `permission_health.rs` | `a_failed_paste_under_a_revoked_accessibility_grant_names_accessibility` | `src/permissions.rs` | `        named(ACCESSIBILITY_OFF_DETAIL)` | `named(ACCESSIBILITY_OFF_DETAIL)` | `fallback.to_string()` | not yet executed |
| 98 | `permission_transitions.rs` | `the_event_name_is_the_one_the_frontend_listens_to` | `src/permissions.rs` | `pub const PERMISSION_CHANGED_EVENT: &str = "permission_changed";` | `"permission_changed"` | `"permissions_changed"` | not yet executed |
| 99 | `pill_idle_tick_during_recording.rs` | `dictation_keeps_the_fast_tick` | `src/float_pill.rs` | `    if dictating {` | `if dictating` | `if false && dictating` | not yet executed |
| 100 | `polish_envelope.rs` | `polish_envelope_is_skipped_with_a_named_reason_when_the_weights_are_absent` | `` | `` | `` | `(empty)` | exempt: measurement-only — the one always-run test checks a helper defined in the test itself; the other two measure the real yap-polish binary + weights and assert a budget, so no single source line flips them |
| 101 | `polish_long_form.rs` | `the_chunk_ceiling_keeps_text_it_will_not_polish` | `src/polish.rs` | `pub const MAX_POLISH_CHUNKS: usize = 40;` | `= 40;` | `= 4000;` | not yet executed |
| 102 | `polish_model_install.rs` | `missing_sidecar_binary_is_reported_once_not_per_take` | `src/polish.rs` | `    if MISSING_SIDECAR_REPORTED.swap(true, Ordering::SeqCst) {` | `if MISSING` | `if false && MISSING` | not yet executed |
| 103 | `rebind_notifies_status.rs` | `successful_rebind_reemits_status_with_new_label` | `src/hotkeys.rs` | `    set_active_dictate(new, new_label);` | `set_active_dictate(new, new_label);` | `let _ = (new, new_label);` | not yet executed |
| 104 | `release_smoke_lines.rs` | `parser_enforces_the_matrix` | `../../scripts/qa/release-smoke.sh` | `*) FAILED="${FAILED:+$FAILED }$1" ;; esac` | `FAILED="${FAILED:+$FAILED }$1"` | `true` | not yet executed |
| 105 | `settings_kv.rs` | `the_two_keys_are_different_versioned_names` | `src/meetings.rs` | `pub const SYSTEM_AUDIO_SETUP_ACK_KEY: &str` | `_ack_v1"` | `_ack_v2"` | not yet executed |
| 106 | `settings_migration.rs` | `a_v1_store_with_light_loads_as_medium` | `src/lib.rs` | `if stored_version < 2 && settings.cleanup_level.trim().eq_ignore_ascii_case("light") {` | `stored_version < 2` | `stored_version < 1` | not yet executed |
| 107 | `sherpa_load_smoke.rs` | `the_sidecar_refuses_bad_model_bytes_and_keeps_stdout_pure_json` | `src/diarize_protocol.rs` | `pub const ERR_MODEL_LOAD_FAILED: &str = "model_load_failed";` | `= "model_load_failed";` | `= "model_load_failed_mut";` | not yet executed |
| 108 | `shipped_defaults.rs` | `shipped_defaults_table` | `src/lib.rs` | `            auto_paste: true,` | `auto_paste: true` | `auto_paste: false` | not yet executed |
| 109 | `silent_capture.rs` | `info_plist_keeps_every_tcc_usage_string` | `Info.plist` | `<key>NSAppleEventsUsageDescription</key>` | `NSAppleEventsUsageDescription` | `NSAppleEventsUsage` | not yet executed |
| 110 | `summarize_e2e.rs` | `summarize_e2e_over_the_yv90_lecture_fixture` | `src/summarize.rs` | `("### Action items", actions),` | `### Action items` | `### Acti~on items` | not yet executed |
| 111 | `summarize_e2e_speaker_attributed.rs` | `summarize_e2e_speaker_attributed` | `src/summarize.rs` | `        format!("{}{}", self.prefix(), one_line(&self.text))` | `self.prefix(), ` | `"", ` | not yet executed |
| 112 | `summarize_empty_is_never_stored.rs` | `a_grammar_legal_empty_map_answer_is_an_error_not_a_stored_summary` | `src/summarize.rs` | `        return Err(SummaryError::Empty);` | `SummaryError::Empty` | `SummaryError::Protocol` | not yet executed |
| 113 | `summarize_grammar_chain.rs` | `a_constrained_pass_produces_well_formed_schema_json` | `src/polish_protocol.rs` | `//!  "deadline_ms":30000,"text":"seg_0001: …","grammar":"root ::= …"}` | `seg_0001` | `seg_~0001` | not yet executed |
| 114 | `summarize_grammar_unchanged_shape.rs` | `the_grammar_has_no_rule_yv97_did_not_ship` | `src/summarize.rs` | `        .join(" \| ");` | `" \| "` | `" \|  "` | not yet executed |
| 115 | `summarize_map_extraction.rs` | `map_items_cite_only_their_own_chunk` | `src/polish_protocol.rs` | `pub const KIND_SUMMARIZE: &str = "summarize";` | `= "summarize";` | `= "summarize_mut";` | not yet executed |
| 116 | `summarize_reduce_budget.rs` | `a_truncated_map_pass_is_rendered_not_only_logged` | `src/summarize.rs` | `"_{truncated_chunks} of {chunks} parts of this meeting were too long for one pass and were truncated._\n"` | `were too long for one pass` | `were too long~ for one pass` | not yet executed |
| 117 | `summarize_speaker_attribution_from_lookup.rs` | `no_speaker_source_renders_exactly_what_yv97_rendered` | `src/polish_protocol.rs` | `//!  "deadline_ms":30000,"text":"seg_0001: …","grammar":"root ::= …"}` | `seg_0001: ` | `seg_0~001: ` | not yet executed |
| 118 | `summarize_token_chunking.rs` | `an_empty_transcript_is_an_error_not_an_empty_summary` | `src/summarize.rs` | `) -> Result<Vec<Chunk>, SummaryError> {` | `SummaryError> {` | `SummaryError> { if lines.is_empty() { return Ok(Vec::new()); }` | not yet executed |
| 119 | `summarize_validators_v1_v7.rs` | `a_fabricated_due_date_or_address_is_rejected` | `src/summarize.rs` | `    common_gates(source, item)?;` | `common_gates(source, item)?;` | `let _ = common_gates(source, item);` | not yet executed |
| 120 | `supply_chain.rs` | `csp_is_a_real_policy_not_null` | `tauri.conf.json` | `"csp": "default-src` | `object-src 'none'` | `object-src 'self'` | not yet executed |
| 121 | `support_bundle_contents.rs` | `the_zip_carries_the_crash_summary_the_logs_and_the_version_block` | `src/polish.rs` | `log::info!("polish sidecar starting");` | `polish sidecar starting` | `polish sidec~ar starting` | not yet executed |
| 122 | `support_bundle_redaction.rs` | `filesystem_paths_do_not_survive_the_bundle` | `src/support.rs` | `fn redact_paths(body: &str) -> String {` | `-> String {` | `-> String { #[allow(unreachable_code)] return body.to_string();` | not yet executed |
| 123 | `syscapture_aggregate_dict.rs` | `the_declared_key_list_is_still_complete_off_macos` | `src/syscapture.rs` | `        keys::SUB_TAP_UID,` | `keys::SUB_TAP_UID,` | `(empty)` | not yet executed |
| 124 | `syscapture_format_binding.rs` | `the_binding_is_its_own_named_stage_between_the_two_it_sits_between` | `src/syscapture.rs` | `TapStage::BindCapture => "bind kAudioTapPropertyFormat to the capture ring",` | `"bind kAudioTapPropertyFormat to the capture ring"` | `"bind the tap format"` | not yet executed |
| 125 | `syscapture_ghost_watchdog_false_positives.rs` | `a_call_that_goes_quiet_after_real_audio_is_not_rebuilt_either` | `src/syscapture.rs` | `match (self.ever_nonzero, output_was_active_during_silence) {` | `(self.ever_nonzero,` | `(false,` | not yet executed |
| 126 | `syscapture_ghost_watchdog_rebuild_budget.rs` | `exactly_three_rebuilds_then_a_degrade_and_then_silence_from_the_watchdog` | `src/syscapture.rs` | `pub const MAX_TAP_REBUILDS_PER_MEETING: u32 = 3;` | `= 3;` | `= 4;` | not yet executed |
| 127 | `syscapture_ghost_watchdog_rebuild_first.rs` | `silence_while_the_system_is_audibly_playing_is_still_a_rebuild_not_a_denial` | `src/syscapture.rs` | `matches!(self, TapWatchdogAction::RebuildFull { .. })` | `RebuildFull` | `DegradeTrackLost` | not yet executed |
| 128 | `syscapture_ioproc_catch_unwind.rs` | `a_panic_inside_the_ring_push_sets_the_same_one_flag` | `src/syscapture.rs` | `            capture.note_callback_panic();` | `capture.note_callback_panic();` | `let _ = capture;` | not yet executed |
| 129 | `syscapture_ioproc_rt_safety.rs` | `the_tap_ioproc_body_allocates_nothing` | `src/meeting.rs` | `        let taken_now = taken + (accepted / channels) as u64;` | `(accepted / channels) as u64;` | `(accepted / channels) as u64; std::hint::black_box(Vec::<u8>::with_capacity(64));` | not yet executed |
| 130 | `syscapture_manual_smoke.rs` | `(none)` | `` | `` | `` | `(empty)` | exempt: ignored-only — every #[test] in the file is #[ignore]d; no default run can go red |
| 131 | `syscapture_never_delivered_denied_state.rs` | `a_tap_whose_ioproc_never_fired_is_the_same_verdict` | `src/syscapture.rs` | `    } else if ran_for >= DENIAL_GRACE {` | `ran_for >= DENIAL_GRACE` | `ran_for > DENIAL_GRACE * 100` | not yet executed |
| 132 | `syscapture_never_zero_discriminator.rs` | `a_rebuild_cannot_erase_the_memory_that_audio_once_arrived` | `src/syscapture.rs` | `pub const MAX_TAP_REBUILDS_PER_MEETING: u32 = 3;` | `= 3;` | `= 2;` | not yet executed |
| 133 | `syscapture_os_gate.rs` | `the_boundary_is_inclusive` | `src/os_version_gate.rs` | `    if os >= OsVersion::SYSTEM_AUDIO_MIN {` | `os >= ` | `os > ` | not yet executed |
| 134 | `syscapture_prewarm_tap.rs` | `the_dwell_is_two_hundred_milliseconds` | `src/syscapture.rs` | `pub const PREWARM_DWELL: Duration = Duration::from_millis(200);` | `from_millis(200)` | `from_millis(250)` | not yet executed |
| 135 | `syscapture_teardown_order.rs` | `a_running_tap_tears_down_all_four_steps_in_order` | `src/syscapture.rs` | `        steps.push(TapStep::AudioDeviceStop);` | `steps.push(TapStep::AudioDeviceStop);` | `(empty)` | not yet executed |
| 136 | `system_audio_verdict_silence_is_not_denial.rs` | `output_ran_but_under_sixty_seconds_is_unknown` | `src/syscapture.rs` | `    } else if output_was_running >= OUTPUT_RAN_FOR_DENIAL {` | `>= OUTPUT_RAN_FOR_DENIAL` | `>= OUTPUT_RAN_FOR_DENIAL / 2` | not yet executed |
| 137 | `tap_health.rs` | `a_user_input_disable_is_classified_and_re_armed` | `src/ptt_macos.rs` | `        KCG_EVENT_TAP_DISABLED_BY_USER_INPUT => Some(TapDisable::UserInput),` | `Some(TapDisable::UserInput)` | `Some(TapDisable::Timeout)` | not yet executed |
| 138 | `transcribe_progress_events.rs` | `the_event_name_is_the_one_the_pill_listens_for` | `src/transcribe_progress.rs` | `pub const MIN_REPORTABLE_CHUNKS: u32 = 2;` | `= 2;` | `= 3;` | not yet executed |
| 139 | `tray_hotkey_no_collision.rs` | `the_meeting_toggle_is_distinct_from_the_dictation_binding` | `src/shortcuts.rs` | `        self.chord() == other.chord()` | `self.chord() == other.chord()` | `self.chord() == other.chord() \|\| true` | not yet executed |
| 140 | `tray_license.rs` | `the_upgrade_item_is_absent_early_in_the_trial` | `src/license.rs` | `Entitlement::Trial { days_left, .. } => (*days_left).max(0) <= PILL_SHOW_DAYS,` | `<= PILL_SHOW_DAYS` | `< PILL_SHOW_DAYS` | not yet executed |
| 141 | `trial_state_machine.rs` | `clock_rolled_back_cannot_rewind_the_trial` | `src/license.rs` | `    let recorded_floor = match (inputs.file_floor_ms, inputs.db_floor_ms) {` | `(inputs.file_floor_ms, inputs.db_floor_ms)` | `(None::<i64>, None::<i64>)` | not yet executed |
| 142 | `two_track_capture_consumer.rs` | `a_tap_block_into_a_mic_only_meeting_is_dropped_not_folded_into_track_zero` | `src/meeting.rs` | `    pub fn accept_track(&self, track: usize, interleaved: &[f32], anchors: &[CaptureAnchor]) {` | `anchors: &[CaptureAnchor]) {` | `anchors: &[CaptureAnchor]) { let track = track.min(0);` | not yet executed |
| 143 | `two_track_journal_round_trip.rs` | `a_mic_only_meeting_is_byte_for_byte_what_22a_produced` | `src/meeting.rs` | `const MEETING_INDEX_EXT: &str = "index.jsonl";` | `"index.jsonl"` | `"idx.jsonl"` | not yet executed |
| 144 | `two_track_merge_no_regression_single_track.rs` | `a_mic_only_meeting_is_byte_for_byte_what_merge_timed_produces` | `src/meetings.rs` | `pub const UNCLUSTERED_SPEAKER_LABEL: &str = "Speaker";` | `= "Speaker";` | `= "Speaker_mut";` | not yet executed |
| 145 | `two_track_merge_ordering.rs` | `the_merge_labels_through_the_shipped_render_rule` | `src/meetings.rs` | `pub const MIC_SPEAKER_LABEL: &str = "Me";` | `= "Me";` | `= "Me_mut";` | not yet executed |
| 146 | `two_track_merge_synthetic_drift.rs` | `a_hundred_ppm_offset_merges_in_the_right_order_and_under_fifty_ms_at_three_hours` | `src/meeting_asr.rs` | `    let target = crate::meetings::diarization_target(kind, !spans_b.is_empty());` | `!spans_b.is_empty()` | `false` | not yet executed |
| 147 | `two_track_phase_e2e.rs` | `a_two_source_clock_offset_meeting_renders_one_ordered_me_them_transcript` | `src/meeting_asr.rs` | `    let target = crate::meetings::diarization_target(kind, !spans_b.is_empty());` | `!spans_b.is_empty()` | `false` | not yet executed |
| 148 | `two_track_true_rate_logging.rs` | `the_measured_rate_is_delta_samples_over_delta_host_seconds` | `src/record.rs` | `pub const TARGET_RATE: u32 = 16_000;` | `= 16_000;` | `= 16_000 + 1;` | not yet executed |
| 149 | `undo_restores_raw.rs` | `undo_declines_when_the_pipeline_changed_nothing` | `src/dictation.rs` | `    if raw.trim().is_empty() \|\| raw.trim() == polished.trim() {` | ` \|\| raw.trim() == polished.trim()` | `(empty)` | not yet executed |
| 150 | `updater_endpoint.rs` | `at_least_two_endpoints` | `tauri.conf.json` | `"https://yap-lemon.vercel.app/updates/latest.json",` | `"https://yap-lemon.vercel.app/updates/latest.json",` | `(empty)` | not yet executed |
| 151 | `vocab_corpus_is_literals_only.rs` | `the_licensing_implementation_comments_are_not_in_the_corpus` | `src/vocab_extract.rs` | `            '/' if i + 1 < n && c[i + 1] == '/' => i = skip_line_comment(&c, i),` | `'/' if i + 1` | `'/' if false && i + 1` | not yet executed |
<!-- ledger:end -->
