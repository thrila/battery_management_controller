//! Battery management controller for ESP32 (bare metal, no_std).
//!
//! Pins:
//!   GPIO34 (ADC1) pack voltage divider      GPIO25  CHG FET enable
//!   GPIO35 (ADC1) current-sense amplifier   GPIO26  DSG FET enable
//!   GPIO32 (ADC1) cell NTC                  GPIO2   status LED
//!   GPIO33 (ADC1) FET heatsink NTC
//!
//! See README.md for the full design.

#![no_std]
#![no_main]

mod config;
mod filter;
mod protection;
mod sensors;
mod soc;

use esp_backtrace as _;
use esp_hal::{
    analog::adc::{Adc, AdcConfig, Attenuation},
    delay::Delay,
    gpio::{Level, Output, OutputConfig},
    main,
    rtc_cntl::{Rtc, RwdtStage},
    time::{Duration, Instant},
};
use esp_println::println;

use config::*;
use filter::Ema;
use protection::{Decision, Measurements, Protection, SystemState};
use soc::SocEstimator;

esp_bootloader_esp_idf::esp_app_desc!();

/// Reads an ADC pin `ADC_OVERSAMPLE` times and returns the average,
/// or `None` if any conversion failed. A macro because esp-hal's ADC pin
/// types are generic over the GPIO, which makes a plain function verbose.
macro_rules! read_avg {
    ($adc:expr, $pin:expr) => {{
        let mut sum: u32 = 0;
        let mut ok = true;
        for _ in 0..ADC_OVERSAMPLE {
            match nb::block!($adc.read_oneshot(&mut $pin)) {
                Ok(v) => sum += v as u32,
                Err(_) => {
                    ok = false;
                    break;
                }
            }
        }
        if ok { Some((sum / ADC_OVERSAMPLE) as u16) } else { None }
    }};
}

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let delay = Delay::new();

    // Outputs. FETs start OFF and stay off until the first valid reading.
    let mut chg_fet = Output::new(peripherals.GPIO25, Level::Low, OutputConfig::default());
    let mut dsg_fet = Output::new(peripherals.GPIO26, Level::Low, OutputConfig::default());
    let mut led = Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default());

    // ADC1 inputs, 11 dB attenuation (~0-3.1 V)
    let mut adc_cfg = AdcConfig::new();
    let mut pin_v = adc_cfg.enable_pin(peripherals.GPIO34, Attenuation::_11dB);
    let mut pin_i = adc_cfg.enable_pin(peripherals.GPIO35, Attenuation::_11dB);
    let mut pin_tc = adc_cfg.enable_pin(peripherals.GPIO32, Attenuation::_11dB);
    let mut pin_tf = adc_cfg.enable_pin(peripherals.GPIO33, Attenuation::_11dB);
    let mut adc = Adc::new(peripherals.ADC1, adc_cfg);

    // Watchdog: if the loop stalls, the chip resets and the FETs drop out.
    let mut rtc = Rtc::new(peripherals.LPWR);
    rtc.rwdt.set_timeout(RwdtStage::Stage0, Duration::from_millis(WATCHDOG_TIMEOUT_MS));
    rtc.rwdt.enable();

    println!("BMS boot: {}S {} mAh", CELLS_IN_SERIES, CAPACITY_MAH as u32);

    let mut protection = Protection::new();
    let mut soc: Option<SocEstimator> = None;
    let mut v_filter = Ema::new(VOLTAGE_EMA_ALPHA);
    let mut tc_filter = Ema::new(TEMP_EMA_ALPHA);
    let mut tf_filter = Ema::new(TEMP_EMA_ALPHA);
    let mut i_display = Ema::new(CURRENT_DISPLAY_EMA_ALPHA);

    let mut loop_count: u32 = 0;
    let mut last = Instant::now();

    loop {
        let loop_start = Instant::now();
        let mut dt_ms =((loop_start - last).as_micros() / 1000) as u32;
        last = loop_start;
        rtc.rwdt.feed();

        // ---- sample
        let raw_v = read_avg!(adc, pin_v);
        let raw_i = read_avg!(adc, pin_i);
        let raw_tc = read_avg!(adc, pin_tc);
        let raw_tf = read_avg!(adc, pin_tf);

        // ---- convert + filter
        let voltage_sensor_ok = matches!(raw_v, Some(r) if !sensors::at_rail(r));
        let pack_v = v_filter.update(raw_v.map_or(0.0, sensors::pack_voltage_v));
        // Current is NOT filtered for protection: overcurrent must react fast.
        // A failed current read counts as an overcurrent, which fails safe.
        let current_ok = raw_i.is_some();
        let current_a = raw_i.map_or(f32::MAX, sensors::current_a);
        let cell_temp_c = raw_tc.and_then(sensors::ntc_temp_c).map(|t| tc_filter.update(t));
        let fet_temp_c = raw_tf.and_then(sensors::ntc_temp_c).map(|t| tf_filter.update(t));

        let m = Measurements { pack_v, current_a, cell_temp_c, fet_temp_c, voltage_sensor_ok };

        // ---- protect
        if loop_count == 0 {
            dt_ms = LOOP_PERIOD_MS;
        }
        let mut decision = protection.update(&m, dt_ms);

        // ---- state of charge (initialised from OCV on the first good reading)
        if soc.is_none() && voltage_sensor_ok {
            soc = Some(SocEstimator::from_pack_voltage(pack_v));
        }
        if let Some(s) = soc.as_mut() {
            let undervoltage = decision.faults & protection::fault::UNDER_VOLTAGE != 0;
            if current_ok {
                s.update(current_a, pack_v, dt_ms as f32 / 1000.0, undervoltage);
            }
        }

        // ---- act. Hold FETs off until SoC init proves the sensors work.
        let ready = soc.is_some();
        chg_fet.set_level((ready && decision.charge_on).into());
        dsg_fet.set_level((ready && decision.discharge_on).into());
        if !ready {
            decision.state = SystemState::Init;
        }
        drive_led(&mut led, decision.state, loop_count);

        // ---- report
        let shown_i = i_display.update(if current_ok { current_a } else { 0.0 });
        if loop_count % REPORT_EVERY_N_LOOPS == 0 {
            report(&decision, pack_v, shown_i, cell_temp_c, fet_temp_c, soc.as_ref(), ready);
        }

        loop_count = loop_count.wrapping_add(1);

        // ---- hold a fixed period
        let elapsed_ms = loop_start.elapsed().as_millis() as u32;
        if elapsed_ms < LOOP_PERIOD_MS {
            delay.delay_millis(LOOP_PERIOD_MS - elapsed_ms);
        }
    }
}

