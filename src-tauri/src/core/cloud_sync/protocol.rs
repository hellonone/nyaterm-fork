use crate::core::portable_snapshot::{
    PortableSnapshot, decode_portable_snapshot, encode_portable_snapshot,
};
use crate::error::{AppError, AppResult, CloudSyncError};

use super::crypto::{decrypt_snapshot_bytes, encrypt_snapshot_bytes};
use super::operator::CloudRemote;
use super::remote::{
    REMOTE_SYNC_POINTER_SCHEMA_VERSION, RemoteSyncPointer, SYNC_CURRENT_FILE,
    legacy_sync_snapshot_file, load_sync_pointer, remote_path, write_sync_pointer,
};

pub(super) fn sync_snapshot_file(revision: &str) -> String {
    legacy_sync_snapshot_file(revision)
}

pub(super) fn sync_snapshot_path(remote_root: &str, revision: &str) -> String {
    remote_path(remote_root, &sync_snapshot_file(revision))
}

pub(super) fn pointer_from_snapshot(snapshot: &PortableSnapshot) -> RemoteSyncPointer {
    RemoteSyncPointer {
        schema_version: REMOTE_SYNC_POINTER_SCHEMA_VERSION,
        revision_id: snapshot.revision_id.clone(),
        created_at_ms: snapshot.created_at_ms,
        payload_hash: snapshot.payload_hash.clone(),
        device_id: snapshot.device_id.clone(),
        app_version: snapshot.app_version.clone(),
    }
}

pub(super) async fn upload_sync_snapshot(
    remote: &CloudRemote,
    remote_root: &str,
    snapshot: &PortableSnapshot,
) -> AppResult<()> {
    let encoded = encode_portable_snapshot(snapshot)?;
    let encrypted = encrypt_snapshot_bytes(&encoded)?;
    remote
        .write(
            &sync_snapshot_path(remote_root, &snapshot.revision_id),
            encrypted,
        )
        .await
}

pub(super) async fn verify_uploaded_sync_snapshot(
    remote: &CloudRemote,
    remote_root: &str,
    pointer: &RemoteSyncPointer,
) -> AppResult<PortableSnapshot> {
    read_snapshot_for_pointer(remote, remote_root, pointer).await
}

pub(super) async fn read_snapshot_for_pointer(
    remote: &CloudRemote,
    remote_root: &str,
    pointer: &RemoteSyncPointer,
) -> AppResult<PortableSnapshot> {
    let path = sync_snapshot_path(remote_root, &pointer.revision_id);
    let Some(raw) = remote.read_if_exists(&path).await? else {
        return Err(CloudSyncError::SnapshotMissing {
            revision: pointer.revision_id.clone(),
        }
        .into());
    };
    let snapshot = decode_remote_sync_snapshot(&raw, &pointer.revision_id)?;
    validate_snapshot_against_pointer(pointer, &snapshot)?;
    Ok(snapshot)
}

pub(super) async fn write_current_sync_snapshot_compat(
    remote: &CloudRemote,
    remote_root: &str,
    snapshot: &PortableSnapshot,
) -> AppResult<()> {
    let encoded = encode_portable_snapshot(snapshot)?;
    let encrypted = encrypt_snapshot_bytes(&encoded)?;
    remote
        .write(&remote_path(remote_root, SYNC_CURRENT_FILE), encrypted)
        .await
}

pub(super) async fn read_current_sync_snapshot_compat(
    remote: &CloudRemote,
    remote_root: &str,
) -> AppResult<Option<PortableSnapshot>> {
    let Some(raw) = remote
        .read_if_exists(&remote_path(remote_root, SYNC_CURRENT_FILE))
        .await?
    else {
        return Ok(None);
    };
    decode_remote_sync_snapshot(&raw, "current").map(Some)
}

pub(super) async fn commit_sync_pointer(
    remote: &CloudRemote,
    remote_root: &str,
    pointer: &RemoteSyncPointer,
) -> AppResult<()> {
    write_sync_pointer(remote, remote_root, pointer).await
}

pub(super) async fn ensure_remote_head_unchanged(
    remote: &CloudRemote,
    remote_root: &str,
    expected: Option<&RemoteSyncPointer>,
) -> AppResult<()> {
    let actual = load_sync_pointer(remote, remote_root).await?;
    let expected_revision = expected.map(|pointer| pointer.revision_id.clone());
    let actual_revision = actual.as_ref().map(|pointer| pointer.revision_id.clone());
    if expected_revision != actual_revision {
        return Err(CloudSyncError::ConcurrentUpdate {
            expected_revision,
            actual_revision,
        }
        .into());
    }
    Ok(())
}

