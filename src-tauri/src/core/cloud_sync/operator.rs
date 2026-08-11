use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::Duration;

#[cfg(test)]
use std::sync::Mutex as StdMutex;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD as BASE64_STANDARD, URL_SAFE_NO_PAD};
use http::header::{AUTHORIZATION, WWW_AUTHENTICATE};
use http::{HeaderValue, Request, Response};
use md5::{Digest as Md5Digest, Md5};
use opendal::layers::{RetryLayer, TimeoutLayer, TracingLayer};
use opendal::services::{AliyunDrive, Gdrive, Onedrive, S3, Webdav};
use opendal::{
    Buffer, EntryMode, Error, ErrorKind, HttpBody, HttpTransport, HttpTransporter,
    OperationContext, Operator,
};
use rand::RngCore;
use sha2::Sha256;

use crate::config::CloudSyncSettings;
use crate::error::{AppError, AppResult};
use crate::utils::url::normalize_storage_endpoint;

use super::remote::remote_path;

const GITEE_REMOTE_FILE_PREFIX: &str = "nyaterm-";
const GITEE_REMOTE_FILE_SUFFIX: &str = ".blob";
const GITEE_REMOTE_TIMEOUT: Duration = Duration::from_secs(30);
const GITHUB_GIST_API_ENDPOINT: &str = "https://api.github.com";
const GITHUB_GIST_REMOTE_TIMEOUT: Duration = Duration::from_secs(30);
const GITHUB_GIST_CONFLICT_RETRY_DELAY: Duration = Duration::from_millis(750);
const GITHUB_API_VERSION: &str = "2022-11-28";

#[derive(Clone)]
pub(super) enum CloudRemote {
    OpenDal(Operator),
    GiteeSnippet(GiteeSnippetRemote),
    GithubGist(GithubGistRemote),
    #[cfg(test)]
    Memory(MemoryRemote),
}

impl CloudRemote {
    pub(super) async fn create_dir(&self, path: &str) -> AppResult<()> {
        match self {
            Self::OpenDal(operator) => operator.create_dir(path).await.map_err(map_storage_error),
            Self::GiteeSnippet(_) => Ok(()),
            Self::GithubGist(_) => Ok(()),
            #[cfg(test)]
            Self::Memory(remote) => remote.create_dir(path),
        }
    }

    pub(super) async fn exists(&self, path: &str) -> AppResult<bool> {
        match self {
            Self::OpenDal(operator) => operator.exists(path).await.map_err(map_storage_error),
            Self::GiteeSnippet(remote) => remote.exists(path).await,
            Self::GithubGist(remote) => remote.exists(path).await,
            #[cfg(test)]
            Self::Memory(remote) => remote.exists(path),
        }
    }

    pub(super) async fn read_if_exists(&self, path: &str) -> AppResult<Option<Vec<u8>>> {
        match self {
            Self::OpenDal(operator) => {
                if !operator.exists(path).await.map_err(map_storage_error)? {
                    return Ok(None);
                }
                Ok(Some(
                    operator
                        .read(path)
                        .await
                        .map_err(map_storage_error)?
                        .to_vec(),
                ))
            }
            Self::GiteeSnippet(remote) => remote.read_if_exists(path).await,
            Self::GithubGist(remote) => remote.read_if_exists(path).await,
            #[cfg(test)]
            Self::Memory(remote) => remote.read_if_exists(path),
        }
    }

    pub(super) async fn write(&self, path: &str, content: Vec<u8>) -> AppResult<()> {
        match self {
            Self::OpenDal(operator) => {
                operator
                    .write(path, content)
                    .await
                    .map_err(map_storage_error)?;
                Ok(())
            }
            Self::GiteeSnippet(remote) => remote.write(path, &content).await,
            Self::GithubGist(remote) => remote.write(path, &content).await,
            #[cfg(test)]
            Self::Memory(remote) => remote.write(path, content),
        }
    }

    pub(super) async fn delete(&self, path: &str) -> AppResult<()> {
        match self {
            Self::OpenDal(operator) => operator.delete(path).await.map_err(map_storage_error),
            Self::GiteeSnippet(remote) => remote.delete(path).await,
            Self::GithubGist(remote) => remote.delete(path).await,
            #[cfg(test)]
            Self::Memory(remote) => remote.delete(path),
        }
    }

    pub(super) async fn list_files(&self, path: &str) -> AppResult<Vec<String>> {
        match self {
            Self::OpenDal(operator) => {
                let entries = operator.list(path).await.map_err(map_storage_error)?;
                Ok(entries
                    .into_iter()
                    .filter_map(|entry| {
                        if entry.metadata().mode() == EntryMode::FILE {
                            Some(entry.path().to_string())
                        } else {
                            None
                        }
                    })
                    .collect())
            }
            Self::GiteeSnippet(remote) => remote.list_files(path).await,
            Self::GithubGist(remote) => remote.list_files(path).await,
            #[cfg(test)]
            Self::Memory(remote) => remote.list_files(path),
        }
    }
}

#[cfg(test)]
#[derive(Clone, Default)]
pub(super) struct MemoryRemote {
    files: Arc<StdMutex<HashMap<String, Vec<u8>>>>,
    fail_writes: Arc<StdMutex<Vec<String>>>,
}

