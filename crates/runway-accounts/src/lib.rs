pub mod claims;
pub mod config;
pub mod domain;
pub mod error;
mod http;
pub mod store;

pub use claims::ClaimsService;
pub use config::AccountsConfig;
pub use domain::{Account, Org, OrgInvite, OrgMember, Plan, Role};
pub use error::AccountError;
pub use store::AccountStore;

use std::sync::Arc;

use axum::{Router, routing};
use commerce_rails_stripe::CommerceRails;
use runway_storage::StorageKit;

#[derive(Clone)]
pub struct AccountsState {
    pub store: AccountStore,
    pub commerce: CommerceRails,
    pub claims: ClaimsService,
    pub config: AccountsConfig,
}

impl AccountsState {
    pub fn new(storage: Arc<StorageKit>, config: AccountsConfig) -> Self {
        // RP-HERMETIC-UNIT (Reflective QUALITY_BACKLOG.md →
        // QF-2026-06-02-05): production fan-out constructor — the same
        // default client is shared with CommerceRails and ClaimsService.
        // Both downstream constructors already take the client as a
        // parameter, so a future hermetic test for AccountsState can
        // call those constructors directly with a stub client without
        // going through AccountsState::new.
        #[allow(clippy::disallowed_methods)]
        let client = reqwest::Client::new();
        Self {
            store: AccountStore::new(storage),
            commerce: CommerceRails::new(client.clone(), config.commerce.clone()),
            claims: ClaimsService::new(client, config.local_dev),
            config,
        }
    }
}

/// Public routes — no auth required. Webhook HMAC-verified internally.
pub fn public_routes(state: AccountsState) -> Router {
    Router::new()
        .route(
            "/v1/billing/webhooks/stripe",
            routing::post(http::billing::stripe_webhook),
        )
        .with_state(state)
}

/// Protected routes — caller must supply a valid Firebase Bearer token.
/// Wire these behind your `AuthLayer` before merging into the main router.
pub fn protected_routes(state: AccountsState) -> Router {
    Router::new()
        .route("/v1/accounts/me", routing::get(http::accounts::get_me))
        .route("/v1/orgs/:org_id", routing::get(http::orgs::get_org))
        .route(
            "/v1/orgs/:org_id/members",
            routing::get(http::members::list_members),
        )
        .route(
            "/v1/orgs/:org_id/members/:uid",
            routing::delete(http::members::remove_member),
        )
        .route(
            "/v1/orgs/:org_id/invites",
            routing::get(http::invites::list_invites).post(http::invites::create_invite),
        )
        .route(
            "/v1/invites/:token/accept",
            routing::post(http::invites::accept_invite),
        )
        .route(
            "/v1/billing/summary",
            routing::get(http::billing::billing_summary),
        )
        .route(
            "/v1/billing/checkout",
            routing::post(http::billing::create_checkout),
        )
        .route(
            "/v1/billing/portal",
            routing::post(http::billing::create_portal),
        )
        .with_state(state)
}
