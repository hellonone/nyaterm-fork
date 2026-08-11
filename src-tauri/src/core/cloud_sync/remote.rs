use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use redb::{Database, ReadableDatabase, TableDefinition};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

use super::operator::CloudRemote;

pub(super) const SYNC_CURRENT_FILE: &str = "sync/current.redb.enc";
pub(super) const SYNC_LATEST_FILE: &str = "sync/latest.redb";
pub(super) const SYNC_SNAPSHOTS_DIR: &str = "sync/snapshots/";
pub(super) const REMOTE_SYNC_POINTER_SCHEMA_VERSION: u32 = 2;

const REMOTE_SYNC_POINTER_TABLE: TableDefinition<&str, &str> = TableDefinition::new("sync_pointer");

const REMOTE_SYNC_POINTER_KEY: &str = "latest";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct RemoteSyncPointer {
    #[serde(default = "default_remote_sync_pointer_schema_version")]
    pub schema_version: u32,
    pub revision_id: String,
    pub created_at_ms: u64,
    pub payload_hash: String,
    pub device_id: String,
    pub app_version: String,
}

fn default_remote_sync_pointer_schema_version() -> u32 {
    1
}

pub(super) fn remote_path(base_root: &str, child: &str) -> String {
    let root = base_root.trim().trim_matches('/');
    let child = child.trim().trim_start_matches('/');
    if root.is_empty() {
        child.to_string()
    } else if child.is_empty() {
        root.to_string()
    } else {
        format!("{root}/{child}")
    }
}

pub(super) fn legacy_sync_snapshot_file(revision: &str) -> String {
    format!("{SYNC_SNAPSHOTS_DIR}{revision}.redb.enc")
}

pub(super) fn is_legacy_sync_snapshot_path(path: &str, base_root: &str) -> bool {
    let prefix = remote_path(base_root, SYNC_SNAPSHOTS_DIR);
    path.starts_with(&prefix) && path.ends_with(".redb.enc")
}

pub(super) fn current_time_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

pub(super) fn elapsed_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

pub(super) async fn load_sync_pointer(
    remote: &CloudRemote,
    base_root: &str,
) -> AppResult<Option<RemoteSyncPointer>> {
    let path = remote_path(base_root, SYNC_LATEST_FILE);
    let Some(raw) = remote.read_if_exists(&path).await? else {
        return Ok(None);
    };
    decode_redb_json_doc(
        raw.as_slice(),
        REMOTE_SYNC_POINTER_TABLE,
        REMOTE_SYNC_POINTER_KEY,
    )
    .map(Some)
}

pub(super) async fn write_sync_pointer(
    remote: &CloudRemote,
    base_root: &str,
    pointer: &RemoteSyncPointer,
) -> AppResult<()> {
    let encoded =
        encode_redb_json_doc(REMOTE_SYNC_POINTER_TABLE, REMOTE_SYNC_POINTER_KEY, pointer)?;
    remote
        .write(&remote_path(base_root, SYNC_LATEST_FILE), encoded)
        .await?;
    Ok(())
}

fn encode_redb_json_doc<T: Serialize>(
    table: TableDefinition<&str, &str>,
    key: &str,
    value: &T,
) -> AppResult<Vec<u8>> {
    let temp = TempRedbFile::new("cloud-meta-encode");
    {
        let db = Database::create(temp.path()).map_err(storage_error)?;
        let txn = db.begin_write().map_err(storage_error)?;
        {
            let mut docs = txn.open_table(table).map_err(storage_error)?;
            let content = serde_json::to_string(value)?;
            docs.insert(key, content.as_str()).map_err(storage_error)?;
        }
        txn.commit().map_err(storage_error)?;
    }
    std::fs::read(temp.path()).map_err(Into::into)
}

fn decode_redb_json_doc<T: DeserializeOwned>(
    bytes: &[u8],
    table: TableDefinition<&str, &str>,
    key: &str,
) -> AppResult<T> {
    let content = read_redb_json_doc(bytes, table, key)?;
    serde_json::from_str(&content).map_err(Into::into)
}

