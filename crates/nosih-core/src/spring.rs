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
}
