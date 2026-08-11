//! SSH Agent connection adapters.
//!
//! Authentication and forwarding each create an independent Agent stream so
//! SSH channels never share a stateful AgentClient. Hardware keys remain
//! managed by the external Agent; NyaTerm only speaks the SSH Agent protocol
//! and never accesses USB or PKCS#11 devices directly.

use crate::config::SshAgentEndpoint;
use crate::error::{AppError, AppResult};
use russh::keys::agent::client::{AgentClient, AgentStream};
#[cfg(windows)]
use std::ffi::OsStr;
use std::path::Path;
use std::time::Duration;

/// Dynamic stream type shared by Unix sockets, Pageant, and Windows named pipes.
pub(crate) type DynamicAgentStream = Box<dyn AgentStream + Send + Unpin + 'static>;

/// Dynamic Agent client used for SSH public-key authentication.
pub(crate) type DynamicAgentClient = AgentClient<DynamicAgentStream>;

const AGENT_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg(windows)]
const WINDOWS_OPENSSH_AGENT_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";

/// Connect to the selected local SSH Agent and return its raw bidirectional stream.
///
/// This function is called only after the remote side opens an Agent channel,
/// so disabled forwarding never creates a local socket, named pipe, or Pageant
/// connection.
pub(crate) async fn connect_agent_stream(
    endpoint: &SshAgentEndpoint,
) -> AppResult<DynamicAgentStream> {
    match endpoint {
        SshAgentEndpoint::Auto => connect_auto().await,
        SshAgentEndpoint::Environment { variable } => {
            #[cfg(unix)]
            {
                let variable = normalize_environment_variable(variable)?;
                let path = std::env::var_os(variable).ok_or_else(|| {
                    AppError::Auth(format!(
                        "SSH Agent environment variable '{}' is not set",
                        variable
                    ))
                })?;
                connect_unix_path(Path::new(&path)).await
            }
            #[cfg(not(unix))]
            {
                let _ = variable;
                Err(AppError::Config(
                    "Environment variable SSH Agent is only supported on macOS and Linux"
                        .to_string(),
                ))
            }
        }
        SshAgentEndpoint::UnixSocket { path } => {
            #[cfg(unix)]
            {
                if path.trim().is_empty() {
                    return Err(AppError::Config(
                        "SSH Agent Unix socket path must not be empty".to_string(),
                    ));
                }
                connect_unix_path(Path::new(path)).await
            }
            #[cfg(not(unix))]
            {
                let _ = path;
                Err(AppError::Config(
                    "Unix socket SSH Agent is only supported on macOS and Linux".to_string(),
                ))
            }
        }
        SshAgentEndpoint::Pageant => {
            #[cfg(windows)]
            {
                connect_pageant().await
            }
            #[cfg(not(windows))]
            {
                Err(AppError::Config(
                    "Pageant SSH Agent is only supported on Windows".to_string(),
                ))
            }
        }
        SshAgentEndpoint::WindowsOpenSsh => {
            #[cfg(windows)]
            {
                connect_windows_openssh().await
            }
            #[cfg(not(windows))]
            {
                Err(AppError::Config(
                    "Windows OpenSSH Agent is only supported on Windows".to_string(),
                ))
            }
        }
    }
}

/// Connect to the Agent and wrap the stream in a russh authentication client.
pub(crate) async fn connect_agent_client(
    endpoint: &SshAgentEndpoint,
) -> AppResult<DynamicAgentClient> {
    Ok(AgentClient::connect(connect_agent_stream(endpoint).await?))
}

#[cfg(unix)]
async fn connect_auto() -> AppResult<DynamicAgentStream> {
    let path = std::env::var_os("SSH_AUTH_SOCK").ok_or_else(|| {
        AppError::Auth("SSH_AUTH_SOCK is not set; no SSH Agent is available".to_string())
    })?;
    connect_unix_path(Path::new(&path)).await
}

#[cfg(windows)]
async fn connect_auto() -> AppResult<DynamicAgentStream> {
    match connect_windows_openssh().await {
        Ok(stream) => Ok(stream),
        Err(open_ssh_error) => connect_pageant().await.map_err(|pageant_error| {
            AppError::Auth(format!(
                "Windows SSH Agent is unavailable (OpenSSH: {}; Pageant: {})",
                open_ssh_error, pageant_error
            ))
        }),
    }
}

#[cfg(not(any(unix, windows)))]
async fn connect_auto() -> AppResult<DynamicAgentStream> {
    Err(AppError::Config(
        "SSH Agent is not supported on this platform".to_string(),
    ))
}

