//! GitHub GraphQL client.
//!
//! The HTTP layer is abstracted by [`HttpTransport`] so the client logic
//! (401 handling, response parsing) is tested without network access.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::auth::{AuthError, Token, TokenProvider};

/// GitHub host supported by the MVP.
pub const GITHUB_HOST: &str = "github.com";
/// GraphQL endpoint for [`GITHUB_HOST`].
pub const GRAPHQL_URL: &str = "https://api.github.com/graphql";

/// Raw HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response body.
    pub body: String,
}

/// Sends GraphQL requests. Implementations must not include the token in
/// errors.
pub trait HttpTransport: Send + Sync {
    /// POSTs a JSON `body` to `url` with bearer `token`.
    fn post_json(&self, url: &str, token: &Token, body: &str) -> Result<HttpResponse, String>;
}

/// Blocking reqwest transport (rustls). Runs on a background thread.
pub struct ReqwestTransport {
    client: reqwest::blocking::Client,
}

impl ReqwestTransport {
    /// Builds the client.
    pub fn new() -> Result<Self, String> {
        let client = reqwest::blocking::Client::builder()
            .user_agent(concat!("hayami/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("failed to build HTTP client: {e}"))?;
        Ok(Self { client })
    }
}

impl HttpTransport for ReqwestTransport {
    fn post_json(&self, url: &str, token: &Token, body: &str) -> Result<HttpResponse, String> {
        let mut auth =
            reqwest::header::HeaderValue::from_str(&format!("bearer {}", token.expose()))
                .map_err(|_| "token contains invalid characters".to_owned())?;
        auth.set_sensitive(true);
        let response = self
            .client
            .post(url)
            .header(reqwest::header::AUTHORIZATION, auth)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_owned())
            .send()
            // reqwest errors carry the URL only, never headers.
            .map_err(|e| format!("request failed: {}", e.without_url()))?;
        let status = response.status().as_u16();
        let body = response
            .text()
            .map_err(|e| format!("failed to read response: {}", e.without_url()))?;
        Ok(HttpResponse { status, body })
    }
}

/// Errors from the GitHub client. No variant contains the token.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum GithubError {
    /// Authentication failed.
    #[error(transparent)]
    Auth(#[from] AuthError),
    /// Network or transport failure.
    #[error("network error: {0}")]
    Transport(String),
    /// Non-success HTTP status other than 401.
    #[error("GitHub API returned HTTP {status}: {message}")]
    Http {
        /// Status code.
        status: u16,
        /// Short message from the response.
        message: String,
    },
    /// GraphQL returned errors.
    #[error("GraphQL error: {0}")]
    GraphQl(String),
    /// The response did not have the expected shape.
    #[error("unexpected response from GitHub: {0}")]
    Parse(String),
}

/// GraphQL client with token refresh on 401.
pub struct GithubClient {
    transport: Arc<dyn HttpTransport>,
    tokens: Arc<TokenProvider>,
    url: String,
}

impl fmt::Debug for GithubClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GithubClient")
            .field("url", &self.url)
            .finish_non_exhaustive()
    }
}

impl GithubClient {
    /// Creates a client for `api.github.com`.
    pub fn new(transport: Arc<dyn HttpTransport>, tokens: Arc<TokenProvider>) -> Self {
        Self {
            transport,
            tokens,
            url: GRAPHQL_URL.to_owned(),
        }
    }

    /// Token provider shared with git fetches.
    pub fn tokens(&self) -> &Arc<TokenProvider> {
        &self.tokens
    }

    /// Runs a GraphQL query and returns its `data`.
    ///
    /// On HTTP 401 the token is re-obtained once and the request retried. If
    /// that fails too, the source's "please log in" error is returned.
    pub fn graphql(&self, query: &str, variables: Value) -> Result<Value, GithubError> {
        let body = json!({ "query": query, "variables": variables }).to_string();
        let token = self.tokens.token()?;
        let mut response = self
            .transport
            .post_json(&self.url, &token, &body)
            .map_err(GithubError::Transport)?;
        if response.status == 401 {
            let token = self
                .tokens
                .refresh()
                .map_err(|_| self.tokens.rejected_error())?;
            response = self
                .transport
                .post_json(&self.url, &token, &body)
                .map_err(GithubError::Transport)?;
            if response.status == 401 {
                return Err(self.tokens.rejected_error().into());
            }
        }
        parse_graphql_response(response)
    }

