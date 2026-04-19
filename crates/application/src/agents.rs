// Copyright 2024-2026 Reflective Labs
// SPDX-License-Identifier: MIT

//! LLM-powered agents for the converge-application.
//!
//! This module contains agents that use LLM providers to generate
//! insights beyond what deterministic agents can produce.

use converge_core::traits::{
    ChatBackend, ChatMessage, ChatRequest, ChatResponse, ChatRole, DynChatBackend, FinishReason,
    LlmError, ResponseFormat, TokenUsage,
};
use converge_core::{AgentEffect, ContextKey, ProposedFact, Suggestor};
use std::fmt::Write;
use std::sync::Arc;

fn proposed_fact(
    provenance: impl Into<String>,
    key: ContextKey,
    id: impl Into<String>,
    content: impl Into<String>,
) -> ProposedFact {
    ProposedFact::new(key, id, content, provenance)
}

/// LLM-powered agent that generates strategic insights from evaluations.
///
/// This agent runs after the `EvaluationAgent` and synthesizes higher-level
/// insights by analyzing the full context through an LLM.
///
/// # Pipeline Position
///
/// ```text
/// Seeds → Signals → Competitors → Strategies → Evaluations
///                                                    │
///                                                    ▼
///                                          StrategicInsightAgent
///                                                    │
///                                                    ▼
///                                              Hypotheses (insights)
/// ```
pub struct StrategicInsightAgent {
    provider: Arc<dyn DynChatBackend>,
    system_prompt: String,
}

impl StrategicInsightAgent {
    /// Creates a new `StrategicInsightAgent` with the given chat backend.
    pub fn new(provider: Arc<dyn DynChatBackend>) -> Self {
        Self {
            provider,
            system_prompt: r"You are a strategic advisor analyzing growth strategies for a business.

Given the context of market signals, competitor analysis, proposed strategies, and their evaluations,
synthesize 2-3 key strategic insights that the business should consider.

Each insight should:
1. Be actionable and specific
2. Reference the data in the context
3. Provide a clear recommendation

Format your response as a numbered list of insights, one per line.
Keep each insight concise (1-2 sentences).".to_string(),
        }
    }

    /// Creates an agent with a custom system prompt.
    pub fn with_prompt(
        provider: Arc<dyn DynChatBackend>,
        system_prompt: impl Into<String>,
    ) -> Self {
        Self {
            provider,
            system_prompt: system_prompt.into(),
        }
    }

    /// Builds the user prompt from context.
    #[allow(clippy::unused_self)]
    fn build_prompt(&self, ctx: &dyn converge_core::ContextView) -> String {
        let mut prompt = String::new();

        prompt.push_str("## Market Signals\n");
        for fact in ctx.get(ContextKey::Signals) {
            let _ = writeln!(prompt, "- {}", fact.content);
        }

        prompt.push_str("\n## Competitor Analysis\n");
        for fact in ctx.get(ContextKey::Competitors) {
            let _ = writeln!(prompt, "- {}", fact.content);
        }

        prompt.push_str("\n## Proposed Strategies\n");
        for fact in ctx.get(ContextKey::Strategies) {
            let _ = writeln!(prompt, "- {}: {}", fact.id, fact.content);
        }

        prompt.push_str("\n## Evaluations\n");
        for fact in ctx.get(ContextKey::Evaluations) {
            let _ = writeln!(prompt, "- {}", fact.content);
        }

        prompt.push_str("\n## Task\nProvide 2-3 strategic insights based on this analysis.");

        prompt
    }

    /// Parses LLM response into facts.
    #[allow(clippy::unused_self)]
    fn parse_response(&self, response: &str) -> Vec<ProposedFact> {
        let mut facts = Vec::new();

        for (i, line) in response.lines().enumerate() {
            let line = line.trim();

            // Skip empty lines
            if line.is_empty() {
                continue;
            }

            // Strip leading numbers like "1.", "2.", etc.
            let content = line
                .trim_start_matches(|c: char| c.is_numeric() || c == '.' || c == ')' || c == ' ')
                .trim();

            if !content.is_empty() && content.len() > 10 {
                facts.push(proposed_fact(
                    self.name(),
                    ContextKey::Hypotheses,
                    format!("insight:{}", i + 1),
                    content.to_string(),
                ));
            }
        }

        // Ensure we have at least one insight
        if facts.is_empty() {
            facts.push(proposed_fact(
                self.name(),
                ContextKey::Hypotheses,
                "insight:fallback",
                "LLM analysis completed but no structured insights extracted. Review raw evaluation data.",
            ));
        }

        facts
    }
}

