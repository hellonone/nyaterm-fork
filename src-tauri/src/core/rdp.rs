use crate::config::{self, ConnectionAuth, ConnectionType};
use crate::error::{AppError, AppResult};
use async_trait::async_trait;
use ironrdp::client::config::{
    Config as IronRdpConfig, ConfigBuilder as IronRdpConfigBuilder,
    Destination as IronRdpDestination, TransportKind as IronRdpTransportKind,
};
use ironrdp::client::rdp::{
    RdpClient as IronRdpClient, RdpInputEvent as IronRdpInputEvent, RdpOutputEvent,
};
use ironrdp::input::{
    Database as IronRdpInputDatabase, MouseButton as IronRdpMouseButton,
    MousePosition as IronRdpMousePosition, Operation as IronRdpInputOperation,
    Scancode as IronRdpScancode, WheelRotations as IronRdpWheelRotations,
};
use ironrdp::pdu::rdp::capability_sets::MajorPlatformType;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, mpsc};
use tokio::time::{Duration, sleep};

const FRAME_HEADER_BYTES: usize = 44;
const PIXEL_FORMAT_BGRA8888: u32 = 1;
const PIXEL_FORMAT_RGBA8888: u32 = 2;
const MAX_FRAME_QUEUE: usize = 2;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum RdpSessionState {
    Connecting,
    CertificateVerification,
    Authenticating,
    Negotiating,
    Active,
    Reconnecting,
    Disconnected,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RdpStateEvent {
    pub session_id: String,
    pub state: RdpSessionState,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
#[allow(dead_code)]
pub enum RdpInputEvent {
    #[serde(rename = "key-down")]
    KeyDown {
        scan_code: u16,
        extended: bool,
        repeat: bool,
    },
    #[serde(rename = "key-up")]
    KeyUp {
        scan_code: u16,
        extended: bool,
        repeat: bool,
    },
    #[serde(rename = "mouse-move")]
    MouseMove { x: u32, y: u32 },
    #[serde(rename = "mouse-button")]
    MouseButton {
        button: String,
        pressed: bool,
        x: u32,
        y: u32,
    },
    #[serde(rename = "mouse-wheel")]
    MouseWheel {
        delta_x: f64,
        delta_y: f64,
        x: u32,
        y: u32,
    },
    #[serde(rename = "unicode")]
    Unicode { text: String },
    #[serde(rename = "release-all-keys")]
    ReleaseAllKeys,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RdpConnectConfig {
    pub session_id: String,
    pub connection_id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub domain: String,
    pub password: Option<String>,
    pub width: u32,
    pub height: u32,
    pub use_nla: bool,
    pub certificate_policy: String,
    pub color_depth: u8,
}

pub struct RdpSession {
    pub config: RdpConnectConfig,
    pub state: Mutex<RdpSessionState>,
    message: Mutex<Option<String>>,
    generation: Mutex<u64>,
    frame_channel: Mutex<Option<Channel<InvokeResponseBody>>>,
    pending_frames: Mutex<VecDeque<Vec<u8>>>,
    input_sender: Mutex<Option<mpsc::UnboundedSender<IronRdpInputEvent>>>,
    input_database: Mutex<IronRdpInputDatabase>,
    frame_sequence: Mutex<u64>,
}

pub struct RdpSessionManager {
    sessions: Mutex<HashMap<String, Arc<RdpSession>>>,
    engine: Arc<dyn RdpEngine>,
}

#[async_trait]
pub trait RdpEngine: Send + Sync {
    async fn connect(&self, app: AppHandle, session: Arc<RdpSession>, generation: u64);
    async fn send_input(
        &self,
        session: Arc<RdpSession>,
        events: Vec<RdpInputEvent>,
    ) -> AppResult<()>;
    async fn resize(&self, session: Arc<RdpSession>, width: u32, height: u32) -> AppResult<()>;
    async fn set_clipboard_text(&self, session: Arc<RdpSession>, text: String) -> AppResult<()>;
    async fn close(&self, session: Arc<RdpSession>) -> AppResult<()>;
}

pub struct IronRdpEngine;

impl IronRdpEngine {
    pub fn new() -> Self {
        Self
    }
}

impl RdpSessionManager {
    pub fn new() -> Self {
        Self::with_engine(Arc::new(IronRdpEngine::new()))
    }

    pub fn with_engine(engine: Arc<dyn RdpEngine>) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            engine,
        }
    }

    pub async fn create_session(
        &self,
        app: AppHandle,
        config: RdpConnectConfig,
    ) -> AppResult<String> {
        let session_id = config.session_id.clone();
        let session = Arc::new(RdpSession {
            config,
            state: Mutex::new(RdpSessionState::Connecting),
            message: Mutex::new(Some("Establishing RDP transport".to_string())),
            generation: Mutex::new(0),
            frame_channel: Mutex::new(None),
            pending_frames: Mutex::new(VecDeque::with_capacity(MAX_FRAME_QUEUE)),
            input_sender: Mutex::new(None),
            input_database: Mutex::new(IronRdpInputDatabase::new()),
            frame_sequence: Mutex::new(0),
        });
        self.sessions
            .lock()
            .await
            .insert(session_id.clone(), session.clone());

        emit_state(
            &app,
            &session_id,
            RdpSessionState::Connecting,
            Some("Establishing RDP transport".to_string()),
        );
        self.engine.connect(app, session, 0).await;
        Ok(session_id)
    }

    pub async fn attach_frame_channel(
        &self,
        app: &AppHandle,
        session_id: &str,
        channel: Channel<InvokeResponseBody>,
    ) -> AppResult<()> {
        let session = self.get(session_id).await?;
        {
            let mut current = session.frame_channel.lock().await;
            *current = Some(channel);
        }
        flush_pending_frames(&session).await;
        emit_current_state(app, &session).await;
        Ok(())
    }

    pub async fn send_input(&self, session_id: &str, events: Vec<RdpInputEvent>) -> AppResult<()> {
        let session = self.get(session_id).await?;
        if events.is_empty() {
            return Ok(());
        }
        self.engine.send_input(session, events).await
    }

    pub async fn resize(&self, session_id: &str, width: u32, height: u32) -> AppResult<()> {
        let session = self.get(session_id).await?;
        if !(640..=7680).contains(&width) || !(480..=4320).contains(&height) {
            return Err(AppError::Config(
                "RDP resize is outside the supported range".to_string(),
            ));
        }
        self.engine.resize(session, width, height).await
    }

    pub async fn set_clipboard_text(&self, session_id: &str, text: String) -> AppResult<()> {
        let session = self.get(session_id).await?;
        if text.len() > 16 * 1024 * 1024 {
            return Err(AppError::Config(
                "RDP clipboard text is too large".to_string(),
            ));
        }
        self.engine.set_clipboard_text(session, text).await
    }

    pub async fn reconnect(&self, app: AppHandle, session_id: &str) -> AppResult<()> {
        let session = self.get(session_id).await?;
        let generation = bump_generation(&session).await;
        set_state(
            &session,
            RdpSessionState::Reconnecting,
            Some("Reconnecting RDP session".to_string()),
        )
        .await;
        emit_state(
            &app,
            session_id,
            RdpSessionState::Reconnecting,
            Some("Reconnecting RDP session".to_string()),
        );
        self.engine.connect(app, session, generation).await;
        Ok(())
    }

    pub async fn close(&self, app: &AppHandle, session_id: &str) -> AppResult<()> {
        let removed = self.sessions.lock().await.remove(session_id);
        if let Some(session) = removed {
            bump_generation(&session).await;
            set_state(
                &session,
                RdpSessionState::Disconnected,
                Some("RDP session closed".to_string()),
            )
            .await;
            self.engine.close(session).await?;
            emit_state(
                app,
                session_id,
                RdpSessionState::Disconnected,
                Some("RDP session closed".to_string()),
            );
            let _ = app.emit("sessions-changed", ());
        }
        Ok(())
    }

    async fn get(&self, session_id: &str) -> AppResult<Arc<RdpSession>> {
        self.sessions
            .lock()
            .await
            .get(session_id)
            .cloned()
            .ok_or_else(|| {
                AppError::SessionNotFound(format!("RDP session '{session_id}' not found"))
            })
    }
}

impl Default for RdpSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RdpEngine for IronRdpEngine {
    async fn connect(&self, app: AppHandle, session: Arc<RdpSession>, generation: u64) {
        spawn_ironrdp_engine(app, session, generation).await;
    }

    async fn send_input(
        &self,
        session: Arc<RdpSession>,
        events: Vec<RdpInputEvent>,
    ) -> AppResult<()> {
        let input_events = {
            let mut database = session.input_database.lock().await;
            let mut output = Vec::new();
            for event in events {
                let fast_path = match rdp_input_to_operations(event) {
                    Some(operations) => database.apply(operations),
                    None => database.release_all(),
                };
                if !fast_path.is_empty() {
                    output.push(IronRdpInputEvent::FastPath(fast_path));
                }
            }
            output
        };

        let sender = session.input_sender.lock().await.clone().ok_or_else(|| {
            AppError::SessionNotFound("RDP session is not connected yet".to_string())
        })?;

        for event in input_events {
            sender
                .send(event)
                .map_err(|_| AppError::Channel("RDP input channel is closed".to_string()))?;
        }
        Ok(())
    }

    async fn resize(&self, session: Arc<RdpSession>, width: u32, height: u32) -> AppResult<()> {
        let sender = session.input_sender.lock().await.clone().ok_or_else(|| {
            AppError::SessionNotFound("RDP session is not connected yet".to_string())
        })?;
        sender
            .send(IronRdpInputEvent::Resize {
                width: u16::try_from(width).map_err(|_| {
                    AppError::Config("RDP width is outside the supported range".to_string())
                })?,
                height: u16::try_from(height).map_err(|_| {
                    AppError::Config("RDP height is outside the supported range".to_string())
                })?,
                scale_factor: 100,
                physical_size: None,
            })
            .map_err(|_| AppError::Channel("RDP input channel is closed".to_string()))?;
        Ok(())
    }

    async fn set_clipboard_text(&self, session: Arc<RdpSession>, text: String) -> AppResult<()> {
        self.send_input(session, vec![RdpInputEvent::Unicode { text }])
            .await
    }

    async fn close(&self, session: Arc<RdpSession>) -> AppResult<()> {
        close_current_input_sender(&session).await;
        Ok(())
    }
}

pub fn load_saved_rdp_config(app: &AppHandle, connection_id: &str) -> AppResult<RdpConnectConfig> {
    let conn = config::load_connection_by_id(app, connection_id)?;
    let password = resolve_rdp_password(app, conn.auth.as_ref())?;
    let ConnectionType::Rdp {
        host,
        port,
        username,
        domain,
        display,
        security,
        ..
    } = conn.config
    else {
        return Err(AppError::Config(
            "Connection is not an RDP connection".to_string(),
        ));
    };

    Ok(RdpConnectConfig {
        session_id: uuid::Uuid::new_v4().to_string(),
        connection_id: connection_id.to_string(),
        name: conn.name,
        host,
        port,
        username,
        domain,
        password,
        width: display.width,
        height: display.height,
        use_nla: security.use_nla,
        certificate_policy: security.certificate_policy,
        color_depth: display.color_depth,
    })
}

fn resolve_rdp_password(
    app: &AppHandle,
    auth: Option<&ConnectionAuth>,
) -> AppResult<Option<String>> {
    let Some(auth) = auth else {
        return Ok(None);
    };
    if auth.mode != "password" {
        return Ok(None);
    }
    if let Some(password_id) = auth.password_id.as_deref().filter(|id| !id.is_empty()) {
        return Ok(config::load_password_by_id(app, password_id)?.password);
    }
    crate::utils::crypto::decrypt_optional(&auth.password)
}

fn emit_state(app: &AppHandle, session_id: &str, state: RdpSessionState, message: Option<String>) {
    let payload = RdpStateEvent {
        session_id: session_id.to_string(),
        state,
        message,
    };
    let _ = app.emit(format!("rdp-state-{session_id}").as_str(), payload);
}

#[allow(dead_code)]
fn spawn_placeholder_engine(app: AppHandle, session: Arc<RdpSession>, generation: u64) {
    tauri::async_runtime::spawn(async move {
        let session_id = session.config.session_id.clone();
        set_state(
            &session,
            RdpSessionState::Negotiating,
            Some("Preparing RDP graphics pipeline".to_string()),
        )
        .await;
        emit_state(
            &app,
            &session_id,
            RdpSessionState::Negotiating,
            Some("Preparing RDP graphics pipeline".to_string()),
        );
        sleep(Duration::from_millis(150)).await;
        set_state(&session, RdpSessionState::Active, None).await;
        emit_state(&app, &session_id, RdpSessionState::Active, None);
        let _ = app.emit("sessions-changed", ());

        let mut sequence = 0_u64;
        loop {
            if *session.generation.lock().await != generation {
                break;
            }
            let state = session.state.lock().await.clone();
            if !matches!(
                state,
                RdpSessionState::Active | RdpSessionState::Reconnecting
            ) {
                break;
            }
            let frame = build_placeholder_frame(&session.config, sequence);
            sequence = sequence.wrapping_add(1);
            queue_or_send_frame(&session, frame).await;
            sleep(Duration::from_millis(1000)).await;
        }
    });
}

async fn spawn_ironrdp_engine(app: AppHandle, session: Arc<RdpSession>, generation: u64) {
    close_current_input_sender(&session).await;

    let iron_config = match build_ironrdp_config(&session.config) {
        Ok(config) => config,
        Err(message) => {
            fail_rdp_session(&app, &session, generation, message).await;
            return;
        }
    };

    tauri::async_runtime::spawn(async move {
        let session_id = session.config.session_id.clone();
        if *session.generation.lock().await != generation {
            return;
        }

        set_state(
            &session,
            RdpSessionState::Authenticating,
            Some("Authenticating RDP session".to_string()),
        )
        .await;
        emit_state(
            &app,
            &session_id,
            RdpSessionState::Authenticating,
            Some("Authenticating RDP session".to_string()),
        );

        let (output_sender, mut output_receiver) = mpsc::channel(2);
        let client = IronRdpClient::new(iron_config, output_sender);
        let input_sender = client.input_sender();
        {
            *session.input_sender.lock().await = Some(input_sender);
            *session.input_database.lock().await = IronRdpInputDatabase::new();
            *session.frame_sequence.lock().await = 0;
        }

        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(client.run()),
                Err(error) => tracing::error!(%error, "Failed to start IronRDP runtime"),
            }
        });
        let mut saw_terminal_event = false;

        while let Some(event) = output_receiver.recv().await {
            if *session.generation.lock().await != generation {
                close_current_input_sender(&session).await;
                break;
            }

            match event {
                RdpOutputEvent::ImagePatch {
                    buffer,
                    desktop_width,
                    desktop_height,
                    x,
                    y,
                    width,
                    height,
                } => {
                    let was_active = matches!(*session.state.lock().await, RdpSessionState::Active);
                    if !was_active {
                        set_state(&session, RdpSessionState::Active, None).await;
                        emit_state(&app, &session_id, RdpSessionState::Active, None);
                        let _ = app.emit("sessions-changed", ());
                    }
                    let sequence = next_frame_sequence(&session).await;
                    let frame = build_frame_from_ironrdp_image(
                        &buffer,
                        desktop_width,
                        desktop_height,
                        x,
                        y,
                        width,
                        height,
                        sequence,
                    );
                    queue_or_send_frame(&session, frame).await;
                }
                RdpOutputEvent::ConnectionFailure(error) => {
                    saw_terminal_event = true;
                    let message = format!("RDP connection failed: {error:?}");
                    tracing::warn!(
                        session_id = %session_id,
                        host = %session.config.host,
                        port = session.config.port,
                        error = %message,
                        "RDP connection failed"
                    );
                    fail_rdp_session(&app, &session, generation, message).await;
                    break;
                }
                RdpOutputEvent::Terminated(result) => {
                    saw_terminal_event = true;
                    match result {
                        Ok(reason) => {
                            let message = format!("RDP session disconnected: {reason}");
                            set_state(
                                &session,
                                RdpSessionState::Disconnected,
                                Some(message.clone()),
                            )
                            .await;
                            emit_state(
                                &app,
                                &session_id,
                                RdpSessionState::Disconnected,
                                Some(message),
                            );
                            let _ = app.emit("sessions-changed", ());
                        }
                        Err(error) => {
                            let message = format!("RDP session error: {error:?}");
                            tracing::warn!(
                                session_id = %session_id,
                                host = %session.config.host,
                                port = session.config.port,
                                error = %message,
                                "RDP active session failed"
                            );
                            fail_rdp_session(&app, &session, generation, message).await;
                        }
                    }
                    break;
                }
                RdpOutputEvent::PointerDefault
                | RdpOutputEvent::PointerHidden
                | RdpOutputEvent::PointerPosition { .. }
                | RdpOutputEvent::PointerBitmap(_) => {}
            }
        }

        close_current_input_sender(&session).await;
        if !saw_terminal_event && *session.generation.lock().await == generation {
            set_state(
                &session,
                RdpSessionState::Disconnected,
                Some("RDP session ended".to_string()),
            )
            .await;
            emit_state(
                &app,
                &session_id,
                RdpSessionState::Disconnected,
                Some("RDP session ended".to_string()),
            );
            let _ = app.emit("sessions-changed", ());
        }
    });
}

