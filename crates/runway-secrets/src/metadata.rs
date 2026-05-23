//! GCE/Cloud Run metadata server helpers.
//!
//! Single source of truth for the GCP instance-metadata URL. All Runway
//! crates that need a service-account access token route through here
//! instead of hardcoding the URL themselves.

const METADATA_TOKEN_URL: &str =
    "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token";

/// Fetch a short-lived OAuth2 access token from the GCE metadata server.
///
/// Returns an error if the request fails or the response is missing
/// `access_token`. Callers in non-GCP environments must use a different
/// token source (e.g. `RemoteConfig::TokenSource::Static`).
pub async fn fetch_access_token(client: &reqwest::Client) -> anyhow::Result<String> {
    let resp: serde_json::Value = client
        .get(METADATA_TOKEN_URL)
        .header("Metadata-Flavor", "Google")
        .send()
        .await?
        .json()
        .await?;
    resp["access_token"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow::anyhow!("no access_token in GCE metadata response"))
}
