//! Shared CarlaOS core.
//!
//! Both the Ally shell and the desktop companion link this crate, so a desktop
//! describes itself and its games in exactly the shape the Ally uses locally.

use serde::{Deserialize, Serialize};

pub mod admin;
pub mod debloat;
pub mod library;
pub mod link;
pub mod secret;

/// Version of the core crate, shown in Settings and reported by the companion.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Version of the companion HTTP API (`/v1/...`).
pub const API_VERSION: u32 = 1;

/// Which program a status report comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The handheld interface.
    Shell,
    /// The background service on a desktop that streams games.
    Companion,
}

/// Minimal self-description every CarlaOS program can produce.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInfo {
    pub role: Role,
    pub version: String,
    pub api_version: u32,
    pub os: String,
    pub arch: String,
}

impl AppInfo {
    pub fn current(role: Role) -> Self {
        Self {
            role,
            version: VERSION.to_string(),
            api_version: API_VERSION,
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_round_trips_as_json() {
        let info = AppInfo::current(Role::Companion);
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"role\":\"companion\""));
        assert_eq!(serde_json::from_str::<AppInfo>(&json).unwrap(), info);
    }
}
