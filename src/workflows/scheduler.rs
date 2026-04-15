use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::clients::ontap::OntapApi;
use crate::clients::proxmox::ProxmoxApi;
use crate::config::{LoadedConfig, RetentionPolicy, ScheduleConfig, StorageConfig};
use crate::display::{OutputFormat, Table};
use crate::error::{AppError, Result};
use crate::logger::ProgressLogger;
use crate::models::{OntapSnapshot, OntapVolume};
use crate::workflows::{nas, san};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleAction {
    Run,
    Create,
    Delete,
}

pub async fn execute_schedule<P, O, F>(
    config: &LoadedConfig,
    proxmox: &P,
    schedule_name: &str,
    action: ScheduleAction,
    ontap_factory: F,
) -> Result<()>
where
    P: ProxmoxApi,
    O: OntapApi,
    F: Fn(&str) -> Result<O>,
{
    let schedule = config.schedule(schedule_name)?;
    if schedule.storages.is_empty() {
        return Err(AppError::Config(format!(
            "schedule `{schedule_name}` has no storages"
        )));
    }

    let retention = schedule.retention_policy()?;
    if action == ScheduleAction::Delete && retention.is_none() {
        return Err(AppError::Config(format!(
            "schedule `{schedule_name}` has no retention policy"
        )));
    }

    let progress = ProgressLogger::new("schedule", action.name(), schedule_name);
    progress.start(format!(
        "executing schedule for {} storage(s)",
        schedule.storages.len()
    ));

    let mut failures = Vec::new();
    for storage_id in &schedule.storages {
        let created = if action.creates_snapshots() {
            match create_for_storage(config, proxmox, storage_id, schedule, &ontap_factory).await {
                Ok(()) => true,
                Err(error) => {
                    progress.warn(format!("create failed for storage `{storage_id}`: {error}"));
                    failures.push(format!("{storage_id}: create failed: {error}"));
                    false
                }
            }
        } else {
            true
        };

        if action.applies_retention() {
            let Some(policy) = retention else {
                progress.skip(format!(
                    "no retention policy configured; skipping delete for `{storage_id}`"
                ));
                continue;
            };

            if !created {
                progress.skip(format!(
                    "skipping retention for `{storage_id}` because create failed"
                ));
                continue;
            }

            match apply_retention(config, proxmox, storage_id, &ontap_factory, policy).await {
                Ok(deleted) => progress.step(format!(
                    "deleted {deleted} retained snapshot(s) for `{storage_id}`"
                )),
                Err(error) => {
                    progress.warn(format!(
                        "retention failed for storage `{storage_id}`: {error}"
                    ));
                    failures.push(format!("{storage_id}: retention failed: {error}"));
                }
            }
        }
    }

    if failures.is_empty() {
        progress.success("schedule completed");
        Ok(())
    } else {
        progress.failed(format!(
            "schedule completed with {} failure(s)",
            failures.len()
        ));
        Err(AppError::CommandFailed(format!(
            "schedule `{schedule_name}` completed with failures: {}",
            failures.join("; ")
        )))
    }
}

pub fn print_schedules(format: OutputFormat, config: &LoadedConfig) -> Result<()> {
    let rows = config
        .schedule
        .iter()
        .map(|(name, schedule)| ScheduleRow {
            name: name.clone(),
            storages: schedule.storages.join(","),
            fsfreeze: schedule.fsfreeze,
            keep_last: schedule.keep_last,
            max_age: schedule.max_age.clone().unwrap_or_default(),
        })
        .collect::<Vec<_>>();

    match format {
        OutputFormat::Table => {
            let mut table = Table::new(["Name", "Storages", "Fsfreeze", "Keep Last", "Max Age"])
                .with_empty_message("No schedules configured.");
            for row in rows {
                table.add_row([
                    row.name,
                    row.storages,
                    row.fsfreeze.to_string(),
                    row.keep_last
                        .map(|value| value.to_string())
                        .unwrap_or_default(),
                    row.max_age,
                ]);
            }
            println!("{}", table.render());
        }
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&rows)?),
    }

    Ok(())
}