fn build_ironrdp_config(config: &RdpConnectConfig) -> Result<IronRdpConfig, String> {
    let width = u16::try_from(config.width)
        .map_err(|_| "RDP display width is outside the supported range".to_string())?;
    let height = u16::try_from(config.height)
        .map_err(|_| "RDP display height is outside the supported range".to_string())?;
    let color_depth = match config.color_depth {
        16 | 32 => u32::from(config.color_depth),
        _ => 32,
    };

    IronRdpConfigBuilder::new()
        .with_destination(IronRdpDestination::from_parts(
            config.host.clone(),
            config.port,
        ))
        .with_transport(IronRdpTransportKind::Direct)
        .with_username(config.username.clone())
        .with_domain(config.domain.clone())
        .with_password(config.password.clone().unwrap_or_default())
        .with_desktop_width(width)
        .with_desktop_height(height)
        .with_desktop_scale_factor(100)
        .with_color_depth(color_depth)
        .with_credssp(config.use_nla)
        .with_tls(true)
        .with_autologon(true)
        .with_compression(true)
        .with_server_pointer(true)
        .with_client_build(client_build())
        .with_client_dir("C:\\Windows\\System32\\mstscax.dll")
        .with_client_name(client_name())
        .with_platform(current_platform())
        .build()
        .map_err(|error| format!("Unable to build RDP config: {error}"))
}

