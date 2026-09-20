use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    runtime::Tokio, trace::span_processor_with_async_runtime::BatchSpanProcessor,
    trace::SdkTracerProvider, Resource,
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

/// Holds the OTel trace provider alive for the process lifetime and gives
/// `main` something to flush on shutdown. `None` when telemetry export is
/// disabled (no `OTEL_EXPORTER_OTLP_ENDPOINT` set) — logging still works
/// either way, just without the trace export.
pub struct Telemetry {
    tracer_provider: Option<SdkTracerProvider>,
}

impl Telemetry {
    /// Best-effort flush of any spans still buffered in the batch exporter.
    /// Call this before the process exits so the last few requests aren't
    /// silently dropped.
    pub fn shutdown(&self) {
        if let Some(provider) = &self.tracer_provider {
            if let Err(err) = provider.shutdown() {
                eprintln!("failed to flush OpenTelemetry traces on shutdown: {err}");
            }
        }
    }
}

/// Wires up logging + (optionally) OpenTelemetry trace export.
///
/// - `RUST_LOG` controls log verbosity as usual (defaults to `info`).
/// - `LOG_FORMAT=json` switches log output to structured JSON, which any
///   log shipper (Filebeat/Vector for the ELK stack, the Datadog Agent,
///   Fluent Bit, ...) can parse straight off stdout — the standard
///   container-native pattern, no vendor-specific SDK needed for logs.
/// - `OTEL_EXPORTER_OTLP_ENDPOINT` (e.g. `http://localhost:4318`) turns on
///   trace export over OTLP/HTTP, the vendor-neutral protocol understood by
///   an OpenTelemetry Collector, Kibana/Elastic APM, Datadog, Grafana
///   Tempo, Jaeger, Honeycomb, etc. Unset by default so local dev doesn't
///   need a collector running just to boot the server.
pub fn init(service_name: &str) -> Telemetry {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let json_logs = std::env::var("LOG_FORMAT")
        .map(|v| v == "json")
        .unwrap_or(false);

    let fmt_layer = if json_logs {
        tracing_subscriber::fmt::layer().json().boxed()
    } else {
        tracing_subscriber::fmt::layer().boxed()
    };

    let otel_endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok();

    let (otel_layer, tracer_provider) = match &otel_endpoint {
        Some(endpoint) => {
            let exporter = SpanExporter::builder()
                .with_http()
                .with_endpoint(endpoint)
                .build()
                .expect("failed to build OTLP span exporter");

            let resource = Resource::builder()
                .with_service_name(service_name.to_string())
                .build();

            // `.with_batch_exporter(exporter)` would use the SDK's default
            // batch processor, which runs its export loop on a plain OS
            // thread with no Tokio reactor attached — fine for exporters
            // that do blocking I/O, but our exporter's HTTP client is
            // async (reqwest's async client, not the blocking one) and
            // panics without a runtime context. `BatchSpanProcessor`'s
            // async-runtime variant drives that loop as a proper Tokio
            // task instead, so the exporter's `.await`s actually work.
            let processor = BatchSpanProcessor::builder(exporter, Tokio).build();

            let provider = SdkTracerProvider::builder()
                .with_span_processor(processor)
                .with_resource(resource)
                .build();

            opentelemetry::global::set_tracer_provider(provider.clone());
            let tracer = provider.tracer(service_name.to_string());

            (
                Some(tracing_opentelemetry::layer().with_tracer(tracer)),
                Some(provider),
            )
        }
        None => (None, None),
    };

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .with(otel_layer)
        .init();

    match &otel_endpoint {
        Some(endpoint) => tracing::info!(%endpoint, "OpenTelemetry trace export enabled"),
        None => tracing::info!(
            "OpenTelemetry trace export disabled (set OTEL_EXPORTER_OTLP_ENDPOINT to enable)"
        ),
    }

    Telemetry { tracer_provider }
}