async fn create_for_storage<P, O, F>(
    config: &LoadedConfig,
    proxmox: &P,
    storage_id: &str,
    schedule: &ScheduleConfig,
    ontap_factory: &F,
) -> Result<()>
where
    P: ProxmoxApi,
    O: OntapApi,
    F: Fn(&str) -> Result<O>,
{
    let ontap = ontap_factory(storage_id)?;
    match config.storage(storage_id)? {
        StorageConfig::Nas(_) => {
            nas::create_storage_snapshot(config, proxmox, &ontap, storage_id, schedule.fsfreeze)
                .await
        }
        StorageConfig::San(_) => {
            san::create_storage_snapshot(config, proxmox, &ontap, storage_id, schedule.fsfreeze)
                .await
        }
    }
}

async fn apply_retention<P, O, F>(
    config: &LoadedConfig,
    proxmox: &P,
    storage_id: &str,
    ontap_factory: &F,
    policy: RetentionPolicy,
) -> Result<usize>
where
    P: ProxmoxApi,
    O: OntapApi,
    F: Fn(&str) -> Result<O>,
{
    let ontap = ontap_factory(storage_id)?;
    let volume = volume_for_storage(config, proxmox, &ontap, storage_id).await?;
    let snapshots = ontap.list_snapshots(&volume.uuid).await?;
    let now = Utc::now();
    let deletions = retention_deletions(&snapshots, policy, now);

    for snapshot in &deletions {
        ontap.delete_snapshot(&volume.uuid, &snapshot.name).await?;
    }

    Ok(deletions.len())
}

async fn volume_for_storage<P, O>(
    config: &LoadedConfig,
    proxmox: &P,
    ontap: &O,
    storage_id: &str,
) -> Result<OntapVolume>
where
    P: ProxmoxApi,
    O: OntapApi,
{
    match config.storage(storage_id)? {
        StorageConfig::Nas(_) => {
            let volume_name = nas::resolve_nas_volume_name(proxmox, storage_id).await?;
            ontap.get_volume_by_name(&volume_name).await
        }
        StorageConfig::San(settings) => ontap.get_volume_by_name(&settings.volume_name).await,
    }
}

fn retention_deletions(
    snapshots: &[OntapSnapshot],
    policy: RetentionPolicy,
    now: DateTime<Utc>,
) -> Vec<OntapSnapshot> {
    let mut proxsnap_snapshots = snapshots
        .iter()
        .filter_map(|snapshot| {
            parse_snapshot_timestamp(&snapshot.name).map(|created_at| (snapshot, created_at))
        })
        .collect::<Vec<_>>();
    proxsnap_snapshots.sort_by(|(_, left), (_, right)| right.cmp(left));

    proxsnap_snapshots
        .into_iter()
        .enumerate()
        .filter_map(|(index, (snapshot, created_at))| {
            let outside_keep = policy
                .keep_last
                .map(|keep_last| index >= keep_last)
                .unwrap_or(true);
            let older_than_max_age = policy
                .max_age
                .map(|max_age| {
                    now.signed_duration_since(created_at)
                        .to_std()
                        .is_ok_and(|age| age >= max_age)
                })
                .unwrap_or(true);

            (outside_keep && older_than_max_age).then(|| snapshot.clone())
        })
        .collect()
}

fn parse_snapshot_timestamp(name: &str) -> Option<DateTime<Utc>> {
    let timestamp = name.strip_prefix("proxmox_snapshot_")?;
    DateTime::parse_from_str(timestamp, "%Y-%m-%d_%H:%M:%S%z")
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

impl ScheduleAction {
    fn name(self) -> &'static str {
        match self {
            Self::Run => "run",
            Self::Create => "create",
            Self::Delete => "delete",
        }
    }

    fn creates_snapshots(self) -> bool {
        matches!(self, Self::Run | Self::Create)
    }

    fn applies_retention(self) -> bool {
        matches!(self, Self::Run | Self::Delete)
    }
}

