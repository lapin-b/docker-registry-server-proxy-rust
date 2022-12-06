use std::borrow::Cow;

use serde::Serialize;

#[derive(Serialize)]
pub struct RegistryJsonError<'a> {
    errors: Vec<InnerRegistryJsonError<'a>>
}

#[derive(Serialize)]
struct InnerRegistryJsonError<'a> {
    code: Cow<'a, str>,
    message: Cow<'a, str>,
    detail: Cow<'a, str>
}

impl<'a> RegistryJsonError<'a> {
    pub fn new(code: &'a str, message: &'a str, detail: &'a str) -> Self {
        let inner = InnerRegistryJsonError {
            code: Cow::from(code),
            message: Cow::from(message),
            detail: Cow::from(detail),
        };

        Self {
            errors: vec![inner]
        }
    }
}