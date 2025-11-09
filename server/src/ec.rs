use std::ptr;
use std::ffi::CString;
use winapi::um::winnt::{HANDLE, GENERIC_READ, GENERIC_WRITE};
use winapi::um::fileapi::{CreateFileA, OPEN_EXISTING};
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::ioapiset::DeviceIoControl;
use winapi::um::minwinbase::OVERLAPPED;
use winapi::um::errhandlingapi::GetLastError;

// WinRing0 driver constants
const WINRING0_DEVICE_NAME: &str = "\\\\.\\WinRing0_1_2_0";
const OLS_TYPE: u32 = 40000;

// IO Control codes for WinRing0
const IOCTL_OLS_READ_IO_PORT_BYTE: u32 = (OLS_TYPE << 16) | (1 << 14) | (0x833 << 2) | 0;
const IOCTL_OLS_WRITE_IO_PORT_BYTE: u32 = (OLS_TYPE << 16) | (2 << 14) | (0x836 << 2) | 0;

// EC constants
const COMMAND_PORT: u32 = 0x66;
const DATA_PORT: u32 = 0x62;
const EC_COMMAND_READ: u8 = 0x80;
const EC_COMMAND_WRITE: u8 = 0x81;
const RW_TIMEOUT: u32 = 500;
const MAX_RETRIES: u32 = 5;

// EC Status flags
const EC_STATUS_OUTPUT_BUFFER_FULL: u8 = 0x01;
const EC_STATUS_INPUT_BUFFER_FULL: u8 = 0x02;

// EC Register mappings (from Linux driver)
const EC_REG_FIRMWARE_MAJOR: u8 = 0x00;
const EC_REG_FIRMWARE_MINOR: u8 = 0x01;
const EC_REG_APU_POWER_MODE: u8 = 0x31;
const EC_REG_APU_TEMPERATURE: u8 = 0x70;

// Fan register mappings
const EC_REG_FAN1_SPEED_HIGH: u8 = 0x35;
const EC_REG_FAN1_SPEED_LOW: u8 = 0x36;
const EC_REG_FAN1_MODE: u8 = 0x21;

const EC_REG_FAN2_SPEED_HIGH: u8 = 0x37;
const EC_REG_FAN2_SPEED_LOW: u8 = 0x38;
const EC_REG_FAN2_MODE: u8 = 0x23;

const EC_REG_FAN3_SPEED_HIGH: u8 = 0x28;
const EC_REG_FAN3_SPEED_LOW: u8 = 0x29;
const EC_REG_FAN3_MODE: u8 = 0x25;

#[repr(C)]
struct WriteIoPortInput {
    port_number: u32,
    value: u8,
}

#[derive(Debug, Clone)]
pub enum EcOperation {
    GetFirmwareVersion,
    GetApuPowerMode,
    SetApuPowerMode(String),
    GetApuTemperature,
    GetFanRpm(u8),
    GetFanMode(u8),
    SetFanMode(u8, String),
    GetFanLevel(u8),
    SetFanLevel(u8, u8),
    GetFanRampupCurve(u8),
    SetFanRampupCurve(u8, [u8; 5]),
    GetFanRampdownCurve(u8),
    SetFanRampdownCurve(u8, [u8; 5]),
}

#[derive(Debug, Clone)]
pub enum EcResult {
    FirmwareVersion { major: u8, minor: u8 },
    ApuPowerMode(String),
    ApuTemperature(u8),
    FanRpm(u16),
    FanMode(String),
    FanLevel(u8),
    FanRampupCurve([u8; 5]),
    FanRampdownCurve([u8; 5]),
}

#[derive(Debug, Clone, Copy)]
pub struct FanCurveData {
    pub rampup_curve: [u8; 5],    // Temperature thresholds for levels 1-5
    pub rampdown_curve: [u8; 5],  // Temperature thresholds for levels 1-5
    pub mode: FanMode,            // Use enum instead of String for Copy trait
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FanMode {
    Auto,
    Fixed,
    Curve,
}

impl FanMode {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            FanMode::Auto => "auto",
            FanMode::Fixed => "fixed",
            FanMode::Curve => "curve",
        }
    }
    
    pub fn from_str(s: &str) -> Option<FanMode> {
        match s {
            "auto" => Some(FanMode::Auto),
            "fixed" => Some(FanMode::Fixed),
            "curve" => Some(FanMode::Curve),
            _ => None,
        }
    }
}

