/// Firebase custom claims management via the Identity Toolkit Admin REST API.
/// Fires in the background after subscription changes so billing endpoints are not blocked.
#[derive(Clone)]
pub struct ClaimsService {
    client: reqwest::Client,
    local_dev: bool,
}

impl ClaimsService {
    pub fn new(client: reqwest::Client, local_dev: bool) -> Self {
        Self { client, local_dev }
    }

    /// Spawn a background task to update Firebase custom claims.
    /// Errors are logged but do not fail the caller — claims propagate on the user's next token refresh.
    pub fn mint_in_background(&self, uid: String, org_id: String, apps: Vec<String>, role: String) {
        let service = self.clone();
        tokio::spawn(async move {
            if let Err(e) = service.mint(&uid, &org_id, &apps, &role).await {
                tracing::warn!(uid, org_id, "Failed to update custom claims: {e}");
            } else {
                tracing::info!(uid, org_id, role, "Custom claims updated");
            }
        });
    }

    async fn mint(
        &self,
        uid: &str,
        org_id: &str,
        apps: &[String],
        role: &str,
    ) -> anyhow::Result<()> {
        if self.local_dev {
            tracing::debug!(
                uid,
                org_id,
                ?apps,
                role,
                "LOCAL_DEV: skipping custom claims update"
            );
            return Ok(());
        }

        let token = self.fetch_gcp_token().await?;
        let claims = serde_json::json!({ "org_id": org_id, "apps": apps, "role": role });
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
        runway_secrets::metadata::fetch_access_token(&self.client).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_service_stores_local_dev_flag() {
        // RP-HERMETIC-UNIT (Reflective QUALITY_BACKLOG.md →
        // QF-2026-06-02-05): the client here is a sentinel — this test
        // exercises only the `local_dev` flag plumbing, not the HTTP
        // path. If a future test needs to hit a stubbed identity
        // service, wire a stub client (e.g. `wiremock`-backed) via the
        // existing `ClaimsService::new(client, local_dev)` DI signature
        // instead.
        #[allow(clippy::disallowed_methods)]
        let client = reqwest::Client::new();
        let svc = ClaimsService::new(client, true);
        assert!(svc.local_dev);
    }
}
