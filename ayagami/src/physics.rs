use std::f32::consts::PI;

use glam::Vec2;

use crate::{
    meta,
    pose::{self, Value},
};

use log::{debug, warn};

const REF_FPS: f32 = 30.;
const DEFAULT_COMPAT_FPS: f32 = 60.;

const ACC_FAC: f32 = REF_FPS * REF_FPS;

const MAX_SIM_TIME: f32 = 5.;
const SETTLE_SIM_TIME: f32 = 60.;

trait Vec2Ext {
    fn flip(&self) -> Self;
}

impl Vec2Ext for Vec2 {
    fn flip(&self) -> Self {
        Vec2::new(self.y, self.x)
    }
}

pub enum XInputFunction {
    /// Normal rotation. X motion is rotated by input angle, and is therefore always perpendicular to
    /// the resting pendulum position.
    Normal,
    /// Broken rotation function, implemented for compatibility purposes.
    ///
    /// This computes the rotation vector as [cos(angle), 0.5 * sin(2 * angle)]
    BrokenRotation,
    /// Normal rotation, with the input X value scaled by abs(cos(angle)). This simulates the behavior
    /// of the BrokenRotation option where if the angle is +/-90 degrees the X input is is ignored,
    /// while being better behaved.
    ///
    /// The rigging technique known as "angle 90 locking" depends on this behavior.
    CosineScaled,
    /// Like CosineScaled, but using a more complex scaling factor:
    ///
    /// 1 - (abs(cos(x)) - 1)^2
    ///
    /// This has a gentler effect at small input angles, and causes the influence of the input
    /// to drop towards 0 more quickly as the angle approaches +/-90 degrees.
    ///
    /// The behavior of BrokenRotation is roughly between that of CosineScaled and CosineSquareScaled.
    CosineSquareScaled,
}

pub struct PhysicsOptions {
    /// Minimum frame rate for simulation. If the physics system is called less
    /// frequently, it will internally simulate multiple frames, to ensure stability.
    pub min_fps: f32,
    /// FPS value to use when scaling physics calculations.
    /// If None, then use the FPS declared in the physics configuration, or 60 if unset.
    ///
    /// This does not need to match the actual FPS rate of physics calculations.
    pub world_fps: Option<f32>,
    /// Apply the force of gravity as if the pendulum position were "in the future".
    /// This causes pendulums to lose energy and fall towards their neutral position,
    /// even without damping.
    pub gravity_lookahead: bool,
    /// Apply angular momentum as if it were linear momentum. This causes momentum to
    /// be lost, with a greater effect at higher angular velocities.
    pub angular_momentum_loss: bool,
    /// Factor to apply incoming angle changes as momentum. This causes pendulums to
    /// start moving faster when the input angle changes, but also means extra energy
    /// materializes out of nowhere in addition to the potential energy change imparted
    /// by the rotation, causing them to swing further on the opposite side in the
    /// absence of damping.
    ///
    /// Since this does not scale with pendulum parameters, the result can be wildly
    /// unexpected. For example, a pendulum with length 10, delay 0.5, acceleration 0.5,
    /// and mobility 1.0 will gain enough energy from a 45 degree input angle change to
    /// swing back over 180 degrees on the other side and loop around.
    ///
    /// Note: The original implementation uses a buggy rotation function to apply this
    /// boost, causing angle-dependent results. This behavior is not emulated.
    ///
    /// The compatibility value is 0.2.
    pub rotation_boost: f32,
    /// Function to map the X input value to pendulum position.
    pub x_input_function: XInputFunction,
}

impl PhysicsOptions {
    pub fn compatible(fps: Option<f32>) -> Self {
        Self {
            min_fps: 50.,
            world_fps: fps,
            gravity_lookahead: true,
            angular_momentum_loss: true,
            rotation_boost: 0.2,
            x_input_function: XInputFunction::BrokenRotation,
        }
    }
    pub fn useful(fps: Option<f32>) -> Self {
        Self {
            min_fps: 50.,
            world_fps: fps,
            gravity_lookahead: false,
            angular_momentum_loss: false,
            rotation_boost: 0.2, // Maybe come up with a better default here?
            x_input_function: XInputFunction::CosineScaled,
        }
    }
    pub fn accurate(fps: Option<f32>) -> Self {
        Self {
            min_fps: 50.,
            world_fps: fps,
            gravity_lookahead: false,
            angular_momentum_loss: false,
            rotation_boost: 0.,
            x_input_function: XInputFunction::Normal,
        }
    }
}

#[derive(Debug)]
pub struct Pendulum {
    pivot: Vec2,
    g_angle: f32,
    angle: f32,
    velocity: f32,
    bob: Vec2,
    cfg: meta::PhysicsVertex,
}

fn norm_angle(mut a: f32) -> f32 {
    // XXX Is there a better way to do this?
    if !a.is_finite() {
        return a;
    }
    assert!(a.abs() < 100000.);
    while a > PI {
        a -= 2. * PI;
    }
    while a < -PI {
        a += 2. * PI;
    }
    a
}

