//! Local bare repositories, fetching PR refs and computing PR diffs with gix.
//!
//! No `git` executable is run by this module.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use gix::ObjectId;
use gix::bstr::ByteSlice;
use gix::object::tree::diff::ChangeDetached;

use crate::auth::Token;
use crate::diff::{FileDiff, FileStatus};

/// Errors from git operations. Messages never contain credentials.
#[derive(Debug, thiserror::Error)]
pub enum GitError {
    /// The cache directory could not be determined or created.
    #[error("cache directory unavailable: {0}")]
    Cache(String),
    /// Opening or creating the bare repository failed.
    #[error("failed to open repository at {path}: {message}")]
    Open {
        /// Repository path.
        path: PathBuf,
        /// Cause.
        message: String,
    },
    /// Fetching from the remote failed.
    #[error("fetch failed: {0}")]
    Fetch(String),
    /// A commit is missing after fetching.
    #[error("commit {0} is not available locally")]
    MissingCommit(String),
    /// Base and head have no common ancestor.
    #[error("no merge base between base {base} and head {head}")]
    NoMergeBase {
        /// Base commit.
        base: String,
        /// Head commit.
        head: String,
    },
    /// Any other gix failure.
    #[error("git error: {0}")]
    Other(String),
}

fn other(e: impl std::fmt::Display) -> GitError {
    GitError::Other(e.to_string())
}

/// Opens the bare repository at `path`, creating it if it does not exist.
pub fn open_or_init_bare(path: &Path) -> Result<gix::Repository, GitError> {
    let open_err = |e: &dyn std::fmt::Display| GitError::Open {
        path: path.to_owned(),
        message: e.to_string(),
    };
    if path.join("HEAD").is_file() {
        let options = gix::open::Options::isolated().strict_config(false);
        return gix::open_opts(path, options).map_err(|e| open_err(&e));
    }
    std::fs::create_dir_all(path).map_err(|e| open_err(&e))?;
    gix::init_bare(path).map_err(|e| open_err(&e))
}

/// What to fetch for a pull request.
#[derive(Debug, Clone)]
pub struct FetchRequest {
    /// HTTPS URL of the base repository.
    pub url: String,
    /// PR number (fetches `refs/pull/<n>/head`).
    pub number: u64,
    /// Base branch name.
    pub base_ref_name: String,
    /// Base commit expected after fetching.
    pub base_oid: String,
    /// Head commit expected after fetching.
    pub head_oid: String,
}

impl FetchRequest {
    /// Local ref the PR head is stored in.
    pub fn local_head_ref(&self) -> String {
        format!("refs/hayami/pull/{}/head", self.number)
    }

    /// Local ref the base branch is stored in.
    pub fn local_base_ref(&self) -> String {
        format!("refs/hayami/pull/{}/base", self.number)
    }

    fn refspecs(&self) -> Vec<String> {
        vec![
            format!("+refs/pull/{}/head:{}", self.number, self.local_head_ref()),
            format!(
                "+refs/heads/{}:{}",
                self.base_ref_name,
                self.local_base_ref()
            ),
        ]
    }
}

fn parse_oid(hex: &str) -> Result<ObjectId, GitError> {
    ObjectId::from_hex(hex.as_bytes()).map_err(|_| GitError::MissingCommit(hex.to_owned()))
}

fn has_commit(repo: &gix::Repository, oid: &ObjectId) -> bool {
    repo.find_commit(*oid).is_ok()
}

/// Fetches the PR head and base unless both commits are already present.
///
/// `token` is called when the server asks for credentials; it is passed to
/// gix through the credential callback, never in the URL. Returns whether a
/// fetch was performed.
pub fn fetch_pr(
    repo: &gix::Repository,
    request: &FetchRequest,
    token: &dyn Fn() -> Option<Token>,
    progress: &dyn Fn(&str),
) -> Result<bool, GitError> {
    let base = parse_oid(&request.base_oid)?;
    let head = parse_oid(&request.head_oid)?;
    if has_commit(repo, &base) && has_commit(repo, &head) {
        return Ok(false);
    }
    progress("Fetching pull request refs…");
    fetch_refspecs(repo, &request.url, &request.refspecs(), token)?;
    if !has_commit(repo, &base) {
        // The base branch moved on; ask for the exact commit.
        progress("Fetching base commit…");
        fetch_refspecs(
            repo,
            &request.url,
            std::slice::from_ref(&request.base_oid),
            token,
        )?;
    }
    for oid in [&base, &head] {
        if !has_commit(repo, oid) {
            return Err(GitError::MissingCommit(oid.to_string()));
        }
    }
    Ok(true)
}

