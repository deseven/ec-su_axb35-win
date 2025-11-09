use std::ptr;
use std::ffi::CString;
use std::path::Path;
use std::fs;
use std::thread;
use std::time::Duration;
use winapi::um::winnt::{GENERIC_READ, GENERIC_WRITE, SERVICE_KERNEL_DRIVER, SERVICE_DEMAND_START, SERVICE_ERROR_NORMAL};
use winapi::um::fileapi::{CreateFileA, OPEN_EXISTING};
use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
use winapi::um::winsvc::*;
use winapi::um::errhandlingapi::GetLastError;
use winapi::shared::winerror::*;

// WinRing0 driver constants
const WINRING0_DEVICE_NAME: &str = "\\\\.\\WinRing0_1_2_0";
const DRIVER_SERVICE_NAME: &str = "WinRing0_1_2_0";

pub struct DriverManager {
    driver_path: String,
}

impl DriverManager {
    pub fn new(driver_path: &str) -> Self {
        DriverManager {
            driver_path: driver_path.to_string(),
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

        let driver_file_path = format!("{}\\{}", self.driver_path, driver_filename);

        if !Path::new(&driver_file_path).exists() {
            return Err(format!("Driver file not found: {}", driver_file_path));
        }

        // Get absolute path
        let absolute_path = match fs::canonicalize(&driver_file_path) {
            Ok(path) => path.to_string_lossy().to_string(),
            Err(e) => return Err(format!("Failed to get absolute path: {}", e)),
        };

        // Try to install the driver
        match self.install_driver(&absolute_path) {
            Ok(_) => {
                // Give the system a moment to register the driver
                thread::sleep(Duration::from_millis(500));
                Ok(())
            }
            Err(_e) => {
                // If installation failed, try to delete and reinstall
                let _ = self.delete_driver(); // Ignore errors here
                thread::sleep(Duration::from_millis(2000)); // Wait for cleanup

                match self.install_driver(&absolute_path) {
                    Ok(_) => {
                        thread::sleep(Duration::from_millis(500));
                        Ok(())
                    }
                    Err(e2) => Err(format!("Failed to install driver after retry: {}", e2))
                }
            }
        }
    }

    fn install_driver(&self, driver_path: &str) -> Result<(), String> {
        let service_name = CString::new(DRIVER_SERVICE_NAME).unwrap();
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
                SERVICE_DEMAND_START,
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
        let service_name = CString::new(DRIVER_SERVICE_NAME).unwrap();

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
        let service_name = CString::new(DRIVER_SERVICE_NAME).unwrap();

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