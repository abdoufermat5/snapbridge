use async_trait::async_trait;
use serde_json::Value;

use crate::error::Result;
use crate::models::{Node, ProxmoxStorage, TaskStatus, VmConfig, VmStatus, VmSummary};

#[async_trait]
pub trait ProxmoxApi: Send + Sync {
    async fn list_nodes(&self) -> Result<Vec<Node>>;
    async fn list_vms(&self, node: &str) -> Result<Vec<VmSummary>>;
    async fn get_vm_status(&self, node: &str, vmid: u32) -> Result<VmStatus>;
    async fn get_vm_config(&self, node: &str, vmid: u32) -> Result<VmConfig>;
    async fn shutdown_vm(&self, node: &str, vmid: u32) -> Result<String>;
    async fn suspend_vm(&self, node: &str, vmid: u32) -> Result<String>;
    async fn start_vm(&self, node: &str, vmid: u32) -> Result<String>;
    async fn get_task_status(&self, node: &str, upid: &str) -> Result<TaskStatus>;
    async fn create_vm_snapshot(
        &self,
        node: &str,
        vmid: u32,
        snapname: &str,
        description: &str,
    ) -> Result<String>;
    async fn delete_vm_snapshot(&self, node: &str, vmid: u32, snapname: &str) -> Result<String>;
    async fn run_guest_agent_command(&self, node: &str, vmid: u32, command: &str) -> Result<Value>;
    async fn get_storage(&self, storage: &str) -> Result<ProxmoxStorage>;
    async fn create_storage(&self, storage: &str, definition: &ProxmoxStorage) -> Result<()>;
    async fn delete_storage(&self, storage: &str) -> Result<()>;
}
