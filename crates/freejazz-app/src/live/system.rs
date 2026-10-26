//! CPU and RAM use every 500 ms, sampled on a thread that exists only while a gauge runs.

use std::sync::mpsc::{RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use sysinfo::System;

use super::SystemStats;

pub const INTERVAL: Duration = Duration::from_millis(500);

pub struct SystemMonitor {
    stats: Arc<Mutex<SystemStats>>,
    stop: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl SystemMonitor {
    pub fn start() -> Self {
        let stats = Arc::new(Mutex::new(SystemStats::default()));
        let (stop, stop_rx) = channel::<()>();
        let shared = Arc::clone(&stats);
        let thread = std::thread::Builder::new()
            .name("freejazz-sysinfo".into())
            .spawn(move || {
                let mut sys = System::new();
                // CPU use needs two samples: the first one only primes it.
                sys.refresh_cpu_usage();
                while let Err(RecvTimeoutError::Timeout) = stop_rx.recv_timeout(INTERVAL) {
                    sys.refresh_cpu_usage();
                    sys.refresh_memory();
                    let total = sys.total_memory().max(1);
                    let s = SystemStats {
                        cpu: sys.global_cpu_usage().clamp(0.0, 100.0),
                        ram: (sys.used_memory() as f64 / total as f64 * 100.0) as f32,
                    };
                    *shared.lock().unwrap_or_else(|e| e.into_inner()) = s;
                }
            })
            .expect("spawn sysinfo thread");
        Self {
            stats,
            stop,
            thread: Some(thread),
        }
    }

    pub fn stats(&self) -> SystemStats {
        *self.stats.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl Drop for SystemMonitor {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_reports_plausible_percentages_and_stops() {
        let m = SystemMonitor::start();
        let start = std::time::Instant::now();
        while m.stats().ram == 0.0 && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(100));
        }
        let s = m.stats();
        assert!((0.0..=100.0).contains(&s.cpu), "{s:?}");
        assert!(s.ram > 0.0 && s.ram <= 100.0, "{s:?}");
        drop(m); // joins the thread
    }
}
