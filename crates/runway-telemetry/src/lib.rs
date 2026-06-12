use anyhow::Result;
use opentelemetry::{global, trace::TracerProvider as _};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{runtime, trace as sdktrace};
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Returned by `init()`. Flushes spans and Sentry events on drop.
pub struct TelemetryGuard {
    #[cfg(feature = "sentry")]
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
    /// Sentry DSN. If empty, Sentry is disabled. Ignored entirely when the
    /// `sentry` feature is disabled.
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
    #[cfg(feature = "sentry")]
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

    // Local app-pairing smokes should not construct the OTLP HTTP exporter:
    // reqwest's macOS system-proxy discovery can panic in headless shells.
    // Keep structured logs + Sentry layer locally; enable OTLP outside local dev.
    let local_dev = std::env::var("LOCAL_DEV").as_deref() == Ok("true");
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    if local_dev && config.otlp_endpoint.is_none() {
        let registry = tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().json().flatten_event(true));

        #[cfg(feature = "sentry")]
        let registry = registry.with(sentry_tracing::layer());

        registry.init();

        tracing::info!(service = %config.service, env = %config.env, "telemetry initialised without otlp");

        return Ok(TelemetryGuard {
            #[cfg(feature = "sentry")]
            _sentry: sentry_guard,
        });
    }

    // OTel tracer → Cloud Trace (OTLP/HTTP)
    let endpoint = config
        .otlp_endpoint
        .unwrap_or_else(|| "https://cloudtrace.googleapis.com/v1/traces".to_string());

    let provider = opentelemetry_otlp::new_pipeline()
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
    global::set_tracer_provider(provider.clone());
    let tracer = provider.tracer(config.service.clone());

    // JSON subscriber (→ Cloud Logging) + OTel layer + (optional) Sentry layer
    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().json().flatten_event(true))
        .with(OpenTelemetryLayer::new(tracer));

    #[cfg(feature = "sentry")]
    let registry = registry.with(sentry_tracing::layer());

    registry.init();

    tracing::info!(service = %config.service, env = %config.env, "telemetry initialised");

    Ok(TelemetryGuard {
        #[cfg(feature = "sentry")]
        _sentry: sentry_guard,
    })
}
