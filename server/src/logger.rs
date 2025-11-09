use std::fs::{File, OpenOptions};
use std::io::{Write, BufWriter};
use std::path::Path;
use chrono::Utc;

pub struct Logger {
    file_writer: Option<BufWriter<File>>,
    service_mode: bool,
}

impl Logger {
    pub fn new(log_path: &str, service_mode: bool) -> Result<Self, String> {
        // Create directory if it doesn't exist
        if let Some(parent) = Path::new(log_path).parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create log directory: {}", e))?;
            }
        }

        // Open log file (overwrite existing)
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(log_path)
            .map_err(|e| format!("Failed to open log file {}: {}", log_path, e))?;

        let file_writer = BufWriter::new(file);

        Ok(Logger {
            file_writer: Some(file_writer),
            service_mode,
        })
    }

    fn log_message(&mut self, level: &str, message: &str) {
        let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
        let log_line = format!("[{}] {}: {}", timestamp, level, message);

        // Write to stdout only if not in service mode
        if !self.service_mode {
            println!("{}", log_line);
        }

        // Write to file
        if let Some(ref mut writer) = self.file_writer {
            if let Err(e) = writeln!(writer, "{}", log_line) {
                eprintln!("Failed to write to log file: {}", e);
            } else if let Err(e) = writer.flush() {
                eprintln!("Failed to flush log file: {}", e);
            }
        }
    }

    pub fn info(&mut self, message: &str) {
        self.log_message("INFO", message);
    }

    pub fn warn(&mut self, message: &str) {
        self.log_message("WARN", message);
    }

    pub fn error(&mut self, message: &str) {
        self.log_message("ERROR", message);
    }

    pub fn debug(&mut self, message: &str) {
        self.log_message("DEBUG", message);
    }
}

impl Drop for Logger {
    fn drop(&mut self) {
        if let Some(ref mut writer) = self.file_writer {
            let _ = writer.flush();
        }
    }
}