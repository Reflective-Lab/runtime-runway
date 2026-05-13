use chrono::{DateTime, Utc};
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
