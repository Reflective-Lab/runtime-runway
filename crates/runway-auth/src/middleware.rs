use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::{
    extract::Request,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use tower::{Layer, Service};

use crate::firebase::{FirebaseAuth, FirebaseClaims};

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("missing Authorization header")]
    MissingHeader,
    #[error("malformed Authorization header")]
    MalformedHeader,
    #[error("invalid token: {0}")]
    InvalidToken(String),
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let status = match &self {
            AuthError::MissingHeader | AuthError::MalformedHeader => StatusCode::UNAUTHORIZED,
            AuthError::InvalidToken(_) => StatusCode::UNAUTHORIZED,
        };
        (status, self.to_string()).into_response()
    }
}

/// Decoded auth context extracted from the Firebase ID token.
/// Injected into Axum handlers via `Extension<AuthContext>`.
#[derive(Debug, Clone)]
pub struct AuthContext {
    pub claims: FirebaseClaims,
}

impl AuthContext {
    pub fn uid(&self) -> &str {
        &self.claims.uid
    }

    pub fn org_id(&self) -> Option<&str> {
        self.claims.org_id.as_deref()
    }

    pub fn has_app(&self, app: &str) -> bool {
        self.claims.has_app(app)
    }

    pub fn is_admin(&self) -> bool {
        matches!(self.claims.role.as_deref(), Some("admin"))
    }
}

/// Tower layer that validates a Firebase Bearer token and injects `AuthContext`.
#[derive(Clone)]
pub struct AuthLayer {
    auth: Arc<FirebaseAuth>,
    /// If set, only allow requests where the org has access to this app.
    required_app: Option<String>,
}

impl AuthLayer {
    pub fn new(auth: FirebaseAuth) -> Self {
        Self {
            auth: Arc::new(auth),
            required_app: None,
        }
    }

    pub fn requiring_app(mut self, app: impl Into<String>) -> Self {
        self.required_app = Some(app.into());
        self
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthMiddleware {
            inner,
            auth: self.auth.clone(),
            required_app: self.required_app.clone(),
        }
    }
}

#[derive(Clone)]
pub struct AuthMiddleware<S> {
    inner: S,
    auth: Arc<FirebaseAuth>,
    required_app: Option<String>,
}

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

impl<S> Service<Request> for AuthMiddleware<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = BoxFuture<Result<Response, S::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request) -> Self::Future {
        let auth = self.auth.clone();
        let required_app = self.required_app.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let token = extract_bearer(req.headers());
            let token = match token {
                Ok(t) => t,
                Err(e) => return Ok(e.into_response()),
            };

            // In LOCAL_DEV mode, accept "dev" as a bypass token and inject a canned context.
            let claims = if std::env::var("LOCAL_DEV").as_deref() == Ok("true") && token == "dev" {
                FirebaseClaims {
                    uid: "dev-uid".into(),
                    email: Some("dev@local".into()),
                    org_id: Some("dev-org".into()),
                    apps: vec!["api-server".into()],
                    role: Some("admin".into()),
                }
            } else {
                match auth.verify(&token).await {
                    Ok(c) => c,
                    Err(e) => return Ok(AuthError::InvalidToken(e.to_string()).into_response()),
                }
            };

            if let Some(app) = &required_app
                && !claims.has_app(app)
            {
                return Ok((StatusCode::FORBIDDEN, "app not in subscription").into_response());
            }

            req.extensions_mut().insert(AuthContext { claims });
            inner.call(req).await
        })
    }
}

fn extract_bearer(headers: &axum::http::HeaderMap) -> Result<String, AuthError> {
    let header = headers
        .get(axum::http::header::AUTHORIZATION)
        .ok_or(AuthError::MissingHeader)?
        .to_str()
        .map_err(|_| AuthError::MalformedHeader)?;

    let token = header
        .strip_prefix("Bearer ")
        .ok_or(AuthError::MalformedHeader)?
        .to_string();

    if token.is_empty() {
        return Err(AuthError::MalformedHeader);
    }
    Ok(token)
}
