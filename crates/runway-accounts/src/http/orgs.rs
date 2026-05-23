use axum::{
    Extension, Json,
    extract::{Path, State},
};
use runway_auth::AuthContext;

use crate::{AccountsState, domain::Org, error::AccountError};

/// GET /v1/orgs/:org_id
pub async fn get_org(
    State(state): State<AccountsState>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<String>,
) -> Result<Json<Org>, AccountError> {
    let org = state
        .store
        .get_org(&org_id)
        .await?
        .ok_or(AccountError::NotFound)?;

    // Require caller to be the billing owner, or have a matching org_id claim.
    let caller_owns = org.billing_owner_uid == ctx.uid();
    let claim_matches = ctx.org_id().map(|id| id == org_id).unwrap_or(false);
    if !caller_owns && !claim_matches {
        return Err(AccountError::Forbidden);
    }

    Ok(Json(org))
}
