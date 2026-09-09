//! Tray icon management module

use anyhow::Result;
use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem, MenuEvent};
use tray_icon::{TrayIcon, TrayIconBuilder};
use std::process::Command;

use crate::config::{self, Config};
use crate::icons::{create_offline_icon, create_offline_fan_icon, create_temp_icon, create_rpm_bar_icon};
use crate::metrics::{MetricsState, temp_color, rpm_color};

/// Menu item IDs
pub const MENU_OPEN_CLIENT: &str = "open_client";
pub const MENU_AUTOSTART: &str = "autostart";
pub const MENU_EXIT: &str = "exit";

/// Tray icon identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayIconId {
    Temperature,
    Fan1,
    Fan2,
    Fan3,
}

impl TrayIconId {
    pub fn label(&self) -> &'static str {
        match self {
            TrayIconId::Temperature => "APU",
            TrayIconId::Fan1 => "F1",
            TrayIconId::Fan2 => "F2",
            TrayIconId::Fan3 => "F3",
        }
    }
}

/// Manages the four tray icons
pub struct TrayManager {
    pub temp_icon: TrayIcon,
    pub fan1_icon: TrayIcon,
    pub fan2_icon: TrayIcon,
    pub fan3_icon: TrayIcon,
    pub config: Config,
    menu: Menu,
    autostart_item: MenuItem,
}

impl TrayManager {
    /// Create new tray manager with 4 icons
    pub fn new(config: Config) -> Result<Self> {
        // Create the context menu (shared by all icons)
        let menu = Menu::new();
        
        let open_client = MenuItem::with_id(MENU_OPEN_CLIENT, "Open Client", true, None);
        
        #[cfg(windows)]
        let autostart_enabled = config::autostart::is_enabled();
        #[cfg(not(windows))]
        let autostart_enabled = false;
        
        let autostart_text = if autostart_enabled {
            "Auto-start: ✓"
        } else {
            "Auto-start: ✗"
        };
        let autostart_item = MenuItem::with_id(MENU_AUTOSTART, autostart_text, true, None);
        
        let exit = MenuItem::with_id(MENU_EXIT, "Exit", true, None);
        
        menu.append(&open_client)?;
        menu.append(&autostart_item)?;
        menu.append(&PredefinedMenuItem::separator())?;
        menu.append(&exit)?;
        
        // Create initial offline icons
        let offline_icon = create_offline_icon();
        
        let temp_icon = TrayIconBuilder::new()
            .with_icon(offline_icon.clone())
            .with_tooltip("APU Temperature: --")
            .with_menu(Box::new(menu.clone()))
            .build()?;
        
        let fan1_icon = TrayIconBuilder::new()
            .with_icon(offline_icon.clone())
            .with_tooltip("Fan 1: -- RPM")
            .with_menu(Box::new(menu.clone()))
            .build()?;
        
        let fan2_icon = TrayIconBuilder::new()
            .with_icon(offline_icon.clone())
            .with_tooltip("Fan 2: -- RPM")
            .with_menu(Box::new(menu.clone()))
            .build()?;
        
        let fan3_icon = TrayIconBuilder::new()
            .with_icon(offline_icon)
            .with_tooltip("Fan 3: -- RPM")
            .with_menu(Box::new(menu.clone()))
            .build()?;
        
        Ok(Self {
            temp_icon,
            fan1_icon,
            fan2_icon,
            fan3_icon,
            config,
            menu,
            autostart_item,
        })
    }
    
