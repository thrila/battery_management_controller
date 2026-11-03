//! Protection state machine. Decides from the measurements whether the
//! charge and discharge FETs may be on. Pure logic, no hardware access.

use crate::config::*;

/// Fault bit flags, reported in telemetry as a hex mask.
pub mod fault {
    pub const OVER_VOLTAGE: u16 = 1 << 0;
    pub const UNDER_VOLTAGE: u16 = 1 << 1;
    pub const OVER_CURRENT_DSG: u16 = 1 << 2;
    pub const OVER_CURRENT_CHG: u16 = 1 << 3;
    pub const OVER_TEMP_CELL: u16 = 1 << 4;
    pub const OVER_TEMP_CHARGE: u16 = 1 << 5;
    pub const UNDER_TEMP_CHARGE: u16 = 1 << 6;
    pub const OVER_TEMP_FET: u16 = 1 << 7;
    pub const SENSOR: u16 = 1 << 8;

    /// Faults that must turn the charge FET off.
    pub const BLOCKS_CHARGE: u16 =
        OVER_VOLTAGE | OVER_CURRENT_CHG | OVER_TEMP_CELL | OVER_TEMP_CHARGE | UNDER_TEMP_CHARGE | SENSOR;
    /// Faults that must turn the discharge FET off.
    pub const BLOCKS_DISCHARGE: u16 =
        UNDER_VOLTAGE | OVER_CURRENT_DSG | OVER_TEMP_CELL | OVER_TEMP_FET | SENSOR;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SystemState {
    Init,
    Normal,
    Warning,
    Protect,
    Fault,
}

impl SystemState {
    pub fn as_str(self) -> &'static str {
        match self {
            SystemState::Init => "INIT",
            SystemState::Normal => "NORMAL",
            SystemState::Warning => "WARNING",
            SystemState::Protect => "PROTECT",
            SystemState::Fault => "FAULT",
        }
    }
}

/// One loop's worth of converted, filtered measurements.
/// A temperature of `None` means the thermistor could not be read.
#[derive(Clone, Copy, Debug)]
pub struct Measurements {
    pub pack_v: f32,
    pub current_a: f32,
    pub cell_temp_c: Option<f32>,
    pub fet_temp_c: Option<f32>,
    pub voltage_sensor_ok: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Decision {
    pub charge_on: bool,
    pub discharge_on: bool,
    pub faults: u16,
    pub state: SystemState,
}

/// Trips after `debounce` consecutive samples past the trip threshold and
/// releases once the value is back past the release threshold.
struct Hysteresis {
    debounce: u16,
    count: u16,
    active: bool,
}

impl Hysteresis {
    const fn new(debounce: u16) -> Self {
        Self { debounce, count: 0, active: false }
    }

    fn update(&mut self, beyond_trip: bool, within_release: bool) -> bool {
        if self.active {
            if within_release {
                self.active = false;
            }
        } else if beyond_trip {
            self.count += 1;
            if self.count >= self.debounce {
                self.active = true;
                self.count = 0;
            }
        } else {
            self.count = 0;
        }
        self.active
    }
}

/// Over-current trip: stays latched for a cooldown, then retries. Too many
/// trips without a clean period in between latch it off permanently.
struct LatchedTrip {
    debounce: u16,
    count: u16,
    cooldown_left_ms: u32,
    clean_ms: u32,
    retries: u8,
    permanent: bool,
}

/// How long the current must stay normal before the retry counter resets.
const OC_RETRY_RESET_MS: u32 = 30_000;

impl LatchedTrip {
    const fn new(debounce: u16) -> Self {
        Self { debounce, count: 0, cooldown_left_ms: 0, clean_ms: 0, retries: 0, permanent: false }
    }

    fn update(&mut self, beyond_trip: bool, dt_ms: u32) -> bool {
        if self.permanent {
            return true;
        }
        if self.cooldown_left_ms > 0 {
            self.cooldown_left_ms = self.cooldown_left_ms.saturating_sub(dt_ms);
            return self.cooldown_left_ms > 0;
        }
        if beyond_trip {
            self.clean_ms = 0;
            self.count += 1;
            if self.count >= self.debounce {
                self.count = 0;
                self.retries += 1;
                self.cooldown_left_ms = OC_COOLDOWN_MS;
                if self.retries >= OC_MAX_RETRIES {
                    self.permanent = true;
                }
                return true;
            }
        } else {
            self.count = 0;
            self.clean_ms = self.clean_ms.saturating_add(dt_ms);
            if self.clean_ms >= OC_RETRY_RESET_MS {
                self.retries = 0;
            }
        }
        false
    }
}

/// Sensor fault: once tripped it never clears without a reset.
struct PermanentTrip {
    debounce: u16,
    count: u16,
    active: bool,
}

impl PermanentTrip {
    const fn new(debounce: u16) -> Self {
        Self { debounce, count: 0, active: false }
    }

