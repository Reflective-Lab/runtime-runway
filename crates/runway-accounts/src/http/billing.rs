use axum::{
    Extension, Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use commerce_rails_stripe::{
    BillingPlan as CommerceBillingPlan, CommerceWebhookAction, SubscriptionProjection,
};
use runway_auth::AuthContext;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Account, AccountsState,
    domain::{Org, Plan, Role},
    error::AccountError,
};

// --- Request / response types ---

#[derive(Debug, Deserialize)]
pub struct CheckoutRequest {
    #[serde(alias = "price_id")]
    pub price_ref: String,
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
    pub commerce_configured: bool,
}

// --- Handlers ---

/// GET /v1/billing/summary
pub async fn billing_summary(
    State(state): State<AccountsState>,
    Extension(ctx): Extension<AuthContext>,
) -> Result<Json<BillingSummary>, AccountError> {
    let account = state.store.get_account(ctx.uid()).await?;
    let org = match account.and_then(|account| account.org_id) {
        Some(id) => state.store.get_org(&id).await?,
        None => None,
    };

    Ok(Json(match org {
        Some(org) => BillingSummary {
            org_id: Some(org.org_id),
            plan: org.plan.as_str().to_string(),
            subscription_status: org.subscription_status,
            current_period_end: org.current_period_end,
            apps: org.apps,
            commerce_configured: state.commerce.is_billing_configured(),
        },
        None => BillingSummary {
            org_id: None,
            plan: Plan::Free.as_str().to_string(),
            subscription_status: "inactive".to_string(),
            current_period_end: None,
            apps: Vec::new(),
            commerce_configured: state.commerce.is_billing_configured(),
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
        .ok_or_else(|| AccountError::Internal("account not provisioned".to_string()))?;

    let customer_ref = state
        .commerce
        .ensure_customer(ctx.uid(), ctx.claims.email.as_deref())
        .await?;

    // Store the provider customer reference on the Runway org mirror so webhook
    // ingress can resolve the identity container without calling Commerce Rails.
    if let Some(org_id) = &account.org_id
        && let Ok(Some(mut org)) = state.store.get_org(org_id).await
        && org.billing_customer_ref.as_deref() != Some(&customer_ref)
    {
        org.billing_customer_ref = Some(customer_ref.clone());
        org.touch();
        let _ = state.store.upsert_org(&org).await;
    }

    let app_url = &state.config.app_url;
    let success_url = req
        .success_url
        .unwrap_or_else(|| format!("{app_url}?checkout=success"));
    let cancel_url = req
        .cancel_url
        .unwrap_or_else(|| format!("{app_url}?checkout=canceled"));

    let url = state
        .commerce
        .create_checkout_session(
            &customer_ref,
            &req.price_ref,
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
        .ok_or_else(|| AccountError::Internal("account not provisioned".to_string()))?;

    let customer_ref = match account.org_id.as_ref() {
        Some(org_id) => state
            .store
            .get_org(org_id)
            .await?
            .and_then(|org| org.billing_customer_ref),
        None => None,
    }
    .ok_or_else(|| {
        AccountError::Commerce("no billing account found - complete a checkout first".to_string())
    })?;

    let app_url = &state.config.app_url;
    let return_url = req
        .return_url
        .unwrap_or_else(|| format!("{app_url}?portal=returned"));

    let url = state
        .commerce
        .create_portal_session(&customer_ref, &return_url)
        .await?;

    Ok(Json(json!({ "portal_url": url })))
}

/// POST /v1/billing/webhooks/stripe  (public; signed provider transport)
pub async fn stripe_webhook(
    State(state): State<AccountsState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let signature = headers
        .get("stripe-signature")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");

    if !state
        .commerce
        .verify_stripe_webhook_signature(&body, signature)
    {
        tracing::warn!("Stripe webhook signature invalid");
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "invalid signature" })),
        ));
    }

    let webhook = state.commerce.accept_stripe_webhook(&body).map_err(|e| {
        let status = if e.is_invalid_webhook_json() {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::BAD_GATEWAY
        };
        (status, Json(json!({ "error": e.to_string() })))
    })?;

    tracing::info!(
        event_type = webhook.event_type.as_str(),
        receipt_id = webhook.receipt.id.as_str(),
        "Stripe webhook accepted by Commerce Rails"
    );

    match webhook.action {
        CommerceWebhookAction::LinkCustomerRef {
            firebase_uid,
            customer_ref,
        } => {
            handle_customer_ref_linked(&state, &firebase_uid, &customer_ref).await;
        }
        CommerceWebhookAction::ApplySubscriptionProjection {
            customer_ref,
            projection,
        } => {
            handle_subscription_projection(&state, &customer_ref, projection).await;
        }
        CommerceWebhookAction::UpdateSubscriptionStatus {
            customer_ref,
            subscription_status,
        } => {
            handle_subscription_status(&state, &customer_ref, &subscription_status).await;
        }
        CommerceWebhookAction::Ignored => {}
    }

    Ok(Json(json!({ "received": true })))
}