#[cfg(test)]
impl MemoryRemote {
    pub(super) fn with_files(files: HashMap<String, Vec<u8>>) -> Self {
        Self {
            files: Arc::new(StdMutex::new(files)),
            fail_writes: Arc::new(StdMutex::new(Vec::new())),
        }
    }

    pub(super) fn fail_next_write_containing(&self, needle: &str) {
        self.fail_writes
            .lock()
            .expect("lock fail writes")
            .push(needle.to_string());
    }

    pub(super) fn file(&self, path: &str) -> Option<Vec<u8>> {
        self.files.lock().expect("lock files").get(path).cloned()
    }

    fn create_dir(&self, _path: &str) -> AppResult<()> {
        Ok(())
    }

    fn exists(&self, path: &str) -> AppResult<bool> {
        Ok(self.files.lock().expect("lock files").contains_key(path))
    }

    fn read_if_exists(&self, path: &str) -> AppResult<Option<Vec<u8>>> {
        Ok(self.files.lock().expect("lock files").get(path).cloned())
    }

    fn write(&self, path: &str, content: Vec<u8>) -> AppResult<()> {
        let mut fail_writes = self.fail_writes.lock().expect("lock fail writes");
        if let Some(index) = fail_writes
            .iter()
            .position(|needle| path.contains(needle.as_str()))
        {
            fail_writes.remove(index);
            return Err(AppError::Io(io::Error::new(
                io::ErrorKind::Other,
                format!("injected memory write failure for {path}"),
            )));
        }
        drop(fail_writes);
        self.files
            .lock()
            .expect("lock files")
            .insert(path.to_string(), content);
        Ok(())
    }

    fn delete(&self, path: &str) -> AppResult<()> {
        self.files.lock().expect("lock files").remove(path);
        Ok(())
    }

    fn list_files(&self, path: &str) -> AppResult<Vec<String>> {
        Ok(self
            .files
            .lock()
            .expect("lock files")
            .keys()
            .filter(|key| key.starts_with(path))
            .cloned()
            .collect())
    }
}

pub(super) fn build_remote(settings: &CloudSyncSettings) -> AppResult<CloudRemote> {
    opendal::install_default();
    match settings.provider.as_str() {
        "webdav" => build_webdav_operator(settings).map(CloudRemote::OpenDal),
        "s3" => build_s3_operator(settings).map(CloudRemote::OpenDal),
        "gitee_snippet" => GiteeSnippetRemote::new(settings).map(CloudRemote::GiteeSnippet),
        "google_drive" => build_google_drive_operator(settings).map(CloudRemote::OpenDal),
        "onedrive" => build_onedrive_operator(settings).map(CloudRemote::OpenDal),
        "aliyun_drive" => build_aliyun_drive_operator(settings).map(CloudRemote::OpenDal),
        "github_gist" => GithubGistRemote::new(settings).map(CloudRemote::GithubGist),
        other => Err(AppError::Config(format!(
            "Unsupported cloud provider '{}'",
            other
        ))),
    }
}

fn build_webdav_operator(settings: &CloudSyncSettings) -> AppResult<Operator> {
    let endpoint = normalize_storage_endpoint(&settings.webdav.endpoint);
    let mut builder = Webdav::default().endpoint(&endpoint);
    if !settings.webdav.root.trim().is_empty() {
        builder = builder.root(&settings.webdav.root);
    }
    if !settings.webdav.username.trim().is_empty() {
        builder = builder.username(&settings.webdav.username);
    }
    if let Some(password) = settings
        .webdav
        .password
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.password(password);
    }
    let digest_client = WebdavDigestHttpClient::new(
        settings.webdav.username.clone(),
        settings.webdav.password.clone().unwrap_or_default(),
    );
    let transport = HttpTransporter::new(digest_client);
    Ok(Operator::new(builder)
        .map_err(map_storage_error)?
        .with_context(OperationContext::new().with_http_transport(transport))
        .layer(storage_timeout_layer())
        .layer(RetryLayer::new().with_max_times(3))
        .layer(TracingLayer::new()))
}

fn build_s3_operator(settings: &CloudSyncSettings) -> AppResult<Operator> {
    let mut builder = S3::default().bucket(&settings.s3.bucket);
    let endpoint = normalize_storage_endpoint(&settings.s3.endpoint);
    if !endpoint.is_empty() {
        builder = builder.endpoint(&endpoint);
    }
    if !settings.s3.region.trim().is_empty() {
        builder = builder.region(&settings.s3.region);
    }
    if !settings.s3.root.trim().is_empty() {
        builder = builder.root(&settings.s3.root);
    }
    if let Some(access_key_id) = settings
        .s3
        .access_key_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.access_key_id(access_key_id);
    }
    if let Some(secret_access_key) = settings
        .s3
        .secret_access_key
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.secret_access_key(secret_access_key);
    }
    if let Some(session_token) = settings
        .s3
        .session_token
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.session_token(session_token);
    }
    if settings.s3.virtual_host_style {
        builder = builder.enable_virtual_host_style();
    }
    finish_opendal_operator(builder)
}

fn build_google_drive_operator(settings: &CloudSyncSettings) -> AppResult<Operator> {
    let mut builder = Gdrive::default();
    if !settings.google_drive.root.trim().is_empty() {
        builder = builder.root(&settings.google_drive.root);
    }
    if let Some(access_token) = settings
        .google_drive
        .access_token
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.access_token(access_token);
    }
    if let Some(refresh_token) = settings
        .google_drive
        .refresh_token
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.refresh_token(refresh_token);
    }
    if let Some(client_id) = settings
        .google_drive
        .client_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.client_id(client_id);
    }
    if let Some(client_secret) = settings
        .google_drive
        .client_secret
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.client_secret(client_secret);
    }
    finish_opendal_operator(builder)
}

