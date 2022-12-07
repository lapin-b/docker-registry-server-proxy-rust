mod configuration;
mod controllers;
mod requests;
mod data;

use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use axum::Router;
use axum::extract::FromRef;
use axum::routing::{get, post, patch};
use axum::ServiceExt;
use tokio::sync::RwLock;
use tower::Layer;
use tower_http::trace::TraceLayer;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use crate::configuration::Configuration;
use crate::data::upload_in_progress::UploadsStore;

pub type UploadsInProgressState = Arc<RwLock<UploadsStore>>;

#[derive(FromRef, Clone)]
pub struct ApplicationState {
    conf: Arc<Configuration>,
    uploads: UploadsStore
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=debug,pull_registry_attempt=debug".into())
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Loading configuration");
    let configuration = toml::from_str::<Configuration>(&tokio::fs::read_to_string("configuration.toml").await?)?;

    info!("Creating registry directories");
    tokio::fs::create_dir_all(&configuration.registry_storage).await?;
    tokio::fs::create_dir_all(&configuration.temporary_registry_storage).await?;

    let application_state = ApplicationState {
        conf: Arc::new(configuration),
        uploads: UploadsStore::new()
    };

    let app = Router::new()
        .route("/", get(controllers::base::root))
        .route("/v2/", get(controllers::base::registry_base))
        .route("/v2/:container_ref/blobs/uploads/", post(controllers::uploads::initiate_upload))
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
        .with_state(application_state)
        .layer(TraceLayer::new_for_http());

    let url_rewrite_layer = axum::middleware::from_fn(requests::rewrite_container_part_url);
    let app_with_rewrite = url_rewrite_layer.layer(app);

    let addr = SocketAddr::from_str("0.0.0.0:8000").unwrap();
    println!("Listen port 8000");
    axum::Server::bind(&addr)
        .serve(app_with_rewrite.into_make_service())
        .await?;

    Ok(())
}

