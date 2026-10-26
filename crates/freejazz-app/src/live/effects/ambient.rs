//! `ambient`: moods that move on their own (`fire`, `aurora`, `matrix`).

use serde_json::Value;

use crate::live::{Effect, Frame, FrameCtx, Rng, center, hsv, lerp, param_str, rgb, scale};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Fire,
    Aurora,
    Matrix,
}

impl Variant {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "fire" => Self::Fire,
            "aurora" => Self::Aurora,
            "matrix" => Self::Matrix,
            _ => return None,
        })
    }
}

/// A falling drop of the matrix rain, per column.
#[derive(Clone, Copy)]
struct Drop {
    head: f32,
    speed: f32,
    trail: f32,
}

pub struct Ambient {
    variant: Variant,
    rng: Rng,
    drops: Vec<Drop>,
}

impl Ambient {
    pub fn new(variant: Variant) -> Self {
        Self {
            variant,
            rng: Rng::new(0x5EED_1234),
            drops: Vec::new(),
        }
    }

    fn respawn(rng: &mut Rng, height: f32) -> Drop {
        Drop {
            head: -rng.unit() * height * 1.5,
            speed: 2.5 + rng.unit() * 4.0,
            trail: 2.0 + rng.unit() * 3.0,
        }
    }
}

fn fire_ramp(v: f32) -> crate::live::Rgb {
    // black, deep red, orange, yellow
    if v < 0.5 {
        scale(rgb(255, 20, 0), v * 2.0)
    } else {
        lerp(rgb(255, 20, 0), rgb(255, 190, 40), (v - 0.5) * 2.0)
    }
}

impl Effect for Ambient {
    fn id(&self) -> &'static str {
        match self.variant {
            Variant::Fire => "fire",
            Variant::Aurora => "aurora",
            Variant::Matrix => "matrix",
        }
    }

    fn configure(&mut self, params: &Value) {
        if let Some(v) = param_str(params, "variant").and_then(Variant::from_name) {
            self.variant = v;
        }
    }

    fn frame(&mut self, ctx: &FrameCtx, out: &mut Frame) {
        let (w, h) = ctx.layout.size();
        let t = ctx.t;
        match self.variant {
            Variant::Fire => {
                for k in &ctx.layout.keys {
                    let (x, y) = center(k);
                    // flames rise: hotter at the bottom, licked by moving waves
                    let wave = 0.5 + 0.5 * (x * 1.9 + t * 2.3 + (y * 2.1 - t * 3.7).sin()).sin();
                    let flicker = 0.5 + 0.5 * (x * 4.1 - t * 5.3 + y * 1.7).sin();
                    let base = y / h;
                    let v = (base * 1.15 - 0.2 + 0.35 * wave + 0.15 * flicker).clamp(0.0, 1.0);
                    out[usize::from(k.index)] = fire_ramp(v);
                }
            }
            Variant::Aurora => {
                for k in &ctx.layout.keys {
                    let (x, y) = center(k);
                    let hue = 0.42 + 0.2 * (x * 0.3 + t * 0.35).sin() + 0.05 * (y * 0.8).sin();
                    let glow = 0.5 + 0.5 * (x * 0.5 - t * 0.7 + y * 0.45).sin();
                    out[usize::from(k.index)] = hsv(hue, 0.85, 0.25 + 0.75 * glow);
                }
            }
            Variant::Matrix => {
                let columns = w.ceil() as usize + 1;
                while self.drops.len() < columns {
                    self.drops.push(Self::respawn(&mut self.rng, h));
                }
                for d in &mut self.drops {
                    d.head += d.speed * ctx.dt;
                    if d.head - d.trail > h {
                        *d = Self::respawn(&mut self.rng, h);
                    }
                }
                for k in &ctx.layout.keys {
                    let (x, y) = center(k);
                    let d = self.drops[(x as usize).min(self.drops.len() - 1)];
                    let behind = d.head - y;
                    out[usize::from(k.index)] = if behind < 0.0 || behind > d.trail {
                        rgb(0, 6, 0)
                    } else if behind < 0.5 {
                        rgb(200, 255, 200)
                    } else {
                        rgb(0, (255.0 * (1.0 - behind / d.trail)).max(8.0) as u8, 0)
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::BLACK;
    use crate::live::testing::*;

    #[test]
    fn variants_light_the_board_and_move() {
        for v in [Variant::Fire, Variant::Aurora, Variant::Matrix] {
            let mut e = Ambient::new(v);
            let mut a = black();
            e.frame(&ctx(0.0, 0.04, &[], None, None), &mut a);
            let mut b = black();
            for i in 1..40 {
                b = black();
                e.frame(&ctx(i as f32 * 0.04, 0.04, &[], None, None), &mut b);
            }
            assert!(b.iter().any(|c| *c != BLACK), "{v:?}");
            assert_ne!(a, b, "{v:?} does not move");
            // slots without a key stay black
            assert_eq!(b[127], BLACK);
        }
    }

    #[test]
    fn variant_param_selects() {
        let mut e = Ambient::new(Variant::Fire);
        e.configure(&serde_json::json!({"variant": "matrix"}));
        assert_eq!(e.id(), "matrix");
    }
}
