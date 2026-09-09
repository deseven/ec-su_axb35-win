//! EC-SU AXB35 Tray Monitor
//! 
//! A lightweight Windows system tray application that displays 4 icons:
//! - APU Temperature
//! - Fan 1 RPM
//! - Fan 2 RPM  
//! - Fan 3 RPM
//!
//! Icons show numeric values on color-coded backgrounds (green/yellow/red).
//! Click any icon to open the full client GUI.

#![windows_subsystem = "windows"]

use std::time::Duration;
use anyhow::Result;
use tray_icon::menu::MenuEvent;
use tokio::sync::watch;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::WindowId;

mod config;
mod icons;
mod metrics;
mod tray;

use config::load_config;
use metrics::{start_polling, MetricsState};
use tray::TrayManager;

/// Mutex name for single instance enforcement
#[cfg(windows)]
const MUTEX_NAME: &str = "Global\\ec-su_axb35-win-tray-mutex";

/// Check if another instance is already running
#[cfg(windows)]
fn check_single_instance() -> Option<winapi::um::winnt::HANDLE> {
    use std::ffi::CString;
    use winapi::um::synchapi::CreateMutexA;
    use winapi::um::errhandlingapi::GetLastError;
    use winapi::shared::winerror::ERROR_ALREADY_EXISTS;
    
    let mutex_name = CString::new(MUTEX_NAME).unwrap();
    
    unsafe {
        let handle = CreateMutexA(
            std::ptr::null_mut(),
            0,
            mutex_name.as_ptr(),
        );
        
        if handle.is_null() {
            return None;
        }
        
        if GetLastError() == ERROR_ALREADY_EXISTS {
            winapi::um::handleapi::CloseHandle(handle);
            return None;
        }
        
        Some(handle)
    }
}

#[cfg(not(windows))]
fn check_single_instance() -> Option<()> {
    Some(())
}

/// Application state
struct App {
    tray_manager: Option<TrayManager>,
    metrics_rx: watch::Receiver<MetricsState>,
    last_state: MetricsState,
    #[allow(dead_code)]
    polling_handle: tokio::task::JoinHandle<()>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        // Nothing to do here for tray-only app
    }
    
    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, _event: WindowEvent) {
        // No windows to handle
    }
    
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Check for metrics updates
        if self.metrics_rx.has_changed().unwrap_or(false) {
            self.last_state = self.metrics_rx.borrow_and_update().clone();
            if let Some(tray) = &mut self.tray_manager {
                tray.update(&self.last_state);
            }
        }
        
        // Handle menu events
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            if let Some(tray) = &mut self.tray_manager {
                if tray.handle_menu_event(&event) {
                    event_loop.exit();
                    return;
                }
            }
        }
        
        // Keep event loop running with a small timeout to prevent busy-waiting
        event_loop.set_control_flow(ControlFlow::wait_duration(Duration::from_millis(100)));
    }
}

fn main() -> Result<()> {
    // Enforce single instance
    let _mutex = check_single_instance();
    if _mutex.is_none() {
        // Another instance is already running
        eprintln!("Another instance of the tray monitor is already running.");
        return Ok(());
    }
    
    // Load configuration
    let config = load_config().unwrap_or_default();
    let server_url = config.server_url();
    
    // Create tokio runtime for async operations
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    
    // Create channel for metrics updates
    let (tx, rx) = watch::channel(MetricsState::default());
    
    // Start polling in background
    let polling_handle = runtime.spawn(async move {
        let handle = start_polling(server_url, tx);
        handle.await.unwrap();
    });
    
    // Create event loop
    let event_loop = EventLoop::new()?;
    
    // Create tray manager
    let tray_manager = TrayManager::new(config)?;
    
    // Create app state
    let mut app = App {
        tray_manager: Some(tray_manager),
        metrics_rx: rx,
        last_state: MetricsState::default(),
        polling_handle,
    };
    
    // Run the event loop
    event_loop.run_app(&mut app)?;
    
    Ok(())
}
