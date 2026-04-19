// Copyright 2024-2026 Reflective Labs

//! Proto <-> Rust type conversions for the KernelService.
//!
//! Converts between generated protobuf types and the native Rust types
//! from `converge-core::kernel_boundary` and `converge-llm`.

use std::collections::HashMap;

use crate::kernel::{
    self as kernel_types, ContextFact, ContractResult, KernelContext, KernelIntent, KernelPolicy,
    KernelTraceLink, ProposalKind, ProposalRecallMetadata, RecallImpact, Replayability,
    ReplayabilityDowngradeReason,
};

use super::proto;

// ============================================================================
// Proto -> Rust conversions (inbound)
// ============================================================================

impl From<proto::KernelIntent> for KernelIntent {
    fn from(p: proto::KernelIntent) -> Self {
        Self {
            task: p.task,
            criteria: p.criteria,
            max_tokens: p.max_tokens as usize,
        }
    }
}

impl From<proto::KernelContext> for KernelContext {
    fn from(p: proto::KernelContext) -> Self {
        let state: HashMap<String, serde_json::Value> = p
            .state
            .into_iter()
            .map(|(k, v)| (k, prost_value_to_json(v)))
            .collect();

        let facts = p.facts.into_iter().map(ContextFact::from).collect();

        Self {
            state,
            facts,
            tenant_id: p.tenant_id,
        }
    }
}

impl From<proto::ContextFact> for ContextFact {
    fn from(p: proto::ContextFact) -> Self {
        Self {
            key: p.key,
            id: p.id,
            content: p.content,
        }
    }
}

impl From<proto::KernelPolicy> for KernelPolicy {
    fn from(p: proto::KernelPolicy) -> Self {
        Self {
            adapter_id: p.adapter_id,
            recall_enabled: p.recall_enabled,
            recall_max_candidates: p.recall_max_candidates as usize,
            recall_min_score: p.recall_min_score,
            seed: p.seed,
            requires_human: p.requires_human,
            required_truths: p.required_truths,
        }
    }
}

impl From<proto::GenerationParams> for crate::inference::GenerationParams {
    fn from(p: proto::GenerationParams) -> Self {
        Self {
            max_new_tokens: p.max_new_tokens as usize,
            temperature: p.temperature,
            top_p: p.top_p,
            top_k: p.top_k as usize,
            repetition_penalty: p.repetition_penalty,
            stop_sequences: p.stop_sequences,
            seed: p.seed,
        }
    }
}

// ============================================================================
// Rust -> Proto conversions (outbound)
// ============================================================================

impl From<kernel_types::KernelProposal> for proto::KernelProposal {
    fn from(r: kernel_types::KernelProposal) -> Self {
        Self {
            id: r.id,
            kind: rust_proposal_kind_to_proto(r.kind).into(),
            payload: r.payload,
            structured_payload: r.structured_payload.map(|v| json_to_prost_struct(&v)),
            trace_link: Some(r.trace_link.into()),
            contract_results: r.contract_results.into_iter().map(Into::into).collect(),
            requires_human: r.requires_human,
            confidence: r.confidence,
        }
    }
}

impl From<KernelTraceLink> for proto::KernelTraceLink {
    fn from(r: KernelTraceLink) -> Self {
        Self {
            trace_hash: r.trace_hash,
            prompt_version: r.prompt_version,
            envelope_hash: r.envelope_hash,
            adapter_id: r.adapter_id,
            recall_metadata: r.recall_metadata.map(Into::into),
            replayability: rust_replayability_to_proto(r.replayability).into(),
            replayability_downgrade_reason: r
                .replayability_downgrade_reason
                .map(|r| rust_downgrade_reason_to_proto(r).into()),
        }
    }
}

impl From<ContractResult> for proto::ContractResult {
    fn from(r: ContractResult) -> Self {
        Self {
            name: r.name,
            passed: r.passed,
            failure_reason: r.failure_reason,
        }
    }
}