#[async_trait::async_trait]
impl Suggestor for StrategicInsightAgent {
    fn name(&self) -> &'static str {
        "StrategicInsightAgent"
    }

    fn dependencies(&self) -> &[ContextKey] {
        &[ContextKey::Evaluations]
    }

    fn accepts(&self, ctx: &dyn converge_core::ContextView) -> bool {
        // Run once when evaluations exist but no hypotheses (insights) yet
        ctx.has(ContextKey::Evaluations) && !ctx.has(ContextKey::Hypotheses)
    }

    async fn execute(&self, ctx: &dyn converge_core::ContextView) -> AgentEffect {
        let prompt = self.build_prompt(ctx);

        let request = ChatRequest {
            messages: vec![
                ChatMessage {
                    role: ChatRole::System,
                    content: self.system_prompt.clone(),
                    tool_calls: Vec::new(),
                    tool_call_id: None,
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: prompt,
                    tool_calls: Vec::new(),
                    tool_call_id: None,
                },
            ],
            system: None,
            tools: Vec::new(),
            response_format: ResponseFormat::default(),
            max_tokens: Some(1024),
            temperature: Some(0.7),
            stop_sequences: Vec::new(),
            model: None,
        };

        let result = self.provider.chat(request).await;

        match result {
            Ok(response) => {
                let facts = self.parse_response(&response.content);
                AgentEffect::with_proposals(facts)
            }
            Err(e) => AgentEffect::with_proposal(proposed_fact(
                self.name(),
                ContextKey::Hypotheses,
                "insight:error",
                format!("LLM call failed: {e}. Manual review recommended."),
            )),
        }
    }
}

/// A simple mock chat backend for testing without API keys.
pub struct MockInsightProvider {
    response: String,
}

impl MockInsightProvider {
    /// Creates a mock provider with a predefined response.
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }

    /// Creates a mock provider with default insights.
    pub fn default_insights() -> Self {
        Self::new(
            r"1. Focus on the LinkedIn B2B campaign as your primary channel - it scores highest and aligns with market signals showing LinkedIn effectiveness for B2B.

2. Invest in self-service demo capabilities as a secondary priority - while it requires development investment, it directly addresses the buyer preference for self-service identified in market signals.

3. Consider a phased approach: launch LinkedIn campaign immediately for quick wins, then build self-service demo experience for long-term competitive advantage.",
        )
    }
}

impl ChatBackend for MockInsightProvider {
    type ChatFut<'a>
        = std::future::Ready<Result<ChatResponse, LlmError>>
    where
        Self: 'a;

    fn chat<'a>(&'a self, _req: ChatRequest) -> Self::ChatFut<'a> {
        std::future::ready(Ok(ChatResponse {
            content: self.response.clone(),
            tool_calls: Vec::new(),
            model: Some("mock-insight-v1".into()),
            usage: Some(TokenUsage {
                prompt_tokens: 100,
                completion_tokens: 50,
                total_tokens: 150,
            }),
            finish_reason: Some(FinishReason::Stop),
            metadata: Default::default(),
        }))
    }
}

// =============================================================================
// RISK ASSESSMENT AGENT
// =============================================================================

/// LLM-powered agent that identifies risks and challenges for proposed strategies.
///
/// This agent analyzes strategies and their evaluations to identify potential
/// risks, challenges, and mitigation recommendations.
///
/// # Pipeline Position
///
/// ```text
/// Seeds → Signals → Competitors → Strategies → Evaluations
///                                                    │
///                                    ┌───────────────┼───────────────┐
///                                    ▼               ▼               ▼
///                          StrategicInsightAgent  RiskAssessmentAgent
///                                    │               │
///                                    ▼               ▼
///                              Hypotheses      Constraints (risks)
/// ```
pub struct RiskAssessmentAgent {
    provider: Arc<dyn DynChatBackend>,
    system_prompt: String,
}

impl RiskAssessmentAgent {
    /// Creates a new `RiskAssessmentAgent` with the given chat backend.
    pub fn new(provider: Arc<dyn DynChatBackend>) -> Self {
        Self {
            provider,
            system_prompt: r"You are a risk analyst evaluating business strategies.

Given the proposed strategies and their evaluations, identify 2-3 key risks or challenges
that could impact successful execution.

For each risk:
1. Name the risk clearly
2. Explain what could go wrong
3. Suggest a mitigation approach

Format your response as a numbered list, one risk per item.
Keep each risk assessment concise (2-3 sentences)."
                .to_string(),
        }
    }