    fn update(&mut self, bad: bool) -> bool {
        if !self.active {
            self.count = if bad { self.count + 1 } else { 0 };
            self.active = self.count >= self.debounce;
        }
        self.active
    }
}

pub struct Protection {
    ov: Hysteresis,
    uv: Hysteresis,
    ocd: LatchedTrip,
    occ: LatchedTrip,
    ot_cell: Hysteresis,
    ot_charge: Hysteresis,
    ut_charge: Hysteresis,
    ot_fet: Hysteresis,
    sensor: PermanentTrip,
}

impl Protection {
    pub const fn new() -> Self {
        Self {
            ov: Hysteresis::new(DEBOUNCE_OV),
            uv: Hysteresis::new(DEBOUNCE_UV),
            ocd: LatchedTrip::new(DEBOUNCE_OC),
            occ: LatchedTrip::new(DEBOUNCE_OC),
            ot_cell: Hysteresis::new(DEBOUNCE_TEMP),
            ot_charge: Hysteresis::new(DEBOUNCE_TEMP),
            ut_charge: Hysteresis::new(DEBOUNCE_TEMP),
            ot_fet: Hysteresis::new(DEBOUNCE_TEMP),
            sensor: PermanentTrip::new(DEBOUNCE_SENSOR),
        }
    }

