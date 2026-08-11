use crate::core::rdp::{self, RdpInputEvent, RdpSessionManager};
use crate::error::AppResult;
use std::sync::Arc;
use tauri::ipc::{Channel, InvokeResponseBody};

#[tauri::command]
pub async fn create_rdp_session(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<RdpSessionManager>>,
    connection_id: String,
    create_request_id: Option<String>,
) -> AppResult<String> {
    let _ = create_request_id;
    let config = rdp::load_saved_rdp_config(&app, &connection_id)?;
    let session_id = state.create_session(app.clone(), config).await?;
    if let Err(error) = crate::storage::mark_connection_used(&connection_id) {
        tracing::warn!(connection_id, %error, "Failed to mark RDP connection as recently used");
    }
    Ok(session_id)
}

#[tauri::command]
pub async fn rdp_attach_frame_channel(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<RdpSessionManager>>,
    session_id: String,
    frame_channel: Channel<InvokeResponseBody>,
) -> AppResult<()> {
    state
        .attach_frame_channel(&app, &session_id, frame_channel)
        .await
}

#[tauri::command]
pub async fn rdp_input_batch(
    state: tauri::State<'_, Arc<RdpSessionManager>>,
    session_id: String,
    events: Vec<RdpInputEvent>,
) -> AppResult<()> {
    state.send_input(&session_id, events).await
}

#[tauri::command]
pub async fn rdp_resize(
    state: tauri::State<'_, Arc<RdpSessionManager>>,
    session_id: String,
    width: u32,
    height: u32,
) -> AppResult<()> {
    state.resize(&session_id, width, height).await
}

#[tauri::command]
pub async fn rdp_set_clipboard_text(
    state: tauri::State<'_, Arc<RdpSessionManager>>,
    session_id: String,
    text: String,
) -> AppResult<()> {
    state.set_clipboard_text(&session_id, text).await
}

#[tauri::command]
pub async fn rdp_reconnect(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<RdpSessionManager>>,
    session_id: String,
) -> AppResult<()> {
    state.reconnect(app, &session_id).await
}

#[tauri::command]
pub async fn close_rdp_session(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<RdpSessionManager>>,
    session_id: String,
) -> AppResult<()> {
    state.close(&app, &session_id).await
}

#[tauri::command]
pub async fn respond_rdp_certificate(
    _state: tauri::State<'_, Arc<RdpSessionManager>>,
    _request_id: String,
    _accepted: bool,
    _remember: bool,
) -> AppResult<()> {
    Ok(())
}