fn build_onedrive_operator(settings: &CloudSyncSettings) -> AppResult<Operator> {
    let mut builder = Onedrive::default();
    if !settings.onedrive.root.trim().is_empty() {
        builder = builder.root(&settings.onedrive.root);
    }
    if let Some(access_token) = settings
        .onedrive
        .access_token
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.access_token(access_token);
    }
    if let Some(refresh_token) = settings
        .onedrive
        .refresh_token
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.refresh_token(refresh_token);
    }
    if let Some(client_id) = settings
        .onedrive
        .client_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.client_id(client_id);
    }
    if let Some(client_secret) = settings
        .onedrive
        .client_secret
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.client_secret(client_secret);
    }
    finish_opendal_operator(builder)
}

fn build_aliyun_drive_operator(settings: &CloudSyncSettings) -> AppResult<Operator> {
    let mut builder = AliyunDrive::default();
    if !settings.aliyun_drive.root.trim().is_empty() {
        builder = builder.root(&settings.aliyun_drive.root);
    }
    if !settings.aliyun_drive.drive_type.trim().is_empty() {
        builder = builder.drive_type(&settings.aliyun_drive.drive_type);
    }
    if let Some(access_token) = settings
        .aliyun_drive
        .access_token
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.access_token(access_token);
    }
    if let Some(refresh_token) = settings
        .aliyun_drive
        .refresh_token
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.refresh_token(refresh_token);
    }
    if let Some(client_id) = settings
        .aliyun_drive
        .client_id
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.client_id(client_id);
    }
    if let Some(client_secret) = settings
        .aliyun_drive
        .client_secret
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.client_secret(client_secret);
    }
    finish_opendal_operator(builder)
}

fn finish_opendal_operator(builder: impl opendal::Builder) -> AppResult<Operator> {
    Ok(Operator::new(builder)
        .map_err(map_storage_error)?
        .layer(storage_timeout_layer())
        .layer(RetryLayer::new().with_max_times(3))
        .layer(TracingLayer::new()))
}

fn storage_timeout_layer() -> TimeoutLayer {
    TimeoutLayer::new()
        .with_timeout(Duration::from_secs(30))
        .with_io_timeout(Duration::from_secs(30))
}

pub(super) async fn ensure_remote_layout(remote: &CloudRemote, base_root: &str) -> AppResult<()> {
    remote
        .create_dir(&remote_path(base_root, super::remote::SYNC_SNAPSHOTS_DIR))
        .await?;
    Ok(())
}

pub(super) fn map_storage_error(error: opendal::Error) -> AppError {
    let raw = error.to_string();
    if let Some(message) = map_webdav_auth_error(&raw) {
        return AppError::Config(message);
    }

    if is_storage_timeout_error(&raw) {
        return AppError::Io(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("cloud storage operation timed out: {raw}"),
        ));
    }

    if error.is_temporary() {
        return AppError::Io(io::Error::new(
            io::ErrorKind::Other,
            format!("temporary cloud storage error: {raw}"),
        ));
    }

    let label = match error.kind() {
        ErrorKind::NotFound => "not found",
        ErrorKind::PermissionDenied => "permission denied",
        ErrorKind::ConfigInvalid => "invalid config",
        ErrorKind::Unsupported => "unsupported",
        ErrorKind::RateLimited => "rate limited",
        _ => "unexpected error",
    };
    AppError::Config(format!("cloud storage {label}: {raw}"))
}

fn is_storage_timeout_error(raw: &str) -> bool {
    let lower = raw.to_ascii_lowercase();
    lower.contains("operation timeout")
        || lower.contains("io timeout")
        || lower.contains("timed out")
        || lower.contains("deadline has elapsed")
}

fn map_webdav_auth_error(raw: &str) -> Option<String> {
    let lower = raw.to_ascii_lowercase();
    let is_webdav = lower.contains("service: webdav");
    let is_unauthorized = lower.contains("status: 401") || lower.contains("401 unauthorized");

    if is_webdav && is_unauthorized {
        return Some(
            "WebDAV authentication failed (401 Unauthorized). Verify the endpoint, username, password or app password, and the authentication methods enabled by your WebDAV provider."
                .to_string(),
        );
    }

    None
}

#[derive(Clone)]
pub(super) struct GiteeSnippetRemote {
    client: reqwest::Client,
    api_endpoint: String,
    gist_id: String,
    access_token: String,
}

#[derive(Debug, serde::Deserialize)]
struct GiteeSnippet {
    #[serde(default)]
    files: HashMap<String, GiteeSnippetFile>,
}

#[derive(Debug, serde::Deserialize)]
struct GiteeSnippetFile {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    raw_url: Option<String>,
}

impl GiteeSnippetRemote {
    fn new(settings: &CloudSyncSettings) -> AppResult<Self> {
        let api_endpoint = normalize_storage_endpoint(&settings.gitee_snippet.api_endpoint);
        let gist_id = settings.gitee_snippet.gist_id.trim().to_string();
        let access_token = settings
            .gitee_snippet
            .access_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::Config("Gitee access token is required".to_string()))?
            .to_string();

