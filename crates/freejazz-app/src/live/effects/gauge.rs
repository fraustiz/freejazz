//! `gauge`: CPU or RAM load shown on F1 to F12 (filling from F1), or as the hue of the
//! whole keyboard (green when idle, red under load).

use serde_json::Value;

use crate::live::{Effect, Frame, FrameCtx, Needs, hsv, lerp, param_str, rgb, scale};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Bar,
    Hue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Metric {
    Cpu,
    Ram,
}

pub struct Gauge {
    mode: Mode,
    metric: Metric,
}

impl Default for Gauge {
    fn default() -> Self {
        Self {
            mode: Mode::Bar,
            metric: Metric::Cpu,
        }
    }
}

impl Effect for Gauge {
    fn id(&self) -> &'static str {
        "gauge"
    }

    /// `mode`: `"bar"` (default) or `"hue"`; `metric`: `"cpu"` (default) or `"ram"`.
    fn configure(&mut self, params: &Value) {
        self.mode = if param_str(params, "mode") == Some("hue") {
            Mode::Hue
        } else {
            Mode::Bar
        };
        self.metric = if param_str(params, "metric") == Some("ram") {
            Metric::Ram
        } else {
            Metric::Cpu
        };
    }

    fn needs(&self) -> Needs {
        Needs {
            audio: false,
            system: true,
        }
    }

    fn frame(&mut self, ctx: &FrameCtx, out: &mut Frame) {
        let Some(sys) = ctx.system else { return };
        let pct = match self.metric {
            Metric::Cpu => sys.cpu,
            Metric::Ram => sys.ram,
        }
        .clamp(0.0, 100.0);
        match self.mode {
            Mode::Hue => {
                let c = hsv(0.33 * (1.0 - pct / 100.0), 1.0, 1.0);
                for k in &ctx.layout.keys {
                    out[usize::from(k.index)] = c;
                }
            }
            Mode::Bar => {
                let filled = pct / 100.0 * 12.0;
                for n in 0..12usize {
                    let label = format!("F{}", n + 1);
                    let Some(k) = ctx.layout.keys.iter().find(|k| k.label == label) else {
                        continue;
                    };
                    let share = (filled - n as f32).clamp(0.0, 1.0);
                    if share > 0.0 {
                        let c = lerp(rgb(0, 220, 70), rgb(255, 30, 20), n as f32 / 11.0);
                        out[usize::from(k.index)] = scale(c, share);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::testing::*;
    use crate::live::{BLACK, SystemStats};

    fn run(mode: &str, cpu: f32) -> Frame {
        let mut e = Gauge::default();
        e.configure(&serde_json::json!({ "mode": mode }));
        let s = SystemStats { cpu, ram: 0.0 };
        let mut out = black();
        e.frame(&ctx(0.0, 0.04, &[], None, Some(&s)), &mut out);
        out
    }

    #[test]
    fn gauge_fills_proportionally() {
        let out = run("bar", 50.0);
        for n in 1..=6 {
            assert_ne!(out[index_of(&format!("F{n}"))], BLACK, "F{n}");
        }
        for n in 7..=12 {
            assert_eq!(out[index_of(&format!("F{n}"))], BLACK, "F{n}");
        }
        assert_eq!(out[index_of("Q")], BLACK);
        let none = run("bar", 0.0);
        assert!(none.iter().all(|c| *c == BLACK));
        let all = run("bar", 100.0);
        assert_ne!(all[index_of("F12")], BLACK);
        // 25 %: three keys, and F4 not yet
        assert_eq!(run("bar", 25.0)[index_of("F4")], BLACK);
        assert_ne!(run("bar", 25.0)[index_of("F3")], BLACK);
    }

    #[test]
    fn hue_mode_goes_from_green_to_red() {
        let idle = run("hue", 0.0);
        let busy = run("hue", 100.0);
        assert!(idle[index_of("Q")].g > 200 && idle[index_of("Q")].r < 20);
        assert!(busy[index_of("Q")].r > 200 && busy[index_of("Q")].g < 20);
    }

    #[test]
    fn needs_the_system_sampler_only() {
        assert!(Gauge::default().needs().system && !Gauge::default().needs().audio);
    }
}