#[derive(Debug, Serialize)]
struct ScheduleRow {
    name: String,
    storages: String,
    fsfreeze: bool,
    keep_last: Option<usize>,
    max_age: String,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use chrono::{TimeZone, Utc};
    use serde_json::{Value, json};

    use super::{ScheduleAction, execute_schedule, parse_snapshot_timestamp, retention_deletions};
    use crate::clients::ontap::OntapApi;
    use crate::clients::proxmox::ProxmoxApi;
    use crate::config::{
        LoadedConfig, NasStorageConfig, ProxmoxConfig, RetentionPolicy, SanStorageConfig,
        ScheduleConfig, SharedStorageConfig, StorageConfig,
    };
    use crate::error::{AppError, Result};
    use crate::models::{
        FileCloneRequest, FlexCloneRequest, Node, OntapLun, OntapLunMap, OntapSnapshot,
        OntapVolume, ProxmoxStorage, TaskStatus, VmConfig, VmStatus, VmSummary,
    };

    #[test]
    fn parses_proxsnap_snapshot_timestamp() {
        let parsed = parse_snapshot_timestamp("proxmox_snapshot_2026-04-15_02:15:06+0200")
            .expect("timestamp should parse");

        assert_eq!(parsed, Utc.with_ymd_and_hms(2026, 4, 15, 0, 15, 6).unwrap());
    }

    #[test]
    fn retention_keeps_newest_and_deletes_only_old_snapshots_when_both_limits_apply() {
        let snapshots = vec![
            snapshot("proxmox_snapshot_2026-04-15_00:00:00+0000"),
            snapshot("proxmox_snapshot_2026-04-14_00:00:00+0000"),
            snapshot("proxmox_snapshot_2026-04-13_00:00:00+0000"),
            snapshot("proxmox_snapshot_2026-04-12_00:00:00+0000"),
            snapshot("manual_snapshot"),
        ];
        let deletions = retention_deletions(
            &snapshots,
            RetentionPolicy {
                keep_last: Some(2),
                max_age: Some(Duration::from_secs(2 * 24 * 60 * 60)),
            },
            Utc.with_ymd_and_hms(2026, 4, 16, 0, 0, 0).unwrap(),
        );

        assert_eq!(
            deletions
                .into_iter()
                .map(|snapshot| snapshot.name)
                .collect::<Vec<_>>(),
            vec![
                "proxmox_snapshot_2026-04-13_00:00:00+0000",
                "proxmox_snapshot_2026-04-12_00:00:00+0000"
            ]
        );
    }

    #[test]
    fn retention_can_delete_by_keep_last_only() {
        let snapshots = vec![
            snapshot("proxmox_snapshot_2026-04-15_00:00:00+0000"),
            snapshot("proxmox_snapshot_2026-04-14_00:00:00+0000"),
        ];
        let deletions = retention_deletions(
            &snapshots,
            RetentionPolicy {
                keep_last: Some(1),
                max_age: None,
            },
            Utc.with_ymd_and_hms(2026, 4, 16, 0, 0, 0).unwrap(),
        );

        assert_eq!(
            deletions[0].name,
            "proxmox_snapshot_2026-04-14_00:00:00+0000"
        );
    }

