//! Runtime configuration for the app-host (the binary container that
//! runs a Runway app inside Cloud Run or locally).

use crate::AppExecutionPacket;

#[derive(Debug, Clone)]
pub struct HostConfig {
    pub local_dev: bool,
    pub storage_path: String,
    pub firebase_project_id: String,
    pub route_prefix: Option<String>,
}

impl HostConfig {
    /// Read from process environment, falling back to packet-supplied
    /// defaults where applicable.
    pub fn from_env(packet: &AppExecutionPacket) -> Self {
        let local_dev = std::env::var("LOCAL_DEV").as_deref() == Ok("true");

        let storage_path =
            std::env::var("STORAGE_PATH").unwrap_or_else(|_| format!("/tmp/{}", packet.app_id));

        let firebase_project_id = std::env::var("FIREBASE_PROJECT_ID")
            .or_else(|_| std::env::var("GOOGLE_CLOUD_PROJECT"))
            .or_else(|_| std::env::var("GCP_PROJECT_ID"))
            .unwrap_or_else(|_| "dev-project".to_string());

        let route_prefix = {
            let raw = std::env::var("ROUTE_PREFIX").unwrap_or_else(|_| packet.route_prefix.clone());
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed == "/" {
                None
            } else if trimmed.starts_with('/') {
                Some(trimmed.to_string())
            } else {
                Some(format!("/{trimmed}"))
            }
        };

        Self {
            local_dev,
            storage_path,
            firebase_project_id,
            route_prefix,
        }
    }
}
