use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use runway_auth::AuthContext;
use serde::Serialize;

use crate::{AccountsState, domain::OrgMember, error::AccountError};

#[derive(Serialize)]
pub struct MemberResponse {
    pub uid: String,
    pub role: String,
    pub invited_by: Option<String>,
    pub joined_at: String,
}

impl From<OrgMember> for MemberResponse {
    fn from(m: OrgMember) -> Self {
        Self {
            uid: m.uid,
            role: m.role.as_str().to_string(),
            invited_by: m.invited_by,
            joined_at: m.joined_at.to_rfc3339(),
        }
    }
}

/// GET /v1/orgs/:org_id/members  (admin only)
pub async fn list_members(
    State(state): State<AccountsState>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<String>,
) -> Result<Json<Vec<MemberResponse>>, AccountError> {
    let org = state.store.get_org(&org_id).await?.ok_or(AccountError::NotFound)?;

    if org.billing_owner_uid != ctx.uid() && !ctx.is_admin() {
        return Err(AccountError::Forbidden);
    }

    let members: Vec<MemberResponse> = state
        .store
        .list_members(&org_id)
        .await?
        .into_iter()
        .map(MemberResponse::from)
        .collect();

    Ok(Json(members))
}

/// DELETE /v1/orgs/:org_id/members/:uid  (admin only, cannot remove billing owner)
pub async fn remove_member(
    State(state): State<AccountsState>,
    Extension(ctx): Extension<AuthContext>,
    Path((org_id, target_uid)): Path<(String, String)>,
) -> Result<StatusCode, AccountError> {
    let org = state.store.get_org(&org_id).await?.ok_or(AccountError::NotFound)?;

    if org.billing_owner_uid != ctx.uid() && !ctx.is_admin() {
        return Err(AccountError::Forbidden);
    }

    if target_uid == org.billing_owner_uid {
        return Err(AccountError::Internal("cannot remove the billing owner".into()));
    }

    state.store.remove_member(&org_id, &target_uid).await?;

    tracing::info!(
        org_id,
        removed_uid = target_uid,
        removed_by = ctx.uid(),
        "Member removed"
    );

    Ok(StatusCode::NO_CONTENT)
}
