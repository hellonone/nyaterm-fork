//! SSH session creation, TOFU known_hosts verification, and I/O loop.
//!
//! Split by concern so connection setup, auth flow, and terminal I/O remain
//! independently maintainable as the SSH feature set grows.

mod agent;
mod auth;
mod client;
mod io;
pub(crate) mod osc;
mod session;
mod tunnel;
pub(crate) mod x11_forwarding;

pub(crate) use auth::load_saved_ssh_config;
pub use auth::{
    PendingAuthManager, PendingSshAgentAuthManager, PendingSshAuthManager, SshAgentAuthAction,
    SshAuthResponse,
};
pub use client::{HostKeyVerifyManager, SupportedSshAlgorithms, get_supported_ssh_algorithms};
pub(crate) use client::{
    RemoteForwardOpen, SshAuth, SshConfig, SshConnectionHandles, SshHandle, SshRawHandle,
    SshStartupCommand, validate_ssh_algorithm_preferences,
};
pub(crate) use session::create_ssh_handle_for_tunnel;
pub use session::{create_multiplexed_ssh_session, create_ssh_session};
pub(crate) use tunnel::{TunnelManager, TunnelRuntimeState};
