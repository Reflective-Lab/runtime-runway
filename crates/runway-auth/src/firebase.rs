use serde::Deserialize;

/// Custom claims expected in Firebase ID tokens issued by the backend after org creation.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct FirebaseClaims {
    pub uid: String,
    pub email: Option<String>,
    /// The org this user belongs to.
    pub org_id: Option<String>,
    /// Apps the org has an active subscription for: ["folio", "wolfgang", ...]
    #[serde(default)]
    pub apps: Vec<String>,
    /// Role within the org.
    pub role: Option<String>,
}

impl FirebaseClaims {
    pub fn has_app(&self, app: &str) -> bool {
        self.apps.iter().any(|a| a == app)
    }
}

pub struct FirebaseAuth {
    api_key: String,
    client: reqwest::Client,
}

impl FirebaseAuth {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            client: reqwest::Client::new(),
        }
    }

    /// Verify a Firebase ID token and return decoded claims.
    ///
    /// Uses the Identity Toolkit lookup endpoint (same approach as Wolfgang).
    /// For a full offline JWT verification, replace with jsonwebtoken + JWKS caching.
    pub async fn verify(&self, id_token: &str) -> anyhow::Result<FirebaseClaims> {
        let url = format!(
            "https://identitytoolkit.googleapis.com/v1/accounts:lookup?key={}",
            self.api_key
        );
        let resp: serde_json::Value = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "idToken": id_token }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let user = resp["users"][0].clone();
        let uid = user["localId"].as_str().unwrap_or("").to_string();
        let email = user["email"].as_str().map(|s| s.to_string());

        // Custom claims are a JSON string stored in customAttributes
        let mut claims = FirebaseClaims {
            uid,
            email,
            ..Default::default()
        };
        if let Some(custom_str) = user["customAttributes"].as_str()
            && let Ok(custom) = serde_json::from_str::<FirebaseClaims>(custom_str)
        {
            claims.org_id = custom.org_id;
            claims.apps = custom.apps;
            claims.role = custom.role;
        }
        Ok(claims)
    }
}
