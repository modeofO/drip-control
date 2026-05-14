#![no_std]

//! Pure decision logic for the irrigation controller.
//!
//! All hardware-touching code lives in `main.rs` and the bench-test binaries
//! under `src/bin/`. Everything in this module is `no_std` but builds and
//! tests fine on a host target — run unit tests with:
//!
//!     cargo test --target x86_64-unknown-linux-gnu --lib

pub mod config {
    // Soil sensor ADC thresholds (12-bit, 0–4095). Calibrate with real soil.
    pub const SOIL_DRY: u16 = 2000;
    pub const SOIL_WET: u16 = 2800;
    pub const SENSOR_FAULT_LO: u16 = 50;
    pub const SENSOR_FAULT_HI: u16 = 4045;

    // Battery (millivolts, measured via 100K/100K divider on ADC).
    pub const BATTERY_LOW_MV: u16 = 3300;
    pub const BATTERY_CRITICAL_MV: u16 = 3000;

    // Limits.
    pub const MAX_DAILY_CYCLES: u8 = 6;
    pub const DAY_SECS: u64 = 86_400;

    // Timing.
    pub const CHECK_INTERVAL_SECS: u64 = 900; // 15 min between sensor checks
    pub const MAX_WATERING_SECS: u64 = 300;   // 5-min hard cap per activation
    pub const WATERING_POLL_SECS: u64 = 5;    // re-read soil every 5s while watering
    pub const WDG_PET_SECS: u64 = 20;         // pet watchdog every 20s during sleep
    pub const WDG_TIMEOUT_US: u32 = 30_000_000; // 30s watchdog timeout

    // ADC reference. The Nucleo-F411RE Vref+ is tied to VDD = 3.3V.
    pub const ADC_VREF_MV: u32 = 3300;
    pub const ADC_FULL_SCALE: u32 = 4096; // 12-bit, treating 4096 as full scale
}

use config::*;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BatteryStatus {
    Critical,
    Low,
    Ok,
}

pub fn classify_battery(mv: u16) -> BatteryStatus {
    if mv < BATTERY_CRITICAL_MV {
        BatteryStatus::Critical
    } else if mv < BATTERY_LOW_MV {
        BatteryStatus::Low
    } else {
        BatteryStatus::Ok
    }
}

pub fn is_sensor_fault(reading: u16) -> bool {
    reading < SENSOR_FAULT_LO || reading > SENSOR_FAULT_HI
}

