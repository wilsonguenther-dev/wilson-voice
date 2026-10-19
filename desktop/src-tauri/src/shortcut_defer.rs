//! yap-p0-chord-deadlock — a global shortcut is ROUTED inside the plugin's
//! handler and ACTED ON only after that handler has returned.
//!
//! Why: `tauri-plugin-global-shortcut` 2.3.2 calls the app's handler while it
//! still holds its shortcut table lock — `if let Some(shortcut) =
//! shortcuts_.lock().unwrap().get(&e.id) { … handler(…) }` (plugin
//! `src/lib.rs:417-422`; the `if let` keeps the guard alive for the whole
//! block). Every `GlobalShortcut` method takes that same non-reentrant
//! `std::sync::Mutex` (`is_registered` at `:237`, `register` at `:97`,
//! `unregister` at `:188`). So a take started inline from the ⌃⌥⌘Space chord —
//! `take_hotkey_edge` → `start_recording` → `register_cancel_shortcut` →
//! `global_shortcut().is_registered()` — parked the main thread in
//! `_pthread_mutex_firstfit_lock_slow` forever on the first press (live walk
//! 2026-10-19, `04-chord-deadlock-sample.txt`), and every hotkey after it was
//! dead.
//!
//! The fix is the fn tap's pattern (`ptt_macos` → `run_on_main_thread` from
//! the tap thread): the handler only turns the event into a [`ShortcutJob`] and
//! posts it to ONE pump thread, which forwards each job, in order, to the main
//! thread. From a non-main thread `run_on_main_thread` QUEUES the job on the
//! event loop, so it runs after the plugin's handler — and its lock — are gone.
//! Calling `run_on_main_thread` straight from the handler would NOT do:
//! tauri-runtime-wry's `send_user_message` runs the closure INLINE when the
//! caller already is the main thread, which the handler always is.

use std::sync::mpsc;

use tauri_plugin_global_shortcut::Shortcut;

use crate::hotkeys::HotkeyEdge;
use crate::shortcuts;

/// What a global shortcut asks the app to do. Carries no handles: it is a
/// value that crosses the pump thread and runs later on the main thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutJob {
    /// The dictate chord (or the legacy ⌘⇧V): one edge for `take_hotkey_edge`.
    Take(HotkeyEdge),
    /// YV95 ⌃⌘M — toggle a meeting (key-down only).
    MeetingToggle,
    /// Y3-D esc — cancel the take in flight (key-down only).
    Cancel,
    /// ⌃⌘V — paste the last transcript (key-down only).
    PasteLast,
    /// YV51 ⌃⌘Z — re-paste the raw take (key-down only).
    UndoAiEdit,
}

/// Map one shortcut event to its job. Pure — no app state, no plugin API — so
/// it is safe inside the plugin's handler. `keep_cmd_shift_v` is read only for
/// a shortcut none of the fixed bindings claimed (the legacy ⌘⇧V fallthrough).
pub fn route(
    shortcut: &Shortcut,
    pressed: bool,
    dictate: &Shortcut,
    keep_cmd_shift_v: impl FnOnce() -> bool,
) -> Option<ShortcutJob> {
    let edge = if pressed {
        HotkeyEdge::Press
    } else {
        HotkeyEdge::Release
    };
    if shortcut == dictate {
        return Some(ShortcutJob::Take(edge));
    }
    let on_press = |job| pressed.then_some(job);
    if shortcut == &crate::hotkey_table::active_meeting().0 {
        return on_press(ShortcutJob::MeetingToggle);
    }
    if shortcut == &crate::hotkey_table::active_cancel().0 {
        return on_press(ShortcutJob::Cancel);
    }
    if shortcut == &shortcuts::PASTE_LAST.shortcut() {
        return on_press(ShortcutJob::PasteLast);
    }
    if shortcut == &shortcuts::UNDO_AI_EDIT.shortcut() {
        return on_press(ShortcutJob::UndoAiEdit);
    }
    // ⌘⇧V is a Carbon chord too: same dispatcher, so a held ⌘⇧V is
    // hold-to-talk and there is no second start/stop path.
    keep_cmd_shift_v().then_some(ShortcutJob::Take(edge))
}

