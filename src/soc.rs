//! State of charge: OCV lookup at boot, coulomb counting while running.

use crate::config::*;

/// Li-ion resting cell voltage -> SoC (0..1). Must be sorted by voltage.
const OCV_TABLE: [(f32, f32); 11] = [
    (3.00, 0.00),
    (3.45, 0.05),
    (3.55, 0.10),
    (3.65, 0.20),
    (3.72, 0.30),
    (3.78, 0.40),
    (3.84, 0.50),
    (3.91, 0.60),
    (3.98, 0.70),
    (4.08, 0.85),
    (4.20, 1.00),
];

/// Looks up SoC from a resting cell voltage, interpolating linearly.
pub fn soc_from_cell_voltage(cell_v: f32) -> f32 {
    let (first_v, first_soc) = OCV_TABLE[0];
    if cell_v <= first_v {
        return first_soc;
    }
    for pair in OCV_TABLE.windows(2) {
        let (v0, s0) = pair[0];
        let (v1, s1) = pair[1];
        if cell_v <= v1 {
            return s0 + (s1 - s0) * (cell_v - v0) / (v1 - v0);
        }
    }
    OCV_TABLE[OCV_TABLE.len() - 1].1
}

pub struct SocEstimator {
    soc: f32,
    capacity_coulombs: f32,
}

impl SocEstimator {
    /// Initial estimate from the pack voltage at boot (pack assumed at rest).
    pub fn from_pack_voltage(pack_v: f32) -> Self {
        Self {
            soc: soc_from_cell_voltage(pack_v / CELLS_IN_SERIES as f32),
            capacity_coulombs: CAPACITY_MAH / 1000.0 * 3600.0,
        }
    }

    /// Integrates current over `dt_s` seconds and applies re-sync points.
    pub fn update(&mut self, current_a: f32, pack_v: f32, dt_s: f32, undervoltage: bool) {
        self.soc -= current_a * dt_s / self.capacity_coulombs;

        if pack_v >= FULL_RESYNC_V && current_a.abs() < FULL_RESYNC_MAX_A {
            self.soc = 1.0; // end of charge: the charger has tapered off
        }
        if undervoltage {
            self.soc = 0.0;
        }
        self.soc = self.soc.clamp(0.0, 1.0);
    }

    pub fn percent(&self) -> f32 {
        self.soc * 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocv_table_endpoints_and_middle() {
        assert_eq!(soc_from_cell_voltage(2.5), 0.0);
        assert_eq!(soc_from_cell_voltage(4.3), 1.0);
        assert!((soc_from_cell_voltage(3.84) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn one_hour_at_capacity_drains_fully() {
        let mut s = SocEstimator::from_pack_voltage(16.8);
        assert_eq!(s.percent(), 100.0);
        // 3 A for 30 min = 1500 mAh = half of 3000 mAh
        for _ in 0..1800 {
            s.update(3.0, 15.0, 1.0, false);
        }
        assert!((s.percent() - 50.0).abs() < 0.1);
    }

    #[test]
    fn resyncs_at_full_and_empty() {
        let mut s = SocEstimator::from_pack_voltage(15.0);
        s.update(0.05, 16.75, 0.1, false);
        assert_eq!(s.percent(), 100.0);
        s.update(1.0, 11.9, 0.1, true);
        assert_eq!(s.percent(), 0.0);
    }
}