    /// Fetches open PRs requesting the viewer's review and PRs authored by the
    /// viewer, across all repositories.
    pub fn list_pull_requests(&self) -> Result<PrList, GithubError> {
        let data = self.graphql(&list_query(), json!({}))?;
        let parsed: ListData =
            serde_json::from_value(data).map_err(|e| GithubError::Parse(e.to_string()))?;
        Ok(PrList {
            review_requested: parsed.review_requested.into_summaries(),
            authored: parsed.authored.into_summaries(),
        })
    }

    /// Fetches the metadata of one PR: refs, changed files and review state.
    pub fn pull_request(&self, pr: &PrRef) -> Result<PrDetail, GithubError> {
        let data = self.graphql(
            DETAIL_QUERY,
            json!({ "owner": pr.owner, "name": pr.repo, "number": pr.number }),
        )?;
        let parsed: DetailData =
            serde_json::from_value(data).map_err(|e| GithubError::Parse(e.to_string()))?;
        let repo = parsed
            .repository
            .ok_or_else(|| GithubError::Parse("repository not found".into()))?;
        let node = repo
            .pull_request
            .ok_or_else(|| GithubError::Parse("pull request not found".into()))?;
        Ok(node.into_detail(pr.clone(), repo.url))
    }
}

fn parse_graphql_response(response: HttpResponse) -> Result<Value, GithubError> {
    let parsed: Result<Value, _> = serde_json::from_str(&response.body);
    if !(200..300).contains(&response.status) {
        let message = parsed
            .ok()
            .and_then(|v| v.get("message").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_else(|| "no message".into());
        return Err(GithubError::Http {
            status: response.status,
            message,
        });
    }
    let mut value = parsed.map_err(|e| GithubError::Parse(e.to_string()))?;
    if let Some(errors) = value.get("errors").and_then(Value::as_array)
        && !errors.is_empty()
    {
        let messages: Vec<&str> = errors
            .iter()
            .filter_map(|e| e.get("message").and_then(Value::as_str))
            .collect();
        return Err(GithubError::GraphQl(messages.join("; ")));
    }
    match value.get_mut("data") {
        Some(data) if !data.is_null() => Ok(data.take()),
        _ => Err(GithubError::Parse("missing data".into())),
    }
}

/// Identifies a pull request.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PrRef {
    /// Repository owner.
    pub owner: String,
    /// Repository name.
    pub repo: String,
    /// PR number.
    pub number: u64,
}

impl fmt::Display for PrRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}#{}", self.owner, self.repo, self.number)
    }
}

/// One row of the PR list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrSummary {
    /// Which PR.
    pub pr: PrRef,
    /// Title.
    pub title: String,
    /// Author login (empty for deleted users).
    pub author: String,
    /// Web URL.
    pub url: String,
    /// Draft flag.
    pub is_draft: bool,
    /// ISO 8601 update time.
    pub updated_at: String,
    /// Added lines.
    pub additions: u64,
    /// Deleted lines.
    pub deletions: u64,
    /// Review decision such as `APPROVED`, if any.
    pub review_decision: Option<String>,
}

/// The two groups of the PR list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrList {
    /// Open PRs requesting the viewer's review.
    pub review_requested: Vec<PrSummary>,
    /// Open PRs authored by the viewer.
    pub authored: Vec<PrSummary>,
}

/// Change type of a file as reported by GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrFile {
    /// Path in the head commit.
    pub path: String,
    /// Added lines.
    pub additions: u64,
    /// Deleted lines.
    pub deletions: u64,
    /// `ADDED`, `DELETED`, `MODIFIED`, `RENAMED`, ...
    pub change_type: String,
}

/// Latest review of one reviewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrReview {
    /// Reviewer login.
    pub author: String,
    /// `APPROVED`, `CHANGES_REQUESTED`, `COMMENTED`, ...
    pub state: String,
}