#[cfg(unix)]
async fn connect_unix_path(path: &Path) -> AppResult<DynamicAgentStream> {
    let stream = tokio::time::timeout(AGENT_CONNECT_TIMEOUT, tokio::net::UnixStream::connect(path))
        .await
        .map_err(|_| AppError::Auth("SSH Agent connection timed out".to_string()))?
        .map_err(|error| {
            AppError::Auth(format!("SSH Agent connection failed: {}", error.kind()))
        })?;
    Ok(Box::new(stream))
}

#[cfg(windows)]
async fn connect_windows_openssh() -> AppResult<DynamicAgentStream> {
    let client = tokio::time::timeout(
        AGENT_CONNECT_TIMEOUT,
        AgentClient::connect_named_pipe(OsStr::new(WINDOWS_OPENSSH_AGENT_PIPE)),
    )
    .await
    .map_err(|_| AppError::Auth("Windows OpenSSH Agent connection timed out".to_string()))?
    .map_err(|error| {
        AppError::Auth(format!(
            "Windows OpenSSH Agent connection failed: {}",
            error
        ))
    })?;
    Ok(client.into_inner())
}

#[cfg(windows)]
async fn connect_pageant() -> AppResult<DynamicAgentStream> {
    let client = tokio::time::timeout(AGENT_CONNECT_TIMEOUT, AgentClient::connect_pageant())
        .await
        .map_err(|_| AppError::Auth("Pageant connection timed out".to_string()))?
        .map_err(|error| AppError::Auth(format!("Pageant connection failed: {}", error)))?;
    Ok(client.into_inner())
}

fn normalize_environment_variable(value: &str) -> AppResult<&str> {
    let variable = value.trim().trim_start_matches('$').trim();
    if variable.is_empty() {
        return Err(AppError::Config(
            "SSH Agent environment variable must not be empty".to_string(),
        ));
    }
    Ok(variable)
}

#[cfg(all(test, unix))]
mod protocol_tests {
    use super::*;
    use russh::keys::agent::AgentIdentity;
    use ssh_key::{Algorithm, PrivateKey};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::UnixListener;
    use tokio::sync::oneshot;
    use tokio::time::{Duration, timeout};

    const REQUEST_IDENTITIES: u8 = 11;
    const IDENTITIES_ANSWER: u8 = 12;
    const SIGN_REQUEST: u8 = 13;
    const SIGN_RESPONSE: u8 = 14;

    async fn read_frame(stream: &mut tokio::net::UnixStream) -> Vec<u8> {
        let mut length = [0; 4];
        stream.read_exact(&mut length).await.unwrap();
        let mut payload = vec![0; u32::from_be_bytes(length) as usize];
        stream.read_exact(&mut payload).await.unwrap();
        payload
    }

    async fn write_frame(stream: &mut tokio::net::UnixStream, payload: &[u8]) {
        let mut frame = vec![0; 4];
        frame.copy_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(payload);
        stream.write_all(&frame).await.unwrap();
        stream.flush().await.unwrap();
    }

    fn encode_string(value: &[u8], output: &mut Vec<u8>) {
        let mut length = [0; 4];
        length.copy_from_slice(&(value.len() as u32).to_be_bytes());
        output.extend_from_slice(&length);
        output.extend_from_slice(value);
    }