    /// Update all icons based on current metrics state
    pub fn update(&mut self, state: &MetricsState) {
        if let Some(metrics) = &state.metrics {
            // Update temperature icon
            let temp = metrics.temperature;
            let color = temp_color(temp);
            let icon = create_temp_icon(temp, color);
            let _ = self.temp_icon.set_icon(Some(icon));
            let _ = self.temp_icon.set_tooltip(Some(format!("APU Temperature: {}°C", temp)));
            
            // Update fan 1 icon (bar style)
            let rpm1 = metrics.fan1.rpm;
            let color1 = rpm_color(rpm1);
            let icon1 = create_rpm_bar_icon(rpm1, 1, color1);
            let _ = self.fan1_icon.set_icon(Some(icon1));
            let _ = self.fan1_icon.set_tooltip(Some(format!(
                "Fan 1: {} RPM ({})", 
                rpm1, 
                metrics.fan1.mode
            )));
            
            // Update fan 2 icon (bar style)
            let rpm2 = metrics.fan2.rpm;
            let color2 = rpm_color(rpm2);
            let icon2 = create_rpm_bar_icon(rpm2, 2, color2);
            let _ = self.fan2_icon.set_icon(Some(icon2));
            let _ = self.fan2_icon.set_tooltip(Some(format!(
                "Fan 2: {} RPM ({})", 
                rpm2, 
                metrics.fan2.mode
            )));
            
            // Update fan 3 icon (bar style)
            let rpm3 = metrics.fan3.rpm;
            let color3 = rpm_color(rpm3);
            let icon3 = create_rpm_bar_icon(rpm3, 3, color3);
            let _ = self.fan3_icon.set_icon(Some(icon3));
            let _ = self.fan3_icon.set_tooltip(Some(format!(
                "Fan 3: {} RPM ({})", 
                rpm3, 
                metrics.fan3.mode
            )));
        } else {
            // Server offline - show gray icons
            let offline_icon = create_offline_icon();
            let offline_fan = create_offline_fan_icon();
            let error_msg = state.error.as_deref().unwrap_or("Server offline");
            
            let _ = self.temp_icon.set_icon(Some(offline_icon));
            let _ = self.temp_icon.set_tooltip(Some(format!("APU: {}", error_msg)));
            
            let _ = self.fan1_icon.set_icon(Some(offline_fan.clone()));
            let _ = self.fan1_icon.set_tooltip(Some(format!("Fan 1: {}", error_msg)));
            
            let _ = self.fan2_icon.set_icon(Some(offline_fan.clone()));
            let _ = self.fan2_icon.set_tooltip(Some(format!("Fan 2: {}", error_msg)));
            
            let _ = self.fan3_icon.set_icon(Some(offline_fan));
            let _ = self.fan3_icon.set_tooltip(Some(format!("Fan 3: {}", error_msg)));
        }
    }
    
    /// Handle menu events
    pub fn handle_menu_event(&mut self, event: &MenuEvent) -> bool {
        match event.id.0.as_str() {
            MENU_OPEN_CLIENT => {
                self.open_client();
                false
            }
            MENU_AUTOSTART => {
                self.toggle_autostart();
                false
            }
            MENU_EXIT => {
                true // Signal to exit
            }
            _ => false
        }
    }
    
    /// Open the client application
    fn open_client(&self) {
        // Try common installation paths
        let paths = [
            r"C:\Program Files\ec-su_axb35-win\ec-su_axb35-win-client.exe",
            r"C:\Program Files (x86)\ec-su_axb35-win\ec-su_axb35-win-client.exe",
        ];
        
        for path in &paths {
            if std::path::Path::new(path).exists() {
                let _ = Command::new(path).spawn();
                return;
            }
        }
        
        // Try to find client in same directory as tray executable
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(parent) = exe_path.parent() {
                let client_path = parent.join("ec-su_axb35-win-client.exe");
                if client_path.exists() {
                    let _ = Command::new(client_path).spawn();
                }
            }
        }
    }
    
    /// Toggle autostart setting
    fn toggle_autostart(&mut self) {
        #[cfg(windows)]
        {
            let currently_enabled = config::autostart::is_enabled();
            let new_state = !currently_enabled;
            
            if config::autostart::set_enabled(new_state).is_ok() {
                self.config.autostart = new_state;
                let _ = config::save_config(&self.config);
                
                // Update menu item text
                let new_text = if new_state {
                    "Auto-start: ✓"
                } else {
                    "Auto-start: ✗"
                };
                let _ = self.autostart_item.set_text(new_text);
            }
        }
    }
}