impl Default for FanCurveData {
    fn default() -> Self {
        FanCurveData {
            rampup_curve: [60, 70, 83, 95, 97],   // Default from Linux driver
            rampdown_curve: [40, 50, 80, 94, 96], // Default from Linux driver
            mode: FanMode::Auto,
        }
    }
}

pub struct EcController {
    driver_handle: HANDLE,
    fan_curves: std::sync::Mutex<[FanCurveData; 3]>, // Data for fans 1, 2, 3
}

impl EcController {
    pub fn new() -> Result<Self, String> {
        let device_name = CString::new(WINRING0_DEVICE_NAME).unwrap();

        let handle = unsafe {
            CreateFileA(
                device_name.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                ptr::null_mut(),
                OPEN_EXISTING,
                0,
                ptr::null_mut(),
            )
        };

        if handle == INVALID_HANDLE_VALUE {
            let error = unsafe { GetLastError() };
            return Err(format!("Failed to open WinRing0 driver. Error code: {}", error));
        }

        // Initialize fan curves with defaults, but customize fan3
        let mut curves = [FanCurveData::default(); 3];
        // Fan3 has different default curves from Linux driver
        curves[2].rampup_curve = [20, 60, 83, 95, 97];
        curves[2].rampdown_curve = [0, 50, 80, 94, 96];

        Ok(EcController {
            driver_handle: handle,
            fan_curves: std::sync::Mutex::new(curves),
        })
    }

