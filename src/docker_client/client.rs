use std::{str::FromStr, time::{Instant, Duration}, ops::Add};

use reqwest::RequestBuilder;
use serde::Deserialize;
use tracing::{info, warn};

use crate::docker_client::www_authenticate::AuthenticationChallenge;

use super::www_authenticate::WwwAuthenticateError;

enum AuthenticationType {
    Basic { username: String, password: Option<String> },
    Token { scope: String, token: String, expires_at: Instant },
    Anonymous
}

#[derive(thiserror::Error, Debug)]
pub enum DockerClientError {
    #[error("Unexpected status code {0}")]
    UnexpectedStatusCode(u16),

    #[error("Unsupported authentication method {0}")]
    UnsupportedAuthenticationMethod(String),

    #[error("Provided HTTP Basic credentials are errorenous or not provided")]
    BadBasicAuthCredentials,

    #[error(transparent)]
    WwwAuthenticateParseError(#[from] WwwAuthenticateError),

    #[error(transparent)]
    ReqwestError(#[from] reqwest::Error)
}

#[derive(Deserialize)]
struct BearerToken {
    token: String,
    expires_in: Option<u64>
}

pub struct DockerClient {
    authentication: Option<AuthenticationType>,
    registry: String,
    container: String,
    http_client: reqwest::Client
}

impl DockerClient {
    pub fn new(registry: &str, container: &str) -> Self {
        let client = reqwest::Client::new();

        Self {
            authentication: None,
            registry: registry.to_string(),
            container: container.to_string(),
            http_client: client
        }
    }

    pub async fn authenticate(&mut self, registry_username: Option<&str>, registry_password: Option<&str>) -> Result<(), DockerClientError> {
        if self.authentication.is_some() {
            return Ok(());
        }

        // Fetch the base and see what the authorization header has to say
        info!("Discovering authentication strategies for the registry {}", self.registry);

        let url = url::Url::from_str(&format!("https://{}/v2/", self.registry)).unwrap();
        let base_response = self.http_client.get(url).send().await.unwrap();

        // If the server responds 200 immediately, we'll consider we don't need authentication.
        if base_response.status() == 200 {
            info!("Got 200, assuming repository can be accessed without any credentials");
            self.authentication = Some(AuthenticationType::Anonymous);
            return Ok(());
        }

        // The next thing we probably will have a 401 Unauthorized code with a WWW-Authenticate header.
        // We don't care about the rest.
        if base_response.status() != 401 {
            warn!("Got a response with status {}, expected 401", base_response.status());
            return Err(DockerClientError::UnexpectedStatusCode(base_response.status().as_u16()));
        }

        // This will be a crude parser. It DOES NOT support registries with multiple challenges and WILL be thrown off
        // if a registry sends multiple challenges.
        let www_authenticate = base_response.headers()
            .get("WWW-Authenticate")
            .expect("If we received a 401, we should have a WWW-Authenticate header. What's the point otherwise ?")
            .to_str()
            .expect("The header should contain only UTF8 characters");
        info!("Got authentication challenge header [{}]", www_authenticate);

        let auth_challenge = AuthenticationChallenge::from_www_authenticate(&www_authenticate)?;

        match auth_challenge {
            AuthenticationChallenge::Basic(_params) if registry_username.is_some() => {
                info!("Applying HTTP Basic for registry {}", self.registry);

                self.authentication = Some(
                    AuthenticationType::Basic {
                        username: registry_username.unwrap().to_string(),
                        password: registry_password.map(|p| p.to_string())
                    }
                );

                if let Err(auth_err) = self.check_authentication().await {
                    self.authentication = None;
                    return Err(auth_err);
                }
            },

            AuthenticationChallenge::Basic(_) => {
                warn!("No provided credential for auth method Basic");
                return Err(DockerClientError::BadBasicAuthCredentials);
            }

            AuthenticationChallenge::Bearer(mut params) => {
                let scope = format!("repository:{}:pull", self.container);
                params.insert("scope", &scope);

                let authentication_service = params.get("realm").expect("Who am I supposed to authenticate to ?");
                let authentication_query_string = params.iter()
                    .filter(|(key, _)| **key != "realm")
                    .map(|(k, v)| [*k, *v].join("="))
                    .collect::<Vec<_>>()
                    .join("&");

                info!("Attempting to authenticate to {}", authentication_service);
                let mut token_request = self.http_client.get(format!("{}?{}", authentication_service, authentication_query_string));
                if let Some(username) = registry_username {
                    token_request = token_request.basic_auth(username, registry_password);
                }
                let response = token_request.send().await?;
                if response.status() != 200 {
                    info!("Response is not 200");
                    return Err(DockerClientError::UnexpectedStatusCode(response.status().as_u16()));
                }
                info!("Deserializing 200 response from {}", authentication_service);
                let token = response.json::<BearerToken>().await?;
                if token.token.is_empty() || token.token == "unauthenticated" {
                    return Err(DockerClientError::BadBasicAuthCredentials);
                }
                self.authentication = Some(
                    AuthenticationType::Token { 
                        scope, 
                        token: token.token, 
                        expires_at: Instant::now().add(Duration::from_secs(token.expires_in.unwrap_or(60))) 
                    }
                );
                info!("Checking token");
                if let Err(err) = self.check_authentication().await {
                    info!("Apparently, invalid token");
                    self.authentication = None;
                    return Err(err);
                }
            }
        }

        Ok(())
    }

    pub async fn query_base(&self) -> Result<(), DockerClientError> {
        let query = self.http_client.get(format!("https://{}/v2/", self.registry));
        let query = self.add_authentication(query);
        let response = query.send().await?;

        if response.status() != 200 {
            return Err(DockerClientError::UnexpectedStatusCode(response.status().as_u16()));
        }

        Ok(())
    }

    fn add_authentication(&self, request: RequestBuilder) -> RequestBuilder {
        match self.authentication {
            Some(AuthenticationType::Basic { ref username, ref password  }) => {
                request.basic_auth(username, password.clone())
            },

            Some(AuthenticationType::Token { ref token, .. }) => {
                request.bearer_auth(token)
            }

            Some(AuthenticationType::Anonymous) | None => request,
        }
    }

    async fn check_authentication(&self) -> Result<(), DockerClientError>{
        let response = self.query_base().await;

        match response {
            Err(DockerClientError::UnexpectedStatusCode(code)) if code == 401 => {
                warn!("Invalid credentials");
                return Err(DockerClientError::BadBasicAuthCredentials);
            },

            Err(other_error) => {
                warn!("Other client error: {:?}", other_error);
                return Err(other_error);
            },

            Ok(_) => {
                info!("Provided credentials are OK");
                return Ok(())
            },
        }
    }
}