/// One named thread that forwards jobs in the order they were posted.
/// `post` never blocks and never runs the job on the caller's thread.
pub struct EdgePump<J> {
    tx: mpsc::Sender<J>,
}

impl<J: Send + 'static> EdgePump<J> {
    pub fn spawn(name: &str, forward: impl Fn(J) + Send + 'static) -> Self {
        let (tx, rx) = mpsc::channel::<J>();
        std::thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                for job in rx {
                    forward(job);
                }
            })
            .expect("spawn the global-shortcut pump thread");
        Self { tx }
    }

    pub fn post(&self, job: J) {
        if self.tx.send(job).is_err() {
            log::warn!("global-shortcut pump is gone — shortcut dropped");
        }
    }
}

/// THE global-shortcut handler body. `ctx` is whatever `forward` needs (the
/// app handle in production); `forward(ctx, job)` runs on the pump thread and
/// must hand the job to the main thread (`run_on_main_thread`). Nothing here
/// touches the plugin, so nothing here can re-enter its lock.
pub fn handler<C: Send + 'static>(
    forward: impl Fn(C, ShortcutJob) + Send + 'static,
    keep_cmd_shift_v: impl Fn() -> bool + Send + Sync + 'static,
    dictate: impl Fn() -> Shortcut + Send + Sync + 'static,
) -> impl Fn(C, &Shortcut, bool) + Send + Sync + 'static {
    let pump = EdgePump::spawn("wv-shortcut-pump", move |(ctx, job)| forward(ctx, job));
    move |ctx, shortcut, pressed| {
        if let Some(job) = route(shortcut, pressed, &dictate(), &keep_cmd_shift_v) {
            pump.post((ctx, job));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::thread::{self, ThreadId};
    use std::time::Duration;

    type Work = Box<dyn FnOnce() + Send>;

    /// The plugin's shortcut table lock: held across the app handler (plugin
    /// `lib.rs:417`) and taken by every `GlobalShortcut` method.
    type PluginLock = Arc<Mutex<()>>;

    /// tauri-runtime-wry's `run_on_main_thread`, faithfully: INLINE when the
    /// caller is the main thread, queued on the event loop otherwise.
    fn run_on_main_thread(main: ThreadId, event_loop: &mpsc::Sender<Work>, work: Work) {
        if thread::current().id() == main {
            work();
        } else {
            let _ = event_loop.send(work);
        }
    }

    /// The job's act half as far as the deadlock goes: `run_shortcut_job` →
    /// `take_hotkey_edge` → `start_recording` → `register_cancel_shortcut` →
    /// `global_shortcut().is_registered()` — a blocking `lock()` on the
    /// plugin's mutex.
    fn act(plugin: &PluginLock, ran: &Arc<Mutex<Vec<ShortcutJob>>>, job: ShortcutJob) {
        let _table = plugin.lock().unwrap();
        ran.lock().unwrap().push(job);
    }

    /// Drive a ⌃⌥⌘Space press + release through the REAL handler with the
    /// plugin's lock held, exactly as the plugin dispatches it, then let the
    /// event loop run. Before the fix the handler acted inline on the main
    /// thread and `lock()` parked forever; the outer timeout turns that hang
    /// into a failure.
    #[test]
    fn chord_edges_run_after_the_plugin_lock_is_released_never_inside_it() {
        let (done_tx, done_rx) = mpsc::channel();
        thread::spawn(move || {
            let main = thread::current().id();
            let plugin: PluginLock = Arc::new(Mutex::new(()));
            let ran = Arc::new(Mutex::new(Vec::new()));
            let (loop_tx, loop_rx) = mpsc::channel::<Work>();
            let on_shortcut = {
                let (plugin, ran, loop_tx) = (plugin.clone(), ran.clone(), loop_tx.clone());
                handler(
                    move |(), job| {
                        let (plugin, ran) = (plugin.clone(), ran.clone());
                        run_on_main_thread(
                            main,
                            &loop_tx,
                            Box::new(move || act(&plugin, &ran, job)),
                        );
                    },
                    || false,
                    || shortcuts::DICTATE_TOGGLE.shortcut(),
                )
            };
            let chord = shortcuts::DICTATE_TOGGLE.shortcut();
            for pressed in [true, false] {
                let _held = plugin.lock().unwrap(); // plugin lib.rs:417
                on_shortcut((), &chord, pressed);
                assert!(
                    ran.lock().unwrap().is_empty(),
                    "a job ran inside the plugin's handler"
                );
            }
            // The event loop's next turns: the queued jobs, in order.
            for _ in 0..2 {
                let work = loop_rx
                    .recv_timeout(Duration::from_secs(2))
                    .expect("the pump forwarded the edge to the main thread");
                work();
            }
            done_tx.send(ran.lock().unwrap().clone()).unwrap();
        });
        let ran = done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("DEADLOCK: a chord edge re-entered the global-shortcut plugin's lock");
        assert_eq!(
            ran,
            vec![
                ShortcutJob::Take(HotkeyEdge::Press),
                ShortcutJob::Take(HotkeyEdge::Release)
            ]
        );
    }

    /// Negative control: the fake is strict enough to catch the old code. The
    /// pre-fix handler acted on the main thread while the plugin's lock was
    /// held, so the act half's lock attempt finds it TAKEN.
    #[test]
    fn acting_inline_in_the_handler_meets_the_held_plugin_lock() {
        let plugin: PluginLock = Arc::new(Mutex::new(()));
        let (loop_tx, _loop_rx) = mpsc::channel::<Work>();
        let main = thread::current().id();
        let saw_held = Arc::new(Mutex::new(None));
        let _held = plugin.lock().unwrap();
        let (p, s) = (plugin.clone(), saw_held.clone());
        run_on_main_thread(
            main,
            &loop_tx,
            Box::new(move || *s.lock().unwrap() = Some(p.try_lock().is_err())),
        );
        assert_eq!(*saw_held.lock().unwrap(), Some(true));
    }

    #[test]
    fn route_keeps_every_binding_and_its_edge_rule() {
        let dictate = shortcuts::DICTATE_TOGGLE.shortcut();
        let r = |sc: &Shortcut, pressed, legacy| route(sc, pressed, &dictate, || legacy);
        assert_eq!(
            r(&dictate, true, false),
            Some(ShortcutJob::Take(HotkeyEdge::Press))
        );
        assert_eq!(
            r(&dictate, false, false),
            Some(ShortcutJob::Take(HotkeyEdge::Release))
        );
        for (b, job) in [
            (&shortcuts::MEETING_TOGGLE, ShortcutJob::MeetingToggle),
            (&shortcuts::CANCEL, ShortcutJob::Cancel),
            (&shortcuts::PASTE_LAST, ShortcutJob::PasteLast),
            (&shortcuts::UNDO_AI_EDIT, ShortcutJob::UndoAiEdit),
        ] {
            assert_eq!(r(&b.shortcut(), true, true), Some(job), "{}", b.label);
            assert_eq!(r(&b.shortcut(), false, true), None, "{} release", b.label);
        }
        // The legacy ⌘⇧V fallthrough follows the setting.
        let other = shortcuts::DICTATE_TOGGLE.shortcut();
        let legacy_dictate = shortcuts::MEETING_TOGGLE.shortcut();
        assert_eq!(route(&other, true, &legacy_dictate, || false), None);
        assert_eq!(
            route(&other, true, &legacy_dictate, || true),
            Some(ShortcutJob::Take(HotkeyEdge::Press))
        );
    }
}
