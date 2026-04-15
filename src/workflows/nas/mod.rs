mod discovery;
mod storage;
#[cfg(test)]
mod tests;
mod vm;

pub use discovery::{get_vms_by_storage, resolve_nas_volume_name, wait_for_task};
pub use storage::{
    create_storage_snapshot, delete_storage_snapshot, list_storage_snapshots,
    mount_storage_snapshot, restore_storage_snapshot, show_storage, storage_snapshot_rows,
    unmount_storage_snapshot,
};
pub use vm::create_vm_snapshot;
