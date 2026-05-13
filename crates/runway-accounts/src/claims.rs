/// Firebase custom claims management via the Identity Toolkit Admin REST API.
/// Fires in the background after subscription changes so billing endpoints are not blocked.
#[derive(Clone)]
pub struct ClaimsService {
    client: reqwest::Client,
    local_dev: bool,
}

impl ClaimsService {
    pub fn new(client: reqwest::Client) -> Self {
        let local_dev = std::env::var("LOCAL_DEV").as_deref() == Ok("true");
        Self { client, local_dev }
    }

    /// Spawn a background task to update Firebase custom claims.
    /// Errors are logged but do not fail the caller — claims propagate on the user's next token refresh.
    pub fn mint_in_background(&self, uid: String, org_id: String, apps: Vec<String>) {
        let service = self.clone();
        tokio::spawn(async move {
            if let Err(e) = service.mint(&uid, &org_id, &apps).await {
                tracing::warn!(uid, org_id, "Failed to update custom claims: {e}");
            } else {
                tracing::info!(uid, org_id, "Custom claims updated");
            }
        });
    }

    async fn mint(&self, uid: &str, org_id: &str, apps: &[String]) -> anyhow::Result<()> {
        if self.local_dev {
            tracing::debug!(uid, org_id, ?apps, "LOCAL_DEV: skipping custom claims update");
            return Ok(());
        }

        let token = self.fetch_gcp_token().await?;
        let claims = serde_json::json!({ "org_id": org_id, "apps": apps });
        let custom_attributes = serde_json::to_string(&claims)?;

        let resp = self
            .client
            .post("https://identitytoolkit.googleapis.com/v1/accounts:update")
            .bearer_auth(&token)
            .json(&serde_json::json!({
                "localId": uid,
                "customAttributes": custom_attributes,
            }))
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Identity Toolkit {status}: {body}");
        }
        Ok(())
    }

    async fn fetch_gcp_token(&self) -> anyhow::Result<String> {
        let resp = self
            .client
            .get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token")
            .header("Metadata-Flavor", "Google")
            .send()
            .await?;

        let body: serde_json::Value = resp.json().await?;
        body["access_token"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow::anyhow!("no access_token in metadata response"))
    }
}
