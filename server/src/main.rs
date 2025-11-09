
use std::sync::{Arc, Mutex};
use std::ptr;
use std::ffi::CString;
use std::time::Duration;
use std::sync::atomic::{AtomicBool, Ordering};

use winapi::um::winnt::TOKEN_ELEVATION;
use winapi::um::processthreadsapi::{GetCurrentProcess, OpenProcessToken};
use winapi::um::securitybaseapi::GetTokenInformation;
use winapi::um::winuser::MessageBoxA;
use winapi::um::handleapi::CloseHandle;
use winapi::um::processenv::GetStdHandle;
use winapi::um::winbase::STD_OUTPUT_HANDLE;
use winapi::um::consoleapi::GetConsoleMode;

use windows_service::{
    define_windows_service,
    service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    },
    service_control_handler::{self, ServiceControlHandlerResult},
    service_dispatcher,
};

use tokio::sync::mpsc;
use warp::Filter;
use serde::{Deserialize, Serialize};
use clap::Parser;

mod ec;
mod config;
mod logger;
mod driver;

use ec::{EcController, EcOperation, EcResult};
use config::ServerConfig;
use logger::Logger;
use driver::DriverManager;

#[derive(Parser, Debug)]
#[command(name = "ec-su_axb35-server")]
#[command(about = "EC Server for SU AXB35 devices")]
struct Args {
    /// Run in service mode (suppress GUI dialogs and stdout output)
    #[arg(long)]
    service: bool,
}

const SERVICE_NAME: &str = "EC-SU-AXB35-Server";
const SERVICE_TYPE: ServiceType = ServiceType::OWN_PROCESS;

// Global shutdown signal for the service
static SHUTDOWN_SIGNAL: AtomicBool = AtomicBool::new(false);

// Service status handle wrapped in a mutex for thread safety
use std::sync::Mutex as StdMutex;
use std::sync::OnceLock;
static SERVICE_STATUS_HANDLE: OnceLock<StdMutex<Option<service_control_handler::ServiceStatusHandle>>> = OnceLock::new();