fn rdp_input_to_operations(event: RdpInputEvent) -> Option<Vec<IronRdpInputOperation>> {
    match event {
        RdpInputEvent::KeyDown {
            scan_code,
            extended,
            ..
        } => Some(vec![IronRdpInputOperation::KeyPressed(
            IronRdpScancode::from_u8(extended, scan_code as u8),
        )]),
        RdpInputEvent::KeyUp {
            scan_code,
            extended,
            ..
        } => Some(vec![IronRdpInputOperation::KeyReleased(
            IronRdpScancode::from_u8(extended, scan_code as u8),
        )]),
        RdpInputEvent::MouseMove { x, y } => Some(vec![IronRdpInputOperation::MouseMove(
            IronRdpMousePosition {
                x: clamp_u32_to_u16(x),
                y: clamp_u32_to_u16(y),
            },
        )]),
        RdpInputEvent::MouseButton {
            button,
            pressed,
            x,
            y,
        } => {
            let Some(button) = ironrdp_mouse_button(&button) else {
                return Some(Vec::new());
            };
            let mut operations = vec![IronRdpInputOperation::MouseMove(IronRdpMousePosition {
                x: clamp_u32_to_u16(x),
                y: clamp_u32_to_u16(y),
            })];
            operations.push(if pressed {
                IronRdpInputOperation::MouseButtonPressed(button)
            } else {
                IronRdpInputOperation::MouseButtonReleased(button)
            });
            Some(operations)
        }
        RdpInputEvent::MouseWheel {
            delta_x,
            delta_y,
            x,
            y,
        } => {
            let mut operations = vec![IronRdpInputOperation::MouseMove(IronRdpMousePosition {
                x: clamp_u32_to_u16(x),
                y: clamp_u32_to_u16(y),
            })];
            if delta_x.abs() > 0.001 {
                operations.push(IronRdpInputOperation::WheelRotations(
                    IronRdpWheelRotations {
                        is_vertical: false,
                        rotation_units: clamp_f64_to_i16(delta_x),
                    },
                ));
            }
            if delta_y.abs() > 0.001 {
                operations.push(IronRdpInputOperation::WheelRotations(
                    IronRdpWheelRotations {
                        is_vertical: true,
                        rotation_units: clamp_f64_to_i16(delta_y),
                    },
                ));
            }
            Some(operations)
        }
        RdpInputEvent::Unicode { text } => {
            let mut operations = Vec::new();
            for character in text.chars() {
                operations.push(IronRdpInputOperation::UnicodeKeyPressed(character));
                operations.push(IronRdpInputOperation::UnicodeKeyReleased(character));
            }
            Some(operations)
        }
        RdpInputEvent::ReleaseAllKeys => None,
    }
}

