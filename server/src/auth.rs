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

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
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

        // A vault token is signed with the same key, so it would otherwise pass
        // here and let a 15-minute unlock browse the whole library.
        if claims.role == easyclean_core::auth::VAULT_ROLE {
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "vault token is not a login".into(),
            ));
        }

        Ok(AuthUser {
            username: claims.sub,
            role: claims.role,
        })
    }
}

/// A caller who has entered the vault PIN within the token's lifetime.
///
/// Its own extractor rather than a flag on `AuthUser`, so a vault route cannot
/// be written by accident: forgetting the PIN check means writing a handler that
/// takes `AuthUser`, and that handler has no way to reach a vaulted row —
/// `db::vault_file` only returns files whose status is already `vault`.
pub struct VaultUser {
    pub username: String,
}

#[async_trait]
impl FromRequestParts<AppState> for VaultUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        // Its own header, so an unlocked vault and an ordinary session can be
        // held at once: the client keeps sending Authorization for everything
        // else and adds this only for vault routes. Thumbnails go through
        // `?vault_token=` for the same reason image routes use `?token=`.
        let token = parts
            .headers
            .get("x-vault-token")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
            .or_else(|| named_token(parts.uri.query(), "vault_token="))
            .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "vault is locked".into()))?;

        let claims = easyclean_core::auth::verify_token(&token, &state.cfg.secret)
            .ok_or_else(|| ApiError(StatusCode::UNAUTHORIZED, "vault is locked".into()))?;

        if claims.role != easyclean_core::auth::VAULT_ROLE {
            return Err(ApiError(
                StatusCode::UNAUTHORIZED,
                "not a vault token".into(),
            ));
        }

        Ok(VaultUser {
            username: claims.sub,
        })
    }
}

/// Pull `token=<jwt>` out of a query string. JWTs are URL-safe, so no decoding.
fn query_token(query: Option<&str>) -> Option<String> {
    named_token(query, "token=")
}

/// Pull `<prefix><jwt>` out of a query string.
///
/// The prefix is matched on a whole pair, so looking for `token=` cannot match
/// the tail of `vault_token=` and hand a vault token to the ordinary extractor.
fn named_token(query: Option<&str>, prefix: &str) -> Option<String> {
    query?
        .split('&')
        .find_map(|pair| pair.strip_prefix(prefix))
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::{named_token, query_token};

    /// `token=` must not match inside `vault_token=`: if it did, a request
    /// carrying only a vault token would be accepted as an ordinary login.
    #[test]
    fn a_vault_token_is_not_mistaken_for_a_login_token() {
        assert_eq!(query_token(Some("vault_token=abc")), None);
        assert_eq!(
            named_token(Some("vault_token=abc"), "vault_token=").as_deref(),
            Some("abc")
        );
        assert_eq!(
            query_token(Some("vault_token=abc&token=xyz")).as_deref(),
            Some("xyz")
        );
    }

    #[test]
    fn extracts_token_from_query() {
        assert_eq!(
            query_token(Some("token=abc.def.ghi")).as_deref(),
            Some("abc.def.ghi")
        );
        assert_eq!(
            query_token(Some("size=5&token=xyz")).as_deref(),
            Some("xyz")
        );
        assert_eq!(query_token(Some("size=5")), None);
        assert_eq!(query_token(Some("token=")), None);
        assert_eq!(query_token(None), None);
    }
}