impl From<ProposalRecallMetadata> for proto::ProposalRecallMetadata {
    fn from(r: ProposalRecallMetadata) -> Self {
        Self {
            candidates_retrieved: r.candidates_retrieved as u64,
            candidates_used: r.candidates_used as u64,
            corpus_fingerprint: r.corpus_fingerprint,
            record_ids: r.record_ids,
            query_hash: r.query_hash,
            embedding_hash: r.embedding_hash,
            impact: rust_recall_impact_to_proto(r.impact).into(),
            embedder_deterministic: r.embedder_deterministic,
            corpus_content_addressed: r.corpus_content_addressed,
        }
    }
}

// ============================================================================
// Enum conversions
// ============================================================================

fn rust_proposal_kind_to_proto(kind: ProposalKind) -> proto::ProposalKind {
    match kind {
        ProposalKind::Claims => proto::ProposalKind::Claims,
        ProposalKind::Plan => proto::ProposalKind::Plan,
        ProposalKind::Classification => proto::ProposalKind::Classification,
        ProposalKind::Evaluation => proto::ProposalKind::Evaluation,
        ProposalKind::DraftDocument => proto::ProposalKind::DraftDocument,
        ProposalKind::Reasoning => proto::ProposalKind::Reasoning,
    }
}

fn rust_replayability_to_proto(r: Replayability) -> proto::Replayability {
    match r {
        Replayability::Deterministic => proto::Replayability::Deterministic,
        Replayability::BestEffort => proto::Replayability::BestEffort,
        Replayability::None => proto::Replayability::None,
    }
}

fn rust_downgrade_reason_to_proto(
    r: ReplayabilityDowngradeReason,
) -> proto::ReplayabilityDowngradeReason {
    match r {
        ReplayabilityDowngradeReason::RecallEmbedderNotDeterministic => {
            proto::ReplayabilityDowngradeReason::RecallEmbedderNotDeterministic
        }
        ReplayabilityDowngradeReason::RecallCorpusNotContentAddressed => {
            proto::ReplayabilityDowngradeReason::RecallCorpusNotContentAddressed
        }
        ReplayabilityDowngradeReason::RemoteBackendUsed => {
            proto::ReplayabilityDowngradeReason::RemoteBackendUsed
        }
        ReplayabilityDowngradeReason::NoSeedProvided => {
            proto::ReplayabilityDowngradeReason::NoSeedProvided
        }
        ReplayabilityDowngradeReason::MultipleReasons => {
            proto::ReplayabilityDowngradeReason::MultipleReasons
        }
    }
}

fn rust_recall_impact_to_proto(impact: RecallImpact) -> proto::RecallImpact {
    match impact {
        RecallImpact::None => proto::RecallImpact::None,
        RecallImpact::Unknown => proto::RecallImpact::Unknown,
        RecallImpact::ReducedIterations => proto::RecallImpact::ReducedIterations,
        RecallImpact::ReducedValidationFailures => proto::RecallImpact::ReducedValidationFailures,
        RecallImpact::ProvidedContext => proto::RecallImpact::ProvidedContext,
    }
}

// ============================================================================
// Prost Value <-> serde_json::Value helpers
// ============================================================================

/// Convert a prost `Value` to a `serde_json::Value`.
pub fn prost_value_to_json(value: prost_types::Value) -> serde_json::Value {
    match value.kind {
        Some(prost_types::value::Kind::NullValue(_)) => serde_json::Value::Null,
        Some(prost_types::value::Kind::NumberValue(n)) => serde_json::Value::Number(
            serde_json::Number::from_f64(n).unwrap_or_else(|| serde_json::Number::from(0)),
        ),
        Some(prost_types::value::Kind::StringValue(s)) => serde_json::Value::String(s),
        Some(prost_types::value::Kind::BoolValue(b)) => serde_json::Value::Bool(b),
        Some(prost_types::value::Kind::StructValue(s)) => prost_struct_to_json(&s),
        Some(prost_types::value::Kind::ListValue(l)) => {
            serde_json::Value::Array(l.values.into_iter().map(prost_value_to_json).collect())
        }
        None => serde_json::Value::Null,
    }
}