fn ironrdp_mouse_button(button: &str) -> Option<IronRdpMouseButton> {
    match button {
        "left" => Some(IronRdpMouseButton::Left),
        "middle" => Some(IronRdpMouseButton::Middle),
        "right" => Some(IronRdpMouseButton::Right),
        "back" => Some(IronRdpMouseButton::X1),
        "forward" => Some(IronRdpMouseButton::X2),
        _ => None,
    }
}

fn clamp_u32_to_u16(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn clamp_f64_to_i16(value: f64) -> i16 {
    if value.is_nan() {
        return 0;
    }
    value.clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16
}

async fn close_current_input_sender(session: &RdpSession) {
    if let Some(sender) = session.input_sender.lock().await.take() {
        let _ = sender.send(IronRdpInputEvent::Close);
    }
}

async fn fail_rdp_session(app: &AppHandle, session: &RdpSession, generation: u64, message: String) {
    if *session.generation.lock().await != generation {
        return;
    }
    set_state(session, RdpSessionState::Failed, Some(message.clone())).await;
    emit_state(
        app,
        &session.config.session_id,
        RdpSessionState::Failed,
        Some(message),
    );
    let _ = app.emit("sessions-changed", ());
}

async fn next_frame_sequence(session: &RdpSession) -> u64 {
    let mut sequence = session.frame_sequence.lock().await;
    let current = *sequence;
    *sequence = sequence.wrapping_add(1);
    current
}

fn build_frame_from_ironrdp_image(
    pixels: &[u32],
    desktop_width: u16,
    desktop_height: u16,
    patch_x: u16,
    patch_y: u16,
    patch_width: u16,
    patch_height: u16,
    sequence: u64,
) -> Vec<u8> {
    let desktop_width = u32::from(desktop_width);
    let desktop_height = u32::from(desktop_height);
    let patch_x = u32::from(patch_x);
    let patch_y = u32::from(patch_y);
    let patch_width = u32::from(patch_width);
    let patch_height = u32::from(patch_height);
    let stride = patch_width * 4;
    let payload_len = (stride * patch_height) as usize;
    let mut buffer = vec![0_u8; FRAME_HEADER_BYTES + payload_len];
    write_u64(&mut buffer, 0, sequence);
    write_u32(&mut buffer, 8, desktop_width);
    write_u32(&mut buffer, 12, desktop_height);
    write_u32(&mut buffer, 16, patch_x);
    write_u32(&mut buffer, 20, patch_y);
    write_u32(&mut buffer, 24, patch_width);
    write_u32(&mut buffer, 28, patch_height);
    write_u32(&mut buffer, 32, stride);
    write_u32(&mut buffer, 36, PIXEL_FORMAT_RGBA8888);
    write_u32(&mut buffer, 40, payload_len as u32);

    for (index, pixel) in pixels
        .iter()
        .take((patch_width * patch_height) as usize)
        .enumerate()
    {
        let [_, red, green, blue] = pixel.to_be_bytes();
        let offset = FRAME_HEADER_BYTES + index * 4;
        buffer[offset] = red;
        buffer[offset + 1] = green;
        buffer[offset + 2] = blue;
        buffer[offset + 3] = 255;
    }

    buffer
}

fn client_build() -> u32 {
    env!("CARGO_PKG_VERSION")
        .split('.')
        .take(3)
        .fold(0_u32, |acc, part| {
            acc.saturating_mul(100)
                .saturating_add(part.parse::<u32>().unwrap_or(0))
        })
}

fn client_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "NyaTerm".to_string())
}

