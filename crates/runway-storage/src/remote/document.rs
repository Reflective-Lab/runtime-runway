use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::{
    remote::GcpToken,
    traits::{
        Error, Result,
        document::{Document, DocumentStore, Filter, Query},
    },
};

/// Firestore document store using the REST v1 API.
/// Collection maps to a Firestore collection; document id maps to a document name.
/// Collection path: `projects/{project}/databases/(default)/documents/{collection}/{id}`
pub struct FirestoreDocumentStore {
    project_id: String,
    token: GcpToken,
    client: reqwest::Client,
}

impl FirestoreDocumentStore {
    pub fn new(project_id: String, token: GcpToken) -> Self {
        Self {
            project_id,
            token,
            client: reqwest::Client::new(),
        }
    }

    fn base_url(&self) -> String {
        format!(
            "https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents",
            self.project_id
        )
    }

    async fn bearer(&self) -> Result<String> {
        self.token
            .get()
            .await
            .map_err(|e| Error::Network(e.to_string()))
    }

    /// Convert a Firestore fields map to our flat HashMap<String, Value>.
    fn from_firestore_fields(fields: &Value) -> HashMap<String, Value> {
        let Some(obj) = fields.as_object() else {
            return HashMap::new();
        };
        obj.iter()
            .map(|(k, v)| (k.clone(), extract_firestore_value(v)))
            .collect()
    }

    fn to_firestore_fields(data: &HashMap<String, Value>) -> Value {
        let fields: serde_json::Map<String, Value> = data
            .iter()
            .map(|(k, v)| (k.clone(), to_firestore_value(v)))
            .collect();
        serde_json::json!({ "fields": fields })
    }
}

#[async_trait]
impl DocumentStore for FirestoreDocumentStore {
    async fn put(&self, collection: &str, doc: Document) -> Result<()> {
        let url = format!("{}/{}/{}", self.base_url(), collection, doc.id);
        let body = Self::to_firestore_fields(&doc.data);
        self.client
            .patch(&url)
            .bearer_auth(self.bearer().await?)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?
            .error_for_status()
            .map_err(|e| Error::Network(e.to_string()))?;
        Ok(())
    }

    async fn get(&self, collection: &str, id: &str) -> Result<Option<Document>> {
        let url = format!("{}/{}/{}", self.base_url(), collection, id);
        let resp = self
            .client
            .get(&url)
            .bearer_auth(self.bearer().await?)
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let body: Value = resp
            .error_for_status()
            .map_err(|e| Error::Network(e.to_string()))?
            .json()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;

        let data = Self::from_firestore_fields(&body["fields"]);
        let created_at = parse_firestore_ts(&body["createTime"]);
        let updated_at = parse_firestore_ts(&body["updateTime"]);

        Ok(Some(Document {
            id: id.to_string(),
            data,
            created_at,
            updated_at,
        }))
    }

    async fn delete(&self, collection: &str, id: &str) -> Result<()> {
        let url = format!("{}/{}/{}", self.base_url(), collection, id);
        self.client
            .delete(&url)
            .bearer_auth(self.bearer().await?)
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?
            .error_for_status()
            .map_err(|e| Error::Network(e.to_string()))?;
        Ok(())
    }