    pub async fn execute_operation(&self, operation: EcOperation) -> Result<EcResult, String> {
        match operation {
            EcOperation::GetFirmwareVersion => {
                let major = self.read_byte(EC_REG_FIRMWARE_MAJOR)?;
                let minor = self.read_byte(EC_REG_FIRMWARE_MINOR)?;
                
                // Check for invalid values (all zeros or all 0xFF)
                if (major == 0 && minor == 0) || (major == 0xFF && minor == 0xFF) {
                    return Err("Invalid firmware version detected".to_string());
                }
                
                Ok(EcResult::FirmwareVersion { major, minor })
            }
            EcOperation::GetApuPowerMode => {
                let mode_val = self.read_byte(EC_REG_APU_POWER_MODE)?;
                let mode = match mode_val {
                    0x00 => "balanced",
                    0x01 => "performance", 
                    0x02 => "quiet",
                    _ => return Err(format!("Unknown power mode: 0x{:02X}", mode_val)),
                };
                Ok(EcResult::ApuPowerMode(mode.to_string()))
            }
            EcOperation::SetApuPowerMode(mode) => {
                let mode_val = match mode.as_str() {
                    "balanced" => 0x00,
                    "performance" => 0x01,
                    "quiet" => 0x02,
                    _ => return Err(format!("Invalid power mode: {}", mode)),
                };
                self.write_byte(EC_REG_APU_POWER_MODE, mode_val)?;
                Ok(EcResult::ApuPowerMode(mode))
            }
            EcOperation::GetApuTemperature => {
                let temp = self.read_byte(EC_REG_APU_TEMPERATURE)?;
                Ok(EcResult::ApuTemperature(temp))
            }
            EcOperation::GetFanRpm(fan_id) => {
                let (high_reg, low_reg) = self.get_fan_speed_registers(fan_id)?;
                let high = self.read_byte(high_reg)?;
                let low = self.read_byte(low_reg)?;
                let mut rpm = ((high as u16) << 8) | (low as u16);
                
                // Handle fan3 weird behavior (shows 8000 before turning to 0)
                if fan_id == 3 && rpm == 8000 {
                    rpm = 0;
                }
                
                Ok(EcResult::FanRpm(rpm))
            }
            EcOperation::GetFanMode(fan_id) => {
                let mode_reg = self.get_fan_mode_register(fan_id)?;
                let mode_val = self.read_byte(mode_reg)?;
                
                let curves = self.fan_curves.lock().unwrap();
                let fan_idx = (fan_id - 1) as usize;
                
                let mode = match mode_val {
                    0x10 | 0x20 | 0x30 => "auto",
                    0x11 | 0x21 | 0x31 => {
                        // Check stored mode to distinguish between fixed and curve
                        if curves[fan_idx].mode == FanMode::Curve {
                            "curve"
                        } else {
                            "fixed"
                        }
                    },
                    _ => return Err(format!("Unknown fan mode: 0x{:02X}", mode_val)),
                };
                
                Ok(EcResult::FanMode(mode.to_string()))
            }
            EcOperation::SetFanMode(fan_id, mode) => {
                let mode_reg = self.get_fan_mode_register(fan_id)?;
                let base_val = match fan_id {
                    1 => 0x10,
                    2 => 0x20,
                    3 => 0x30,
                    _ => return Err(format!("Invalid fan ID: {}", fan_id)),
                };
                
                let fan_mode = FanMode::from_str(&mode)
                    .ok_or_else(|| format!("Invalid fan mode: {}", mode))?;
                
                let mode_val = match fan_mode {
                    FanMode::Auto => base_val,
                    FanMode::Fixed | FanMode::Curve => base_val + 1,
                };
                
                // Update stored mode
                {
                    let mut curves = self.fan_curves.lock().unwrap();
                    let fan_idx = (fan_id - 1) as usize;
                    curves[fan_idx].mode = fan_mode;
                }
                
                self.write_byte(mode_reg, mode_val)?;
                
                // When switching to curve mode, set initial fan level based on current temperature
                if fan_mode == FanMode::Curve {
                    if let Ok(temp) = self.read_byte(EC_REG_APU_TEMPERATURE) {
                        let curves = self.fan_curves.lock().unwrap();
                        let fan_idx = (fan_id - 1) as usize;
                        let mut initial_level = 0;
                        
                        // Find appropriate level based on current temperature using rampup curve
                        for i in (1..=5).rev() {
                            if temp >= curves[fan_idx].rampup_curve[i - 1] {
                                initial_level = i as u8;
                                break;
                            }
                        }
                        
                        drop(curves); // Release lock before calling write_fan_level
                        self.write_fan_level(fan_id, initial_level)?;
                    }
                }
                
                Ok(EcResult::FanMode(mode))
            }
            EcOperation::GetFanLevel(fan_id) => {
                let mode_reg = self.get_fan_mode_register(fan_id)?;
                let level_val = self.read_byte(mode_reg + 1)?;
                
                let level = match level_val & 0xF {
                    0x7 => 0, // off
                    0x2 => 1, // 20%
                    0x3 => 2, // 40%
                    0x4 => 3, // 60%
                    0x5 => 4, // 80%
                    0x6 => 5, // 100%
                    _ => 0,   // default to off
                };
                
                Ok(EcResult::FanLevel(level))
            }
            EcOperation::SetFanLevel(fan_id, level) => {
                if level > 5 {
                    return Err("Fan level must be 0-5".to_string());
                }
                
                self.write_fan_level(fan_id, level)?;
                Ok(EcResult::FanLevel(level))
            }
            EcOperation::GetFanRampupCurve(fan_id) => {
                if fan_id < 1 || fan_id > 3 {
                    return Err(format!("Invalid fan ID: {}", fan_id));
                }
                
                let curves = self.fan_curves.lock().unwrap();
                let fan_idx = (fan_id - 1) as usize;
                Ok(EcResult::FanRampupCurve(curves[fan_idx].rampup_curve))
            }
            EcOperation::SetFanRampupCurve(fan_id, curve) => {
                if fan_id < 1 || fan_id > 3 {
                    return Err(format!("Invalid fan ID: {}", fan_id));
                }
                
                // Validate curve values (0-100°C)
                for &temp in &curve {
                    if temp > 100 {
                        return Err("Temperature values must be 0-100°C".to_string());
                    }
                }
                
                let mut curves = self.fan_curves.lock().unwrap();
                let fan_idx = (fan_id - 1) as usize;
                curves[fan_idx].rampup_curve = curve;
                Ok(EcResult::FanRampupCurve(curve))
            }
            EcOperation::GetFanRampdownCurve(fan_id) => {
                if fan_id < 1 || fan_id > 3 {
                    return Err(format!("Invalid fan ID: {}", fan_id));
                }
                
                let curves = self.fan_curves.lock().unwrap();
                let fan_idx = (fan_id - 1) as usize;
                Ok(EcResult::FanRampdownCurve(curves[fan_idx].rampdown_curve))
            }
            EcOperation::SetFanRampdownCurve(fan_id, curve) => {
                if fan_id < 1 || fan_id > 3 {
                    return Err(format!("Invalid fan ID: {}", fan_id));
                }
                
                // Validate curve values (0-100°C)
                for &temp in &curve {
                    if temp > 100 {
                        return Err("Temperature values must be 0-100°C".to_string());
                    }
                }
                
                let mut curves = self.fan_curves.lock().unwrap();
                let fan_idx = (fan_id - 1) as usize;
                curves[fan_idx].rampdown_curve = curve;
                Ok(EcResult::FanRampdownCurve(curve))
            }
        }
    }

