use axum::{
    Extension, Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use runway_auth::AuthContext;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Account, AccountsState,
    domain::{Org, Plan},
    error::AccountError,
    stripe::StripeSubscription,
};

// --- Request / response types ---

#[derive(Debug, Deserialize)]
pub struct CheckoutRequest {
    pub price_id: String,
    pub success_url: Option<String>,
    pub cancel_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PortalRequest {
    pub return_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BillingSummary {
    pub org_id: Option<String>,
    pub plan: String,
    pub subscription_status: String,
    pub current_period_end: Option<i64>,
    pub apps: Vec<String>,
    pub stripe_configured: bool,
}

// --- Handlers ---

/// GET /v1/billing/summary
pub async fn billing_summary(
    State(state): State<AccountsState>,
    Extension(ctx): Extension<AuthContext>,
) -> Result<Json<BillingSummary>, AccountError> {
    let account = state.store.get_account(ctx.uid()).await?;
    let org = match account.and_then(|a| a.org_id) {
        Some(id) => state.store.get_org(&id).await?,
        None => None,
    };

    Ok(Json(match org {
        Some(o) => BillingSummary {
            org_id: Some(o.org_id),
            plan: o.plan.as_str().to_string(),
            subscription_status: o.subscription_status,
            current_period_end: o.current_period_end,
            apps: o.apps,
            stripe_configured: state.stripe.is_configured(),
        },
        None => BillingSummary {
            org_id: None,
            plan: Plan::Free.as_str().to_string(),
            subscription_status: "inactive".to_string(),
            current_period_end: None,
            apps: vec![],
            stripe_configured: state.stripe.is_configured(),
        },
    }))
}

/// POST /v1/billing/checkout
pub async fn create_checkout(
    State(state): State<AccountsState>,
    Extension(ctx): Extension<AuthContext>,
    Json(req): Json<CheckoutRequest>,
) -> Result<Json<Value>, AccountError> {
    let account = state
        .store
        .get_account(ctx.uid())
        .await?
        .ok_or_else(|| AccountError::Internal("account not provisioned".into()))?;

    let customer_id = state
        .stripe
        .ensure_customer(ctx.uid(), ctx.claims.email.as_deref())
        .await?;

    // Store customer ID on the org so webhooks can look it up.
    if let Some(org_id) = &account.org_id {
        if let Ok(Some(mut org)) = state.store.get_org(org_id).await {
            if org.stripe_customer_id.as_deref() != Some(&customer_id) {
                org.stripe_customer_id = Some(customer_id.clone());
                org.touch();
                let _ = state.store.upsert_org(&org).await;
            }
        }
    }

    let app_url = std::env::var("APP_URL").unwrap_or_else(|_| "https://apps.reflective.se".into());
    let success_url = req
        .success_url
        .unwrap_or_else(|| format!("{app_url}?checkout=success"));
    let cancel_url = req
        .cancel_url
        .unwrap_or_else(|| format!("{app_url}?checkout=canceled"));

    let url = state
        .stripe
        .create_checkout_session(
            &customer_id,
            &req.price_id,
            "subscription",
            &success_url,
            &cancel_url,
            ctx.uid(),
        )
        .await?;

    Ok(Json(json!({ "checkout_url": url })))
}

/// POST /v1/billing/portal
pub async fn create_portal(
    State(state): State<AccountsState>,
    Extension(ctx): Extension<AuthContext>,
    Json(req): Json<PortalRequest>,
) -> Result<Json<Value>, AccountError> {
    if !ctx.is_admin() {
        return Err(AccountError::Forbidden);
    }

    let account = state
        .store
        .get_account(ctx.uid())
        .await?
        .ok_or_else(|| AccountError::Internal("account not provisioned".into()))?;

    let customer_id = match account.org_id.as_ref() {
        Some(org_id) => state
            .store
            .get_org(org_id)
            .await?
            .and_then(|o| o.stripe_customer_id),
        None => None,
    }
    .ok_or_else(|| AccountError::Stripe("no billing account found — complete a checkout first".into()))?;

    let app_url = std::env::var("APP_URL").unwrap_or_else(|_| "https://apps.reflective.se".into());
    let return_url = req
        .return_url
        .unwrap_or_else(|| format!("{app_url}?portal=returned"));

    let url = state
        .stripe
        .create_portal_session(&customer_id, &return_url)
        .await?;

    Ok(Json(json!({ "portal_url": url })))
}

/// POST /v1/billing/webhooks/stripe  (public — HMAC-verified internally)
pub async fn stripe_webhook(
    State(state): State<AccountsState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let webhook_secret = std::env::var("STRIPE_WEBHOOK_SECRET").unwrap_or_default();

    if !webhook_secret.is_empty() {
        let sig = headers
            .get("stripe-signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !state.stripe.verify_signature(&body, sig, &webhook_secret) {
            tracing::warn!("Stripe webhook signature invalid");
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "invalid signature" })),
            ));
        }
    }

    let event: Value = serde_json::from_slice(&body).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("invalid JSON: {e}") })),
        )
    })?;

    let event_type = event["type"].as_str().unwrap_or("");
    tracing::info!(event_type, "Stripe webhook received");

    match event_type {
        "checkout.session.completed" => {
            handle_checkout_completed(&state, &event).await;
        }
        "customer.subscription.created" | "customer.subscription.updated" => {
            handle_subscription_updated(&state, &event["data"]["object"]).await;
        }
        "customer.subscription.deleted" => {
            handle_subscription_deleted(&state, &event["data"]["object"]).await;
        }
        "invoice.payment_failed" => {
            handle_payment_failed(&state, &event["data"]["object"]).await;
        }
        _ => {}
    }

    Ok(Json(json!({ "received": true })))
}

