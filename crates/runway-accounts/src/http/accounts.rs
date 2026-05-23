use axum::{Extension, Json, extract::State};
use runway_auth::AuthContext;
use serde::Serialize;

use crate::{
    AccountsState,
    domain::{Account, Org, OrgMember, Role},
    error::AccountError,
};

#[derive(Serialize)]
pub struct MeResponse {
    pub account: Account,
    pub org: Option<Org>,
}

/// GET /v1/accounts/me
/// Returns the caller's account and org. Provisions both on first access.
pub async fn get_me(
    State(state): State<AccountsState>,
    Extension(ctx): Extension<AuthContext>,
) -> Result<Json<MeResponse>, AccountError> {
    let uid = ctx.uid();

    let account = match state.store.get_account(uid).await? {
        Some(a) => a,
        None => provision(uid, ctx.claims.email.as_deref(), &state).await?,
    };

    let org = match &account.org_id {
        Some(id) => state.store.get_org(id).await?,
        None => None,
    };

    Ok(Json(MeResponse { account, org }))
}

/// Create account + personal org for a user logging in for the first time.
async fn provision(
    uid: &str,
    email: Option<&str>,
    state: &AccountsState,
) -> Result<Account, AccountError> {
    let org = Org::new_personal(uid);
    state.store.upsert_org(&org).await?;

    // Billing owner is always admin.
    let member = OrgMember::new_owner(&org.org_id, uid);
    state.store.upsert_member(&member).await?;

    let mut account = Account::new(uid);
    account.email = email.map(str::to_string);
    account.org_id = Some(org.org_id.clone());
    state.store.upsert_account(&account).await?;

    state.claims.mint_in_background(
        uid.to_string(),
        org.org_id,
        vec![],
        Role::Admin.as_str().to_string(),
    );

    Ok(account)
}
