#![windows_subsystem = "windows"]

use anyhow::{Context, Result};
use dirs::config_dir;
use eframe::egui;
use image::GenericImageView;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::interval;

// Embed the PNG icons as bytes at compile time
const ICON_BYTES: &[u8] = include_bytes!("../ec-su_axb35-win.png");
const COG_ICON_BYTES: &[u8] = include_bytes!("../cog.png");
const CHECK_ICON_BYTES: &[u8] = include_bytes!("../check.png");

// Configuration structure
#[derive(Serialize, Deserialize, Clone)]
struct Config {
    server_ip: String,
    server_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            server_ip: "127.0.0.1".to_string(),
            server_port: 8395,
        }
    }
}

// API response structures
#[derive(Deserialize, Debug, Clone)]
struct StatusResponse {
    status: i32,
    version: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
struct FanMetrics {
    mode: String,
    level: i32,
    rpm: i32,
    rampup_curve: Vec<i32>,
    rampdown_curve: Vec<i32>,
}

#[derive(Deserialize, Debug, Clone)]
struct MetricsResponse {
    power_mode: String,
    temperature: i32,
    fan1: FanMetrics,
    fan2: FanMetrics,
    fan3: FanMetrics,
}

// Request structures for API calls
#[derive(Serialize, Debug)]
struct PowerModeRequest {
    power_mode: String,
}

#[derive(Serialize, Debug)]
struct FanModeRequest {
    mode: String,
}

#[derive(Serialize, Debug)]
struct FanLevelRequest {
    level: i32,
}

#[derive(Serialize, Debug)]
struct FanCurveRequest {
    curve: Vec<i32>,
}

// Edit mode state for each block
#[derive(Clone, Debug)]
struct EditState {
    apu_edit_mode: bool,
    fan1_edit_mode: bool,
    fan2_edit_mode: bool,
    fan3_edit_mode: bool,
    // Spinner states for apply operations
    apu_applying: bool,
    fan1_applying: bool,
    fan2_applying: bool,
    fan3_applying: bool,
    // Temporary edit values
    temp_apu_power_mode: String,
    temp_fan1_mode: String,
    temp_fan1_level: i32,
    temp_fan1_rampup: String,
    temp_fan1_rampdown: String,
    temp_fan2_mode: String,
    temp_fan2_level: i32,
    temp_fan2_rampup: String,
    temp_fan2_rampdown: String,
    temp_fan3_mode: String,
    temp_fan3_level: i32,
    temp_fan3_rampup: String,
    temp_fan3_rampdown: String,
}

impl Default for EditState {
    fn default() -> Self {
        Self {
            apu_edit_mode: false,
            fan1_edit_mode: false,
            fan2_edit_mode: false,
            fan3_edit_mode: false,
            apu_applying: false,
            fan1_applying: false,
            fan2_applying: false,
            fan3_applying: false,
            temp_apu_power_mode: "balanced".to_string(),
            temp_fan1_mode: "auto".to_string(),
            temp_fan1_level: 0,
            temp_fan1_rampup: "60,70,83,95,97".to_string(),
            temp_fan1_rampdown: "40,50,80,94,96".to_string(),
            temp_fan2_mode: "auto".to_string(),
            temp_fan2_level: 0,
            temp_fan2_rampup: "60,70,83,95,97".to_string(),
            temp_fan2_rampdown: "40,50,80,94,96".to_string(),
            temp_fan3_mode: "auto".to_string(),
            temp_fan3_level: 0,
            temp_fan3_rampup: "60,70,83,95,97".to_string(),
            temp_fan3_rampdown: "40,50,80,94,96".to_string(),
        }
    }
}

// Color thresholds - can be adjusted later
struct ColorThresholds {
    temp_green: i32,
    temp_yellow: i32,
    rpm_green: i32,
    rpm_yellow: i32,
}

impl Default for ColorThresholds {
    fn default() -> Self {
        Self {
            temp_green: 50,
            temp_yellow: 80,
            rpm_green: 1200,
            rpm_yellow: 2600,
        }
    }
}

// Historical data for charts
const CHART_HISTORY_SIZE: usize = 60; // Keep 60 data points

#[derive(Clone)]
struct ChartData {
    temperature_history: VecDeque<i32>,
    fan1_rpm_history: VecDeque<i32>,
    fan2_rpm_history: VecDeque<i32>,
    fan3_rpm_history: VecDeque<i32>,
}

impl ChartData {
    fn new() -> Self {
        Self {
            temperature_history: VecDeque::with_capacity(CHART_HISTORY_SIZE),
            fan1_rpm_history: VecDeque::with_capacity(CHART_HISTORY_SIZE),
            fan2_rpm_history: VecDeque::with_capacity(CHART_HISTORY_SIZE),
            fan3_rpm_history: VecDeque::with_capacity(CHART_HISTORY_SIZE),
        }
    }
    
