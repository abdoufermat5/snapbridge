pub mod cli;
pub mod clients;
pub mod config;
pub mod display;
pub mod error;
pub mod logger;
pub mod models;
pub mod shell;
pub mod util;
pub mod workflows;

use crate::cli::{
    Cli, NasCommand, NasStorageCommand, SanCommand, SanStorageCommand, ScheduleCommand, VmCommand,
};
use crate::clients::ontap::ReqwestOntapClient;
use crate::clients::proxmox::ReqwestProxmoxClient;
use crate::config::{LoadedConfig, StorageBackend, StorageConfig};
use crate::display::SnapshotRow;
use crate::error::Result;
use crate::shell::TokioShellRunner;
use crate::workflows::nas;
use crate::workflows::san;
use crate::workflows::scheduler::{self, ScheduleAction};

pub async fn run(cli: Cli) -> Result<()> {
    let output = cli.output;
    let config = LoadedConfig::from_path(&cli.config)?;
    let proxmox = ReqwestProxmoxClient::new(&config.proxmox)?;
    let shell = TokioShellRunner;

    match cli.command {
        crate::cli::TopCommand::Nas(command) => match command {
            NasCommand::Vm(VmCommand::Create(args)) => {
                nas::create_vm_snapshot(&config, &proxmox, args.vm, args.suspend, args.shutdown)
                    .await
            }
            NasCommand::Storage(command) => match command {
                NasStorageCommand::Create(args) => {
                    nas::create_storage_snapshot(
                        &config,
                        &proxmox,
                        &ontap_for_storage(&config, &args.storage)?,
                        &args.storage,
                        args.fsfreeze,
                    )
                    .await
                }
                NasStorageCommand::Restore(args) => {
                    nas::restore_storage_snapshot(
                        &config,
                        &proxmox,
                        &ontap_for_storage(&config, &args.storage)?,
                        &args.storage,
                        &args.snapshot,
                    )
                    .await
                }
                NasStorageCommand::Delete(args) => {
                    nas::delete_storage_snapshot(
                        &config,
                        &proxmox,
                        &ontap_for_storage(&config, &args.storage)?,
                        &args.storage,
                        &args.snapshot,
                    )
                    .await
                }
                NasStorageCommand::List(args) => {
                    if let Some(storage_id) = args.storage.as_deref() {
                        nas::list_storage_snapshots(
                            &config,
                            &proxmox,
                            &ontap_for_storage(&config, storage_id)?,
                            storage_id,
                            output,
                        )
                        .await
                    } else {
                        let mut snapshots = Vec::<SnapshotRow>::new();
                        for storage_id in config.storage_ids_for_backend(StorageBackend::Nas) {
                            snapshots.extend(
                                nas::storage_snapshot_rows(
                                    &config,
                                    &proxmox,
                                    &ontap_for_storage(&config, storage_id)?,
                                    storage_id,
                                )
                                .await?,
                            );
                        }

                        crate::display::print_snapshots_with_storage(output, &snapshots, true)
                    }
                }
                NasStorageCommand::Mount(args) => {
                    nas::mount_storage_snapshot(
                        &config,
                        &proxmox,
                        &ontap_for_storage(&config, &args.storage)?,
                        &args.storage,
                        &args.snapshot,
                    )
                    .await
                }
                NasStorageCommand::Unmount(args) => {
                    nas::unmount_storage_snapshot(
                        &config,
                        &proxmox,
                        &ontap_for_storage(&config, &args.storage)?,
                        &args.storage,
                    )
                    .await
                }
                NasStorageCommand::Show(args) => {
                    nas::show_storage(
                        &config,
                        &proxmox,
                        &ontap_for_storage(&config, &args.storage)?,
                        &args.storage,
                        output,
                    )
                    .await
                }
            },
        },
        crate::cli::TopCommand::San(command) => match command {
            SanCommand::Storage(command) => match command {
                SanStorageCommand::Create(args) => {
                    san::create_storage_snapshot(
                        &config,
                        &proxmox,
                        &ontap_for_storage(&config, &args.storage)?,
                        &args.storage,
                        args.fsfreeze,
                    )
                    .await
                }
                SanStorageCommand::Restore(args) => {
                    san::restore_storage_snapshot(
                        &config,
                        &proxmox,
                        &ontap_for_storage(&config, &args.storage)?,
                        &shell,
                        &args.storage,
                        &args.snapshot,
                    )
                    .await
                }
                SanStorageCommand::Delete(args) => {
                    san::delete_storage_snapshot(
                        &config,
                        &proxmox,
                        &ontap_for_storage(&config, &args.storage)?,
                        &args.storage,
                        &args.snapshot,
                    )
                    .await
                }
                SanStorageCommand::List(args) => {
                    if let Some(storage_id) = args.storage.as_deref() {
                        san::list_storage_snapshots(
                            &config,
                            &proxmox,
                            &ontap_for_storage(&config, storage_id)?,
                            storage_id,
                            output,
                        )
                        .await
                    } else {
                        let mut snapshots = Vec::<SnapshotRow>::new();
                        for storage_id in config.storage_ids_for_backend(StorageBackend::San) {
                            snapshots.extend(
                                san::storage_snapshot_rows(
                                    &config,
                                    &ontap_for_storage(&config, storage_id)?,
                                    storage_id,
                                )
                                .await?,
                            );
                        }

                        crate::display::print_snapshots_with_storage(output, &snapshots, true)
                    }
                }
                SanStorageCommand::Show(args) => {
                    san::show_storage(
                        &config,
                        &proxmox,
                        &ontap_for_storage(&config, &args.storage)?,
                        &args.storage,
                        output,
                    )
                    .await
                }
            },
        },
        crate::cli::TopCommand::Schedule(command) => match command {
            ScheduleCommand::Run(args) => {
                scheduler::execute_schedule(
                    &config,
                    &proxmox,
                    &args.name,
                    ScheduleAction::Run,
                    |storage_id| ontap_for_storage(&config, storage_id),
                )
                .await
            }
            ScheduleCommand::Create(args) => {
                scheduler::execute_schedule(
                    &config,
                    &proxmox,
                    &args.name,
                    ScheduleAction::Create,
                    |storage_id| ontap_for_storage(&config, storage_id),
                )
                .await
            }
            ScheduleCommand::Delete(args) => {
                scheduler::execute_schedule(
                    &config,
                    &proxmox,
                    &args.name,
                    ScheduleAction::Delete,
                    |storage_id| ontap_for_storage(&config, storage_id),
                )
                .await
            }
            ScheduleCommand::List => scheduler::print_schedules(output, &config),
        },
    }
}

fn ontap_for_storage(config: &LoadedConfig, storage_id: &str) -> Result<ReqwestOntapClient> {
    let storage = config.storage(storage_id)?;
    let settings = match storage {
        StorageConfig::Nas(settings) => settings.shared(),
        StorageConfig::San(settings) => settings.shared(),
    };
    ReqwestOntapClient::new(settings)
}
