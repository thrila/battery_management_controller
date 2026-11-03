//! Raw ADC counts -> engineering units. Pure maths, no hardware access.

use crate::config::*;

/// Converts a 12-bit ADC reading to millivolts at the pin.
pub fn raw_to_mv(raw: u16) -> f32 {
    raw as f32 * ADC_VREF_MV / ADC_MAX_RAW
}

/// True when a reading is stuck at 0 or full scale (open/short circuit).
pub fn at_rail(raw: u16) -> bool {
    raw <= ADC_RAIL_MARGIN || raw >= ADC_MAX_RAW as u16 - ADC_RAIL_MARGIN
}

/// Pack voltage in volts, from the 100k/18k divider.
pub fn pack_voltage_v(raw: u16) -> f32 {
    raw_to_mv(raw) * V_DIVIDER_RATIO / 1000.0
}

/// Pack current in amps. Positive = discharge, negative = charge.
pub fn current_a(raw: u16) -> f32 {
    let v_shunt_mv = (raw_to_mv(raw) - CURRENT_OFFSET_MV) / CSA_GAIN;
    v_shunt_mv / 1000.0 / SHUNT_OHM
}

/// NTC temperature in degrees C using the Beta equation.
/// Returns `None` when the thermistor is open or shorted.
pub fn ntc_temp_c(raw: u16) -> Option<f32> {
    if at_rail(raw) {
        return None;
    }
    let v = raw_to_mv(raw);
    if v >= NTC_SUPPLY_MV {
        return None;
    }
    // NTC is the low side of a divider: R_ntc = R_fixed * V / (Vs - V)
    let r_ntc = NTC_R_FIXED_OHM * v / (NTC_SUPPLY_MV - v);
    let t0_k = NTC_T0_C + 273.15;
    let inv_t = 1.0 / t0_k + libm::logf(r_ntc / NTC_R0_OHM) / NTC_BETA;
    Some(1.0 / inv_t - 273.15)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_for_mv(mv: f32) -> u16 {
        (mv * ADC_MAX_RAW / ADC_VREF_MV + 0.5) as u16
    }

    #[test]
    fn full_pack_reads_about_16v8() {
        let raw = raw_for_mv(16_800.0 / V_DIVIDER_RATIO);
        assert!((pack_voltage_v(raw) - 16.8).abs() < 0.05);
    }

    #[test]
    fn zero_current_at_offset() {
        let raw = raw_for_mv(CURRENT_OFFSET_MV);
        assert!(current_a(raw).abs() < 0.1);
    }

    #[test]
    fn current_sign_convention() {
        assert!(current_a(raw_for_mv(CURRENT_OFFSET_MV + 500.0)) > 9.0); // ~+10 A discharge
        assert!(current_a(raw_for_mv(CURRENT_OFFSET_MV - 500.0)) < -9.0); // ~-10 A charge
    }

    #[test]
    fn ntc_25c_at_half_supply() {
        let t = ntc_temp_c(raw_for_mv(NTC_SUPPLY_MV / 2.0)).unwrap();
        assert!((t - 25.0).abs() < 1.0, "t = {t}");
    }

    #[test]
    fn ntc_open_or_short_is_none() {
        assert!(ntc_temp_c(0).is_none());
        assert!(ntc_temp_c(4095).is_none());
    }
}
