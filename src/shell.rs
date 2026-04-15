use async_trait::async_trait;
use tokio::process::Command;

use crate::error::{AppError, Result};

#[async_trait]
pub trait ShellRunner: Send + Sync {
    async fn ssh(&self, host: &str, user: &str, command: &str) -> Result<()>;
}

#[derive(Debug, Clone, Copy)]
pub struct TokioShellRunner;

#[async_trait]
impl ShellRunner for TokioShellRunner {
    async fn ssh(&self, host: &str, user: &str, command: &str) -> Result<()> {
        let output = Command::new("ssh")
            .args([
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-o",
                "ConnectTimeout=10",
                &format!("{user}@{host}"),
                "--",
                command,
            ])
            .output()
            .await?;

        if !output.status.success() {
            return Err(AppError::CommandFailed(format!(
                "ssh {user}@{host} -- {command} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        Ok(())
    }
}