fn current_platform() -> MajorPlatformType {
    #[cfg(target_os = "windows")]
    {
        MajorPlatformType::WINDOWS
    }
    #[cfg(target_os = "macos")]
    {
        MajorPlatformType::MACINTOSH
    }
    #[cfg(target_os = "ios")]
    {
        MajorPlatformType::IOS
    }
    #[cfg(target_os = "android")]
    {
        MajorPlatformType::ANDROID
    }
    #[cfg(all(
        not(target_os = "windows"),
        not(target_os = "macos"),
        not(target_os = "ios"),
        not(target_os = "android")
    ))]
    {
        MajorPlatformType::UNIX
    }
}

async fn set_state(session: &RdpSession, state: RdpSessionState, message: Option<String>) {
    *session.state.lock().await = state;
    *session.message.lock().await = message;
}

async fn emit_current_state(app: &AppHandle, session: &RdpSession) {
    let state = session.state.lock().await.clone();
    let message = session.message.lock().await.clone();
    emit_state(app, &session.config.session_id, state, message);
}

async fn bump_generation(session: &RdpSession) -> u64 {
    let mut generation = session.generation.lock().await;
    *generation = generation.wrapping_add(1);
    *generation
}

async fn queue_or_send_frame(session: &RdpSession, frame: Vec<u8>) {
    let sent = {
        let channel = session.frame_channel.lock().await;
        channel
            .as_ref()
            .is_some_and(|sender| sender.send(InvokeResponseBody::Raw(frame.clone())).is_ok())
    };
    if sent {
        return;
    }

    let mut pending = session.pending_frames.lock().await;
    while pending.len() >= MAX_FRAME_QUEUE {
        pending.pop_front();
    }
    pending.push_back(frame);
}

