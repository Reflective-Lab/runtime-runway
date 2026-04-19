// Copyright 2024-2026 Reflective Labs
// SPDX-License-Identifier: MIT
// See LICENSE file in the project root for full license information.

//! Domain pack management for Converge.
//!
//! Domain packs are defined in `converge-domain` and loaded here for
//! composition into the runtime. This module:
//!
//! - Lists available packs
//! - Loads templates from packs
//! - Provides pack metadata
//!
//! # Architecture Note
//!
//! This module does NOT define business semantics. It only selects
//! which already-defined domain packs are available in this distribution.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// A seed fact for input to convergence runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedFact {
    pub id: String,
    pub content: String,
}

/// Suggestor wiring configuration within a pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWiring {
    pub id: String,
    pub requirements: Option<RequirementsConfig>,
}

/// Requirements configuration for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RequirementsConfig {
    Preset(String),
}

/// Compatibility requirements for a pack.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompatibilityRequirements {
    pub core: Option<String>,
    pub runtime_api: Option<String>,
}

/// Budget configuration for convergence runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    pub max_cycles: u32,
    pub max_facts: u32,
}

/// Pack configuration (template definition).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackConfig {
    pub name: String,
    pub version: String,
    pub description: String,
    pub spec: Option<String>,
    pub requires: CompatibilityRequirements,
    pub budget: BudgetConfig,
    pub agents: Vec<AgentWiring>,
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

/// Registry of pack templates.
pub struct TemplateRegistry {
    templates: HashMap<String, Arc<PackConfig>>,
}

impl TemplateRegistry {
    pub fn new() -> Self {
        Self {
            templates: HashMap::new(),
        }
    }

    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        // Register built-in templates
        for name in [
            "growth-strategy",
            "ask-converge",
            "patent-research",
            "linkedin-research",
            "release-readiness",
            "drafting-short",
            "novelty-search",
        ] {
            let info = pack_info(name);
            let agents = info
                .invariants
                .iter()
                .map(|id| AgentWiring {
                    id: id.clone(),
                    requirements: None,
                })
                .collect();
            registry.register(PackConfig {
                name: name.to_string(),
                version: info.version,
                description: info.description,
                spec: None,
                requires: CompatibilityRequirements::default(),
                budget: BudgetConfig {
                    max_cycles: 50,
                    max_facts: 256,
                },
                agents,
                metadata: HashMap::new(),
            });
        }
        registry
    }

    pub fn register(&mut self, config: PackConfig) {
        self.templates.insert(config.name.clone(), Arc::new(config));
    }

    pub fn get(&self, name: &str) -> Option<&Arc<PackConfig>> {
        self.templates.get(name)
    }

    pub fn contains(&self, name: &str) -> bool {
        self.templates.contains_key(name)
    }
}

/// Information about a domain pack.
pub struct PackInfo {
    pub name: String,
    pub description: String,
    pub version: String,
    pub templates: Vec<String>,
    pub invariants: Vec<String>,
}

/// Returns all available domain packs (compiled into this distribution).
pub fn available_packs() -> Vec<String> {
    let mut packs = vec![
        // Always available (core packs)
        "growth-strategy".to_string(),
        "ask-converge".to_string(),
        "patent-research".to_string(),
        "linkedin-research".to_string(),
        "release-readiness".to_string(),
        "drafting-short".to_string(),
        "novelty-search".to_string(),
    ];

    // Deduplicate
    packs.sort();
    packs.dedup();
    packs
}

/// Returns the default packs to enable.
pub fn default_packs() -> Vec<String> {
    vec![
        "growth-strategy".to_string(),
        "ask-converge".to_string(),
        "patent-research".to_string(),
        "linkedin-research".to_string(),
        "release-readiness".to_string(),
        "drafting-short".to_string(),
        "novelty-search".to_string(),
    ]
}

