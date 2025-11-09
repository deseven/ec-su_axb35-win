use std::ptr;
use std::ffi::CString;
use std::path::Path;
use std::fs;
use std::thread;
use std::time::Duration;
use winapi::um::winnt::{HANDLE, GENERIC_READ, GENERIC_WRITE};
use winapi::um::fileapi::{CreateFileA, OPEN_EXISTING};
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::ioapiset::DeviceIoControl;
use winapi::um::minwinbase::OVERLAPPED;
use winapi::um::winsvc::*;
use winapi::um::errhandlingapi::GetLastError;
use winapi::shared::winerror::*;

// WinRing0 driver constants
const WINRING0_DEVICE_NAME: &str = "\\\\.\\WinRing0_1_2_0";
const OLS_TYPE: u32 = 40000;

// IO Control codes for WinRing0 (calculated from Ring0.cs)
// CTL_CODE macro: (DeviceType << 16) | (Access << 14) | (Function << 2) | Method
// METHOD_BUFFERED = 0, FILE_READ_DATA = 1, FILE_WRITE_DATA = 2
const IOCTL_OLS_READ_IO_PORT_BYTE: u32 = (OLS_TYPE << 16) | (1 << 14) | (0x833 << 2) | 0;  // Read access
const IOCTL_OLS_WRITE_IO_PORT_BYTE: u32 = (OLS_TYPE << 16) | (2 << 14) | (0x836 << 2) | 0; // Write access

// EC constants (from EmbeddedControllerBase.cs)
const COMMAND_PORT: u32 = 0x66;
const DATA_PORT: u32 = 0x62;
const EC_COMMAND_READ: u8 = 0x80;
const EC_COMMAND_WRITE: u8 = 0x81;
const RW_TIMEOUT: u32 = 500;
const MAX_RETRIES: u32 = 5;

// EC Status flags
const EC_STATUS_OUTPUT_BUFFER_FULL: u8 = 0x01;
const EC_STATUS_INPUT_BUFFER_FULL: u8 = 0x02;

// Driver management constants
const DRIVER_SERVICE_NAME: &str = "WinRing0_1_2_0";

#[repr(C)]
struct WriteIoPortInput {
    port_number: u32,
    value: u8,
}

// Driver management module
mod driver_manager {
    use super::*;
    use winapi::um::winnt::{SERVICE_KERNEL_DRIVER, SERVICE_DEMAND_START, SERVICE_ERROR_NORMAL};
    
    pub struct DriverManager {
        service_name: String,
    }
    
    impl DriverManager {
        pub fn new() -> Self {
            DriverManager {
                service_name: DRIVER_SERVICE_NAME.to_string(),
            }
        }
        
        pub fn is_driver_loaded(&self) -> bool {
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
            
            if handle != INVALID_HANDLE_VALUE {
                unsafe { CloseHandle(handle); }
                true
            } else {
                false
            }
        }
        
        pub fn install_and_load_driver(&self) -> Result<(), String> {
            // Determine the correct driver file based on architecture
            let driver_filename = if cfg!(target_arch = "x86_64") {
                "WinRing0x64.sys"
            } else {
                "WinRing0.sys"
            };
            
            let driver_path = format!("src/winring0/{}", driver_filename);
            
            if !Path::new(&driver_path).exists() {
                return Err(format!("Driver file not found: {}", driver_path));
            }
            
            // Get absolute path
            let absolute_path = match fs::canonicalize(&driver_path) {
                Ok(path) => path.to_string_lossy().to_string(),
                Err(e) => return Err(format!("Failed to get absolute path: {}", e)),
            };
            
            println!("Attempting to install driver from: {}", absolute_path);
            
            // Try to install the driver
            match self.install_driver(&absolute_path) {
                Ok(_) => {
                    println!("Driver installed successfully");
                    // Give the system a moment to register the driver
                    thread::sleep(Duration::from_millis(500));
                    Ok(())
                }
                Err(e) => {
                    // If installation failed, try to delete and reinstall
                    println!("Initial installation failed: {}", e);
                    println!("Attempting to delete existing service and reinstall...");
                    
                    let _ = self.delete_driver(); // Ignore errors here
                    thread::sleep(Duration::from_millis(2000)); // Wait for cleanup
                    
                    match self.install_driver(&absolute_path) {
                        Ok(_) => {
                            println!("Driver reinstalled successfully");
                            thread::sleep(Duration::from_millis(500));
                            Ok(())
                        }
                        Err(e2) => Err(format!("Failed to install driver after retry: {}", e2))
                    }
                }
            }
        }
        