async fn flush_pending_frames(session: &RdpSession) {
    let channel = session.frame_channel.lock().await;
    let Some(channel) = channel.as_ref() else {
        return;
    };
    let mut pending = session.pending_frames.lock().await;
    while let Some(frame) = pending.pop_front() {
        if channel.send(InvokeResponseBody::Raw(frame)).is_err() {
            break;
        }
    }
}

fn build_placeholder_frame(config: &RdpConnectConfig, sequence: u64) -> Vec<u8> {
    let width = config.width.max(640);
    let height = config.height.max(480);
    let stride = width * 4;
    let payload_len = (stride * height) as usize;
    let mut buffer = vec![0_u8; FRAME_HEADER_BYTES + payload_len];
    write_u64(&mut buffer, 0, sequence);
    write_u32(&mut buffer, 8, width);
    write_u32(&mut buffer, 12, height);
    write_u32(&mut buffer, 16, 0);
    write_u32(&mut buffer, 20, 0);
    write_u32(&mut buffer, 24, width);
    write_u32(&mut buffer, 28, height);
    write_u32(&mut buffer, 32, stride);
    write_u32(&mut buffer, 36, PIXEL_FORMAT_BGRA8888);
    write_u32(&mut buffer, 40, payload_len as u32);

    let tick = (sequence % 255) as u8;
    for y in 0..height {
        for x in 0..width {
            let offset = FRAME_HEADER_BYTES + ((y * stride) + x * 4) as usize;
            buffer[offset] = ((x * 255 / width) as u8).saturating_add(tick / 4);
            buffer[offset + 1] = ((y * 255 / height) as u8).saturating_add(32);
            buffer[offset + 2] = 48_u8.saturating_add(tick);
            buffer[offset + 3] = 255;
        }
    }
    buffer
}

