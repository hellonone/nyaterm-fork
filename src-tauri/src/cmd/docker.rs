use crate::core::SessionManager;
use crate::core::monitoring::docker::{
    DockerComposeProject, DockerComposeService, DockerContainerDetails, DockerContainerStats,
    DockerImage, DockerNetwork, DockerVolume, RemoteDockerOverview, docker_compose_projects_script,
    docker_container_details_script, docker_images_script, docker_networks_script,
    docker_overview_script, docker_volumes_script, parse_compose_projects,
    parse_compose_services_output, parse_docker_container_details_output,
    parse_docker_images_output, parse_docker_networks_output, parse_docker_overview_output,
    parse_docker_stats_output, parse_docker_volumes_output,
};
use crate::core::remote_exec::{
    RemoteCommandOutput, exec_ssh_session_command, exec_ssh_session_command_with_stdin, sh_quote,
};
use crate::core::ssh::{SshAuth, SshConfig};
use crate::error::{AppError, AppResult};
use crate::observability::{self, StructuredLog, StructuredLogLevel};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tauri::Emitter;
use tokio::sync::{Mutex, oneshot};

const DOCKER_TIMEOUT: Duration = Duration::from_secs(20);
const DOCKER_LOG_TIMEOUT: Duration = Duration::from_secs(30);
const COMPOSE_PS_JSON_BEGIN: &str = "COMPOSE_PS_JSON_BEGIN";
const COMPOSE_PS_JSON_END: &str = "COMPOSE_PS_JSON_END";
const DOCKER_PLAIN: &str = "PATH=/usr/local/bin:/var/packages/ContainerManager/target/usr/bin:/var/packages/Docker/target/usr/bin:$PATH docker";
const DOCKER_SUDO_NON_INTERACTIVE: &str = "sudo -n env PATH=/usr/local/bin:/var/packages/ContainerManager/target/usr/bin:/var/packages/Docker/target/usr/bin:$PATH docker";
const DOCKER_SUDO_STDIN_TERMINAL: &str = "sudo -S -p \"\" env PATH=/usr/local/bin:/var/packages/ContainerManager/target/usr/bin:/var/packages/Docker/target/usr/bin:$PATH docker";

pub struct DockerSudoManager {
    pending: Mutex<HashMap<String, oneshot::Sender<Option<String>>>>,
    cached_passwords: Mutex<HashMap<String, String>>,
}

