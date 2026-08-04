//! Reading a base configuration from another repository.
//!
//! Implements [`BaseLoader`] over the GitHub transport, which is why it lives
//! here rather than in `config`: the configuration layer is the innermost one
//! and must not know that GitHub exists.

use async_trait::async_trait;

use crate::config::{BaseLoader, LoadedBase, Reference};
use crate::github::client::{GitHubClient, Request};

/// Loads a base configuration through the GitHub API.
pub struct GitHubBaseLoader<'a> {
    client: &'a dyn GitHubClient,
}

impl<'a> GitHubBaseLoader<'a> {
    /// Read base documents through this client.
    pub fn new(client: &'a dyn GitHubClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl BaseLoader for GitHubBaseLoader<'_> {
    async fn load(&self, reference: &Reference) -> Result<LoadedBase, String> {
        let endpoint = format!(
            "repos/{}/{}/contents/{}?ref={}",
            reference.owner, reference.repository, reference.path, reference.git_ref
        );

        // Asked for raw rather than as the usual JSON envelope, which would
        // return the document base64-encoded and require a decoder for no gain.
        let response = self
            .client
            .request(Request::get(&endpoint).raw())
            .await
            .map_err(|error| explain(reference, &error))?;

        let text = response
            .text()
            .ok_or_else(|| format!("`{reference}` returned no content"))?
            .to_string();

        // The ETag is the blob SHA GitHub served, so recording it costs no extra
        // request. A saved plan uses it to tell "the base moved" apart from "the
        // repository drifted".
        let commit = response
            .header("etag")
            .map(|etag| etag.trim_matches(['"', 'W', '/'].as_slice()).to_string());

        Ok(LoadedBase { text, commit })
    }
}

/// Turn a transport failure into something that names the base.
///
/// A bare `404` here is confusing: the repository the user is configuring exists
/// perfectly well, and the one that does not is the one they inherited from.
fn explain(reference: &Reference, error: &crate::github::GitHubError) -> String {
    if error.is_not_found() {
        return format!(
            "`{reference}` could not be read: no such repository, ref or path, \
             or this token cannot see it"
        );
    }
    if error.is_permission_denied() {
        return format!(
            "`{reference}` could not be read: this token is not allowed to read that repository. \
             Reading a base configuration needs `contents: read` on it, which the Actions \
             GITHUB_TOKEN does not have outside its own repository"
        );
    }
    format!("`{reference}` could not be read: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::client::Response;
    use pretty_assertions::assert_eq;
    use std::sync::Mutex;

    /// Records the requests it is given and replays one canned response.
    struct Stub {
        response: Result<Response, crate::github::GitHubError>,
        seen: Mutex<Vec<Request>>,
    }

    #[async_trait]
    impl GitHubClient for Stub {
        async fn request(&self, request: Request) -> Result<Response, crate::github::GitHubError> {
            self.seen.lock().unwrap().push(request);
            match &self.response {
                Ok(response) => Ok(response.clone()),
                Err(error) => Err(api_error(error.status().unwrap_or(500))),
            }
        }
    }

    fn api_error(status: u16) -> crate::github::GitHubError {
        crate::github::GitHubError::Api {
            method: crate::github::Method::Get,
            endpoint: "repos/acme/.github/contents/.github/settings.yml".into(),
            status,
            message: "nope".into(),
            body: String::new(),
        }
    }

    fn reference() -> Reference {
        "acme/.github@v1".parse().expect("valid")
    }

    #[tokio::test]
    async fn reads_the_document_at_the_pinned_ref() {
        let stub = Stub {
            response: Ok(Response {
                status: 200,
                body: serde_json::Value::Null,
                text: Some("version: 1\n".into()),
                headers: vec![("etag".into(), "\"abc123\"".into())],
            }),
            seen: Mutex::new(Vec::new()),
        };

        let loaded = GitHubBaseLoader::new(&stub)
            .load(&reference())
            .await
            .expect("loaded");

        assert_eq!(loaded.text, "version: 1\n");
        assert_eq!(loaded.commit.as_deref(), Some("abc123"));

        let seen = stub.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(
            seen[0].endpoint, "repos/acme/.github/contents/.github/settings.yml?ref=v1",
            "the doubled `.github` is the repository and then the directory"
        );
        assert!(seen[0].raw, "the document is text, not JSON");
    }

    #[tokio::test]
    async fn a_missing_base_says_which_base() {
        // A bare 404 would suggest the repository being configured is missing,
        // when it is the inherited one that cannot be read.
        let stub = Stub {
            response: Err(api_error(404)),
            seen: Mutex::new(Vec::new()),
        };
        let message = GitHubBaseLoader::new(&stub)
            .load(&reference())
            .await
            .unwrap_err();

        assert!(message.contains("acme/.github@v1"), "{message}");
        assert!(message.contains("no such repository"), "{message}");
    }

    #[tokio::test]
    async fn a_forbidden_base_names_the_permission_it_needs() {
        let stub = Stub {
            response: Err(api_error(403)),
            seen: Mutex::new(Vec::new()),
        };
        let message = GitHubBaseLoader::new(&stub)
            .load(&reference())
            .await
            .unwrap_err();

        assert!(message.contains("contents: read"), "{message}");
        assert!(message.contains("GITHUB_TOKEN"), "{message}");
    }
}
