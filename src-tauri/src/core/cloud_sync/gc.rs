use std::collections::HashSet;
use std::time::Duration;

use crate::error::AppResult;

use super::operator::CloudRemote;
use super::protocol::sync_snapshot_file;
use super::remote::{
    RemoteSyncPointer, SYNC_SNAPSHOTS_DIR, current_time_ms, is_legacy_sync_snapshot_path,
    remote_path,
};

pub(super) const SYNC_SNAPSHOT_KEEP_RECENT: usize = 5;
pub(super) const SYNC_SNAPSHOT_GC_GRACE_PERIOD: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SnapshotGcEntry {
    pub path: String,
    pub revision_id: String,
    pub created_at_ms: u64,
    pub deletable: bool,
}

pub(super) async fn cleanup_sync_snapshots(
    remote: &CloudRemote,
    remote_root: &str,
    latest: Option<&RemoteSyncPointer>,
) {
    let result = collect_snapshots(remote, remote_root)
        .await
        .map(|snapshots| {
            plan_snapshot_gc(
                snapshots,
                latest.map(|pointer| pointer.revision_id.as_str()),
                current_time_ms(),
                SYNC_SNAPSHOT_KEEP_RECENT,
                SYNC_SNAPSHOT_GC_GRACE_PERIOD,
            )
        });

    let paths = match result {
        Ok(paths) => paths,
        Err(error) => {
            tracing::warn!("Failed to plan cloud sync snapshot cleanup: {}", error);
            return;
        }
    };

    for path in paths {
        if let Err(error) = remote.delete(&path).await {
            tracing::warn!(
                path = %path,
                error = %error,
                "Failed to delete old cloud sync snapshot"
            );
        }
    }
}

async fn collect_snapshots(
    remote: &CloudRemote,
    remote_root: &str,
) -> AppResult<Vec<SnapshotGcEntry>> {
    let prefix = remote_path(remote_root, SYNC_SNAPSHOTS_DIR);
    let paths = remote.list_files(&prefix).await?;
    let mut snapshots = Vec::new();
    for path in paths
        .into_iter()
        .filter(|path| is_legacy_sync_snapshot_path(path, remote_root))
    {
        let Some(revision_id) = snapshot_revision_from_path(&path) else {
            snapshots.push(SnapshotGcEntry {
                path,
                revision_id: String::new(),
                created_at_ms: 0,
                deletable: false,
            });
            continue;
        };
        let pointer = RemoteSyncPointer {
            schema_version: 2,
            revision_id: revision_id.clone(),
            created_at_ms: 0,
            payload_hash: String::new(),
            device_id: String::new(),
            app_version: String::new(),
        };
        match read_snapshot_for_gc(remote, remote_root, &pointer).await {
            Ok((created_at_ms, payload_hash)) => snapshots.push(SnapshotGcEntry {
                path,
                revision_id,
                created_at_ms,
                deletable: !payload_hash.is_empty(),
            }),
            Err(error) => {
                tracing::warn!(
                    path = %path,
                    error = %error,
                    "Cloud sync snapshot is not readable; keeping it during cleanup"
                );
                snapshots.push(SnapshotGcEntry {
                    path,
                    revision_id,
                    created_at_ms: 0,
                    deletable: false,
                });
            }
        }
    }
    Ok(snapshots)
}

async fn read_snapshot_for_gc(
    remote: &CloudRemote,
    remote_root: &str,
    pointer: &RemoteSyncPointer,
) -> AppResult<(u64, String)> {
    let Some(raw) = remote
        .read_if_exists(&remote_path(
            remote_root,
            &sync_snapshot_file(&pointer.revision_id),
        ))
        .await?
    else {
        return Ok((0, String::new()));
    };
    let decrypted = super::crypto::decrypt_snapshot_bytes(&raw)?;
    let snapshot = crate::core::portable_snapshot::decode_portable_snapshot(&decrypted)?;
    if snapshot.revision_id != pointer.revision_id {
        return Err(crate::error::CloudSyncError::RevisionMismatch {
            pointer_revision: pointer.revision_id.clone(),
            snapshot_revision: snapshot.revision_id,
        }
        .into());
    }
    Ok((snapshot.created_at_ms, snapshot.payload_hash))
}

pub(super) fn plan_snapshot_gc(
    mut snapshots: Vec<SnapshotGcEntry>,
    latest_revision: Option<&str>,
    now_ms: u64,
    keep_recent: usize,
    grace_period: Duration,
) -> Vec<String> {
    let mut protected: HashSet<String> = HashSet::new();
    if let Some(latest_revision) = latest_revision {
        protected.insert(latest_revision.to_string());
    }

    snapshots.sort_by_key(|snapshot| snapshot.created_at_ms);
    for snapshot in snapshots.iter().rev().take(keep_recent) {
        protected.insert(snapshot.revision_id.clone());
    }

    let grace_ms = u64::try_from(grace_period.as_millis()).unwrap_or(u64::MAX);
    snapshots
        .into_iter()
        .filter(|snapshot| snapshot.deletable)
        .filter(|snapshot| !protected.contains(&snapshot.revision_id))
        .filter(|snapshot| now_ms.saturating_sub(snapshot.created_at_ms) > grace_ms)
        .map(|snapshot| snapshot.path)
        .collect()
}

fn snapshot_revision_from_path(path: &str) -> Option<String> {
    let filename = path.rsplit('/').next()?;
    filename
        .strip_suffix(".redb.enc")
        .filter(|revision| !revision.is_empty())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(revision: &str, created_at_ms: u64) -> SnapshotGcEntry {
        SnapshotGcEntry {
            path: format!("nyaterm/sync/snapshots/{revision}.redb.enc"),
            revision_id: revision.to_string(),
            created_at_ms,
            deletable: true,
        }
    }

    #[test]
    fn gc_keeps_latest_even_when_it_is_old() {
        let delete = plan_snapshot_gc(
            vec![
                entry("r1", 1),
                entry("r2", 2),
                entry("r3", 3),
                entry("r4", 4),
                entry("r5", 5),
                entry("r6", 6),
                entry("r7", 7),
            ],
            Some("r1"),
            100_000_000,
            5,
            Duration::from_secs(0),
        );

        assert!(!delete.iter().any(|path| path.contains("r1.redb.enc")));
    }

    #[test]
    fn gc_protects_recent_orphans() {
        let delete = plan_snapshot_gc(
            vec![entry("old", 1), entry("fresh", 99_000)],
            None,
            100_000,
            0,
            Duration::from_secs(2),
        );

        assert_eq!(delete, vec!["nyaterm/sync/snapshots/old.redb.enc"]);
    }
}
