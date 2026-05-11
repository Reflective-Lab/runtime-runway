use anyhow::Result;
use opentelemetry::global;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{runtime, trace as sdktrace};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Returned by `init()`. Flushes spans and Sentry events on drop.
pub struct TelemetryGuard {
    _sentry: sentry::ClientInitGuard,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        global::shutdown_tracer_provider();
    }
}

/// Configuration for the telemetry stack.
pub struct TelemetryConfig {
    /// Cloud Run service name — used as the OTel service name and Sentry release.
    pub service: String,
    /// Deployment environment: "dev", "staging", "prod".
    pub env: String,
    /// Sentry DSN. If empty, Sentry is disabled.
    pub sentry_dsn: String,
    /// OTLP endpoint. Defaults to Cloud Trace via the standard GCP OTLP endpoint.
    pub otlp_endpoint: Option<String>,
}

impl TelemetryConfig {
    pub fn from_env(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            env: std::env::var("ENV").unwrap_or_else(|_| "dev".into()),
            sentry_dsn: std::env::var("SENTRY_DSN").unwrap_or_default(),
            otlp_endpoint: std::env::var("OTLP_ENDPOINT").ok(),
        }
    }
}

/// Initialise OTel tracing → Cloud Trace, Sentry error tracking, and JSON structured logging.
///
/// Call once at the top of `main()`. Hold the returned `TelemetryGuard` for the process lifetime.
pub fn init(config: TelemetryConfig) -> Result<TelemetryGuard> {
    let sentry_guard = if !config.sentry_dsn.is_empty() {
        sentry::init((
            config.sentry_dsn.clone(),
            sentry::ClientOptions {
                release: sentry::release_name!(),
                environment: Some(config.env.clone().into()),
                traces_sample_rate: if config.env == "prod" { 0.1 } else { 1.0 },
                ..Default::default()
            },
        ))
    } else {
        sentry::init(sentry::ClientOptions::default())
    };

    // OTel tracer → Cloud Trace (OTLP/HTTP)
    let endpoint = config
        .otlp_endpoint
        .unwrap_or_else(|| "https://cloudtrace.googleapis.com/v1/traces".to_string());

    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .http()
                .with_endpoint(endpoint),
        )
        .with_trace_config(sdktrace::Config::default().with_resource(
            opentelemetry_sdk::Resource::new(vec![
                opentelemetry::KeyValue::new("service.name", config.service.clone()),
                opentelemetry::KeyValue::new("deployment.environment", config.env.clone()),
            ]),
        ))
        .install_batch(runtime::Tokio)?;

    // JSON subscriber (→ Cloud Logging) + OTel layer + Sentry layer
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().json().flatten_event(true))
        .with(OpenTelemetryLayer::new(tracer))
        .with(sentry_tracing::layer())
        .init();

    tracing::info!(service = %config.service, env = %config.env, "telemetry initialised");

    Ok(TelemetryGuard {
        _sentry: sentry_guard,
    })
}