    /// Creates an agent with a custom system prompt.
    pub fn with_prompt(
        provider: Arc<dyn DynChatBackend>,
        system_prompt: impl Into<String>,
    ) -> Self {
        Self {
            provider,
            system_prompt: system_prompt.into(),
        }
    }

    /// Builds the user prompt from context.
    #[allow(clippy::unused_self)]
    fn build_prompt(&self, ctx: &dyn converge_core::ContextView) -> String {
        let mut prompt = String::new();

        prompt.push_str("## Company Context\n");
        for fact in ctx.get(ContextKey::Seeds) {
            let _ = writeln!(prompt, "- {}", fact.content);
        }

        prompt.push_str("\n## Market Signals\n");
        for fact in ctx.get(ContextKey::Signals) {
            let _ = writeln!(prompt, "- {}", fact.content);
        }

        prompt.push_str("\n## Competitive Landscape\n");
        for fact in ctx.get(ContextKey::Competitors) {
            let _ = writeln!(prompt, "- {}", fact.content);
        }

        prompt.push_str("\n## Proposed Strategies\n");
        for fact in ctx.get(ContextKey::Strategies) {
            let _ = writeln!(prompt, "- {}: {}", fact.id, fact.content);
        }

        prompt.push_str("\n## Strategy Evaluations\n");
        for fact in ctx.get(ContextKey::Evaluations) {
            let _ = writeln!(prompt, "- {}", fact.content);
        }

        prompt.push_str("\n## Task\nIdentify 2-3 key risks or challenges for these strategies and suggest mitigations.");

        prompt
    }

    /// Parses LLM response into risk facts.
    #[allow(clippy::unused_self)]
    fn parse_response(&self, response: &str) -> Vec<ProposedFact> {
        let mut facts = Vec::new();
        let mut risk_count = 0;

        for line in response.lines() {
            let line = line.trim();

            // Skip empty lines
            if line.is_empty() {
                continue;
            }

            // Strip leading numbers like "1.", "2.", etc.
            let content = line
                .trim_start_matches(|c: char| c.is_numeric() || c == '.' || c == ')' || c == ' ')
                .trim();

            if !content.is_empty() && content.len() > 20 {
                risk_count += 1;
                facts.push(proposed_fact(
                    self.name(),
                    ContextKey::Constraints,
                    format!("risk:{risk_count}"),
                    content.to_string(),
                ));
            }
        }

        // Ensure we have at least one risk identified
        if facts.is_empty() {
            facts.push(proposed_fact(
                self.name(),
                ContextKey::Constraints,
                "risk:none-identified",
                "No significant risks identified. Recommend manual review of assumptions.",
            ));
        }

        facts
    }
}

#[async_trait::async_trait]
impl Suggestor for RiskAssessmentAgent {
    fn name(&self) -> &'static str {
        "RiskAssessmentAgent"
    }

    fn dependencies(&self) -> &[ContextKey] {
        &[ContextKey::Strategies, ContextKey::Evaluations]
    }

    fn accepts(&self, ctx: &dyn converge_core::ContextView) -> bool {
        // Run once when strategies and evaluations exist but no constraints (risks) yet
        ctx.has(ContextKey::Strategies)
            && ctx.has(ContextKey::Evaluations)
            && !ctx.has(ContextKey::Constraints)
    }

    async fn execute(&self, ctx: &dyn converge_core::ContextView) -> AgentEffect {
        let prompt = self.build_prompt(ctx);

        let request = ChatRequest {
            messages: vec![
                ChatMessage {
                    role: ChatRole::System,
                    content: self.system_prompt.clone(),
                    tool_calls: Vec::new(),
                    tool_call_id: None,
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: prompt,
                    tool_calls: Vec::new(),
                    tool_call_id: None,
                },
            ],
            system: None,
            tools: Vec::new(),
            response_format: ResponseFormat::default(),
            max_tokens: Some(1024),
            temperature: Some(0.7),
            stop_sequences: Vec::new(),
            model: None,
        };

        let result = self.provider.chat(request).await;

        match result {
            Ok(response) => {
                let facts = self.parse_response(&response.content);
                AgentEffect::with_proposals(facts)
            }
            Err(e) => AgentEffect::with_proposal(proposed_fact(
                self.name(),
                ContextKey::Constraints,
                "risk:error",
                format!("Risk assessment failed: {e}. Manual review recommended."),
            )),
        }
    }
}

/// A mock chat backend for risk assessment testing.
pub struct MockRiskProvider {
    response: String,
}