// --- Webhook event handlers ---

async fn handle_customer_ref_linked(state: &AccountsState, uid: &str, customer_ref: &str) {
    let org = match resolve_or_create_org(state, uid, customer_ref).await {
        Ok(org) => org,
        Err(e) => {
            tracing::error!(uid, "checkout: failed to resolve org: {e}");
            return;
        }
    };

    // The subscription will arrive in a follow-up subscription event; no plan update here.
    tracing::info!(
        uid,
        org_id = org.org_id.as_str(),
        customer_ref,
        "checkout completed, org mirror linked"
    );
}

async fn handle_subscription_projection(
    state: &AccountsState,
    customer_ref: &str,
    projection: SubscriptionProjection,
) {
    let Some(mut org) = find_org_by_customer(state, customer_ref).await else {
        tracing::warn!(customer_ref, "subscription update: org not found");
        return;
    };

    apply_subscription_projection(&mut org, projection);

    if let Err(e) = state.store.upsert_org(&org).await {
        tracing::error!(
            org_id = org.org_id.as_str(),
            "subscription update: failed to save org: {e}"
        );
        return;
    }

    mint_claims_for_org(state, &org);

    tracing::info!(
        org_id = org.org_id.as_str(),
        plan = org.plan.as_str(),
        status = org.subscription_status.as_str(),
        "subscription mirror updated"
    );
}

async fn handle_subscription_status(
    state: &AccountsState,
    customer_ref: &str,
    subscription_status: &str,
) {
    let Some(mut org) = find_org_by_customer(state, customer_ref).await else {
        return;
    };

    org.subscription_status = subscription_status.to_string();
    org.touch();

    if let Err(e) = state.store.upsert_org(&org).await {
        tracing::error!(
            org_id = org.org_id.as_str(),
            "payment_failed: failed to save org: {e}"
        );
    } else {
        tracing::info!(org_id = org.org_id.as_str(), "org mirror marked past_due");
    }
}

// --- Helpers ---

async fn find_org_by_customer(state: &AccountsState, customer_ref: &str) -> Option<Org> {
    match state
        .store
        .find_org_by_billing_customer_ref(customer_ref)
        .await
    {
        Ok(org) => org,
        Err(e) => {
            tracing::error!(customer_ref, "find org by customer ref: {e}");
            None
        }
    }
}

/// Find the org for a Firebase UID (creating one if needed) and link the customer ref.
async fn resolve_or_create_org(
    state: &AccountsState,
    uid: &str,
    customer_ref: &str,
) -> anyhow::Result<Org> {
    let account = state.store.get_account(uid).await?;

    let mut org = match account.and_then(|account| account.org_id) {
        Some(org_id) => state
            .store
            .get_org(&org_id)
            .await?
            .unwrap_or_else(|| Org::new_personal(uid)),
        None => {
            // Provision account and org if checkout completed before /v1/accounts/me.
            let mut account = Account::new(uid);
            let org = Org::new_personal(uid);
            account.org_id = Some(org.org_id.clone());
            state.store.upsert_account(&account).await?;
            org
        }
    };

    org.billing_customer_ref = Some(customer_ref.to_string());
    org.touch();
    state.store.upsert_org(&org).await?;
    Ok(org)
}

fn apply_subscription_projection(org: &mut Org, projection: SubscriptionProjection) {
    org.plan = runway_plan(projection.plan);
    org.apps = projection.apps;
    org.subscription_status = projection.subscription_status;
    org.subscription_id = projection.subscription_ref;
    org.current_period_end = projection.current_period_end;
    org.touch();
}

fn runway_plan(plan: CommerceBillingPlan) -> Plan {
    match plan {
        CommerceBillingPlan::Free => Plan::Free,
        CommerceBillingPlan::Starter => Plan::Starter,
        CommerceBillingPlan::Team => Plan::Team,
        CommerceBillingPlan::Enterprise => Plan::Enterprise,
    }
}

fn mint_claims_for_org(state: &AccountsState, org: &Org) {
    state.claims.mint_in_background(
        org.billing_owner_uid.clone(),
        org.org_id.clone(),
        org.apps.clone(),
        Role::Admin.as_str().to_string(),
    );
}
