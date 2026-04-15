use std::fmt;
use std::io::Write;

use log::{LevelFilter, error, info, warn};

pub fn init(level: LevelFilter) {
    env_logger::Builder::new()
        .filter_level(level)
        .format_timestamp_secs()
        .format_module_path(false)
        .format_target(false)
        .format(|buf, record| writeln!(buf, "[{}] {}", record.level(), record.args()))
        .init();
}

#[derive(Debug, Clone)]
pub struct ProgressLogger {
    component: &'static str,
    operation: &'static str,
    subject: String,
}

impl ProgressLogger {
    pub fn new(
        component: &'static str,
        operation: &'static str,
        subject: impl Into<String>,
    ) -> Self {
        Self {
            component,
            operation,
            subject: subject.into(),
        }
    }

    pub fn start(&self, message: impl fmt::Display) {
        info!("{}", self.format_message("start", message));
    }

    pub fn step(&self, message: impl fmt::Display) {
        info!("{}", self.format_message("step", message));
    }

    pub fn skip(&self, message: impl fmt::Display) {
        info!("{}", self.format_message("skip", message));
    }

    pub fn success(&self, message: impl fmt::Display) {
        info!("{}", self.format_message("done", message));
    }

    pub fn warn(&self, message: impl fmt::Display) {
        warn!("{}", self.format_message("warn", message));
    }

    pub fn failed(&self, message: impl fmt::Display) {
        error!("{}", self.format_message("failed", message));
    }

    fn format_message(&self, phase: &'static str, message: impl fmt::Display) -> String {
        format!(
            "[{} {}:{}] {}: {}",
            self.component, self.operation, self.subject, phase, message
        )
    }
}

#[cfg(test)]
mod tests {
    use super::ProgressLogger;

    #[test]
    fn formats_progress_messages_with_component_operation_and_subject() {
        let logger = ProgressLogger::new("san", "snapshot", "SAN01");

        assert_eq!(
            logger.format_message("step", "creating ONTAP snapshot"),
            "[san snapshot:SAN01] step: creating ONTAP snapshot"
        );
    }
}
