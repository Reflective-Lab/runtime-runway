use std::sync::Arc;

use anyhow::Result;
use runway_storage::{Document, DocumentStore, Filter, Query, StorageKit};
use serde_json::Value;

use crate::domain::{Account, Org};

fn from_doc<T: serde::de::DeserializeOwned>(doc: Document) -> Result<T> {
    let v = serde_json::to_value(doc.data)?;
    Ok(serde_json::from_value(v)?)
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
        match self.docs.get("accounts", uid).await.map_err(|e| anyhow::anyhow!("{e}"))? {
            None => Ok(None),
            Some(doc) => Ok(Some(from_doc(doc)?)),
        }
    }

    pub async fn upsert_account(&self, account: &Account) -> Result<()> {
        let doc = Document::new(&account.uid, account)?;
        self.docs.put("accounts", doc).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn get_org(&self, org_id: &str) -> Result<Option<Org>> {
        match self.docs.get("orgs", org_id).await.map_err(|e| anyhow::anyhow!("{e}"))? {
            None => Ok(None),
            Some(doc) => Ok(Some(from_doc(doc)?)),
        }
    }

    pub async fn upsert_org(&self, org: &Org) -> Result<()> {
        let doc = Document::new(&org.org_id, org)?;
        self.docs.put("orgs", doc).await.map_err(|e| anyhow::anyhow!("{e}"))
    }

    pub async fn find_org_by_stripe_customer(&self, customer_id: &str) -> Result<Option<Org>> {
        let q = Query::new()
            .filter(Filter::Eq(
                "stripe_customer_id".into(),
                Value::String(customer_id.to_string()),
            ))
            .limit(1);
        let docs = self.docs.query("orgs", q).await.map_err(|e| anyhow::anyhow!("{e}"))?;
        match docs.into_iter().next() {
            None => Ok(None),
            Some(doc) => Ok(Some(from_doc(doc)?)),
        }
    }
}