/// Convert a prost `Struct` to a `serde_json::Value::Object`.
pub fn prost_struct_to_json(s: &prost_types::Struct) -> serde_json::Value {
    let map: serde_json::Map<String, serde_json::Value> = s
        .fields
        .iter()
        .map(|(k, v)| (k.clone(), prost_value_to_json(v.clone())))
        .collect();
    serde_json::Value::Object(map)
}

/// Convert a `serde_json::Value` to a prost `Struct`.
pub fn json_to_prost_struct(value: &serde_json::Value) -> prost_types::Struct {
    match value {
        serde_json::Value::Object(map) => prost_types::Struct {
            fields: map
                .iter()
                .map(|(k, v)| (k.clone(), json_to_prost_value(v)))
                .collect(),
        },
        _ => prost_types::Struct {
            fields: std::collections::BTreeMap::new(),
        },
    }
}

/// Convert a `serde_json::Value` to a prost `Value`.
pub fn json_to_prost_value(value: &serde_json::Value) -> prost_types::Value {
    let kind = match value {
        serde_json::Value::Null => prost_types::value::Kind::NullValue(0),
        serde_json::Value::Bool(b) => prost_types::value::Kind::BoolValue(*b),
        serde_json::Value::Number(n) => {
            prost_types::value::Kind::NumberValue(n.as_f64().unwrap_or(0.0))
        }
        serde_json::Value::String(s) => prost_types::value::Kind::StringValue(s.clone()),
        serde_json::Value::Array(arr) => {
            prost_types::value::Kind::ListValue(prost_types::ListValue {
                values: arr.iter().map(json_to_prost_value).collect(),
            })
        }
        serde_json::Value::Object(map) => {
            prost_types::value::Kind::StructValue(prost_types::Struct {
                fields: map
                    .iter()
                    .map(|(k, v)| (k.clone(), json_to_prost_value(v)))
                    .collect(),
            })
        }
    };
    prost_types::Value { kind: Some(kind) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kernel_intent_roundtrip() {
        let proto_intent = proto::KernelIntent {
            task: "analyze_metrics".to_string(),
            criteria: vec!["find anomalies".to_string()],
            max_tokens: 1024,
        };

        let rust_intent: KernelIntent = proto_intent.into();
        assert_eq!(rust_intent.task, "analyze_metrics");
        assert_eq!(rust_intent.criteria, vec!["find anomalies"]);
        assert_eq!(rust_intent.max_tokens, 1024);
    }

    #[test]
    fn test_kernel_policy_roundtrip() {
        let proto_policy = proto::KernelPolicy {
            adapter_id: Some("test-adapter".to_string()),
            recall_enabled: true,
            recall_max_candidates: 10,
            recall_min_score: 0.8,
            seed: Some(42),
            requires_human: true,
            required_truths: vec!["grounded-answering".to_string()],
        };

        let rust_policy: KernelPolicy = proto_policy.into();
        assert_eq!(rust_policy.adapter_id, Some("test-adapter".to_string()));
        assert!(rust_policy.recall_enabled);
        assert_eq!(rust_policy.recall_max_candidates, 10);
        assert!((rust_policy.recall_min_score - 0.8).abs() < f32::EPSILON);
        assert_eq!(rust_policy.seed, Some(42));
        assert!(rust_policy.requires_human);
    }

    #[test]
    fn test_json_prost_roundtrip() {
        let json = serde_json::json!({
            "key": "value",
            "number": 42.0,
            "nested": { "inner": true }
        });

        let prost_struct = json_to_prost_struct(&json);
        let back = prost_struct_to_json(&prost_struct);

        assert_eq!(json, back);
    }
}
