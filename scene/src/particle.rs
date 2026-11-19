use std::{sync::Arc, time::Duration};

use rxwpe_gpu::{Blend, GpuImage, Layer};
use rxwpe_parser::{BlendMode, Initializer, Operator, ParticleLayer, SceneCamera, TextureFrame};

const STEP: f64 = 1. / 60.;

#[derive(Clone, Debug, PartialEq)]
struct Particle {
    position: [f32; 3],
    velocity: [f32; 3],
    rotation: [f32; 3],
    color: [f32; 3],
    alpha: f32,
    size: f32,
    lifetime: f32,
    age: f32,
}

pub(super) struct ParticleSystem {
    pub definition: ParticleLayer,
    image: Arc<GpuImage>,
    frames: Vec<TextureFrame>,
    particles: Vec<Particle>,
    timers: Vec<f64>,
    tick: u64,
    rng: u64,
}

impl ParticleSystem {
    pub fn new(definition: ParticleLayer, image: Arc<GpuImage>, frames: Vec<TextureFrame>) -> Self {
        let timers = vec![0.; definition.emitters.len()];
        let rng = definition.id as u64 ^ 0x9e3779b97f4a7c15;
        Self {
            definition,
            image,
            frames,
            particles: Vec::new(),
            timers,
            tick: 0,
            rng,
        }
    }

    fn reset(&mut self) {
        self.tick = 0;
        self.rng = self.definition.id as u64 ^ 0x9e3779b97f4a7c15;
        self.particles.clear();
        self.timers.fill(0.);
    }

    pub fn advance(&mut self, elapsed: Duration) {
        let target =
            ((elapsed.as_secs_f64() + self.definition.start_time as f64) / STEP).floor() as u64;
        if target < self.tick {
            self.reset();
        }
        // After a long pause, rebuild a bounded warm-up instead of simulating
        // hours of invisible frames. Normal playback always uses fixed steps.
        if target.saturating_sub(self.tick) > 7200 {
            self.reset();
            self.tick = target.saturating_sub(7200);
        }
        while self.tick < target {
            self.step();
            self.tick += 1;
        }
    }

    fn step(&mut self) {
        let dt = STEP as f32;
        for p in &mut self.particles {
            p.age += dt;
            for op in &self.definition.operators {
                if let Operator::Movement { gravity, drag } = op {
                    for (i, force) in gravity.iter().enumerate() {
                        p.velocity[i] = (p.velocity[i] + force * dt) * (-drag * dt).exp();
                        p.position[i] += p.velocity[i] * dt;
                    }
                }
            }
        }
        self.particles.retain(|p| p.age < p.lifetime);
        for index in 0..self.definition.emitters.len() {
            let emitter = self.definition.emitters[index].clone();
            self.timers[index] += STEP * f64::from(emitter.rate * self.definition.rate);
            let count = self.timers[index].floor() as usize;
            self.timers[index] -= count as f64;
            let burst = if self.tick == 0 {
                emitter.instantaneous
            } else {
                0
            };
            for _ in 0..(count + burst).min(
                self.definition
                    .max_count
                    .saturating_sub(self.particles.len()),
            ) {
                let mut position = [0.; 3];
                if emitter.sphere {
                    let angle = self.random() * std::f32::consts::TAU;
                    let radius = (emitter.min[0].powi(2)
                        + self.random() * (emitter.max[0].powi(2) - emitter.min[0].powi(2)))
                    .sqrt();
                    position = [
                        radius * angle.cos(),
                        radius * angle.sin(),
                        (2. * self.random() - 1.) * emitter.max[0],
                    ];
                } else {
                    for (i, value) in position.iter_mut().enumerate() {
                        *value = self.between([emitter.min[i], emitter.max[i]])
                            * if self.random() < 0.5 { -1. } else { 1. };
                    }
                }
                for (i, value) in position.iter_mut().enumerate() {
                    *value *= emitter.directions[i];
                    if emitter.sign[i] > 0. {
                        *value = value.abs();
                    } else if emitter.sign[i] < 0. {
                        *value = -value.abs();
                    }
                }
                let length = position.iter().map(|v| v * v).sum::<f32>().sqrt();
                let speed = self.between(emitter.speed);
                let velocity = if length > 0. {
                    position.map(|v| v / length * speed)
                } else {
                    [0., speed, 0.]
                };
                for (position, origin) in position.iter_mut().zip(emitter.origin) {
                    *position += origin;
                }
                let mut particle = Particle {
                    position,
                    velocity,
                    rotation: [0.; 3],
                    color: self.definition.color,
                    alpha: self.definition.alpha,
                    size: 20. * self.definition.size,
                    lifetime: self.definition.lifetime,
                    age: 0.,
                };
                for init in self.definition.initializers.clone() {
                    match init {
                        Initializer::Lifetime(range) => {
                            particle.lifetime = self.between(range) * self.definition.lifetime
                        }
                        Initializer::Size(range, exponent) => {
                            particle.size = (range[0]
                                + self.random().powf(exponent) * (range[1] - range[0]))
                                * self.definition.size
                        }
                        Initializer::Alpha(range) => {
                            particle.alpha = self.between(range) * self.definition.alpha
                        }
                        Initializer::Color(range) => {
                            for (i, value) in particle.color.iter_mut().enumerate() {
                                *value = self.between([range[0][i], range[1][i]])
                                    * self.definition.color[i];
                            }
                        }
                        Initializer::Rotation(range) => {
                            for (i, value) in particle.rotation.iter_mut().enumerate() {
                                *value = self.between([range[0][i], range[1][i]]);
                            }
                        }
                        Initializer::Velocity(range) => {
                            for (i, value) in particle.velocity.iter_mut().enumerate() {
                                *value = self.between([range[0][i], range[1][i]]);
                            }
                        }
                    }
                }
                if particle.lifetime > 0. {
                    self.particles.push(particle);
                }
            }
        }
    }