        if api_endpoint.is_empty() {
            return Err(AppError::Config(
                "Gitee API endpoint is required".to_string(),
            ));
        }
        if gist_id.is_empty() {
            return Err(AppError::Config("Gitee snippet ID is required".to_string()));
        }

        let client = reqwest::Client::builder()
            .timeout(GITEE_REMOTE_TIMEOUT)
            .build()
            .map_err(map_gitee_client_error)?;

        Ok(Self {
            client,
            api_endpoint,
            gist_id,
            access_token,
        })
    }

    async fn exists(&self, path: &str) -> AppResult<bool> {
        let snippet = self.fetch_snippet().await?;
        Ok(snippet.files.contains_key(&gitee_remote_filename(path)))
    }

    async fn read_if_exists(&self, path: &str) -> AppResult<Option<Vec<u8>>> {
        let filename = gitee_remote_filename(path);
        if let Ok(content) = self.fetch_raw_filename(&filename).await {
            return decode_gitee_file_content(&content).map(Some);
        }

        let snippet = self.fetch_snippet().await?;
        let Some(file) = snippet.files.get(&filename) else {
            return Ok(None);
        };
        let content = match file
            .content
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            Some(content) => content.to_string(),
            None => self.fetch_raw_file(&filename, file).await?,
        };
        decode_gitee_file_content(&content).map(Some)
    }

    async fn write(&self, path: &str, content: &[u8]) -> AppResult<()> {
        let encoded = BASE64_STANDARD.encode(content);
        self.patch_file(&gitee_remote_filename(path), encoded).await
    }

    async fn delete(&self, path: &str) -> AppResult<()> {
        self.delete_file(&gitee_remote_filename(path)).await
    }

    async fn list_files(&self, path: &str) -> AppResult<Vec<String>> {
        let snippet = self.fetch_snippet().await?;
        let prefix = path.trim_start_matches('/');
        Ok(snippet
            .files
            .keys()
            .filter_map(|filename| gitee_remote_path(filename))
            .filter(|remote_path| remote_path.starts_with(prefix))
            .collect())
    }

    async fn fetch_snippet(&self) -> AppResult<GiteeSnippet> {
        let url = format!("{}/gists/{}", self.api_endpoint, self.gist_id);
        let response = self
            .client
            .get(url)
            .query(&[("access_token", self.access_token.as_str())])
            .send()
            .await
            .map_err(map_gitee_client_error)?;
        decode_gitee_response(response).await
    }

    async fn fetch_raw_file(&self, filename: &str, file: &GiteeSnippetFile) -> AppResult<String> {
        let raw_url = file.raw_url.as_deref().filter(|value| !value.is_empty());
        let url = raw_url.map(str::to_string).unwrap_or_else(|| {
            format!(
                "{}/gists/{}/raw/{}",
                self.api_endpoint, self.gist_id, filename
            )
        });
        let response = self
            .client
            .get(url)
            .query(&[("access_token", self.access_token.as_str())])
            .send()
            .await
            .map_err(map_gitee_client_error)?;
        decode_gitee_text_response(response).await
    }

    async fn fetch_raw_filename(&self, filename: &str) -> AppResult<String> {
        let url = format!(
            "{}/gists/{}/raw/{}",
            self.api_endpoint, self.gist_id, filename
        );
        let response = self
            .client
            .get(url)
            .query(&[("access_token", self.access_token.as_str())])
            .send()
            .await
            .map_err(map_gitee_client_error)?;
        decode_gitee_text_response(response).await
    }

    async fn patch_file(&self, filename: &str, content: String) -> AppResult<()> {
        let file_value = serde_json::json!({ "content": content });
        let mut files = serde_json::Map::new();
        files.insert(filename.to_string(), file_value);
        self.patch_files(files).await
    }

    async fn delete_file(&self, filename: &str) -> AppResult<()> {
        let mut files = serde_json::Map::new();
        files.insert(filename.to_string(), serde_json::Value::Null);
        self.patch_files(files).await
    }

    async fn patch_files(
        &self,
        files: serde_json::Map<String, serde_json::Value>,
    ) -> AppResult<()> {
        let body = gitee_patch_body(self.access_token.as_str(), files);
        let url = format!("{}/gists/{}", self.api_endpoint, self.gist_id);
        let response = self
            .client
            .patch(url)
            .json(&body)
            .send()
            .await
            .map_err(map_gitee_client_error)?;
        let _: serde_json::Value = decode_gitee_response(response).await?;
        Ok(())
    }
}

fn gitee_remote_filename(path: &str) -> String {
    format!(
        "{}{}{}",
        GITEE_REMOTE_FILE_PREFIX,
        URL_SAFE_NO_PAD.encode(path.as_bytes()),
        GITEE_REMOTE_FILE_SUFFIX
    )
}

fn gitee_remote_path(filename: &str) -> Option<String> {
    let encoded = filename
        .strip_prefix(GITEE_REMOTE_FILE_PREFIX)?
        .strip_suffix(GITEE_REMOTE_FILE_SUFFIX)?;
    let bytes = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    String::from_utf8(bytes).ok()
}

fn gitee_patch_body(
    access_token: &str,
    files: serde_json::Map<String, serde_json::Value>,
) -> serde_json::Value {
    serde_json::json!({
        "access_token": access_token,
        "files": files,
    })
}