    #[tokio::test]
    async fn fake_unix_agent_lists_identity_and_returns_signature() {
        let socket_path =
            std::env::temp_dir().join(format!("nyaterm-agent-{}", uuid::Uuid::new_v4()));
        let listener = UnixListener::bind(&socket_path).unwrap();
        let mut rng = rand::thread_rng();
        let private_key = PrivateKey::random(&mut rng, Algorithm::Ed25519).unwrap();
        let public_key_blob = private_key.public_key().to_bytes().unwrap();

        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();

            let request = read_frame(&mut stream).await;
            assert_eq!(request, vec![REQUEST_IDENTITIES]);
            let mut identities = vec![IDENTITIES_ANSWER];
            identities.extend_from_slice(&1u32.to_be_bytes());
            encode_string(&public_key_blob, &mut identities);
            encode_string(b"fake-confirmed-key", &mut identities);
            write_frame(&mut stream, &identities).await;

            let request = read_frame(&mut stream).await;
            assert_eq!(request[0], SIGN_REQUEST);
            let mut signature = vec![SIGN_RESPONSE];
            let mut signature_blob = Vec::new();
            encode_string(b"ssh-ed25519", &mut signature_blob);
            encode_string(&[0x42; 64], &mut signature_blob);
            encode_string(&signature_blob, &mut signature);
            write_frame(&mut stream, &signature).await;
        });

        let endpoint = SshAgentEndpoint::UnixSocket {
            path: socket_path.to_string_lossy().into_owned(),
        };
        let mut client = connect_agent_client(&endpoint).await.unwrap();
        let identities = client.request_identities().await.unwrap();
        assert_eq!(identities.len(), 1);

        let identity = identities.into_iter().next().unwrap();
        assert!(matches!(identity, AgentIdentity::PublicKey { .. }));
        let signed = client
            .sign_request(&identity, None, b"authentication-data".to_vec())
            .await
            .unwrap();
        assert!(!signed.is_empty());

        timeout(Duration::from_secs(2), server)
            .await
            .expect("fake agent should finish the protocol exchange")
            .expect("fake agent task should not panic");
        let _ = tokio::fs::remove_file(socket_path).await;
    }

    #[tokio::test]
    async fn cancelling_identity_request_closes_stream_before_retry() {
        let socket_path =
            std::env::temp_dir().join(format!("nyaterm-agent-{}", uuid::Uuid::new_v4()));
        let listener = UnixListener::bind(&socket_path).unwrap();
        let (first_request_started_tx, mut first_request_started_rx) = oneshot::channel();

        let server = tokio::spawn(async move {
            let (mut first, _) = timeout(Duration::from_secs(2), listener.accept())
                .await
                .expect("fake Agent should accept the first connection")
                .expect("first fake Agent connection should succeed");
            let first_request = timeout(Duration::from_secs(2), read_frame(&mut first))
                .await
                .expect("fake Agent should receive the first identity request");
            assert_eq!(first_request, vec![REQUEST_IDENTITIES]);
            first_request_started_tx.send(()).unwrap();

            let mut byte = [0u8; 1];
            let first_closed = timeout(Duration::from_secs(2), first.read(&mut byte))
                .await
                .expect("cancelled Agent request should close the first stream")
                .expect("reading the closed Agent stream should succeed");
            assert_eq!(first_closed, 0);

            let (mut second, _) = timeout(Duration::from_secs(2), listener.accept())
                .await
                .expect("retry should open a fresh Agent stream")
                .expect("retry Agent connection should be accepted");
            let second_request = timeout(Duration::from_secs(2), read_frame(&mut second))
                .await
                .expect("fake Agent should receive the retry identity request");
            assert_eq!(second_request, vec![REQUEST_IDENTITIES]);
            write_frame(&mut second, &[IDENTITIES_ANSWER, 0, 0, 0, 0]).await;
        });

        let endpoint = SshAgentEndpoint::UnixSocket {
            path: socket_path.to_string_lossy().into_owned(),
        };
        let mut client = timeout(Duration::from_secs(2), connect_agent_client(&endpoint))
            .await
            .expect("first fake Agent connection should complete")
            .unwrap();
        {
            let request = client.request_identities();
            tokio::pin!(request);
            tokio::select! {
                _ = &mut request => panic!("identity request should remain pending"),
                started = timeout(Duration::from_secs(2), &mut first_request_started_rx) => {
                    started.expect("fake Agent should receive the first request before timeout")
                        .expect("fake Agent request-start signal should remain connected");
                }
            }
        }
        drop(client);

        let mut retry_client = timeout(Duration::from_secs(2), connect_agent_client(&endpoint))
            .await
            .expect("retry fake Agent connection should complete")
            .unwrap();
        let identities = timeout(Duration::from_secs(2), retry_client.request_identities())
            .await
            .expect("retry identity request should not inherit the cancelled stream")
            .expect("retry identity request should succeed");
        assert!(identities.is_empty());

        timeout(Duration::from_secs(2), server)
            .await
            .expect("fake Agent server should finish before timeout")
            .expect("fake Agent server task should not panic");
        let _ = tokio::fs::remove_file(socket_path).await;
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_environment_variable;

    #[test]
    fn normalizes_agent_environment_variable_prefix() {
        assert_eq!(
            normalize_environment_variable("$SSH_AUTH_SOCK").unwrap(),
            "SSH_AUTH_SOCK"
        );
        assert_eq!(
            normalize_environment_variable(" SSH_DEFAULT_SOCK ").unwrap(),
            "SSH_DEFAULT_SOCK"
        );
    }

    #[test]
    fn rejects_empty_agent_environment_variable() {
        assert!(normalize_environment_variable("$").is_err());
        assert!(normalize_environment_variable(" ").is_err());
    }
}
