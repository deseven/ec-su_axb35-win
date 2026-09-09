//! Metrics module for polling the EC server API

use serde::Deserialize;
use std::time::Duration;
use tokio::sync::watch;
use tokio::time::interval;

/// Fan metrics from the API
#[derive(Debug, Deserialize, Clone, Default)]
pub struct FanMetrics {
    pub mode: String,
    pub level: i32,
    pub rpm: i32,
    pub rampup_curve: Vec<i32>,
    pub rampdown_curve: Vec<i32>,
}

/// Combined metrics response from /metrics endpoint
#[derive(Debug, Deserialize, Clone, Default)]
pub struct MetricsResponse {
    pub power_mode: String,
    pub temperature: i32,
    pub fan1: FanMetrics,
    pub fan2: FanMetrics,
    pub fan3: FanMetrics,
}

/// Current state of the metrics
#[derive(Debug, Clone)]
pub struct MetricsState {
    pub metrics: Option<MetricsResponse>,
    pub connected: bool,
    pub error: Option<String>,
}

impl Default for MetricsState {
    fn default() -> Self {
        Self {
            metrics: None,
            connected: false,
            error: None,
        }
    }
}

/// Start the metrics polling loop
pub fn start_polling(
    server_url: String,
    tx: watch::Sender<MetricsState>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("Failed to create HTTP client");
        
        let mut poll_interval = interval(Duration::from_secs(1));
        
        loop {
            poll_interval.tick().await;
            
            let url = format!("{}/metrics", server_url);
            
            match client.get(&url).send().await {
                Ok(response) => {
                    match response.json::<MetricsResponse>().await {
                        Ok(metrics) => {
                            let _ = tx.send(MetricsState {
                                metrics: Some(metrics),
                                connected: true,
                                error: None,
                            });
                        }
                        Err(e) => {
                            let _ = tx.send(MetricsState {
                                metrics: None,
                                connected: false,
                                error: Some(format!("Parse error: {}", e)),
                            });
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(MetricsState {
                        metrics: None,
                        connected: false,
                        error: Some(format!("Connection error: {}", e)),
                    });
                }
            }
        }
    })
}

/// Format RPM value for display (compact format: "2.4K")
pub fn format_rpm(rpm: i32) -> String {
    if rpm >= 1000 {
        format!("{:.1}K", rpm as f32 / 1000.0)
    } else {
        format!("{}", rpm)
    }
}

/// Format temperature for display
pub fn format_temp(temp: i32) -> String {
    format!("{}°", temp)
}

/// Get color based on temperature thresholds
pub fn temp_color(temp: i32) -> (u8, u8, u8) {
    if temp <= 50 {
        (76, 175, 80)    // Green
    } else if temp <= 85 {
        (255, 193, 7)    // Yellow/Amber
    } else {
        (244, 67, 54)    // Red
    }
}

/// Get color based on RPM thresholds
pub fn rpm_color(rpm: i32) -> (u8, u8, u8) {
    if rpm <= 1250 {
        (76, 175, 80)    // Green (low/quiet)
    } else if rpm <= 2500 {
        (255, 193, 7)    // Yellow/Amber (moderate)
    } else {
        (244, 67, 54)    // Red (high)
    }
}

/// Gray color for offline/error state
pub fn offline_color() -> (u8, u8, u8) {
    (128, 128, 128)  // Gray
}