fn read_redb_json_doc(
    bytes: &[u8],
    table: TableDefinition<&str, &str>,
    key: &str,
) -> AppResult<String> {
    catch_unwind(AssertUnwindSafe(|| {
        let temp = TempRedbFile::new("cloud-meta-decode");
        std::fs::write(temp.path(), bytes)?;
        let content = {
            let db = Database::open(temp.path()).map_err(storage_error)?;
            let read = db.begin_read().map_err(storage_error)?;
            let docs = read.open_table(table).map_err(storage_error)?;
            docs.get(key)
                .map_err(storage_error)?
                .ok_or_else(|| AppError::Config("remote redb metadata is missing".to_string()))?
                .value()
                .to_string()
        };
        Ok(content)
    }))
    .unwrap_or_else(|_| {
        Err(AppError::Storage(
            "Remote redb metadata is corrupt or incomplete".to_string(),
        ))
    })
}

fn storage_error(error: impl std::fmt::Display) -> AppError {
    AppError::Storage(format!("Storage error: {error}"))
}

struct TempRedbFile {
    path: PathBuf,
}

impl TempRedbFile {
    fn new(prefix: &str) -> Self {
        Self {
            path: std::env::temp_dir()
                .join(format!("nyaterm-{prefix}-{}.redb", uuid::Uuid::new_v4())),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempRedbFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_path_joins_without_duplicate_slashes() {
        assert_eq!(
            remote_path("nyaterm", "sync/latest.redb"),
            "nyaterm/sync/latest.redb"
        );
        assert_eq!(
            remote_path("/nyaterm/", "/sync/latest.redb"),
            "nyaterm/sync/latest.redb"
        );
        assert_eq!(remote_path("", "sync/latest.redb"), "sync/latest.redb");
    }

    #[test]
    fn legacy_sync_snapshot_path_helpers_match_old_layout() {
        assert_eq!(
            legacy_sync_snapshot_file("rev"),
            "sync/snapshots/rev.redb.enc"
        );
        assert!(is_legacy_sync_snapshot_path(
            "nyaterm/sync/snapshots/rev.redb.enc",
            "nyaterm"
        ));
        assert!(!is_legacy_sync_snapshot_path(
            "nyaterm/sync/current.redb.enc",
            "nyaterm"
        ));
        assert!(!is_legacy_sync_snapshot_path(
            "nyaterm/backups/snapshots/rev.redb.enc",
            "nyaterm"
        ));
    }

    #[test]
    fn remote_redb_metadata_roundtrips() {
        let pointer = RemoteSyncPointer {
            schema_version: REMOTE_SYNC_POINTER_SCHEMA_VERSION,
            revision_id: "rev".to_string(),
            created_at_ms: 1,
            payload_hash: "hash".to_string(),
            device_id: "dev".to_string(),
            app_version: "1.0.0".to_string(),
        };
        let encoded =
            encode_redb_json_doc(REMOTE_SYNC_POINTER_TABLE, REMOTE_SYNC_POINTER_KEY, &pointer)
                .expect("encode pointer");
        let decoded: RemoteSyncPointer =
            decode_redb_json_doc(&encoded, REMOTE_SYNC_POINTER_TABLE, REMOTE_SYNC_POINTER_KEY)
                .expect("decode pointer");

        assert_eq!(decoded.revision_id, pointer.revision_id);
        assert_eq!(decoded.payload_hash, pointer.payload_hash);
        assert_eq!(decoded.schema_version, REMOTE_SYNC_POINTER_SCHEMA_VERSION);
    }

    #[test]
    fn corrupt_remote_redb_metadata_returns_error() {
        let error = decode_redb_json_doc::<RemoteSyncPointer>(
            b"not a redb file",
            REMOTE_SYNC_POINTER_TABLE,
            REMOTE_SYNC_POINTER_KEY,
        )
        .expect_err("corrupt metadata should fail");

        assert!(matches!(error, AppError::Storage(_)));
    }
}
