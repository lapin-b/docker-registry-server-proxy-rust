use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;
use tracing::debug;

use crate::data::helpers::split_registry_and_container;

use super::client::{DockerClient, DockerClientError};

#[derive(Clone)]
pub struct DockerClientsStore {
    http_client: reqwest::Client,
    docker_clients_store: Arc<RwLock<HashMap<String, Arc<DockerClient>>>>
}

impl DockerClientsStore {
    pub fn new() -> Self {
        Self {
            http_client: reqwest::Client::new(),
            docker_clients_store: Default::default()
        }
    }

    #[tracing::instrument(skip_all, fields(registry_key = registry_container_key))]
    pub async fn get_client(&self, registry_container_key: &str) -> Result<Arc<DockerClient>, DockerClientError> {
        let map_lock = self.docker_clients_store.read().await;

        debug!("Checking if key exists");
        if map_lock.contains_key(registry_container_key) {
            debug!("Key exists");
            let client = map_lock
                .get(registry_container_key)
                .expect("Registry key for the client must exist");

            // Check if the authentication needs revalidation or not.
            // If yes, we'll replace the client further in this function body. Otherwise, we can return
            // it to the caller.
            debug!("Check if key needs revalidation");
            if !client.authentication_needs_revalidation() {
                debug!("Doesn't need revalidation, returning");
                return Ok(Arc::clone(client));
            }

            debug!("Key needs revalidation, continuing");
        }

        drop(map_lock);
        // Client doesn't exist or needs revalidation. We drop the existing read and will non-atomically upgrade to a write
        // lock on the map.
        let mut map_lock = self.docker_clients_store.write().await;
        let (registry, container) = split_registry_and_container(&registry_container_key);
        let mut client = DockerClient::new(registry, container, self.http_client.clone());
        client.authenticate(None, None).await?;
        let client = Arc::new(client);

        map_lock.insert(registry_container_key.to_string(), Arc::clone(&client));

        return Ok(client);
    }
}