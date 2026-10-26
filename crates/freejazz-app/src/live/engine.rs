//! The effects engine: one thread renders the effect at 25 fps (30 at most, the rate
//! measured without margin on the keyboard) and sends each frame to the device worker.
//! A frame that cannot be sent in time is dropped, never queued. The stream always ends
//! with `LiveEnd` (the stored effect written back): on `stop`, on app exit, in the panic
//! hook, and after a crash through the marker the device worker keeps in
//! `settings.json` (`device::Manager::with_marker`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use freejazz_protocol::live::{FRAME_SLOTS, LIVE_AMPLITUDE};
use serde_json::Value;

use super::audio::AudioCapture;
use super::input::{self, Subscription};
use super::system::SystemMonitor;
use super::{BLACK, Effect, Frame, FrameCtx, KeyPress, Spectrum, SystemStats};
use crate::device::Request;
use crate::keyboard::Layout;

pub const DEFAULT_FPS: f32 = 25.0;
pub const MAX_FPS: f32 = 30.0;

/// Depths shared with whoever can read them (normalised 0..1 per key slot).
pub type SharedDepths = Arc<Mutex<Option<Box<[f32; FRAME_SLOTS]>>>>;

#[derive(Debug, Clone, Copy)]
pub struct EngineConfig {
    pub fps: f32,
    /// Install the global key hook (only while the engine runs).
    pub hook: bool,
    /// Capture audio / sample the system, when the effect needs them.
    pub audio: bool,
    pub system: bool,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            fps: DEFAULT_FPS,
            hook: true,
            audio: true,
            system: true,
        }
    }
}

impl EngineConfig {
    /// No hook, no capture: for tests and `--mock`.
    pub fn headless() -> Self {
        Self {
            fps: MAX_FPS,
            hook: false,
            audio: false,
            system: false,
        }
    }
}

/// Renders an effect frame by frame. The engine thread and the in-app preview both use
/// it, so what the preview shows is what the keyboard gets.
pub struct Renderer {
    effect: Box<dyn Effect>,
    last_t: Option<f32>,
}

impl Renderer {
    pub fn new(effect: Box<dyn Effect>) -> Self {
        Self {
            effect,
            last_t: None,
        }
    }

    pub fn effect(&self) -> &dyn Effect {
        &*self.effect
    }

    /// One frame at time `t` (the [`input::now`] clock).
    pub fn render(
        &mut self,
        t: f32,
        presses: &[KeyPress],
        depths: Option<&[f32; FRAME_SLOTS]>,
        audio: Option<&Spectrum>,
        system: Option<&SystemStats>,
    ) -> Frame {
        let dt = self.last_t.map_or(0.0, |l| (t - l).clamp(0.0, 0.25));
        self.last_t = Some(t);
        let ctx = FrameCtx {
            t,
            dt,
            layout: Layout::ak980max(),
            presses,
            depths,
            audio,
            system,
        };
        let mut out = [BLACK; FRAME_SLOTS];
        self.effect.frame(&ctx, &mut out);
        out
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The channels of the running engines, by engine id: every request of a stream goes
/// through its entry, so ending it (its own `stop`, or `end_active`) also silences its
/// render thread. An engine only ever ends its own stream.
static ACTIVE: Mutex<Vec<(u64, Sender<Request>)>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Sends `request` on the stream of engine `id`; `false` once that stream was ended.
fn send_as(id: u64, request: Request) -> bool {
    match lock(&ACTIVE).iter().find(|(i, _)| *i == id) {
        Some((_, tx)) => {
            let _ = tx.send(request);
            true
        }
        None => false,
    }
}

/// Ends the stream of engine `id` (`LiveEnd`), if it still runs.
fn end_own(id: u64) {
    let tx = {
        let mut active = lock(&ACTIVE);
        let pos = active.iter().position(|(i, _)| *i == id);
        pos.map(|p| active.swap_remove(p).1)
    };
    if let Some(tx) = tx {
        let _ = tx.send(Request::LiveEnd);
    }
}

/// Whether engine `id` still streams.
fn is_active(id: u64) -> bool {
    lock(&ACTIVE).iter().any(|(i, _)| *i == id)
}

/// Ends every running stream: `LiveEnd` is sent on each. Safe to call from anywhere
/// (exit, panic hook). The crash marker is cleared by the device worker once the
/// stored effect is written back, not here.
pub fn end_active() {
    let all = std::mem::take(&mut *lock(&ACTIVE));
    for (_, tx) in all {
        let _ = tx.send(Request::LiveEnd);
    }
}

/// For a panic hook: ends the streams, then lets the device worker end (it writes the
/// stored lighting back, for a live stream or a preview) before the process goes.
pub fn emergency_end() {
    end_active();
    crate::device::emergency_stop(Duration::from_millis(1500));
}

/// Chains [`emergency_end`] in front of the current panic hook.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        emergency_end();
        previous(info);
    }));
}