    #[tokio::test]
    async fn schedule_run_creates_snapshots_and_applies_retention_for_nas_and_san() {
        let proxmox = MockProxmox {
            storage_defs: [(
                "NAS01".to_owned(),
                ProxmoxStorage {
                    storage_type: "nfs".to_owned(),
                    server: "nas.local".to_owned(),
                    content: "images".to_owned(),
                    export: "/nasvol".to_owned(),
                },
            )]
            .into_iter()
            .collect(),
        };
        let ontap = MockOntap {
            snapshots: vec![snapshot("proxmox_snapshot_2026-04-14_00:00:00+0000")],
            ..Default::default()
        };

        execute_schedule(
            &sample_config(),
            &proxmox,
            "daily",
            ScheduleAction::Run,
            |_| Ok(ontap.clone()),
        )
        .await
        .expect("schedule should run");

        let calls = ontap.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 4);
        assert!(calls[0].starts_with("create:nasvol:proxmox_snapshot_"));
        assert_eq!(
            calls[1],
            "delete:nasvol:proxmox_snapshot_2026-04-14_00:00:00+0000"
        );
        assert!(calls[2].starts_with("create:sanvol:proxmox_snapshot_"));
        assert_eq!(
            calls[3],
            "delete:sanvol:proxmox_snapshot_2026-04-14_00:00:00+0000"
        );
    }

    #[tokio::test]
    async fn schedule_delete_requires_retention_policy() {
        let config = LoadedConfig {
            schedule: [(
                "daily".to_owned(),
                ScheduleConfig {
                    storages: vec!["NAS01".to_owned()],
                    fsfreeze: false,
                    keep_last: None,
                    max_age: None,
                },
            )]
            .into_iter()
            .collect(),
            ..sample_config()
        };

        let error = execute_schedule(
            &config,
            &MockProxmox::default(),
            "daily",
            ScheduleAction::Delete,
            |_| Ok(MockOntap::default()),
        )
        .await
        .expect_err("delete should reject missing retention");

        assert!(error.to_string().contains("no retention policy"));
    }

    fn snapshot(name: &str) -> OntapSnapshot {
        OntapSnapshot {
            uuid: name.to_owned(),
            name: name.to_owned(),
            comment: None,
        }
    }

    fn sample_config() -> LoadedConfig {
        LoadedConfig {
            proxmox: ProxmoxConfig {
                host: "pve.local".to_owned(),
                user: "root@pam".to_owned(),
                token_name: "snap".to_owned(),
                token_value: "secret".to_owned(),
                verify_ssl: false,
                timezone: "Europe/Paris".to_owned(),
            },
            storage: [
                (
                    "NAS01".to_owned(),
                    StorageConfig::Nas(NasStorageConfig {
                        common: SharedStorageConfig {
                            ontap_host: "ontap.local".to_owned(),
                            ontap_user: "admin".to_owned(),
                            ontap_password: "pw".to_owned(),
                            verify_ssl: false,
                        },
                    }),
                ),
                (
                    "SAN01".to_owned(),
                    StorageConfig::San(SanStorageConfig {
                        common: SharedStorageConfig {
                            ontap_host: "ontap.local".to_owned(),
                            ontap_user: "admin".to_owned(),
                            ontap_password: "pw".to_owned(),
                            verify_ssl: false,
                        },
                        volume_name: "sanvol".to_owned(),
                        lun_path: "/vol/sanvol/lun0".to_owned(),
                        ssh_user: "root".to_owned(),
                    }),
                ),
            ]
            .into_iter()
            .collect(),
            schedule: [(
                "daily".to_owned(),
                ScheduleConfig {
                    storages: vec!["NAS01".to_owned(), "SAN01".to_owned()],
                    fsfreeze: false,
                    keep_last: Some(0),
                    max_age: None,
                },
            )]
            .into_iter()
            .collect(),
        }
    }

    #[derive(Clone, Default)]
    struct MockProxmox {
        storage_defs: HashMap<String, ProxmoxStorage>,
    }

    #[async_trait]
    impl ProxmoxApi for MockProxmox {
        async fn list_nodes(&self) -> Result<Vec<Node>> {
            Ok(Vec::new())
        }

        async fn list_vms(&self, _: &str) -> Result<Vec<VmSummary>> {
            Ok(Vec::new())
        }

        async fn get_vm_status(&self, _: &str, _: u32) -> Result<VmStatus> {
            Err(AppError::Missing("unused".to_owned()))
        }

        async fn get_vm_config(&self, _: &str, _: u32) -> Result<VmConfig> {
            Err(AppError::Missing("unused".to_owned()))
        }

        async fn shutdown_vm(&self, _: &str, _: u32) -> Result<String> {
            Err(AppError::Missing("unused".to_owned()))
        }

        async fn suspend_vm(&self, _: &str, _: u32) -> Result<String> {
            Err(AppError::Missing("unused".to_owned()))
        }

        async fn start_vm(&self, _: &str, _: u32) -> Result<String> {
            Err(AppError::Missing("unused".to_owned()))
        }

        async fn get_task_status(&self, _: &str, _: &str) -> Result<TaskStatus> {
            Err(AppError::Missing("unused".to_owned()))
        }

        async fn create_vm_snapshot(&self, _: &str, _: u32, _: &str, _: &str) -> Result<String> {
            Err(AppError::Missing("unused".to_owned()))
        }

        async fn delete_vm_snapshot(&self, _: &str, _: u32, _: &str) -> Result<String> {
            Err(AppError::Missing("unused".to_owned()))
        }

        async fn run_guest_agent_command(&self, _: &str, _: u32, _: &str) -> Result<Value> {
            Ok(json!(1))
        }

        async fn get_storage(&self, storage: &str) -> Result<ProxmoxStorage> {
            self.storage_defs
                .get(storage)
                .cloned()
                .ok_or_else(|| AppError::Missing("storage".to_owned()))
        }

        async fn create_storage(&self, _: &str, _: &ProxmoxStorage) -> Result<()> {
            Err(AppError::Missing("unused".to_owned()))
        }

        async fn delete_storage(&self, _: &str) -> Result<()> {
            Err(AppError::Missing("unused".to_owned()))
        }
    }

    #[derive(Clone, Default)]
    struct MockOntap {
        snapshots: Vec<OntapSnapshot>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl OntapApi for MockOntap {
        async fn get_volume_by_name(&self, name: &str) -> Result<OntapVolume> {
            Ok(OntapVolume {
                uuid: name.to_owned(),
                name: name.to_owned(),
                svm_name: "svm1".to_owned(),
                nas_path: None,
                is_flexclone: false,
            })
        }

        async fn get_volume_detail(&self, _: &str) -> Result<Value> {
            Ok(json!({}))
        }

        async fn list_snapshots(&self, _: &str) -> Result<Vec<OntapSnapshot>> {
            Ok(self.snapshots.clone())
        }

        async fn create_snapshot(&self, volume: &OntapVolume, name: &str, _: &str) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("create:{}:{name}", volume.name));
            Ok(())
        }

        async fn restore_snapshot(&self, _: &OntapVolume, _: &str) -> Result<()> {
            Ok(())
        }

        async fn delete_snapshot(&self, volume_uuid: &str, snapshot_name: &str) -> Result<()> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("delete:{volume_uuid}:{snapshot_name}"));
            Ok(())
        }

        async fn clone_file(&self, _: &FileCloneRequest) -> Result<()> {
            Ok(())
        }

        async fn create_flexclone(&self, _: &FlexCloneRequest) -> Result<()> {
            Ok(())
        }

        async fn delete_volume(&self, _: &str, _: bool) -> Result<()> {
            Ok(())
        }

        async fn get_lun_by_name(&self, _: &str) -> Result<OntapLun> {
            Err(AppError::Missing("unused".to_owned()))
        }

        async fn get_lun_detail(&self, _: &str) -> Result<Value> {
            Err(AppError::Missing("unused".to_owned()))
        }

        async fn list_lun_maps(&self) -> Result<Vec<OntapLunMap>> {
            Ok(Vec::new())
        }

        async fn get_igroup_detail(&self, _: &str) -> Result<Value> {
            Err(AppError::Missing("unused".to_owned()))
        }
    }
}
