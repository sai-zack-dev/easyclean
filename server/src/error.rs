use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// What a 5xx tells the client. Deliberately the same sentence every time.
///
/// The detail belongs in the server log, not in the response. `core::Error`
/// wraps `sqlx::Error`, `io::Error` and the model loader's own messages, and
/// `From` below turns any of them into a 500 — so before this, an error body
/// could carry SQL text, a `<data_dir>` path or the CLIP mirror's URL. Worse,
/// `login` reaches the database before it has authenticated anyone, which put
/// that channel in front of an unauthenticated caller.
const INTERNAL_MSG: &str = "internal server error";

/// An API error that renders as `{ "error": "..." }` with a status code.
pub struct ApiError(pub StatusCode, pub String);

impl ApiError {
    pub fn not_found(msg: &str) -> Self {
        ApiError(StatusCode::NOT_FOUND, msg.to_string())
    }
    pub fn bad(msg: &str) -> Self {
        ApiError(StatusCode::BAD_REQUEST, msg.to_string())
    }

    /// Something broke that the caller can do nothing about.
    ///
    /// The cause is logged and then dropped: a 500 is not a place to explain
    /// the server's internals to whoever provoked it. Messages written by hand
    /// for a 4xx — "an album needs a name" — are the opposite case and are
    /// passed through untouched.
    pub fn internal<E: std::fmt::Display>(e: E) -> Self {
        tracing::error!(detail = %e, "request failed");
        ApiError(StatusCode::INTERNAL_SERVER_ERROR, INTERNAL_MSG.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        // Scrubbed here as well as in `internal`, so the guarantee holds for any
        // 5xx however it was built — including one a later handler constructs
        // directly with `ApiError(StatusCode::…, detail)`. There is nowhere else
        // a response body is produced, which makes this the one place that has
        // to be right.
        let body = if self.0.is_server_error() {
            INTERNAL_MSG
        } else {
            self.1.as_str()
        };
        (self.0, Json(json!({ "error": body }))).into_response()
    }
}

impl From<easyclean_core::Error> for ApiError {
    fn from(e: easyclean_core::Error) -> Self {
        ApiError::internal(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    async fn body_of(e: ApiError) -> String {
        let resp = e.into_response();
        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// The case that was reachable without logging in: `login` queries the
    /// database before it authenticates anyone, so a `sqlx` error went straight
    /// out to whoever posted the request.
    #[tokio::test]
    async fn a_wrapped_core_error_says_nothing_about_the_internals() {
        let e = ApiError::from(easyclean_core::Error::Auth(
            "no such column: pass_hash in SELECT FROM user".into(),
        ));
        let body = body_of(e).await;
        assert!(!body.contains("pass_hash"), "leaked the schema: {body}");
        assert!(!body.contains("SELECT"), "leaked the query: {body}");
        assert_eq!(body, r#"{"error":"internal server error"}"#);
    }

    /// Paths and URLs are the other half of it — the model loader's errors
    /// carry `<data_dir>` and whatever `EASYCLEAN_CLIP_BASE_URL` points at.
    #[tokio::test]
    async fn internal_details_are_not_on_the_wire() {
        let body = body_of(ApiError::internal(
            "loading vision.onnx: /data/models/clip-vit-b32-int8/vision.onnx not found",
        ))
        .await;
        assert!(!body.contains("/data/"), "leaked a server path: {body}");
        assert!(!body.contains("vision.onnx"), "leaked a filename: {body}");
    }

    /// Belt and braces: a 5xx built by hand, bypassing `internal`, is scrubbed
    /// by `into_response` too. This is what keeps the guarantee true for code
    /// nobody has written yet.
    #[tokio::test]
    async fn a_hand_built_5xx_is_scrubbed_as_well() {
        let e = ApiError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "/srv/photos/saiz/source/IMG_4821.heic".into(),
        );
        let body = body_of(e).await;
        assert!(!body.contains("/srv/photos"), "leaked a media path: {body}");
    }

    /// And the messages written deliberately for the user still arrive, or the
    /// fix would have traded a leak for an app that cannot explain itself.
    #[tokio::test]
    async fn deliberate_client_messages_still_reach_the_client() {
        assert!(body_of(ApiError::bad("an album needs a name"))
            .await
            .contains("an album needs a name"));
        assert!(body_of(ApiError::not_found("media not found"))
            .await
            .contains("media not found"));
        assert!(body_of(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many PIN attempts — try again later".into()
        ))
        .await
        .contains("too many PIN attempts"));
    }
}