/// Tests that start engines run one at a time (the registry is global).
#[cfg(test)]
pub(crate) static TEST_SERIAL: Mutex<()> = Mutex::new(());

#[cfg(test)]
pub(crate) fn test_serial() -> MutexGuard<'static, ()> {
    lock(&TEST_SERIAL)
}

pub struct Engine {
    id: u64,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    depths: SharedDepths,
    warnings: Vec<String>,
}

impl Engine {
    /// Starts the effect `effect_id` (see [`super::create`]) with `params`.
    pub fn start(
        tx: Sender<Request>,
        effect_id: &str,
        params: &Value,
        config: EngineConfig,
    ) -> Result<Engine, String> {
        let effect = super::create(effect_id, params)
            .ok_or_else(|| format!("Effet inconnu : {effect_id}"))?;
        Ok(Self::start_effect(tx, effect, effect_id, config))
    }

    /// Starts any effect (a custom one built from layers, for instance). `name` is for
    /// the user only.
    pub fn start_effect(
        tx: Sender<Request>,
        effect: Box<dyn Effect>,
        name: &str,
        config: EngineConfig,
    ) -> Engine {
        let _ = name;
        let needs = effect.needs();
        let mut warnings = Vec::new();
        let keys = if config.hook {
            input::subscribe().map_err(|e| warnings.push(e)).ok()
        } else {
            None
        };
        let audio = if config.audio && needs.audio {
            AudioCapture::start().map_err(|e| warnings.push(e)).ok()
        } else {
            None
        };
        let system = (config.system && needs.system).then(SystemMonitor::start);

        let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
        lock(&ACTIVE).push((id, tx));
        send_as(id, Request::LiveBegin);

        let stop = Arc::new(AtomicBool::new(false));
        let depths: SharedDepths = Arc::new(Mutex::new(None));
        let period = Duration::from_secs_f32(1.0 / config.fps.clamp(1.0, MAX_FPS));
        let thread = {
            let (stop, depths) = (Arc::clone(&stop), Arc::clone(&depths));
            std::thread::Builder::new()
                .name("freejazz-effects".into())
                .spawn(move || {
                    run(
                        id,
                        Renderer::new(effect),
                        keys,
                        audio,
                        system,
                        &depths,
                        &stop,
                        period,
                    )
                })
                .expect("spawn effects thread")
        };
        Engine {
            id,
            stop,
            thread: Some(thread),
            depths,
            warnings,
        }
    }

    /// Sources that could not start (no audio device, hook refused): the effect then runs
    /// without them.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Where to put key depths, when the keyboard gives them.
    pub fn depths(&self) -> SharedDepths {
        Arc::clone(&self.depths)
    }

    pub fn running(&self) -> bool {
        self.thread.is_some()
    }

    /// Stops the render thread (and with it the hook, the capture and the sampling), then
    /// ends its own stream: the keyboard goes back to its stored effect. Another
    /// engine's stream is never touched.
    pub fn stop(&mut self) {
        let Some(thread) = self.thread.take() else {
            return;
        };
        self.stop.store(true, Ordering::SeqCst);
        let _ = thread.join();
        end_own(self.id);
    }