    fn get_fan_speed_registers(&self, fan_id: u8) -> Result<(u8, u8), String> {
        match fan_id {
            1 => Ok((EC_REG_FAN1_SPEED_HIGH, EC_REG_FAN1_SPEED_LOW)),
            2 => Ok((EC_REG_FAN2_SPEED_HIGH, EC_REG_FAN2_SPEED_LOW)),
            3 => Ok((EC_REG_FAN3_SPEED_HIGH, EC_REG_FAN3_SPEED_LOW)),
            _ => Err(format!("Invalid fan ID: {}", fan_id)),
        }
    }

    fn get_fan_mode_register(&self, fan_id: u8) -> Result<u8, String> {
        match fan_id {
            1 => Ok(EC_REG_FAN1_MODE),
            2 => Ok(EC_REG_FAN2_MODE),
            3 => Ok(EC_REG_FAN3_MODE),
            _ => Err(format!("Invalid fan ID: {}", fan_id)),
        }
    }

    fn write_fan_level(&self, fan_id: u8, level: u8) -> Result<(), String> {
        if level > 5 {
            return Err("Fan level must be 0-5".to_string());
        }
        
        let mode_reg = self.get_fan_mode_register(fan_id)?;
        let base_val = match fan_id {
            1 => 0x10,
            2 => 0x20,
            3 => 0x30,
            _ => return Err(format!("Invalid fan ID: {}", fan_id)),
        };
        
        let level_val = base_val + match level {
            0 => 0x7, // off
            1 => 0x2, // 20%
            2 => 0x3, // 40%
            3 => 0x4, // 60%
            4 => 0x5, // 80%
            5 => 0x6, // 100%
            _ => 0x7, // default to off
        };
        
        self.write_byte(mode_reg + 1, level_val)
    }

    fn read_fan_level(&self, fan_id: u8) -> Result<u8, String> {
        let mode_reg = self.get_fan_mode_register(fan_id)?;
        let level_val = self.read_byte(mode_reg + 1)?;
        
        let level = match level_val & 0xF {
            0x7 => 0, // off
            0x2 => 1, // 20%
            0x3 => 2, // 40%
            0x4 => 3, // 60%
            0x5 => 4, // 80%
            0x6 => 5, // 100%
            _ => 0,   // default to off
        };
        
        Ok(level)
    }

    pub fn update_curve_fans(&self) -> Result<Vec<String>, String> {
        let mut log_messages = Vec::new();
        let temp = self.read_byte(EC_REG_APU_TEMPERATURE)?;
        
        let curves = self.fan_curves.lock().unwrap();
        
        for fan_id in 1..=3 {
            let fan_idx = (fan_id - 1) as usize;
            
            if curves[fan_idx].mode == FanMode::Curve {
                let current_level = self.read_fan_level(fan_id)?;
                let mut new_level = current_level;
                
                // Check if we should ramp up
                if current_level < 5 && temp >= curves[fan_idx].rampup_curve[current_level as usize] {
                    new_level = current_level + 1;
                    log_messages.push(format!("Fan{} ramping up to level {} (temp: {}°C, threshold: {}°C)",
                        fan_id, new_level, temp, curves[fan_idx].rampup_curve[current_level as usize]));
                }
                // Check if we should ramp down
                else if current_level > 0 && temp <= curves[fan_idx].rampdown_curve[(current_level - 1) as usize] {
                    new_level = current_level - 1;
                    log_messages.push(format!("Fan{} ramping down to level {} (temp: {}°C, threshold: {}°C)",
                        fan_id, new_level, temp, curves[fan_idx].rampdown_curve[(current_level - 1) as usize]));
                }
                
                if new_level != current_level {
                    drop(curves); // Release lock before writing
                    self.write_fan_level(fan_id, new_level)?;
                    return Ok(log_messages); // Return early to reacquire lock on next iteration
                }
            }
        }
        
        Ok(log_messages)
    }