    fn random(&mut self) -> f32 {
        self.rng = self.rng.wrapping_add(0x9e3779b97f4a7c15);
        let mut v = self.rng;
        v = (v ^ (v >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        v = (v ^ (v >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((v ^ (v >> 31)) >> 40) as f32 / 16777216.
    }
    fn between(&mut self, range: [f32; 2]) -> f32 {
        range[0] + self.random() * (range[1] - range[0])
    }

    pub fn layers(&self, camera: &SceneCamera) -> Vec<Layer> {
        self.particles
            .iter()
            .map(|p| {
                let alpha = self
                    .definition
                    .operators
                    .iter()
                    .fold(p.alpha, |alpha, op| match op {
                        Operator::AlphaFade { fade_in, fade_out } => {
                            alpha * fade(p.age / p.lifetime, *fade_in, *fade_out)
                        }
                        _ => alpha,
                    });
                let center = p.position;
                let corners = [[-0.5, -0.5], [0.5, -0.5], [0.5, 0.5], [-0.5, 0.5]].map(|point| {
                    let point = rotate([point[0] * p.size, point[1] * p.size, 0.], p.rotation);
                    let point =
                        std::array::from_fn(|i| (point[i] + center[i]) * self.definition.scale[i]);
                    let point = rotate(point, self.definition.angles.map(f32::to_radians));
                    camera.project(std::array::from_fn(|i| {
                        point[i] + self.definition.origin[i]
                    }))
                });
                Layer {
                    image: self.image.clone(),
                    center: [0.; 2],
                    size: [p.size; 2],
                    rotation: 0.,
                    corners: Some(corners),
                    color: [p.color[0], p.color[1], p.color[2], alpha.clamp(0., 1.)],
                    blend: match self.definition.blending {
                        BlendMode::Additive => Blend::Additive,
                        BlendMode::Opaque | BlendMode::Normal => Blend::Replace,
                        BlendMode::Translucent => Blend::Alpha,
                    },
                    uv: self.uv(p),
                }
            })
            .collect()
    }

    fn uv(&self, p: &Particle) -> [f32; 4] {
        let duration: f32 = self.frames.iter().map(|f| f.duration).sum();
        if duration <= 0. {
            return [0., 0., 1., 1.];
        }
        let time = if self.definition.animation_mode == "lifetime" {
            p.age / p.lifetime * duration
        } else {
            p.age * self.definition.sequence_multiplier
        };
        let mut time = time.rem_euclid(duration);
        for frame in &self.frames {
            if time < frame.duration {
                return frame.uv;
            }
            time -= frame.duration;
        }
        self.frames.last().unwrap().uv
    }
}

fn fade(life: f32, fade_in: f32, fade_out: f32) -> f32 {
    if life <= fade_in && fade_in > 0. {
        (life / fade_in).clamp(0., 1.)
    } else if life > fade_out && fade_out < 1. {
        ((1. - life) / (1. - fade_out)).clamp(0., 1.)
    } else {
        1.
    }
}

pub(super) fn rotate(mut point: [f32; 3], angles: [f32; 3]) -> [f32; 3] {
    for (axis, angle) in angles.iter().enumerate() {
        let (s, c) = angle.sin_cos();
        let a = (axis + 1) % 3;
        let b = (axis + 2) % 3;
        let v = point[a] * c - point[b] * s;
        point[b] = point[a] * s + point[b] * c;
        point[a] = v;
    }
    point
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn particle_alpha_fades_over_normalized_lifetime() {
        for (life, expected) in [(0., 0.), (0.25, 0.5), (0.5, 1.), (0.75, 0.5), (1., 0.)] {
            assert!((fade(life, 0.5, 0.5) - expected).abs() < 1e-6);
        }
    }
}