/// Convert a raw ADC reading on the battery divider input to mV at the cell.
/// The divider is 100K/100K, so the ADC sees Vbat/2.
pub fn adc_to_battery_mv(raw: u16) -> u16 {
    ((raw as u32 * ADC_VREF_MV * 2) / ADC_FULL_SCALE) as u16
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum WateringDecision {
    Skip,
    Water,
}

/// Decide whether to start a watering cycle this iteration.
///
/// Skip rules (in priority order): critical battery → any sensor fault →
/// daily cap reached → soil not dry. Otherwise: water.
pub fn watering_decision(
    s1: u16,
    s2: u16,
    daily_cycles: u8,
    battery: BatteryStatus,
) -> WateringDecision {
    if battery == BatteryStatus::Critical {
        return WateringDecision::Skip;
    }
    if is_sensor_fault(s1) || is_sensor_fault(s2) {
        return WateringDecision::Skip;
    }
    if daily_cycles >= MAX_DAILY_CYCLES {
        return WateringDecision::Skip;
    }
    if s1 < SOIL_DRY || s2 < SOIL_DRY {
        WateringDecision::Water
    } else {
        WateringDecision::Skip
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum WaterResult {
    SoilWet,
    MaxDuration,
    SensorFault,
}

impl WaterResult {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SoilWet => "soil_wet",
            Self::MaxDuration => "max_duration",
            Self::SensorFault => "sensor_fault",
        }
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum WaterTick {
    Continue,
    Stop(WaterResult),
}

/// Decide whether to keep watering, given current sensor readings and elapsed time.
///
/// Stop rules (priority order): max duration exceeded → sensor fault →
/// both sensors saturated wet. Otherwise: continue.
pub fn water_tick(s1: u16, s2: u16, elapsed_secs: u64) -> WaterTick {
    if elapsed_secs >= MAX_WATERING_SECS {
        return WaterTick::Stop(WaterResult::MaxDuration);
    }
    if is_sensor_fault(s1) || is_sensor_fault(s2) {
        return WaterTick::Stop(WaterResult::SensorFault);
    }
    if s1 > SOIL_WET && s2 > SOIL_WET {
        return WaterTick::Stop(WaterResult::SoilWet);
    }
    WaterTick::Continue
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- adc_to_battery_mv ------------------------------------------------

    #[test]
    fn adc_zero_is_zero_mv() {
        assert_eq!(adc_to_battery_mv(0), 0);
    }

    #[test]
    fn adc_full_scale_is_two_vref() {
        // 4095 raw ≈ 2 × Vref = 6600 mV (just under, due to /4096)
        let mv = adc_to_battery_mv(4095);
        assert!(mv >= 6597 && mv <= 6600, "got {} mV", mv);
    }

    #[test]
    fn adc_mid_scale_is_one_vref() {
        // 2048 raw → divider midpoint = Vref/2, Vbat = Vref = 3300 mV
        assert_eq!(adc_to_battery_mv(2048), 3300);
    }

    #[test]
    fn adc_typical_charged_cell() {
        // 4.0 V cell → 2.0 V at divider → raw = 2.0/3.3 × 4096 ≈ 2483
        let mv = adc_to_battery_mv(2483);
        assert!(mv >= 3995 && mv <= 4005, "got {} mV", mv);
    }

    // ---- classify_battery -------------------------------------------------

    #[test]
    fn battery_above_low_is_ok() {
        assert_eq!(classify_battery(4000), BatteryStatus::Ok);
        assert_eq!(classify_battery(BATTERY_LOW_MV), BatteryStatus::Ok);
    }

    #[test]
    fn battery_just_below_low_is_low() {
        assert_eq!(classify_battery(BATTERY_LOW_MV - 1), BatteryStatus::Low);
        assert_eq!(classify_battery(BATTERY_CRITICAL_MV), BatteryStatus::Low);
    }

    #[test]
    fn battery_below_critical_is_critical() {
        assert_eq!(classify_battery(BATTERY_CRITICAL_MV - 1), BatteryStatus::Critical);
        assert_eq!(classify_battery(0), BatteryStatus::Critical);
    }

    // ---- is_sensor_fault --------------------------------------------------

    #[test]
    fn sensor_fault_boundaries() {
        assert!(is_sensor_fault(0));
        assert!(is_sensor_fault(SENSOR_FAULT_LO - 1));
        assert!(!is_sensor_fault(SENSOR_FAULT_LO));
        assert!(!is_sensor_fault(SENSOR_FAULT_HI));
        assert!(is_sensor_fault(SENSOR_FAULT_HI + 1));
        assert!(is_sensor_fault(4095));
    }

    #[test]
    fn sensor_fault_mid_range_ok() {
        assert!(!is_sensor_fault(2000));
        assert!(!is_sensor_fault(SOIL_DRY));
        assert!(!is_sensor_fault(SOIL_WET));
    }

    // ---- watering_decision ------------------------------------------------

    #[test]
    fn skip_when_battery_critical() {
        // Even with bone-dry soil and no cycles used, critical battery wins.
        let d = watering_decision(0 + SENSOR_FAULT_LO, 100, 0, BatteryStatus::Critical);
        assert_eq!(d, WateringDecision::Skip);
    }

    #[test]
    fn skip_when_sensor_faulted() {
        // s1 below fault threshold → skip even if s2 is dry.
        let d = watering_decision(0, 500, 0, BatteryStatus::Ok);
        assert_eq!(d, WateringDecision::Skip);
        // s2 above fault threshold → skip.
        let d = watering_decision(500, 4095, 0, BatteryStatus::Ok);
        assert_eq!(d, WateringDecision::Skip);
    }

    #[test]
    fn skip_when_daily_cap_reached() {
        let d = watering_decision(500, 500, MAX_DAILY_CYCLES, BatteryStatus::Ok);
        assert_eq!(d, WateringDecision::Skip);
        let d = watering_decision(500, 500, MAX_DAILY_CYCLES + 1, BatteryStatus::Ok);
        assert_eq!(d, WateringDecision::Skip);
    }

    #[test]
    fn skip_when_soil_wet_enough() {
        let d = watering_decision(SOIL_DRY, SOIL_DRY, 0, BatteryStatus::Ok);
        assert_eq!(d, WateringDecision::Skip);
        let d = watering_decision(3000, 3000, 0, BatteryStatus::Ok);
        assert_eq!(d, WateringDecision::Skip);
    }

    #[test]
    fn water_when_either_sensor_dry() {
        let d = watering_decision(SOIL_DRY - 1, 3000, 0, BatteryStatus::Ok);
        assert_eq!(d, WateringDecision::Water);
        let d = watering_decision(3000, SOIL_DRY - 1, 0, BatteryStatus::Ok);
        assert_eq!(d, WateringDecision::Water);
    }

    #[test]
    fn water_when_low_battery_but_not_critical() {
        // Low battery still allows watering — only critical blocks it.
        let d = watering_decision(500, 500, 0, BatteryStatus::Low);
        assert_eq!(d, WateringDecision::Water);
    }

    // ---- water_tick -------------------------------------------------------

    #[test]
    fn tick_continue_in_normal_conditions() {
        let t = water_tick(1500, 1500, 60);
        assert_eq!(t, WaterTick::Continue);
    }

    #[test]
    fn tick_stops_at_max_duration() {
        let t = water_tick(1500, 1500, MAX_WATERING_SECS);
        assert_eq!(t, WaterTick::Stop(WaterResult::MaxDuration));
    }

    #[test]
    fn tick_max_duration_takes_priority_over_wet() {
        // Even if soil reads wet at the timeout instant, log as MaxDuration —
        // priority matches the documented order.
        let t = water_tick(3500, 3500, MAX_WATERING_SECS);
        assert_eq!(t, WaterTick::Stop(WaterResult::MaxDuration));
    }

    #[test]
    fn tick_stops_on_sensor_fault() {
        let t = water_tick(0, 1500, 60);
        assert_eq!(t, WaterTick::Stop(WaterResult::SensorFault));
        let t = water_tick(1500, 4095, 60);
        assert_eq!(t, WaterTick::Stop(WaterResult::SensorFault));
    }

    #[test]
    fn tick_stops_when_both_sensors_wet() {
        let t = water_tick(SOIL_WET + 1, SOIL_WET + 1, 60);
        assert_eq!(t, WaterTick::Stop(WaterResult::SoilWet));
    }

    #[test]
    fn tick_continues_when_only_one_sensor_wet() {
        // Two-sensor AND logic: stop only when *both* are wet.
        let t = water_tick(SOIL_WET + 1, 1500, 60);
        assert_eq!(t, WaterTick::Continue);
    }

    #[test]
    fn water_result_strings_stable() {
        assert_eq!(WaterResult::SoilWet.as_str(), "soil_wet");
        assert_eq!(WaterResult::MaxDuration.as_str(), "max_duration");
        assert_eq!(WaterResult::SensorFault.as_str(), "sensor_fault");
    }
}