/// Solid = normal, slow blink = warning/init, fast blink = protect/fault.
fn drive_led(led: &mut Output<'_>, state: SystemState, loop_count: u32) {
    let on = match state {
        SystemState::Normal => true,
        SystemState::Warning | SystemState::Init => (loop_count / 5) % 2 == 0, // 1 Hz
        SystemState::Protect | SystemState::Fault => loop_count % 2 == 0, // 5 Hz
    };
    led.set_level(on.into());
}

fn report(
    d: &Decision,
    pack_v: f32,
    current_a: f32,
    cell_t: Option<f32>,
    fet_t: Option<f32>,
    soc: Option<&SocEstimator>,
    ready: bool,
) {
    let mode = if current_a > IDLE_CURRENT_A {
        "DSG"
    } else if current_a < -IDLE_CURRENT_A {
        "CHG"
    } else {
        "IDLE"
    };
    println!(
        "BMS st={} mode={} V={:.2} I={:+.2} Tc={:.1} Tf={:.1} SoC={:.1}% chg={} dsg={} faults=0x{:04X}",
        d.state.as_str(),
        mode,
        pack_v,
        current_a,
        cell_t.unwrap_or(f32::NAN),
        fet_t.unwrap_or(f32::NAN),
        soc.map_or(0.0, |s| s.percent()),
        (ready && d.charge_on) as u8,
        (ready && d.discharge_on) as u8,
        d.faults,
    );
}
