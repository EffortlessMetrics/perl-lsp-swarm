//! Paginated, provider-native GitHub review and thread facts.
//!
//! This command is a factual snapshot. It does not assign ownership, infer a
//! lifecycle label, or authorize a merge. A complete snapshot can report a
//! blocking review state; a failed or partial snapshot is `NOT_PROVEN`.

use color_eyre::eyre::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::process::Command;

const GRAPHQL_QUERY: &str = r#"
query(
  $owner: String!,
  $repo: String!,
  $number: Int!,
  $reviewsCursor: String,
  $reviewRequestsCursor: String,
  $threadsCursor: String,
  $reviewsActive: Boolean!,
  $reviewRequestsActive: Boolean!,
  $threadsActive: Boolean!
) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $number) {
      headRefOid
      reviews(first: 100, after: $reviewsCursor) @include(if: $reviewsActive) {
        nodes {
          author { login __typename }
          state
          submittedAt
          commit { oid }
        }
        pageInfo { hasNextPage endCursor }
      }
      reviewRequests(first: 100, after: $reviewRequestsCursor) @include(if: $reviewRequestsActive) {
        nodes {
          requestedReviewer {
            ... on User { login }
            ... on Team { name }
          }
        }
        pageInfo { hasNextPage endCursor }
      }
      reviewThreads(first: 100, after: $threadsCursor) @include(if: $threadsActive) {
        nodes {
          id
          isResolved
          isOutdated
          path
          line
          comments(first: 1) { totalCount }
        }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}
"#;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewSnapshot {
    pub repository: String,
    pub pr: u64,
    pub head_sha: String,
    pub result: String,
    pub converged: bool,
    pub submitted_reviews: Vec<SubmittedReview>,
    pub pending_reviewers: Vec<String>,
    pub unresolved_active: Vec<ThreadFact>,
    pub unresolved_outdated: Vec<ThreadFact>,
    pub resolved_without_disposition: Vec<ThreadFact>,
    pub currentness_basis: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubmittedReview {
    pub reviewer: String,
    pub reviewer_kind: String,
    pub state: String,
    pub submitted_at: Option<String>,
    pub reviewed_head: Option<String>,
    pub current_at_head: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ThreadFact {
    pub id: String,
    pub path: Option<String>,
    pub line: Option<u64>,
    pub comment_count: u64,
}

#[derive(Debug, Deserialize)]
struct GraphQlEnvelope {
    data: Option<GraphQlData>,
    #[serde(default)]
    errors: Vec<GraphQlError>,
}

#[derive(Debug, Deserialize)]
struct GraphQlError {
    message: String,
}

#[derive(Debug, Deserialize)]
struct GraphQlData {
    repository: Option<GraphQlRepository>,
}

#[derive(Debug, Deserialize)]
struct GraphQlRepository {
    #[serde(rename = "pullRequest")]
    pull_request: Option<GraphQlPullRequest>,
}

#[derive(Debug, Deserialize)]
struct GraphQlPullRequest {
    #[serde(rename = "headRefOid")]
    head_ref_oid: String,
    reviews: Option<GraphQlConnection<GraphQlReview>>,
    #[serde(rename = "reviewRequests")]
    review_requests: Option<GraphQlConnection<GraphQlReviewRequest>>,
    #[serde(rename = "reviewThreads")]
    review_threads: Option<GraphQlConnection<GraphQlThread>>,
}

#[derive(Debug, Deserialize)]
struct GraphQlConnection<T> {
    nodes: Vec<T>,
    #[serde(rename = "pageInfo")]
    page_info: GraphQlPageInfo,
}

#[derive(Debug, Deserialize)]
struct GraphQlPageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphQlReview {
    author: Option<GraphQlActor>,
    state: String,
    #[serde(rename = "submittedAt")]
    submitted_at: Option<String>,
    commit: Option<GraphQlCommit>,
}

#[derive(Debug, Deserialize)]
struct GraphQlActor {
    login: Option<String>,
    #[serde(rename = "__typename")]
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphQlCommit {
    oid: String,
}

#[derive(Debug, Deserialize)]
struct GraphQlReviewRequest {
    #[serde(rename = "requestedReviewer")]
    requested_reviewer: Option<GraphQlRequestedReviewer>,
}

#[derive(Debug, Deserialize)]
struct GraphQlRequestedReviewer {
    login: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GraphQlThread {
    id: String,
    #[serde(rename = "isResolved")]
    is_resolved: bool,
    #[serde(rename = "isOutdated")]
    is_outdated: bool,
    path: Option<String>,
    line: Option<u64>,
    comments: GraphQlComments,
}

#[derive(Debug, Deserialize)]
struct GraphQlComments {
    #[serde(rename = "totalCount")]
    total_count: u64,
}

pub fn run_review_convergence(pr: u64, json_only: bool) -> Result<()> {
    let snapshot = review_snapshot(pr)?;
    if !json_only {
        println!("review PR #{}: {} (head {})", snapshot.pr, snapshot.result, snapshot.head_sha);
        println!(
            "  submitted reviews: {}, pending reviewers: {}, unresolved threads: {}",
            snapshot.submitted_reviews.len(),
            snapshot.pending_reviewers.len(),
            snapshot.unresolved_active.len() + snapshot.unresolved_outdated.len()
        );
    } else {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    }

    if snapshot.result == "NOT_PROVEN" {
        bail!("review snapshot is NOT_PROVEN for PR #{}", pr);
    }
    Ok(())
}

/// Collect the review snapshot for composition by another factual instrument.
pub fn review_snapshot(pr: u64) -> Result<ReviewSnapshot> {
    let repository =
        command_text("gh", &["repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner"])?
            .trim()
            .to_string();
    let (owner, repo) = repository
        .split_once('/')
        .ok_or_else(|| color_eyre::eyre::eyre!("gh returned invalid repository {repository:?}"))?;
    Ok(collect_snapshot(repository.clone(), owner, repo, pr))
}

fn collect_snapshot(repository: String, owner: &str, repo: &str, pr: u64) -> ReviewSnapshot {
    let mut reviews = Vec::new();
    let mut pending_reviewers = BTreeSet::new();
    let mut unresolved_active = Vec::new();
    let mut unresolved_outdated = Vec::new();
    let mut resolved_without_disposition = Vec::new();
    let mut errors = Vec::new();
    let mut head_sha = None;
    let mut reviews_cursor = None;
    let mut review_requests_cursor = None;
    let mut threads_cursor = None;
    let mut reviews_done = false;
    let mut review_requests_done = false;
    let mut threads_done = false;

    loop {
        let page = match graphql_page(
            owner,
            repo,
            pr,
            reviews_cursor.as_deref(),
            review_requests_cursor.as_deref(),
            threads_cursor.as_deref(),
            !reviews_done,
            !review_requests_done,
            !threads_done,
        ) {
            Ok(page) => page,
            Err(error) => {
                errors.push(error.to_string());
                break;
            }
        };

        let Some(data) = page.data else {
            errors.extend(page.errors.into_iter().map(|error| error.message));
            errors.push("GraphQL returned no pull-request data".to_string());
            break;
        };
        if !page.errors.is_empty() {
            errors.extend(page.errors.into_iter().map(|error| error.message));
            break;
        }
        let Some(pull_request) = data.repository.and_then(|repo| repo.pull_request) else {
            errors.push(format!("PR #{pr} was not present in GraphQL response"));
            break;
        };

        if let Some(previous_head) = &head_sha {
            if previous_head != &pull_request.head_ref_oid {
                errors.push("PR head moved during paginated review snapshot".to_string());
                break;
            }
        } else {
            head_sha = Some(pull_request.head_ref_oid.clone());
        }

        let current_head = pull_request.head_ref_oid.clone();
        if !reviews_done {
            let Some(reviews_page) = pull_request.reviews else {
                errors.push("GraphQL omitted active review page".to_string());
                break;
            };
            reviews.extend(reviews_page.nodes.into_iter().map(|review| {
                SubmittedReview {
                    reviewer: review
                        .author
                        .as_ref()
                        .and_then(|actor| actor.login.clone())
                        .unwrap_or_else(|| "unknown".to_string()),
                    reviewer_kind: review
                        .author
                        .and_then(|actor| actor.kind)
                        .unwrap_or_else(|| "Unknown".to_string()),
                    state: review.state,
                    submitted_at: review.submitted_at,
                    current_at_head: review
                        .commit
                        .as_ref()
                        .is_some_and(|commit| commit.oid == current_head),
                    reviewed_head: review.commit.map(|commit| commit.oid),
                }
            }));
            reviews_done = !reviews_page.page_info.has_next_page;
            reviews_cursor = reviews_page.page_info.end_cursor;
        }
        if !review_requests_done {
            let Some(review_requests_page) = pull_request.review_requests else {
                errors.push("GraphQL omitted active review-request page".to_string());
                break;
            };
            pending_reviewers.extend(review_requests_page.nodes.into_iter().filter_map(
                |request| {
                    request.requested_reviewer.and_then(|reviewer| reviewer.login.or(reviewer.name))
                },
            ));
            review_requests_done = !review_requests_page.page_info.has_next_page;
            review_requests_cursor = review_requests_page.page_info.end_cursor;
        }
        if !threads_done {
            let Some(threads_page) = pull_request.review_threads else {
                errors.push("GraphQL omitted active review-thread page".to_string());
                break;
            };
            for thread in threads_page.nodes {
                let fact = ThreadFact {
                    id: thread.id,
                    path: thread.path,
                    line: thread.line,
                    comment_count: thread.comments.total_count,
                };
                if !thread.is_resolved {
                    if thread.is_outdated {
                        unresolved_outdated.push(fact);
                    } else {
                        unresolved_active.push(fact);
                    }
                } else if thread.comments.total_count <= 1 {
                    resolved_without_disposition.push(fact);
                }
            }
            threads_done = !threads_page.page_info.has_next_page;
            threads_cursor = threads_page.page_info.end_cursor;
        }

        if reviews_done && review_requests_done && threads_done {
            break;
        }
    }

    if errors.is_empty() {
        let pr_arg = pr.to_string();
        match command_text(
            "gh",
            &[
                "pr",
                "view",
                &pr_arg,
                "--repo",
                repository.as_str(),
                "--json",
                "headRefOid",
                "--jq",
                ".headRefOid",
            ],
        ) {
            Ok(final_head) if final_head.trim() == head_sha.as_deref().unwrap_or_default() => {}
            Ok(final_head) => errors.push(format!(
                "PR head moved after paginated review snapshot: captured {}, final {}",
                head_sha.as_deref().unwrap_or_default(),
                final_head.trim()
            )),
            Err(error) => errors.push(format!("failed to verify PR head after snapshot: {error}")),
        }
    }

    let head_sha = head_sha.unwrap_or_default();
    let stale_human = stale_human_reviews(&reviews);
    let converged = errors.is_empty()
        && pending_reviewers.is_empty()
        && !stale_human
        && unresolved_active.is_empty()
        && unresolved_outdated.is_empty()
        && resolved_without_disposition.is_empty();

    ReviewSnapshot {
        repository,
        pr,
        head_sha,
        result: if errors.is_empty() {
            if converged { "CURRENT" } else { "BLOCKED" }
        } else {
            "NOT_PROVEN"
        }
        .to_string(),
        converged,
        submitted_reviews: reviews,
        pending_reviewers: pending_reviewers.into_iter().collect(),
        unresolved_active,
        unresolved_outdated,
        resolved_without_disposition,
        currentness_basis: vec![
            page_basis("submitted review", reviews_done),
            page_basis("requested-reviewer", review_requests_done),
            page_basis("review-thread", threads_done),
            if errors.is_empty() {
                "review commit OIDs compared with the captured pullRequest.headRefOid".to_string()
            } else {
                "review commit currentness is incomplete because the snapshot has errors"
                    .to_string()
            },
        ],
        errors,
    }
}

fn page_basis(kind: &str, complete: bool) -> String {
    if complete {
        format!("all {kind} pages fetched")
    } else {
        format!("{kind} pages incomplete; result is NOT_PROVEN")
    }
}

fn stale_human_reviews(reviews: &[SubmittedReview]) -> bool {
    let mut latest = std::collections::BTreeMap::<String, &SubmittedReview>::new();
    for review in reviews.iter().filter(|review| review.reviewer_kind != "Bot") {
        let replace = latest.get(&review.reviewer).is_none_or(|current| {
            review.submitted_at.as_deref().unwrap_or("")
                >= current.submitted_at.as_deref().unwrap_or("")
        });
        if replace {
            latest.insert(review.reviewer.clone(), review);
        }
    }
    latest.values().any(|review| !review.current_at_head)
}

#[allow(clippy::too_many_arguments)]
fn graphql_page(
    owner: &str,
    repo: &str,
    pr: u64,
    reviews_cursor: Option<&str>,
    review_requests_cursor: Option<&str>,
    threads_cursor: Option<&str>,
    reviews_active: bool,
    review_requests_active: bool,
    threads_active: bool,
) -> Result<GraphQlEnvelope> {
    let number = pr.to_string();
    let reviews = reviews_cursor.unwrap_or("null");
    let review_requests = review_requests_cursor.unwrap_or("null");
    let threads = threads_cursor.unwrap_or("null");
    let query = format!("query={GRAPHQL_QUERY}");
    let output = Command::new("gh")
        .args([
            "api",
            "graphql",
            "-f",
            query.as_str(),
            "-F",
            &format!("owner={owner}"),
            "-F",
            &format!("repo={repo}"),
            "-F",
            &format!("number={number}"),
            "-F",
            &format!("reviewsCursor={reviews}"),
            "-F",
            &format!("reviewRequestsCursor={review_requests}"),
            "-F",
            &format!("threadsCursor={threads}"),
            "-F",
            &format!("reviewsActive={reviews_active}"),
            "-F",
            &format!("reviewRequestsActive={review_requests_active}"),
            "-F",
            &format!("threadsActive={threads_active}"),
        ])
        .output()
        .context("failed to execute gh GraphQL review snapshot")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("gh GraphQL review snapshot failed: {stderr}");
    }
    serde_json::from_slice(&output.stdout).context("failed to parse gh GraphQL review snapshot")
}

fn command_text(program: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to execute {program}"))?;
    if !output.status.success() {
        bail!(
            "{program} failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_single_comment_is_separated_as_silent() {
        let mut snapshot = ReviewSnapshot {
            repository: "owner/repo".to_string(),
            pr: 1,
            head_sha: "head".to_string(),
            result: "BLOCKED".to_string(),
            converged: false,
            submitted_reviews: Vec::new(),
            pending_reviewers: Vec::new(),
            unresolved_active: Vec::new(),
            unresolved_outdated: Vec::new(),
            resolved_without_disposition: vec![ThreadFact {
                id: "thread-1".to_string(),
                path: Some("src/lib.rs".to_string()),
                line: Some(10),
                comment_count: 1,
            }],
            currentness_basis: Vec::new(),
            errors: Vec::new(),
        };
        snapshot.converged = snapshot.resolved_without_disposition.is_empty();
        assert!(!snapshot.converged);
    }

    #[test]
    fn incomplete_snapshot_is_not_proven() {
        let snapshot = ReviewSnapshot {
            repository: "owner/repo".to_string(),
            pr: 1,
            head_sha: "head".to_string(),
            result: "NOT_PROVEN".to_string(),
            converged: false,
            submitted_reviews: Vec::new(),
            pending_reviewers: Vec::new(),
            unresolved_active: Vec::new(),
            unresolved_outdated: Vec::new(),
            resolved_without_disposition: Vec::new(),
            currentness_basis: vec!["head".to_string()],
            errors: vec!["rate limit".to_string()],
        };
        assert!(!snapshot.converged);
        assert_eq!(snapshot.result, "NOT_PROVEN");
    }

    #[test]
    fn only_latest_human_submission_controls_currentness() {
        let reviews = vec![
            SubmittedReview {
                reviewer: "reviewer".to_string(),
                reviewer_kind: "User".to_string(),
                state: "CHANGES_REQUESTED".to_string(),
                submitted_at: Some("2026-08-01T00:00:00Z".to_string()),
                reviewed_head: Some("old-head".to_string()),
                current_at_head: false,
            },
            SubmittedReview {
                reviewer: "reviewer".to_string(),
                reviewer_kind: "User".to_string(),
                state: "APPROVED".to_string(),
                submitted_at: Some("2026-08-02T00:00:00Z".to_string()),
                reviewed_head: Some("current-head".to_string()),
                current_at_head: true,
            },
        ];
        assert!(!stale_human_reviews(&reviews));
    }

    #[test]
    fn graphql_page_shape_preserves_reviews_requests_and_threads() -> Result<()> {
        let envelope: GraphQlEnvelope = serde_json::from_str(
            r#"{
              "data": {"repository": {"pullRequest": {
                "headRefOid": "head-1",
                "reviews": {"nodes": [{
                  "author": {"login": "reviewer", "__typename": "User"},
                  "state": "APPROVED",
                  "submittedAt": "2026-08-02T00:00:00Z",
                  "commit": {"oid": "head-1"}
                }], "pageInfo": {"hasNextPage": false, "endCursor": null}},
                "reviewRequests": {"nodes": [{
                  "requestedReviewer": {"login": "pending", "name": null}
                }], "pageInfo": {"hasNextPage": false, "endCursor": null}},
                "reviewThreads": {"nodes": [{
                  "id": "thread-1", "isResolved": false, "isOutdated": true,
                  "path": "src/lib.rs", "line": 7,
                  "comments": {"totalCount": 1}
                }], "pageInfo": {"hasNextPage": false, "endCursor": null}}
              }}}
            }"#,
        )?;
        let pull_request = envelope
            .data
            .and_then(|data| data.repository)
            .and_then(|repository| repository.pull_request)
            .ok_or_else(|| color_eyre::eyre::eyre!("missing pull request"))?;
        assert_eq!(pull_request.head_ref_oid, "head-1");
        assert_eq!(pull_request.reviews.as_ref().map(|page| page.nodes.len()), Some(1));
        assert_eq!(pull_request.review_requests.as_ref().map(|page| page.nodes.len()), Some(1));
        assert_eq!(
            pull_request
                .review_threads
                .as_ref()
                .and_then(|page| page.nodes.first())
                .map(|thread| thread.is_outdated),
            Some(true)
        );
        Ok(())
    }
}
