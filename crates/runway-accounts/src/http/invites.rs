use axum::{
    Extension, Json,
    extract::{Path, State},
};
use chrono::Utc;
use runway_auth::AuthContext;
use serde::{Deserialize, Serialize};

use crate::{
    AccountsState,
    domain::{OrgInvite, OrgMember, Role},
    error::AccountError,
};

#[derive(Deserialize)]
pub struct CreateInviteRequest {
    pub email: Option<String>,
    #[serde(default)]
    pub role: Role,
}

#[derive(Serialize)]
pub struct InviteResponse {
    pub token: String,
    pub org_id: String,
    pub email: Option<String>,
    pub role: String,
    pub expires_at: String,
}

impl From<&OrgInvite> for InviteResponse {
    fn from(i: &OrgInvite) -> Self {
        Self {
            token: i.token.clone(),
            org_id: i.org_id.clone(),
            email: i.email.clone(),
            role: i.role.as_str().to_string(),
            expires_at: i.expires_at.to_rfc3339(),
        }
    }
}

/// POST /v1/orgs/:org_id/invites  (admin only)
pub async fn create_invite(
    State(state): State<AccountsState>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<String>,
    Json(req): Json<CreateInviteRequest>,
) -> Result<Json<InviteResponse>, AccountError> {
    let org = state
        .store
        .get_org(&org_id)
        .await?
        .ok_or(AccountError::NotFound)?;

    if org.billing_owner_uid != ctx.uid() && !ctx.is_admin() {
        return Err(AccountError::Forbidden);
    }

    let invite = OrgInvite::new(&org_id, ctx.uid(), req.email, req.role);
    state.store.upsert_invite(&invite).await?;

    tracing::info!(
        org_id,
        token = invite.token,
        invited_by = ctx.uid(),
        "Org invite created"
    );

    Ok(Json(InviteResponse::from(&invite)))
}

/// GET /v1/orgs/:org_id/invites  (admin only)
pub async fn list_invites(
    State(state): State<AccountsState>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<String>,
) -> Result<Json<Vec<InviteResponse>>, AccountError> {
    let org = state
        .store
        .get_org(&org_id)
        .await?
        .ok_or(AccountError::NotFound)?;

    if org.billing_owner_uid != ctx.uid() && !ctx.is_admin() {
        return Err(AccountError::Forbidden);
    }

    let invites: Vec<InviteResponse> = state
        .store
        .list_invites(&org_id)
        .await?
        .iter()
        .map(InviteResponse::from)
        .collect();

    Ok(Json(invites))
}

/// POST /v1/invites/:token/accept  (any authenticated user)
pub async fn accept_invite(
    State(state): State<AccountsState>,
    Extension(ctx): Extension<AuthContext>,
    Path(token): Path<String>,
) -> Result<Json<serde_json::Value>, AccountError> {
    let mut invite = state
        .store
        .get_invite(&token)
        .await?
        .ok_or(AccountError::NotFound)?;

    if !invite.is_valid() {
        return Err(AccountError::Internal(
            "invite expired or already used".into(),
        ));
    }

    // Idempotent: if the user is already a member, return success.
    if let Some(existing) = state.store.get_member(&invite.org_id, ctx.uid()).await? {
        return Ok(Json(serde_json::json!({
            "org_id": existing.org_id,
            "role": existing.role.as_str(),
        })));
    }

    let member = OrgMember::from_invite(
        &invite.org_id,
        ctx.uid(),
        invite.role.clone(),
        &invite.invited_by_uid,
    );
    state.store.upsert_member(&member).await?;

    // Mark invite as used.
    invite.accepted_by_uid = Some(ctx.uid().to_string());
    invite.accepted_at = Some(Utc::now());
    state.store.upsert_invite(&invite).await?;

    // Update account to point at this org (if not already in one).
    let account = state.store.get_account(ctx.uid()).await?;
    let needs_org = account.as_ref().map(|a| a.org_id.is_none()).unwrap_or(true);
    if needs_org {
        let mut acc = account.unwrap_or_else(|| crate::domain::Account::new(ctx.uid()));
        acc.org_id = Some(invite.org_id.clone());
        acc.touch();
        state.store.upsert_account(&acc).await?;
    }

    // Get org apps so we can mint complete claims.
    let apps = state
        .store
        .get_org(&invite.org_id)
        .await?
        .map(|o| o.apps)
        .unwrap_or_default();

    state.claims.mint_in_background(
        ctx.uid().to_string(),
        invite.org_id.clone(),
        apps,
        member.role.as_str().to_string(),
    );

    tracing::info!(
        uid = ctx.uid(),
        org_id = invite.org_id,
        role = member.role.as_str(),
        "Invite accepted"
    );

    Ok(Json(serde_json::json!({
        "org_id": invite.org_id,
        "role": member.role.as_str(),
    })))
}
