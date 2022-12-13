mod configuration;
mod controllers;
mod requests;
mod data;
mod docker_client;

use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use axum::Router;
use axum::extract::FromRef;
use axum::routing::{get, post, patch};
use axum::ServiceExt;
use docker_client::clients_store::DockerClientsStore;
use tokio::signal::unix::signal;
use tokio::signal::unix::SignalKind;
use tokio::sync::RwLock;
use tower::Layer;
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use crate::configuration::Configuration;
use crate::data::uploads::UploadsStore;

pub type UploadsInProgressState = Arc<RwLock<UploadsStore>>;

static UPLOAD_PRUNE_INTERVAL: u64 = 60;
static UPLOAD_PRUNE_AGE: u64 = 180;

#[derive(FromRef, Clone)]
pub struct ApplicationState {
    conf: Arc<Configuration>,
    docker_clients: DockerClientsStore,
    uploads: UploadsStore
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    // Logging setup
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=debug,docker_storage_proxy_registry=debug".into())
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Configuration and registry directories setup
    info!("Loading configuration");
    let configuration = toml::from_str::<Configuration>(&tokio::fs::read_to_string("configuration.toml").await?)?;

    info!("Creating registry directories");
    tokio::fs::create_dir_all(&configuration.registry_storage).await?;
    tokio::fs::create_dir_all(&configuration.temporary_registry_storage).await?;
    tokio::fs::create_dir_all(&configuration.proxy_storage).await?;

    // Application state setup
    let application_state = ApplicationState {
        conf: Arc::new(configuration),
        docker_clients: DockerClientsStore::new(),
        uploads: UploadsStore::new()
    };

    let uploads_cleanup_task = {
        let uploads_app_state = application_state.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(UPLOAD_PRUNE_INTERVAL)).await;
                uploads_app_state.uploads.prune().await;
            }
        })
    };

    // HTTP server setup
    let app = Router::new()
        .route("/", get(controllers::base::root))
        .route("/v2/", get(controllers::base::registry_base))
        .route(
            "/v2/:container_ref/blobs/uploads/", 
            post(controllers::uploads::initiate_upload)
        )
        .route(
            "/v2/:container_ref/blobs/uploads/:uuid", 
            patch(controllers::uploads::process_blob_chunk_upload)
                .put(controllers::uploads::finalize_blob_upload)
                .delete(controllers::uploads::delete_upload)
        )
        .route(
            "/v2/:container_ref/blobs/:digest", 
            get(controllers::blobs::check_blob_exists)
                .head(controllers::blobs::check_blob_exists)
        )
        .route(
            "/v2/:container_ref/manifests/:reference", 
            get(controllers::manifests::fetch_manifest)
                .put(controllers::manifests::upload_manifest)
        )
        .route(
            "/v2/proxy/:container_ref/manifests/:reference",
            get(controllers::manifests::proxy_fetch_manifest)
        )
        .route(
            "/v2/proxy/:container_ref/blobs/:digest",
            get(controllers::blobs::proxy_blob)
        )
        .with_state(application_state)
        .layer(TraceLayer::new_for_http());

    let url_rewrite_layer = axum::middleware::from_fn(requests::rewrite_container_part_url);
    let app_with_rewrite = url_rewrite_layer.layer(app);

    // Http server and termination setup handling
    let (server_termination_tx, server_termination_rx) = tokio::sync::oneshot::channel::<()>();

    let http_server = tokio::spawn(async {
        let address = SocketAddr::from_str("0.0.0.0:8000").unwrap();
        warn!("Listening on port 8000");
        axum::Server::bind(&address)
            .serve(app_with_rewrite.into_make_service())
            .with_graceful_shutdown(async {
                server_termination_rx.await.ok();
                info!("HTTP server received termination");
            }).await.unwrap();
    });

    server_shutdown_signal().await;

    server_termination_tx.send(()).unwrap();
    http_server.await.unwrap();
    uploads_cleanup_task.abort();

    Ok(())
}

async fn server_shutdown_signal() {
    // Graceful termination setup
    let mut interrupt_signal = signal(SignalKind::interrupt()).unwrap();
    let mut terminate_signal = signal(SignalKind::terminate()).unwrap();

    tokio::select! {
        _ = interrupt_signal.recv() => warn!("Received SIGINT"),
        _ = terminate_signal.recv() => warn!("Received SIGTERM"),
    };
}