    /// Whether its stream still runs (not ended by [`end_active`]).
    pub fn streaming(&self) -> bool {
        is_active(self.id)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    id: u64,
    mut renderer: Renderer,
    keys: Option<Subscription>,
    audio: Option<AudioCapture>,
    system: Option<SystemMonitor>,
    depths: &SharedDepths,
    stop: &AtomicBool,
    period: Duration,
) {
    let mut next = Instant::now();
    while !stop.load(Ordering::SeqCst) {
        let presses = keys.as_ref().map(Subscription::presses).unwrap_or_default();
        let spectrum = audio.as_ref().map(AudioCapture::spectrum);
        let stats = system.as_ref().map(SystemMonitor::stats);
        let depth = lock(depths).clone();
        let frame = renderer.render(
            input::now(),
            &presses,
            depth.as_deref(),
            spectrum.as_ref(),
            stats.as_ref(),
        );
        if !send_as(id, Request::LiveFrame(Box::new(frame), LIVE_AMPLITUDE)) {
            break; // ended from outside (`end_active`): nothing more to send
        }

        next += period;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            // late: drop the frames we missed instead of catching up
            next = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::channel;
    use std::time::Duration;

    use super::*;
    use crate::device::Manager;
    use crate::live::testing::*;
    use freejazz_protocol::reconnect::Wait;

    const FAST: Wait = Wait {
        timeout: Duration::from_millis(200),
        poll: Duration::from_millis(5),
    };

    #[test]
    fn stop_restores_stored_effect() {
        let _g = test_serial();
        let sim = crate::mock::Sim::new(&crate::mock::fixture());
        let dir = tempfile::tempdir().unwrap();
        let mut m = Manager::new(sim.port(), Some(dir.path().to_owned())).with_wait(FAST);
        m.tick();
        assert!(m.connected());

        let (tx, rx) = channel();
        let mut engine =
            Engine::start(tx, "ripple", &Value::Null, EngineConfig::headless()).unwrap();
        let id = engine.id;
        std::thread::sleep(Duration::from_millis(200));
        engine.stop();
        assert!(!engine.running());
        let requests: Vec<Request> = rx.try_iter().collect();

        assert!(matches!(requests.first(), Some(Request::LiveBegin)));
        assert!(matches!(requests.last(), Some(Request::LiveEnd)));
        let frames = requests
            .iter()
            .filter(|r| matches!(r, Request::LiveFrame(_, 0)))
            .count();
        assert!(frames >= 3, "{frames} frames in 200 ms");
        assert!(frames <= 8, "{frames} frames: more than 30 fps");
        let ends = requests
            .iter()
            .filter(|r| matches!(r, Request::LiveEnd))
            .count();
        assert_eq!(ends, 1);

        // played into the device manager: the stored effect is written back, once, last
        sim.clear_written();
        for r in requests {
            m.handle(r);
        }
        let written = sim.written();
        let led_sets: Vec<usize> = written
            .iter()
            .enumerate()
            .filter(|(_, r)| r[0] == 0xAA && r[1] == 0x23)
            .map(|(i, _)| i)
            .collect();
        assert!(!led_sets.is_empty(), "no SET_LED_EFFECT written");
        let last_frame = written.iter().rposition(|r| r[0] == 0xAA && r[1] == 0x32);
        assert!(last_frame.unwrap() < led_sets[0], "a frame after the end");
        // nothing more once ended
        assert!(!is_active(id));
    }

    #[test]
    fn end_active_silences_the_thread_and_sends_live_end() {
        let _g = test_serial();
        let (tx, rx) = channel();
        let mut engine =
            Engine::start(tx, "heatmap", &Value::Null, EngineConfig::headless()).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        end_active(); // what the panic hook does
        assert!(!engine.streaming());
        let at_end: Vec<Request> = rx.try_iter().collect();
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(rx.try_iter().count(), 0, "frames after LiveEnd");
        assert!(at_end.len() >= 2);
        assert!(matches!(at_end.last(), Some(Request::LiveEnd)));
        // its own stop sends nothing more: the stream is already ended
        engine.stop();
        assert_eq!(rx.try_iter().count(), 0);
    }

    #[test]
    fn an_engine_never_ends_another_engines_stream() {
        let _g = test_serial();
        let (tx, rx) = channel();
        let mut old =
            Engine::start(tx.clone(), "wave", &Value::Null, EngineConfig::headless()).unwrap();
        let new = Engine::start(tx, "ripple", &Value::Null, EngineConfig::headless()).unwrap();
        std::thread::sleep(Duration::from_millis(60));
        old.stop();
        assert!(new.streaming(), "the new stream still runs");
        let requests: Vec<Request> = rx.try_iter().collect();
        let ends = requests
            .iter()
            .filter(|r| matches!(r, Request::LiveEnd))
            .count();
        assert_eq!(ends, 1, "only the old stream ended");
        std::thread::sleep(Duration::from_millis(80));
        assert!(
            rx.try_iter().any(|r| matches!(r, Request::LiveFrame(..))),
            "the new engine keeps sending frames"
        );
        drop(new);
    }

    #[test]
    fn unknown_effect_is_refused() {
        let (tx, _rx) = channel();
        assert!(Engine::start(tx, "nope", &Value::Null, EngineConfig::headless()).is_err());
    }

    #[test]
    fn renderer_gives_the_same_frames_without_a_device() {
        let mut a = Renderer::new(crate::live::create("aurora", &Value::Null).unwrap());
        let mut b = Renderer::new(crate::live::create("aurora", &Value::Null).unwrap());
        let _ = ctx(0.0, 0.0, &[], None, None);
        let f1 = a.render(1.0, &[], None, None, None);
        let f2 = b.render(1.0, &[], None, None, None);
        assert_eq!(f1, f2);
        assert!(f1.iter().any(|c| *c != BLACK));
    }
}