impl DockerSudoManager {
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            cached_passwords: Mutex::new(HashMap::new()),
        }
    }

    async fn register(&self, request_id: String) -> oneshot::Receiver<Option<String>> {
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(request_id, tx);
        rx
    }

    pub async fn respond(&self, request_id: &str, password: Option<String>) -> bool {
        if let Some(tx) = self.pending.lock().await.remove(request_id) {
            let _ = tx.send(password);
            true
        } else {
            false
        }
    }

    async fn cached_password(&self, session_id: &str) -> Option<String> {
        self.cached_passwords.lock().await.get(session_id).cloned()
    }

    async fn cache_password(&self, session_id: &str, password: String) {
        self.cached_passwords
            .lock()
            .await
            .insert(session_id.to_string(), password);
    }

    async fn invalidate_password(&self, session_id: &str) {
        self.cached_passwords.lock().await.remove(session_id);
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerSudoPasswordRequest {
    request_id: String,
    session_id: String,
    session_name: String,
    target_window_label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedDockerTerminalCommand {
    command: String,
    stdin: Option<String>,
}

#[derive(Debug)]
struct DockerRunResult {
    output: RemoteCommandOutput,
    mode: DockerCommandMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DockerCommandMode {
    Plain,
    SudoNoPassword,
    SudoPassword { password: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DockerOutputClassification {
    Success,
    DockerSocketPermissionDenied,
    SudoPasswordRequired,
    SudoAuthFailed,
    OtherFailure,
}

impl DockerOutputClassification {
    fn as_str(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::DockerSocketPermissionDenied => "docker_socket_permission_denied",
            Self::SudoPasswordRequired => "sudo_password_required",
            Self::SudoAuthFailed => "sudo_auth_failed",
            Self::OtherFailure => "other_failure",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DockerAttemptStage {
    Plain,
    SudoNoPassword,
    CachedPassword,
    SshSessionPassword,
    PromptPassword,
}

impl DockerAttemptStage {
    fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::SudoNoPassword => "sudo_n",
            Self::CachedPassword => "cached_password",
            Self::SshSessionPassword => "ssh_session_password",
            Self::PromptPassword => "prompt_password",
        }
    }
}

#[tauri::command]
pub async fn get_remote_docker_overview(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
) -> AppResult<RemoteDockerOverview> {
    let output = run_docker_command(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        docker_overview_script,
        "Docker overview failed",
        DOCKER_TIMEOUT,
    )
    .await?
    .output;

    let overview = parse_docker_overview_output(&output.stdout);
    if !overview.available {
        log_docker_overview_unavailable(&session_id, &output);
    }

    Ok(overview)
}

#[tauri::command]
pub async fn get_remote_docker_images(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
) -> AppResult<Vec<DockerImage>> {
    let output = run_docker_command(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        docker_images_script,
        "Docker images failed",
        DOCKER_TIMEOUT,
    )
    .await?
    .output;

    Ok(parse_docker_images_output(&output.stdout))
}

#[tauri::command]
pub async fn get_remote_docker_volumes(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
) -> AppResult<Vec<DockerVolume>> {
    let output = run_docker_command(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        docker_volumes_script,
        "Docker volumes failed",
        DOCKER_TIMEOUT,
    )
    .await?
    .output;

    Ok(parse_docker_volumes_output(&output.stdout))
}

#[tauri::command]
pub async fn get_remote_docker_networks(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
) -> AppResult<Vec<DockerNetwork>> {
    let output = run_docker_command(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        docker_networks_script,
        "Docker networks failed",
        DOCKER_TIMEOUT,
    )
    .await?
    .output;

    Ok(parse_docker_networks_output(&output.stdout))
}

#[tauri::command]
pub async fn get_remote_docker_compose_projects(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
) -> AppResult<Vec<DockerComposeProject>> {
    let output = run_docker_command(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        docker_compose_projects_script,
        "Docker compose projects failed",
        DOCKER_TIMEOUT,
    )
    .await?
    .output;

    Ok(parse_compose_projects(&output.stdout))
}

#[tauri::command]
pub async fn get_docker_container_details(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
    container_id: String,
) -> AppResult<DockerContainerDetails> {
    let output = run_docker_command(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        |docker| docker_container_details_script(docker, &container_id),
        "Docker container details failed",
        DOCKER_TIMEOUT,
    )
    .await?
    .output;

    Ok(parse_docker_container_details_output(&output.stdout))
}

#[tauri::command]
pub async fn get_docker_container_stats(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
    container_id: String,
) -> AppResult<Option<DockerContainerStats>> {
    let output = run_docker_command(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        |docker| docker_stats_command(docker, &container_id),
        "Docker container stats failed",
        DOCKER_TIMEOUT,
    )
    .await?
    .output;

    Ok(parse_docker_stats_output(&output.stdout).into_iter().next())
}

#[tauri::command]
pub async fn docker_container_action(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
    container_id: String,
    action: String,
) -> AppResult<RemoteCommandOutput> {
    let action = normalize_container_action(&action)?;
    run_docker_action(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        |docker| format!("{docker} {action} {}", sh_quote(&container_id)),
        "Docker container action failed",
    )
    .await
}

#[tauri::command]
pub async fn docker_image_remove(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
    image_id: String,
    force: bool,
) -> AppResult<RemoteCommandOutput> {
    let force_arg = if force { " -f" } else { "" };
    run_docker_action(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        |docker| format!("{docker} image rm{force_arg} {}", sh_quote(&image_id)),
        "Docker image remove failed",
    )
    .await
}

#[tauri::command]
pub async fn docker_volume_remove(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
    volume_name: String,
    force: bool,
) -> AppResult<RemoteCommandOutput> {
    let force_arg = if force { " -f" } else { "" };
    run_docker_action(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        |docker| format!("{docker} volume rm{force_arg} {}", sh_quote(&volume_name)),
        "Docker volume remove failed",
    )
    .await
}

#[tauri::command]
pub async fn docker_network_remove(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
    network_id: String,
) -> AppResult<RemoteCommandOutput> {
    run_docker_action(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        |docker| format!("{docker} network rm {}", sh_quote(&network_id)),
        "Docker network remove failed",
    )
    .await
}

#[tauri::command]
pub async fn docker_system_prune(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
    volumes: bool,
) -> AppResult<RemoteCommandOutput> {
    let volumes_arg = if volumes { " --volumes" } else { "" };
    run_docker_action(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        |docker| format!("{docker} system prune -f{volumes_arg}"),
        "Docker system prune failed",
    )
    .await
}

#[tauri::command]
pub async fn get_docker_container_logs(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
    container_id: String,
    tail: u32,
) -> AppResult<RemoteCommandOutput> {
    let tail = tail.clamp(10, 2000);
    let output = run_docker_command(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        |docker| format!("{docker} logs --tail {tail} {}", sh_quote(&container_id)),
        "Docker logs failed",
        DOCKER_LOG_TIMEOUT,
    )
    .await?;
    Ok(output.output)
}

#[tauri::command]
pub async fn docker_compose_action(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
    project_name: String,
    config_files: Option<String>,
    action: String,
) -> AppResult<RemoteCommandOutput> {
    let action = normalize_compose_action(&action)?;

    run_docker_action(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        |docker| {
            build_compose_action_command(docker, &project_name, config_files.as_deref(), action)
        },
        "Docker compose action failed",
    )
    .await
}

#[tauri::command]
pub async fn get_docker_compose_services(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
    project_name: String,
    config_files: Option<String>,
) -> AppResult<Vec<DockerComposeService>> {
    let output = run_docker_command(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        |docker| build_compose_services_command(docker, &project_name, config_files.as_deref()),
        "Docker compose services failed",
        DOCKER_TIMEOUT,
    )
    .await?
    .output;
    let (services_raw, ps_json_raw) = split_compose_services_output(&output.stdout);

    Ok(parse_compose_services_output(&services_raw, &ps_json_raw))
}

#[tauri::command]
pub async fn docker_compose_service_action(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
    project_name: String,
    config_files: Option<String>,
    service_name: String,
    action: String,
) -> AppResult<RemoteCommandOutput> {
    let action = normalize_compose_service_action(&action)?;
    run_docker_action(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        |docker| {
            build_compose_service_action_command(
                docker,
                &project_name,
                config_files.as_deref(),
                &service_name,
                action,
            )
        },
        "Docker compose service action failed",
    )
    .await
}

#[tauri::command]
pub async fn submit_docker_sudo_password(
    state: tauri::State<'_, Arc<DockerSudoManager>>,
    request_id: String,
    password: String,
) -> AppResult<()> {
    if state.respond(&request_id, Some(password)).await {
        Ok(())
    } else {
        Err(AppError::Auth(format!(
            "No pending Docker sudo request with id '{request_id}'"
        )))
    }
}

#[tauri::command]
pub async fn cancel_docker_sudo_password(
    state: tauri::State<'_, Arc<DockerSudoManager>>,
    request_id: String,
) -> AppResult<()> {
    let _ = state.respond(&request_id, None).await;
    Ok(())
}

#[tauri::command]
pub async fn prepare_docker_container_logs_command(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
    container_id: String,
    tail: u32,
) -> AppResult<PreparedDockerTerminalCommand> {
    let mode = probe_container_access(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        &container_id,
    )
    .await?;
    let tail = tail.clamp(10, 2000);
    Ok(prepare_terminal_command(mode, |docker| {
        format!("{docker} logs -f --tail {tail} {}", sh_quote(&container_id))
    }))
}

#[tauri::command]
pub async fn prepare_docker_container_shell_command(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
    container_id: String,
) -> AppResult<PreparedDockerTerminalCommand> {
    let mode = probe_container_access(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        &container_id,
    )
    .await?;
    Ok(prepare_terminal_command(mode, |docker| {
        format!(
            "{docker} exec -it {} sh -lc {}",
            sh_quote(&container_id),
            sh_quote(
                "if command -v bash >/dev/null 2>&1; then exec bash; elif command -v zsh >/dev/null 2>&1; then exec zsh; elif command -v fish >/dev/null 2>&1; then exec fish; elif command -v ash >/dev/null 2>&1; then exec ash; else exec sh; fi"
            )
        )
    }))
}

#[tauri::command]
pub async fn prepare_docker_compose_service_logs_command(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<SessionManager>>,
    sudo_state: tauri::State<'_, Arc<DockerSudoManager>>,
    session_id: String,
    project_name: String,
    config_files: Option<String>,
    service_name: String,
    tail: u32,
) -> AppResult<PreparedDockerTerminalCommand> {
    let mode = run_docker_command(
        &app,
        state.inner(),
        sudo_state.inner(),
        &session_id,
        |docker| {
            let base = build_compose_base_command(docker, &project_name, config_files.as_deref());
            format!("{base} ps --all --format json {}", sh_quote(&service_name))
        },
        "Docker compose service probe failed",
        DOCKER_TIMEOUT,
    )
    .await?
    .mode;
    let tail = tail.clamp(10, 2000);
    Ok(prepare_terminal_command(mode, |docker| {
        let base = build_compose_base_command(docker, &project_name, config_files.as_deref());
        format!("{base} logs -f --tail {tail} {}", sh_quote(&service_name))
    }))
}

async fn run_docker_action(
    app: &tauri::AppHandle,
    manager: &Arc<SessionManager>,
    sudo_manager: &Arc<DockerSudoManager>,
    session_id: &str,
    build_command: impl Fn(&str) -> String,
    context: &str,
) -> AppResult<RemoteCommandOutput> {
    Ok(run_docker_command(
        app,
        manager,
        sudo_manager,
        session_id,
        build_command,
        context,
        DOCKER_TIMEOUT,
    )
    .await?
    .output)
}

async fn probe_container_access(
    app: &tauri::AppHandle,
    manager: &Arc<SessionManager>,
    sudo_manager: &Arc<DockerSudoManager>,
    session_id: &str,
    container_id: &str,
) -> AppResult<DockerCommandMode> {
    Ok(run_docker_command(
        app,
        manager,
        sudo_manager,
        session_id,
        |docker| format!("{docker} inspect {}", sh_quote(container_id)),
        "Docker container probe failed",
        DOCKER_TIMEOUT,
    )
    .await?
    .mode)
}

fn prepare_terminal_command(
    mode: DockerCommandMode,
    build_command: impl Fn(&str) -> String,
) -> PreparedDockerTerminalCommand {
    match mode {
        DockerCommandMode::Plain => PreparedDockerTerminalCommand {
            command: build_command(DOCKER_PLAIN),
            stdin: None,
        },
        DockerCommandMode::SudoNoPassword => PreparedDockerTerminalCommand {
            command: build_command(DOCKER_SUDO_NON_INTERACTIVE),
            stdin: None,
        },
        DockerCommandMode::SudoPassword { password } => PreparedDockerTerminalCommand {
            command: build_command(DOCKER_SUDO_STDIN_TERMINAL),
            stdin: Some(format!("{password}\n")),
        },
    }
}

async fn run_docker_command(
    app: &tauri::AppHandle,
    manager: &Arc<SessionManager>,
    sudo_manager: &Arc<DockerSudoManager>,
    session_id: &str,
    build_command: impl Fn(&str) -> String,
    context: &str,
    timeout: Duration,
) -> AppResult<DockerRunResult> {
    let plain = exec_docker_attempt(
        manager,
        session_id,
        &build_command(DOCKER_PLAIN),
        None,
        timeout,
    )
    .await?;
    let plain_classification = classify_docker_output(&plain);
    log_docker_attempt_result(
        attempt_log_level(plain_classification),
        "docker.attempt.result",
        "Docker command attempt completed",
        session_id,
        context,
        DockerAttemptStage::Plain,
        plain_classification,
        &plain,
    );

    match plain_classification {
        DockerOutputClassification::Success => {
            return Ok(DockerRunResult {
                output: plain,
                mode: DockerCommandMode::Plain,
            });
        }
        DockerOutputClassification::DockerSocketPermissionDenied => {}
        DockerOutputClassification::SudoPasswordRequired
        | DockerOutputClassification::SudoAuthFailed
        | DockerOutputClassification::OtherFailure => {
            return Err(docker_output_error(context, &plain));
        }
    }

    log_docker_fallback(
        StructuredLogLevel::Info,
        "docker.fallback.triggered",
        "Docker command requires sudo fallback",
        session_id,
        context,
        Some("socket_permission_denied"),
        None,
    );

    let sudo_no_password = exec_docker_attempt(
        manager,
        session_id,
        &build_command(DOCKER_SUDO_NON_INTERACTIVE),
        None,
        timeout,
    )
    .await?;
    let sudo_no_password_classification = classify_docker_output(&sudo_no_password);
    log_docker_attempt_result(
        attempt_log_level(sudo_no_password_classification),
        "docker.attempt.result",
        "Docker command attempt completed",
        session_id,
        context,
        DockerAttemptStage::SudoNoPassword,
        sudo_no_password_classification,
        &sudo_no_password,
    );

    match sudo_no_password_classification {
        DockerOutputClassification::Success => {
            return Ok(DockerRunResult {
                output: sudo_no_password,
                mode: DockerCommandMode::SudoNoPassword,
            });
        }
        DockerOutputClassification::SudoPasswordRequired
        | DockerOutputClassification::SudoAuthFailed => {}
        DockerOutputClassification::DockerSocketPermissionDenied
        | DockerOutputClassification::OtherFailure => {
            return Err(docker_output_error(context, &sudo_no_password));
        }
    }

    if let Some(password) = sudo_manager.cached_password(session_id).await {
        if let Some(result) = try_docker_password_attempt(
            manager,
            sudo_manager,
            session_id,
            &build_command(DOCKER_SUDO_STDIN_TERMINAL),
            password,
            DockerAttemptStage::CachedPassword,
            context,
            timeout,
        )
        .await?
        {
            return Ok(result);
        }
    }

    if let Some(password) = session_auth_password(manager, session_id).await? {
        if let Some(result) = try_docker_password_attempt(
            manager,
            sudo_manager,
            session_id,
            &build_command(DOCKER_SUDO_STDIN_TERMINAL),
            password,
            DockerAttemptStage::SshSessionPassword,
            context,
            timeout,
        )
        .await?
        {
            return Ok(result);
        }
    } else {
        log_docker_fallback(
            StructuredLogLevel::Debug,
            "docker.fallback.session_password_unavailable",
            "No SSH session password is available for Docker sudo fallback",
            session_id,
            context,
            Some("socket_permission_denied"),
            None,
        );
    }

    for _ in 0..2 {
        log_docker_fallback(
            StructuredLogLevel::Info,
            "docker.fallback.password_prompt",
            "Requesting Docker sudo password from frontend",
            session_id,
            context,
            Some("socket_permission_denied"),
            None,
        );
        let password = request_docker_sudo_password(app, manager, sudo_manager, session_id).await?;
        if let Some(result) = try_docker_password_attempt(
            manager,
            sudo_manager,
            session_id,
            &build_command(DOCKER_SUDO_STDIN_TERMINAL),
            password,
            DockerAttemptStage::PromptPassword,
            context,
            timeout,
        )
        .await?
        {
            return Ok(result);
        }
    }

    Err(AppError::Auth(
        "Docker sudo authentication failed".to_string(),
    ))
}

async fn session_auth_password(
    manager: &Arc<SessionManager>,
    session_id: &str,
) -> AppResult<Option<String>> {
    let sessions = manager.sessions.lock().await;
    let session = sessions
        .get(session_id)
        .ok_or_else(|| AppError::SessionNotFound(format!("Session '{session_id}' not found")))?;

    let password = session
        .ssh_config
        .as_ref()
        .and_then(|config| config.downcast_ref::<SshConfig>())
        .and_then(|config| ssh_auth_password(&config.auth));

    Ok(password)
}

fn ssh_auth_password(auth: &SshAuth) -> Option<String> {
    match auth {
        SshAuth::Password {
            password: Some(password),
        } if !password.is_empty() => Some(password.clone()),
        SshAuth::None | SshAuth::Password { .. } | SshAuth::Key { .. } => None,
        SshAuth::Agent => None,
    }
}

async fn exec_docker_attempt(
    manager: &Arc<SessionManager>,
    session_id: &str,
    command: &str,
    stdin: Option<&str>,
    timeout: Duration,
) -> AppResult<RemoteCommandOutput> {
    match stdin {
        Some(stdin) => {
            exec_ssh_session_command_with_stdin(
                manager,
                session_id,
                command.as_bytes(),
                stdin.as_bytes(),
                timeout,
            )
            .await
        }
        None => exec_ssh_session_command(manager, session_id, command.as_bytes(), timeout).await,
    }
}

async fn try_docker_password_attempt(
    manager: &Arc<SessionManager>,
    sudo_manager: &Arc<DockerSudoManager>,
    session_id: &str,
    command: &str,
    password: String,
    stage: DockerAttemptStage,
    context: &str,
    timeout: Duration,
) -> AppResult<Option<DockerRunResult>> {
    let stdin = format!("{password}\n");
    let output = exec_docker_attempt(manager, session_id, command, Some(&stdin), timeout).await?;
    let classification = classify_docker_output(&output);
    log_docker_attempt_result(
        attempt_log_level(classification),
        "docker.attempt.result",
        "Docker command attempt completed",
        session_id,
        context,
        stage,
        classification,
        &output,
    );

    match classification {
        DockerOutputClassification::Success => {
            sudo_manager
                .cache_password(session_id, password.clone())
                .await;
            Ok(Some(DockerRunResult {
                output,
                mode: DockerCommandMode::SudoPassword { password },
            }))
        }
        DockerOutputClassification::SudoPasswordRequired
        | DockerOutputClassification::SudoAuthFailed => {
            sudo_manager.invalidate_password(session_id).await;
            Ok(None)
        }
        DockerOutputClassification::DockerSocketPermissionDenied
        | DockerOutputClassification::OtherFailure => Err(docker_output_error(context, &output)),
    }
}

async fn request_docker_sudo_password(
    app: &tauri::AppHandle,
    manager: &Arc<SessionManager>,
    sudo_manager: &Arc<DockerSudoManager>,
    session_id: &str,
) -> AppResult<String> {
    let (session_name, target_window_label) =
        docker_session_prompt_context(manager, session_id).await?;
    let request_id = uuid::Uuid::new_v4().to_string();
    let rx = sudo_manager.register(request_id.clone()).await;
    let payload = DockerSudoPasswordRequest {
        request_id: request_id.clone(),
        session_id: session_id.to_string(),
        session_name,
        target_window_label,
    };
    let _ = app.emit("docker-sudo-password-request", &payload);

    match rx.await {
        Ok(Some(password)) if !password.is_empty() => Ok(password),
        Ok(_) => Err(AppError::Auth(
            "Docker sudo password request cancelled".to_string(),
        )),
        Err(_) => Err(AppError::Auth(
            "Docker sudo password request was interrupted".to_string(),
        )),
    }
}

async fn docker_session_prompt_context(
    manager: &Arc<SessionManager>,
    session_id: &str,
) -> AppResult<(String, Option<String>)> {
    let sessions = manager.sessions.lock().await;
    let session = sessions
        .get(session_id)
        .ok_or_else(|| AppError::SessionNotFound(format!("Session '{session_id}' not found")))?;
    Ok((
        session.info.name.clone(),
        session.info.owner_window_label.clone(),
    ))
}

fn build_compose_base_command(
    docker: &str,
    project_name: &str,
    config_files: Option<&str>,
) -> String {
    let mut command = format!("{docker} compose");

    if let Some(config_files) = config_files.filter(|value| !value.trim().is_empty()) {
        for file in config_files
            .split(',')
            .map(str::trim)
            .filter(|file| !file.is_empty())
        {
            command.push_str(" -f ");
            command.push_str(&sh_quote(file));
        }
    }

    command.push_str(" -p ");
    command.push_str(&sh_quote(project_name));
    command
}

fn docker_stats_command(docker: &str, container_id: &str) -> String {
    format!(
        "{docker} stats --no-stream --no-trunc --format \"CONTAINER_STATS\\t{{{{.ID}}}}\\t{{{{.CPUPerc}}}}\\t{{{{.MemUsage}}}}\\t{{{{.MemPerc}}}}\\t{{{{.NetIO}}}}\\t{{{{.BlockIO}}}}\\t{{{{.PIDs}}}}\" {}",
        sh_quote(container_id)
    )
}

fn build_compose_action_command(
    docker: &str,
    project_name: &str,
    config_files: Option<&str>,
    action: &str,
) -> String {
    let mut command = build_compose_base_command(docker, project_name, config_files);
    command.push(' ');
    command.push_str(action);
    if action == "up" {
        command.push_str(" -d");
    }
    command
}

fn build_compose_services_command(
    docker: &str,
    project_name: &str,
    config_files: Option<&str>,
) -> String {
    let base = build_compose_base_command(docker, project_name, config_files);
    format!(
        "services_output=$({base} config --services) || exit $?; \
         printf '%s\\n' \"$services_output\"; \
         printf '\\n{COMPOSE_PS_JSON_BEGIN}\\n'; \
         {base} ps --all --format json || true; \
         printf '\\n{COMPOSE_PS_JSON_END}\\n'"
    )
}

fn build_compose_service_action_command(
    docker: &str,
    project_name: &str,
    config_files: Option<&str>,
    service_name: &str,
    action: &str,
) -> String {
    let mut command = build_compose_action_command(docker, project_name, config_files, action);
    command.push(' ');
    command.push_str(&sh_quote(service_name));
    command
}

fn split_compose_services_output(output: &str) -> (String, String) {
    let mut services = String::new();
    let mut ps_json = String::new();
    let mut in_ps_json = false;

    for line in output.lines() {
        if line == COMPOSE_PS_JSON_BEGIN {
            in_ps_json = true;
            continue;
        }
        if line == COMPOSE_PS_JSON_END {
            in_ps_json = false;
            continue;
        }
        if in_ps_json {
            ps_json.push_str(line);
            ps_json.push('\n');
        } else {
            services.push_str(line);
            services.push('\n');
        }
    }

    (services, ps_json)
}

fn command_succeeded(output: &RemoteCommandOutput) -> bool {
    matches!(output.exit_status, Some(0) | None)
}

fn classify_docker_output(output: &RemoteCommandOutput) -> DockerOutputClassification {
    if is_sudo_password_required(output) {
        return DockerOutputClassification::SudoPasswordRequired;
    }
    if is_sudo_auth_failure(output) {
        return DockerOutputClassification::SudoAuthFailed;
    }
    if is_docker_socket_permission_error(output) {
        return DockerOutputClassification::DockerSocketPermissionDenied;
    }
    if command_succeeded(output) {
        return DockerOutputClassification::Success;
    }
    DockerOutputClassification::OtherFailure
}

fn combined_output_lower(output: &RemoteCommandOutput) -> String {
    normalize_match_text(&format!("{}\n{}", output.stdout, output.stderr))
}

fn normalize_match_text(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut in_escape = false;
    for ch in value.chars() {
        if in_escape {
            if ch.is_ascii_alphabetic() || ch == '~' {
                in_escape = false;
            }
            continue;
        }
        if ch == '\u{1b}' {
            in_escape = true;
            normalized.push(' ');
            continue;
        }
        if ch.is_control() {
            normalized.push(' ');
        } else {
            normalized.extend(ch.to_lowercase());
        }
    }
    normalized
}

fn is_docker_socket_permission_error(output: &RemoteCommandOutput) -> bool {
    let text = combined_output_lower(output);
    text.contains("permission denied")
        && (text.contains("docker daemon socket")
            || text.contains("docker api at unix")
            || text.contains("/var/run/docker.sock")
            || text.contains("docker.sock")
            || text.contains("connect to the docker daemon"))
}

fn docker_fallback_reason(output: &RemoteCommandOutput) -> Option<&'static str> {
    if is_docker_socket_permission_error(output) {
        Some("socket_permission_denied")
    } else {
        None
    }
}

fn is_sudo_password_required(output: &RemoteCommandOutput) -> bool {
    let text = combined_output_lower(output);
    text.contains("a password is required")
        || text.contains("sudo: a terminal is required")
        || text.contains("sudo: a password is required")
        || text.contains("no tty present")
        || (text.contains("sudo") && text.contains("password") && text.contains("required"))
        || (text.contains("sudo") && text.contains("password") && text.contains("terminal"))
        || (text.contains("sudo") && text.contains("password") && text.contains("tty"))
}

fn is_sudo_auth_failure(output: &RemoteCommandOutput) -> bool {
    let text = combined_output_lower(output);
    text.contains("sorry, try again")
        || text.contains("incorrect password")
        || text.contains("authentication failure")
        || text.contains("incorrect password attempt")
        || text.contains("incorrect password attempts")
}

fn docker_output_error(context: &str, output: &RemoteCommandOutput) -> AppError {
    let stderr = output.stderr.trim();
    let stdout = output.stdout.trim();
    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "remote command failed"
    };

    AppError::Channel(format!("{context}: {detail}"))
}

fn attempt_log_level(classification: DockerOutputClassification) -> StructuredLogLevel {
    match classification {
        DockerOutputClassification::Success => StructuredLogLevel::Debug,
        DockerOutputClassification::DockerSocketPermissionDenied
        | DockerOutputClassification::SudoPasswordRequired => StructuredLogLevel::Info,
        DockerOutputClassification::SudoAuthFailed | DockerOutputClassification::OtherFailure => {
            StructuredLogLevel::Warn
        }
    }
}

fn log_docker_fallback(
    level: StructuredLogLevel,
    event: &str,
    message: &str,
    session_id: &str,
    context: &str,
    reason: Option<&str>,
    exit_status: Option<u32>,
) {
    observability::log_event(StructuredLog {
        level,
        domain: "docker.manager".to_string(),
        event: event.to_string(),
        message: message.to_string(),
        ids: Some(json!({ "session_id": session_id })),
        data: Some(json!({
            "context": context,
            "reason": reason,
            "exit_status": exit_status,
        })),
        error: None,
        client_timestamp: None,
    });
}

fn log_docker_attempt_result(
    level: StructuredLogLevel,
    event: &str,
    message: &str,
    session_id: &str,
    context: &str,
    stage: DockerAttemptStage,
    classification: DockerOutputClassification,
    output: &RemoteCommandOutput,
) {
    observability::log_event(StructuredLog {
        level,
        domain: "docker.manager".to_string(),
        event: event.to_string(),
        message: message.to_string(),
        ids: Some(json!({ "session_id": session_id })),
        data: Some(json!({
            "context": context,
            "stage": stage.as_str(),
            "classification": classification.as_str(),
            "exit_status": output.exit_status,
            "stdout": truncate_log_text(&output.stdout, 240),
            "stderr": truncate_log_text(&output.stderr, 500),
        })),
        error: None,
        client_timestamp: None,
    });
}

fn log_docker_overview_unavailable(session_id: &str, output: &RemoteCommandOutput) {
    observability::log_event(StructuredLog {
        level: StructuredLogLevel::Info,
        domain: "docker.manager".to_string(),
        event: "docker.overview.unavailable".to_string(),
        message: "Docker overview reported unavailable".to_string(),
        ids: Some(json!({ "session_id": session_id })),
        data: Some(json!({
            "exit_status": output.exit_status,
            "stdout": truncate_log_text(&output.stdout, 240),
            "stderr": truncate_log_text(&output.stderr, 500),
            "fallback_reason": docker_fallback_reason(output),
        })),
        error: None,
        client_timestamp: None,
    });
}

fn truncate_log_text(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    let mut result = String::new();
    for ch in trimmed.chars().take(max_chars) {
        result.push(ch);
    }
    if trimmed.chars().count() > max_chars {
        result.push_str("...");
    }
    result
}

fn normalize_container_action(action: &str) -> AppResult<&'static str> {
    match action.trim().to_ascii_lowercase().as_str() {
        "start" => Ok("start"),
        "stop" => Ok("stop"),
        "restart" => Ok("restart"),
        "kill" => Ok("kill"),
        "remove" | "rm" => Ok("rm"),
        _ => Err(AppError::Config(
            "Unsupported Docker container action".to_string(),
        )),
    }
}

fn normalize_compose_action(action: &str) -> AppResult<&'static str> {
    match action.trim().to_ascii_lowercase().as_str() {
        "up" => Ok("up"),
        "down" => Ok("down"),
        "restart" => Ok("restart"),
        _ => Err(AppError::Config(
            "Unsupported Docker compose action".to_string(),
        )),
    }
}

fn normalize_compose_service_action(action: &str) -> AppResult<&'static str> {
    match action.trim().to_ascii_lowercase().as_str() {
        "up" => Ok("up"),
        "stop" => Ok("stop"),
        "restart" => Ok("restart"),
        _ => Err(AppError::Config(
            "Unsupported Docker compose service action".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(stdout: &str, stderr: &str, exit_status: Option<u32>) -> RemoteCommandOutput {
        RemoteCommandOutput {
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            exit_status,
        }
    }

    #[test]
    fn detects_docker_socket_permission_errors() {
        let denied = output(
            "",
            "permission denied while trying to connect to the Docker daemon socket at unix:///var/run/docker.sock",
            Some(1),
        );
        assert!(is_docker_socket_permission_error(&denied));
        assert_eq!(
            docker_fallback_reason(&denied),
            Some("socket_permission_denied")
        );

        let ordinary_error = output("", "Error: No such image: missing", Some(1));
        assert!(!is_docker_socket_permission_error(&ordinary_error));
        assert_eq!(docker_fallback_reason(&ordinary_error), None);
    }

    #[test]
    fn detects_socket_permission_even_without_exit_status() {
        let denied = output(
            "DOCKER_AVAILABLE\t0\n",
            "permission denied while trying to connect to the docker API at unix:///var/run/docker.sock",
            None,
        );

        assert_eq!(
            docker_fallback_reason(&denied),
            Some("socket_permission_denied")
        );
    }

    #[test]
    fn does_not_sudo_fallback_for_missing_docker_binary() {
        let missing = output(
            "DOCKER_AVAILABLE\t0\n",
            "sh: 1: docker: not found",
            Some(127),
        );

        assert_eq!(docker_fallback_reason(&missing), None);
    }

    #[test]
    fn detects_sudo_password_and_auth_failures() {
        let needs_password = output("", "sudo: a password is required", Some(1));
        assert!(is_sudo_password_required(&needs_password));
        assert!(!is_sudo_auth_failure(&needs_password));
        assert_eq!(
            classify_docker_output(&needs_password),
            DockerOutputClassification::SudoPasswordRequired
        );

        let ansi_password_required = output(
            "",
            "\u{1b}[?2004hsudo: a password is required\u{1b}[?2004l",
            Some(1),
        );
        assert!(is_sudo_password_required(&ansi_password_required));

        let split_password_required = output("", "sudo: password\nis required", Some(1));
        assert!(is_sudo_password_required(&split_password_required));

        let bad_password = output(
            "",
            "Sorry, try again.\nsudo: 1 incorrect password attempt",
            Some(1),
        );
        assert!(is_sudo_auth_failure(&bad_password));
        assert_eq!(
            classify_docker_output(&bad_password),
            DockerOutputClassification::SudoAuthFailed
        );
    }

    #[test]
    fn classifies_non_permission_docker_errors_without_sudo_fallback() {
        let missing_image = output(
            "",
            "Error response from daemon: No such image: nope",
            Some(1),
        );
        assert_eq!(
            classify_docker_output(&missing_image),
            DockerOutputClassification::OtherFailure
        );

        let daemon_down = output("", "Cannot connect to the Docker daemon", Some(1));
        assert_eq!(
            classify_docker_output(&daemon_down),
            DockerOutputClassification::OtherFailure
        );
    }

    #[test]
    fn builds_expected_fallback_attempt_order() {
        let build = |docker: &str| format!("{docker} ps -a");
        let attempts = [
            build(DOCKER_PLAIN),
            build(DOCKER_SUDO_NON_INTERACTIVE),
            build(DOCKER_SUDO_STDIN_TERMINAL),
        ];
        assert_eq!(
            attempts,
            [
                format!("{DOCKER_PLAIN} ps -a"),
                format!("{DOCKER_SUDO_NON_INTERACTIVE} ps -a"),
                format!("{DOCKER_SUDO_STDIN_TERMINAL} ps -a"),
            ]
        );
    }

    #[test]
    fn sudo_password_prefix_is_safe_inside_sh_c_scripts() {
        let command = docker_overview_script(DOCKER_SUDO_STDIN_TERMINAL);

        assert!(command.contains(&format!("{DOCKER_SUDO_STDIN_TERMINAL} info")));
        assert!(!command.contains("sudo -S -p '' docker"));
    }

    #[test]
    fn compose_command_builders_quote_arguments() {
        let command = build_compose_service_action_command(
            "sudo -n docker",
            "demo prod",
            Some("/srv/demo/docker-compose.yml,/srv/demo/it's.yml"),
            "web api",
            "restart",
        );
        assert_eq!(
            command,
            "sudo -n docker compose -f '/srv/demo/docker-compose.yml' -f '/srv/demo/it'\"'\"'s.yml' -p 'demo prod' restart 'web api'"
        );
    }

    #[test]
    fn prepared_terminal_command_keeps_password_out_of_command() {
        let prepared = prepare_terminal_command(
            DockerCommandMode::SudoPassword {
                password: "secret".to_string(),
            },
            |docker| format!("{docker} logs -f 'abc'"),
        );
        assert_eq!(
            prepared.command,
            format!("{DOCKER_SUDO_STDIN_TERMINAL} logs -f 'abc'")
        );
        assert_eq!(prepared.stdin.as_deref(), Some("secret\n"));
        assert!(!prepared.command.contains("secret"));
    }

    #[test]
    fn extracts_ssh_session_password_for_sudo_fallback() {
        assert_eq!(
            ssh_auth_password(&SshAuth::Password {
                password: Some("login-secret".to_string()),
            })
            .as_deref(),
            Some("login-secret")
        );
        assert_eq!(
            ssh_auth_password(&SshAuth::Password {
                password: Some(String::new()),
            }),
            None
        );
        assert_eq!(ssh_auth_password(&SshAuth::None), None);
        assert_eq!(
            ssh_auth_password(&SshAuth::Key {
                key_id: None,
                key_data: "key".to_string(),
                cert_data: None,
                passphrase: Some("passphrase".to_string()),
            }),
            None
        );
    }

    #[test]
    fn validates_container_actions() {
        assert_eq!(normalize_container_action("remove").unwrap(), "rm");
        assert!(normalize_container_action("exec").is_err());
    }

    #[test]
    fn validates_compose_actions() {
        assert_eq!(normalize_compose_action("up").unwrap(), "up");
        assert!(normalize_compose_action("pull").is_err());
    }

    #[test]
    fn validates_compose_service_actions() {
        assert_eq!(normalize_compose_service_action("up").unwrap(), "up");
        assert_eq!(
            normalize_compose_service_action("restart").unwrap(),
            "restart"
        );
        assert!(normalize_compose_service_action("down").is_err());
    }
}
