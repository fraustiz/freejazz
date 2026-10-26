//! System audio spectrum: WASAPI loopback of the default output through `cpal` (an output
//! device opened as an input is captured in loopback), a 2048-sample Hann-windowed FFT and
//! 21 log-spaced bands. Started only while an audio effect runs.

use std::collections::VecDeque;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use super::Spectrum;

pub const FFT_SIZE: usize = 2048;
pub const BANDS: usize = 21;
const LOW_HZ: f32 = 40.0;
const HIGH_HZ: f32 = 16_000.0;
/// Level of a full-scale sine, and of the bottom of the scale, in dB.
const FLOOR_DB: f32 = -60.0;

/// The forward FFT of [`FFT_SIZE`] samples (planning it costs more than running it).
pub fn plan() -> Arc<dyn Fft<f32>> {
    FftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE)
}

/// Spectrum of the last [`FFT_SIZE`] mono samples (fewer are zero-padded at the front).
pub fn bands_from_samples(samples: &[f32], sample_rate: u32) -> Spectrum {
    bands_with(&*plan(), samples, sample_rate)
}

/// [`bands_from_samples`] with an FFT planned once ([`plan`]).
pub fn bands_with(fft: &dyn Fft<f32>, samples: &[f32], sample_rate: u32) -> Spectrum {
    let mut buf: Vec<Complex<f32>> = vec![Complex::new(0.0, 0.0); FFT_SIZE];
    let take = samples.len().min(FFT_SIZE);
    let tail = &samples[samples.len() - take..];
    for (i, &s) in tail.iter().enumerate() {
        let n = FFT_SIZE - take + i;
        let hann =
            0.5 - 0.5 * (2.0 * std::f32::consts::PI * n as f32 / (FFT_SIZE as f32 - 1.0)).cos();
        buf[n] = Complex::new(s * hann, 0.0);
    }
    fft.process(&mut buf);
    let bin_hz = sample_rate as f32 / FFT_SIZE as f32;
    // A sine of amplitude A gives |X| = A * N / 4 with a Hann window.
    let norm = FFT_SIZE as f32 / 4.0;
    let level = |bin: usize| {
        let amp = buf[bin].norm() / norm;
        ((20.0 * amp.max(1e-9).log10() - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
    };
    let ratio = (HIGH_HZ / LOW_HZ).powf(1.0 / BANDS as f32);
    let bands = (0..BANDS)
        .map(|b| {
            let lo = LOW_HZ * ratio.powi(b as i32);
            let hi = lo * ratio;
            let nyquist = FFT_SIZE / 2 - 1;
            let first = ((lo / bin_hz).ceil() as usize).clamp(1, nyquist);
            let last = ((hi / bin_hz).floor() as usize).clamp(first, nyquist);
            (first..=last).map(level).fold(0.0f32, f32::max)
        })
        .collect();
    Spectrum { bands }
}

struct Shared {
    samples: VecDeque<f32>,
    sample_rate: u32,
}

/// A running capture. Dropping it stops the stream.
pub struct AudioCapture {
    shared: Arc<Mutex<Shared>>,
    /// Planned once for the whole capture, not at every frame.
    fft: Arc<dyn Fft<f32>>,
    stop: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl AudioCapture {
    /// Starts capturing the default output; `Err` (French) when there is no device or no
    /// float format.
    pub fn start() -> Result<Self, String> {
        let shared = Arc::new(Mutex::new(Shared {
            samples: VecDeque::with_capacity(FFT_SIZE * 2),
            sample_rate: 48_000,
        }));
        let (stop_tx, stop_rx) = channel::<()>();
        let (ready_tx, ready_rx) = channel::<Result<(), String>>();
        let sh = Arc::clone(&shared);
        // The stream lives (and is dropped) on this thread: it is not `Send` everywhere.
        let thread = std::thread::Builder::new()
            .name("freejazz-audio".into())
            .spawn(move || match open(sh) {
                Ok(stream) => {
                    let _ = ready_tx.send(Ok(()));
                    let _ = stop_rx.recv();
                    drop(stream);
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            })
            .map_err(|e| e.to_string())?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                shared,
                fft: plan(),
                stop: stop_tx,
                thread: Some(thread),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("La capture audio ne démarre pas".into()),
        }
    }

    /// The spectrum of the latest samples.
    pub fn spectrum(&self) -> Spectrum {
        let (samples, rate) = {
            let s = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            (
                s.samples.iter().copied().collect::<Vec<f32>>(),
                s.sample_rate,
            )
        };
        bands_with(&*self.fft, &samples, rate)
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn open(shared: Arc<Mutex<Shared>>) -> Result<cpal::Stream, String> {
    use cpal::SampleFormat;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    const NO_AUDIO: &str = "Aucune sortie audio à écouter";
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or(NO_AUDIO)?;
    let supported = device.default_output_config().map_err(|_| NO_AUDIO)?;
    if supported.sample_format() != SampleFormat::F32 {
        return Err("Format audio non géré (flottant attendu)".into());
    }
    let config = supported.config();
    let channels = usize::from(config.channels.max(1));
    shared.lock().unwrap_or_else(|e| e.into_inner()).sample_rate = config.sample_rate;
    let stream = device
        .build_input_stream::<f32, _, _>(
            config,
            move |data: &[f32], _| {
                let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
                for frame in data.chunks(channels) {
                    let mono = frame.iter().sum::<f32>() / frame.len() as f32;
                    if s.samples.len() == FFT_SIZE {
                        s.samples.pop_front();
                    }
                    s.samples.push_back(mono);
                }
            },
            |_| {},
            None,
        )
        .map_err(|_| "Impossible d'écouter la sortie audio")?;
    stream.play().map_err(|_| "Impossible de lancer l'écoute")?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, rate: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| 0.8 * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin())
            .collect()
    }

    fn band_of(hz: f32) -> usize {
        let ratio = (HIGH_HZ / LOW_HZ).powf(1.0 / BANDS as f32);
        ((hz / LOW_HZ).ln() / ratio.ln()).floor() as usize
    }

    #[test]
    fn bands_from_sine() {
        for rate in [44_100, 48_000] {
            let s = bands_from_samples(&sine(1000.0, rate, FFT_SIZE), rate);
            assert_eq!(s.bands.len(), BANDS);
            let max = s
                .bands
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .unwrap()
                .0;
            assert_eq!(max, band_of(1000.0), "{:?}", s.bands);
            assert!(s.bands[max] > 0.8);
            // far bands stay quiet
            assert!(
                s.bands[0] < 0.3 && s.bands[BANDS - 1] < 0.3,
                "{:?}",
                s.bands
            );
        }
    }

    #[test]
    fn a_plan_made_once_gives_the_same_bands() {
        let fft = plan();
        let samples = sine(440.0, 48_000, FFT_SIZE);
        let once = bands_from_samples(&samples, 48_000);
        for _ in 0..3 {
            assert_eq!(bands_with(&*fft, &samples, 48_000), once);
        }
    }

    #[test]
    fn silence_is_flat_zero() {
        let s = bands_from_samples(&[], 48_000);
        assert!(s.bands.iter().all(|&b| b == 0.0));
        assert_eq!(s.level(), 0.0);
    }
}