        fn install_driver(&self, driver_path: &str) -> Result<(), String> {
            let service_name = CString::new(self.service_name.as_str()).unwrap();
            let driver_path_cstr = CString::new(driver_path).unwrap();
            
            unsafe {
                // Open Service Control Manager
                let sc_manager = OpenSCManagerA(
                    ptr::null(),
                    ptr::null(),
                    SC_MANAGER_ALL_ACCESS,
                );
                
                if sc_manager.is_null() {
                    let error = GetLastError();
                    return Err(format!("Failed to open Service Control Manager. Error: {}", error));
                }
                
                // Create the service
                let service = CreateServiceA(
                    sc_manager,
                    service_name.as_ptr(),
                    service_name.as_ptr(),
                    SERVICE_ALL_ACCESS,
                    SERVICE_KERNEL_DRIVER,
                    SERVICE_DEMAND_START, // Changed from SERVICE_SYSTEM_START to SERVICE_DEMAND_START
                    SERVICE_ERROR_NORMAL,
                    driver_path_cstr.as_ptr(),
                    ptr::null(),
                    ptr::null_mut(),
                    ptr::null(),
                    ptr::null(),
                    ptr::null(),
                );
                
                if service.is_null() {
                    let error = GetLastError();
                    CloseServiceHandle(sc_manager);
                    
                    if error == ERROR_SERVICE_EXISTS {
                        // Service already exists, try to start it
                        return self.start_existing_service(sc_manager);
                    } else {
                        return Err(format!("Failed to create service. Error: {}", error));
                    }
                }
                
                // Start the service
                let start_result = StartServiceA(service, 0, ptr::null_mut());
                let start_error = GetLastError();
                
                CloseServiceHandle(service);
                CloseServiceHandle(sc_manager);
                
                if start_result == 0 && start_error != ERROR_SERVICE_ALREADY_RUNNING {
                    return Err(format!("Failed to start service. Error: {}", start_error));
                }
                
                Ok(())
            }
        }
        
        fn start_existing_service(&self, sc_manager: SC_HANDLE) -> Result<(), String> {
            let service_name = CString::new(self.service_name.as_str()).unwrap();
            
            unsafe {
                let service = OpenServiceA(
                    sc_manager,
                    service_name.as_ptr(),
                    SERVICE_ALL_ACCESS,
                );
                
                if service.is_null() {
                    let error = GetLastError();
                    return Err(format!("Failed to open existing service. Error: {}", error));
                }
                
                let start_result = StartServiceA(service, 0, ptr::null_mut());
                let start_error = GetLastError();
                
                CloseServiceHandle(service);
                
                if start_result == 0 && start_error != ERROR_SERVICE_ALREADY_RUNNING {
                    return Err(format!("Failed to start existing service. Error: {}", start_error));
                }
                
                Ok(())
            }
        }
        
        pub fn delete_driver(&self) -> Result<(), String> {
            let service_name = CString::new(self.service_name.as_str()).unwrap();
            
            unsafe {
                let sc_manager = OpenSCManagerA(
                    ptr::null(),
                    ptr::null(),
                    SC_MANAGER_ALL_ACCESS,
                );
                
                if sc_manager.is_null() {
                    let error = GetLastError();
                    return Err(format!("Failed to open Service Control Manager. Error: {}", error));
                }
                
                let service = OpenServiceA(
                    sc_manager,
                    service_name.as_ptr(),
                    SERVICE_ALL_ACCESS,
                );
                
                if service.is_null() {
                    CloseServiceHandle(sc_manager);
                    // Service doesn't exist, that's fine
                    return Ok(());
                }
                
                // Try to stop the service first
                let mut service_status = SERVICE_STATUS {
                    dwServiceType: 0,
                    dwCurrentState: 0,
                    dwControlsAccepted: 0,
                    dwWin32ExitCode: 0,
                    dwServiceSpecificExitCode: 0,
                    dwCheckPoint: 0,
                    dwWaitHint: 0,
                };
                
                ControlService(service, SERVICE_CONTROL_STOP, &mut service_status);
                
                // Delete the service
                let delete_result = DeleteService(service);
                let delete_error = GetLastError();
                
                CloseServiceHandle(service);
                CloseServiceHandle(sc_manager);
                
                if delete_result == 0 {
                    return Err(format!("Failed to delete service. Error: {}", delete_error));
                }
                
                Ok(())
            }
        }
    }
}

struct EcProbe {
    driver_handle: HANDLE,
}