fn fetch_refspecs(
    repo: &gix::Repository,
    url: &str,
    refspecs: &[String],
    token: &dyn Fn() -> Option<Token>,
) -> Result<(), GitError> {
    use gix::credentials::helper::Action;
    use gix::credentials::protocol::Outcome;
    use gix::remote::Direction;

    let fetch_err = |e: &dyn std::fmt::Display| GitError::Fetch(e.to_string());
    let remote = repo
        .remote_at(url)
        .map_err(|e| fetch_err(&e))?
        .with_fetch_tags(gix::remote::fetch::Tags::None)
        .with_refspecs(refspecs.iter().map(String::as_str), Direction::Fetch)
        .map_err(|e| fetch_err(&e))?;
    let connection = remote
        .connect(Direction::Fetch)
        .map_err(|e| fetch_err(&e))?
        .with_credentials(move |action| match action {
            Action::Get(context) => Ok(token().map(|token| Outcome {
                identity: gix::sec::identity::Account {
                    username: "x-access-token".into(),
                    password: token.expose().to_owned(),
                    oauth_refresh_token: None,
                },
                next: context.into(),
            })),
            Action::Store(_) | Action::Erase(_) => Ok(None),
        });
    let interrupt = AtomicBool::new(false);
    connection
        .prepare_fetch(gix::progress::Discard, Default::default())
        .map_err(|e| fetch_err(&e))?
        .receive(gix::progress::Discard, &interrupt)
        .map_err(|e| fetch_err(&e))?;
    Ok(())
}

/// Result of [`compute_pr_diff`].
#[derive(Debug, Clone)]
pub struct PrDiff {
    /// Merge base of base and head.
    pub merge_base: String,
    /// Changed files sorted by path.
    pub files: Vec<FileDiff>,
}

