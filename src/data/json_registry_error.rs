use serde::Serialize;

#[derive(Serialize)]
pub struct RegistryJsonErrorReprWrapper {
    errors: Vec<RegistryJsonErrorRepr>,
}

impl RegistryJsonErrorReprWrapper {
    pub fn single<C, M, D>(code: C, message: M, detail: D) -> Self
    where
        C: ToString,
        M: ToString,
        D: ToString,
    {
        let inner = RegistryJsonErrorRepr::new(code, message, detail);

        Self {
            errors: vec![inner],
        }
    }

    #[allow(dead_code)]
    pub fn multiple(errors: &[RegistryJsonErrorRepr]) -> Self {
        Self {
            errors: errors.to_vec(),
        }
    }
}

#[derive(Serialize, Clone)]
pub struct RegistryJsonErrorRepr {
    code: String,
    message: String,
    detail: String,
}

impl RegistryJsonErrorRepr {
    pub fn new<C, M, D>(code: C, message: M, detail: D) -> Self
    where
        C: ToString,
        M: ToString,
        D: ToString,
    {
        Self {
            code: code.to_string(),
            message: message.to_string(),
            detail: detail.to_string(),
        }
    }
}
