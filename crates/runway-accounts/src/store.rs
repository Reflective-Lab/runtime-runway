use std::sync::Arc;

use anyhow::Result;
use runway_storage::{Document, DocumentStore, Filter, Query, StorageKit};
use serde_json::Value;

use crate::domain::{Account, Org, OrgInvite, OrgMember};

fn from_doc<T: serde::de::DeserializeOwned>(doc: Document) -> Result<T> {
    let v = serde_json::to_value(doc.data)?;
    Ok(serde_json::from_value(v)?)
}

fn member_id(org_id: &str, uid: &str) -> String {
    format!("{org_id}_{uid}")
}

#[derive(Clone)]
pub struct AccountStore {
    docs: Arc<dyn DocumentStore>,
}

impl AccountStore {
    pub fn new(storage: Arc<StorageKit>) -> Self {
        Self {
            docs: Arc::clone(&storage.documents),
        }
    }

    pub async fn get_account(&self, uid: &str) -> Result<Option<Account>> {
        match self
            .docs
            .get("accounts", uid)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?
        {
            None => Ok(None),
            Some(doc) => Ok(Some(from_doc(doc)?)),
        }
    }

    pub async fn upsert_account(&self, account: &Account) -> Result<()> {
        let doc = Document::new(&account.uid, account)?;
        self.docs
            .put("accounts", doc)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn get_org(&self, org_id: &str) -> Result<Option<Org>> {
        match self
            .docs
            .get("orgs", org_id)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?
        {
            None => Ok(None),
            Some(doc) => Ok(Some(from_doc(doc)?)),
        }
    }

    pub async fn upsert_org(&self, org: &Org) -> Result<()> {
        let doc = Document::new(&org.org_id, org)?;
        self.docs
            .put("orgs", doc)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn find_org_by_billing_customer_ref(
        &self,
        customer_ref: &str,
    ) -> Result<Option<Org>> {
        let q = Query::new()
            .filter(Filter::Eq(
                "billing_customer_ref".into(),
                Value::String(customer_ref.to_string()),
            ))
            .limit(1);
        let docs = self
            .docs
            .query("orgs", q)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        match docs.into_iter().next() {
            None => Ok(None),
            Some(doc) => Ok(Some(from_doc(doc)?)),
        }
    }

    // --- Members ---

    pub async fn upsert_member(&self, member: &OrgMember) -> Result<()> {
        let doc = Document::new(&member.id, member)?;
        self.docs
            .put("orgMembers", doc)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn get_member(&self, org_id: &str, uid: &str) -> Result<Option<OrgMember>> {
        let id = member_id(org_id, uid);
        match self
            .docs
            .get("orgMembers", &id)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?
        {
            None => Ok(None),
            Some(doc) => Ok(Some(from_doc(doc)?)),
        }
    }

    pub async fn list_members(&self, org_id: &str) -> Result<Vec<OrgMember>> {
        let q = Query::new().filter(Filter::Eq(
            "org_id".into(),
            Value::String(org_id.to_string()),
        ));
        let docs = self
            .docs
            .query("orgMembers", q)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        docs.into_iter().map(from_doc).collect()
    }

    pub async fn remove_member(&self, org_id: &str, uid: &str) -> Result<()> {
        let id = member_id(org_id, uid);
        self.docs
            .delete("orgMembers", &id)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    // --- Invites ---

    pub async fn upsert_invite(&self, invite: &OrgInvite) -> Result<()> {
        let doc = Document::new(&invite.token, invite)?;
        self.docs
            .put("orgInvites", doc)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn get_invite(&self, token: &str) -> Result<Option<OrgInvite>> {
        match self
            .docs
            .get("orgInvites", token)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?
        {
            None => Ok(None),
            Some(doc) => Ok(Some(from_doc(doc)?)),
        }
    }

    pub async fn list_invites(&self, org_id: &str) -> Result<Vec<OrgInvite>> {
        let q = Query::new().filter(Filter::Eq(
            "org_id".into(),
            Value::String(org_id.to_string()),
        ));
        let docs = self
            .docs
            .query("orgInvites", q)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        docs.into_iter().map(from_doc).collect()
    }
}
