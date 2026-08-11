use crate::core::portable_snapshot::PortableSnapshot;
use crate::error::{AppError, AppResult, CloudSyncError};

use super::operator::CloudRemote;
use super::protocol::{
    commit_sync_pointer, pointer_from_snapshot, read_current_sync_snapshot_compat,
    read_snapshot_for_pointer, upload_sync_snapshot, validate_snapshot_against_pointer,
    verify_uploaded_sync_snapshot,
};
use super::remote::RemoteSyncPointer;

#[derive(Debug)]
pub(super) enum RemoteSnapshotResolution {
    Current(PortableSnapshot),
    LegacyMigrated(PortableSnapshot),
    Inconsistent {
        pointer: RemoteSyncPointer,
        recovery_candidate: PortableSnapshot,
    },
}

pub(super) async fn resolve_remote_snapshot(
    remote: &CloudRemote,
    remote_root: &str,
    pointer: &RemoteSyncPointer,
) -> AppResult<RemoteSnapshotResolution> {
    match read_snapshot_for_pointer(remote, remote_root, pointer).await {
        Ok(snapshot) => return Ok(RemoteSnapshotResolution::Current(snapshot)),
        Err(AppError::CloudSync(CloudSyncError::SnapshotMissing { .. })) => {}
        Err(error) => return Err(error),
    }

    let Some(current) = read_current_sync_snapshot_compat(remote, remote_root).await? else {
        return Err(CloudSyncError::SnapshotMissing {
            revision: pointer.revision_id.clone(),
        }
        .into());
    };

    if validate_snapshot_against_pointer(pointer, &current).is_ok() {
        migrate_legacy_snapshot(remote, remote_root, pointer, &current).await?;
        return Ok(RemoteSnapshotResolution::LegacyMigrated(current));
    }

    Ok(RemoteSnapshotResolution::Inconsistent {
        pointer: pointer.clone(),
        recovery_candidate: current,
    })
}

pub(super) async fn migrate_legacy_snapshot(
    remote: &CloudRemote,
    remote_root: &str,
    pointer: &RemoteSyncPointer,
    snapshot: &PortableSnapshot,
) -> AppResult<()> {
    upload_sync_snapshot(remote, remote_root, snapshot).await?;
    verify_uploaded_sync_snapshot(remote, remote_root, pointer).await?;
    Ok(())
}

pub(super) async fn recover_current_remote_snapshot(
    remote: &CloudRemote,
    remote_root: &str,
) -> AppResult<PortableSnapshot> {
    let Some(snapshot) = read_current_sync_snapshot_compat(remote, remote_root).await? else {
        return Err(AppError::Config(
            "No current cloud sync snapshot is available for recovery".to_string(),
        ));
    };
    let pointer = pointer_from_snapshot(&snapshot);
    upload_sync_snapshot(remote, remote_root, &snapshot).await?;
    verify_uploaded_sync_snapshot(remote, remote_root, &pointer).await?;
    commit_sync_pointer(remote, remote_root, &pointer).await?;
    Ok(snapshot)
}
