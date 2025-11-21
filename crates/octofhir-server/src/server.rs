use std::net::SocketAddr;

use axum::{Router, middleware, routing::get};
use tower_http::{compression::CompressionLayer, cors::CorsLayer, trace::TraceLayer};

use crate::{config::AppConfig, handlers, middleware as app_middleware};
use octofhir_db_memory::{
    DynStorage, StorageBackend as DbBackend, StorageConfig as DbStorageConfig,
    StorageOptions as DbStorageOptions, create_storage,
};
use octofhir_search::SearchConfig as EngineSearchConfig;

#[derive(Clone)]
pub struct AppState {
    pub storage: DynStorage,
    pub search_cfg: EngineSearchConfig,
    pub fhir_version: String,
}

pub struct OctofhirServer {
    addr: SocketAddr,
    app: Router,
}

pub fn build_app(cfg: &AppConfig) -> Router {
    let body_limit = cfg.server.body_limit_bytes;

    // Build storage from server config (in-memory backend)
    let db_cfg = DbStorageConfig {
        backend: match cfg.storage.backend {
            crate::config::StorageBackend::InMemoryPapaya => DbBackend::InMemoryPapaya,
        },
        options: DbStorageOptions {
            memory_limit_bytes: cfg.storage.memory_limit_bytes,
            preallocate_items: cfg.storage.preallocate_items,
        },
    };
    let storage = create_storage(&db_cfg);

    // Build search engine config using counts from AppConfig
    let search_cfg = EngineSearchConfig {
        default_count: cfg.search.default_count,
        max_count: cfg.search.max_count,
        ..Default::default()
    };

    let state = AppState {
        storage,
        search_cfg,
        fhir_version: cfg.fhir.version.clone(),
    };

    Router::new()
        // Health and info endpoints
        .route("/", get(handlers::root))
        .route("/healthz", get(handlers::healthz))
        .route("/readyz", get(handlers::readyz))
        .route("/metadata", get(handlers::metadata))
        // Browser favicon shortcut
        .route("/favicon.ico", get(handlers::favicon))
        // New API endpoints for UI
        .route("/api/health", get(handlers::api_health))
        .route("/api/build-info", get(handlers::api_build_info))
        .route("/api/resource-types", get(handlers::api_resource_types))
        
        // Embedded UI under /ui
        .route("/ui", get(handlers::ui_index))
        .route("/ui/{*path}", get(handlers::ui_static))
        // CRUD and search placeholders
        .route(
            "/{resource_type}",
            get(handlers::search_resource).post(handlers::create_resource),
        )
        .route(
            "/{resource_type}/{id}",
            get(handlers::read_resource)
                .put(handlers::update_resource)
                .delete(handlers::delete_resource),
        )
        // Middleware stack (order: request id -> content negotiation -> compression/cors/trace -> body limit)
        .layer(middleware::from_fn(app_middleware::request_id))
        .layer(middleware::from_fn(app_middleware::content_negotiation))
        .layer(CorsLayer::permissive())
        .layer(CompressionLayer::new())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|req: &axum::http::Request<_>| {
                    use tracing::field::Empty;
                    // Skip creating a span for browser favicon requests to avoid noisy logs
                    if req.uri().path() == "/favicon.ico" {
                        return tracing::span!(tracing::Level::TRACE, "noop");
                    }
                    let method = req.method().clone();
                    let uri = req.uri().clone();
                    let req_id = req
                        .extensions()
                        .get::<axum::http::HeaderValue>()
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string();
                    tracing::info_span!(
                        "http.request",
                        http.method = %method,
                        http.target = %uri,
                        http.route = Empty,
                        http.status_code = Empty,
                        request_id = %req_id
                    )
                })
                .on_response(
                    |res: &axum::http::Response<_>,
                     latency: std::time::Duration,
                     span: &tracing::Span| {
                        // Record status on the span; access log emission is handled only for non-favicon paths via the span field presence
                        span.record(
                            "http.status_code",
                            tracing::field::display(res.status().as_u16()),
                        );
                        // Determine if this span is our real request span by checking that it has the http.method field recorded (noop span won't)
                        // Unfortunately Span API doesn't expose field inspection, so we conservatively avoid extra logic and instead rely on make_span_with to avoid logging favicon.
                        // Thus, only emit the access log if the span's metadata target matches our request span name.
                        if let Some(meta) = span.metadata() {
                            if meta.name() != "noop" {
                                tracing::info!(
                                    http.status = %res.status().as_u16(),
                                    elapsed_ms = %latency.as_millis(),
                                    "request handled"
                                );
                            }
                        }
                    },
                ),
        )
        .with_state(state)
        .layer(axum::extract::DefaultBodyLimit::max(body_limit))
}

pub struct ServerBuilder {
    addr: SocketAddr,
    config: AppConfig,
}

impl ServerBuilder {
    pub fn new() -> Self {
        let cfg = AppConfig::default();
        Self {
            addr: cfg.addr(),
            config: cfg,
        }
    }

    pub fn with_addr(mut self, addr: SocketAddr) -> Self {
        self.addr = addr;
        self
    }

    pub fn with_config(mut self, cfg: AppConfig) -> Self {
        self.addr = cfg.addr();
        self.config = cfg;
        self
    }

    pub fn build(self) -> OctofhirServer {
        let app = build_app(&self.config);

        OctofhirServer {
            addr: self.addr,
            app,
        }
    }
}

impl Default for ServerBuilder {
    fn default() -> Self { Self::new() }
}

impl OctofhirServer {
    pub async fn run(self) -> anyhow::Result<()> {
        let listener = tokio::net::TcpListener::bind(self.addr).await?;
        tracing::info!("listening on {}", self.addr);
        axum::serve(listener, self.app)
            .with_graceful_shutdown(shutdown_signal())
            .await?;
        Ok(())
    }
}

async fn shutdown_signal() {
    // Wait for Ctrl+C
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutdown signal received");
}
