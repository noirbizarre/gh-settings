//! The [`GitHubClient`] port.
//!
//! This is the single seam between resources and GitHub. Everything a resource
//! needs to do is expressible here, and nothing here leaks the transport: there is
//! no mention of processes, sockets or headers in the signatures a resource sees.

use async_trait::async_trait;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::github::{GitHubError, Method, Result};

/// A single API call.
#[derive(Debug, Clone)]
pub struct Request {
    /// HTTP method.
    pub method: Method,
    /// Endpoint relative to the API root, e.g. `repos/o/r/labels`.
    pub endpoint: String,
    /// JSON request body, if any.
    pub body: Option<Value>,
    /// Whether every page should be fetched and concatenated.
    pub paginate: bool,
    /// Extra request headers, e.g. a preview `Accept` value.
    pub headers: Vec<(String, String)>,
    /// Whether the response body is text rather than JSON.
    ///
    /// A handful of endpoints — reading a file's contents, notably — can return
    /// the bytes themselves. Decoding those as JSON fails, so the transport has
    /// to be told not to.
    pub raw: bool,
}

impl Request {
    /// Start building a request.
    pub fn new(method: Method, endpoint: impl Into<String>) -> Self {
        Self {
            method,
            endpoint: endpoint.into(),
            body: None,
            paginate: false,
            headers: Vec::new(),
            raw: false,
        }
    }

    /// A `GET`.
    pub fn get(endpoint: impl Into<String>) -> Self {
        Self::new(Method::Get, endpoint)
    }

    /// A `GET` that walks every page.
    pub fn list(endpoint: impl Into<String>) -> Self {
        Self::get(endpoint).paginated()
    }

    /// A `POST` with a JSON body.
    pub fn post(endpoint: impl Into<String>, body: impl Serialize) -> Self {
        Self::new(Method::Post, endpoint).with_body(body)
    }

    /// A `PATCH` with a JSON body.
    pub fn patch(endpoint: impl Into<String>, body: impl Serialize) -> Self {
        Self::new(Method::Patch, endpoint).with_body(body)
    }

    /// A `PUT` with a JSON body.
    pub fn put(endpoint: impl Into<String>, body: impl Serialize) -> Self {
        Self::new(Method::Put, endpoint).with_body(body)
    }

    /// A `DELETE`.
    pub fn delete(endpoint: impl Into<String>) -> Self {
        Self::new(Method::Delete, endpoint)
    }

    /// Attach a JSON body.
    ///
    /// Serialization is infallible for our own types; a failure here is a bug, so
    /// it degrades to `null` rather than complicating every call site.
    pub fn with_body(mut self, body: impl Serialize) -> Self {
        self.body = Some(serde_json::to_value(body).unwrap_or(Value::Null));
        self
    }

    /// Request every page.
    pub fn paginated(mut self) -> Self {
        self.paginate = true;
        self
    }

    /// Ask for the body verbatim rather than as JSON.
    ///
    /// Sets the media type GitHub uses to return a file's own bytes, and tells
    /// the transport not to try to decode them.
    pub fn raw(mut self) -> Self {
        self.raw = true;
        self.headers
            .push(("Accept".into(), "application/vnd.github.raw".into()));
        self
    }

    /// Add a request header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
}

/// The outcome of an API call.
#[derive(Debug, Clone)]
pub struct Response {
    /// HTTP status code.
    pub status: u16,
    /// Decoded JSON body. `Null` for empty (`204`) and raw responses.
    pub body: Value,
    /// The body verbatim, for requests that asked for it raw.
    pub text: Option<String>,
    /// Response headers we care about, lowercased.
    ///
    /// Notably `x-oauth-scopes`, which is the only reliable way to enumerate the
    /// scopes of a classic personal access token.
    pub headers: Vec<(String, String)>,
}

impl Response {
    /// A JSON response, which is all but a handful of endpoints.
    pub fn json(status: u16, body: Value, headers: Vec<(String, String)>) -> Self {
        Self {
            status,
            body,
            text: None,
            headers,
        }
    }

    /// The body verbatim, for a request that asked for it raw.
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    /// Look up a response header, case-insensitively.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// The port every resource depends on.
///
/// Deliberately kept object safe: the engine stores resources as trait objects and
/// hands them a `&dyn GitHubClient`. The typed, generic helpers live in
/// [`GitHubClientExt`], which is blanket-implemented and so is available on `dyn
/// GitHubClient` too.
#[async_trait]
pub trait GitHubClient: Send + Sync {
    /// Perform a request.
    async fn request(&self, request: Request) -> Result<Response>;
}

/// Typed conveniences over [`GitHubClient`].
///
/// Separated out because generic methods are not object safe; this trait is
/// blanket-implemented for every client, including unsized ones.
#[async_trait]
pub trait GitHubClientExt: GitHubClient {
    /// Perform a request and decode its body.
    async fn send<T: DeserializeOwned>(&self, request: Request) -> Result<T> {
        let endpoint = request.endpoint.clone();
        let response = self.request(request).await?;
        serde_json::from_value(response.body)
            .map_err(|source| GitHubError::Decode { endpoint, source })
    }

    /// Perform a request, mapping `404` to `None`.
    ///
    /// Most "current state" reads need this: a repository with no rulesets and a
    /// repository we cannot see are different situations, but a missing *optional*
    /// sub-resource is routinely a `404` and is not an error.
    async fn send_optional<T: DeserializeOwned>(&self, request: Request) -> Result<Option<T>> {
        match self.send::<T>(request).await {
            Ok(value) => Ok(Some(value)),
            Err(err) if err.is_not_found() => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Perform a request, discarding the body.
    async fn execute(&self, request: Request) -> Result<()> {
        self.request(request).await.map(|_| ())
    }
}

impl<T: GitHubClient + ?Sized> GitHubClientExt for T {}

// Blanket impl so `&dyn GitHubClient`, `Box<dyn GitHubClient>` and `Arc<_>` all
// satisfy the bound without every call site having to deref.
#[async_trait]
impl<T: GitHubClient + ?Sized> GitHubClient for &T {
    async fn request(&self, request: Request) -> Result<Response> {
        (**self).request(request).await
    }
}

#[async_trait]
impl<T: GitHubClient + ?Sized> GitHubClient for std::sync::Arc<T> {
    async fn request(&self, request: Request) -> Result<Response> {
        (**self).request(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builds_requests() {
        let request = Request::post("repos/o/r/labels", json!({"name": "bug"}));
        assert_eq!(request.method, Method::Post);
        assert_eq!(request.endpoint, "repos/o/r/labels");
        assert_eq!(request.body, Some(json!({"name": "bug"})));
        assert!(!request.paginate);
    }

    #[test]
    fn a_raw_request_asks_for_the_bytes_themselves() {
        let request = Request::get("repos/o/r/contents/x.yml").raw();
        assert!(request.raw);
        assert!(
            request
                .headers
                .iter()
                .any(|(name, value)| name == "Accept" && value == "application/vnd.github.raw"),
            "{:?}",
            request.headers
        );
    }

    #[test]
    fn lists_are_paginated_by_default() {
        assert!(Request::list("repos/o/r/labels").paginate);
        assert!(!Request::get("repos/o/r").paginate);
    }

    #[test]
    fn header_lookup_ignores_case() {
        let response = Response::json(
            200,
            Value::Null,
            vec![("X-OAuth-Scopes".into(), "repo, read:org".into())],
        );
        assert_eq!(response.header("x-oauth-scopes"), Some("repo, read:org"));
        assert_eq!(response.header("missing"), None);
    }
}