    pub fn has_curve_fans(&self) -> bool {
        let curves = self.fan_curves.lock().unwrap();
        curves.iter().any(|curve| curve.mode == FanMode::Curve)
    }

    fn read_io_port(&self, port: u32) -> Result<u8, String> {
        let mut value: u32 = 0;
        let mut bytes_returned: u32 = 0;

        let success = unsafe {
            DeviceIoControl(
                self.driver_handle,
                IOCTL_OLS_READ_IO_PORT_BYTE,
                &port as *const u32 as *mut _,
                std::mem::size_of::<u32>() as u32,
                &mut value as *mut u32 as *mut _,
                std::mem::size_of::<u32>() as u32,
                &mut bytes_returned,
                ptr::null_mut() as *mut OVERLAPPED,
            )
        };

        if success == 0 {
            let error = unsafe { GetLastError() };
            Err(format!("Failed to read IO port 0x{:X}. Error code: {}", port, error))
        } else {
            Ok((value & 0xFF) as u8)
        }
    }

    fn write_io_port(&self, port: u32, value: u8) -> Result<(), String> {
        let input = WriteIoPortInput {
            port_number: port,
            value,
        };
        let mut bytes_returned: u32 = 0;

        let success = unsafe {
            DeviceIoControl(
                self.driver_handle,
                IOCTL_OLS_WRITE_IO_PORT_BYTE,
                &input as *const WriteIoPortInput as *mut _,
                std::mem::size_of::<WriteIoPortInput>() as u32,
                ptr::null_mut(),
                0,
                &mut bytes_returned,
                ptr::null_mut() as *mut OVERLAPPED,
            )
        };

        if success == 0 {
            let error = unsafe { GetLastError() };
            Err(format!("Failed to write IO port 0x{:X} value 0x{:02X}. Error code: {}", port, value, error))
        } else {
            Ok(())
        }
    }

    fn wait_for_ec_status(&self, status: u8, is_set: bool) -> bool {
        for _ in 0..RW_TIMEOUT {
            if let Ok(mut value) = self.read_io_port(COMMAND_PORT) {
                if is_set {
                    value = !value;
                }
                if (status & value) == 0 {
                    return true;
                }
            } else {
                return false;
            }
        }
        false
    }

    fn wait_write(&self) -> bool {
        self.wait_for_ec_status(EC_STATUS_INPUT_BUFFER_FULL, false)
    }

    fn wait_read(&self) -> bool {
        self.wait_for_ec_status(EC_STATUS_OUTPUT_BUFFER_FULL, true)
    }

    fn try_read_byte(&self, register: u8) -> Result<u8, String> {
        if !self.wait_write() {
            return Err("Timeout waiting for write".to_string());
        }

        self.write_io_port(COMMAND_PORT, EC_COMMAND_READ)?;

        if !self.wait_write() {
            return Err("Timeout waiting for write after command".to_string());
        }

        self.write_io_port(DATA_PORT, register)?;

        if !self.wait_write() || !self.wait_read() {
            return Err("Timeout waiting for read".to_string());
        }

        self.read_io_port(DATA_PORT)
    }

    fn try_write_byte(&self, register: u8, value: u8) -> Result<(), String> {
        if !self.wait_write() {
            return Err("Timeout waiting for write".to_string());
        }

        self.write_io_port(COMMAND_PORT, EC_COMMAND_WRITE)?;

        if !self.wait_write() {
            return Err("Timeout waiting for write after command".to_string());
        }

        self.write_io_port(DATA_PORT, register)?;

        if !self.wait_write() {
            return Err("Timeout waiting for write after register".to_string());
        }

        self.write_io_port(DATA_PORT, value)?;
        Ok(())
    }

    fn read_byte(&self, register: u8) -> Result<u8, String> {
        for _ in 0..MAX_RETRIES {
            if let Ok(value) = self.try_read_byte(register) {
                return Ok(value);
            }
        }
        Err("Failed to read byte after retries".to_string())
    }

    fn write_byte(&self, register: u8, value: u8) -> Result<(), String> {
        for _ in 0..MAX_RETRIES {
            if self.try_write_byte(register, value).is_ok() {
                return Ok(());
            }
        }
        Err("Failed to write byte after retries".to_string())
    }
}

impl Drop for EcController {
    fn drop(&mut self) {
        if self.driver_handle != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.driver_handle);
            }
        }
    }
}

unsafe impl Send for EcController {}
unsafe impl Sync for EcController {}