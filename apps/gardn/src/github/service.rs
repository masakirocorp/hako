/*
GitHub workflow behavior adapted from ghui.
MIT License
Copyright (c) 2026 Kit Langton

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
*/

use super::{diff::DiffFile, domain::*, GithubRepository, ResolvedGithubScope};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub(crate) struct GithubService {
    pub scope: Option<ResolvedGithubScope>,
    pub cancelled: Arc<AtomicBool>,
    pub program: std::path::PathBuf,
}

impl GithubService {
    fn command(&self, args: &[String], input: Option<Value>) -> GithubResult<Vec<u8>> {
        if self.cancelled.load(Ordering::Relaxed) {
            return Err("GitHub request cancelled".into());
        }
        let mut command = crate::noninteractive_process::command(&self.program);
        command
            .args(args)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_PAGER", "cat")
            .env("GH_HOST", "github.com")
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        crate::platform::configure_cancellable_command(&mut command);
        let mut child = command
            .spawn()
            .map_err(|e| format!("Cannot start gh: {e}"))?;
        let stdout = child.stdout.take().ok_or("Missing gh stdout")?;
        let stderr = child.stderr.take().ok_or("Missing gh stderr")?;
        let out = thread::spawn(move || {
            let mut bytes = Vec::new();
            stdout
                .take(32 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        });
        let err = thread::spawn(move || {
            let mut bytes = Vec::new();
            stderr
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        });
        let writer = input.map(|value| {
            let stdin = child.stdin.take();
            thread::spawn(move || -> GithubResult<()> {
                let mut stdin = stdin.ok_or("Missing gh stdin")?;
                serde_json::to_writer(&mut stdin, &value).map_err(|e| e.to_string())?;
                stdin.flush().map_err(|e| e.to_string())
            })
        });
        let start = Instant::now();
        let status = loop {
            if self.cancelled.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(60) {
                crate::platform::terminate_cancellable_child(&mut child);
                return Err(if self.cancelled.load(Ordering::Relaxed) { "GitHub request cancelled" } else { "GitHub request timed out; mutation outcome may be unknown. Refresh before retrying." }.into());
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(15)),
                Err(e) => {
                    crate::platform::terminate_cancellable_child(&mut child);
                    return Err(e.to_string());
                }
            }
        };
        while !out.is_finished()
            || !err.is_finished()
            || writer.as_ref().is_some_and(|writer| !writer.is_finished())
        {
            if self.cancelled.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(60) {
                crate::platform::terminate_cancellable_child(&mut child);
                return Err(
                    "GitHub output timed out or was cancelled. Refresh before retrying a mutation."
                        .into(),
                );
            }
            thread::sleep(Duration::from_millis(15));
        }
        let stdout = out
            .join()
            .map_err(|_| "GitHub stdout reader failed")?
            .map_err(|e| e.to_string())?;
        let stderr = err
            .join()
            .map_err(|_| "GitHub stderr reader failed")?
            .map_err(|e| e.to_string())?;
        if !status.success() {
            let message = serde_json::from_slice::<Value>(&stdout)
                .ok()
                .and_then(|v| api_error(&v));
            return Err(
                message.unwrap_or_else(|| String::from_utf8_lossy(&stderr).trim().to_owned())
            );
        }
        if let Some(writer) = writer {
            writer.join().map_err(|_| "GitHub stdin writer failed")??;
        }
        if stdout.len() > 32 * 1024 * 1024 || stderr.len() > 1024 * 1024 {
            return Err("GitHub response exceeded the size limit".into());
        }
        Ok(stdout)
    }
    fn api(&self, method: &str, endpoint: &str, body: Option<Value>) -> GithubResult<Value> {
        let mut args = vec![
            "api".into(),
            "--hostname".into(),
            "github.com".into(),
            "--method".into(),
            method.into(),
            endpoint.into(),
        ];
        if body.is_some() {
            args.extend(["--input".into(), "-".into()]);
        }
        let bytes = self.command(&args, body)?;
        if bytes.is_empty() {
            return Ok(Value::Null);
        }
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("Invalid GitHub JSON: {e}"))?;
        if let Some(error) = api_error(&value) {
            return Err(error);
        }
        Ok(value)
    }
    fn graphql(&self, query: &str, variables: Value) -> GithubResult<Value> {
        let value = self.api(
            "POST",
            "graphql",
            Some(json!({"query":query,"variables":variables})),
        )?;
        value
            .get("data")
            .cloned()
            .filter(|v| !v.is_null())
            .ok_or_else(|| "GitHub GraphQL returned no data".into())
    }
    fn all<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        field: Option<&str>,
    ) -> GithubResult<Vec<T>> {
        let mut items = Vec::new();
        for page in 1u64.. {
            let separator = if endpoint.contains('?') { '&' } else { '?' };
            let value = self.api(
                "GET",
                &format!("{endpoint}{separator}per_page=100&page={page}"),
                None,
            )?;
            let rows = match field {
                Some(field) => value
                    .get(field)
                    .ok_or_else(|| format!("GitHub response missing {field}"))?,
                None => &value,
            };
            let mut batch: Vec<T> = decode(rows.clone())?;
            let count = batch.len();
            items.append(&mut batch);
            if count < 100 {
                if field.is_some()
                    && value
                        .get("total_count")
                        .and_then(Value::as_u64)
                        .is_some_and(|total| total > items.len() as u64)
                {
                    return Err(
                        "GitHub returned a truncated collection. Refresh or narrow the query."
                            .into(),
                    );
                }
                return Ok(items);
            }
            if self.cancelled.load(Ordering::Relaxed) {
                return Err("GitHub request cancelled".into());
            }
        }
        unreachable!()
    }
    fn scope(&self) -> GithubResult<&ResolvedGithubScope> {
        self.scope
            .as_ref()
            .ok_or_else(|| "GitHub scope has not been resolved".into())
    }
    fn catalog(&self) -> GithubResult<Vec<GithubRepository>> {
        let scope = self.scope()?;
        if !scope.repositories.is_empty() {
            return Ok(scope.repositories.clone());
        }
        let endpoint = match &scope.organization {
            Some(org) => format!("orgs/{}/repos", org.as_str()),
            None => "user/repos?affiliation=owner,collaborator,organization_member".into(),
        };
        #[derive(Deserialize)]
        struct Repo {
            full_name: String,
        }
        let mut repos = self
            .all::<Repo>(&endpoint, None)?
            .into_iter()
            .map(|r| GithubRepository::parse(&r.full_name))
            .collect::<GithubResult<Vec<_>>>()?;
        repos.sort();
        repos.dedup();
        Ok(repos)
    }
    fn authorize(&self, repo: &GithubRepository) -> GithubResult<()> {
        let scope = self.scope()?;
        let allowed = if !scope.repositories.is_empty() {
            scope.repositories.contains(repo)
        } else if let Some(org) = &scope.organization {
            repo.owner.eq_ignore_ascii_case(org.as_str())
        } else {
            let repository = self.api("GET", &format!("repos/{repo}"), None)?;
            validate_rest_repository(&repository, repo)?;
            true
        };
        if allowed {
            Ok(())
        } else {
            Err(format!("Repository {repo} is outside this GitHub scope"))
        }
    }
    fn repository(&self, repo: &GithubRepository) -> GithubResult<RepositoryDetails> {
        self.authorize(repo)?;
        let mut value = self.api("GET", &format!("repos/{repo}"), None)?;
        if GithubRepository::parse(&required_string(&value, "full_name")?)? != *repo {
            return Err("GitHub redirected this repository outside its resolved identity. Resolve the scope again.".into());
        }
        value
            .as_object_mut()
            .ok_or("Invalid repository response")?
            .insert(
                "repository".into(),
                serde_json::to_value(repo).map_err(|e| e.to_string())?,
            );
        let settings = self.graphql("query($owner:String!,$name:String!){repository(owner:$owner,name:$name){mergeCommitAllowed squashMergeAllowed rebaseMergeAllowed autoMergeAllowed issues(states:OPEN){totalCount} pullRequests(states:OPEN){totalCount}}}",json!({"owner":repo.owner,"name":repo.repo}))?;
        let settings = settings
            .get("repository")
            .filter(|repo| !repo.is_null())
            .ok_or("Repository settings are unavailable")?;
        for (rest, graphql) in [
            ("allow_merge_commit", "mergeCommitAllowed"),
            ("allow_squash_merge", "squashMergeAllowed"),
            ("allow_rebase_merge", "rebaseMergeAllowed"),
            ("allow_auto_merge", "autoMergeAllowed"),
        ] {
            value[rest] = json!(required_bool(settings, graphql)?);
        }
        value["open_issues_count"] = json!(required_u64(
            settings.get("issues").ok_or("Missing issue count")?,
            "totalCount"
        )?);
        value["open_pull_requests_count"] = json!(required_u64(
            settings
                .get("pullRequests")
                .ok_or("Missing pull request count")?,
            "totalCount"
        )?);
        decode(value)
    }
    pub(crate) fn execute(&self, request: &GithubRequest) -> GithubResult<GithubResponse> {
        match request {
            GithubRequest::Viewer => Ok(GithubResponse::Viewer(decode(
                self.api("GET", "user", None)?,
            )?)),
            GithubRequest::Organizations { cursor, page_size } => {
                let size = page_size_value(*page_size)?;
                let binding = format!("organizations:{size}");
                let page = offset_cursor(cursor.as_deref(), &binding)?
                    .checked_add(1)
                    .ok_or("Organization pagination cursor overflowed")?;
                let items: Vec<Organization> = decode(self.api(
                    "GET",
                    &format!("user/orgs?per_page={size}&page={page}"),
                    None,
                )?)?;
                if items.len() > size {
                    return Err("GitHub returned too many organizations for this page".into());
                }
                for organization in &items {
                    let parsed = crate::app::state::GithubOrganization::parse(&organization.login)
                        .map_err(|_| "GitHub returned an invalid organization login")?;
                    if parsed.as_ref().map(|org| org.as_str()) != Some(organization.login.as_str())
                    {
                        return Err("GitHub returned an invalid organization login".into());
                    }
                }
                let next_cursor = if items.len() == size {
                    Some(make_offset(&binding, page)?)
                } else {
                    None
                };
                Ok(GithubResponse::Organizations(Page { items, next_cursor }))
            }
            GithubRequest::Repositories { cursor, page_size }
            | GithubRequest::ScopeRepositories { cursor, page_size } => {
                let binding = match request {
                    GithubRequest::ScopeRepositories { .. } => {
                        format!("scope-catalog:{:?}:{page_size}", self.scope()?)
                    }
                    _ => format!("catalog:{:?}", self.scope()?),
                };
                let offset = offset_cursor(cursor.as_deref(), &binding)?;
                let repos = self.catalog()?;
                let size = page_size_value(*page_size)?;
                if offset > repos.len() {
                    return Err("Repository cursor is no longer valid".into());
                }
                let end = offset.saturating_add(size).min(repos.len());
                let page = Page {
                    items: repos[offset..end].to_vec(),
                    next_cursor: if end < repos.len() {
                        Some(make_offset(&binding, end)?)
                    } else {
                        None
                    },
                };
                Ok(match request {
                    GithubRequest::ScopeRepositories { .. } => {
                        GithubResponse::ScopeRepositories(page)
                    }
                    _ => GithubResponse::Repositories(page),
                })
            }
            GithubRequest::Repository(repo) => {
                Ok(GithubResponse::Repository(self.repository(repo)?))
            }
            GithubRequest::Queue(request) => Ok(GithubResponse::Queue(self.queue(request)?)),
            GithubRequest::Overview {
                repository,
                page_size,
            } => {
                let request = |kind, queue| QueueRequest {
                    kind,
                    queue,
                    repository: repository.clone(),
                    cursor: None,
                    page_size: *page_size,
                };
                Ok(GithubResponse::Overview(Overview {
                    authored: self.queue(&request(ItemKind::PullRequest, Queue::Authored))?,
                    review_requested: self
                        .queue(&request(ItemKind::PullRequest, Queue::ReviewRequested))?,
                    assigned_issues: self.queue(&request(ItemKind::Issue, Queue::Assigned))?,
                }))
            }
            GithubRequest::Details(item) => {
                Ok(GithubResponse::Details(Box::new(self.details(item)?)))
            }
            GithubRequest::Diff { item, head_sha } => {
                self.authorize(&item.repository)?;
                require_pr(item)?;
                validate_sha(head_sha)?;
                let endpoint = format!("repos/{}/pulls/{}", item.repository, item.number);
                let before = self.api("GET", &endpoint, None)?;
                if before.pointer("/head/sha").and_then(Value::as_str) != Some(head_sha.as_str()) {
                    return Err(
                        "Pull request head changed. Refresh details before loading its diff."
                            .into(),
                    );
                }
                #[derive(Deserialize)]
                struct File {
                    filename: String,
                    previous_filename: Option<String>,
                    status: String,
                    patch: Option<String>,
                }
                let files = self.all::<File>(
                    &format!("repos/{}/pulls/{}/files", item.repository, item.number),
                    None,
                )?;
                let pull = self.api(
                    "GET",
                    &format!("repos/{}/pulls/{}", item.repository, item.number),
                    None,
                )?;
                if pull.pointer("/head/sha").and_then(Value::as_str) != Some(head_sha.as_str()) {
                    return Err(
                        "Pull request head changed while loading its diff. Refresh details.".into(),
                    );
                }
                validate_rest_repository(
                    pull.pointer("/base/repo")
                        .ok_or("Missing pull request base repository")?,
                    &item.repository,
                )?;
                if required_u64(&pull, "changed_files")? != files.len() as u64 {
                    return Err("GitHub truncated this pull request's file list; not all files can be displayed".into());
                }
                Ok(GithubResponse::Diff(
                    files
                        .into_iter()
                        .map(|f| DiffFile {
                            path: f.filename,
                            previous_path: f.previous_filename,
                            status: f.status,
                            patch: f.patch,
                        })
                        .collect(),
                ))
            }
            GithubRequest::Labels(repo) => {
                self.authorize(repo)?;
                Ok(GithubResponse::Labels(
                    self.all(&format!("repos/{repo}/labels"), None)?,
                ))
            }
            GithubRequest::Runs {
                repository,
                head_sha,
                cursor,
                page_size,
            } => {
                self.authorize(repository)?;
                let size = page_size_value(*page_size)?;
                if let Some(sha) = head_sha {
                    validate_sha(sha)?;
                }
                let binding = format!("runs:{repository}:{head_sha:?}:{size}");
                let page = offset_cursor(cursor.as_deref(), &binding)?
                    .checked_add(1)
                    .ok_or("Workflow pagination cursor overflowed")?;
                let mut endpoint =
                    format!("repos/{repository}/actions/runs?per_page={size}&page={page}");
                if let Some(sha) = head_sha {
                    endpoint.push_str(&format!("&head_sha={sha}"));
                }
                let value = self.api("GET", &endpoint, None)?;
                let total = required_u64(&value, "total_count")?;
                for run in value
                    .get("workflow_runs")
                    .and_then(Value::as_array)
                    .ok_or("Missing workflow runs")?
                {
                    validate_rest_repository(
                        run.get("repository").ok_or("Missing workflow repository")?,
                        repository,
                    )?;
                }
                let items: Vec<WorkflowRun> = decode(
                    value
                        .get("workflow_runs")
                        .cloned()
                        .ok_or("Missing workflow runs")?,
                )?;
                let more = (page as u64).saturating_mul(size as u64) < total;
                if more && items.is_empty() {
                    return Err("GitHub truncated the workflow runs; narrow the query".into());
                }
                if head_sha.is_some() && more && page.saturating_mul(size) >= 1000 {
                    return Err(
                        "GitHub's filtered workflow query exceeds 1000 results; narrow the query"
                            .into(),
                    );
                }
                Ok(GithubResponse::Runs(Page {
                    items,
                    next_cursor: if more {
                        Some(make_offset(&binding, page)?)
                    } else {
                        None
                    },
                }))
            }
            GithubRequest::Run { repository, run_id } => {
                self.authorize(repository)?;
                let run = self.api(
                    "GET",
                    &format!("repos/{repository}/actions/runs/{run_id}"),
                    None,
                )?;
                validate_rest_repository(
                    run.get("repository").ok_or("Missing workflow repository")?,
                    repository,
                )?;
                Ok(GithubResponse::Run(RunDetails {
                    run: decode(run)?,
                    jobs: self.all(
                        &format!("repos/{repository}/actions/runs/{run_id}/jobs?filter=all"),
                        Some("jobs"),
                    )?,
                }))
            }
            GithubRequest::Mutate(mutation) => {
                self.mutate(mutation)?;
                Ok(GithubResponse::Mutated)
            }
        }
    }
    fn queue(&self, request: &QueueRequest) -> GithubResult<Page<Summary>> {
        let size = page_size_value(request.page_size)?;
        if request.kind == ItemKind::Issue && request.queue == Queue::ReviewRequested {
            return Err("Issues do not have review requests".into());
        }
        let scope = self.scope()?;
        if request.queue == Queue::All
            && request.repository.is_none()
            && scope.repositories.is_empty()
            && scope.organization.is_none()
        {
            return Err("Select a repository before browsing All in personal scope".into());
        }
        let repos = if let Some(repo) = &request.repository {
            self.authorize(repo)?;
            vec![Some(repo.clone())]
        } else if !scope.repositories.is_empty() {
            scope.repositories.iter().cloned().map(Some).collect()
        } else {
            vec![None]
        };
        let organization = scope.organization.as_ref().map(|org| org.as_str());
        let binding =
            serde_json::to_string(&(repos.clone(), organization, request.kind, request.queue))
                .map_err(|e| e.to_string())?;
        let mut cursor: SearchCursor = match &request.cursor {
            Some(cursor) => decode_cursor(cursor)?,
            None => SearchCursor {
                binding: binding.clone(),
                streams: repos
                    .iter()
                    .cloned()
                    .map(|repo| SearchStream {
                        repo,
                        after: None,
                        done: false,
                        buffered: Vec::new(),
                    })
                    .collect(),
            },
        };
        if cursor.binding != binding
            || cursor.streams.len() != repos.len()
            || cursor.streams.iter().zip(&repos).any(|(s, r)| &s.repo != r)
        {
            return Err("Queue cursor does not belong to this scope or query".into());
        }
        if cursor.streams.iter().any(|stream| {
            stream.buffered.iter().any(|item| {
                stream
                    .repo
                    .as_ref()
                    .is_some_and(|repo| item.key.repository != *repo)
                    || organization
                        .is_some_and(|org| !item.key.repository.owner.eq_ignore_ascii_case(org))
                    || item.key.kind != request.kind
            })
        }) {
            return Err("Queue cursor contains an item outside its scope".into());
        }
        let mut items = Vec::with_capacity(size);
        while items.len() < size {
            for stream in &mut cursor.streams {
                if stream.buffered.is_empty() && !stream.done {
                    self.fill_stream(stream, request.kind, request.queue)?;
                }
            }
            let next = cursor
                .streams
                .iter()
                .enumerate()
                .filter_map(|(i, s)| s.buffered.first().map(|item| (i, item)))
                .max_by(|(_, a), (_, b)| {
                    a.updated_at
                        .cmp(&b.updated_at)
                        .then_with(|| b.key.repository.cmp(&a.key.repository))
                        .then_with(|| b.key.number.cmp(&a.key.number))
                })
                .map(|(i, _)| i);
            match next {
                Some(i) => items.push(cursor.streams[i].buffered.remove(0)),
                None => break,
            }
        }
        let more = cursor
            .streams
            .iter()
            .any(|s| !s.done || !s.buffered.is_empty());
        Ok(Page {
            items,
            next_cursor: if more {
                Some(encode_cursor(&cursor)?)
            } else {
                None
            },
        })
    }
    fn fill_stream(
        &self,
        stream: &mut SearchStream,
        kind: ItemKind,
        queue: Queue,
    ) -> GithubResult<()> {
        let data;
        let search;
        if let (Queue::All, Some(repo)) = (queue, &stream.repo) {
            let connection = match kind {
                ItemKind::PullRequest => "pullRequests",
                ItemKind::Issue => "issues",
            };
            let draft = if kind == ItemKind::PullRequest {
                "isDraft"
            } else {
                ""
            };
            let query = format!("query($owner:String!,$name:String!,$after:String){{repository(owner:$owner,name:$name){{{connection}(states:OPEN,first:100,after:$after,orderBy:{{field:UPDATED_AT,direction:DESC}}){{pageInfo{{hasNextPage endCursor}} nodes{{number title url state createdAt updatedAt {draft} author{{login}} repository{{nameWithOwner}}}}}}}}}}");
            data = self.graphql(
                &query,
                json!({"owner":repo.owner,"name":repo.repo,"after":stream.after}),
            )?;
            search = data
                .get("repository")
                .and_then(|repo| repo.get(connection))
                .ok_or("Missing repository queue")?;
        } else {
            let qualifier = match queue {
                Queue::Authored => "author:@me",
                Queue::ReviewRequested => "review-requested:@me",
                Queue::Assigned => "assignee:@me",
                Queue::Mentioned => "mentions:@me",
                Queue::All => "",
            };
            let item_type = match kind {
                ItemKind::PullRequest => "pr",
                ItemKind::Issue => "issue",
            };
            let scope = if let Some(repo) = &stream.repo {
                format!("repo:{repo}")
            } else if let Some(org) = &self.scope()?.organization {
                format!("org:{}", org.as_str())
            } else {
                String::new()
            };
            let query = format!(
                "{scope} is:{item_type} is:open archived:false sort:updated-desc {qualifier}"
            );
            data = self.graphql("query($query:String!,$after:String){search(query:$query,type:ISSUE,first:100,after:$after){issueCount pageInfo{hasNextPage endCursor} nodes{... on PullRequest{number title url state createdAt updatedAt isDraft author{login} repository{nameWithOwner}} ... on Issue{number title url state createdAt updatedAt author{login} repository{nameWithOwner}}}}}", json!({"query":query,"after":stream.after}))?;
            search = data.get("search").ok_or("Missing GitHub search result")?;
            if required_u64(search, "issueCount")? > 1000 {
                return Err("GitHub search exceeds 1000 results. Select a repository to narrow this queue; All within a repository has no search limit.".into());
            }
        }
        let nodes = search
            .get("nodes")
            .and_then(Value::as_array)
            .ok_or("Invalid search nodes")?;
        stream.buffered = nodes
            .iter()
            .map(|node| {
                let repository = GithubRepository::parse(&required_string(
                    node.get("repository")
                        .ok_or("GitHub search item is missing its repository")?,
                    "nameWithOwner",
                )?)?;
                if stream.repo.as_ref().is_some_and(|repo| *repo != repository)
                    || self
                        .scope()?
                        .organization
                        .as_ref()
                        .is_some_and(|org| !repository.owner.eq_ignore_ascii_case(org.as_str()))
                {
                    return Err("GitHub returned a queue item outside its requested scope".into());
                }
                parse_summary(
                    node,
                    ItemKey {
                        repository,
                        kind,
                        number: required_u64(node, "number")?,
                    },
                )
            })
            .collect::<GithubResult<_>>()?;
        let info = search.get("pageInfo").ok_or("Missing search page info")?;
        stream.done = !required_bool(info, "hasNextPage")?;
        let after = optional_string(info, "endCursor")?;
        if !stream.done && (after.is_none() || after == stream.after || nodes.is_empty()) {
            return Err("GitHub returned a non-advancing search cursor".into());
        }
        stream.after = after;
        Ok(())
    }
    fn details(&self, item: &ItemKey) -> GithubResult<ItemDetails> {
        self.authorize(&item.repository)?;
        if item.number == 0 {
            return Err("GitHub item numbers start at 1".into());
        }
        let repo = &item.repository;
        let issue = self.api("GET", &format!("repos/{repo}/issues/{}", item.number), None)?;
        if !required_string(&issue, "repository_url")?
            .eq_ignore_ascii_case(&format!("https://api.github.com/repos/{repo}"))
        {
            return Err("GitHub returned an item outside its requested repository".into());
        }
        if issue.get("pull_request").is_some() != (item.kind == ItemKind::PullRequest) {
            return Err("GitHub item kind does not match the requested item".into());
        }
        let author = issue
            .get("user")
            .and_then(|v| v.get("login"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let mut summary = Summary {
            key: item.clone(),
            title: required_string(&issue, "title")?,
            url: required_string(&issue, "html_url")?,
            state: required_string(&issue, "state")?,
            author,
            updated_at: required_string(&issue, "updated_at")?,
            created_at: required_string(&issue, "created_at")?,
            is_draft: false,
        };
        let mut details = ItemDetails {
            summary: summary.clone(),
            body: optional_string(&issue, "body")?,
            labels: decode(issue.get("labels").cloned().ok_or("Missing item labels")?)?,
            assignees: decode(issue.get("assignees").cloned().ok_or("Missing assignees")?)?,
            comments: self.all(
                &format!("repos/{repo}/issues/{}/comments", item.number),
                None,
            )?,
            review_comments: Vec::new(),
            reviews: Vec::new(),
            checks: Vec::new(),
            merge: None,
            additions: None,
            deletions: None,
            changed_files: None,
            locked: required_bool(&issue, "locked")?,
        };
        if item.kind == ItemKind::PullRequest {
            let pull = self.api("GET", &format!("repos/{repo}/pulls/{}", item.number), None)?;
            summary.is_draft = required_bool(&pull, "draft")?;
            if required_bool(&pull, "merged")? {
                summary.state = "merged".into();
            }
            details.summary = summary;
            details.additions = Some(required_u64(&pull, "additions")?);
            details.deletions = Some(required_u64(&pull, "deletions")?);
            details.changed_files = Some(required_u64(&pull, "changed_files")?);
            details.review_comments = self.all(
                &format!("repos/{repo}/pulls/{}/comments", item.number),
                None,
            )?;
            details.reviews =
                self.all(&format!("repos/{repo}/pulls/{}/reviews", item.number), None)?;
            let data = self.graphql("query($owner:String!,$name:String!,$number:Int!){repository(owner:$owner,name:$name){pullRequest(number:$number){id headRefOid headRefName baseRefName mergeable mergeStateStatus reviewDecision autoMergeRequest{enabledAt} mergeQueue{id} viewerCanUpdate}}}",json!({"owner":repo.owner,"name":repo.repo,"number":item.number}))?;
            let state = data
                .pointer("/repository/pullRequest")
                .filter(|v| !v.is_null())
                .ok_or("Pull request no longer exists")?;
            let head_sha = required_string(state, "headRefOid")?;
            if pull.pointer("/head/sha").and_then(Value::as_str) != Some(head_sha.as_str()) {
                return Err(
                    "Pull request head changed during loading. Refresh its details.".into(),
                );
            }
            details.checks = self.checks(repo, &head_sha)?;
            details.merge = Some(MergeState {
                node_id: required_string(state, "id")?,
                head_sha,
                head_branch: required_string(state, "headRefName")?,
                base_branch: required_string(state, "baseRefName")?,
                mergeable: required_string(state, "mergeable")?,
                merge_state_status: required_string(state, "mergeStateStatus")?,
                review_decision: optional_string(state, "reviewDecision")?,
                auto_merge_enabled: !state
                    .get("autoMergeRequest")
                    .ok_or("Missing auto merge state")?
                    .is_null(),
                queue_enabled: !state
                    .get("mergeQueue")
                    .ok_or("Missing merge queue state")?
                    .is_null(),
                viewer_can_update: required_bool(state, "viewerCanUpdate")?,
            });
        }
        Ok(details)
    }
    fn checks(&self, repo: &GithubRepository, sha: &str) -> GithubResult<Vec<Check>> {
        #[derive(Deserialize)]
        struct Run {
            name: String,
            status: String,
            conclusion: Option<String>,
            html_url: Option<String>,
        }
        #[derive(Deserialize)]
        struct Status {
            context: String,
            state: String,
            target_url: Option<String>,
        }
        let mut checks: Vec<Check> = self
            .all::<Run>(
                &format!("repos/{repo}/commits/{sha}/check-runs?filter=latest"),
                Some("check_runs"),
            )?
            .into_iter()
            .map(|r| Check {
                name: r.name,
                status: r.status,
                conclusion: r.conclusion,
                url: r.html_url,
            })
            .collect();
        let statuses = self.all::<Status>(&format!("repos/{repo}/commits/{sha}/statuses"), None)?;
        let mut contexts = std::collections::HashSet::new();
        for status in statuses {
            if contexts.insert(status.context.clone()) {
                checks.push(Check {
                    name: status.context,
                    status: if status.state == "pending" {
                        "in_progress".into()
                    } else {
                        "completed".into()
                    },
                    conclusion: if status.state == "pending" {
                        None
                    } else {
                        Some(status.state)
                    },
                    url: status.target_url,
                });
            }
        }
        Ok(checks)
    }
    fn mutate(&self, mutation: &GithubMutation) -> GithubResult<()> {
        let item = mutation.item();
        let repo = &item.repository;
        let details = self.details(item)?;
        let viewer: Viewer = decode(self.api("GET", "user", None)?)?;
        let repository = self.repository(repo)?;
        let author = details.summary.author.as_deref() == Some(viewer.login.as_str());
        let write = repository.permissions.push
            || repository.permissions.admin
            || repository.permissions.maintain == Some(true);
        let triage = write || repository.permissions.triage == Some(true);
        let is_open = details.summary.state.eq_ignore_ascii_case("open");
        let pr_state = |sha: &str| -> GithubResult<&MergeState> {
            require_pr(item)?;
            validate_sha(sha)?;
            let state = details.merge.as_ref().ok_or("Missing pull request state")?;
            if state.head_sha != sha {
                return Err(
                    "Pull request head changed. Refresh before performing this action.".into(),
                );
            }
            if !is_open {
                return Err("Pull request is no longer open".into());
            }
            Ok(state)
        };
        match mutation {
            GithubMutation::Comment { body, .. } => {
                validate_body(body)?;
                if details.locked && !write {
                    return Err("This conversation is locked".into());
                }
                self.api(
                    "POST",
                    &format!("repos/{repo}/issues/{}/comments", item.number),
                    Some(json!({"body":body})),
                )?;
            }
            GithubMutation::Reply {
                comment_id, body, ..
            } => {
                require_pr(item)?;
                validate_body(body)?;
                let comment = details
                    .review_comments
                    .iter()
                    .find(|c| c.id == *comment_id)
                    .ok_or("Reply target does not belong to this pull request")?;
                if details.locked && !write {
                    return Err("This conversation is locked".into());
                }
                let root = comment.in_reply_to_id.unwrap_or(comment.id);
                self.api(
                    "POST",
                    &format!("repos/{repo}/pulls/{}/comments/{root}/replies", item.number),
                    Some(json!({"body":body})),
                )?;
            }
            GithubMutation::EditComment {
                kind,
                comment_id,
                body,
                ..
            } => {
                validate_body(body)?;
                own_comment(&details, *kind, *comment_id, &viewer.login)?;
                self.api(
                    "PATCH",
                    &comment_endpoint(repo, *kind, *comment_id),
                    Some(json!({"body":body})),
                )?;
            }
            GithubMutation::DeleteComment {
                kind, comment_id, ..
            } => {
                own_comment(&details, *kind, *comment_id, &viewer.login)?;
                self.api("DELETE", &comment_endpoint(repo, *kind, *comment_id), None)?;
            }
            GithubMutation::InlineComment { comment, .. } => {
                pr_state(&comment.commit_id)?;
                validate_body(&comment.body)?;
                if details.locked && !write {
                    return Err("This conversation is locked".into());
                }
                if comment.path.is_empty() || comment.line == 0 {
                    return Err("Select a valid diff line".into());
                }
                if let Some((line, side)) = comment.start {
                    if line == 0 || line >= comment.line || side != comment.side {
                        return Err("A multiline comment must select an increasing range on the same diff side".into());
                    }
                }
                let mut body = json!({"body":comment.body,"commit_id":comment.commit_id,"path":comment.path,"line":comment.line,"side":comment.side});
                if let Some((line, side)) = comment.start {
                    body["start_line"] = json!(line);
                    body["start_side"] = json!(side);
                }
                self.api(
                    "POST",
                    &format!("repos/{repo}/pulls/{}/comments", item.number),
                    Some(body),
                )?;
            }
            GithubMutation::Review {
                head_sha,
                event,
                body,
                ..
            } => {
                pr_state(head_sha)?;
                if !matches!(event, ReviewEvent::Approve) {
                    validate_body(body)?;
                }
                if author && !matches!(event, ReviewEvent::Comment) {
                    return Err(
                        "You cannot approve or request changes on your own pull request".into(),
                    );
                }
                if details.locked && !write {
                    return Err("This conversation is locked".into());
                }
                let event = match event {
                    ReviewEvent::Comment => "COMMENT",
                    ReviewEvent::Approve => "APPROVE",
                    ReviewEvent::RequestChanges => "REQUEST_CHANGES",
                };
                self.api(
                    "POST",
                    &format!("repos/{repo}/pulls/{}/reviews", item.number),
                    Some(json!({"commit_id":head_sha,"event":event,"body":body})),
                )?;
            }
            GithubMutation::Labels { add, remove, .. } => {
                if !triage {
                    return Err("Changing labels requires triage permission".into());
                }
                if add.iter().chain(remove).any(|s| s.trim().is_empty()) {
                    return Err("Label names cannot be empty".into());
                }
                if add.iter().any(|s| remove.contains(s)) {
                    return Err("A label cannot be added and removed in the same action".into());
                }
                #[derive(Deserialize)]
                struct ExistingLabel {
                    name: String,
                    node_id: String,
                }
                let available = self.all::<ExistingLabel>(&format!("repos/{repo}/labels"), None)?;
                let ids = |names: &[String]| {
                    names.iter().map(|name|available.iter().find(|label|label.name.eq_ignore_ascii_case(name)).map(|label|label.node_id.clone()).ok_or_else(||format!("Label {name} no longer exists. Refresh labels before changing them."))).collect::<GithubResult<Vec<_>>>()
                };
                let add = ids(add)?;
                let remove = ids(remove)?;
                let issue =
                    self.api("GET", &format!("repos/{repo}/issues/{}", item.number), None)?;
                let id = required_string(&issue, "node_id")?;
                if !add.is_empty() {
                    self.graphql("mutation($id:ID!,$labels:[ID!]!){addLabelsToLabelable(input:{labelableId:$id,labelIds:$labels}){labelable{__typename}}}",json!({"id":id,"labels":add}))?;
                }
                if !remove.is_empty() {
                    self.graphql("mutation($id:ID!,$labels:[ID!]!){removeLabelsFromLabelable(input:{labelableId:$id,labelIds:$labels}){labelable{__typename}}}",json!({"id":id,"labels":remove}))?;
                }
            }
            GithubMutation::Draft {
                head_sha, draft, ..
            } => {
                let state = pr_state(head_sha)?;
                if !state.viewer_can_update || !(write || author) {
                    return Err("You cannot change this pull request's draft state".into());
                }
                if details.summary.is_draft == *draft {
                    return Err(
                        "Draft state has already changed. Refresh this pull request.".into(),
                    );
                }
                let query = if *draft {
                    "mutation($id:ID!){convertPullRequestToDraft(input:{pullRequestId:$id}){pullRequest{id}}}"
                } else {
                    "mutation($id:ID!){markPullRequestReadyForReview(input:{pullRequestId:$id}){pullRequest{id}}}"
                };
                self.graphql(query, json!({"id":state.node_id}))?;
            }
            GithubMutation::Close { .. } => {
                if !is_open {
                    return Err("This item is no longer open".into());
                }
                if !(author || triage) {
                    return Err("You cannot close this item".into());
                }
                self.api(
                    "PATCH",
                    &format!("repos/{repo}/issues/{}", item.number),
                    Some(json!({"state":"closed"})),
                )?;
            }
            GithubMutation::Merge {
                head_sha, action, ..
            } => {
                let state = pr_state(head_sha)?;
                if !write {
                    return Err("Merging requires repository write permission".into());
                }
                if matches!(action, MergeAction::DisableAuto) {
                    if !state.auto_merge_enabled {
                        return Err("Auto merge is not enabled".into());
                    }
                    self.graphql("mutation($id:ID!){disablePullRequestAutoMerge(input:{pullRequestId:$id}){pullRequest{id}}}",json!({"id":state.node_id}))?;
                    return Ok(());
                }
                if details.summary.is_draft {
                    return Err("Mark this pull request ready before merging".into());
                }
                let method = match action {
                    MergeAction::Now(m) | MergeAction::Auto(m) => *m,
                    MergeAction::DisableAuto => unreachable!(),
                };
                let allowed = match method {
                    MergeMethod::Merge => repository.allow_merge_commit,
                    MergeMethod::Squash => repository.allow_squash_merge,
                    MergeMethod::Rebase => repository.allow_rebase_merge,
                };
                if !state.queue_enabled && !allowed {
                    return Err("This merge method is disabled by the repository".into());
                }
                if state.mergeable != "MERGEABLE" {
                    return Err("Mergeability is unknown or the pull request has conflicts. Refresh before merging.".into());
                }
                if matches!(action, MergeAction::Auto(_)) {
                    if state.auto_merge_enabled {
                        return Err("Auto merge is already enabled".into());
                    }
                    if !state.queue_enabled && repository.allow_auto_merge != Some(true) {
                        return Err("Auto merge is disabled by the repository".into());
                    }
                } else {
                    if matches!(
                        state.review_decision.as_deref(),
                        Some("CHANGES_REQUESTED" | "REVIEW_REQUIRED")
                    ) || details.checks.iter().any(|c| {
                        c.status != "completed"
                            || !matches!(
                                c.conclusion.as_deref(),
                                Some("success" | "neutral" | "skipped")
                            )
                    }) {
                        return Err("Required reviews or checks are not ready".into());
                    }
                    if !state.queue_enabled
                        && !matches!(state.merge_state_status.as_str(), "CLEAN" | "HAS_HOOKS")
                    {
                        return Err(
                            "GitHub branch protection does not currently allow a safe merge".into(),
                        );
                    }
                }
                let mut args = vec![
                    "pr".into(),
                    "merge".into(),
                    item.number.to_string(),
                    "--repo".into(),
                    repo.as_str(),
                    "--match-head-commit".into(),
                    head_sha.clone(),
                ];
                if !state.queue_enabled {
                    args.push(
                        match method {
                            MergeMethod::Merge => "--merge",
                            MergeMethod::Squash => "--squash",
                            MergeMethod::Rebase => "--rebase",
                        }
                        .into(),
                    );
                }
                if matches!(action, MergeAction::Auto(_)) {
                    args.push("--auto".into());
                }
                self.command(&args, None)?;
            }
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct SearchCursor {
    binding: String,
    streams: Vec<SearchStream>,
}
#[derive(Serialize, Deserialize)]
struct SearchStream {
    repo: Option<GithubRepository>,
    after: Option<String>,
    done: bool,
    buffered: Vec<Summary>,
}
#[derive(Serialize, Deserialize)]
struct OffsetCursor {
    binding: String,
    offset: usize,
}
fn encode_cursor<T: Serialize>(value: &T) -> GithubResult<String> {
    serde_json::to_string(value).map_err(|e| e.to_string())
}
fn decode_cursor<T: DeserializeOwned>(value: &str) -> GithubResult<T> {
    if value.len() > 8 * 1024 * 1024 {
        return Err("GitHub cursor is too large".into());
    }
    serde_json::from_str(value).map_err(|_| "Invalid GitHub pagination cursor".into())
}
fn make_offset(binding: &str, offset: usize) -> GithubResult<String> {
    encode_cursor(&OffsetCursor {
        binding: binding.into(),
        offset,
    })
}
fn offset_cursor(cursor: Option<&str>, binding: &str) -> GithubResult<usize> {
    match cursor {
        None => Ok(0),
        Some(c) => {
            let cursor: OffsetCursor = decode_cursor(c)?;
            if cursor.binding != binding {
                return Err("Pagination cursor belongs to a different query or scope".into());
            }
            Ok(cursor.offset)
        }
    }
}
fn page_size_value(size: usize) -> GithubResult<usize> {
    if (1..=100).contains(&size) {
        Ok(size)
    } else {
        Err("Page size must be between 1 and 100".into())
    }
}
fn decode<T: DeserializeOwned>(value: Value) -> GithubResult<T> {
    serde_json::from_value(value).map_err(|e| format!("Invalid GitHub response: {e}"))
}
fn required_string(value: &Value, key: &str) -> GithubResult<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("GitHub response missing string {key}"))
}
fn optional_string(value: &Value, key: &str) -> GithubResult<Option<String>> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        _ => Err(format!("Invalid GitHub string {key}")),
    }
}
fn required_u64(value: &Value, key: &str) -> GithubResult<u64> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("GitHub response missing integer {key}"))
}
fn required_bool(value: &Value, key: &str) -> GithubResult<bool> {
    value
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("GitHub response missing boolean {key}"))
}
fn api_error(value: &Value) -> Option<String> {
    if let Some(errors) = value
        .get("errors")
        .and_then(Value::as_array)
        .filter(|e| !e.is_empty())
    {
        return Some(
            errors
                .iter()
                .map(|e| {
                    e.get("message")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                        .unwrap_or_else(|| e.to_string())
                })
                .collect::<Vec<_>>()
                .join("; "),
        );
    }
    value
        .get("message")
        .and_then(Value::as_str)
        .filter(|_| value.get("documentation_url").is_some() || value.get("status").is_some())
        .map(str::to_owned)
}
fn parse_summary(node: &Value, key: ItemKey) -> GithubResult<Summary> {
    Ok(Summary {
        is_draft: if key.kind == ItemKind::PullRequest {
            required_bool(node, "isDraft")?
        } else {
            false
        },
        key,
        title: required_string(node, "title")?,
        url: required_string(node, "url")?,
        state: required_string(node, "state")?,
        author: node
            .get("author")
            .and_then(|v| v.get("login"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        updated_at: required_string(node, "updatedAt")?,
        created_at: required_string(node, "createdAt")?,
    })
}
fn require_pr(item: &ItemKey) -> GithubResult<()> {
    if item.kind == ItemKind::PullRequest {
        Ok(())
    } else {
        Err("This action requires a pull request".into())
    }
}
fn validate_sha(sha: &str) -> GithubResult<()> {
    if sha.len() == 40 && sha.bytes().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("A full 40-character commit SHA is required".into())
    }
}
fn validate_body(body: &str) -> GithubResult<()> {
    if body.trim().is_empty() {
        Err("Comment body cannot be empty".into())
    } else if body.len() > 65536 {
        Err("Comment exceeds GitHub's body limit".into())
    } else {
        Ok(())
    }
}
fn own_comment(details: &ItemDetails, kind: CommentKind, id: u64, login: &str) -> GithubResult<()> {
    let comments = match kind {
        CommentKind::General => &details.comments,
        CommentKind::Review => &details.review_comments,
    };
    let comment = comments
        .iter()
        .find(|c| c.id == id)
        .ok_or("Comment does not belong to this item")?;
    if comment.user.as_ref().map(|u| u.login.as_str()) != Some(login) {
        return Err("Only your own comments can be edited or deleted".into());
    }
    Ok(())
}
fn comment_endpoint(repo: &GithubRepository, kind: CommentKind, id: u64) -> String {
    format!(
        "repos/{repo}/{}/comments/{id}",
        match kind {
            CommentKind::General => "issues",
            CommentKind::Review => "pulls",
        }
    )
}

fn validate_rest_repository(value: &Value, expected: &GithubRepository) -> GithubResult<()> {
    if GithubRepository::parse(&required_string(value, "full_name")?)? != *expected {
        return Err("GitHub returned a resource outside its requested repository".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    struct Fixture(std::path::PathBuf);

    #[cfg(unix)]
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(unix)]
    fn fake_service(script: &str, scope: ResolvedGithubScope) -> (Fixture, GithubService) {
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!(
            "gardn-github-service-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos(),
        ));
        std::fs::create_dir(&root).expect("fixture directory");
        let program = root.join("gh");
        std::fs::write(&program, script).expect("fake GitHub executable");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
            .expect("executable permissions");
        (
            Fixture(root),
            GithubService {
                scope: Some(scope),
                cancelled: Arc::new(AtomicBool::new(false)),
                program,
            },
        )
    }

    #[cfg(unix)]
    #[test]
    fn organizations_paginate_authenticated_endpoint() {
        let (_fixture, mut service) = fake_service(
            r#"#!/bin/sh
case "$6" in
  "user/orgs?per_page=2&page=1") printf '%s' '[{"login":"acme"},{"login":"beta"}]' ;;
  "user/orgs?per_page=2&page=2") printf '%s' '[{"login":"zeta"}]' ;;
  *) printf '%s' '{}' ;;
esac
"#,
            ResolvedGithubScope {
                repositories: Vec::new(),
                organization: None,
            },
        );
        let GithubResponse::Organizations(first) = service
            .execute(&GithubRequest::Organizations {
                cursor: None,
                page_size: 2,
            })
            .expect("first organization page")
        else {
            panic!("wrong response")
        };
        assert_eq!(
            first
                .items
                .iter()
                .map(|organization| organization.login.as_str())
                .collect::<Vec<_>>(),
            vec!["acme", "beta"]
        );
        service.scope = Some(ResolvedGithubScope {
            repositories: Vec::new(),
            organization: Some(
                crate::app::state::GithubOrganization::parse("acme")
                    .expect("valid organization")
                    .expect("organization"),
            ),
        });
        let GithubResponse::Organizations(second) = service
            .execute(&GithubRequest::Organizations {
                cursor: first.next_cursor,
                page_size: 2,
            })
            .expect("second organization page")
        else {
            panic!("wrong response")
        };
        assert_eq!(second.items[0].login, "zeta");
        assert!(second.next_cursor.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn organizations_reject_non_array_response() {
        let (_fixture, service) = fake_service(
            "#!/bin/sh\nprintf '%s' '{\"login\":\"acme\"}'\n",
            ResolvedGithubScope {
                repositories: Vec::new(),
                organization: None,
            },
        );
        let result = service.execute(&GithubRequest::Organizations {
            cursor: None,
            page_size: 20,
        });
        assert!(matches!(result, Err(error) if error.contains("Invalid GitHub response")));
    }

    #[cfg(unix)]
    #[test]
    fn organizations_reject_invalid_login() {
        let (_fixture, service) = fake_service(
            "#!/bin/sh\nprintf '%s' '[{\"login\":\"bad/login\"}]'\n",
            ResolvedGithubScope {
                repositories: Vec::new(),
                organization: None,
            },
        );
        let result = service.execute(&GithubRequest::Organizations {
            cursor: None,
            page_size: 20,
        });
        assert!(matches!(result, Err(error) if error.contains("invalid organization login")));
    }

    #[cfg(unix)]
    #[test]
    fn organization_cursor_rejects_page_size_change() {
        let (_fixture, service) = fake_service(
            "#!/bin/sh\nprintf '%s' '[{\"login\":\"acme\"}]'\n",
            ResolvedGithubScope {
                repositories: Vec::new(),
                organization: None,
            },
        );
        let GithubResponse::Organizations(page) = service
            .execute(&GithubRequest::Organizations {
                cursor: None,
                page_size: 1,
            })
            .expect("organization page")
        else {
            panic!("wrong response")
        };
        let result = service.execute(&GithubRequest::Organizations {
            cursor: page.next_cursor,
            page_size: 2,
        });
        assert!(matches!(result, Err(error) if error.contains("different query or scope")));
    }

    #[test]
    fn scope_repository_cursor_is_not_a_repository_cursor() {
        let repositories = ["team/one", "team/two"]
            .into_iter()
            .map(|name| GithubRepository::parse(name).expect("repository"))
            .collect::<Vec<_>>();
        let service = GithubService {
            scope: Some(ResolvedGithubScope {
                repositories: repositories.clone(),
                organization: None,
            }),
            cancelled: Arc::new(AtomicBool::new(false)),
            program: "gh".into(),
        };
        let GithubResponse::ScopeRepositories(page) = service
            .execute(&GithubRequest::ScopeRepositories {
                cursor: None,
                page_size: 1,
            })
            .expect("scope repository page")
        else {
            panic!("wrong response")
        };
        let result = service.execute(&GithubRequest::Repositories {
            cursor: page.next_cursor,
            page_size: 1,
        });
        assert!(matches!(result, Err(error) if error.contains("different query or scope")));
    }

    #[cfg(unix)]
    #[test]
    fn personal_authored_queue_includes_public_contributions_outside_catalog() {
        use std::os::unix::fs::PermissionsExt;
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let fixture = Fixture(std::env::temp_dir().join(format!(
                "gardn-personal-queue-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .expect("clock")
                    .as_nanos(),
            )));
        std::fs::create_dir(&fixture.0).expect("fixture directory");
        let program = fixture.0.join("gh");
        std::fs::write(&program, r#"#!/bin/sh
case " $* " in
  *" graphql "*)
    cat >/dev/null
    printf '%s' '{"data":{"search":{"issueCount":1,"pageInfo":{"hasNextPage":false,"endCursor":null},"nodes":[{"number":17,"title":"Public contribution","url":"https://github.com/community/project/pull/17","state":"OPEN","createdAt":"2026-06-01T00:00:00Z","updatedAt":"2026-06-02T00:00:00Z","isDraft":false,"author":{"login":"contributor"},"repository":{"nameWithOwner":"community/project"}}]}}}'
    ;;
  *) printf '%s' '[]' ;;
esac
"#).expect("fake GitHub executable");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))
            .expect("executable permissions");
        let service = GithubService {
            scope: Some(ResolvedGithubScope {
                repositories: Vec::new(),
                organization: None,
            }),
            cancelled: Arc::new(AtomicBool::new(false)),
            program,
        };
        let GithubResponse::Queue(page) = service
            .execute(&GithubRequest::Queue(QueueRequest {
                kind: ItemKind::PullRequest,
                queue: Queue::Authored,
                repository: None,
                cursor: None,
                page_size: 20,
            }))
            .expect("personal authored queue")
        else {
            panic!("expected queue")
        };
        assert_eq!(
            page.items
                .iter()
                .map(|item| (item.key.repository.as_str(), item.key.number))
                .collect::<Vec<_>>(),
            vec![("community/project".to_string(), 17)]
        );
        assert!(page.next_cursor.is_none());
    }

    #[test]
    fn repository_pages_preserve_every_selected_repository() {
        let repositories = ["team/one", "team/two", "team/three"]
            .into_iter()
            .map(|name| GithubRepository::parse(name).expect("repository"))
            .collect::<Vec<_>>();
        let service = GithubService {
            scope: Some(ResolvedGithubScope {
                repositories: repositories.clone(),
                organization: None,
            }),
            cancelled: Arc::new(AtomicBool::new(false)),
            program: "gh".into(),
        };
        let mut cursor = None;
        let mut actual = Vec::new();
        loop {
            let response = service
                .execute(&GithubRequest::Repositories {
                    cursor,
                    page_size: 2,
                })
                .expect("repository page");
            let GithubResponse::Repositories(page) = response else {
                panic!("wrong response")
            };
            actual.extend(page.items);
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(actual, repositories);
    }

    #[test]
    fn repository_cursor_cannot_be_reused_in_another_scope() {
        let make_service = |names: &[&str]| GithubService {
            scope: Some(ResolvedGithubScope {
                repositories: names
                    .iter()
                    .map(|name| GithubRepository::parse(name).expect("repository"))
                    .collect(),
                organization: None,
            }),
            cancelled: Arc::new(AtomicBool::new(false)),
            program: "gh".into(),
        };
        let service = make_service(&["team/one", "team/two"]);
        let GithubResponse::Repositories(page) = service
            .execute(&GithubRequest::Repositories {
                cursor: None,
                page_size: 1,
            })
            .expect("first page")
        else {
            panic!("wrong response")
        };
        let other = make_service(&["team/three", "team/four"]);
        let error = other
            .execute(&GithubRequest::Repositories {
                cursor: page.next_cursor,
                page_size: 1,
            })
            .expect_err("foreign scope cursor");
        assert!(error.contains("different query or scope"), "{error}");
    }

    #[test]
    fn mutation_cannot_target_a_repository_outside_selected_scope() {
        let service = GithubService {
            scope: Some(ResolvedGithubScope {
                repositories: vec![GithubRepository::parse("team/one").expect("repository")],
                organization: None,
            }),
            cancelled: Arc::new(AtomicBool::new(false)),
            program: "gh".into(),
        };
        let request = GithubRequest::Mutate(GithubMutation::Close {
            item: ItemKey {
                repository: GithubRepository::parse("other/private").expect("repository"),
                number: 1,
                kind: ItemKind::Issue,
            },
        });
        let error = service
            .execute(&request)
            .expect_err("out-of-scope mutation");
        assert!(
            error.contains("other/private is outside this GitHub scope"),
            "{error}"
        );
    }
}