/// Get information about a specific pack.
pub fn pack_info(name: &str) -> PackInfo {
    match name {
        "growth-strategy" => PackInfo {
            name: "growth-strategy".to_string(),
            description: "Multi-agent growth strategy analysis with market signals, \
                         competitor analysis, strategy synthesis, and evaluation."
                .to_string(),
            version: "1.0.0".to_string(),
            templates: vec!["growth-strategy".to_string()],
            invariants: vec![
                "BrandSafetyInvariant".to_string(),
                "RequireMultipleStrategies".to_string(),
                "RequireStrategyEvaluations".to_string(),
            ],
        },
        "sdr-pipeline" => PackInfo {
            name: "sdr-pipeline".to_string(),
            description: "SDR/sales funnel automation with lead qualification, \
                         outreach sequencing, and meeting scheduling."
                .to_string(),
            version: "0.1.0".to_string(),
            templates: vec!["sdr-qualify".to_string(), "sdr-outreach".to_string()],
            invariants: vec![
                "LeadQualificationInvariant".to_string(),
                "OutreachComplianceInvariant".to_string(),
            ],
        },
        "ask-converge" => PackInfo {
            name: "ask-converge".to_string(),
            description: "Grounded Q&A with recall-only sources.".to_string(),
            version: "0.1.0".to_string(),
            templates: vec!["ask-converge".to_string()],
            invariants: vec![
                "GroundedAnswerInvariant".to_string(),
                "RecallNotEvidenceInvariant".to_string(),
            ],
        },
        "patent-research" => PackInfo {
            name: "patent-research".to_string(),
            description: "Governed patent research with evidence and approvals.".to_string(),
            version: "1.0.0".to_string(),
            templates: vec!["patent-research".to_string()],
            invariants: vec![
                "PatentEvidenceHasProvenanceInvariant".to_string(),
                "PaidActionRequiresApprovalInvariant".to_string(),
                "SubmissionRequiresEvidenceInvariant".to_string(),
            ],
        },
        "linkedin-research" => PackInfo {
            name: "linkedin-research".to_string(),
            description: "Governed LinkedIn research with evidence and approvals.".to_string(),
            version: "1.0.0".to_string(),
            templates: vec!["linkedin-research".to_string()],
            invariants: vec![
                "EvidenceRequiresProvenanceInvariant".to_string(),
                "NetworkPathRequiresVerificationInvariant".to_string(),
                "ApprovalRequiredForExternalActionInvariant".to_string(),
            ],
        },
        "release-readiness" => PackInfo {
            name: "release-readiness".to_string(),
            description: "Engineering dependency and release quality gates.".to_string(),
            version: "1.0.0".to_string(),
            templates: vec!["release-readiness".to_string()],
            invariants: vec![
                "RequireAllChecksComplete".to_string(),
                "RequireMinimumCoverage".to_string(),
                "RequireNoCriticalVulnerabilities".to_string(),
            ],
        },
        "drafting-short" => PackInfo {
            name: "drafting-short".to_string(),
            description: "Short drafting flow with Perplexity research and Anthropic drafting."
                .to_string(),
            version: "1.0.0".to_string(),
            templates: vec!["drafting-short".to_string()],
            invariants: vec![],
        },
        "novelty-search" => PackInfo {
            name: "novelty-search".to_string(),
            description: "Short novelty search flow for patent prior art.".to_string(),
            version: "1.0.0".to_string(),
            templates: vec!["novelty-search".to_string()],
            invariants: vec![
                "PatentEvidenceHasProvenanceInvariant".to_string(),
                "EvidenceCitationInvariant".to_string(),
            ],
        },
        _ => PackInfo {
            name: name.to_string(),
            description: "Unknown pack".to_string(),
            version: "0.0.0".to_string(),
            templates: vec![],
            invariants: vec![],
        },
    }
}

/// Load templates from the specified domain packs.
pub fn load_templates(packs: &[String]) -> Result<TemplateRegistry> {
    let mut registry = TemplateRegistry::new();

    for pack in packs {
        match pack.as_str() {
            "growth-strategy" | "ask-converge" | "patent-research" | "linkedin-research"
            | "release-readiness" | "drafting-short" | "novelty-search" => {
                let default_registry = TemplateRegistry::with_defaults();
                if let Some(template) = default_registry.get(pack) {
                    registry.register(PackConfig::clone(template));
                }
            }
            "sdr-pipeline" => {
                tracing::warn!(pack = %pack, "Pack not yet implemented");
            }
            _ => {
                tracing::warn!(pack = %pack, "Unknown pack requested");
            }
        }
    }

    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_available_packs() {
        let packs = available_packs();
        assert!(packs.contains(&"growth-strategy".to_string()));
    }

    #[test]
    fn test_pack_info() {
        let info = pack_info("growth-strategy");
        assert_eq!(info.name, "growth-strategy");
        assert!(!info.templates.is_empty());
        assert!(!info.invariants.is_empty());
    }

    #[test]
    fn test_load_templates() {
        let registry = load_templates(&["growth-strategy".to_string()]).unwrap();
        assert!(registry.contains("growth-strategy"));
    }

    #[test]
    fn test_load_release_readiness_template() {
        let registry = load_templates(&["release-readiness".to_string()]).unwrap();
        assert!(registry.contains("release-readiness"));
    }
}