    fn add_data_point(&mut self, temp: i32, fan1_rpm: i32, fan2_rpm: i32, fan3_rpm: i32) {
        if self.temperature_history.len() >= CHART_HISTORY_SIZE {
            self.temperature_history.pop_front();
        }
        self.temperature_history.push_back(temp);
        
        if self.fan1_rpm_history.len() >= CHART_HISTORY_SIZE {
            self.fan1_rpm_history.pop_front();
        }
        self.fan1_rpm_history.push_back(fan1_rpm);
        
        if self.fan2_rpm_history.len() >= CHART_HISTORY_SIZE {
            self.fan2_rpm_history.pop_front();
        }
        self.fan2_rpm_history.push_back(fan2_rpm);
        
        if self.fan3_rpm_history.len() >= CHART_HISTORY_SIZE {
            self.fan3_rpm_history.pop_front();
        }
        self.fan3_rpm_history.push_back(fan3_rpm);
    }
}

// Application state
struct AppState {
    config: Config,
    http_client: Client,
    ec_version: Option<String>,
    metrics: Option<MetricsResponse>,
    last_update: Option<Instant>,
    error_message: Option<String>,
    error_timestamp: Option<Instant>,
    color_thresholds: ColorThresholds,
    edit_state: EditState,
    cog_icon: Option<egui::TextureHandle>,
    check_icon: Option<egui::TextureHandle>,
    chart_data: ChartData,
}

impl AppState {
    fn new(config: Config) -> Self {
        Self {
            config,
            http_client: Client::new(),
            ec_version: None,
            metrics: None,
            last_update: None,
            error_message: None,
            error_timestamp: None,
            color_thresholds: ColorThresholds::default(),
            edit_state: EditState::default(),
            cog_icon: None,
            check_icon: None,
            chart_data: ChartData::new(),
        }
    }

    fn server_url(&self) -> String {
        format!("http://{}:{}", self.config.server_ip, self.config.server_port)
    }

    async fn check_status(&mut self) -> Result<()> {
        let url = format!("{}/status", self.server_url());
        let response: StatusResponse = self
            .http_client
            .get(&url)
            .send()
            .await
            .context("Failed to connect to server")?
            .json()
            .await
            .context("Failed to parse status response")?;

        if response.status == 1 {
            self.ec_version = response.version;
            Ok(())
        } else {
            anyhow::bail!("EC status check failed")
        }
    }


    fn load_icons(&mut self, ctx: &egui::Context) {
        if self.cog_icon.is_none() {
            if let Ok(img) = image::load_from_memory(COG_ICON_BYTES) {
                let rgba = img.to_rgba8();
                let (width, height) = img.dimensions();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [width as usize, height as usize],
                    &rgba,
                );
                self.cog_icon = Some(ctx.load_texture("cog", color_image, egui::TextureOptions::default()));
            }
        }
        
        if self.check_icon.is_none() {
            if let Ok(img) = image::load_from_memory(CHECK_ICON_BYTES) {
                let rgba = img.to_rgba8();
                let (width, height) = img.dimensions();
                let color_image = egui::ColorImage::from_rgba_unmultiplied(
                    [width as usize, height as usize],
                    &rgba,
                );
                self.check_icon = Some(ctx.load_texture("check", color_image, egui::TextureOptions::default()));
            }
        }
    }

    fn get_temp_color(&self, temp: i32) -> egui::Color32 {
        if temp <= self.color_thresholds.temp_green {
            egui::Color32::GREEN
        } else if temp <= self.color_thresholds.temp_yellow {
            egui::Color32::YELLOW
        } else {
            egui::Color32::RED
        }
    }

    fn get_rpm_color(&self, rpm: i32) -> egui::Color32 {
        if rpm <= self.color_thresholds.rpm_green {
            egui::Color32::GREEN
        } else if rpm <= self.color_thresholds.rpm_yellow {
            egui::Color32::YELLOW
        } else {
            egui::Color32::RED
        }
    }

    fn get_mode_color(&self, mode: &str) -> egui::Color32 {
        match mode {
            "auto" => egui::Color32::MAGENTA,
            "fixed" => egui::Color32::GRAY,
            "curve" => egui::Color32::from_rgb(0, 255, 255), // Cyan
            _ => egui::Color32::WHITE,
        }
    }

    fn get_power_mode_color(&self, power_mode: &str) -> egui::Color32 {
        match power_mode {
            "quiet" => egui::Color32::GREEN,
            "balanced" => egui::Color32::LIGHT_BLUE,
            "performance" => egui::Color32::RED,
            _ => egui::Color32::WHITE,
        }
    }


    fn curve_to_string(&self, curve: &[i32]) -> String {
        curve.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",")
    }

    fn set_error(&mut self, message: String) {
        self.error_message = Some(message);
        self.error_timestamp = Some(Instant::now());
    }

    fn clear_old_error(&mut self) {
        if let (Some(_), Some(timestamp)) = (&self.error_message, self.error_timestamp) {
            if timestamp.elapsed() > Duration::from_secs(5) {
                self.error_message = None;
                self.error_timestamp = None;
            }
        }
    }
}

// Main application
struct EcMonitorApp {
    state: Arc<Mutex<AppState>>,
    metrics_task: Option<tokio::task::JoinHandle<()>>,
    window_configured: bool,
    last_content_height: f32,
}

impl EcMonitorApp {
    fn new(state: Arc<Mutex<AppState>>) -> Self {
        Self {
            state,
            metrics_task: None,
            window_configured: false,
            last_content_height: 0.0,
        }
    }