impl Pendulum {
    fn new(pivot: Vec2, cfg: meta::PhysicsVertex) -> Self {
        Self {
            pivot,
            g_angle: 0.,
            angle: 0.,
            velocity: 0.,
            bob: pivot + Vec2::new(0., cfg.radius),
            cfg,
        }
    }

    fn deriv(&self, cur: Vec2, off: f32, opts: &PhysicsOptions) -> Vec2 {
        let Vec2 { x: a, y: v } = cur;

        let da = v;
        let mut a_adj = a - off;
        let world_fps = opts.world_fps.unwrap() / self.cfg.delay;
        if opts.gravity_lookahead {
            a_adj += da / world_fps;
        }
        let mut dv = -ACC_FAC * self.cfg.acceleration / self.cfg.radius * a_adj.sin();

        if opts.angular_momentum_loss {
            let v2 = (v / world_fps).atan() * world_fps;
            dv += (v2 - v) * world_fps * 3.;
        }

        dv -= v * world_fps * (1. - self.cfg.mobility);

        Vec2::new(da, dv)
    }

    fn simulate(&mut self, dt: f32, pivot: Vec2, g_angle: f32, opts: &PhysicsOptions) -> bool {
        if dt.is_infinite() {
            // For infinite dt, settle the system
            let changed = self.pivot != pivot || self.g_angle != g_angle;
            self.pivot = pivot;
            self.g_angle = g_angle;
            self.angle = g_angle;
            self.velocity = 0.;
            self.bob = Vec2::from_angle(self.angle).flip() * self.cfg.radius + self.pivot;
            return changed;
        }

        let dt = self.cfg.delay * dt;

        if pivot != self.pivot {
            // Pendulum hangs down at +Y, standard coords are zero degrees at +X, so flip delta effect
            let delta = (self.pivot - pivot).flip();
            let before = Vec2::from_angle(self.angle) * self.cfg.radius;
            self.angle = (before + delta).to_angle();
            self.pivot = pivot;
        }
        if g_angle != self.g_angle {
            let world_fps = opts.world_fps.unwrap() / self.cfg.delay;
            self.velocity += ((g_angle - self.g_angle) * opts.rotation_boost) * world_fps;
            self.g_angle = g_angle;
        }

        let cur = Vec2::new(self.angle, self.velocity);
        let k1 = dt * self.deriv(cur, g_angle, opts);
        let k2 = dt * self.deriv(cur + 0.5 * k1, g_angle, opts);
        let k3 = dt * self.deriv(cur + 0.5 * k2, g_angle, opts);
        let k4 = dt * self.deriv(cur + k3, g_angle, opts);
        let next = cur + (k1 + 2. * k2 + 2. * k3 + k4) / 6.;

        self.angle = norm_angle(next.x);
        self.velocity = next.y;
        self.bob = Vec2::from_angle(self.angle).flip() * self.cfg.radius + self.pivot;
        true
    }
}

pub struct System {
    gravity_angle: f32,
    pendulums: Vec<Pendulum>,
    setting: meta::PhysicsSetting,
}

impl System {
    fn simulate(&mut self, dt: f32, input_x: f32, input_angle: f32, opts: &PhysicsOptions) -> bool {
        let mut pivot = input_x
            * match opts.x_input_function {
                XInputFunction::Normal => Vec2::from_angle(input_angle),
                XInputFunction::BrokenRotation => {
                    Vec2::new(input_angle.cos(), 0.5 * (input_angle * 2.).sin())
                }
                XInputFunction::CosineScaled => {
                    Vec2::from_angle(input_angle) * input_angle.cos().abs()
                }
                XInputFunction::CosineSquareScaled => {
                    let k = input_angle.cos().abs() - 1.;
                    Vec2::from_angle(input_angle) * (1. - k * k)
                }
            };

        let mut changed = false;
        for pendulum in self.pendulums.iter_mut() {
            if pendulum.simulate(dt, pivot, input_angle, opts) {
                changed = true;
            }
            pivot = pendulum.bob;
        }
        changed
    }

    fn get_inputs(&mut self, pose: &pose::Pose) -> (f32, f32) {
        let mut angle = 0.;
        let mut x = 0.;
        for input in self.setting.input.iter() {
            assert!(input.source.target == meta::TargetType::Parameter);
            let key = pose::Key::param(&input.source.id);
            let Some((_, desc)) = pose.map().get(&key) else {
                continue;
            };
            let value = pose.get_flattened(&key).unwrap_or(0.);
            let mut t = (value - desc.min) / (desc.max - desc.min);
            t = 2. * t - 1.;
            t = t * input.weight / 100.;
            if input.reflect {
                t = -t;
            }
            match input.input_type {
                meta::PhysicsType::X => x += t,
                meta::PhysicsType::Angle => angle += t,
            }
        }

        angle = self.normalize(angle, &self.setting.normalization.angle);
        x = self.normalize(x, &self.setting.normalization.position);

        (angle, x)
    }

