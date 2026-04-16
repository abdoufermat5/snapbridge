use std::path::PathBuf;

use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};

use crate::display::OutputFormat;

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
#[command(name = "snapbridge")]
#[command(about = "Proxmox + ONTAP snapshot workflows for NAS and SAN storage")]
pub struct Cli {
    #[arg(long, default_value = "/etc/snapbridge/snapbridge.toml")]
    pub config: PathBuf,

    #[arg(long, value_enum, default_value = "info")]
    pub log_level: LogLevel,

    #[arg(long, value_enum, default_value = "table", global = true)]
    pub output: OutputFormat,

    #[command(subcommand)]
    pub command: TopCommand,
}

#[derive(Debug, Subcommand)]
pub enum TopCommand {
    #[command(subcommand)]
    Nas(NasCommand),
    #[command(subcommand)]
    San(SanCommand),
    #[command(subcommand)]
    Schedule(ScheduleCommand),
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

#[derive(Debug, Args)]
pub struct OptionalStorageArgs {
    #[arg(long)]
    pub storage: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum NasStorageCommand {
    Create(CreateStorageArgs),
    Restore(SnapshotStorageArgs),
    Delete(SnapshotStorageArgs),
    List(OptionalStorageArgs),
    Mount(SnapshotStorageArgs),
    Unmount(StorageArgs),
    Show(StorageArgs),
}

#[derive(Debug, Subcommand)]
pub enum SanStorageCommand {
    Create(CreateStorageArgs),
    Restore(SnapshotStorageArgs),
    Delete(SnapshotStorageArgs),
    List(OptionalStorageArgs),
    Show(StorageArgs),
}

#[derive(Debug, Subcommand)]
pub enum ScheduleCommand {
    Run(ScheduleNameArgs),
    Create(ScheduleNameArgs),
    Delete(ScheduleNameArgs),
    List,
}

#[derive(Debug, Args)]
pub struct ScheduleNameArgs {
    pub name: String,
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{
        Cli, NasCommand, NasStorageCommand, OutputFormat, SanCommand, SanStorageCommand,
        ScheduleCommand, TopCommand, VmCommand,
    };

    #[test]
    fn parses_nas_vm_command() {
        let cli = Cli::try_parse_from([
            "snapbridge",
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
    fn defaults_to_system_config_path() {
        let cli = Cli::try_parse_from(["snapbridge", "nas", "storage", "list"])
            .expect("cli should parse");

        assert_eq!(
            cli.config,
            std::path::PathBuf::from("/etc/snapbridge/snapbridge.toml")
        );
    }

    #[test]
    fn parses_san_storage_command() {
        let cli = Cli::try_parse_from([
            "snapbridge",
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

    #[test]
    fn parses_nas_storage_list_without_storage_filter() {
        let cli = Cli::try_parse_from(["snapbridge", "nas", "storage", "list"])
            .expect("cli should parse");

        match cli.command {
            TopCommand::Nas(NasCommand::Storage(NasStorageCommand::List(args))) => {
                assert_eq!(args.storage, None);
            }
            _ => panic!("unexpected command shape"),
        }
    }

    #[test]
    fn parses_san_storage_list_without_storage_filter() {
        let cli = Cli::try_parse_from(["snapbridge", "san", "storage", "list"])
            .expect("cli should parse");

        match cli.command {
            TopCommand::San(SanCommand::Storage(SanStorageCommand::List(args))) => {
                assert_eq!(args.storage, None);
            }
            _ => panic!("unexpected command shape"),
        }
    }

    #[test]
    fn parses_global_output_format_after_subcommand() {
        let cli = Cli::try_parse_from([
            "snapbridge",
            "nas",
            "storage",
            "list",
            "--storage",
            "NAS01",
            "--output",
            "json",
        ])
        .expect("cli should parse");

        assert_eq!(cli.output, OutputFormat::Json);
    }

    #[test]
    fn parses_schedule_commands() {
        let cli = Cli::try_parse_from(["snapbridge", "schedule", "run", "daily"])
            .expect("cli should parse");
        match cli.command {
            TopCommand::Schedule(ScheduleCommand::Run(args)) => assert_eq!(args.name, "daily"),
            _ => panic!("unexpected command shape"),
        }

        let cli = Cli::try_parse_from(["snapbridge", "schedule", "create", "daily"])
            .expect("cli should parse");
        match cli.command {
            TopCommand::Schedule(ScheduleCommand::Create(args)) => assert_eq!(args.name, "daily"),
            _ => panic!("unexpected command shape"),
        }

        let cli = Cli::try_parse_from(["snapbridge", "schedule", "delete", "daily"])
            .expect("cli should parse");
        match cli.command {
            TopCommand::Schedule(ScheduleCommand::Delete(args)) => assert_eq!(args.name, "daily"),
            _ => panic!("unexpected command shape"),
        }

        let cli =
            Cli::try_parse_from(["snapbridge", "schedule", "list"]).expect("cli should parse");
        assert!(matches!(
            cli.command,
            TopCommand::Schedule(ScheduleCommand::List)
        ));
    }
}
