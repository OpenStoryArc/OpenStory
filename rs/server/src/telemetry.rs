//! OpenTelemetry tracing init — opt-in via env var.
//!
//! Set `OPEN_STORY_OTEL_ENABLED=true` to export spans via OTLP/gRPC to
//! `OTEL_EXPORTER_OTLP_ENDPOINT` (default `http://localhost:4317`).
//! When disabled, falls back to a plain `tracing_subscriber` fmt layer so
//! `tracing::info!` etc. still print to stderr.

use anyhow::Result;
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::{
    runtime,
    trace::{self as sdktrace, Sampler},
    Resource,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

const SERVICE_NAME: &str = "open-story-server";

pub struct OtelGuard {
    enabled: bool,
}

impl Drop for OtelGuard {
    fn drop(&mut self) {
        if self.enabled {
            global::shutdown_tracer_provider();
        }
    }
}

pub fn init() -> Result<OtelGuard> {
    let filter = EnvFilter::try_from_env("OPEN_STORY_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=info,open_story=debug"));

    let fmt_layer = tracing_subscriber::fmt::layer().with_target(false);

    let otel_enabled = std::env::var("OPEN_STORY_OTEL_ENABLED")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE"))
        .unwrap_or(false);

    if otel_enabled {
        let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:4317".to_string());

        let exporter = opentelemetry_otlp::SpanExporter::builder()
            .with_tonic()
            .with_endpoint(&endpoint)
            .build()?;

        let provider = sdktrace::TracerProvider::builder()
            .with_batch_exporter(exporter, runtime::Tokio)
            .with_sampler(Sampler::AlwaysOn)
            .with_resource(Resource::new(vec![KeyValue::new(
                "service.name",
                SERVICE_NAME,
            )]))
            .build();

        let tracer = opentelemetry::trace::TracerProvider::tracer(&provider, SERVICE_NAME);
        global::set_tracer_provider(provider);

        let otel_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .with(otel_layer)
            .try_init()
            .ok();

        eprintln!("  \x1b[2mOTel tracing:\x1b[0m   enabled → {endpoint}");
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt_layer)
            .try_init()
            .ok();
    }

    Ok(OtelGuard { enabled: otel_enabled })
}