fn decode_gitee_file_content(content: &str) -> AppResult<Vec<u8>> {
    BASE64_STANDARD
        .decode(content.trim())
        .map_err(|error| AppError::Config(format!("Invalid Gitee snippet content: {error}")))
}

async fn decode_gitee_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> AppResult<T> {
    let text = decode_gitee_text_response(response).await?;
    serde_json::from_str(&text).map_err(Into::into)
}

async fn decode_gitee_text_response(response: reqwest::Response) -> AppResult<String> {
    let status = response.status();
    let text = response.text().await.map_err(map_gitee_client_error)?;
    if !status.is_success() {
        return Err(AppError::Config(format!(
            "Gitee snippet request failed ({status}): {}",
            text.trim()
        )));
    }
    Ok(text)
}

fn map_gitee_client_error(error: reqwest::Error) -> AppError {
    if error.is_timeout() {
        AppError::Io(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("Gitee snippet operation timed out: {error}"),
        ))
    } else {
        AppError::Config(format!("Gitee snippet request failed: {error}"))
    }
}

#[derive(Clone)]
pub(super) struct GithubGistRemote {
    client: reqwest::Client,
    gist_id: String,
    access_token: String,
}

#[derive(Debug, serde::Deserialize)]
struct GithubGist {
    #[serde(default)]
    files: HashMap<String, GithubGistFile>,
}

#[derive(Debug, serde::Deserialize)]
struct GithubGistFile {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    raw_url: Option<String>,
    #[serde(default)]
    truncated: bool,
}

impl GithubGistRemote {
    fn new(settings: &CloudSyncSettings) -> AppResult<Self> {
        let gist_id = settings.github_gist.gist_id.trim().to_string();
        let access_token = settings
            .github_gist
            .access_token
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::Config("GitHub Gist access token is required".to_string()))?
            .to_string();

        if gist_id.is_empty() {
            return Err(AppError::Config("GitHub Gist ID is required".to_string()));
        }

        let client = github_client()?;

        Ok(Self {
            client,
            gist_id,
            access_token,
        })
    }

    async fn exists(&self, path: &str) -> AppResult<bool> {
        let gist = self.fetch_gist().await?;
        Ok(gist.files.contains_key(&github_gist_remote_filename(path)))
    }

    async fn read_if_exists(&self, path: &str) -> AppResult<Option<Vec<u8>>> {
        let filename = github_gist_remote_filename(path);
        let gist = self.fetch_gist().await?;
        let Some(file) = gist.files.get(&filename) else {
            return Ok(None);
        };
        let content = if file.truncated {
            self.fetch_raw_file(file).await?
        } else {
            match file
                .content
                .as_deref()
                .filter(|value| !value.trim().is_empty())
            {
                Some(content) => content.to_string(),
                None => self.fetch_raw_file(file).await?,
            }
        };
        decode_github_gist_file_content(&content).map(Some)
    }

    async fn write(&self, path: &str, content: &[u8]) -> AppResult<()> {
        let encoded = BASE64_STANDARD.encode(content);
        self.patch_file(&github_gist_remote_filename(path), encoded)
            .await
    }

    async fn delete(&self, path: &str) -> AppResult<()> {
        self.delete_file(&github_gist_remote_filename(path)).await
    }

    async fn list_files(&self, path: &str) -> AppResult<Vec<String>> {
        let gist = self.fetch_gist().await?;
        let prefix = path.trim_start_matches('/');
        Ok(gist
            .files
            .keys()
            .filter_map(|filename| github_gist_remote_path(filename))
            .filter(|remote_path| remote_path.starts_with(prefix))
            .collect())
    }

    async fn fetch_gist(&self) -> AppResult<GithubGist> {
        let response = self
            .client
            .get(format!("{GITHUB_GIST_API_ENDPOINT}/gists/{}", self.gist_id))
            .bearer_auth(&self.access_token)
            .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
            .send()
            .await
            .map_err(map_github_gist_client_error)?;
        decode_github_gist_response(response).await
    }

    async fn fetch_raw_file(&self, file: &GithubGistFile) -> AppResult<String> {
        let raw_url = file
            .raw_url
            .as_deref()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| AppError::Config("GitHub Gist file raw URL is missing".to_string()))?;
        let response = self
            .client
            .get(raw_url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .map_err(map_github_gist_client_error)?;
        decode_github_gist_text_response(response).await
    }

    async fn patch_file(&self, filename: &str, content: String) -> AppResult<()> {
        let file_value = serde_json::json!({ "content": content });
        let mut files = serde_json::Map::new();
        files.insert(filename.to_string(), file_value);
        self.patch_files(files).await
    }

    async fn delete_file(&self, filename: &str) -> AppResult<()> {
        let mut files = serde_json::Map::new();
        files.insert(filename.to_string(), serde_json::Value::Null);
        self.patch_files(files).await
    }

    async fn patch_files(
        &self,
        files: serde_json::Map<String, serde_json::Value>,
    ) -> AppResult<()> {
        match self.patch_files_once(&files).await {
            Ok(()) => Ok(()),
            Err(error) if is_github_gist_update_conflict(&error) => {
                tracing::warn!(
                    gist_id = %self.gist_id,
                    "GitHub Gist update conflict; retrying once"
                );
                tokio::time::sleep(GITHUB_GIST_CONFLICT_RETRY_DELAY).await;
                self.patch_files_once(&files).await
            }
            Err(error) => Err(error),
        }
    }

    async fn patch_files_once(
        &self,
        files: &serde_json::Map<String, serde_json::Value>,
    ) -> AppResult<()> {
        let body = serde_json::json!({ "files": files });
        let response = self
            .client
            .patch(format!("{GITHUB_GIST_API_ENDPOINT}/gists/{}", self.gist_id))
            .bearer_auth(&self.access_token)
            .header("X-GitHub-Api-Version", GITHUB_API_VERSION)
            .json(&body)
            .send()
            .await
            .map_err(map_github_gist_client_error)?;
        let _: serde_json::Value = decode_github_gist_response(response).await?;
        Ok(())
    }
}

