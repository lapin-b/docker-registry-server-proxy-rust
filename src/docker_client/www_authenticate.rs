use std::collections::HashMap;

use once_cell::sync::Lazy;
use regex::Regex;

static WWW_AUTHENTICATE_HEADER_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"((?P<method>[A-Za-z]+)\s)?(?P<key>[A-Za-z]+)\s*=\s*"(?P<value>[^"]+)""#).unwrap()
});

#[derive(thiserror::Error, Debug)]
pub enum WwwAuthenticateError {
    #[error("Missing authentication method in header")]
    MissingMethod,
    #[error("Unsupported authentication method {0}")]
    UnsupportedMethod(String)
}

pub enum AuthenticationChallenge<'auth> {
    Basic(HashMap<&'auth str, &'auth str>),
    Bearer(HashMap<&'auth str, &'auth str>)
}

impl<'auth> AuthenticationChallenge<'auth> {
    pub fn from_www_authenticate(header_value: &'auth str) -> Result<Self, WwwAuthenticateError> {
        let mut auth_strategy = None;
        let mut auth_parameters = HashMap::new();

        for capture in WWW_AUTHENTICATE_HEADER_REGEX.captures_iter(header_value) {
            if let Some(method) = capture.name("method") {
                auth_strategy = Some(method.as_str().to_lowercase());
            }

            auth_parameters.insert(capture.name("key").unwrap().as_str(), capture.name("value").unwrap().as_str());
        }

        match auth_strategy {
            Some(method) if method == "basic" => Ok(AuthenticationChallenge::Basic(auth_parameters)),
            Some(method) if method == "bearer" => Ok(AuthenticationChallenge::Bearer(auth_parameters)),
            Some(method) => Err(WwwAuthenticateError::UnsupportedMethod(method)),
            None => Err(WwwAuthenticateError::MissingMethod)
        }
    }

    pub fn authentication_parameters(&self) -> &HashMap<&'auth str, &'auth str> {
        match self {
            AuthenticationChallenge::Basic(ref params) => params,
            AuthenticationChallenge::Bearer(ref params) => params,
        }
    }
}