    fn start_metrics_polling(&mut self) {
        if self.metrics_task.is_some() {
            return;
        }

        let state = Arc::clone(&self.state);
        self.metrics_task = Some(tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                
                // Clone the HTTP client and config outside the lock
                let (client, server_url) = {
                    let state_guard = state.lock().unwrap();
                    (state_guard.http_client.clone(), state_guard.server_url())
                };
                
                // Make the HTTP request and parse JSON outside the lock
                let url = format!("{}/metrics", server_url);
                let result = async {
                    let response = client.get(&url).send().await?;
                    let metrics: MetricsResponse = response.json().await?;
                    Ok::<MetricsResponse, reqwest::Error>(metrics)
                }.await;
                
                // Update state with the result
                {
                    let mut state_guard = state.lock().unwrap();
                    match result {
                        Ok(metrics) => {
                            // Add to chart data history
                            state_guard.chart_data.add_data_point(
                                metrics.temperature,
                                metrics.fan1.rpm,
                                metrics.fan2.rpm,
                                metrics.fan3.rpm,
                            );
                            
                            state_guard.metrics = Some(metrics);
                            state_guard.last_update = Some(Instant::now());
                            // Don't clear error messages here - let them expire naturally after 5 seconds
                            // Only clear metrics-related errors, not API call errors
                            if let Some(error) = &state_guard.error_message {
                                if error.starts_with("Failed to fetch metrics:") {
                                    state_guard.error_message = None;
                                    state_guard.error_timestamp = None;
                                }
                            }
                        }
                        Err(e) => {
                            state_guard.set_error(format!("Failed to fetch metrics: {}", e));
                        }
                    }
                }
            }
        }));
    }

    fn stop_metrics_polling(&mut self) {
        if let Some(task) = self.metrics_task.take() {
            task.abort();
        }
    }

    fn draw_bar_chart(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        history: &VecDeque<i32>,
        max_value: i32,
        color: egui::Color32,
    ) {
        if history.is_empty() {
            return;
        }
        
        let painter = ui.painter();
        let num_bars = history.len();
        
        if num_bars == 0 {
            return;
        }
        
        // Calculate bar width based on max capacity so bars fill width when at max
        let bar_width = rect.width() / CHART_HISTORY_SIZE as f32;
        
        // Calculate how many bars fit in the rect
        let max_bars = (rect.width() / bar_width).floor() as usize;
        
        // Determine which bars to draw (most recent ones)
        let start_index = if num_bars > max_bars {
            num_bars - max_bars
        } else {
            0
        };
        
        // Draw bars from right to left, starting with the most recent
        for (i, &value) in history.iter().skip(start_index).enumerate() {
            let normalized_height = (value as f32 / max_value as f32).clamp(0.0, 1.0);
            let bar_height = rect.height() * normalized_height;
            
            // Position bars from left to right within available space
            let x_offset = if num_bars <= max_bars {
                // If we have fewer bars than max, align them to the left
                i as f32 * bar_width
            } else {
                // If we have more bars, fill from left to right
                i as f32 * bar_width
            };
            
            let bar_rect = egui::Rect::from_min_max(
                egui::pos2(
                    rect.min.x + x_offset,
                    rect.max.y - bar_height,
                ),
                egui::pos2(
                    rect.min.x + x_offset + bar_width,
                    rect.max.y,
                ),
            );
            
            // Draw with 10% opacity
            let chart_color = egui::Color32::from_rgba_unmultiplied(
                color.r(),
                color.g(),
                color.b(),
                40, // 15% opacity
            );
            painter.rect_filled(bar_rect, 0.0, chart_color);
        }
    }

    fn draw_apu_block(&self, ui: &mut egui::Ui, metrics: &MetricsResponse, state: &mut AppState) {
        let response = ui.group(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.heading("APU");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if state.edit_state.apu_applying {
                            // Show spinner while applying
                            ui.add(egui::Spinner::new());
                        } else {
                            let icon = if state.edit_state.apu_edit_mode {
                                &state.check_icon
                            } else {
                                &state.cog_icon
                            };
                            
                            if let Some(texture) = icon {
                                let image = egui::Image::from_texture(texture).fit_to_exact_size(egui::Vec2::new(16.0, 16.0));
                                if ui.add(egui::Button::image(image).frame(false)).clicked() {
                                    if state.edit_state.apu_edit_mode {
                                        // Set applying state and spawn async task
                                        state.edit_state.apu_applying = true;
                                        let power_mode = state.edit_state.temp_apu_power_mode.clone();
                                        let state_clone = Arc::clone(&self.state);
                                        tokio::spawn(async move {
                                        // Extract the HTTP client and server URL outside the lock
                                        let (client, server_url) = {
                                            let state_guard = state_clone.lock().unwrap();
                                            (state_guard.http_client.clone(), state_guard.server_url())
                                        };
                                        
                                        // Make the API call without holding the lock
                                        let url = format!("{}/apu/power_mode", server_url);
                                        let request = PowerModeRequest {
                                            power_mode: power_mode.clone(),
                                        };
                                        
                                        let result = client
                                            .post(&url)
                                            .json(&request)
                                            .send()
                                            .await;
                                        
                                        // Update state based on result
                                        match result {
                                            Ok(response) if response.status().is_success() => {
                                                // Refresh metrics immediately after successful change
                                                let metrics_url = format!("{}/metrics", server_url);
                                                if let Ok(metrics_response) = client.get(&metrics_url).send().await {
                                                    if let Ok(metrics) = metrics_response.json::<MetricsResponse>().await {
                                                        let mut state_guard = state_clone.lock().unwrap();
                                                        state_guard.metrics = Some(metrics);
                                                        state_guard.last_update = Some(Instant::now());
                                                        state_guard.edit_state.apu_edit_mode = false;
                                                        state_guard.edit_state.apu_applying = false;
                                                    } else {
                                                        let mut state_guard = state_clone.lock().unwrap();
                                                        state_guard.edit_state.apu_edit_mode = false;
                                                        state_guard.edit_state.apu_applying = false;
                                                    }
                                                } else {
                                                    let mut state_guard = state_clone.lock().unwrap();
                                                    state_guard.edit_state.apu_edit_mode = false;
                                                    state_guard.edit_state.apu_applying = false;
                                                }
                                            }
                                            Ok(response) => {
                                                let mut state_guard = state_clone.lock().unwrap();
                                                state_guard.set_error(format!("Failed to set APU power mode: {}", response.status()));
                                                state_guard.edit_state.apu_applying = false;
                                                // Don't clear edit mode on error, let user see the error and try again
                                            }
                                            Err(e) => {
                                                let mut state_guard = state_clone.lock().unwrap();
                                                state_guard.set_error(format!("Failed to set APU power mode: {}", e));
                                                state_guard.edit_state.apu_applying = false;
                                                // Don't clear edit mode on error, let user see the error and try again
                                            }
                                        }
                                        });
                                    } else {
                                        // Enter edit mode
                                        state.edit_state.apu_edit_mode = true;
                                        state.edit_state.temp_apu_power_mode = metrics.power_mode.clone();
                                    }
                                }
                            }
                        }
                    });
                });
                
                if state.edit_state.apu_edit_mode {
                    // Edit mode UI
                    ui.horizontal(|ui| {
                        ui.label("Power Mode:");
                        egui::ComboBox::from_label("")
                            .selected_text(&state.edit_state.temp_apu_power_mode)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut state.edit_state.temp_apu_power_mode, "quiet".to_string(), "quiet");
                                ui.selectable_value(&mut state.edit_state.temp_apu_power_mode, "balanced".to_string(), "balanced");
                                ui.selectable_value(&mut state.edit_state.temp_apu_power_mode, "performance".to_string(), "performance");
                            });
                    });
                } else {
                    // Display mode UI
                    ui.horizontal(|ui| {
                        ui.label("Temperature:");
                        ui.colored_label(
                            state.get_temp_color(metrics.temperature),
                            format!("{}°C", metrics.temperature),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("Power Mode:");
                        ui.colored_label(
                            state.get_power_mode_color(&metrics.power_mode),
                            &metrics.power_mode,
                        );
                    });
                }
            })
        });
        
        // Draw chart in the background after content is drawn, only if not in edit mode
        if !state.edit_state.apu_edit_mode {
            let mut rect = response.response.rect;
            rect.set_width(ui.available_width());
            self.draw_bar_chart(
                ui,
                rect,
                &state.chart_data.temperature_history,
                100, // Max temperature range 0-100
                state.get_temp_color(metrics.temperature),
            );
        }
    }

    fn draw_fan_block_with_edit(&self, ui: &mut egui::Ui, fan_name: &str, fan_id: i32, fan: &FanMetrics, state: &mut AppState) {
        // Clone chart data and determine edit mode before the closure to avoid borrow issues
        let (history_clone, max_rpm) = match fan_id {
            1 => (state.chart_data.fan1_rpm_history.clone(), 5000),
            2 => (state.chart_data.fan2_rpm_history.clone(), 5000),
            3 => (state.chart_data.fan3_rpm_history.clone(), 2500),
            _ => return, // Invalid fan_id
        };
        
        let is_edit_mode = match fan_id {
            1 => state.edit_state.fan1_edit_mode,
            2 => state.edit_state.fan2_edit_mode,
            3 => state.edit_state.fan3_edit_mode,
            _ => false,
        };
        
        let response = ui.group(|ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.heading(fan_name);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let is_edit_mode = match fan_id {
                            1 => state.edit_state.fan1_edit_mode,
                            2 => state.edit_state.fan2_edit_mode,
                            3 => state.edit_state.fan3_edit_mode,
                            _ => false,
                        };
                        
                        let is_applying = match fan_id {
                            1 => state.edit_state.fan1_applying,
                            2 => state.edit_state.fan2_applying,
                            3 => state.edit_state.fan3_applying,
                            _ => false,
                        };
                        
                        if is_applying {
                            // Show spinner while applying
                            ui.add(egui::Spinner::new());
                        } else {
                            let icon = if is_edit_mode {
                                &state.check_icon
                            } else {
                                &state.cog_icon
                            };
                            
                            if let Some(texture) = icon {
                                let image = egui::Image::from_texture(texture).fit_to_exact_size(egui::Vec2::new(16.0, 16.0));
                                if ui.add(egui::Button::image(image).frame(false)).clicked() {
                                    if is_edit_mode {
                                        // Set applying state and spawn async task
                                        match fan_id {
                                            1 => state.edit_state.fan1_applying = true,
                                            2 => state.edit_state.fan2_applying = true,
                                            3 => state.edit_state.fan3_applying = true,
                                            _ => return,
                                        };
                                        
                                        let (mode, level, rampup_str, rampdown_str) = match fan_id {
                                            1 => (state.edit_state.temp_fan1_mode.clone(), state.edit_state.temp_fan1_level,
                                                  state.edit_state.temp_fan1_rampup.clone(), state.edit_state.temp_fan1_rampdown.clone()),
                                            2 => (state.edit_state.temp_fan2_mode.clone(), state.edit_state.temp_fan2_level,
                                                  state.edit_state.temp_fan2_rampup.clone(), state.edit_state.temp_fan2_rampdown.clone()),
                                            3 => (state.edit_state.temp_fan3_mode.clone(), state.edit_state.temp_fan3_level,
                                                  state.edit_state.temp_fan3_rampup.clone(), state.edit_state.temp_fan3_rampdown.clone()),
                                            _ => return,
                                        };
                                        
                                        let state_clone = Arc::clone(&self.state);
                                    tokio::spawn(async move {
                                        // Extract the HTTP client and server URL outside the lock
                                        let (client, server_url) = {
                                            let state_guard = state_clone.lock().unwrap();
                                            (state_guard.http_client.clone(), state_guard.server_url())
                                        };
                                        
                                        let mut success = true;
                                        let mut error_msg = None;
                                        
                                        // Set fan mode
                                        if success {
                                            let url = format!("{}/fan{}/mode", server_url, fan_id);
                                            let request = FanModeRequest {
                                                mode: mode.clone(),
                                            };
                                            
                                            match client.post(&url).json(&request).send().await {
                                                Ok(response) if response.status().is_success() => {},
                                                Ok(response) => {
                                                    success = false;
                                                    error_msg = Some(format!("Failed to set fan mode: {}", response.status()));
                                                }
                                                Err(e) => {
                                                    success = false;
                                                    error_msg = Some(format!("Failed to set fan mode: {}", e));
                                                }
                                            }
                                        }
                                        
                                        // Set level if in fixed mode and previous call succeeded
                                        if success && mode == "fixed" {
                                            let url = format!("{}/fan{}/level", server_url, fan_id);
                                            let request = FanLevelRequest { level };
                                            
                                            match client.post(&url).json(&request).send().await {
                                                Ok(response) if response.status().is_success() => {},
                                                Ok(response) => {
                                                    success = false;
                                                    error_msg = Some(format!("Failed to set fan level: {}", response.status()));
                                                }
                                                Err(e) => {
                                                    success = false;
                                                    error_msg = Some(format!("Failed to set fan level: {}", e));
                                                }
                                            }
                                        }
                                        
                                        // Set curves if in curve mode and previous calls succeeded
                                        if success && mode == "curve" {
                                            // Parse curves
                                            let rampup_curve: Vec<i32> = rampup_str
                                                .split(',')
                                                .filter_map(|s| s.trim().parse().ok())
                                                .collect();
                                            let rampdown_curve: Vec<i32> = rampdown_str
                                                .split(',')
                                                .filter_map(|s| s.trim().parse().ok())
                                                .collect();
                                            
                                            if rampup_curve.len() != 5 {
                                                success = false;
                                                error_msg = Some("Rampup curve must have exactly 5 values".to_string());
                                            } else if rampdown_curve.len() != 5 {
                                                success = false;
                                                error_msg = Some("Rampdown curve must have exactly 5 values".to_string());
                                            } else {
                                                // Set rampup curve
                                                let url = format!("{}/fan{}/rampup_curve", server_url, fan_id);
                                                let request = FanCurveRequest { curve: rampup_curve };
                                                
                                                match client.post(&url).json(&request).send().await {
                                                    Ok(response) if response.status().is_success() => {},
                                                    Ok(response) => {
                                                        success = false;
                                                        error_msg = Some(format!("Failed to set rampup curve: {}", response.status()));
                                                    }
                                                    Err(e) => {
                                                        success = false;
                                                        error_msg = Some(format!("Failed to set rampup curve: {}", e));
                                                    }
                                                }
                                                
                                                // Set rampdown curve if rampup succeeded
                                                if success {
                                                    let url = format!("{}/fan{}/rampdown_curve", server_url, fan_id);
                                                    let request = FanCurveRequest { curve: rampdown_curve };
                                                    
                                                    match client.post(&url).json(&request).send().await {
                                                        Ok(response) if response.status().is_success() => {},
                                                        Ok(response) => {
                                                            success = false;
                                                            error_msg = Some(format!("Failed to set rampdown curve: {}", response.status()));
                                                        }
                                                        Err(e) => {
                                                            success = false;
                                                            error_msg = Some(format!("Failed to set rampdown curve: {}", e));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        
                                        // Update UI state
                                        if success {
                                            // Refresh metrics immediately after successful change
                                            let metrics_url = format!("{}/metrics", server_url);
                                            if let Ok(metrics_response) = client.get(&metrics_url).send().await {
                                                if let Ok(metrics) = metrics_response.json::<MetricsResponse>().await {
                                                    let mut state_guard = state_clone.lock().unwrap();
                                                    state_guard.metrics = Some(metrics);
                                                    state_guard.last_update = Some(Instant::now());
                                                    match fan_id {
                                                        1 => {
                                                            state_guard.edit_state.fan1_edit_mode = false;
                                                            state_guard.edit_state.fan1_applying = false;
                                                        },
                                                        2 => {
                                                            state_guard.edit_state.fan2_edit_mode = false;
                                                            state_guard.edit_state.fan2_applying = false;
                                                        },
                                                        3 => {
                                                            state_guard.edit_state.fan3_edit_mode = false;
                                                            state_guard.edit_state.fan3_applying = false;
                                                        },
                                                        _ => {}
                                                    }
                                                } else {
                                                    let mut state_guard = state_clone.lock().unwrap();
                                                    match fan_id {
                                                        1 => {
                                                            state_guard.edit_state.fan1_edit_mode = false;
                                                            state_guard.edit_state.fan1_applying = false;
                                                        },
                                                        2 => {
                                                            state_guard.edit_state.fan2_edit_mode = false;
                                                            state_guard.edit_state.fan2_applying = false;
                                                        },
                                                        3 => {
                                                            state_guard.edit_state.fan3_edit_mode = false;
                                                            state_guard.edit_state.fan3_applying = false;
                                                        },
                                                        _ => {}
                                                    }
                                                }
                                            } else {
                                                let mut state_guard = state_clone.lock().unwrap();
                                                match fan_id {
                                                    1 => {
                                                        state_guard.edit_state.fan1_edit_mode = false;
                                                        state_guard.edit_state.fan1_applying = false;
                                                    },
                                                    2 => {
                                                        state_guard.edit_state.fan2_edit_mode = false;
                                                        state_guard.edit_state.fan2_applying = false;
                                                    },
                                                    3 => {
                                                        state_guard.edit_state.fan3_edit_mode = false;
                                                        state_guard.edit_state.fan3_applying = false;
                                                    },
                                                    _ => {}
                                                }
                                            }
                                        } else if let Some(msg) = error_msg {
                                            let mut state_guard = state_clone.lock().unwrap();
                                            state_guard.set_error(msg);
                                            match fan_id {
                                                1 => state_guard.edit_state.fan1_applying = false,
                                                2 => state_guard.edit_state.fan2_applying = false,
                                                3 => state_guard.edit_state.fan3_applying = false,
                                                _ => {}
                                            }
                                            // Don't clear edit mode on error, let user see the error and try again
                                        }
                                        });
                                    } else {
                                        // Enter edit mode
                                        match fan_id {
                                            1 => {
                                                state.edit_state.fan1_edit_mode = true;
                                                state.edit_state.temp_fan1_mode = fan.mode.clone();
                                                state.edit_state.temp_fan1_level = fan.level;
                                                state.edit_state.temp_fan1_rampup = state.curve_to_string(&fan.rampup_curve);
                                                state.edit_state.temp_fan1_rampdown = state.curve_to_string(&fan.rampdown_curve);
                                            }
                                            2 => {
                                                state.edit_state.fan2_edit_mode = true;
                                                state.edit_state.temp_fan2_mode = fan.mode.clone();
                                                state.edit_state.temp_fan2_level = fan.level;
                                                state.edit_state.temp_fan2_rampup = state.curve_to_string(&fan.rampup_curve);
                                                state.edit_state.temp_fan2_rampdown = state.curve_to_string(&fan.rampdown_curve);
                                            }
                                            3 => {
                                                state.edit_state.fan3_edit_mode = true;
                                                state.edit_state.temp_fan3_mode = fan.mode.clone();
                                                state.edit_state.temp_fan3_level = fan.level;
                                                state.edit_state.temp_fan3_rampup = state.curve_to_string(&fan.rampup_curve);
                                                state.edit_state.temp_fan3_rampdown = state.curve_to_string(&fan.rampdown_curve);
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                            }
                        }
                    });
                });
                
                let is_edit_mode = match fan_id {
                    1 => state.edit_state.fan1_edit_mode,
                    2 => state.edit_state.fan2_edit_mode,
                    3 => state.edit_state.fan3_edit_mode,
                    _ => false,
                };
                
                if is_edit_mode {
                    // Edit mode UI
                    match fan_id {
                        1 => {
                            ui.horizontal(|ui| {
                                ui.label("Mode:");
                                egui::ComboBox::from_label("")
                                    .selected_text(&state.edit_state.temp_fan1_mode)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut state.edit_state.temp_fan1_mode, "auto".to_string(), "auto");
                                        ui.selectable_value(&mut state.edit_state.temp_fan1_mode, "fixed".to_string(), "fixed");
                                        ui.selectable_value(&mut state.edit_state.temp_fan1_mode, "curve".to_string(), "curve");
                                    });
                            });
                            
                            if state.edit_state.temp_fan1_mode == "fixed" {
                                ui.horizontal(|ui| {
                                    ui.label("Level:");
                                    ui.add(egui::Slider::new(&mut state.edit_state.temp_fan1_level, 0..=5));
                                });
                            }
                            
                            if state.edit_state.temp_fan1_mode == "curve" {
                                ui.horizontal(|ui| {
                                    ui.label("Ramp-Up:");
                                    ui.text_edit_singleline(&mut state.edit_state.temp_fan1_rampup);
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Ramp-Down:");
                                    ui.text_edit_singleline(&mut state.edit_state.temp_fan1_rampdown);
                                });
                                ui.label(egui::RichText::new("Hint: 5 temperature thresholds (°C) that trigger fan level increases (Ramp-Up) or decreases (Ramp-Down), comma separated.").weak());
                            }
                        }
                        2 => {
                            ui.horizontal(|ui| {
                                ui.label("Mode:");
                                egui::ComboBox::from_label("")
                                    .selected_text(&state.edit_state.temp_fan2_mode)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut state.edit_state.temp_fan2_mode, "auto".to_string(), "auto");
                                        ui.selectable_value(&mut state.edit_state.temp_fan2_mode, "fixed".to_string(), "fixed");
                                        ui.selectable_value(&mut state.edit_state.temp_fan2_mode, "curve".to_string(), "curve");
                                    });
                            });
                            
                            if state.edit_state.temp_fan2_mode == "fixed" {
                                ui.horizontal(|ui| {
                                    ui.label("Level:");
                                    ui.add(egui::Slider::new(&mut state.edit_state.temp_fan2_level, 0..=5));
                                });
                            }
                            
                            if state.edit_state.temp_fan2_mode == "curve" {
                                ui.horizontal(|ui| {
                                    ui.label("Ramp-Up:");
                                    ui.text_edit_singleline(&mut state.edit_state.temp_fan2_rampup);
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Ramp-Down:");
                                    ui.text_edit_singleline(&mut state.edit_state.temp_fan2_rampdown);
                                });
                                ui.label(egui::RichText::new("Hint: 5 temperature thresholds (°C) that trigger fan level increases (Ramp-Up) or decreases (Ramp-Down), comma separated.").weak());
                            }
                        }
                        3 => {
                            ui.horizontal(|ui| {
                                ui.label("Mode:");
                                egui::ComboBox::from_label("")
                                    .selected_text(&state.edit_state.temp_fan3_mode)
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut state.edit_state.temp_fan3_mode, "auto".to_string(), "auto");
                                        ui.selectable_value(&mut state.edit_state.temp_fan3_mode, "fixed".to_string(), "fixed");
                                        ui.selectable_value(&mut state.edit_state.temp_fan3_mode, "curve".to_string(), "curve");
                                    });
                            });
                            
                            if state.edit_state.temp_fan3_mode == "fixed" {
                                ui.horizontal(|ui| {
                                    ui.label("Level:");
                                    ui.add(egui::Slider::new(&mut state.edit_state.temp_fan3_level, 0..=5));
                                });
                            }
                            
                            if state.edit_state.temp_fan3_mode == "curve" {
                                ui.horizontal(|ui| {
                                    ui.label("Ramp-Up:");
                                    ui.text_edit_singleline(&mut state.edit_state.temp_fan3_rampup);
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Ramp-Down:");
                                    ui.text_edit_singleline(&mut state.edit_state.temp_fan3_rampdown);
                                });
                                ui.label(egui::RichText::new("Hint: 5 temperature thresholds (°C) that trigger fan level increases (Ramp-Up) or decreases (Ramp-Down), comma separated.").weak());
                            }
                        }
                        _ => {}
                    }
                } else {
                    // Display mode UI
                    ui.horizontal(|ui| {
                        ui.label("Mode:");
                        ui.colored_label(state.get_mode_color(&fan.mode), &fan.mode);
                    });

                    ui.horizontal(|ui| {
                        ui.label("RPM:");
                        ui.colored_label(state.get_rpm_color(fan.rpm), format!("{}", fan.rpm));
                    });

                    if fan.mode == "fixed" || fan.mode == "curve" {
                        ui.horizontal(|ui| {
                            ui.label("Level:");
                            ui.label(format!("{}", fan.level));
                        });
                    }

                    if fan.mode == "curve" {
                        ui.horizontal(|ui| {
                            ui.label("Ramp-Up:");
                            ui.label(format!("{:?}", fan.rampup_curve));
                        });
                        ui.horizontal(|ui| {
                            ui.label("Ramp-Down:");
                            ui.label(format!("{:?}", fan.rampdown_curve));
                       });
                   }
               }
           })
       });
       
       // Draw chart in the background after content is drawn, only if not in edit mode
       if !is_edit_mode {
           let mut rect = response.response.rect;
           rect.set_width(ui.available_width());
           self.draw_bar_chart(
               ui,
               rect,
               &history_clone,
               max_rpm,
               state.get_rpm_color(fan.rpm),
           );
       }
   }
}

