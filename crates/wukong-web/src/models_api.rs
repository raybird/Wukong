//! `GET /api/models`: the models `opencode models` lists in this container,
//! grouped by provider for the Settings and schedule model pickers.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use std::future::Future;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// The list changes on its own (opencode's free models came and went between
/// two runs on 2026-10-27), so it is cached briefly rather than until restart.
pub const MODELS_CACHE_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ModelCatalog {
    pub providers: Vec<ProviderModels>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProviderModels {
    pub id: String,
    /// Full `provider/model` IDs, as accepted by `--model`.
    pub models: Vec<String>,
}

/// Group `opencode models` stdout (one `provider/model` per line) by provider,
/// keeping opencode's order. Model IDs may contain further slashes
/// (`nvidia/deepseek-ai/deepseek-v4.1-flash`), so only the first one splits.
pub fn parse_models(stdout: &str) -> ModelCatalog {
    let mut catalog = ModelCatalog::default();
    for line in stdout.lines().map(str::trim) {
        let Some((provider, model)) = line.split_once('/') else {
            continue;
        };
        if provider.is_empty() || model.is_empty() {
            continue;
        }
        match catalog.providers.iter_mut().find(|p| p.id == provider) {
            Some(group) => group.models.push(line.to_string()),
            None => catalog.providers.push(ProviderModels {
                id: provider.to_string(),
                models: vec![line.to_string()],
            }),
        }
    }
    catalog
}

pub struct ModelCatalogCache {
    entry: Mutex<Option<(Instant, ModelCatalog)>>,
}

impl ModelCatalogCache {
    pub const fn new() -> Self {
        Self {
            entry: Mutex::const_new(None),
        }
    }

    /// Holding the lock across `fetch` lets concurrent page loads share one
    /// `opencode models` run. Failures and empty listings are not cached.
    pub async fn get_or_fetch<F, Fut>(
        &self,
        ttl: Duration,
        fetch: F,
    ) -> Result<ModelCatalog, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<String, String>>,
    {
        let mut entry = self.entry.lock().await;
        if let Some((fetched_at, catalog)) = entry.as_ref() {
            if fetched_at.elapsed() < ttl {
                return Ok(catalog.clone());
            }
        }
        let catalog = parse_models(&fetch().await?);
        *entry = (!catalog.providers.is_empty()).then(|| (Instant::now(), catalog.clone()));
        Ok(catalog)
    }
}

impl Default for ModelCatalogCache {
    fn default() -> Self {
        Self::new()
    }
}

pub fn models_response(result: Result<ModelCatalog, String>) -> Response {
    match result {
        Ok(catalog) => Json(catalog).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, e).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Captured 2026-10-27 from opencode 1.19.3 in the RunWuKong test deployment,
    // with NVIDIA_API_KEY set and a custom provider in a temporary OPENCODE_CONFIG.
    const REAL_OUTPUT: &str = include_str!("../tests/fixtures/opencode-models-1.19.3.txt");

    fn ids(catalog: &ModelCatalog) -> Vec<&str> {
        catalog.providers.iter().map(|p| p.id.as_str()).collect()
    }

    // SCN-001
    #[test]
    fn real_output_is_grouped_by_provider_in_listed_order() {
        let catalog = parse_models(REAL_OUTPUT);
        assert_eq!(ids(&catalog), ["opencode", "nvidia", "probe26"]);
        let counts: Vec<usize> = catalog.providers.iter().map(|p| p.models.len()).collect();
        assert_eq!(counts, [9, 38, 2]);
        assert!(catalog.providers[1]
            .models
            .contains(&"nvidia/deepseek-ai/deepseek-v4.1-flash".to_string()));
        assert_eq!(
            catalog.providers[2].models,
            ["probe26/model-b", "probe26/org/model-a"]
        );
    }

    // SCN-001
    #[test]
    fn lines_without_a_provider_and_model_are_ignored() {
        let catalog = parse_models("\n  \nmodels\n/x\nopencode/\n  opencode/a  \n");
        assert_eq!(
            catalog,
            ModelCatalog {
                providers: vec![ProviderModels {
                    id: "opencode".to_string(),
                    models: vec!["opencode/a".to_string()],
                }],
            }
        );
    }

    // SCN-005
    #[test]
    fn empty_output_yields_no_providers() {
        assert_eq!(parse_models(""), ModelCatalog::default());
    }

    async fn fetch_counted(
        cache: &ModelCatalogCache,
        ttl: Duration,
        calls: &AtomicUsize,
        result: Result<&str, &str>,
    ) -> Result<ModelCatalog, String> {
        cache
            .get_or_fetch(ttl, || async {
                calls.fetch_add(1, Ordering::SeqCst);
                result.map(str::to_string).map_err(str::to_string)
            })
            .await
    }

    // SCN-001
    #[tokio::test]
    async fn cache_reuses_a_result_within_ttl() {
        let cache = ModelCatalogCache::new();
        let calls = AtomicUsize::new(0);
        let ttl = Duration::from_secs(60);
        fetch_counted(&cache, ttl, &calls, Ok("opencode/a"))
            .await
            .unwrap();
        let second = fetch_counted(&cache, ttl, &calls, Ok("opencode/b"))
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(second.providers[0].models, ["opencode/a"]);
    }

    // SCN-001
    #[tokio::test]
    async fn cache_refetches_after_ttl() {
        let cache = ModelCatalogCache::new();
        let calls = AtomicUsize::new(0);
        fetch_counted(&cache, Duration::ZERO, &calls, Ok("opencode/a"))
            .await
            .unwrap();
        let second = fetch_counted(&cache, Duration::ZERO, &calls, Ok("opencode/b"))
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(second.providers[0].models, ["opencode/b"]);
    }

    // SCN-005: a failed or empty listing must not pin the pickers to manual
    // input for the whole TTL.
    #[tokio::test]
    async fn failures_and_empty_listings_are_not_cached() {
        let cache = ModelCatalogCache::new();
        let calls = AtomicUsize::new(0);
        let ttl = Duration::from_secs(60);
        assert_eq!(
            fetch_counted(&cache, ttl, &calls, Err("command timed out")).await,
            Err("command timed out".to_string())
        );
        assert_eq!(
            fetch_counted(&cache, ttl, &calls, Ok("")).await,
            Ok(ModelCatalog::default())
        );
        let third = fetch_counted(&cache, ttl, &calls, Ok("opencode/a"))
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(ids(&third), ["opencode"]);
    }

    // SCN-005: the pickers fall back to manual input on a non-2xx status.
    #[tokio::test]
    async fn listing_failure_is_reported_as_bad_gateway_with_reason() {
        let response = models_response(Err("command timed out".to_string()));
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"command timed out");
    }

    // SCN-001
    #[tokio::test]
    async fn listing_success_is_json_grouped_by_provider() {
        let response = models_response(Ok(parse_models("opencode/a\nnvidia/x/y\n")));
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"providers": [
                {"id": "opencode", "models": ["opencode/a"]},
                {"id": "nvidia", "models": ["nvidia/x/y"]}
            ]})
        );
    }
}
