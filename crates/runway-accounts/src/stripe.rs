use chrono::Utc;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;

use crate::error::AccountError;

const STRIPE_API_BASE: &str = "https://api.stripe.com/v1";
const WEBHOOK_TOLERANCE_SECONDS: i64 = 300;

#[derive(Clone)]
pub struct StripeClient {
    client: reqwest::Client,
    secret_key: Option<String>,
}

impl StripeClient {
    pub fn new(client: reqwest::Client) -> Self {
        let secret_key = std::env::var("STRIPE_SECRET_KEY")
            .ok()
            .filter(|v| !v.trim().is_empty());
        Self { client, secret_key }
    }

    pub fn is_configured(&self) -> bool {
        self.secret_key.is_some()
    }

    fn key(&self) -> Result<&str, AccountError> {
        self.secret_key
            .as_deref()
            .ok_or_else(|| AccountError::Stripe("STRIPE_SECRET_KEY not configured".into()))
    }

    /// Find an existing Stripe customer for a Firebase UID, returning the customer ID if found.
    pub async fn find_customer_id(&self, uid: &str) -> Result<Option<String>, AccountError> {
        let Some(key) = &self.secret_key else {
            return Ok(None);
        };
        let query = format!("metadata['firebase_uid']:'{}'", uid.replace('\'', "\\'"));
        let resp = self
            .client
            .get(format!("{STRIPE_API_BASE}/customers/search"))
            .bearer_auth(key)
            .query(&[("query", query.as_str())])
            .send()
            .await
            .map_err(|e| AccountError::Stripe(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(AccountError::Stripe(format!(
                "customer search failed: {}",
                resp.status()
            )));
        }

        let list: StripeList<StripeCustomer> =
            resp.json().await.map_err(|e| AccountError::Stripe(e.to_string()))?;
        Ok(list.data.into_iter().next().map(|c| c.id))
    }

    /// Find or create a Stripe customer for a Firebase UID, returning the customer ID.
    pub async fn ensure_customer(
        &self,
        uid: &str,
        email: Option<&str>,
    ) -> Result<String, AccountError> {
        if let Some(id) = self.find_customer_id(uid).await? {
            return Ok(id);
        }
        let key = self.key()?;
        let mut form: Vec<(&str, String)> = vec![("metadata[firebase_uid]", uid.to_string())];
        if let Some(e) = email {
            form.push(("email", e.to_string()));
        }
        let resp = self
            .client
            .post(format!("{STRIPE_API_BASE}/customers"))
            .bearer_auth(key)
            .form(&form)
            .send()
            .await
            .map_err(|e| AccountError::Stripe(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(AccountError::Stripe(format!(
                "customer creation failed: {}",
                resp.status()
            )));
        }

        let customer: StripeCustomer =
            resp.json().await.map_err(|e| AccountError::Stripe(e.to_string()))?;
        Ok(customer.id)
    }

    /// Create a Stripe Checkout session and return the redirect URL.
    pub async fn create_checkout_session(
        &self,
        customer_id: &str,
        price_id: &str,
        mode: &str,
        success_url: &str,
        cancel_url: &str,
        firebase_uid: &str,
    ) -> Result<String, AccountError> {
        let key = self.key()?;
        let idempotency_key = format!("checkout_{}_{}", firebase_uid, uuid::Uuid::new_v4());
        let form: Vec<(&str, &str)> = vec![
            ("mode", mode),
            ("customer", customer_id),
            ("success_url", success_url),
            ("cancel_url", cancel_url),
            ("line_items[0][price]", price_id),
            ("line_items[0][quantity]", "1"),
            ("client_reference_id", firebase_uid),
            ("metadata[firebase_uid]", firebase_uid),
            ("allow_promotion_codes", "true"),
        ];
        let resp = self
            .client
            .post(format!("{STRIPE_API_BASE}/checkout/sessions"))
            .bearer_auth(key)
            .header("Idempotency-Key", &idempotency_key)
            .form(&form)
            .send()
            .await
            .map_err(|e| AccountError::Stripe(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(AccountError::Stripe(format!(
                "checkout session failed: {}",
                resp.status()
            )));
        }

        let session: StripeCheckoutSession =
            resp.json().await.map_err(|e| AccountError::Stripe(e.to_string()))?;
        session
            .url
            .ok_or_else(|| AccountError::Stripe("no URL in checkout session response".into()))
    }

    /// Create a Stripe Billing Portal session and return the redirect URL.
    pub async fn create_portal_session(
        &self,
        customer_id: &str,
        return_url: &str,
    ) -> Result<String, AccountError> {
        let key = self.key()?;
        let form: Vec<(&str, &str)> = vec![("customer", customer_id), ("return_url", return_url)];
        let resp = self
            .client
            .post(format!("{STRIPE_API_BASE}/billing_portal/sessions"))
            .bearer_auth(key)
            .form(&form)
            .send()
            .await
            .map_err(|e| AccountError::Stripe(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(AccountError::Stripe(format!(
                "portal session failed: {}",
                resp.status()
            )));
        }

        let session: StripePortalSession =
            resp.json().await.map_err(|e| AccountError::Stripe(e.to_string()))?;
        Ok(session.url)
    }

    /// Get the most relevant active or trialing subscription for a customer.
    pub async fn get_subscription(
        &self,
        customer_id: &str,
    ) -> Result<Option<StripeSubscription>, AccountError> {
        let Some(key) = &self.secret_key else {
            return Ok(None);
        };
        let resp = self
            .client
            .get(format!("{STRIPE_API_BASE}/subscriptions"))
            .bearer_auth(key)
            .query(&[
                ("customer", customer_id),
                ("limit", "1"),
                ("status", "active"),
            ])
            .send()
            .await
            .map_err(|e| AccountError::Stripe(e.to_string()))?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        let list: StripeList<StripeSubscription> =
            resp.json().await.map_err(|e| AccountError::Stripe(e.to_string()))?;

        // Also try trialing if no active found
        if list.data.is_empty() {
            let resp2 = self
                .client
                .get(format!("{STRIPE_API_BASE}/subscriptions"))
                .bearer_auth(key)
                .query(&[("customer", customer_id), ("limit", "1"), ("status", "trialing")])
                .send()
                .await
                .map_err(|e| AccountError::Stripe(e.to_string()))?;
            if resp2.status().is_success() {
                let list2: StripeList<StripeSubscription> =
                    resp2.json().await.map_err(|e| AccountError::Stripe(e.to_string()))?;
                return Ok(list2.data.into_iter().next());
            }
        }

        Ok(list.data.into_iter().next())
    }

    /// Verify a Stripe webhook signature (HMAC-SHA256).
    /// Returns false if the signature is invalid or the timestamp is too old.
    pub fn verify_signature(&self, payload: &[u8], sig_header: &str, secret: &str) -> bool {
        let mut timestamp: Option<&str> = None;
        let mut signatures: Vec<&str> = Vec::new();
        for part in sig_header.split(',') {
            let part = part.trim();
            if let Some(t) = part.strip_prefix("t=") {
                timestamp = Some(t);
            } else if let Some(sig) = part.strip_prefix("v1=") {
                signatures.push(sig);
            }
        }

        let Some(ts_str) = timestamp else {
            return false;
        };
        if signatures.is_empty() {
            return false;
        }
        let Ok(ts) = ts_str.parse::<i64>() else {
            return false;
        };
        if (Utc::now().timestamp() - ts).abs() > WEBHOOK_TOLERANCE_SECONDS {
            return false;
        }

        let signed_payload = format!("{ts_str}.{}", String::from_utf8_lossy(payload));
        let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
            return false;
        };
        mac.update(signed_payload.as_bytes());
        let expected: String = mac
            .finalize()
            .into_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();

        // Constant-time comparison
        signatures.iter().any(|sig| {
            sig.len() == expected.len()
                && sig
                    .bytes()
                    .zip(expected.bytes())
                    .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                    == 0
        })
    }
}

// --- Stripe response types ---

#[derive(Debug, Deserialize)]
pub(crate) struct StripeList<T> {
    pub data: Vec<T>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StripeCustomer {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub struct StripeSubscription {
    pub id: String,
    pub status: String,
    pub items: StripeSubscriptionItems,
    #[serde(default)]
    pub current_period_end: i64,
}

impl StripeSubscription {
    pub fn price_ids(&self) -> Vec<&str> {
        self.items.data.iter().map(|item| item.price.id.as_str()).collect()
    }

    pub fn is_active(&self) -> bool {
        self.status == "active" || self.status == "trialing"
    }
}

#[derive(Debug, Deserialize)]
pub struct StripeSubscriptionItems {
    pub data: Vec<StripeSubscriptionItem>,
}

#[derive(Debug, Deserialize)]
pub struct StripeSubscriptionItem {
    pub price: StripePrice,
}

#[derive(Debug, Deserialize)]
pub struct StripePrice {
    pub id: String,
}

#[derive(Debug, Deserialize)]
struct StripeCheckoutSession {
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StripePortalSession {
    url: String,
}
