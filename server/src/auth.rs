use axum::async_trait;
use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::http::StatusCode;

use crate::error::ApiError;
use crate::state::AppState;

/// The authenticated caller, extracted from a `Bearer` JWT. Any handler that
/// takes this argument is automatically protected.
pub struct AuthUser {
    pub username: String,
    #[allow(dead_code)]
    pub role: String,
}

#[async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let header_token = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .map(str::to_owned);

        // `<img src>` / `EventSource` can't set headers, so image and stream
        // routes pass the JWT as `?token=` instead.
        let token = match header_token {
            Some(t) => t,
            None => query_token(parts.uri.query())
                .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "missing bearer token".into()))?,
        };

        let claims = easyclean_core::auth::verify_token(&token, &state.cfg.secret)
            .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "invalid or expired token".into()))?;

        Ok(AuthUser { username: claims.sub, role: claims.role })
    }
}

/// Pull `token=<jwt>` out of a query string. JWTs are URL-safe, so no decoding.
fn query_token(query: Option<&str>) -> Option<String> {
    query?
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::query_token;

    #[test]
    fn extracts_token_from_query() {
        assert_eq!(query_token(Some("token=abc.def.ghi")).as_deref(), Some("abc.def.ghi"));
        assert_eq!(query_token(Some("size=5&token=xyz")).as_deref(), Some("xyz"));
        assert_eq!(query_token(Some("size=5")), None);
        assert_eq!(query_token(Some("token=")), None);
        assert_eq!(query_token(None), None);
    }
}