fn write_u32(buffer: &mut [u8], offset: usize, value: u32) {
    buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(buffer: &mut [u8], offset: usize, value: u64) {
    buffer[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> RdpConnectConfig {
        RdpConnectConfig {
            session_id: "s".to_string(),
            connection_id: "c".to_string(),
            name: "RDP".to_string(),
            host: "127.0.0.1".to_string(),
            port: 3389,
            username: "user".to_string(),
            domain: String::new(),
            password: None,
            width: 640,
            height: 480,
            use_nla: true,
            certificate_policy: "prompt".to_string(),
            color_depth: 32,
        }
    }

    fn test_session() -> RdpSession {
        RdpSession {
            config: test_config(),
            state: Mutex::new(RdpSessionState::Connecting),
            message: Mutex::new(None),
            generation: Mutex::new(0),
            frame_channel: Mutex::new(None),
            pending_frames: Mutex::new(VecDeque::with_capacity(MAX_FRAME_QUEUE)),
            input_sender: Mutex::new(None),
            input_database: Mutex::new(IronRdpInputDatabase::new()),
            frame_sequence: Mutex::new(0),
        }
    }

    #[test]
    fn placeholder_frame_has_expected_header() {
        let config = test_config();
        let frame = build_placeholder_frame(&config, 7);
        assert_eq!(&frame[0..8], &7_u64.to_le_bytes());
        assert_eq!(&frame[8..12], &640_u32.to_le_bytes());
        assert_eq!(&frame[12..16], &480_u32.to_le_bytes());
        assert_eq!(&frame[36..40], &PIXEL_FORMAT_BGRA8888.to_le_bytes());
    }

    #[test]
    fn ironrdp_image_patch_has_expected_header_and_payload() {
        let pixels = [0x0011_2233, 0x0044_5566, 0x0077_8899, 0x00aa_bbcc];
        let frame = build_frame_from_ironrdp_image(&pixels, 1920, 1080, 10, 20, 2, 2, 9);

        assert_eq!(&frame[0..8], &9_u64.to_le_bytes());
        assert_eq!(&frame[8..12], &1920_u32.to_le_bytes());
        assert_eq!(&frame[12..16], &1080_u32.to_le_bytes());
        assert_eq!(&frame[16..20], &10_u32.to_le_bytes());
        assert_eq!(&frame[20..24], &20_u32.to_le_bytes());
        assert_eq!(&frame[24..28], &2_u32.to_le_bytes());
        assert_eq!(&frame[28..32], &2_u32.to_le_bytes());
        assert_eq!(&frame[32..36], &8_u32.to_le_bytes());
        assert_eq!(&frame[36..40], &PIXEL_FORMAT_RGBA8888.to_le_bytes());
        assert_eq!(&frame[40..44], &16_u32.to_le_bytes());
        assert_eq!(
            &frame[44..],
            &[
                0x11, 0x22, 0x33, 0xff, 0x44, 0x55, 0x66, 0xff, 0x77, 0x88, 0x99, 0xff, 0xaa, 0xbb,
                0xcc, 0xff
            ]
        );
    }

    #[tokio::test]
    async fn pending_frame_queue_keeps_latest_frames_under_pressure() {
        let session = test_session();

        queue_or_send_frame(&session, vec![1]).await;
        queue_or_send_frame(&session, vec![2]).await;
        queue_or_send_frame(&session, vec![3]).await;

        let pending = session.pending_frames.lock().await;
        assert_eq!(pending.len(), MAX_FRAME_QUEUE);
        assert_eq!(pending[0], vec![2]);
        assert_eq!(pending[1], vec![3]);
    }
}
