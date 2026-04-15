use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

impl LogLevel {
    pub fn as_level_filter(self) -> log::LevelFilter {
        match self {
            Self::Error => log::LevelFilter::Error,
            Self::Warn => log::LevelFilter::Warn,
            Self::Info => log::LevelFilter::Info,
            Self::Debug => log::LevelFilter::Debug,
        }
    }
}

#[derive(Debug, Parser)]
#[command(name = "proxsnap")]
#[command(about = "Proxmox + ONTAP snapshot workflows for NAS and SAN storage")]
pub struct Cli {
    #[arg(long, default_value = "proxsnap.toml")]
    pub config: PathBuf,

    #[arg(long, value_enum, default_value = "info")]
    pub log_level: LogLevel,

    #[command(subcommand)]
    pub command: TopCommand,
}

#[derive(Debug, Subcommand)]
pub enum TopCommand {
    #[command(subcommand)]
    Nas(NasCommand),
    #[command(subcommand)]
    San(SanCommand),
}

#[derive(Debug, Subcommand)]
pub enum NasCommand {
    #[command(subcommand)]
    Vm(VmCommand),
    #[command(subcommand)]
    Storage(NasStorageCommand),
}

#[derive(Debug, Subcommand)]
pub enum SanCommand {
    #[command(subcommand)]
    Storage(SanStorageCommand),
}

#[derive(Debug, Subcommand)]
pub enum VmCommand {
    Create(VmCreateArgs),
}

#[derive(Debug, Args)]
#[command(group(
    ArgGroup::new("vm_state")
        .args(["suspend", "shutdown"])
        .multiple(false)
))]
pub struct VmCreateArgs {
    #[arg(long)]
    pub vm: u32,

    #[arg(long)]
    pub suspend: bool,

    #[arg(long)]
    pub shutdown: bool,
}

#[derive(Debug, Args)]
pub struct CreateStorageArgs {
    #[arg(long)]
    pub storage: String,

    #[arg(long)]
    pub fsfreeze: bool,
}

#[derive(Debug, Args)]
pub struct SnapshotStorageArgs {
    #[arg(long)]
    pub storage: String,

    #[arg(long)]
    pub snapshot: String,
}

#[derive(Debug, Args)]
pub struct StorageArgs {
    #[arg(long)]
    pub storage: String,
}

#[derive(Debug, Subcommand)]
pub enum NasStorageCommand {
    Create(CreateStorageArgs),
    Restore(SnapshotStorageArgs),
    Delete(SnapshotStorageArgs),
    List(StorageArgs),
    Mount(SnapshotStorageArgs),
    Unmount(StorageArgs),
    Show(StorageArgs),
}

#[derive(Debug, Subcommand)]
pub enum SanStorageCommand {
    Create(CreateStorageArgs),
    Restore(SnapshotStorageArgs),
    Delete(SnapshotStorageArgs),
    List(StorageArgs),
    Show(StorageArgs),
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, NasCommand, SanCommand, SanStorageCommand, TopCommand, VmCommand};

    #[test]
    fn parses_nas_vm_command() {
        let cli = Cli::try_parse_from([
            "proxsnap",
            "nas",
            "vm",
            "create",
            "--vm",
            "101",
            "--suspend",
        ])
        .expect("cli should parse");

        match cli.command {
            TopCommand::Nas(NasCommand::Vm(VmCommand::Create(args))) => {
                assert_eq!(args.vm, 101);
                assert!(args.suspend);
                assert!(!args.shutdown);
            }
            _ => panic!("unexpected command shape"),
        }
    }

    #[test]
    fn parses_san_storage_command() {
        let cli = Cli::try_parse_from([
            "proxsnap",
            "san",
            "storage",
            "restore",
            "--storage",
            "SAN01",
            "--snapshot",
            "snap",
        ])
        .expect("cli should parse");

        match cli.command {
            TopCommand::San(SanCommand::Storage(SanStorageCommand::Restore(args))) => {
                assert_eq!(args.storage, "SAN01");
                assert_eq!(args.snapshot, "snap");
            }
            _ => panic!("unexpected command shape"),
        }
    }
}
