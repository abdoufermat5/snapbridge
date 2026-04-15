use async_trait::async_trait;
use serde_json::{Value, json};
use urlencoding::encode;

use crate::config::SharedStorageConfig;
use crate::error::{AppError, Result};
use crate::models::{
    FileCloneRequest, FlexCloneRequest, OntapLun, OntapLunMap, OntapSnapshot, OntapVolume,
};

use super::api::OntapApi;
use super::http::OntapHttpClient;
use super::types::{LunRecord, SnapshotRecord, VolumeRecord};

#[derive(Clone)]
pub struct ReqwestOntapClient {
    http: OntapHttpClient,
}

impl ReqwestOntapClient {
    pub fn new(config: &SharedStorageConfig) -> Result<Self> {
        Ok(Self {
            http: OntapHttpClient::new(config)?,
        })
    }
}

#[async_trait]
impl OntapApi for ReqwestOntapClient {
    async fn get_volume_by_name(&self, name: &str) -> Result<OntapVolume> {
        let mut volumes: Vec<VolumeRecord> = self
            .http
            .get_records(&format!(
                "/storage/volumes?name={}&fields=uuid,name,svm,nas,clone",
                encode(name)
            ))
            .await?;
        let volume = volumes
            .pop()
            .ok_or_else(|| AppError::Missing(format!("ONTAP volume `{name}` not found")))?;
        Ok(volume.into())
    }

    async fn get_volume_detail(&self, uuid: &str) -> Result<Value> {
        self.http
            .get_json(&format!("/storage/volumes/{}", encode(uuid)))
            .await
    }

    async fn list_snapshots(&self, volume_uuid: &str) -> Result<Vec<OntapSnapshot>> {
        let snapshots: Vec<SnapshotRecord> = self
            .http
            .get_records(&format!(
                "/storage/volumes/{}/snapshots?fields=uuid,name,comment",
                encode(volume_uuid)
            ))
            .await?;
        Ok(snapshots.into_iter().map(Into::into).collect())
    }

    async fn create_snapshot(&self, volume: &OntapVolume, name: &str, comment: &str) -> Result<()> {
        self.http
            .post_json(
                &format!("/storage/volumes/{}/snapshots", encode(&volume.uuid)),
                json!({
                    "name": name,
                    "comment": comment,
                }),
            )
            .await
    }

    async fn restore_snapshot(&self, volume: &OntapVolume, snapshot: &str) -> Result<()> {
        self.http
            .post_json(
                "/private/cli/volume/snapshot/restore",
                json!({
                    "vserver": volume.svm_name,
                    "volume": volume.name,
                    "snapshot": snapshot,
                    "force": true,
                }),
            )
            .await
    }

    async fn delete_snapshot(&self, volume_uuid: &str, snapshot_name: &str) -> Result<()> {
        let snapshot = self
            .list_snapshots(volume_uuid)
            .await?
            .into_iter()
            .find(|snapshot| snapshot.name == snapshot_name)
            .ok_or_else(|| AppError::Missing(format!("snapshot `{snapshot_name}` not found")))?;

        self.http
            .delete(&format!(
                "/storage/volumes/{}/snapshots/{}",
                encode(volume_uuid),
                encode(&snapshot.uuid)
            ))
            .await
    }

    async fn clone_file(&self, request: &FileCloneRequest) -> Result<()> {
        self.http
            .post_json(
                "/storage/file/clone",
                json!({
                    "volume": {
                        "name": request.volume_name,
                        "uuid": request.volume_uuid,
                    },
                    "source_path": request.source_path,
                    "destination_path": request.destination_path,
                    "overwrite_destination": request.overwrite_destination,
                }),
            )
            .await
    }

    async fn create_flexclone(&self, request: &FlexCloneRequest) -> Result<()> {
        self.http
            .post_json(
                "/storage/volumes",
                json!({
                    "name": request.clone_name,
                    "svm": {"name": request.svm_name},
                    "clone": {
                        "parent_volume": {"name": request.parent_volume_name},
                        "parent_snapshot": {"name": request.parent_snapshot_name},
                        "is_flexclone": true,
                        "type": "rw"
                    },
                    "nas": {"path": request.nas_path}
                }),
            )
            .await
    }

    async fn delete_volume(&self, uuid: &str, force: bool) -> Result<()> {
        self.http
            .delete(&format!("/storage/volumes/{}?force={force}", encode(uuid)))
            .await
    }

    async fn get_lun_by_name(&self, path: &str) -> Result<OntapLun> {
        let mut luns: Vec<LunRecord> = self
            .http
            .get_records(&format!(
                "/storage/luns?name={}&fields=uuid,name",
                encode(path)
            ))
            .await?;
        let lun = luns
            .pop()
            .ok_or_else(|| AppError::Missing(format!("ONTAP LUN `{path}` not found")))?;
        Ok(lun.into())
    }

    async fn get_lun_detail(&self, uuid: &str) -> Result<Value> {
        self.http
            .get_json(&format!("/storage/luns/{}", encode(uuid)))
            .await
    }

    async fn list_lun_maps(&self) -> Result<Vec<OntapLunMap>> {
        let records: Vec<Value> = self
            .http
            .get_records("/protocols/san/lun-maps?fields=lun,igroup")
            .await?;

        Ok(records
            .into_iter()
            .map(|raw| OntapLunMap {
                lun_name: raw
                    .get("lun")
                    .and_then(|value| value.get("name"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                igroup_name: raw
                    .get("igroup")
                    .and_then(|value| value.get("name"))
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                raw,
            })
            .collect())
    }

    async fn get_igroup_detail(&self, name: &str) -> Result<Value> {
        let mut records: Vec<Value> = self
            .http
            .get_records(&format!("/protocols/san/igroups?name={}", encode(name)))
            .await?;
        records
            .pop()
            .ok_or_else(|| AppError::Missing(format!("ONTAP igroup `{name}` not found")))
    }
}