    pub fn update(&mut self, m: &Measurements, dt_ms: u32) -> Decision {
        let mut faults = 0u16;
        let mut set = |active: bool, bit: u16| {
            if active {
                faults |= bit;
            }
        };

        let v = m.pack_v;
        let i = m.current_a;

        set(self.ov.update(v > OV_TRIP_V, v < OV_RELEASE_V), fault::OVER_VOLTAGE);
        set(self.uv.update(v < UV_TRIP_V, v > UV_RELEASE_V), fault::UNDER_VOLTAGE);
        set(self.ocd.update(i > OCD_TRIP_A, dt_ms), fault::OVER_CURRENT_DSG);
        set(self.occ.update(i < -OCC_TRIP_A, dt_ms), fault::OVER_CURRENT_CHG);

        let sensors_ok = m.voltage_sensor_ok && m.cell_temp_c.is_some() && m.fet_temp_c.is_some();
        set(self.sensor.update(!sensors_ok), fault::SENSOR);

        // A missing temperature skips its checks; the sensor fault covers it.
        if let Some(t) = m.cell_temp_c {
            set(self.ot_cell.update(t > OT_CELL_TRIP_C, t < OT_CELL_RELEASE_C), fault::OVER_TEMP_CELL);
            set(self.ot_charge.update(t > OT_CHARGE_TRIP_C, t < OT_CHARGE_RELEASE_C), fault::OVER_TEMP_CHARGE);
            set(self.ut_charge.update(t < UT_CHARGE_TRIP_C, t > UT_CHARGE_RELEASE_C), fault::UNDER_TEMP_CHARGE);
        }
        if let Some(t) = m.fet_temp_c {
            set(self.ot_fet.update(t > OT_FET_TRIP_C, t < OT_FET_RELEASE_C), fault::OVER_TEMP_FET);
        }

        let latched_off = self.sensor.active || self.ocd.permanent || self.occ.permanent;
        let state = if latched_off {
            SystemState::Fault
        } else if faults != 0 {
            SystemState::Protect
        } else if near_limit(m) {
            SystemState::Warning
        } else {
            SystemState::Normal
        };

        Decision {
            charge_on: faults & fault::BLOCKS_CHARGE == 0,
            discharge_on: faults & fault::BLOCKS_DISCHARGE == 0,
            faults,
            state,
        }
    }
}

/// True when any reading is within WARNING_MARGIN of its trip limit.
fn near_limit(m: &Measurements) -> bool {
    let hi = 1.0 - WARNING_MARGIN;
    let lo = 1.0 + WARNING_MARGIN;
    let temp_hot = |t: Option<f32>, limit: f32| t.map_or(false, |t| t > limit * hi);
    m.pack_v > OV_TRIP_V * hi
        || m.pack_v < UV_TRIP_V * lo
        || m.current_a > OCD_TRIP_A * hi
        || m.current_a < -OCC_TRIP_A * hi
        || temp_hot(m.cell_temp_c, OT_CELL_TRIP_C)
        || temp_hot(m.fet_temp_c, OT_FET_TRIP_C)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: u32 = LOOP_PERIOD_MS;

    fn healthy() -> Measurements {
        Measurements {
            pack_v: 15.0,
            current_a: 2.0,
            cell_temp_c: Some(25.0),
            fet_temp_c: Some(30.0),
            voltage_sensor_ok: true,
        }
    }

    fn run(p: &mut Protection, m: Measurements, loops: u32) -> Decision {
        let mut d = p.update(&m, DT);
        for _ in 1..loops {
            d = p.update(&m, DT);
        }
        d
    }

    #[test]
    fn healthy_pack_is_normal() {
        let d = run(&mut Protection::new(), healthy(), 5);
        assert_eq!(d.state, SystemState::Normal);
        assert!(d.charge_on && d.discharge_on);
        assert_eq!(d.faults, 0);
    }

    #[test]
    fn single_noisy_sample_does_not_trip() {
        let mut p = Protection::new();
        let d = p.update(&Measurements { pack_v: 17.5, ..healthy() }, DT);
        assert_eq!(d.faults, 0);
        let d = p.update(&healthy(), DT);
        assert_eq!(d.faults, 0);
    }

    #[test]
    fn over_voltage_blocks_charge_only_and_has_hysteresis() {
        let mut p = Protection::new();
        let d = run(&mut p, Measurements { pack_v: 17.2, ..healthy() }, DEBOUNCE_OV as u32);
        assert_eq!(d.faults, fault::OVER_VOLTAGE);
        assert!(!d.charge_on && d.discharge_on);
        assert_eq!(d.state, SystemState::Protect);

        // Between release and trip: stays tripped
        let d = run(&mut p, Measurements { pack_v: 16.8, ..healthy() }, 20);
        assert!(!d.charge_on);
        // Below release: clears
        let d = run(&mut p, Measurements { pack_v: 16.5, ..healthy() }, 1);
        assert!(d.charge_on);
    }

    #[test]
    fn under_voltage_blocks_discharge_only() {
        let d = run(&mut Protection::new(), Measurements { pack_v: 11.5, ..healthy() }, DEBOUNCE_UV as u32);
        assert!(d.charge_on && !d.discharge_on);
    }

    #[test]
    fn over_current_cools_down_then_retries() {
        let mut p = Protection::new();
        let d = run(&mut p, Measurements { current_a: 30.0, ..healthy() }, DEBOUNCE_OC as u32);
        assert!(!d.discharge_on);
        // Still latched during the cooldown, even though current is now 0
        let off = Measurements { current_a: 0.0, ..healthy() };
        let d = run(&mut p, off, OC_COOLDOWN_MS / DT - 1);
        assert!(!d.discharge_on);
        let d = run(&mut p, off, 1);
        assert!(d.discharge_on);
    }

    #[test]
    fn repeated_over_current_latches_fault() {
        let mut p = Protection::new();
        let short = Measurements { current_a: 40.0, ..healthy() };
        let mut d = p.update(&short, DT);
        for _ in 0..(OC_MAX_RETRIES as u32 * (OC_COOLDOWN_MS / DT + DEBOUNCE_OC as u32)) {
            d = p.update(&short, DT);
        }
        assert_eq!(d.state, SystemState::Fault);
        let d = run(&mut p, healthy(), 1000);
        assert!(!d.discharge_on);
        assert_eq!(d.state, SystemState::Fault);
    }

    #[test]
    fn cold_pack_blocks_charging_but_allows_discharge() {
        let d = run(&mut Protection::new(), Measurements { cell_temp_c: Some(-5.0), ..healthy() }, DEBOUNCE_TEMP as u32);
        assert_eq!(d.faults, fault::UNDER_TEMP_CHARGE);
        assert!(!d.charge_on && d.discharge_on);
    }

    #[test]
    fn hot_cells_block_both() {
        let d = run(&mut Protection::new(), Measurements { cell_temp_c: Some(65.0), ..healthy() }, DEBOUNCE_TEMP as u32);
        assert!(!d.charge_on && !d.discharge_on);
    }

    #[test]
    fn lost_thermistor_is_permanent_fault() {
        let mut p = Protection::new();
        let d = run(&mut p, Measurements { cell_temp_c: None, ..healthy() }, DEBOUNCE_SENSOR as u32);
        assert_eq!(d.state, SystemState::Fault);
        let d = run(&mut p, healthy(), 100);
        assert!(!d.charge_on && !d.discharge_on);
    }

    #[test]
    fn near_limit_is_warning() {
        let d = run(&mut Protection::new(), Measurements { pack_v: 16.3, ..healthy() }, 3);
        assert_eq!(d.state, SystemState::Warning);
        assert!(d.charge_on && d.discharge_on);
    }
}
