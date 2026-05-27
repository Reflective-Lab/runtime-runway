//! EmbeddingProvider shape suite. Embeddings are not equivalence-testable
//! across backends (different models), so we test only shape and error modes.

use std::sync::Arc;

use runway_storage::EmbeddingProvider;

use crate::harness::{ContractContext, SuiteReport};
use crate::{contract_assert, contract_test};

pub async fn run_embedding_shape_suite(
    provider: Arc<dyn EmbeddingProvider>,
    ctx: ContractContext,
) -> SuiteReport {
    let report = SuiteReport::new(&ctx.backend, "EmbeddingProvider");

    contract_test!(&report, "embed_returns_valid_embedding", async {
        let e = provider
            .embed("hello world")
            .await
            .map_err(|e| e.to_string())?;
        contract_assert!(e.as_slice().len() == 768, "embedding length must be 768");
        Ok(())
    });

    contract_test!(&report, "embed_batch_returns_one_per_input", async {
        let results = provider
            .embed_batch(&["foo", "bar"])
            .await
            .map_err(|e| e.to_string())?;
        contract_assert!(
            results.len() == 2,
            "expected 2 embeddings, got {}",
            results.len()
        );
        Ok(())
    });

    contract_test!(&report, "embed_empty_string_rejected", async {
        match provider.embed("").await {
            Ok(_) => Err("expected error for empty input".to_string()),
            Err(e) => {
                contract_assert!(
                    e.to_string().contains("empty"),
                    "expected 'empty' in error message, got: {}",
                    e
                );
                Ok(())
            }
        }
    });

    contract_test!(&report, "embed_whitespace_only_rejected", async {
        match provider.embed("   \n\t  ").await {
            Ok(_) => Err("expected error for whitespace-only input".to_string()),
            Err(e) => {
                contract_assert!(
                    e.to_string().contains("empty"),
                    "expected 'empty' in error message, got: {}",
                    e
                );
                Ok(())
            }
        }
    });

    report
}
