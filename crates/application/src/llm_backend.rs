// Copyright 2024-2026 Reflective Labs
// SPDX-License-Identifier: MIT

use std::sync::Arc;

use tracing::{info, warn};

use converge_core::traits::DynChatBackend;
use converge_provider::{ChatBackendSelectionConfig, select_chat_backend};

use crate::agents::MockInsightProvider;

pub fn create_chat_backend_or_mock() -> Arc<dyn DynChatBackend> {
    match ChatBackendSelectionConfig::from_env() {
        Ok(config) => match select_chat_backend(&config) {
            Ok(selected) => {
                info!(
                    provider = selected.provider(),
                    model = selected.model(),
                    fitness = selected.selection.fitness.total,
                    "Using requirements-selected LLM backend"
                );
                selected.backend
            }
            Err(error) => {
                warn!("{error}");
                warn!(
                    "Falling back to mock provider. Configure API keys and selection env vars if you want a real backend."
                );
                Arc::new(MockInsightProvider::default_insights()) as Arc<dyn DynChatBackend>
            }
        },
        Err(error) => {
            warn!("{error}");
            warn!("Using mock provider instead.");
            Arc::new(MockInsightProvider::default_insights()) as Arc<dyn DynChatBackend>
        }
    }
}

#[cfg(test)]
mod tests {
    use converge_provider::ChatBackendSelectionConfig;

    #[test]
    fn default_selection_config_is_interactive() {
        let config = ChatBackendSelectionConfig::default();
        assert_eq!(
            config.criteria,
            converge_core::model_selection::SelectionCriteria::interactive()
        );
    }
}