impl eframe::App for EcMonitorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Start metrics polling when the app starts
        self.start_metrics_polling();

        let mut content_height = 0.0;
        
        egui::CentralPanel::default().show(ctx, |ui| {
            // Load icons first
            {
                let mut state = self.state.lock().unwrap();
                state.load_icons(ctx);
            }

            let mut state = self.state.lock().unwrap();

            // Clear old error messages
            state.clear_old_error();

            // Track the starting position
            let start_y = ui.cursor().top();

            // EC Firmware version
            if let Some(version) = &state.ec_version {
                ui.horizontal(|ui| {
                    ui.label("EC firmware version:");
                    ui.label(version);
                });
                ui.separator();
            }

            // Error message
            if let Some(error) = &state.error_message {
                ui.colored_label(egui::Color32::RED, format!("Error: {}", error));
                ui.separator();
            }

            // Metrics display
            if let Some(metrics) = state.metrics.clone() {
                // APU block
                self.draw_apu_block(ui, &metrics, &mut state);

                ui.separator();

                // Fan blocks in a vertical layout to ensure all are visible
                self.draw_fan_block_with_edit(ui, "Fan1", 1, &metrics.fan1, &mut state);
                self.draw_fan_block_with_edit(ui, "Fan2", 2, &metrics.fan2, &mut state);
                self.draw_fan_block_with_edit(ui, "Fan3", 3, &metrics.fan3, &mut state);

            } else {
                ui.label("Loading metrics...");
            }

            // Calculate content height
            let end_y = ui.cursor().top();
            content_height = end_y - start_y + 15.0; // Add some padding
        });

        // Configure window size and position
        let window_width = 400.0;
        let min_height = 200.0;
        let max_height = 800.0;
        
        // Clamp the content height to reasonable bounds
        let target_height = content_height.max(min_height).min(max_height);
        
        // Only update window size if content height changed significantly (avoid constant resizing)
        if !self.window_configured || (target_height - self.last_content_height).abs() > 5.0 {
            let window_size = egui::Vec2::new(window_width, target_height);
            
            // Get screen dimensions for centering
            let screen_size = {
                #[cfg(windows)]
                {
                    use winapi::um::winuser::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
                    unsafe {
                        let width = GetSystemMetrics(SM_CXSCREEN) as f32;
                        let height = GetSystemMetrics(SM_CYSCREEN) as f32;
                        [width, height]
                    }
                }
                #[cfg(not(windows))]
                {
                    [1920.0, 1080.0] // Default fallback
                }
            };
            
            // Calculate center position
            let center_x = (screen_size[0] - window_size.x) / 2.0;
            let center_y = (screen_size[1] - window_size.y) / 2.0;
            let window_pos = egui::Pos2::new(center_x, center_y);
            
            // Set viewport properties
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(window_size));
            if !self.window_configured {
                // Only set position on first configuration to avoid jumping
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(window_pos));
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(false));
            
            self.last_content_height = target_height;
            self.window_configured = true;
        }

        // Request repaint every second to update metrics
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_metrics_polling();
    }
}