#[derive(Debug, Serialize, Deserialize)]
struct StatusResponse {
    status: u8,
    version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PowerModeResponse {
    power_mode: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct PowerModeRequest {
    power_mode: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TemperatureResponse {
    temperature: u8,
}

#[derive(Debug, Serialize, Deserialize)]
struct FanRpmResponse {
    rpm: u16,
}

#[derive(Debug, Serialize, Deserialize)]
struct FanModeResponse {
    mode: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct FanModeRequest {
    mode: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct FanLevelResponse {
    level: u8,
}

#[derive(Debug, Serialize, Deserialize)]
struct FanLevelRequest {
    level: u8,
}

#[derive(Debug, Serialize, Deserialize)]
struct FanCurveResponse {
    curve: [u8; 5],
}

#[derive(Debug, Serialize, Deserialize)]
struct FanCurveRequest {
    curve: [u8; 5],
}

#[derive(Debug, Serialize, Deserialize)]
struct MetricsResponse {
    power_mode: String,
    temperature: u8,
    fan1: FanMetrics,
    fan2: FanMetrics,
    fan3: FanMetrics,
}

#[derive(Debug, Serialize, Deserialize)]
struct FanMetrics {
    mode: String,
    level: u8,
    rpm: u16,
    rampup_curve: [u8; 5],
    rampdown_curve: [u8; 5],
}

#[derive(Debug, Serialize, Deserialize)]
struct ErrorResponse {
    error: String,
}

// Check if running as administrator
fn is_admin() -> bool {
    unsafe {
        let mut token = ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), winapi::um::winnt::TOKEN_QUERY, &mut token) == 0 {
            return false;
        }

        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut size = std::mem::size_of::<TOKEN_ELEVATION>() as u32;

        let result = GetTokenInformation(
            token,
            winapi::um::winnt::TokenElevation,
            &mut elevation as *mut _ as *mut _,
            size,
            &mut size,
        );

        CloseHandle(token);

        if result == 0 {
            return false;
        }

        elevation.TokenIsElevated != 0
    }
}

// Check if we have an active TTY/console
fn has_console() -> bool {
    unsafe {
        let stdout_handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if stdout_handle.is_null() {
            return false;
        }
        
        let mut console_mode = 0u32;
        GetConsoleMode(stdout_handle, &mut console_mode) != 0
    }
}

fn show_error_and_exit(message: &str, service_mode: bool) -> ! {
    // Always log to stderr for service logs
    eprintln!("Error: {}", message);
    
    // Only show GUI dialog if not in service mode and we have a console
    if !service_mode && has_console() {
        unsafe {
            let title = CString::new("EC Server Error").unwrap();
            let msg = CString::new(message).unwrap();
            MessageBoxA(
                ptr::null_mut(),
                msg.as_ptr(),
                title.as_ptr(),
                0x10, // MB_ICONERROR
            );
        }
    }
    
    std::process::exit(1);
}

// Define the Windows service entry point
define_windows_service!(ffi_service_main, my_service_main);

// Service main function
fn my_service_main(_arguments: Vec<std::ffi::OsString>) {
    if let Err(_e) = run_service() {
        // Log error to Windows Event Log if possible
    }
}

// Service control handler
fn service_control_handler(control_event: ServiceControl) -> ServiceControlHandlerResult {
    match control_event {
        ServiceControl::Stop => {
            // Log the service stop event to stderr for service logs
            eprintln!("Service stop requested - shutting down server");
            
            // Signal the service to stop
            SHUTDOWN_SIGNAL.store(true, Ordering::SeqCst);
            
            // Report that we're stopping
            if let Some(status_handle_mutex) = SERVICE_STATUS_HANDLE.get() {
                if let Ok(status_handle_guard) = status_handle_mutex.lock() {
                    if let Some(ref status_handle) = *status_handle_guard {
                        let _ = status_handle.set_service_status(ServiceStatus {
                            service_type: SERVICE_TYPE,
                            current_state: ServiceState::StopPending,
                            controls_accepted: ServiceControlAccept::empty(),
                            exit_code: ServiceExitCode::Win32(0),
                            checkpoint: 0,
                            wait_hint: Duration::from_secs(30),
                            process_id: None,
                        });
                    }
                }
            }
            
            ServiceControlHandlerResult::NoError
        }
        ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
        _ => ServiceControlHandlerResult::NotImplemented,
    }
}

// Run the service
fn run_service() -> windows_service::Result<()> {
    // Initialize the global status handle storage
    SERVICE_STATUS_HANDLE.set(StdMutex::new(None)).map_err(|_| {
        windows_service::Error::Winapi(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to initialize service status handle storage"
        ))
    })?;

    // Register service control handler
    let event_handler = move |control_event| -> ServiceControlHandlerResult {
        service_control_handler(control_event)
    };

    let status_handle = service_control_handler::register(SERVICE_NAME, event_handler)?;
    
    // Store the status handle globally
    if let Some(status_handle_mutex) = SERVICE_STATUS_HANDLE.get() {
        if let Ok(mut status_handle_guard) = status_handle_mutex.lock() {
            *status_handle_guard = Some(status_handle.clone());
        }
    }

    // Tell the system that service is running
    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Running,
        controls_accepted: ServiceControlAccept::STOP,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;

    // Create a new Tokio runtime for the service
    let rt = tokio::runtime::Runtime::new().unwrap();
    
    // Create a shutdown channel for graceful shutdown
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    
    // Spawn a task to monitor the shutdown signal
    let shutdown_monitor = rt.spawn(async move {
        loop {
            if SHUTDOWN_SIGNAL.load(Ordering::SeqCst) {
                let _ = shutdown_tx.send(());
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    
    // Run the server with shutdown signal
    rt.block_on(async {
        tokio::select! {
            _ = run_server_with_shutdown(true, shutdown_rx) => {},
            _ = shutdown_monitor => {},
        }
    });

    // Log service shutdown completion
    eprintln!("Service shutdown completed");
    
    // Tell the system that service has stopped
    status_handle.set_service_status(ServiceStatus {
        service_type: SERVICE_TYPE,
        current_state: ServiceState::Stopped,
        controls_accepted: ServiceControlAccept::empty(),
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::default(),
        process_id: None,
    })?;
    
    // Clear the global status handle
    if let Some(status_handle_mutex) = SERVICE_STATUS_HANDLE.get() {
        if let Ok(mut status_handle_guard) = status_handle_mutex.lock() {
            *status_handle_guard = None;
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() {
    // Parse command line arguments
    let args = Args::parse();
    
    // Check if we're being started by the Service Control Manager
    if args.service || !has_console() {
        // We're running as a service
        if let Err(e) = service_dispatcher::start(SERVICE_NAME, ffi_service_main) {
            eprintln!("Failed to start service: {}", e);
        }
        return;
    }
    
    // We're running in console mode - set up Ctrl+C handler
    let shutdown_signal = Arc::new(AtomicBool::new(false));
    let shutdown_signal_clone = shutdown_signal.clone();
    
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        eprintln!("User interrupt received (Ctrl+C) - shutting down server");
        shutdown_signal_clone.store(true, Ordering::SeqCst);
    });
    
    run_server_console(false, shutdown_signal).await;
}


async fn run_server_console(service_mode: bool, shutdown_signal: Arc<AtomicBool>) {
    // Create a shutdown channel that triggers when the signal is set
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    
    // Spawn a task to monitor the shutdown signal
    tokio::spawn(async move {
        loop {
            if shutdown_signal.load(Ordering::SeqCst) {
                let _ = shutdown_tx.send(());
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    });
    
    run_server_with_shutdown(service_mode, shutdown_rx).await;
}

async fn run_server_with_shutdown(service_mode: bool, shutdown_rx: tokio::sync::oneshot::Receiver<()>) {

    // Check admin privileges first
    if !is_admin() {
        show_error_and_exit("This application must be run as Administrator to access the EC driver.", service_mode);
    }

    // Load configuration
    let config = match ServerConfig::load() {
        Ok(config) => Arc::new(Mutex::new(config)),
        Err(e) => {
            show_error_and_exit(&format!("Failed to load configuration: {}", e), service_mode);
        }
    };

    // Initialize logger
    let logger = {
        let config_guard = config.lock().unwrap();
        match Logger::new(&config_guard.log_path, service_mode) {
            Ok(logger) => Arc::new(Mutex::new(logger)),
            Err(e) => {
                show_error_and_exit(&format!("Failed to initialize logger: {}", e), service_mode);
            }
        }
    };

    // Log startup
    {
        let mut log = logger.lock().unwrap();
        log.info("EC Server starting up...");
        let config_guard = config.lock().unwrap();
        log.info(&format!("Listening on {}:{}", config_guard.host, config_guard.port));
    }

    // Initialize driver manager
    let driver_manager = {
        let config_guard = config.lock().unwrap();
        DriverManager::new(&config_guard.driver_path)
    };
    
    // Check if driver is loaded or try to load it
    if !driver_manager.is_driver_loaded() {
        {
            let mut log = logger.lock().unwrap();
            log.info("WinRing0 driver not loaded, attempting to load...");
        }
        
        if let Err(e) = driver_manager.install_and_load_driver() {
            let error_msg = format!("Failed to load WinRing0 driver: {}. Make sure the driver files are in the correct location.", e);
            {
                let mut log = logger.lock().unwrap();
                log.error(&error_msg);
            }
            show_error_and_exit(&error_msg, service_mode);
        }
        
        {
            let mut log = logger.lock().unwrap();
            log.info("WinRing0 driver loaded successfully");
        }
    } else {
        let mut log = logger.lock().unwrap();
        log.info("WinRing0 driver already loaded");
    }

    // Initialize EC controller
    let ec_controller = match EcController::new() {
        Ok(controller) => Arc::new(controller),
        Err(e) => {
            let error_msg = format!("Failed to initialize EC controller: {}", e);
            {
                let mut log = logger.lock().unwrap();
                log.error(&error_msg);
            }
            show_error_and_exit(&error_msg, service_mode);
        }
    };

    {
        let mut log = logger.lock().unwrap();
        log.info("EC controller initialized successfully");
    }

    // Restore saved parameters from config
    {
        let mut log = logger.lock().unwrap();
        log.info("Restoring saved parameters from configuration...");
    }
    
    let config_guard = config.lock().unwrap();
    
    // Restore APU power mode if saved
    if let Some(ref power_mode) = config_guard.apu_power_mode {
        if ec_controller.execute_operation(EcOperation::SetApuPowerMode(power_mode.clone())).await.is_ok() {
            let mut log = logger.lock().unwrap();
            log.info(&format!("Restored APU power mode: {}", power_mode));
        }
    }
    
    // Restore fan configurations
    let fan_configs = [&config_guard.fan1, &config_guard.fan2, &config_guard.fan3];
    for (fan_id, fan_config_opt) in fan_configs.iter().enumerate() {
        let fan_id = (fan_id + 1) as u8;
        
        if let Some(fan_config) = fan_config_opt {
            // Restore fan mode
            if ec_controller.execute_operation(EcOperation::SetFanMode(fan_id, fan_config.mode.clone())).await.is_ok() {
                let mut log = logger.lock().unwrap();
                log.info(&format!("Restored Fan{} mode: {}", fan_id, fan_config.mode));
            }
            
            // Restore fan level if not in auto mode
            if fan_config.mode != "auto" {
                if ec_controller.execute_operation(EcOperation::SetFanLevel(fan_id, fan_config.level)).await.is_ok() {
                    let mut log = logger.lock().unwrap();
                    log.info(&format!("Restored Fan{} level: {}", fan_id, fan_config.level));
                }
            }
            
            // Restore fan curves
            if ec_controller.execute_operation(EcOperation::SetFanRampupCurve(fan_id, fan_config.rampup_curve)).await.is_ok() {
                let mut log = logger.lock().unwrap();
                log.info(&format!("Restored Fan{} rampup curve: {:?}", fan_id, fan_config.rampup_curve));
            }
            
            if ec_controller.execute_operation(EcOperation::SetFanRampdownCurve(fan_id, fan_config.rampdown_curve)).await.is_ok() {
                let mut log = logger.lock().unwrap();
                log.info(&format!("Restored Fan{} rampdown curve: {:?}", fan_id, fan_config.rampdown_curve));
            }
        } else {
            let mut log = logger.lock().unwrap();
            log.info(&format!("Fan{} configuration not found in config, leaving in original state", fan_id));
        }
    }
    
    drop(config_guard);
    
    {
        let mut log = logger.lock().unwrap();
        log.info("Parameter restoration completed");
    }

    // Create EC operation queue
    let (tx, mut rx) = mpsc::unbounded_channel::<(EcOperation, tokio::sync::oneshot::Sender<Result<EcResult, String>>)>();
    let ec_queue = Arc::new(tx);

    // Spawn EC operation handler task
    let ec_controller_clone = ec_controller.clone();
    let logger_clone = logger.clone();
    tokio::spawn(async move {
        while let Some((operation, response_tx)) = rx.recv().await {
            let result = ec_controller_clone.execute_operation(operation).await;
            
            // Log the operation
            {
                let mut log = logger_clone.lock().unwrap();
                match &result {
                    Ok(_) => log.debug("EC operation completed successfully"),
                    Err(e) => log.warn(&format!("EC operation failed: {}", e)),
                }
            }
            
            let _ = response_tx.send(result);
        }
    });

    // Spawn curve monitoring task
    let ec_controller_curve = ec_controller.clone();
    let logger_curve = logger.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(1));
        let mut curve_monitoring_active = false;
        
        loop {
            interval.tick().await;
            
            let has_curve_fans = ec_controller_curve.has_curve_fans();
            
            // Log when curve monitoring starts or stops
            if has_curve_fans && !curve_monitoring_active {
                let mut log = logger_curve.lock().unwrap();
                log.info("Curve monitoring started - fans in curve mode detected");
                curve_monitoring_active = true;
            } else if !has_curve_fans && curve_monitoring_active {
                let mut log = logger_curve.lock().unwrap();
                log.info("Curve monitoring stopped - no fans in curve mode");
                curve_monitoring_active = false;
            }
            
            // Only run curve logic if any fans are in curve mode
            if has_curve_fans {
                match ec_controller_curve.update_curve_fans() {
                    Ok(log_messages) => {
                        if !log_messages.is_empty() {
                            let mut log = logger_curve.lock().unwrap();
                            for message in log_messages {
                                log.info(&message);
                            }
                        }
                    }
                    Err(e) => {
                        let mut log = logger_curve.lock().unwrap();
                        log.warn(&format!("Curve monitoring error: {}", e));
                    }
                }
            }
        }
    });

    // Create routes
    let logger_clone_for_filter = logger.clone();
    let logger_filter = warp::any().map(move || logger_clone_for_filter.clone());
    let ec_queue_filter = warp::any().map(move || ec_queue.clone());
    let config_clone_for_filter = config.clone();
    let config_filter = warp::any().map(move || config_clone_for_filter.clone());

    // GET /status
    let status_route = warp::path("status")
        .and(warp::get())
        .and(logger_filter.clone())
        .and(ec_queue_filter.clone())
        .and_then(handle_status);

    // GET /metrics
    let metrics_route = warp::path("metrics")
        .and(warp::get())
        .and(logger_filter.clone())
        .and(ec_queue_filter.clone())
        .and_then(handle_metrics);

    // GET/POST /apu/power_mode
    let apu_power_mode_get = warp::path!("apu" / "power_mode")
        .and(warp::get())
        .and(logger_filter.clone())
        .and(ec_queue_filter.clone())
        .and_then(handle_apu_power_mode_get);

    let apu_power_mode_post = warp::path!("apu" / "power_mode")
        .and(warp::post())
        .and(warp::body::json())
        .and(logger_filter.clone())
        .and(ec_queue_filter.clone())
        .and(config_filter.clone())
        .and_then(handle_apu_power_mode_post);

    // GET /apu/temp
    let apu_temp_route = warp::path!("apu" / "temp")
        .and(warp::get())
        .and(logger_filter.clone())
        .and(ec_queue_filter.clone())
        .and_then(handle_apu_temp);

    // Fan routes (fan1, fan2, fan3)
    let fan_rpm_routes = warp::path!("fan1" / "rpm")
        .and(warp::get())
        .and(warp::any().map(|| 1u8))
        .and(logger_filter.clone())
        .and(ec_queue_filter.clone())
        .and_then(handle_fan_rpm)
        .or(warp::path!("fan2" / "rpm")
            .and(warp::get())
            .and(warp::any().map(|| 2u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and_then(handle_fan_rpm))
        .or(warp::path!("fan3" / "rpm")
            .and(warp::get())
            .and(warp::any().map(|| 3u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and_then(handle_fan_rpm));

    let fan_mode_get_routes = warp::path!("fan1" / "mode")
        .and(warp::get())
        .and(warp::any().map(|| 1u8))
        .and(logger_filter.clone())
        .and(ec_queue_filter.clone())
        .and_then(handle_fan_mode_get)
        .or(warp::path!("fan2" / "mode")
            .and(warp::get())
            .and(warp::any().map(|| 2u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and_then(handle_fan_mode_get))
        .or(warp::path!("fan3" / "mode")
            .and(warp::get())
            .and(warp::any().map(|| 3u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and_then(handle_fan_mode_get));

    let fan_mode_post_routes = warp::path!("fan1" / "mode")
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::any().map(|| 1u8))
        .and(logger_filter.clone())
        .and(ec_queue_filter.clone())
        .and(config_filter.clone())
        .and_then(handle_fan_mode_post)
        .or(warp::path!("fan2" / "mode")
            .and(warp::post())
            .and(warp::body::json())
            .and(warp::any().map(|| 2u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and(config_filter.clone())
            .and_then(handle_fan_mode_post))
        .or(warp::path!("fan3" / "mode")
            .and(warp::post())
            .and(warp::body::json())
            .and(warp::any().map(|| 3u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and(config_filter.clone())
            .and_then(handle_fan_mode_post));

    let fan_level_get_routes = warp::path!("fan1" / "level")
        .and(warp::get())
        .and(warp::any().map(|| 1u8))
        .and(logger_filter.clone())
        .and(ec_queue_filter.clone())
        .and_then(handle_fan_level_get)
        .or(warp::path!("fan2" / "level")
            .and(warp::get())
            .and(warp::any().map(|| 2u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and_then(handle_fan_level_get))
        .or(warp::path!("fan3" / "level")
            .and(warp::get())
            .and(warp::any().map(|| 3u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and_then(handle_fan_level_get));

    let fan_level_post_routes = warp::path!("fan1" / "level")
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::any().map(|| 1u8))
        .and(logger_filter.clone())
        .and(ec_queue_filter.clone())
        .and(config_filter.clone())
        .and_then(handle_fan_level_post)
        .or(warp::path!("fan2" / "level")
            .and(warp::post())
            .and(warp::body::json())
            .and(warp::any().map(|| 2u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and(config_filter.clone())
            .and_then(handle_fan_level_post))
        .or(warp::path!("fan3" / "level")
            .and(warp::post())
            .and(warp::body::json())
            .and(warp::any().map(|| 3u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and(config_filter.clone())
            .and_then(handle_fan_level_post));

    // Fan curve routes
    let fan_rampup_curve_get_routes = warp::path!("fan1" / "rampup_curve")
        .and(warp::get())
        .and(warp::any().map(|| 1u8))
        .and(logger_filter.clone())
        .and(ec_queue_filter.clone())
        .and_then(handle_fan_rampup_curve_get)
        .or(warp::path!("fan2" / "rampup_curve")
            .and(warp::get())
            .and(warp::any().map(|| 2u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and_then(handle_fan_rampup_curve_get))
        .or(warp::path!("fan3" / "rampup_curve")
            .and(warp::get())
            .and(warp::any().map(|| 3u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and_then(handle_fan_rampup_curve_get));

    let fan_rampup_curve_post_routes = warp::path!("fan1" / "rampup_curve")
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::any().map(|| 1u8))
        .and(logger_filter.clone())
        .and(ec_queue_filter.clone())
        .and(config_filter.clone())
        .and_then(handle_fan_rampup_curve_post)
        .or(warp::path!("fan2" / "rampup_curve")
            .and(warp::post())
            .and(warp::body::json())
            .and(warp::any().map(|| 2u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and(config_filter.clone())
            .and_then(handle_fan_rampup_curve_post))
        .or(warp::path!("fan3" / "rampup_curve")
            .and(warp::post())
            .and(warp::body::json())
            .and(warp::any().map(|| 3u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and(config_filter.clone())
            .and_then(handle_fan_rampup_curve_post));

    let fan_rampdown_curve_get_routes = warp::path!("fan1" / "rampdown_curve")
        .and(warp::get())
        .and(warp::any().map(|| 1u8))
        .and(logger_filter.clone())
        .and(ec_queue_filter.clone())
        .and_then(handle_fan_rampdown_curve_get)
        .or(warp::path!("fan2" / "rampdown_curve")
            .and(warp::get())
            .and(warp::any().map(|| 2u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and_then(handle_fan_rampdown_curve_get))
        .or(warp::path!("fan3" / "rampdown_curve")
            .and(warp::get())
            .and(warp::any().map(|| 3u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and_then(handle_fan_rampdown_curve_get));

    let fan_rampdown_curve_post_routes = warp::path!("fan1" / "rampdown_curve")
        .and(warp::post())
        .and(warp::body::json())
        .and(warp::any().map(|| 1u8))
        .and(logger_filter.clone())
        .and(ec_queue_filter.clone())
        .and(config_filter.clone())
        .and_then(handle_fan_rampdown_curve_post)
        .or(warp::path!("fan2" / "rampdown_curve")
            .and(warp::post())
            .and(warp::body::json())
            .and(warp::any().map(|| 2u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and(config_filter.clone())
            .and_then(handle_fan_rampdown_curve_post))
        .or(warp::path!("fan3" / "rampdown_curve")
            .and(warp::post())
            .and(warp::body::json())
            .and(warp::any().map(|| 3u8))
            .and(logger_filter.clone())
            .and(ec_queue_filter.clone())
            .and(config_filter.clone())
            .and_then(handle_fan_rampdown_curve_post));

    // Combine all routes
    let routes = status_route
        .or(metrics_route)
        .or(apu_power_mode_get)
        .or(apu_power_mode_post)
        .or(apu_temp_route)
        .or(fan_rpm_routes)
        .or(fan_mode_get_routes)
        .or(fan_mode_post_routes)
        .or(fan_level_get_routes)
        .or(fan_level_post_routes)
        .or(fan_rampup_curve_get_routes)
        .or(fan_rampup_curve_post_routes)
        .or(fan_rampdown_curve_get_routes)
        .or(fan_rampdown_curve_post_routes)
        .with(warp::cors().allow_any_origin().allow_headers(vec!["content-type"]).allow_methods(vec!["GET", "POST"]));

    {
        let mut log = logger.lock().unwrap();
        log.info("Server started successfully");
    }

    // Parse host address
    let (host_addr, port) = {
        let config_guard = config.lock().unwrap();
        let host_addr: std::net::IpAddr = config_guard.host.parse()
            .unwrap_or_else(|_| {
                let error_msg = format!("Invalid host address in config: {}", config_guard.host);
                {
                    let mut log = logger.lock().unwrap();
                    log.error(&error_msg);
                }
                show_error_and_exit(&error_msg, service_mode);
            });
        (host_addr, config_guard.port)
    };

    // Start server with graceful shutdown
    let server_result = warp::serve(routes)
        .try_bind_with_graceful_shutdown((host_addr, port), async move {
            shutdown_rx.await.ok();
        });
    
    let (_addr, server) = match server_result {
        Ok(server) => server,
        Err(e) => {
            let error_msg = format!("Failed to bind to {}:{} - {}", host_addr, port, e);
            {
                let mut log = logger.lock().unwrap();
                log.error(&error_msg);
            }
            eprintln!("Error: {}", error_msg);
            std::process::exit(1);
        }
    };
    
    server.await;
    
    // Log shutdown
    {
        let mut log = logger.lock().unwrap();
        log.info("Server shutdown completed");
    }
}

// Handler functions
async fn handle_status(
    logger: Arc<Mutex<Logger>>,
    ec_queue: Arc<mpsc::UnboundedSender<(EcOperation, tokio::sync::oneshot::Sender<std::result::Result<EcResult, String>>)>>,
) -> std::result::Result<impl warp::Reply, warp::Rejection> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    if ec_queue.send((EcOperation::GetFirmwareVersion, tx)).is_err() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "EC queue unavailable".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    match rx.await {
        Ok(Ok(EcResult::FirmwareVersion { major, minor })) => {
            let version = if minor < 10 {
                format!("{}.0{}", major, minor)
            } else {
                format!("{}.{}", major, minor)
            };
            
            {
                let mut log = logger.lock().unwrap();
                log.info(&format!("Status check: EC firmware version {}", version));
            }
            
            Ok(warp::reply::with_status(
                warp::reply::json(&StatusResponse {
                    status: 1,
                    version: Some(version),
                }),
                warp::http::StatusCode::OK,
            ))
        }
        Ok(Err(e)) => {
            {
                let mut log = logger.lock().unwrap();
                log.warn(&format!("Status check failed: {}", e));
            }
            
            Ok(warp::reply::with_status(
                warp::reply::json(&StatusResponse {
                    status: 0,
                    version: None,
                }),
                warp::http::StatusCode::OK,
            ))
        }
        Ok(Ok(_)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Unexpected response type".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Err(_) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Communication timeout".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}

async fn handle_apu_power_mode_get(
    logger: Arc<Mutex<Logger>>,
    ec_queue: Arc<mpsc::UnboundedSender<(EcOperation, tokio::sync::oneshot::Sender<std::result::Result<EcResult, String>>)>>,
) -> std::result::Result<impl warp::Reply, warp::Rejection> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    if ec_queue.send((EcOperation::GetApuPowerMode, tx)).is_err() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "EC queue unavailable".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    match rx.await {
        Ok(Ok(EcResult::ApuPowerMode(mode))) => {
            {
                let mut log = logger.lock().unwrap();
                log.info(&format!("APU power mode get: {}", mode));
            }
            
            Ok(warp::reply::with_status(
                warp::reply::json(&PowerModeResponse { power_mode: mode }),
                warp::http::StatusCode::OK,
            ))
        }
        Ok(Err(e)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: e }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Ok(Ok(_)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Unexpected response type".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Err(_) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Communication timeout".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}

async fn handle_apu_power_mode_post(
    request: PowerModeRequest,
    logger: Arc<Mutex<Logger>>,
    ec_queue: Arc<mpsc::UnboundedSender<(EcOperation, tokio::sync::oneshot::Sender<std::result::Result<EcResult, String>>)>>,
    config: Arc<Mutex<ServerConfig>>,
) -> std::result::Result<impl warp::Reply, warp::Rejection> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    if ec_queue.send((EcOperation::SetApuPowerMode(request.power_mode.clone()), tx)).is_err() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "EC queue unavailable".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    match rx.await {
        Ok(Ok(EcResult::ApuPowerMode(mode))) => {
            {
                let mut log = logger.lock().unwrap();
                log.info(&format!("APU power mode set to: {}", mode));
            }
            
            // Save to config
            {
                let mut config_guard = config.lock().unwrap();
                config_guard.apu_power_mode = Some(mode.clone());
                if let Err(e) = config_guard.save() {
                    let mut log = logger.lock().unwrap();
                    log.warn(&format!("Failed to save APU power mode to config: {}", e));
                }
            }
            
            Ok(warp::reply::with_status(
                warp::reply::json(&PowerModeResponse { power_mode: mode }),
                warp::http::StatusCode::OK,
            ))
        }
        Ok(Err(e)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: e }),
            warp::http::StatusCode::BAD_REQUEST,
        )),
        Ok(Ok(_)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Unexpected response type".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Err(_) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Communication timeout".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}

async fn handle_apu_temp(
    logger: Arc<Mutex<Logger>>,
    ec_queue: Arc<mpsc::UnboundedSender<(EcOperation, tokio::sync::oneshot::Sender<std::result::Result<EcResult, String>>)>>,
) -> std::result::Result<impl warp::Reply, warp::Rejection> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    if ec_queue.send((EcOperation::GetApuTemperature, tx)).is_err() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "EC queue unavailable".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    match rx.await {
        Ok(Ok(EcResult::ApuTemperature(temp))) => {
            {
                let mut log = logger.lock().unwrap();
                log.info(&format!("APU temperature: {}°C", temp));
            }
            
            Ok(warp::reply::with_status(
                warp::reply::json(&TemperatureResponse { temperature: temp }),
                warp::http::StatusCode::OK,
            ))
        }
        Ok(Err(e)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: e }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Ok(Ok(_)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Unexpected response type".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Err(_) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Communication timeout".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}

async fn handle_metrics(
    logger: Arc<Mutex<Logger>>,
    ec_queue: Arc<mpsc::UnboundedSender<(EcOperation, tokio::sync::oneshot::Sender<std::result::Result<EcResult, String>>)>>,
) -> std::result::Result<impl warp::Reply, warp::Rejection> {
    {
        let mut log = logger.lock().unwrap();
        log.info("Metrics request received");
    }

    // Helper function to execute EC operation
    let execute_operation = |operation: EcOperation| async {
        let (tx, rx) = tokio::sync::oneshot::channel();
        if ec_queue.send((operation, tx)).is_err() {
            return Err("EC queue unavailable".to_string());
        }
        match rx.await {
            Ok(result) => result,
            Err(_) => Err("Communication timeout".to_string()),
        }
    };

    // Get power mode
    let power_mode = match execute_operation(EcOperation::GetApuPowerMode).await {
        Ok(EcResult::ApuPowerMode(mode)) => mode,
        Ok(_) => return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: "Unexpected response type for power mode".to_string() }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Err(e) => return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: format!("Failed to get power mode: {}", e) }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    };

    // Get temperature
    let temperature = match execute_operation(EcOperation::GetApuTemperature).await {
        Ok(EcResult::ApuTemperature(temp)) => temp,
        Ok(_) => return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: "Unexpected response type for temperature".to_string() }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Err(e) => return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: format!("Failed to get temperature: {}", e) }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    };

    // Helper function to get fan metrics
    let get_fan_metrics = |fan_id: u8| async move {
        // Get fan mode
        let mode = match execute_operation(EcOperation::GetFanMode(fan_id)).await {
            Ok(EcResult::FanMode(mode)) => mode,
            _ => return Err(format!("Failed to get Fan{} mode", fan_id)),
        };

        // Get fan level
        let level = match execute_operation(EcOperation::GetFanLevel(fan_id)).await {
            Ok(EcResult::FanLevel(level)) => level,
            _ => return Err(format!("Failed to get Fan{} level", fan_id)),
        };

        // Get fan RPM
        let rpm = match execute_operation(EcOperation::GetFanRpm(fan_id)).await {
            Ok(EcResult::FanRpm(rpm)) => rpm,
            _ => return Err(format!("Failed to get Fan{} RPM", fan_id)),
        };

        // Get rampup curve
        let rampup_curve = match execute_operation(EcOperation::GetFanRampupCurve(fan_id)).await {
            Ok(EcResult::FanRampupCurve(curve)) => curve,
            _ => return Err(format!("Failed to get Fan{} rampup curve", fan_id)),
        };

        // Get rampdown curve
        let rampdown_curve = match execute_operation(EcOperation::GetFanRampdownCurve(fan_id)).await {
            Ok(EcResult::FanRampdownCurve(curve)) => curve,
            _ => return Err(format!("Failed to get Fan{} rampdown curve", fan_id)),
        };

        Ok(FanMetrics {
            mode,
            level,
            rpm,
            rampup_curve,
            rampdown_curve,
        })
    };

    // Get metrics for all fans
    let fan1 = match get_fan_metrics(1).await {
        Ok(metrics) => metrics,
        Err(e) => return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: e }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    };

    let fan2 = match get_fan_metrics(2).await {
        Ok(metrics) => metrics,
        Err(e) => return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: e }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    };

    let fan3 = match get_fan_metrics(3).await {
        Ok(metrics) => metrics,
        Err(e) => return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: e }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    };

    let metrics = MetricsResponse {
        power_mode,
        temperature,
        fan1,
        fan2,
        fan3,
    };

    {
        let mut log = logger.lock().unwrap();
        log.info("Metrics response prepared successfully");
    }

    Ok(warp::reply::with_status(
        warp::reply::json(&metrics),
        warp::http::StatusCode::OK,
    ))
}

async fn handle_fan_rpm(
    fan_id: u8,
    logger: Arc<Mutex<Logger>>,
    ec_queue: Arc<mpsc::UnboundedSender<(EcOperation, tokio::sync::oneshot::Sender<std::result::Result<EcResult, String>>)>>,
) -> std::result::Result<impl warp::Reply, warp::Rejection> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    if ec_queue.send((EcOperation::GetFanRpm(fan_id), tx)).is_err() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "EC queue unavailable".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    match rx.await {
        Ok(Ok(EcResult::FanRpm(rpm))) => {
            {
                let mut log = logger.lock().unwrap();
                log.info(&format!("Fan{} RPM: {}", fan_id, rpm));
            }
            
            Ok(warp::reply::with_status(
                warp::reply::json(&FanRpmResponse { rpm }),
                warp::http::StatusCode::OK,
            ))
        }
        Ok(Err(e)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: e }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Ok(Ok(_)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Unexpected response type".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Err(_) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Communication timeout".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}

async fn handle_fan_mode_get(
    fan_id: u8,
    logger: Arc<Mutex<Logger>>,
    ec_queue: Arc<mpsc::UnboundedSender<(EcOperation, tokio::sync::oneshot::Sender<std::result::Result<EcResult, String>>)>>,
) -> std::result::Result<impl warp::Reply, warp::Rejection> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    if ec_queue.send((EcOperation::GetFanMode(fan_id), tx)).is_err() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "EC queue unavailable".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    match rx.await {
        Ok(Ok(EcResult::FanMode(mode))) => {
            {
                let mut log = logger.lock().unwrap();
                log.info(&format!("Fan{} mode: {}", fan_id, mode));
            }
            
            Ok(warp::reply::with_status(
                warp::reply::json(&FanModeResponse { mode }),
                warp::http::StatusCode::OK,
            ))
        }
        Ok(Err(e)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: e }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Ok(Ok(_)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Unexpected response type".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Err(_) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Communication timeout".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}

async fn handle_fan_mode_post(
    request: FanModeRequest,
    fan_id: u8,
    logger: Arc<Mutex<Logger>>,
    ec_queue: Arc<mpsc::UnboundedSender<(EcOperation, tokio::sync::oneshot::Sender<std::result::Result<EcResult, String>>)>>,
    config: Arc<Mutex<ServerConfig>>,
) -> std::result::Result<impl warp::Reply, warp::Rejection> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    if ec_queue.send((EcOperation::SetFanMode(fan_id, request.mode.clone()), tx)).is_err() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "EC queue unavailable".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    match rx.await {
        Ok(Ok(EcResult::FanMode(mode))) => {
            {
                let mut log = logger.lock().unwrap();
                log.info(&format!("Fan{} mode set to: {}", fan_id, mode));
            }
            
            // Save to config
            {
                let mut config_guard = config.lock().unwrap();
                let fan_config_opt = match fan_id {
                    1 => &mut config_guard.fan1,
                    2 => &mut config_guard.fan2,
                    3 => &mut config_guard.fan3,
                    _ => return Ok(warp::reply::with_status(
                        warp::reply::json(&ErrorResponse { error: "Invalid fan ID".to_string() }),
                        warp::http::StatusCode::BAD_REQUEST,
                    )),
                };
                
                // Create fan config if it doesn't exist
                if fan_config_opt.is_none() {
                    *fan_config_opt = Some(config::FanConfig::default());
                }
                
                if let Some(fan_config) = fan_config_opt {
                    fan_config.mode = mode.clone();
                    if let Err(e) = config_guard.save() {
                        let mut log = logger.lock().unwrap();
                        log.warn(&format!("Failed to save Fan{} mode to config: {}", fan_id, e));
                    }
                }
            }
            
            Ok(warp::reply::with_status(
                warp::reply::json(&FanModeResponse { mode }),
                warp::http::StatusCode::OK,
            ))
        }
        Ok(Err(e)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: e }),
            warp::http::StatusCode::BAD_REQUEST,
        )),
        Ok(Ok(_)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Unexpected response type".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Err(_) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Communication timeout".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}

// Curve handler functions
async fn handle_fan_rampup_curve_get(
    fan_id: u8,
    logger: Arc<Mutex<Logger>>,
    ec_queue: Arc<mpsc::UnboundedSender<(EcOperation, tokio::sync::oneshot::Sender<std::result::Result<EcResult, String>>)>>,
) -> std::result::Result<impl warp::Reply, warp::Rejection> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    if ec_queue.send((EcOperation::GetFanRampupCurve(fan_id), tx)).is_err() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "EC queue unavailable".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    match rx.await {
        Ok(Ok(EcResult::FanRampupCurve(curve))) => {
            {
                let mut log = logger.lock().unwrap();
                log.info(&format!("Fan{} rampup curve get: {:?}", fan_id, curve));
            }
            
            Ok(warp::reply::with_status(
                warp::reply::json(&FanCurveResponse { curve }),
                warp::http::StatusCode::OK,
            ))
        }
        Ok(Err(e)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: e }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Ok(Ok(_)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Unexpected response type".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Err(_) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Communication timeout".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}

async fn handle_fan_rampup_curve_post(
    request: FanCurveRequest,
    fan_id: u8,
    logger: Arc<Mutex<Logger>>,
    ec_queue: Arc<mpsc::UnboundedSender<(EcOperation, tokio::sync::oneshot::Sender<std::result::Result<EcResult, String>>)>>,
    config: Arc<Mutex<ServerConfig>>,
) -> std::result::Result<impl warp::Reply, warp::Rejection> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    if ec_queue.send((EcOperation::SetFanRampupCurve(fan_id, request.curve), tx)).is_err() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "EC queue unavailable".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    match rx.await {
        Ok(Ok(EcResult::FanRampupCurve(curve))) => {
            {
                let mut log = logger.lock().unwrap();
                log.info(&format!("Fan{} rampup curve set to: {:?}", fan_id, curve));
            }
            
            // Save to config
            {
                let mut config_guard = config.lock().unwrap();
                let fan_config_opt = match fan_id {
                    1 => &mut config_guard.fan1,
                    2 => &mut config_guard.fan2,
                    3 => &mut config_guard.fan3,
                    _ => return Ok(warp::reply::with_status(
                        warp::reply::json(&ErrorResponse { error: "Invalid fan ID".to_string() }),
                        warp::http::StatusCode::BAD_REQUEST,
                    )),
                };
                
                // Create fan config if it doesn't exist
                if fan_config_opt.is_none() {
                    *fan_config_opt = Some(config::FanConfig::default());
                }
                
                if let Some(fan_config) = fan_config_opt {
                    fan_config.rampup_curve = curve;
                    if let Err(e) = config_guard.save() {
                        let mut log = logger.lock().unwrap();
                        log.warn(&format!("Failed to save Fan{} rampup curve to config: {}", fan_id, e));
                    }
                }
            }
            
            Ok(warp::reply::with_status(
                warp::reply::json(&FanCurveResponse { curve }),
                warp::http::StatusCode::OK,
            ))
        }
        Ok(Err(e)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: e }),
            warp::http::StatusCode::BAD_REQUEST,
        )),
        Ok(Ok(_)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Unexpected response type".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Err(_) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Communication timeout".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}

async fn handle_fan_rampdown_curve_get(
    fan_id: u8,
    logger: Arc<Mutex<Logger>>,
    ec_queue: Arc<mpsc::UnboundedSender<(EcOperation, tokio::sync::oneshot::Sender<std::result::Result<EcResult, String>>)>>,
) -> std::result::Result<impl warp::Reply, warp::Rejection> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    if ec_queue.send((EcOperation::GetFanRampdownCurve(fan_id), tx)).is_err() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "EC queue unavailable".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    match rx.await {
        Ok(Ok(EcResult::FanRampdownCurve(curve))) => {
            {
                let mut log = logger.lock().unwrap();
                log.info(&format!("Fan{} rampdown curve get: {:?}", fan_id, curve));
            }
            
            Ok(warp::reply::with_status(
                warp::reply::json(&FanCurveResponse { curve }),
                warp::http::StatusCode::OK,
            ))
        }
        Ok(Err(e)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: e }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Ok(Ok(_)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Unexpected response type".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Err(_) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Communication timeout".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}

async fn handle_fan_rampdown_curve_post(
    request: FanCurveRequest,
    fan_id: u8,
    logger: Arc<Mutex<Logger>>,
    ec_queue: Arc<mpsc::UnboundedSender<(EcOperation, tokio::sync::oneshot::Sender<std::result::Result<EcResult, String>>)>>,
    config: Arc<Mutex<ServerConfig>>,
) -> std::result::Result<impl warp::Reply, warp::Rejection> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    if ec_queue.send((EcOperation::SetFanRampdownCurve(fan_id, request.curve), tx)).is_err() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "EC queue unavailable".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    match rx.await {
        Ok(Ok(EcResult::FanRampdownCurve(curve))) => {
            {
                let mut log = logger.lock().unwrap();
                log.info(&format!("Fan{} rampdown curve set to: {:?}", fan_id, curve));
            }
            
            // Save to config
            {
                let mut config_guard = config.lock().unwrap();
                let fan_config_opt = match fan_id {
                    1 => &mut config_guard.fan1,
                    2 => &mut config_guard.fan2,
                    3 => &mut config_guard.fan3,
                    _ => return Ok(warp::reply::with_status(
                        warp::reply::json(&ErrorResponse { error: "Invalid fan ID".to_string() }),
                        warp::http::StatusCode::BAD_REQUEST,
                    )),
                };
                
                // Create fan config if it doesn't exist
                if fan_config_opt.is_none() {
                    *fan_config_opt = Some(config::FanConfig::default());
                }
                
                if let Some(fan_config) = fan_config_opt {
                    fan_config.rampdown_curve = curve;
                    if let Err(e) = config_guard.save() {
                        let mut log = logger.lock().unwrap();
                        log.warn(&format!("Failed to save Fan{} rampdown curve to config: {}", fan_id, e));
                    }
                }
            }
            
            Ok(warp::reply::with_status(
                warp::reply::json(&FanCurveResponse { curve }),
                warp::http::StatusCode::OK,
            ))
        }
        Ok(Err(e)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: e }),
            warp::http::StatusCode::BAD_REQUEST,
        )),
        Ok(Ok(_)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Unexpected response type".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Err(_) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Communication timeout".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}

async fn handle_fan_level_get(
    fan_id: u8,
    logger: Arc<Mutex<Logger>>,
    ec_queue: Arc<mpsc::UnboundedSender<(EcOperation, tokio::sync::oneshot::Sender<Result<EcResult, String>>)>>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    if ec_queue.send((EcOperation::GetFanLevel(fan_id), tx)).is_err() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "EC queue unavailable".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    match rx.await {
        Ok(Ok(EcResult::FanLevel(level))) => {
            {
                let mut log = logger.lock().unwrap();
                log.info(&format!("Fan{} level: {}", fan_id, level));
            }
            
            Ok(warp::reply::with_status(
                warp::reply::json(&FanLevelResponse { level }),
                warp::http::StatusCode::OK,
            ))
        }
        Ok(Err(e)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: e }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Ok(Ok(_)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Unexpected response type".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Err(_) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Communication timeout".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}

async fn handle_fan_level_post(
    request: FanLevelRequest,
    fan_id: u8,
    logger: Arc<Mutex<Logger>>,
    ec_queue: Arc<mpsc::UnboundedSender<(EcOperation, tokio::sync::oneshot::Sender<Result<EcResult, String>>)>>,
    config: Arc<Mutex<ServerConfig>>,
) -> Result<impl warp::Reply, warp::Rejection> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    if ec_queue.send((EcOperation::SetFanLevel(fan_id, request.level), tx)).is_err() {
        return Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "EC queue unavailable".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        ));
    }

    match rx.await {
        Ok(Ok(EcResult::FanLevel(level))) => {
            {
                let mut log = logger.lock().unwrap();
                log.info(&format!("Fan{} level set to: {}", fan_id, level));
            }
            
            // Save to config
            {
                let mut config_guard = config.lock().unwrap();
                let fan_config_opt = match fan_id {
                    1 => &mut config_guard.fan1,
                    2 => &mut config_guard.fan2,
                    3 => &mut config_guard.fan3,
                    _ => return Ok(warp::reply::with_status(
                        warp::reply::json(&ErrorResponse { error: "Invalid fan ID".to_string() }),
                        warp::http::StatusCode::BAD_REQUEST,
                    )),
                };
                
                // Create fan config if it doesn't exist
                if fan_config_opt.is_none() {
                    *fan_config_opt = Some(config::FanConfig::default());
                }
                
                if let Some(fan_config) = fan_config_opt {
                    fan_config.level = level;
                    if let Err(e) = config_guard.save() {
                        let mut log = logger.lock().unwrap();
                        log.warn(&format!("Failed to save Fan{} level to config: {}", fan_id, e));
                    }
                }
            }
            
            
            Ok(warp::reply::with_status(
                warp::reply::json(&FanLevelResponse { level }),
                warp::http::StatusCode::OK,
            ))
        }
        Ok(Err(e)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse { error: e }),
            warp::http::StatusCode::BAD_REQUEST,
        )),
        Ok(Ok(_)) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Unexpected response type".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
        Err(_) => Ok(warp::reply::with_status(
            warp::reply::json(&ErrorResponse {
                error: "Communication timeout".to_string(),
            }),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )),
    }
}