pub(super) fn validate_snapshot_against_pointer(
    pointer: &RemoteSyncPointer,
    snapshot: &PortableSnapshot,
) -> AppResult<()> {
    if snapshot.revision_id != pointer.revision_id {
        return Err(CloudSyncError::RevisionMismatch {
            pointer_revision: pointer.revision_id.clone(),
            snapshot_revision: snapshot.revision_id.clone(),
        }
        .into());
    }

    if snapshot.payload_hash != pointer.payload_hash {
        return Err(CloudSyncError::HashMismatch {
            expected: pointer.payload_hash.clone(),
            actual: snapshot.payload_hash.clone(),
        }
        .into());
    }

    Ok(())
}

fn decode_remote_sync_snapshot(raw: &[u8], revision: &str) -> AppResult<PortableSnapshot> {
    let decrypted = decrypt_snapshot_bytes(raw).map_err(|error| match error {
        AppError::CloudSync(_) => error,
        _ => CloudSyncError::CorruptedSnapshot {
            revision: revision.to_string(),
        }
        .into(),
    })?;
    decode_portable_snapshot(&decrypted).map_err(|error| match error {
        AppError::CloudSync(_) => error,
        _ => CloudSyncError::CorruptedSnapshot {
            revision: revision.to_string(),
        }
        .into(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use crate::config::AppSettings;
    use crate::core::portable_snapshot::{
        PortableAppSettings, PortableSnapshotKind, calculate_payload_hash,
    };
    use crate::utils::crypto::set_master_password;

    use super::super::migration::{RemoteSnapshotResolution, resolve_remote_snapshot};
    use super::super::operator::MemoryRemote;
    use super::super::remote::{load_sync_pointer, remote_path};
    use super::*;

    static MASTER_PASSWORD_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn memory_remote() -> (MemoryRemote, CloudRemote) {
        let memory = MemoryRemote::with_files(HashMap::new());
        let remote = CloudRemote::Memory(memory.clone());
        (memory, remote)
    }

    fn sample_snapshot(revision_id: &str, created_at_ms: u64) -> PortableSnapshot {
        let settings = PortableAppSettings::from_app_settings(
            &AppSettings::default(),
            &PortableSnapshotKind::Sync,
        );
        let mut snapshot = PortableSnapshot {
            schema_version: 3,
            snapshot_kind: PortableSnapshotKind::Sync,
            revision_id: revision_id.to_string(),
            device_id: "device".to_string(),
            created_at_ms,
            payload_hash: String::new(),
            app_version: "test".to_string(),
            settings,
            sessions: Default::default(),
            keys: Default::default(),
            passwords: Default::default(),
            credentials: Default::default(),
            otp: Default::default(),
            proxies: Default::default(),
            proxy_groups: Default::default(),
            tunnels: Default::default(),
            tunnel_groups: Default::default(),
            quick_commands: Default::default(),
            history: Default::default(),
            master_key_token: None,
            known_hosts: String::new(),
            notes: Default::default(),
        };
        snapshot.payload_hash = calculate_payload_hash(&snapshot).expect("hash snapshot");
        snapshot
    }

    async fn write_committed_snapshot(
        remote: &CloudRemote,
        revision_id: &str,
    ) -> RemoteSyncPointer {
        let snapshot = sample_snapshot(revision_id, 1);
        let pointer = pointer_from_snapshot(&snapshot);
        upload_sync_snapshot(remote, "nyaterm", &snapshot)
            .await
            .expect("upload snapshot");
        commit_sync_pointer(remote, "nyaterm", &pointer)
            .await
            .expect("commit pointer");
        pointer
    }

    #[tokio::test(flavor = "current_thread")]
    async fn snapshot_upload_failure_leaves_latest_unchanged() {
        let _guard = MASTER_PASSWORD_TEST_LOCK.lock().expect("lock password");
        set_master_password(Some("secret".to_string()));
        let (memory, remote) = memory_remote();
        let old_pointer = write_committed_snapshot(&remote, "r1").await;
        memory.fail_next_write_containing("snapshots/r2");

        let new_snapshot = sample_snapshot("r2", 2);
        let result = upload_sync_snapshot(&remote, "nyaterm", &new_snapshot).await;

        assert!(result.is_err());
        let latest = load_sync_pointer(&remote, "nyaterm")
            .await
            .expect("load latest")
            .expect("latest");
        assert_eq!(latest.revision_id, old_pointer.revision_id);
        set_master_password(None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pointer_write_failure_keeps_old_revision_readable() {
        let _guard = MASTER_PASSWORD_TEST_LOCK.lock().expect("lock password");
        set_master_password(Some("secret".to_string()));
        let (memory, remote) = memory_remote();
        let old_pointer = write_committed_snapshot(&remote, "r1").await;
        let new_snapshot = sample_snapshot("r2", 2);
        let new_pointer = pointer_from_snapshot(&new_snapshot);

        upload_sync_snapshot(&remote, "nyaterm", &new_snapshot)
            .await
            .expect("upload new snapshot");
        memory.fail_next_write_containing("latest.redb");
        let result = commit_sync_pointer(&remote, "nyaterm", &new_pointer).await;

        assert!(result.is_err());
        let old_snapshot = read_snapshot_for_pointer(&remote, "nyaterm", &old_pointer)
            .await
            .expect("old snapshot readable");
        assert_eq!(old_snapshot.revision_id, "r1");
        set_master_password(None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn current_pointer_mismatch_returns_inconsistent_resolution() {
        let _guard = MASTER_PASSWORD_TEST_LOCK.lock().expect("lock password");
        set_master_password(Some("secret".to_string()));
        let (_memory, remote) = memory_remote();
        let pointer = pointer_from_snapshot(&sample_snapshot("r1", 1));
        commit_sync_pointer(&remote, "nyaterm", &pointer)
            .await
            .expect("commit pointer");
        write_current_sync_snapshot_compat(&remote, "nyaterm", &sample_snapshot("r2", 2))
            .await
            .expect("write current");

        let resolution = resolve_remote_snapshot(&remote, "nyaterm", &pointer)
            .await
            .expect("resolve remote");

        match resolution {
            RemoteSnapshotResolution::Inconsistent {
                pointer,
                recovery_candidate,
            } => {
                assert_eq!(pointer.revision_id, "r1");
                assert_eq!(recovery_candidate.revision_id, "r2");
            }
            _ => panic!("expected inconsistent remote"),
        }
        set_master_password(None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn new_protocol_reads_latest_snapshot() {
        let _guard = MASTER_PASSWORD_TEST_LOCK.lock().expect("lock password");
        set_master_password(Some("secret".to_string()));
        let (_memory, remote) = memory_remote();
        let pointer = write_committed_snapshot(&remote, "r2").await;

        let snapshot = read_snapshot_for_pointer(&remote, "nyaterm", &pointer)
            .await
            .expect("read snapshot");

        assert_eq!(snapshot.revision_id, "r2");
        set_master_password(None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn legacy_current_is_migrated_to_snapshot_path() {
        let _guard = MASTER_PASSWORD_TEST_LOCK.lock().expect("lock password");
        set_master_password(Some("secret".to_string()));
        let (memory, remote) = memory_remote();
        let snapshot = sample_snapshot("r1", 1);
        let pointer = pointer_from_snapshot(&snapshot);
        commit_sync_pointer(&remote, "nyaterm", &pointer)
            .await
            .expect("commit pointer");
        write_current_sync_snapshot_compat(&remote, "nyaterm", &snapshot)
            .await
            .expect("write current");

        let resolution = resolve_remote_snapshot(&remote, "nyaterm", &pointer)
            .await
            .expect("resolve legacy");

        assert!(matches!(
            resolution,
            RemoteSnapshotResolution::LegacyMigrated(_)
        ));
        assert!(
            memory
                .file(&remote_path("nyaterm", &sync_snapshot_file("r1")))
                .is_some()
        );
        set_master_password(None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn concurrent_update_is_detected_before_pointer_commit() {
        let _guard = MASTER_PASSWORD_TEST_LOCK.lock().expect("lock password");
        set_master_password(Some("secret".to_string()));
        let (_memory, remote) = memory_remote();
        let base_pointer = write_committed_snapshot(&remote, "r1").await;
        let next_pointer = pointer_from_snapshot(&sample_snapshot("r2", 2));
        commit_sync_pointer(&remote, "nyaterm", &next_pointer)
            .await
            .expect("commit competing pointer");

        let result = ensure_remote_head_unchanged(&remote, "nyaterm", Some(&base_pointer)).await;

        assert!(matches!(
            result,
            Err(AppError::CloudSync(CloudSyncError::ConcurrentUpdate { .. }))
        ));
        let latest = load_sync_pointer(&remote, "nyaterm")
            .await
            .expect("load latest")
            .expect("latest");
        assert_eq!(latest.revision_id, "r2");
        set_master_password(None);
    }

    #[test]
    fn pointer_snapshot_hash_mismatch_is_rejected() {
        let snapshot = sample_snapshot("r1", 1);
        let mut pointer = pointer_from_snapshot(&snapshot);
        pointer.payload_hash = "wrong".to_string();

        let result = validate_snapshot_against_pointer(&pointer, &snapshot);

        assert!(matches!(
            result,
            Err(AppError::CloudSync(CloudSyncError::HashMismatch { .. }))
        ));
    }
}