// --- Webhook event handlers ---

async fn handle_checkout_completed(state: &AccountsState, event: &Value) {
    let session = &event["data"]["object"];
    let Some(uid) = session["client_reference_id"].as_str() else {
        tracing::warn!("checkout.session.completed missing client_reference_id");
        return;
    };
    let Some(customer_id) = session["customer"].as_str() else {
        return;
    };

    let org = match resolve_or_create_org(state, uid, customer_id).await {
        Ok(o) => o,
        Err(e) => {
            tracing::error!(uid, "checkout: failed to resolve org: {e}");
            return;
        }
    };

    // The subscription will arrive in a follow-up subscription.created event — no plan update here.
    tracing::info!(uid, org_id = org.org_id, customer_id, "checkout completed, org linked");
}

async fn handle_subscription_updated(state: &AccountsState, subscription: &Value) {
    let Some(customer_id) = subscription["customer"].as_str() else {
        return;
    };
    let Some(sub_id) = subscription["id"].as_str() else {
        return;
    };
    let status = subscription["status"].as_str().unwrap_or("unknown");
    let period_end = subscription["current_period_end"].as_i64();

    let price_ids: Vec<&str> = subscription["items"]["data"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["price"]["id"].as_str())
                .collect()
        })
        .unwrap_or_default();

    let plan = plan_from_price_ids(&price_ids);

    let Some(mut org) = find_org_by_customer(state, customer_id).await else {
        tracing::warn!(customer_id, "subscription update: org not found");
        return;
    };

    org.plan = plan;
    org.apps = org.plan.apps();
    org.subscription_status = status.to_string();
    org.subscription_id = Some(sub_id.to_string());
    org.current_period_end = period_end;
    org.touch();

    if let Err(e) = state.store.upsert_org(&org).await {
        tracing::error!(org_id = org.org_id, "subscription update: failed to save org: {e}");
        return;
    }

    state.claims.mint_in_background(
        org.billing_owner_uid.clone(),
        org.org_id.clone(),
        org.apps.clone(),
        crate::domain::Role::Admin.as_str().to_string(),
    );

    tracing::info!(
        org_id = org.org_id,
        plan = org.plan.as_str(),
        status,
        "subscription updated"
    );
}

async fn handle_subscription_deleted(state: &AccountsState, subscription: &Value) {
    let Some(customer_id) = subscription["customer"].as_str() else {
        return;
    };
    let Some(mut org) = find_org_by_customer(state, customer_id).await else {
        return;
    };

    org.plan = Plan::Free;
    org.apps = vec![];
    org.subscription_status = "canceled".to_string();
    org.subscription_id = None;
    org.current_period_end = None;
    org.touch();

    if let Err(e) = state.store.upsert_org(&org).await {
        tracing::error!(org_id = org.org_id, "subscription delete: failed to save org: {e}");
        return;
    }

    state.claims.mint_in_background(
        org.billing_owner_uid.clone(),
        org.org_id.clone(),
        vec![],
        crate::domain::Role::Admin.as_str().to_string(),
    );

    tracing::info!(org_id = org.org_id, "subscription canceled");
}

async fn handle_payment_failed(state: &AccountsState, invoice: &Value) {
    let Some(customer_id) = invoice["customer"].as_str() else {
        return;
    };
    let Some(mut org) = find_org_by_customer(state, customer_id).await else {
        return;
    };

    org.subscription_status = "past_due".to_string();
    org.touch();

    if let Err(e) = state.store.upsert_org(&org).await {
        tracing::error!(org_id = org.org_id, "payment_failed: failed to save org: {e}");
    } else {
        tracing::info!(org_id = org.org_id, "org marked past_due");
    }
}

// --- Helpers ---

async fn find_org_by_customer(state: &AccountsState, customer_id: &str) -> Option<Org> {
    match state.store.find_org_by_stripe_customer(customer_id).await {
        Ok(o) => o,
        Err(e) => {
            tracing::error!(customer_id, "find org by customer: {e}");
            None
        }
    }
}

/// Find the org for a Firebase UID (creating one if needed) and link the Stripe customer ID.
async fn resolve_or_create_org(
    state: &AccountsState,
    uid: &str,
    customer_id: &str,
) -> anyhow::Result<Org> {
    let account = state.store.get_account(uid).await?;

    let mut org = match account.and_then(|a| a.org_id) {
        Some(org_id) => state
            .store
            .get_org(&org_id)
            .await?
            .unwrap_or_else(|| Org::new_personal(uid)),
        None => {
            // Provision account + org if the user completed checkout without calling /v1/accounts/me first
            let mut acc = Account::new(uid);
            let org = Org::new_personal(uid);
            acc.org_id = Some(org.org_id.clone());
            state.store.upsert_account(&acc).await?;
            org
        }
    };

    org.stripe_customer_id = Some(customer_id.to_string());
    org.touch();
    state.store.upsert_org(&org).await?;
    Ok(org)
}

/// Map Stripe price IDs to a Plan by comparing against env-configured price IDs.
fn plan_from_price_ids(price_ids: &[&str]) -> Plan {
    let team = std::env::var("STRIPE_PRICE_TEAM_MONTHLY").unwrap_or_default();
    let starter = std::env::var("STRIPE_PRICE_STARTER_MONTHLY").unwrap_or_default();

    if !team.is_empty() && price_ids.contains(&team.as_str()) {
        return Plan::Team;
    }
    if !starter.is_empty() && price_ids.contains(&starter.as_str()) {
        return Plan::Starter;
    }
    Plan::Free
}

/// Silence unused-import warning — `StripeSubscription` is only used via serde in webhook JSON.
const _: () = {
    let _ = std::mem::size_of::<StripeSubscription>();
};