impl EcProbe {
    fn new() -> Result<Self, String> {
        let device_name = CString::new(WINRING0_DEVICE_NAME).unwrap();
        
        println!("Attempting to open WinRing0 driver: {}", WINRING0_DEVICE_NAME);
        
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
            println!("Driver not accessible (Error: {}). Attempting to load driver...", error);
            
            // Try to load the driver
            let driver_manager = driver_manager::DriverManager::new();
            
            if !driver_manager.is_driver_loaded() {
                println!("Driver not loaded. Installing and loading driver...");
                match driver_manager.install_and_load_driver() {
                    Ok(_) => {
                        println!("Driver loaded successfully. Retrying connection...");
                        // Wait a bit for the driver to be ready
                        thread::sleep(Duration::from_millis(1000));
                    }
                    Err(e) => {
                        return Err(format!("Failed to load driver: {}. Make sure you're running as administrator.", e));
                    }
                }
            } else {
                println!("Driver appears to be loaded but not accessible. This might be a permissions issue.");
            }
            
            // Try to open the driver again after loading
            let handle_retry = unsafe {
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
            
            if handle_retry == INVALID_HANDLE_VALUE {
                let error_retry = unsafe { GetLastError() };
                return Err(format!("Failed to open WinRing0 driver after loading attempt. Error code: {}. Make sure you're running as administrator.", error_retry));
            }
            
            println!("Successfully opened WinRing0 driver handle after loading: {:?}", handle_retry);
            Ok(EcProbe {
                driver_handle: handle_retry,
            })
        } else {
            println!("Successfully opened WinRing0 driver handle: {:?}", handle);
            Ok(EcProbe {
                driver_handle: handle,
            })
        }
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
            Err(format!("Failed to read IO port 0x{:X}. Error code: {}, bytes_returned: {}", port, error, bytes_returned))
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
            Err(format!("Failed to write IO port 0x{:X} value 0x{:02X}. Error code: {}, bytes_returned: {}", port, value, error, bytes_returned))
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

    pub fn read_byte(&self, register: u8) -> Result<u8, String> {
        for _ in 0..MAX_RETRIES {
            if let Ok(value) = self.try_read_byte(register) {
                return Ok(value);
            }
        }
        Err("Failed to read byte after retries".to_string())
    }

    pub fn write_byte(&self, register: u8, value: u8) -> Result<(), String> {
        for _ in 0..MAX_RETRIES {
            if self.try_write_byte(register, value).is_ok() {
                return Ok(());
            }
        }
        Err("Failed to write byte after retries".to_string())
    }

    pub fn dump_registers(&self) {
        println!("   | 00 01 02 03 04 05 06 07 08 09 0A 0B 0C 0D 0E 0F");
        println!("---|------------------------------------------------");

        for i in (0..=0xF0).step_by(0x10) {
            print!("{:02X} | ", i);
            
            for j in 0..=0xF {
                let register = (i + j) as u8;
                match self.read_byte(register) {
                    Ok(value) => {
                        if value == 0 {
                            print!("\x1b[90m{:02X}\x1b[0m ", value); // Dark gray for 0
                        } else if value == 0xFF {
                            print!("\x1b[32m{:02X}\x1b[0m ", value); // Green for 0xFF
                        } else {
                            print!("\x1b[31m{:02X}\x1b[0m ", value); // Red for other values
                        }
                    }
                    Err(_) => print!("?? "),
                }
            }
            println!();
        }
    }
}

impl Drop for EcProbe {
    fn drop(&mut self) {
        if self.driver_handle != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.driver_handle);
            }
        }
    }
}

fn main() {
    println!("EC Probe - Rust Implementation");
    println!("==============================");

    let ec = match EcProbe::new() {
        Ok(ec) => ec,
        Err(e) => {
            eprintln!("Error: {}", e);
            eprintln!("Make sure:");
            eprintln!("1. You're running as Administrator");
            eprintln!("2. WinRing0x64.sys driver is installed");
            eprintln!("3. The driver is loaded and accessible");
            return;
        }
    };

    println!("\n=== EC Register Dump ===");
    ec.dump_registers();

    // Example: Read a specific register
    let test_register = 0x00;
    println!("\n=== Reading Register 0x{:02X} ===", test_register);
    match ec.read_byte(test_register) {
        Ok(value) => println!("Register 0x{:02X}: {} (0x{:02X})", test_register, value, value),
        Err(e) => println!("Failed to read register 0x{:02X}: {}", test_register, e),
    }

    // Example: Write to a register (DANGEROUS - commented out for safety)
    /*
    let test_register = 0x01;
    let test_value = 0x42;
    println!("\n=== Writing to Register 0x{:02X} ===", test_register);
    match ec.write_byte(test_register, test_value) {
        Ok(()) => {
            println!("Successfully wrote {} (0x{:02X}) to register 0x{:02X}", test_value, test_value, test_register);
            // Read back to verify
            match ec.read_byte(test_register) {
                Ok(value) => println!("Verification read: {} (0x{:02X})", value, value),
                Err(e) => println!("Failed to verify write: {}", e),
            }
        }
        Err(e) => println!("Failed to write to register 0x{:02X}: {}", test_register, e),
    }
    */

    println!("\nDone!");
}