/// Metadata of one PR needed to compute and show its diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrDetail {
    /// Which PR.
    pub pr: PrRef,
    /// Title.
    pub title: String,
    /// Author login.
    pub author: String,
    /// Web URL.
    pub url: String,
    /// HTTPS clone URL of the base repository.
    pub repo_url: String,
    /// Base branch name.
    pub base_ref_name: String,
    /// Head branch name.
    pub head_ref_name: String,
    /// Commit the base branch pointed to.
    pub base_ref_oid: String,
    /// Head commit of the PR.
    pub head_ref_oid: String,
    /// Changed files (first page only).
    pub files: Vec<PrFile>,
    /// Review decision such as `APPROVED`, if any.
    pub review_decision: Option<String>,
    /// Latest reviews.
    pub reviews: Vec<PrReview>,
}

const PR_SUMMARY_FIELDS: &str = "
    ... on PullRequest {
        number title url isDraft updatedAt additions deletions reviewDecision
        author { login }
        repository { name owner { login } }
    }";

const LIST_QUERY_TEMPLATE: &str = r#"query {
  reviewRequested: search(query: "is:open is:pr archived:false review-requested:@me sort:updated-desc", type: ISSUE, first: 50) {
    nodes { FIELDS }
  }
  authored: search(query: "is:open is:pr archived:false author:@me sort:updated-desc", type: ISSUE, first: 50) {
    nodes { FIELDS }
  }
}"#;

fn list_query() -> String {
    LIST_QUERY_TEMPLATE.replace("FIELDS", PR_SUMMARY_FIELDS)
}

const DETAIL_QUERY: &str = r#"query($owner: String!, $name: String!, $number: Int!) {
  repository(owner: $owner, name: $name) {
    url
    pullRequest(number: $number) {
      number title url baseRefName headRefName baseRefOid headRefOid reviewDecision
      author { login }
      files(first: 100) { nodes { path additions deletions changeType } }
      latestReviews(first: 50) { nodes { state author { login } } }
    }
  }
}"#;

#[derive(Deserialize)]
struct Login {
    login: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListData {
    review_requested: SearchResult,
    authored: SearchResult,
}

#[derive(Deserialize)]
struct SearchResult {
    nodes: Vec<Option<SummaryNode>>,
}

impl SearchResult {
    fn into_summaries(self) -> Vec<PrSummary> {
        self.nodes
            .into_iter()
            .flatten()
            .filter_map(SummaryNode::into_summary)
            .collect()
    }
}

// Search nodes of other types deserialize with all fields missing.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SummaryNode {
    number: Option<u64>,
    title: Option<String>,
    url: Option<String>,
    #[serde(default)]
    is_draft: bool,
    #[serde(default)]
    updated_at: String,
    #[serde(default)]
    additions: u64,
    #[serde(default)]
    deletions: u64,
    review_decision: Option<String>,
    author: Option<Login>,
    repository: Option<RepoNode>,
}

#[derive(Deserialize)]
struct RepoNode {
    name: String,
    owner: Login,
}

impl SummaryNode {
    fn into_summary(self) -> Option<PrSummary> {
        let repo = self.repository?;
        Some(PrSummary {
            pr: PrRef {
                owner: repo.owner.login,
                repo: repo.name,
                number: self.number?,
            },
            title: self.title?,
            author: self.author.map(|a| a.login).unwrap_or_default(),
            url: self.url.unwrap_or_default(),
            is_draft: self.is_draft,
            updated_at: self.updated_at,
            additions: self.additions,
            deletions: self.deletions,
            review_decision: self.review_decision,
        })
    }
}