fn github_client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(GITHUB_GIST_REMOTE_TIMEOUT)
        .user_agent("NyaTerm")
        .build()
        .map_err(map_github_gist_client_error)
}

fn github_gist_remote_filename(path: &str) -> String {
    format!(
        "{}{}{}",
        GITEE_REMOTE_FILE_PREFIX,
        URL_SAFE_NO_PAD.encode(path.as_bytes()),
        GITEE_REMOTE_FILE_SUFFIX
    )
}

fn github_gist_remote_path(filename: &str) -> Option<String> {
    let encoded = filename
        .strip_prefix(GITEE_REMOTE_FILE_PREFIX)?
        .strip_suffix(GITEE_REMOTE_FILE_SUFFIX)?;
    let bytes = URL_SAFE_NO_PAD.decode(encoded).ok()?;
    String::from_utf8(bytes).ok()
}

fn decode_github_gist_file_content(content: &str) -> AppResult<Vec<u8>> {
    BASE64_STANDARD
        .decode(content.trim())
        .map_err(|error| AppError::Config(format!("Invalid GitHub Gist content: {error}")))
}

async fn decode_github_gist_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> AppResult<T> {
    let text = decode_github_gist_text_response(response).await?;
    serde_json::from_str(&text).map_err(Into::into)
}

async fn decode_github_gist_text_response(response: reqwest::Response) -> AppResult<String> {
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(map_github_gist_client_error)?;
    if !status.is_success() {
        return Err(AppError::Config(format!(
            "GitHub Gist request failed ({status}): {}",
            text.trim()
        )));
    }
    Ok(text)
}

fn map_github_gist_client_error(error: reqwest::Error) -> AppError {
    if error.is_timeout() {
        AppError::Io(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("GitHub Gist operation timed out: {error}"),
        ))
    } else {
        AppError::Config(format!("GitHub Gist request failed: {error}"))
    }
}

fn is_github_gist_update_conflict(error: &AppError) -> bool {
    matches!(
        error,
        AppError::Config(message)
            if message.contains("GitHub Gist request failed (409 Conflict)")
                && message.contains("Gist cannot be updated")
    )
}

#[derive(Clone)]
struct WebdavDigestHttpClient {
    inner: HttpTransporter,
    username: Arc<str>,
    password: Arc<str>,
}

impl WebdavDigestHttpClient {
    fn new(username: String, password: String) -> Self {
        Self {
            inner: HttpTransporter::default(),
            username: Arc::from(username),
            password: Arc::from(password),
        }
    }
}

impl HttpTransport for WebdavDigestHttpClient {
    async fn fetch(&self, req: Request<Buffer>) -> opendal::Result<Response<HttpBody>> {
        let retry_req = clone_request(&req)?;
        let resp = self.inner.fetch(req).await?;
        if resp.status() != http::StatusCode::UNAUTHORIZED {
            return Ok(resp);
        }

        let Some(challenge) = digest_challenge(resp.headers()) else {
            return Ok(resp);
        };
        if self.username.is_empty() || self.password.is_empty() {
            return Ok(resp);
        }

        let auth = build_digest_authorization(
            &challenge,
            self.username.as_ref(),
            self.password.as_ref(),
            retry_req.method().as_str(),
            retry_req
                .uri()
                .path_and_query()
                .map_or("/", |path| path.as_str()),
            &random_cnonce(),
            "00000001",
        )?;
        let mut retry_req = retry_req;
        let header = HeaderValue::from_str(&auth).map_err(|err| {
            Error::new(
                ErrorKind::Unexpected,
                "build WebDAV Digest authorization header",
            )
            .set_source(err)
        })?;
        retry_req.headers_mut().insert(AUTHORIZATION, header);
        self.inner.fetch(retry_req).await
    }
}

fn clone_request(req: &Request<Buffer>) -> opendal::Result<Request<Buffer>> {
    let mut builder = Request::builder()
        .method(req.method().clone())
        .uri(req.uri().clone())
        .version(req.version());
    *builder.headers_mut().expect("request builder has headers") = req.headers().clone();
    builder.body(req.body().clone()).map_err(|err| {
        Error::new(ErrorKind::Unexpected, "clone WebDAV Digest retry request").set_source(err)
    })
}

