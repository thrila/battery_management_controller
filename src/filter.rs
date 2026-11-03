//! Exponential moving average: y += alpha * (x - y).

pub struct Ema {
    alpha: f32,
    value: Option<f32>,
}

impl Ema {
    pub const fn new(alpha: f32) -> Self {
        Self { alpha, value: None }
    }

    /// Feeds a sample and returns the filtered value. The first sample
    /// seeds the filter so it does not ramp up from zero.
    pub fn update(&mut self, x: f32) -> f32 {
        let y = match self.value {
            Some(y) => y + self.alpha * (x - y),
            None => x,
        };
        self.value = Some(y);
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeds_then_converges() {
        let mut f = Ema::new(0.5);
        assert_eq!(f.update(10.0), 10.0);
        assert_eq!(f.update(20.0), 15.0);
        for _ in 0..50 {
            f.update(20.0);
        }
        assert!((f.update(20.0) - 20.0).abs() < 1e-3);
    }
}
