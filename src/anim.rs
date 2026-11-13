//! Animation maths: easing curves, interpolation and a smoother for progress values. Integer only, pure and
//! host-testable. Fractions are permille (0..=1000).

/// Cubic ease-out: fast start, gentle stop. Input above 1000 is clamped.
pub fn ease_out_cubic(t: u16) -> u16 {
    let t = u32::from(t.min(1000));
    let inv = 1000 - t;
    (1000 - inv * inv * inv / 1_000_000) as u16
}

/// Smooth start and stop (smoothstep). Input above 1000 is clamped.
pub fn ease_in_out(t: u16) -> u16 {
    let t = u32::from(t.min(1000));
    // 3t^2 - 2t^3 on a 0..1000 scale.
    (t * t * (3000 - 2 * t) / 1_000_000) as u16
}

/// `a` to `b` at `t` permille (clamped). Works in both directions and cannot overflow.
pub fn lerp(a: u32, b: u32, t: u16) -> u32 {
    let t = u64::from(t.min(1000));
    let (a64, b64) = (u64::from(a), u64::from(b));
    if b64 >= a64 {
        (a64 + (b64 - a64) * t / 1000) as u32
    } else {
        (a64 - (a64 - b64) * t / 1000) as u32
    }
}

/// Moves a shown value toward a target at a limited speed, so a progress bar glides instead of jumping.
/// It never overshoots the target and never moves backwards.
#[derive(Clone, Copy, Debug, Default)]
pub struct Smoother {
    shown: u32,
    target: u32,
}

/// Fastest the bar may move: the whole bar in 600 ms.
const FULL_SWEEP_US: u64 = 600_000;

impl Smoother {
    pub const fn new() -> Self {
        Self { shown: 0, target: 0 }
    }

    /// Sets the goal (0..=1000). A lower goal than the current one is ignored.
    pub fn target(&mut self, permille: u32) {
        self.target = self.target.max(permille.min(1000));
    }

    /// Advances by `dt_us` microseconds and returns the value to show. The speed grows with the distance
    /// left, so the last bit still arrives in bounded time.
    pub fn step(&mut self, dt_us: u64) -> u32 {
        if dt_us == 0 || self.shown >= self.target {
            return self.shown;
        }
        let left = u64::from(self.target - self.shown);
        // A whole sweep (1000 permille) takes FULL_SWEEP_US; closer to the goal it moves at least 1 permille per 1 ms.
        let by = (1000 * dt_us / FULL_SWEEP_US).max(dt_us / 1000).max(1).min(left);
        self.shown += by as u32;
        self.shown
    }

    pub fn shown(&self) -> u32 {
        self.shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_endpoints_and_monotonic() {
        for f in [ease_out_cubic, ease_in_out] {
            assert_eq!(f(0), 0);
            assert_eq!(f(1000), 1000);
            assert_eq!(f(5000), 1000);
            assert_eq!(f(u16::MAX), 1000);
            let mut last = 0;
            for t in 0..=1000 {
                let v = f(t);
                assert!(v >= last && v <= 1000, "t={t}");
                last = v;
            }
        }
        // Ease-out is ahead of linear in the first half, ease-in-out is behind it.
        assert!(ease_out_cubic(300) > 300);
        assert!(ease_in_out(300) < 300);
    }

    #[test]
    fn lerp_endpoints_and_no_overflow() {
        assert_eq!(lerp(10, 20, 0), 10);
        assert_eq!(lerp(10, 20, 1000), 20);
        assert_eq!(lerp(10, 20, 500), 15);
        assert_eq!(lerp(20, 10, 500), 15);
        assert_eq!(lerp(0, u32::MAX, 1000), u32::MAX);
        assert_eq!(lerp(u32::MAX, 0, 1000), 0);
        assert_eq!(lerp(0, u32::MAX, u16::MAX), u32::MAX);
        assert_eq!(lerp(7, 7, 400), 7);
    }

    #[test]
    fn smoother_never_overshoots_or_reverses() {
        let mut s = Smoother::new();
        s.target(400);
        let mut last = 0;
        for _ in 0..1000 {
            let v = s.step(7_000);
            assert!(v >= last && v <= 400);
            last = v;
        }
        // A lower goal is ignored, a higher goal continues from here.
        s.target(100);
        assert_eq!(s.step(1_000_000), 400);
        s.target(1000);
        assert!(s.step(1_000) > 400);
        s.target(u32::MAX);
        for _ in 0..1000 {
            assert!(s.step(50_000) <= 1000);
        }
    }

    #[test]
    fn smoother_reaches_target_in_bounded_time() {
        let mut s = Smoother::new();
        s.target(1000);
        let mut elapsed = 0u64;
        while s.shown() < 1000 {
            s.step(1_000);
            elapsed += 1_000;
            assert!(elapsed <= 1_000_000, "stuck at {}", s.shown());
        }
        // Zero time passing never moves it, and an idle smoother stays put.
        let mut z = Smoother::new();
        z.target(500);
        assert_eq!(z.step(0), 0);
    }
}