fn digest_challenge(headers: &http::HeaderMap) -> Option<String> {
    headers
        .get_all(WWW_AUTHENTICATE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .find_map(|value| {
            value
                .split_once("Digest")
                .map(|(_, challenge)| challenge.trim().to_string())
        })
        .filter(|value| !value.is_empty())
}

fn build_digest_authorization(
    challenge: &str,
    username: &str,
    password: &str,
    method: &str,
    uri: &str,
    cnonce: &str,
    nc: &str,
) -> opendal::Result<String> {
    let params = parse_digest_challenge(challenge);
    let realm = required_digest_param(&params, "realm")?;
    let nonce = required_digest_param(&params, "nonce")?;
    let qop = choose_digest_qop(params.get("qop").map(String::as_str))?;
    let algorithm = params
        .get("algorithm")
        .map_or("MD5", String::as_str)
        .trim()
        .to_ascii_uppercase();

    let ha1 = digest_hash(&algorithm, &format!("{username}:{realm}:{password}"))?;
    let ha2 = digest_hash(&algorithm, &format!("{method}:{uri}"))?;
    let response = digest_hash(
        &algorithm,
        &format!("{ha1}:{nonce}:{nc}:{cnonce}:{qop}:{ha2}"),
    )?;

    let opaque = params
        .get("opaque")
        .map(|value| format!(", opaque=\"{}\"", escape_digest_value(value)))
        .unwrap_or_default();

    Ok(format!(
        "Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", algorithm={}, response=\"{}\", qop={}, nc={}, cnonce=\"{}\"{}",
        escape_digest_value(username),
        escape_digest_value(realm),
        escape_digest_value(nonce),
        escape_digest_value(uri),
        algorithm,
        response,
        qop,
        nc,
        escape_digest_value(cnonce),
        opaque
    ))
}

fn parse_digest_challenge(challenge: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    let mut rest = challenge.trim();
    while !rest.is_empty() {
        rest = rest.trim_start_matches(|ch: char| ch == ',' || ch.is_whitespace());
        let Some((key, after_key)) = rest.split_once('=') else {
            break;
        };
        let key = key.trim().to_ascii_lowercase();
        let after_key = after_key.trim_start();
        let (value, next) = if let Some(quoted) = after_key.strip_prefix('"') {
            parse_quoted_digest_value(quoted)
        } else {
            let split_at = after_key.find(',').unwrap_or(after_key.len());
            (
                after_key[..split_at].trim().to_string(),
                after_key[split_at..].trim_start_matches(','),
            )
        };
        if !key.is_empty() {
            values.insert(key, value);
        }
        rest = next;
    }
    values
}

fn parse_quoted_digest_value(input: &str) -> (String, &str) {
    let mut value = String::new();
    let mut escaped = false;
    for (index, ch) in input.char_indices() {
        if escaped {
            value.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => return (value, &input[index + ch.len_utf8()..]),
            _ => value.push(ch),
        }
    }
    (value, "")
}

fn required_digest_param<'a>(
    params: &'a HashMap<String, String>,
    key: &str,
) -> opendal::Result<&'a str> {
    params
        .get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error::new(
                ErrorKind::ConfigInvalid,
                format!("WebDAV Digest authentication challenge is missing {key}"),
            )
        })
}

fn choose_digest_qop(qop: Option<&str>) -> opendal::Result<&'static str> {
    let Some(qop) = qop else {
        return Err(Error::new(
            ErrorKind::Unsupported,
            "WebDAV Digest authentication without qop=auth is not supported",
        ));
    };
    if qop
        .split(',')
        .map(|value| value.trim().trim_matches('"').to_ascii_lowercase())
        .any(|value| value == "auth")
    {
        Ok("auth")
    } else {
        Err(Error::new(
            ErrorKind::Unsupported,
            "WebDAV Digest authentication requires qop=auth",
        ))
    }
}

fn digest_hash(algorithm: &str, value: &str) -> opendal::Result<String> {
    match algorithm {
        "MD5" => {
            let mut hasher = Md5::new();
            hasher.update(value.as_bytes());
            Ok(hex::encode(hasher.finalize()))
        }
        "SHA-256" | "SHA256" => {
            let mut hasher = Sha256::new();
            hasher.update(value.as_bytes());
            Ok(hex::encode(hasher.finalize()))
        }
        other => Err(Error::new(
            ErrorKind::Unsupported,
            format!("WebDAV Digest algorithm {other} is not supported"),
        )),
    }
}

