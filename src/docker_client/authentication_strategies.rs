use std::{time::Duration, collections::HashMap};

use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use tracing::{info, error};

use super::client::DockerClientError;

#[async_trait]
pub trait AuthenticationStrategy: Send + Sync {
    fn inject_authentication(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder;
    fn needs_reauthenticating(&self) -> bool;
    async fn execute_authentication(&mut self, client: &reqwest::Client, authentication_parameters: &HashMap<&str, &str>, username: Option<&str>, password: Option<&str>) -> Result<(), DockerClientError>; 
}

pub struct HttpBasicAuthStrategy {
    username: String,
    password: Option<String>,
}

impl<> HttpBasicAuthStrategy<> {
    pub fn new(username: &str, password: Option<&str>) -> Self {
        Self {
            username: username.to_string(),
            password: password.map(|s| s.to_string())
        }
    }
}

#[async_trait]
impl<> AuthenticationStrategy for HttpBasicAuthStrategy<> {
    fn inject_authentication(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.basic_auth(&self.username, self.password.as_ref())
    }

    fn needs_reauthenticating(&self) -> bool {
        false
    }

    async fn execute_authentication(&mut self, _client: &reqwest::Client, _authentication_parameters: &HashMap<&str, &str>, username: Option<&str>, password: Option<&str>) -> Result<(), DockerClientError> { 
        self.username = username.map(|u| u.to_string()).ok_or(DockerClientError::BadAuthenticationCredentials)?;
        self.password = password.map(|u| u.to_string());

        Ok(())
    }
}

pub struct BearerTokenAuthStrategy {
    token: Option<String>,
    created_at: chrono::DateTime<Utc>,
    expires_in: Duration, 
    scope: String,
}

#[derive(Deserialize)]
struct BearerToken {
    token: String,
    issued_at: Option<String>,
    expires_in: Option<u64>
}

impl BearerTokenAuthStrategy {
    pub fn new(container_repository: &str) -> Self {
        let scope = format!("repository:{}:pull", container_repository);
        Self {
            token: None,
            created_at: Utc::now(),
            expires_in: Duration::from_secs(0),
            scope,
        }
    }
}

#[async_trait]
impl AuthenticationStrategy for BearerTokenAuthStrategy {
    fn inject_authentication(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.bearer_auth(self.token.as_ref().expect("The authentication flow has not been executed"))
    }

    fn needs_reauthenticating(&self) -> bool {
        let now = Utc::now();
        now.timestamp() - self.created_at.timestamp() >= self.expires_in.as_secs() as i64
    }

    async fn execute_authentication(&mut self, client: &reqwest::Client, authentication_parameters: &HashMap<&str, &str>, username: Option<&str>, password: Option<&str>) -> Result<(), DockerClientError> {
        let mut authentication_parameters = authentication_parameters.clone();
        authentication_parameters.insert("scope", &self.scope);

        let authentication_service = authentication_parameters.get("realm").expect("Who am I supposed to authenticate to ?");
        let authentication_query_string = authentication_parameters.iter()
            .filter(|(key, _)| **key != "realm")
            .map(|(k, v)| [*k, *v].join("="))
            .collect::<Vec<_>>()
            .join("&");

            info!("Attempting to authenticate to {}", authentication_service);
            let mut token_request = client.get(format!("{}?{}", authentication_service, authentication_query_string));
            if let Some(username) = username {
                token_request = token_request.basic_auth(username, password);
            }

            let response = token_request.send().await?;
            if response.status() == 401 {
                info!("Response is 401, credentials are propably rejected");
                return Err(DockerClientError::BadAuthenticationCredentials);
            } else if response.status() != 200 {
                info!("Response is {}, not the expected 200", response.status());
                return Err(DockerClientError::UnexpectedStatusCode(response.status().as_u16()));
            }

            info!("Deserializing 200 response from {}", authentication_service);
            let token = response.json::<BearerToken>().await?;
            // Inspiration from https://github.com/camallo/dkregistry-rs/blob/37acecb4b8139dd1b1cc83795442f94f90e1ffc5/src/v2/auth.rs#L67.
            // Apparently, token servers can return a 200 and "unauthenticated" as a token. Why ?
            if token.token.is_empty() || token.token == "unauthenticated" {
                error!("Registry token server did return a 200 response but NO TOKEN. Bailing out.");
                return Err(DockerClientError::BadAuthenticationCredentials);
            }

            self.created_at = token.issued_at
                .map(|issued| chrono::DateTime::parse_from_rfc3339(&issued).unwrap())
                .unwrap_or_else(|| Utc::now().into())
                .into();
            self.expires_in = token.expires_in.map(Duration::from_secs).unwrap_or_else(|| Duration::from_secs(60));
            self.token = Some(token.token);
        Ok(())
    }
}

pub struct AnonymousAuthStrategy;

#[async_trait]
impl AuthenticationStrategy for AnonymousAuthStrategy {
    fn inject_authentication(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request
    }

    fn needs_reauthenticating(&self) -> bool {
        false
    }

    async fn execute_authentication(&mut self, _client: &reqwest::Client, _authentication_parameters: &HashMap<&str, &str>, _username: Option<&str>, _password: Option<&str>) -> Result<(), DockerClientError> {
        Ok(())
    }
}