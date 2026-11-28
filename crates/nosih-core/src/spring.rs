//! Critically damped spring that reaches its target as quickly as possible
//! without overshooting. The exact solution keeps it stable at any frame duration.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    /// Angular frequency in rad/s. The transition completes in about `6 / omega` seconds.
    pub omega: f32,
}

/// The spring is considered settled below this displacement and velocity.
const REST_DISTANCE: f32 = 0.001;
const REST_VELOCITY: f32 = 0.01;

impl Spring {
    #[must_use]
    pub fn at(value: f32, omega: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
            omega,
        }
    }

    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.value == self.target && self.velocity == 0.0
    }

    pub fn step(&mut self, dt: f32) {
        let distance = self.value - self.target;
        let c = self.velocity + self.omega * distance;
        let decay = (-self.omega * dt).exp();
        let next = (distance + c * dt) * decay;
        self.velocity = (c - self.omega * (distance + c * dt)) * decay;
        self.value = self.target + next;

        if next.abs() < REST_DISTANCE && self.velocity.abs() < REST_VELOCITY {
            self.value = self.target;
            self.velocity = 0.0;
        }
    }
}

/// Underdamped harmonic oscillator (`0.05 <= damping <= 0.99`) with an exact
/// analytical closed-form step, giving fluid Liquid Glass momentum and slight
/// elastic overshoot while staying frame-rate independent and unconditionally
/// stable at any `dt`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElasticSpring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    /// Undamped angular frequency in rad/s.
    pub omega: f32,
    /// Damping ratio (`zeta`), clamped to `0.05..=0.99`.
    pub damping: f32,
}

impl ElasticSpring {
    #[must_use]
    pub fn at(value: f32, omega: f32, damping: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
            target: value,
            omega,
            damping: damping.clamp(0.05, 0.99),
        }
    }

    #[must_use]
    pub fn is_settled(&self) -> bool {
        self.value == self.target && self.velocity == 0.0
    }

    pub fn impulse(&mut self, delta_velocity: f32) {
        self.velocity += delta_velocity;
    }

    pub fn step(&mut self, dt: f32) {
        let a = self.value - self.target;
        if dt <= 0.0 || self.omega <= 0.0 {
            if a.abs() < REST_DISTANCE && self.velocity.abs() < REST_VELOCITY {
                self.value = self.target;
                self.velocity = 0.0;
            }
            return;
        }

        let zeta = self.damping.clamp(0.05, 0.99);
        let omega = self.omega;
        let omega_d = omega * (1.0 - zeta * zeta).sqrt();
        let b = (self.velocity + zeta * omega * a) / omega_d;
        let decay = (-zeta * omega * dt).exp();
        let (sin, cos) = (omega_d * dt).sin_cos();

        let x = decay * (a * cos + b * sin);
        self.velocity =
            decay * ((omega_d * b - zeta * omega * a) * cos - (omega_d * a + zeta * omega * b) * sin);
        self.value = self.target + x;

        if x.abs() < REST_DISTANCE && self.velocity.abs() < REST_VELOCITY {
            self.value = self.target;
            self.velocity = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(mut spring: Spring, seconds: f32, dt: f32) -> Spring {
        (0..(seconds / dt) as usize).for_each(|_| spring.step(dt));
        spring
    }

    fn toward_one() -> Spring {
        Spring {
            target: 1.0,
            ..Spring::at(0.0, 30.0)
        }
    }

    #[test]
    fn spring_should_settle_on_target() {
        assert!(run(toward_one(), 1.0, 1.0 / 60.0).is_settled());
    }

    #[test]
    fn spring_should_never_overshoot_target() {
        let mut spring = toward_one();
        let overshoots = (0..240).any(|_| {
            spring.step(1.0 / 240.0);
            spring.value > 1.0
        });
        assert!(!overshoots);
    }

    #[test]
    fn spring_should_cover_most_of_the_distance_in_200_ms() {
        assert!(run(toward_one(), 0.2, 1.0 / 120.0).value > 0.95);
    }

    #[test]
    fn spring_should_give_same_result_regardless_of_frame_rate() {
        let slow = run(toward_one(), 0.1, 1.0 / 50.0).value;
        let fast = run(toward_one(), 0.1, 1.0 / 200.0).value;
        assert!((slow - fast).abs() < 1e-3);
    }

    #[test]
    fn spring_should_stay_stable_with_a_very_long_frame() {
        let mut spring = toward_one();
        spring.step(5.0);
        assert!(spring.is_settled());
    }

    #[test]
    fn spring_should_be_settled_when_created_at_rest() {
        assert!(Spring::at(0.0, 30.0).is_settled());
    }

    fn run_elastic(mut spring: ElasticSpring, seconds: f32, dt: f32) -> ElasticSpring {
        (0..(seconds / dt) as usize).for_each(|_| spring.step(dt));
        spring
    }

    fn elastic_toward_one() -> ElasticSpring {
        ElasticSpring {
            target: 1.0,
            ..ElasticSpring::at(0.0, 18.0, 0.76)
        }
    }

    #[test]
    fn elastic_spring_should_settle_on_target() {
        let settled = run_elastic(elastic_toward_one(), 2.0, 1.0 / 60.0);
        assert!(settled.is_settled());
        assert_eq!(settled.value, 1.0);
    }

    #[test]
    fn elastic_spring_should_overshoot_slightly_when_underdamped() {
        let mut spring = elastic_toward_one();
        let mut peak = 0.0_f32;
        for _ in 0..240 {
            spring.step(1.0 / 240.0);
            peak = peak.max(spring.value);
        }
        assert!(peak > 1.0);
        assert!(peak < 1.15);
    }

    #[test]
    fn elastic_spring_should_be_frame_rate_independent() {
        let slow = run_elastic(elastic_toward_one(), 0.2, 1.0 / 60.0);
        let fast = run_elastic(elastic_toward_one(), 0.2, 1.0 / 240.0);
        assert!((slow.value - fast.value).abs() < 1e-3);
        assert!((slow.velocity - fast.velocity).abs() < 1e-2);
    }

    #[test]
    fn elastic_spring_should_stay_stable_on_large_dt() {
        let mut spring = elastic_toward_one();
        spring.step(5.0);
        assert!(spring.is_settled());
        assert_eq!(spring.value, 1.0);
        assert_eq!(spring.velocity, 0.0);
    }

    #[test]
    fn elastic_spring_impulse_kicks_velocity_and_returns_to_target() {
        let mut spring = ElasticSpring::at(0.0, 18.0, 0.76);
        assert!(spring.is_settled());
        spring.impulse(50.0);
        assert!(!spring.is_settled());
        assert_eq!(spring.velocity, 50.0);

        let mut peak = 0.0_f32;
        for _ in 0..240 {
            spring.step(1.0 / 120.0);
            peak = peak.max(spring.value);
        }
        assert!(peak > 0.5);
        assert!(spring.is_settled());
        assert_eq!(spring.value, 0.0);
    }

    #[test]
    fn elastic_spring_clamps_damping_bounds() {
        assert_eq!(ElasticSpring::at(0.0, 18.0, 0.0).damping, 0.05);
        assert_eq!(ElasticSpring::at(0.0, 18.0, 1.5).damping, 0.99);
    }
}