// Configuration management
fn get_config_path() -> Result<PathBuf> {
    let config_dir = config_dir().context("Failed to get config directory")?;
    Ok(config_dir.join("ec-su_axb35-win").join("client.json"))
}

fn load_config() -> Result<(Config, bool)> {
    let config_path = get_config_path()?;
    
    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)
            .context("Failed to read config file")?;
        let config: Config = serde_json::from_str(&content)
            .context("Failed to parse config file")?;
        Ok((config, true))
    } else {
        Ok((Config::default(), false))
    }
}

fn save_config(config: &Config) -> Result<()> {
    let config_path = get_config_path()?;
    
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .context("Failed to create config directory")?;
    }
    
    let content = serde_json::to_string_pretty(config)
        .context("Failed to serialize config")?;
    std::fs::write(&config_path, content)
        .context("Failed to write config file")?;
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load or create configuration
    let (config, config_existed) = load_config()?;
    
    // Save default config if it didn't exist
    if !config_existed {
        save_config(&config)?;
    }

    // Create application state
    let mut app_state = AppState::new(config);
    
    // Check server status
    if let Err(e) = app_state.check_status().await {
        eprintln!("Failed to connect to server: {}", e);
        #[cfg(windows)]
        {
            use winapi::um::winuser::{MessageBoxA, MB_OK, MB_ICONERROR};
            use std::ffi::CString;
            
            let title = CString::new("EC Monitor Error").unwrap();
            let message = CString::new(format!("Server couldn't be reached: {}", e)).unwrap();
            
            unsafe {
                MessageBoxA(
                    std::ptr::null_mut(),
                    message.as_ptr(),
                    title.as_ptr(),
                    MB_OK | MB_ICONERROR,
                );
            }
        }
        return Err(e);
    }

    let state = Arc::new(Mutex::new(app_state));
    
    // Create the application
    let app = EcMonitorApp::new(Arc::clone(&state));
    
    // Load and configure the window icon
    let icon_data = match image::load_from_memory(ICON_BYTES) {
        Ok(img) => {
            let rgba = img.to_rgba8();
            let (width, height) = img.dimensions();
            Some(egui::IconData {
                rgba: rgba.into_raw(),
                width: width as u32,
                height: height as u32,
            })
        }
        Err(e) => {
            eprintln!("Failed to load icon: {}", e);
            None
        }
    };

    // Create and run the application
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("EC SU_AXB35 Client")
            .with_maximize_button(false)
            .with_icon(icon_data.unwrap_or_default()),
        ..Default::default()
    };

    eframe::run_native(
        "EC Monitor",
        options,
        Box::new(move |_cc| {
            Ok(Box::new(app))
        }),
    ).map_err(|e| anyhow::anyhow!("Failed to run application: {}", e))?;

    Ok(())
}