/// Diffs the merge base of `base` and `head` against `head`, like GitHub's PR
/// diff, with rename detection.
pub fn compute_pr_diff(repo: &gix::Repository, base: &str, head: &str) -> Result<PrDiff, GitError> {
    let base_id = parse_oid(base)?;
    let head_id = parse_oid(head)?;
    for oid in [&base_id, &head_id] {
        if !has_commit(repo, oid) {
            return Err(GitError::MissingCommit(oid.to_string()));
        }
    }
    let merge_base = repo
        .merge_base(base_id, head_id)
        .map_err(other)?
        .ok_or_else(|| GitError::NoMergeBase {
            base: base.to_owned(),
            head: head.to_owned(),
        })?
        .detach();
    let old_tree = repo
        .find_commit(merge_base)
        .map_err(other)?
        .tree()
        .map_err(other)?;
    let new_tree = repo
        .find_commit(head_id)
        .map_err(other)?
        .tree()
        .map_err(other)?;
    let options = gix::diff::Options::default().with_rewrites(Some(gix::diff::Rewrites::default()));
    let changes = repo
        .diff_tree_to_tree(&old_tree, &new_tree, options)
        .map_err(other)?;

    let blob = |id: &ObjectId| -> Result<Vec<u8>, GitError> {
        Ok(repo.find_blob(*id).map_err(other)?.take_data())
    };
    let mut files = Vec::new();
    for change in changes {
        let file = match change {
            ChangeDetached::Addition {
                location,
                entry_mode,
                id,
                ..
            } if is_file(entry_mode) => FileDiff::from_blobs(
                location.to_str_lossy().into_owned(),
                FileStatus::Added,
                None,
                Some(&blob(&id)?),
            ),
            ChangeDetached::Deletion {
                location,
                entry_mode,
                id,
                ..
            } if is_file(entry_mode) => FileDiff::from_blobs(
                location.to_str_lossy().into_owned(),
                FileStatus::Deleted,
                Some(&blob(&id)?),
                None,
            ),
            ChangeDetached::Modification {
                location,
                previous_entry_mode,
                previous_id,
                entry_mode,
                id,
            } if is_file(entry_mode) || is_file(previous_entry_mode) => {
                let old = is_file(previous_entry_mode)
                    .then(|| blob(&previous_id))
                    .transpose()?;
                let new = is_file(entry_mode).then(|| blob(&id)).transpose()?;
                FileDiff::from_blobs(
                    location.to_str_lossy().into_owned(),
                    FileStatus::Modified,
                    old.as_deref(),
                    new.as_deref(),
                )
            }
            ChangeDetached::Rewrite {
                source_location,
                source_id,
                entry_mode,
                id,
                location,
                copy,
                ..
            } if is_file(entry_mode) => {
                let path = location.to_str_lossy().into_owned();
                let status = if copy {
                    FileStatus::Added
                } else {
                    FileStatus::Renamed {
                        from: source_location.to_str_lossy().into_owned(),
                    }
                };
                let old = if copy { None } else { Some(blob(&source_id)?) };
                FileDiff::from_blobs(path, status, old.as_deref(), Some(&blob(&id)?))
            }
            _ => continue,
        };
        files.push(file);
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(PrDiff {
        merge_base: merge_base.to_string(),
        files,
    })
}

fn is_file(mode: gix::object::tree::EntryMode) -> bool {
    mode.is_blob_or_symlink()
}

/// Splits an HTTPS repository URL into host, owner and name.
pub fn parse_repo_url(url: &str) -> Option<(String, String, String)> {
    let rest = url.strip_prefix("https://")?;
    let mut parts = rest.trim_end_matches('/').split('/');
    let host = parts.next()?;
    let owner = parts.next()?;
    let name = parts.next()?.trim_end_matches(".git");
    if parts.next().is_some() {
        return None;
    }
    Some((host.to_owned(), owner.to_owned(), name.to_owned()))
}

/// Opens (creating on first use) the cache repository of the PR, fetches the
/// PR refs if needed and computes the diff. Runs on a background thread.
///
/// If the fetch fails and a token provider is given, the token is
/// re-obtained once and the fetch retried (the token may have expired).
pub fn load_pr_diff(
    detail: &crate::github::PrDetail,
    repos_root: &Path,
    tokens: Option<&crate::auth::TokenProvider>,
    progress: &dyn Fn(&str),
) -> Result<PrDiff, GitError> {
    let (host, owner, name) = parse_repo_url(&detail.repo_url).ok_or_else(|| {
        GitError::Cache(format!("unsupported repository URL {}", detail.repo_url))
    })?;
    let path = crate::paths::repo_dir(repos_root, &host, &owner, &name)
        .ok_or_else(|| GitError::Cache(format!("invalid repository name {owner}/{name}")))?;
    progress("Opening local repository…");
    let repo = open_or_init_bare(&path)?;
    let request = FetchRequest {
        url: detail.repo_url.clone(),
        number: detail.pr.number,
        base_ref_name: detail.base_ref_name.clone(),
        base_oid: detail.base_ref_oid.clone(),
        head_oid: detail.head_ref_oid.clone(),
    };
    let token = || tokens.and_then(|t| t.token().ok());
    if let Err(first) = fetch_pr(&repo, &request, &token, progress) {
        let Some(provider) = tokens else {
            return Err(first);
        };
        if provider.refresh().is_err() {
            return Err(first);
        }
        progress("Retrying fetch with a refreshed token…");
        fetch_pr(&repo, &request, &token, progress)?;
    }
    progress("Computing diff…");
    compute_pr_diff(&repo, &request.base_oid, &request.head_oid)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::diff::FileContent;
    use gix::object::tree::EntryKind;
    use gix::refs::transaction::PreviousValue;

    /// Writes a commit whose tree contains exactly `files` and points `refname` at it.
    pub(crate) fn commit(
        repo: &gix::Repository,
        refname: &str,
        files: &[(&str, &[u8])],
        parents: &[ObjectId],
    ) -> ObjectId {
        let mut editor = repo
            .edit_tree(ObjectId::empty_tree(repo.object_hash()))
            .unwrap();
        for (path, content) in files {
            let blob = repo.write_blob(content).unwrap();
            editor.upsert(*path, EntryKind::Blob, blob).unwrap();
        }
        let tree = editor.write().unwrap();
        let sig = gix::actor::SignatureRef {
            name: "Test".into(),
            email: "test@example.com".into(),
            time: "1700000000 +0000",
        };
        let id = repo
            .new_commit_as(sig, sig, "commit", tree, parents.iter().copied())
            .unwrap()
            .id;
        repo.reference(refname, id, PreviousValue::Any, "test")
            .unwrap();
        id
    }

    fn numbered(n: usize) -> Vec<u8> {
        (1..=n)
            .map(|i| format!("line {i}\n"))
            .collect::<String>()
            .into_bytes()
    }

    #[test]
    fn diff_is_against_merge_base() {
        let dir = tempfile::tempdir().unwrap();
        let repo = open_or_init_bare(&dir.path().join("r.git")).unwrap();
        let common = commit(
            &repo,
            "refs/heads/main",
            &[("main.txt", b"a\n"), ("feature.txt", b"x\n")],
            &[],
        );
        // The base branch moved on after the PR branched off.
        let base = commit(
            &repo,
            "refs/heads/main",
            &[("main.txt", b"a\nb\n"), ("feature.txt", b"x\n")],
            &[common],
        );
        let head = commit(
            &repo,
            "refs/heads/feature",
            &[("main.txt", b"a\n"), ("feature.txt", b"x\ny\n")],
            &[common],
        );
        let diff = compute_pr_diff(&repo, &base.to_string(), &head.to_string()).unwrap();
        assert_eq!(diff.merge_base, common.to_string());
        assert_eq!(diff.files.len(), 1, "{:?}", diff.files);
        let file = &diff.files[0];
        assert_eq!(file.path, "feature.txt");
        assert_eq!(file.status, FileStatus::Modified);
        assert_eq!((file.additions, file.deletions), (1, 0));
    }

    #[test]
    fn detects_rename_add_delete_and_binary() {
        let dir = tempfile::tempdir().unwrap();
        let repo = open_or_init_bare(&dir.path().join("r.git")).unwrap();
        let old_body = numbered(40);
        let mut new_body = old_body.clone();
        new_body.extend_from_slice(b"one more line\n");
        let base = commit(
            &repo,
            "refs/heads/main",
            &[
                ("src/old_name.rs", &old_body),
                ("gone.txt", b"bye\n"),
                ("logo.png", b"\x89PNG\0\0\0"),
            ],
            &[],
        );
        let head = commit(
            &repo,
            "refs/heads/feature",
            &[
                ("src/new_name.rs", &new_body),
                ("added.txt", b"hello\n"),
                ("logo.png", b"\x89PNG\0\0\x01"),
            ],
            &[base],
        );
        let diff = compute_pr_diff(&repo, &base.to_string(), &head.to_string()).unwrap();
        let paths: Vec<_> = diff.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            ["added.txt", "gone.txt", "logo.png", "src/new_name.rs"]
        );
        assert_eq!(diff.files[0].status, FileStatus::Added);
        assert_eq!(diff.files[1].status, FileStatus::Deleted);
        assert_eq!(diff.files[2].content, FileContent::Binary);
        let renamed = &diff.files[3];
        assert_eq!(
            renamed.status,
            FileStatus::Renamed {
                from: "src/old_name.rs".into()
            }
        );
        assert_eq!((renamed.additions, renamed.deletions), (1, 0));
    }

    #[test]
    fn parses_repo_urls() {
        assert_eq!(
            parse_repo_url("https://github.com/octo/hello"),
            Some(("github.com".into(), "octo".into(), "hello".into()))
        );
        assert_eq!(
            parse_repo_url("https://github.com/octo/hello.git/"),
            Some(("github.com".into(), "octo".into(), "hello".into()))
        );
        assert_eq!(parse_repo_url("http://github.com/a/b"), None);
        assert_eq!(parse_repo_url("https://github.com/a/b/c"), None);
    }

    #[test]
    fn missing_commits_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let repo = open_or_init_bare(&dir.path().join("r.git")).unwrap();
        let missing = "1111111111111111111111111111111111111111";
        assert!(matches!(
            compute_pr_diff(&repo, missing, missing),
            Err(GitError::MissingCommit(_))
        ));
    }

    #[test]
    fn reopens_existing_repository() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/owner/r.git");
        let repo = open_or_init_bare(&path).unwrap();
        let id = commit(&repo, "refs/heads/main", &[("a", b"a")], &[]);
        drop(repo);
        let repo = open_or_init_bare(&path).unwrap();
        assert!(repo.is_bare());
        assert!(has_commit(&repo, &id));
    }

    /// Fetching over the `file://` transport makes gix spawn
    /// `git-upload-pack` (the server side), so this test needs git installed.
    /// hayami itself never runs git.
    #[test]
    fn fetches_pull_ref_and_base_from_remote() {
        if std::env::var_os("PATH")
            .and_then(|p| crate::auth::find_executable("git", &p))
            .is_none()
        {
            eprintln!("skipping: git-upload-pack is not available");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let upstream = open_or_init_bare(&dir.path().join("upstream.git")).unwrap();
        let common = commit(&upstream, "refs/heads/main", &[("f", b"1\n")], &[]);
        let base = commit(&upstream, "refs/heads/main", &[("f", b"1\n2\n")], &[common]);
        let head = commit(
            &upstream,
            "refs/pull/7/head",
            &[("f", b"1\nx\n")],
            &[common],
        );
        // The base branch moves on after GitHub reported base_oid.
        commit(
            &upstream,
            "refs/heads/main",
            &[("f", b"1\n2\n3\n")],
            &[base],
        );

        let local = open_or_init_bare(&dir.path().join("cache/github.com/o/r.git")).unwrap();
        let url = format!("file://{}", dir.path().join("upstream.git").display());
        let request = FetchRequest {
            url,
            number: 7,
            base_ref_name: "main".into(),
            base_oid: base.to_string(),
            head_oid: head.to_string(),
        };
        let messages = std::sync::Mutex::new(Vec::new());
        let progress = |m: &str| messages.lock().unwrap().push(m.to_owned());
        assert!(fetch_pr(&local, &request, &|| None, &progress).unwrap());
        assert!(local.find_reference(&request.local_head_ref()).is_ok());
        let diff = compute_pr_diff(&local, &request.base_oid, &request.head_oid).unwrap();
        assert_eq!(diff.files.len(), 1);
        // Second call finds both commits locally and does not fetch.
        assert!(!fetch_pr(&local, &request, &|| None, &progress).unwrap());
        assert!(!messages.lock().unwrap().is_empty());
    }

    /// When `base_oid` is not reachable from the base branch any more (e.g.
    /// force-pushed), it is fetched by object id.
    #[test]
    fn fetches_base_commit_by_id_when_branch_does_not_contain_it() {
        if std::env::var_os("PATH")
            .and_then(|p| crate::auth::find_executable("git", &p))
            .is_none()
        {
            eprintln!("skipping: git-upload-pack is not available");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let upstream = open_or_init_bare(&dir.path().join("upstream.git")).unwrap();
        let common = commit(&upstream, "refs/heads/main", &[("f", b"1\n")], &[]);
        let base = commit(&upstream, "refs/heads/old", &[("f", b"1\n2\n")], &[common]);
        let head = commit(
            &upstream,
            "refs/pull/3/head",
            &[("f", b"1\nx\n")],
            &[common],
        );
        let local = open_or_init_bare(&dir.path().join("local.git")).unwrap();
        let request = FetchRequest {
            url: format!("file://{}", dir.path().join("upstream.git").display()),
            number: 3,
            base_ref_name: "main".into(),
            base_oid: base.to_string(),
            head_oid: head.to_string(),
        };
        assert!(fetch_pr(&local, &request, &|| None, &|_| {}).unwrap());
        assert!(has_commit(&local, &base));
    }
}