    fn apply_outputs(&mut self, pose: &mut pose::Pose) {
        for output in self.setting.output.iter() {
            assert!(output.destination.target == meta::TargetType::Parameter);
            let key = pose::Key::param(&output.destination.id);
            let Some((_, desc)) = pose.map().get(&key) else {
                continue;
            };
            let mut value = self.angle(output.vertex_index as usize - 1) * output.scale;
            if output.reflect {
                value = -value;
            }
            let a = output.weight / 100.;
            value = value.clamp(desc.min, desc.max);
            if let Some(v) = pose.get_mut_flattened(&key) {
                *v = Value::opaque(value * a + v.value * (1. - a));
            }
        }
    }

    fn update(&mut self, pose: &mut pose::Pose, dt: f32, opts: &PhysicsOptions) -> bool {
        let (angle, x) = self.get_inputs(pose);
        let changed = self.simulate(dt, x, angle / 180. * PI, opts);
        self.apply_outputs(pose);
        changed
    }

    fn normalize(&self, v: f32, norm: &meta::PhysicsRange) -> f32 {
        let v = v.clamp(-1., 1.);
        if v > 0. {
            norm.default + v * (norm.maximum - norm.default)
        } else {
            norm.default - v * (norm.minimum - norm.default)
        }
    }

    fn angle(&self, i: usize) -> f32 {
        let base = if i == 0 {
            self.gravity_angle
        } else {
            self.pendulums[i - 1].angle
        };
        norm_angle(self.pendulums[i].angle - base)
    }
}

pub struct PhysicsEngine {
    meta_fps: Option<f32>,
    options: PhysicsOptions,
    systems: Vec<System>,
}

impl PhysicsEngine {
    pub fn new(config: meta::Physics3, mut options: PhysicsOptions) -> Self {
        if options.world_fps.is_none() {
            options.world_fps = Some(config.meta.fps.unwrap_or(DEFAULT_COMPAT_FPS));
        }

        let mut systems = Vec::new();

        for (i, mut setting) in config.physics_settings.into_iter().enumerate() {
            let mut pendulums = Vec::new();
            let mut pivot = Vec2::ZERO;
            for vertex in setting.vertices.iter().skip(1) {
                pendulums.push(Pendulum::new(pivot.clone(), vertex.clone()));
                pivot.y += vertex.radius;
            }

            setting.output.retain(|o| {
                if o.vertex_index == 0 || o.vertex_index as usize > pendulums.len() {
                    warn!("Physics setting #{} ({}) output has invalid vertex index {} (Valid: 1..{}). Ignoring this output.",
                        i, setting.id, o.vertex_index, pendulums.len());
                    false
                } else {
                    true
                }
            });

            let g = &config.meta.effective_forces.gravity;
            systems.push(System {
                pendulums,
                gravity_angle: -g.x.atan2(-g.y),
                setting,
            })
        }

        Self {
            meta_fps: config.meta.fps,
            options,
            systems,
        }
    }

    pub fn update(&mut self, pose: &mut pose::Pose, mut dt: f32) {
        // Simulate a maximum of 5 seconds worth of physics at once. If the frame rate
        // drops below this, physics will run slower than real time. In practice
        // this tends to happen when an app is paused (minimized, computer goes to sleep,
        // etc.).
        if dt < 0. {
            warn!("Physics update with negative dt: {}", dt);
            dt = 0.;
        } else if dt > MAX_SIM_TIME {
            warn!(
                "Physics update with large dt {}, limiting to {}",
                dt, MAX_SIM_TIME
            );
            dt = MAX_SIM_TIME;
        }
        // Split dt into an integer number of steps, such that it remains below
        // 1 / min_fps.
        let mut ticks: usize = 1;
        if self.options.min_fps > 0. {
            let max_dt = 1. / self.options.min_fps;
            ticks = ((max_dt + dt) / max_dt).floor() as usize;
            if ticks > 1 {
                debug!(
                    "Slow physics update (dt={}), splitting into {} updates",
                    dt, ticks
                );
            }
            dt /= ticks as f32;
        }
        for _ in 0..ticks {
            for system in self.systems.iter_mut() {
                system.update(pose, dt, &self.options);
            }
        }
    }

    pub fn settle(&mut self, pose: &pose::Pose) {
        let mut pose = pose.clone();
        // One iteration will settle the system if and only if the physics settings
        // have only forward dependencies. As many iterations as settings will
        // settle the system if there are backwards dependencies but no cycles.
        // If there are dependency cycles, then there is no guarantee the system
        // will settle.
        for _ in 0..(self.systems.len() + 1) {
            let mut changed = false;
            for system in self.systems.iter_mut() {
                if system.update(&mut pose, f32::INFINITY, &self.options) {
                    changed = true;
                }
            }
            if !changed {
                return;
            }
        }
        warn!(
            "Physics system failed to settle, it likely has circular dependencies. Trying to simulate..."
        );
        // Just run the physics for a while
        for _ in 0..((SETTLE_SIM_TIME / MAX_SIM_TIME).ceil() as usize) {
            self.update(&mut pose, MAX_SIM_TIME);
        }
    }

    pub fn set_options(&mut self, mut options: PhysicsOptions) {
        if options.world_fps.is_none() {
            options.world_fps = Some(self.meta_fps.unwrap_or(DEFAULT_COMPAT_FPS));
        }
        self.options = options;
    }
}