fn random_cnonce() -> String {
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn escape_digest_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn webdav_401_error_reports_generic_auth_hint() {
        let message = map_webdav_auth_error(
            "Unexpected (persistent) at stat, context: { service: webdav, response: Parts { status: 401 } } => 401 Unauthorized",
        );

        assert!(message.is_some());
        let message = message.unwrap();
        assert!(message.contains("WebDAV authentication failed"));
        assert!(!message.contains("currently supports"));
    }

    #[test]
    fn cloud_storage_endpoint_accepts_trailing_slashes() {
        assert_eq!(
            normalize_storage_endpoint("https://dav.example.com/remote.php/webdav"),
            "https://dav.example.com/remote.php/webdav"
        );
        assert_eq!(
            normalize_storage_endpoint("https://dav.example.com/remote.php/webdav/"),
            "https://dav.example.com/remote.php/webdav"
        );
        assert_eq!(
            normalize_storage_endpoint(" https://s3.example.com// "),
            "https://s3.example.com"
        );
    }

    #[test]
    fn digest_challenge_parser_handles_quoted_commas() {
        let parsed = parse_digest_challenge(
            r#"realm="Nya,Term", nonce="abc", algorithm=MD5, qop="auth,auth-int", opaque="xyz""#,
        );

        assert_eq!(parsed.get("realm").map(String::as_str), Some("Nya,Term"));
        assert_eq!(parsed.get("nonce").map(String::as_str), Some("abc"));
        assert_eq!(parsed.get("qop").map(String::as_str), Some("auth,auth-int"));
    }

    #[test]
    fn digest_authorization_supports_md5_qop_auth() {
        let header = build_digest_authorization(
            r#"realm="testrealm@host.com", qop="auth", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#,
            "Mufasa",
            "Circle Of Life",
            "GET",
            "/dir/index.html",
            "0a4f113b",
            "00000001",
        )
        .expect("digest auth header");

        assert!(header.contains("Digest username=\"Mufasa\""));
        assert!(header.contains("qop=auth"));
        assert!(header.contains("response=\"6629fae49393a05397450978507c4ef1\""));
    }

    #[test]
    fn digest_authorization_rejects_unsupported_qop() {
        let error = build_digest_authorization(
            r#"realm="test", qop="auth-int", nonce="abc""#,
            "user",
            "pass",
            "GET",
            "/",
            "cnonce",
            "00000001",
        )
        .expect_err("auth-int is unsupported");

        assert_eq!(error.kind(), ErrorKind::Unsupported);
    }

    #[test]
    fn non_webdav_error_does_not_report_digest_hint() {
        let message = map_webdav_auth_error(
            "Unexpected (persistent) at stat, context: { service: s3, response: Parts { status: 401 } } => 401 Unauthorized",
        );

        assert!(message.is_none());
    }

    #[test]
    fn gitee_remote_filename_is_path_safe() {
        let filename = gitee_remote_filename("nyaterm/sync/latest.redb");

        assert!(filename.starts_with(GITEE_REMOTE_FILE_PREFIX));
        assert!(filename.ends_with(GITEE_REMOTE_FILE_SUFFIX));
        assert!(!filename.contains('/'));
    }

    #[test]
    fn gitee_remote_filename_roundtrips_path() {
        let path = "nyaterm/sync/snapshots/rev.redb.enc";
        let filename = gitee_remote_filename(path);

        assert_eq!(gitee_remote_path(&filename).as_deref(), Some(path));
    }

    #[test]
    fn github_gist_remote_filename_roundtrips_path() {
        let path = "nyaterm/backups/snapshots/rev.redb.enc";
        let filename = github_gist_remote_filename(path);

        assert!(filename.starts_with(GITEE_REMOTE_FILE_PREFIX));
        assert!(filename.ends_with(GITEE_REMOTE_FILE_SUFFIX));
        assert!(!filename.contains('/'));
        assert_eq!(github_gist_remote_path(&filename).as_deref(), Some(path));
    }

    #[test]
    fn github_gist_file_deserializes_truncated_flag() {
        let file: GithubGistFile = serde_json::from_str(
            r#"{"content":"partial","raw_url":"https://gist.githubusercontent.com/raw","truncated":true}"#,
        )
        .expect("deserialize gist file");

        assert!(file.truncated);
    }

    #[test]
    fn github_gist_update_conflict_is_retryable() {
        let error = AppError::Config(
            "GitHub Gist request failed (409 Conflict): {\"message\":\"Gist cannot be updated.\"}"
                .to_string(),
        );

        assert!(is_github_gist_update_conflict(&error));
    }

    #[test]
    fn github_gist_non_conflict_error_is_not_retryable() {
        let error = AppError::Config(
            "GitHub Gist request failed (404 Not Found): {\"message\":\"Not Found\"}".to_string(),
        );

        assert!(!is_github_gist_update_conflict(&error));
    }

    #[test]
    fn gitee_delete_patch_body_marks_file_as_null() {
        let filename = gitee_remote_filename("nyaterm/sync/snapshots/rev.redb.enc");
        let mut files = serde_json::Map::new();
        files.insert(filename.clone(), serde_json::Value::Null);
        let body = gitee_patch_body("token", files);

        assert_eq!(body["access_token"], "token");
        assert!(body["files"][filename].is_null());
    }

    #[test]
    fn timeout_storage_error_maps_to_retryable_io() {
        let mapped = map_storage_error(
            Error::new(ErrorKind::Unexpected, "operation timeout reached").set_temporary(),
        );

        match mapped {
            AppError::Io(error) => assert_eq!(error.kind(), io::ErrorKind::TimedOut),
            other => panic!("expected timeout IO error, got {other:?}"),
        }
    }

    #[test]
    fn temporary_storage_error_maps_to_retryable_io() {
        let mapped = map_storage_error(
            Error::new(ErrorKind::Unexpected, "service temporarily unavailable").set_temporary(),
        );

        assert!(matches!(mapped, AppError::Io(_)));
    }

    #[test]
    fn webdav_401_storage_error_stays_config_error() {
        let mapped = map_storage_error(
            Error::new(
                ErrorKind::Unexpected,
                "Unexpected at stat, context: { service: webdav, response: Parts { status: 401 } } => 401 Unauthorized",
            )
            .set_temporary(),
        );

        match mapped {
            AppError::Config(message) => assert!(message.contains("WebDAV authentication failed")),
            other => panic!("expected config auth error, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_provider_reports_config_error() {
        let mut settings = CloudSyncSettings::default();
        settings.provider = "unknown".to_string();

        let error = match build_remote(&settings) {
            Ok(_) => panic!("unknown provider should fail"),
            Err(error) => error,
        };

        assert!(
            matches!(error, AppError::Config(message) if message.contains("Unsupported cloud provider"))
        );
    }
}
