use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Plan {
    #[default]
    Free,
    Starter,
    Team,
    Enterprise,
}

impl Plan {
    pub fn as_str(&self) -> &'static str {
        match self {
            Plan::Free => "free",
            Plan::Starter => "starter",
            Plan::Team => "team",
            Plan::Enterprise => "enterprise",
        }
    }

    /// App IDs granted by this plan. Expanded as Marquee app subscriptions are wired up.
    pub fn apps(&self) -> Vec<String> {
        match self {
            Plan::Free => vec![],
            Plan::Starter | Plan::Team | Plan::Enterprise => vec!["marquee".to_string()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub uid: String,
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub org_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Account {
    pub fn new(uid: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            uid: uid.into(),
            email: None,
            display_name: None,
            org_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Org {
    pub org_id: String,
    pub name: String,
    pub billing_owner_uid: String,
    pub plan: Plan,
    pub apps: Vec<String>,
    pub stripe_customer_id: Option<String>,
    pub subscription_status: String,
    pub subscription_id: Option<String>,
    pub current_period_end: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Org {
    pub fn new_personal(uid: impl Into<String>) -> Self {
        let uid = uid.into();
        let now = Utc::now();
        Self {
            org_id: uuid::Uuid::new_v4().to_string(),
            name: "Personal".to_string(),
            billing_owner_uid: uid,
            plan: Plan::Free,
            apps: vec![],
            stripe_customer_id: None,
            subscription_status: "inactive".to_string(),
            subscription_id: None,
            current_period_end: None,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    #[default]
    Member,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Member => "member",
        }
    }
}

/// A user's membership in an org. Document ID = `{org_id}_{uid}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgMember {
    pub id: String,
    pub org_id: String,
    pub uid: String,
    pub role: Role,
    pub invited_by: Option<String>,
    pub joined_at: DateTime<Utc>,
}

impl OrgMember {
    pub fn new_owner(org_id: impl Into<String>, uid: impl Into<String>) -> Self {
        let org_id = org_id.into();
        let uid = uid.into();
        Self {
            id: format!("{org_id}_{uid}"),
            org_id,
            uid,
            role: Role::Admin,
            invited_by: None,
            joined_at: Utc::now(),
        }
    }

    pub fn from_invite(
        org_id: impl Into<String>,
        uid: impl Into<String>,
        role: Role,
        invited_by: impl Into<String>,
    ) -> Self {
        let org_id = org_id.into();
        let uid = uid.into();
        Self {
            id: format!("{org_id}_{uid}"),
            org_id,
            uid,
            role,
            invited_by: Some(invited_by.into()),
            joined_at: Utc::now(),
        }
    }
}

/// A pending invite to join an org. Document ID = token (UUID).
/// The token is the only credential — any authenticated user who holds it may accept.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgInvite {
    pub token: String,
    pub org_id: String,
    pub invited_by_uid: String,
    pub email: Option<String>,
    pub role: Role,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub accepted_by_uid: Option<String>,
    pub accepted_at: Option<DateTime<Utc>>,
}

impl OrgInvite {
    pub fn new(
        org_id: impl Into<String>,
        invited_by_uid: impl Into<String>,
        email: Option<String>,
        role: Role,
    ) -> Self {
        let now = Utc::now();
        Self {
            token: uuid::Uuid::new_v4().to_string(),
            org_id: org_id.into(),
            invited_by_uid: invited_by_uid.into(),
            email,
            role,
            created_at: now,
            expires_at: now + Duration::days(7),
            accepted_by_uid: None,
            accepted_at: None,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.accepted_by_uid.is_none() && Utc::now() < self.expires_at
    }
}