impl MockRiskProvider {
    /// Creates a mock provider with a predefined response.
    pub fn new(response: impl Into<String>) -> Self {
        Self {
            response: response.into(),
        }
    }

    /// Creates a mock provider with default risk assessments.
    pub fn default_risks() -> Self {
        Self::new(
            r"1. **Resource Constraint Risk** - The self-service demo requires significant development investment while the team may be focused on the LinkedIn campaign. Mitigation: Phase the initiatives and allocate dedicated resources for each.

2. **Market Timing Risk** - The unclear competitive landscape means competitors could launch similar initiatives first. Mitigation: Conduct rapid competitor analysis within 2 weeks before committing to campaign messaging.

3. **Channel Saturation Risk** - LinkedIn B2B campaigns face increasing competition and rising costs. Mitigation: Test multiple audience segments with small budgets before scaling spend.",
        )
    }
}

impl ChatBackend for MockRiskProvider {
    type ChatFut<'a>
        = std::future::Ready<Result<ChatResponse, LlmError>>
    where
        Self: 'a;

    fn chat<'a>(&'a self, _req: ChatRequest) -> Self::ChatFut<'a> {
        std::future::ready(Ok(ChatResponse {
            content: self.response.clone(),
            tool_calls: Vec::new(),
            model: Some("mock-risk-v1".into()),
            usage: Some(TokenUsage {
                prompt_tokens: 120,
                completion_tokens: 80,
                total_tokens: 200,
            }),
            finish_reason: Some(FinishReason::Stop),
            metadata: Default::default(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use converge_core::{Context, Engine, ProposedFact};

    async fn promoted_context(entries: &[(ContextKey, &str, &str)]) -> Context {
        let mut ctx = Context::new();
        for (key, id, content) in entries {
            ctx.add_input(*key, *id, *content).unwrap();
        }
        Engine::new().run(ctx).await.unwrap().context
    }

    async fn promote_proposals(mut ctx: Context, proposals: Vec<ProposedFact>) -> Context {
        for proposal in proposals {
            ctx.add_proposal(proposal).unwrap();
        }
        Engine::new().run(ctx).await.unwrap().context
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn strategic_insight_agent_parses_numbered_list() {
        let provider = Arc::new(MockInsightProvider::default_insights());
        let agent = StrategicInsightAgent::new(provider);

        // Create a context with evaluations
        let ctx =
            promoted_context(&[(ContextKey::Evaluations, "eval:test", "Score: 80/100")]).await;

        assert!(agent.accepts(&ctx));

        let effect = agent.execute(&ctx).await;

        assert!(!effect.proposals.is_empty());
        assert!(
            effect
                .proposals
                .iter()
                .any(|f| f.id.starts_with("insight:"))
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn strategic_insight_agent_runs_once() {
        let provider = Arc::new(MockInsightProvider::default_insights());
        let agent = StrategicInsightAgent::new(provider);

        let ctx = promoted_context(&[
            (ContextKey::Evaluations, "eval:test", "Score: 80/100"),
            (ContextKey::Hypotheses, "insight:1", "Existing insight"),
        ])
        .await;

        // Should not accept because Hypotheses already exist
        assert!(!agent.accepts(&ctx));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn risk_assessment_agent_identifies_risks() {
        let provider = Arc::new(MockRiskProvider::default_risks());
        let agent = RiskAssessmentAgent::new(provider);

        // Create a context with strategies and evaluations
        let ctx = promoted_context(&[
            (ContextKey::Strategies, "strategy:test", "Test strategy"),
            (ContextKey::Evaluations, "eval:test", "Score: 75/100"),
        ])
        .await;

        assert!(agent.accepts(&ctx));

        let effect = agent.execute(&ctx).await;

        assert!(!effect.proposals.is_empty());
        assert!(effect.proposals.iter().any(|f| f.id.starts_with("risk:")));
        assert!(
            effect
                .proposals
                .iter()
                .all(|f| f.key == ContextKey::Constraints)
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn risk_assessment_agent_runs_once() {
        let provider = Arc::new(MockRiskProvider::default_risks());
        let agent = RiskAssessmentAgent::new(provider);

        let ctx = promoted_context(&[
            (ContextKey::Strategies, "strategy:test", "Test strategy"),
            (ContextKey::Evaluations, "eval:test", "Score: 75/100"),
            (ContextKey::Constraints, "risk:1", "Existing risk"),
        ])
        .await;

        // Should not accept because Constraints (risks) already exist
        assert!(!agent.accepts(&ctx));
    }
}