    async fn query(&self, collection: &str, q: Query) -> Result<Vec<Document>> {
        let url = format!(
            "https://firestore.googleapis.com/v1/projects/{}/databases/(default)/documents:runQuery",
            self.project_id
        );

        let mut filters = vec![serde_json::json!({
            "fieldFilter": {
                "field": { "fieldPath": "__name__" },
                "op": "GREATER_THAN_OR_EQUAL",
                "value": { "stringValue": format!("projects/{}/databases/(default)/documents/{}/", self.project_id, collection) }
            }
        })];

        if let Some(ts) = q.updated_after {
            filters.push(serde_json::json!({
                "fieldFilter": {
                    "field": { "fieldPath": "updated_at" },
                    "op": "GREATER_THAN",
                    "value": { "timestampValue": ts.to_rfc3339() }
                }
            }));
        }

        if let Some(Filter::Eq(field, value)) = &q.filter {
            filters.push(serde_json::json!({
                "fieldFilter": {
                    "field": { "fieldPath": field },
                    "op": "EQUAL",
                    "value": to_firestore_value(value)
                }
            }));
        }

        let composite = if filters.len() == 1 {
            filters.remove(0)
        } else {
            serde_json::json!({ "compositeFilter": { "op": "AND", "filters": filters } })
        };

        let mut body = serde_json::json!({
            "structuredQuery": {
                "from": [{ "collectionId": collection }],
                "where": composite
            }
        });

        if let Some(limit) = q.limit {
            body["structuredQuery"]["limit"] = serde_json::json!(limit);
        }

        let resp: Value = self
            .client
            .post(&url)
            .bearer_auth(self.bearer().await?)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?
            .error_for_status()
            .map_err(|e| Error::Network(e.to_string()))?
            .json()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;

        let mut docs = Vec::new();
        if let Some(results) = resp.as_array() {
            for result in results {
                if let Some(doc) = result.get("document") {
                    let name = doc["name"].as_str().unwrap_or("");
                    let id = name.rsplit('/').next().unwrap_or("").to_string();
                    let data = Self::from_firestore_fields(&doc["fields"]);
                    let created_at = parse_firestore_ts(&doc["createTime"]);
                    let updated_at = parse_firestore_ts(&doc["updateTime"]);
                    docs.push(Document {
                        id,
                        data,
                        created_at,
                        updated_at,
                    });
                }
            }
        }
        Ok(docs)
    }
}

fn to_firestore_value(v: &Value) -> Value {
    match v {
        Value::String(s) => serde_json::json!({ "stringValue": s }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                serde_json::json!({ "integerValue": i.to_string() })
            } else {
                serde_json::json!({ "doubleValue": n.as_f64().unwrap_or(0.0) })
            }
        }
        Value::Bool(b) => serde_json::json!({ "booleanValue": b }),
        Value::Null => serde_json::json!({ "nullValue": null }),
        Value::Array(arr) => serde_json::json!({
            "arrayValue": { "values": arr.iter().map(to_firestore_value).collect::<Vec<_>>() }
        }),
        Value::Object(map) => serde_json::json!({
            "mapValue": {
                "fields": map.iter().map(|(k, v)| (k.clone(), to_firestore_value(v))).collect::<serde_json::Map<_,_>>()
            }
        }),
    }
}

fn extract_firestore_value(v: &Value) -> Value {
    if let Some(s) = v.get("stringValue") {
        return s.clone();
    }
    if let Some(n) = v.get("integerValue") {
        return Value::Number(
            n.as_str()
                .and_then(|s| s.parse::<i64>().ok())
                .map(|i| i.into())
                .unwrap_or(0.into()),
        );
    }
    if let Some(n) = v.get("doubleValue") {
        return n.clone();
    }
    if let Some(b) = v.get("booleanValue") {
        return b.clone();
    }
    if v.get("nullValue").is_some() {
        return Value::Null;
    }
    if let Some(a) = v.get("arrayValue") {
        let vals = a["values"]
            .as_array()
            .map(|arr| arr.iter().map(extract_firestore_value).collect())
            .unwrap_or_default();
        return Value::Array(vals);
    }
    if let Some(m) = v.get("mapValue") {
        let fields: serde_json::Map<String, Value> = m["fields"]
            .as_object()
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| (k.clone(), extract_firestore_value(v)))
                    .collect()
            })
            .unwrap_or_default();
        return Value::Object(fields);
    }
    Value::Null
}

fn parse_firestore_ts(v: &Value) -> DateTime<Utc> {
    v.as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(Utc::now)
}
