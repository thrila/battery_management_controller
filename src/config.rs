//! Every hardware constant and protection threshold lives here.
//! Adjust this file for a different pack or board; nothing else should
//! need to change.

// ---------------------------------------------------------------- pack
pub const CELLS_IN_SERIES: u32 = 4;
pub const CAPACITY_MAH: f32 = 3000.0;

// ---------------------------------------------------------------- timing
pub const LOOP_PERIOD_MS: u32 = 100; // 10 Hz control loop
pub const REPORT_EVERY_N_LOOPS: u32 = 10; // 1 Hz telemetry
pub const WATCHDOG_TIMEOUT_MS: u64 = 2000;
pub const ADC_OVERSAMPLE: u32 = 16;

// ---------------------------------------------------------------- ADC
/// Full-scale input voltage of the ESP32 ADC at 11 dB attenuation.
/// Calibrate against a multimeter; typical range is 3100-3300 mV.
pub const ADC_VREF_MV: f32 = 3100.0;
pub const ADC_MAX_RAW: f32 = 4095.0;
/// Raw values this close to a rail mean an open or shorted sensor.
pub const ADC_RAIL_MARGIN: u16 = 20;

// ---------------------------------------------------------------- voltage divider
pub const V_DIV_R_TOP_OHM: f32 = 100_000.0;
pub const V_DIV_R_BOTTOM_OHM: f32 = 18_000.0;
pub const V_DIVIDER_RATIO: f32 = (V_DIV_R_TOP_OHM + V_DIV_R_BOTTOM_OHM) / V_DIV_R_BOTTOM_OHM;

// ---------------------------------------------------------------- current sense
pub const SHUNT_OHM: f32 = 0.001;
pub const CSA_GAIN: f32 = 50.0; // INA240A2
/// Amplifier output at zero current (REF pin). Calibrate with no load.
pub const CURRENT_OFFSET_MV: f32 = 1650.0;

// ---------------------------------------------------------------- thermistors
pub const NTC_SUPPLY_MV: f32 = 3300.0;
pub const NTC_R_FIXED_OHM: f32 = 10_000.0;
pub const NTC_R0_OHM: f32 = 10_000.0;
pub const NTC_T0_C: f32 = 25.0;
pub const NTC_BETA: f32 = 3950.0;

// ---------------------------------------------------------------- filtering
pub const VOLTAGE_EMA_ALPHA: f32 = 0.3;
pub const TEMP_EMA_ALPHA: f32 = 0.1;
pub const CURRENT_DISPLAY_EMA_ALPHA: f32 = 0.2;

// ---------------------------------------------------------------- protection
// Voltage (whole pack)
pub const OV_TRIP_V: f32 = 4.25 * CELLS_IN_SERIES as f32; // 17.0 V
pub const OV_RELEASE_V: f32 = 4.15 * CELLS_IN_SERIES as f32; // 16.6 V
pub const UV_TRIP_V: f32 = 3.00 * CELLS_IN_SERIES as f32; // 12.0 V
pub const UV_RELEASE_V: f32 = 3.20 * CELLS_IN_SERIES as f32; // 12.8 V

// Current (positive = discharge)
pub const OCD_TRIP_A: f32 = 25.0;
pub const OCC_TRIP_A: f32 = 4.0; // magnitude of charge current
pub const OC_COOLDOWN_MS: u32 = 5000;
pub const OC_MAX_RETRIES: u8 = 3;

// Temperature
pub const OT_CELL_TRIP_C: f32 = 60.0;
pub const OT_CELL_RELEASE_C: f32 = 50.0;
pub const OT_CHARGE_TRIP_C: f32 = 45.0;
pub const OT_CHARGE_RELEASE_C: f32 = 40.0;
pub const UT_CHARGE_TRIP_C: f32 = 0.0;
pub const UT_CHARGE_RELEASE_C: f32 = 3.0;
pub const OT_FET_TRIP_C: f32 = 90.0;
pub const OT_FET_RELEASE_C: f32 = 75.0;

/// Fraction of a limit at which the WARNING state is shown.
pub const WARNING_MARGIN: f32 = 0.05;

// Debounce, in loop iterations
pub const DEBOUNCE_OV: u16 = 3; // 300 ms
pub const DEBOUNCE_UV: u16 = 5; // 500 ms
pub const DEBOUNCE_OC: u16 = 2; // 200 ms
pub const DEBOUNCE_TEMP: u16 = 10; // 1 s
pub const DEBOUNCE_SENSOR: u16 = 3; // 300 ms

// ---------------------------------------------------------------- SoC
pub const IDLE_CURRENT_A: f32 = 0.1;
pub const FULL_RESYNC_V: f32 = 4.175 * CELLS_IN_SERIES as f32; // 16.7 V
pub const FULL_RESYNC_MAX_A: f32 = 0.15;
