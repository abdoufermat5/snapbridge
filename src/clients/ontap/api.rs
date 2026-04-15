use async_trait::async_trait;
use serde_json::Value;

use crate::error::Result;
use crate::models::{
    FileCloneRequest, FlexCloneRequest, OntapLun, OntapLunMap, OntapSnapshot, OntapVolume,
};

#[async_trait]
pub trait OntapApi: Send + Sync {
    async fn get_volume_by_name(&self, name: &str) -> Result<OntapVolume>;
    async fn get_volume_detail(&self, uuid: &str) -> Result<Value>;
    async fn list_snapshots(&self, volume_uuid: &str) -> Result<Vec<OntapSnapshot>>;
    async fn create_snapshot(&self, volume: &OntapVolume, name: &str, comment: &str) -> Result<()>;
    async fn restore_snapshot(&self, volume: &OntapVolume, snapshot: &str) -> Result<()>;
    async fn delete_snapshot(&self, volume_uuid: &str, snapshot_name: &str) -> Result<()>;
    async fn clone_file(&self, request: &FileCloneRequest) -> Result<()>;
    async fn create_flexclone(&self, request: &FlexCloneRequest) -> Result<()>;
    async fn delete_volume(&self, uuid: &str, force: bool) -> Result<()>;
    async fn get_lun_by_name(&self, path: &str) -> Result<OntapLun>;
    async fn get_lun_detail(&self, uuid: &str) -> Result<Value>;
    async fn list_lun_maps(&self) -> Result<Vec<OntapLunMap>>;
    async fn get_igroup_detail(&self, name: &str) -> Result<Value>;
}