#[derive(Deserialize)]
struct DetailData {
    repository: Option<DetailRepo>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetailRepo {
    url: String,
    pull_request: Option<DetailNode>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetailNode {
    title: String,
    url: String,
    base_ref_name: String,
    head_ref_name: String,
    base_ref_oid: String,
    head_ref_oid: String,
    review_decision: Option<String>,
    author: Option<Login>,
    files: Option<Nodes<FileNode>>,
    latest_reviews: Option<Nodes<ReviewNode>>,
}

#[derive(Deserialize)]
struct Nodes<T> {
    nodes: Vec<Option<T>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileNode {
    path: String,
    additions: u64,
    deletions: u64,
    change_type: String,
}

#[derive(Deserialize)]
struct ReviewNode {
    state: String,
    author: Option<Login>,
}

impl DetailNode {
    fn into_detail(self, pr: PrRef, repo_url: String) -> PrDetail {
        PrDetail {
            pr,
            title: self.title,
            author: self.author.map(|a| a.login).unwrap_or_default(),
            url: self.url,
            repo_url,
            base_ref_name: self.base_ref_name,
            head_ref_name: self.head_ref_name,
            base_ref_oid: self.base_ref_oid,
            head_ref_oid: self.head_ref_oid,
            files: self
                .files
                .map(|f| {
                    f.nodes
                        .into_iter()
                        .flatten()
                        .map(|n| PrFile {
                            path: n.path,
                            additions: n.additions,
                            deletions: n.deletions,
                            change_type: n.change_type,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            review_decision: self.review_decision,
            reviews: self
                .latest_reviews
                .map(|r| {
                    r.nodes
                        .into_iter()
                        .flatten()
                        .map(|n| PrReview {
                            author: n.author.map(|a| a.login).unwrap_or_default(),
                            state: n.state,
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::TokenSource;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Transport returning scripted responses and recording used tokens.
    struct Scripted {
        responses: Mutex<VecDeque<HttpResponse>>,
        seen_tokens: Mutex<Vec<String>>,
        bodies: Mutex<Vec<String>>,
    }

    impl Scripted {
        fn new(responses: Vec<HttpResponse>) -> Arc<Self> {
            Arc::new(Self {
                responses: Mutex::new(responses.into()),
                seen_tokens: Mutex::new(Vec::new()),
                bodies: Mutex::new(Vec::new()),
            })
        }
    }

    impl HttpTransport for Scripted {
        fn post_json(&self, _url: &str, token: &Token, body: &str) -> Result<HttpResponse, String> {
            self.seen_tokens
                .lock()
                .unwrap()
                .push(token.expose().to_owned());
            self.bodies.lock().unwrap().push(body.to_owned());
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| "no more responses".to_owned())
        }
    }

    struct Seq {
        calls: AtomicUsize,
        fail_after: usize,
    }

    impl TokenSource for Seq {
        fn fetch(&self) -> Result<Token, AuthError> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            if n >= self.fail_after {
                return Err(AuthError::GhFailed);
            }
            Ok(Token::new(format!("tok{n}")).unwrap())
        }
        fn rejected_error(&self) -> AuthError {
            AuthError::LoginRequired
        }
    }

    fn client(transport: Arc<Scripted>, fail_after: usize) -> GithubClient {
        let tokens = Arc::new(TokenProvider::new(Box::new(Seq {
            calls: AtomicUsize::new(0),
            fail_after,
        })));
        GithubClient::new(transport, tokens)
    }

    fn ok(body: &str) -> HttpResponse {
        HttpResponse {
            status: 200,
            body: body.to_owned(),
        }
    }

    fn unauthorized() -> HttpResponse {
        HttpResponse {
            status: 401,
            body: r#"{"message":"Bad credentials"}"#.to_owned(),
        }
    }

    #[test]
    fn retries_with_refreshed_token_after_401() {
        let transport = Scripted::new(vec![unauthorized(), ok(r#"{"data":{"x":1}}"#)]);
        let client = client(transport.clone(), usize::MAX);
        let data = client.graphql("query { x }", json!({})).unwrap();
        assert_eq!(data, json!({"x": 1}));
        assert_eq!(*transport.seen_tokens.lock().unwrap(), vec!["tok0", "tok1"]);
    }

    #[test]
    fn second_401_asks_for_gh_auth_login() {
        let transport = Scripted::new(vec![unauthorized(), unauthorized()]);
        let client = client(transport, usize::MAX);
        let err = client.graphql("query { x }", json!({})).unwrap_err();
        assert_eq!(err, GithubError::Auth(AuthError::LoginRequired));
        assert!(err.to_string().contains("gh auth login"));
    }

    #[test]
    fn failed_refresh_after_401_asks_for_login() {
        let transport = Scripted::new(vec![unauthorized()]);
        let client = client(transport, 1);
        let err = client.graphql("query { x }", json!({})).unwrap_err();
        assert_eq!(err, GithubError::Auth(AuthError::LoginRequired));
    }

    #[test]
    fn initial_token_failure_is_reported() {
        let transport = Scripted::new(vec![]);
        let client = client(transport, 0);
        let err = client.graphql("query { x }", json!({})).unwrap_err();
        assert_eq!(err, GithubError::Auth(AuthError::GhFailed));
    }

    #[test]
    fn graphql_errors_and_http_errors() {
        let transport = Scripted::new(vec![
            ok(r#"{"data":null,"errors":[{"message":"boom"}]}"#),
            HttpResponse {
                status: 502,
                body: "<html>".into(),
            },
        ]);
        let client = client(transport, usize::MAX);
        assert_eq!(
            client.graphql("q", json!({})).unwrap_err(),
            GithubError::GraphQl("boom".into())
        );
        assert!(matches!(
            client.graphql("q", json!({})).unwrap_err(),
            GithubError::Http { status: 502, .. }
        ));
    }

    #[test]
    fn errors_do_not_contain_token() {
        let transport = Scripted::new(vec![unauthorized(), unauthorized()]);
        let client = client(transport, usize::MAX);
        let err = client.graphql("q", json!({})).unwrap_err();
        let printed = format!("{err} {err:?} {client:?}");
        assert!(
            !printed.contains("tok0") && !printed.contains("tok1"),
            "{printed}"
        );
    }

    #[test]
    fn parses_pr_list_groups() {
        let body = json!({"data": {
            "reviewRequested": {"nodes": [
                {"number": 7, "title": "Fix bug", "url": "https://github.com/a/b/pull/7",
                 "isDraft": false, "updatedAt": "2026-11-01T00:00:00Z", "additions": 3,
                 "deletions": 1, "reviewDecision": "REVIEW_REQUIRED",
                 "author": {"login": "alice"},
                 "repository": {"name": "b", "owner": {"login": "a"}}},
                {}
            ]},
            "authored": {"nodes": [
                {"number": 9, "title": "Mine", "url": "u", "isDraft": true,
                 "updatedAt": "t", "additions": 0, "deletions": 0, "reviewDecision": null,
                 "author": null, "repository": {"name": "r", "owner": {"login": "me"}}}
            ]}
        }});
        let transport = Scripted::new(vec![ok(&body.to_string())]);
        let client = client(transport.clone(), usize::MAX);
        let list = client.list_pull_requests().unwrap();
        assert_eq!(list.review_requested.len(), 1);
        let first = &list.review_requested[0];
        assert_eq!(first.pr.to_string(), "a/b#7");
        assert_eq!(first.author, "alice");
        assert_eq!(list.authored[0].pr.number, 9);
        assert!(list.authored[0].is_draft);
        let sent = transport.bodies.lock().unwrap()[0].clone();
        assert!(sent.contains("review-requested:@me"));
        assert!(sent.contains("author:@me"));
    }

    #[test]
    fn parses_pr_detail() {
        let body = json!({"data": {"repository": {
            "url": "https://github.com/a/b",
            "pullRequest": {
                "number": 7, "title": "T", "url": "https://github.com/a/b/pull/7",
                "baseRefName": "main", "headRefName": "feature",
                "baseRefOid": "1111111111111111111111111111111111111111",
                "headRefOid": "2222222222222222222222222222222222222222",
                "reviewDecision": "APPROVED", "author": {"login": "alice"},
                "files": {"nodes": [{"path": "src/lib.rs", "additions": 1, "deletions": 2, "changeType": "MODIFIED"}]},
                "latestReviews": {"nodes": [{"state": "APPROVED", "author": {"login": "bob"}}]}
            }
        }}});
        let transport = Scripted::new(vec![ok(&body.to_string())]);
        let client = client(transport, usize::MAX);
        let pr = PrRef {
            owner: "a".into(),
            repo: "b".into(),
            number: 7,
        };
        let detail = client.pull_request(&pr).unwrap();
        assert_eq!(detail.repo_url, "https://github.com/a/b");
        assert_eq!(detail.base_ref_name, "main");
        assert_eq!(detail.files[0].path, "src/lib.rs");
        assert_eq!(detail.reviews[0].author, "bob");
    }
}
