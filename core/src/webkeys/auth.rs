/// Bearer token authentication for the `/v1/*` OpenAI-compatible surface.
///
/// Validates `Authorization: Bearer sk-vt-...` headers against the
/// WebKeysService virtual key store. Returns the public key record on success.
use axum::http::HeaderMap;

use crate::webkeys::{
    service::WebKeysService,
    types::{OpenAiErrorBody, OpenAiErrorEnvelope, VirtualKeyPublicRecord},
};

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("missing Authorization header")]
    MissingAuthorization,
    #[error("invalid Authorization scheme")]
    InvalidScheme,
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("key disabled or rate limited: {0}")]
    Denied(String),
}

impl AuthError {
    pub fn as_openai_error(&self) -> OpenAiErrorEnvelope {
        let code = match self {
            AuthError::MissingAuthorization => "missing_auth",
            AuthError::InvalidScheme => "invalid_auth_scheme",
            AuthError::InvalidCredentials => "invalid_api_key",
            AuthError::Denied(_) => "access_denied",
        };
        OpenAiErrorEnvelope {
            error: OpenAiErrorBody {
                message: self.to_string(),
                r#type: "invalid_request_error".to_string(),
                code: code.to_string(),
            },
        }
    }
}

/// Authenticate an incoming request by verifying its `sk-vt-*` bearer token.
/// Returns the public virtual key record on success.
pub async fn authenticate_bearer(
    headers: &HeaderMap,
    service: &WebKeysService,
) -> Result<VirtualKeyPublicRecord, AuthError> {
    let auth_value = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .ok_or(AuthError::MissingAuthorization)?;

    let mut parts = auth_value.splitn(2, ' ');
    let scheme = parts.next().unwrap_or_default();
    let token = parts.next().unwrap_or_default().trim();

    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err(AuthError::InvalidScheme);
    }

    if token.is_empty() {
        return Err(AuthError::InvalidCredentials);
    }

    // verify_virtual_key returns (public_record, binding_id)
    service
        .verify_virtual_key(token, None)
        .await
        .map(|(record, _binding_id)| record)
        .map_err(|e| {
            // Service returns human-readable strings; map to auth errors
            let msg = e.to_lowercase();
            if msg.contains("rate limit") {
                AuthError::Denied(e)
            } else {
                AuthError::InvalidCredentials
            }
        })
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue};

    use super::{AuthError, authenticate_bearer};
    use crate::webkeys::{
        service::WebKeysService,
        types::{CreateProviderBindingRequest, CreateVirtualKeyRequest, WebProviderType},
    };

    async fn service_with_key() -> (WebKeysService, String) {
        let svc = WebKeysService::new_in_memory().await;
        let binding = svc
            .create_provider_binding(CreateProviderBindingRequest {
                provider_type: WebProviderType::GeminiWeb,
                account_id: "test@example.com".to_string(),
                display_name: "Test".to_string(),
                profile_path: "profiles/gemini".to_string(),
            })
            .await
            .unwrap();
        let result = svc
            .create_virtual_key(CreateVirtualKeyRequest {
                name: "auth-test".to_string(),
                provider_binding_ids: vec![binding.id],
                default_binding_id: None,
                model_allowlist: None,
                metadata: None,
                rate_limit_rpm: None,
                rate_limit_tpm: None,
            })
            .await
            .unwrap();
        (svc, result.raw_key)
    }

    #[tokio::test]
    async fn validates_bearer_token() {
        let (svc, raw_key) = service_with_key().await;
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {}", raw_key)).unwrap(),
        );
        assert!(authenticate_bearer(&headers, &svc).await.is_ok());
    }

    #[tokio::test]
    async fn rejects_missing_header() {
        let (svc, _) = service_with_key().await;
        let headers = HeaderMap::new();
        assert!(matches!(
            authenticate_bearer(&headers, &svc).await,
            Err(AuthError::MissingAuthorization)
        ));
    }

    #[tokio::test]
    async fn rejects_wrong_scheme() {
        let (svc, raw_key) = service_with_key().await;
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_str(&format!("Token {}", raw_key)).unwrap(),
        );
        assert!(matches!(
            authenticate_bearer(&headers, &svc).await,
            Err(AuthError::InvalidScheme)
        ));
    }

    #[tokio::test]
    async fn rejects_invalid_key() {
        let (svc, _) = service_with_key().await;
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_str("Bearer sk-vt-invalid000000000000000000000000").unwrap(),
        );
        assert!(matches!(
            authenticate_bearer(&headers, &svc).await,
            Err(AuthError::InvalidCredentials)
        ));
    }
}
