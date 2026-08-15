//! YV100 — a fake [`TapPlatform`], so the tap's setup/teardown contract is
//! provable with **no audio device, no macOS 14.4 and no TCC grant**.
//!
//! This is the whole point of the platform seam. The real CoreAudio calls need
//! a Mac that has shown a permission dialog; the *order* those calls happen in
//! is the part that leaks a process tap or strands the user's output device
//! inside an orphaned aggregate, and the order is pure logic. So the order is
//! tested here, on CI, on every commit, and the FFI is the only thing left that
//! needs hardware.
//!
//! Lives under `tests/support/` (and is `#[path]`-included) so cargo does not
//! build it as a test target of its own — the same posture `support/meeting.rs`
//! already takes for the stub decoder.
#![allow(dead_code)]

use std::sync::Arc;

use wilson_voice_lib::meeting::RtCapture;
use wilson_voice_lib::syscapture::{
    capture_for_format, capture_matches_format, DictValue, IoProcToken, TapError, TapFormat,
    TapPlatform,
};

/// Every call the state machine can make, recorded in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    CreateTap { excluded: Vec<u32> },
    TapUid,
    TapFormat,
    BindCapture,
    DefaultOutputUid,
    CreateAggregate,
    CreateIoProc,
    Start,
    Stop,
    DestroyIoProc,
    DestroyAggregate,
    DestroyTap,
}

/// Which step should fail, and how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fail {
    Never,
    /// Return a non-zero `OSStatus` from this step.
    Status(&'static str),
    /// Panic inside this step — the "panic mid-setup" exit path.
    Panic(&'static str),
}

pub struct FakePlatform {
    pub calls: Vec<Call>,
    pub description: Option<Vec<(String, DictValue)>>,
    pub fail_at: Fail,
    pub format: TapFormat,
    pub output_uid: String,
    pub tap_uid: String,
    /// Bind THIS ring instead of one built from the tap's format.
    ///
    /// The only way to reach the mismatch path deliberately: the production
    /// platform builds its ring from the format and therefore cannot disagree
    /// with itself, so proving the guard fires needs a fake that can.
    pub capture_override: Option<Arc<RtCapture>>,
    /// The ring that was actually bound, once `bind_capture` has run — what the
    /// IOProc would have stamped through.
    pub bound_capture: Option<Arc<RtCapture>>,
}

impl Default for FakePlatform {
    fn default() -> Self {
        Self {
            calls: Vec::new(),
            description: None,
            fail_at: Fail::Never,
            format: TapFormat {
                sample_rate: 48_000,
                channels: 1,
            },
            output_uid: "BuiltInSpeakerDevice".to_string(),
            tap_uid: "11111111-2222-3333-4444-555555555555".to_string(),
            capture_override: None,
            bound_capture: None,
        }
    }
}

impl FakePlatform {
    pub fn failing_with_status(step: &'static str) -> Self {
        Self {
            fail_at: Fail::Status(step),
            ..Self::default()
        }
    }

    pub fn panicking_at(step: &'static str) -> Self {
        Self {
            fail_at: Fail::Panic(step),
            ..Self::default()
        }
    }

    /// Record the call, then apply whatever failure this fake was built for.
    /// `-4` is `kAudio_UnimplementedError`, a real status rather than a
    /// made-up one, so an assertion on it reads like a log line would.
    fn step(&mut self, name: &'static str, call: Call) -> Result<(), i32> {
        self.calls.push(call);
        match self.fail_at {
            Fail::Status(at) if at == name => Err(-4),
            Fail::Panic(at) if at == name => panic!("injected panic in {name}"),
            _ => Ok(()),
        }
    }

    /// The teardown calls, in the order they were made.
    pub fn teardown_calls(&self) -> Vec<Call> {
        self.calls
            .iter()
            .filter(|c| {
                matches!(
                    c,
                    Call::Stop | Call::DestroyIoProc | Call::DestroyAggregate | Call::DestroyTap
                )
            })
            .cloned()
            .collect()
    }
}

impl TapPlatform for FakePlatform {
    fn create_tap(&mut self, exclude_process_objects: &[u32]) -> Result<u32, i32> {
        self.step(
            "create_tap",
            Call::CreateTap {
                excluded: exclude_process_objects.to_vec(),
            },
        )?;
        Ok(9001)
    }

    fn tap_uid(&mut self, _tap: u32) -> Result<String, i32> {
        self.step("tap_uid", Call::TapUid)?;
        Ok(self.tap_uid.clone())
    }

    fn tap_format(&mut self, _tap: u32) -> Result<TapFormat, i32> {
        self.step("tap_format", Call::TapFormat)?;
        Ok(self.format)
    }

    fn bind_capture(&mut self, format: TapFormat) -> Result<(), TapError> {
        self.calls.push(Call::BindCapture);
        // The same two lines the real platform runs, in the same order: build
        // from the tap's own format, then check the constructed ring against it.
        // The fake exists to make the ORDER testable, so it must not shortcut
        // the step whose whole purpose is to happen at a particular moment.
        let capture = self
            .capture_override
            .clone()
            .unwrap_or_else(|| capture_for_format(format));
        capture_matches_format(&capture, format)?;
        self.bound_capture = Some(capture);
        Ok(())
    }

    fn default_output_uid(&mut self) -> Result<String, i32> {
        self.step("default_output_uid", Call::DefaultOutputUid)?;
        Ok(self.output_uid.clone())
    }

    fn create_aggregate(&mut self, description: &[(String, DictValue)]) -> Result<u32, i32> {
        self.description = Some(description.to_vec());
        self.step("create_aggregate", Call::CreateAggregate)?;
        Ok(9002)
    }

    fn create_ioproc(&mut self, _aggregate: u32, _format: TapFormat) -> Result<IoProcToken, i32> {
        self.step("create_ioproc", Call::CreateIoProc)?;
        Ok(1)
    }

    fn start(&mut self, _aggregate: u32, _ioproc: IoProcToken) -> Result<(), i32> {
        self.step("start", Call::Start)
    }

    fn stop(&mut self, _aggregate: u32, _ioproc: IoProcToken) {
        self.calls.push(Call::Stop);
    }

    fn destroy_ioproc(&mut self, _aggregate: u32, _ioproc: IoProcToken) {
        self.calls.push(Call::DestroyIoProc);
    }

    fn destroy_aggregate(&mut self, _aggregate: u32) {
        self.calls.push(Call::DestroyAggregate);
    }

    fn destroy_tap(&mut self, _tap: u32) {
        self.calls.push(Call::DestroyTap);
    }
}
