use super::*;

impl GithubScreen {
    pub(super) fn invalidate(&mut self) {
        self.refresh_generation = false;
        self.force_refresh = false;
        if self.tab == GithubTab::Actions && self.repository.is_none() {
            let interrupted = self
                .pending
                .values()
                .map(|tracked| &tracked.request)
                .chain(self.queued.iter())
                .filter_map(|request| match request {
                    GithubRequest::Runs {
                        repository, cursor, ..
                    } => Some((repository.clone(), cursor.clone())),
                    _ => None,
                })
                .collect::<Vec<_>>();
            for request in self
                .pending
                .values()
                .map(|tracked| &tracked.request)
                .chain(self.queued.iter())
            {
                if let GithubRequest::Repositories { cursor, .. } = request {
                    self.catalog_cursor = cursor.clone();
                }
            }
            self.runs_backlog.extend(interrupted);
        }
        self.generation = self.generation.wrapping_add(1);
        self.obsolete.extend(self.pending.keys().copied());
        self.pending.clear();
        self.queued.clear();
        self.awaiting_ids.clear();
        self.error = None;
        self.notice = None;
    }
    pub(super) fn clear_detail(&mut self) {
        self.detail = None;
        self.selected_key = None;
        self.selected_run = None;
        self.detail_rows.clear();
        self.detail_scroll = 0;
        self.selected_row = None;
        self.merge_repository = None;
        self.labels.clear();
        self.rows_dirty = true;
        self.diff = None;
        self.file_scroll = 0;
        self.file_filter.clear();
        self.diff_drag = false;
    }
    pub(super) fn refresh(&mut self) {
        self.invalidate();
        self.clear_detail();
        self.entries.clear();
        self.selected = 0;
        self.list_scroll = 0;
        self.entries_dirty = true;
        self.runs_backlog.clear();
        self.runs_cursors.clear();
        self.catalog_cursor = None;
        self.next_cursor = None;
        self.overview_cursors.clear();
        if self.viewer.is_none() {
            self.enqueue(GithubRequest::Viewer);
        }
        match self.tab {
            GithubTab::Overview => self.enqueue(GithubRequest::Overview {
                repository: self.repository.clone(),
                page_size: 30,
            }),
            GithubTab::Repositories => self.enqueue(GithubRequest::Repositories {
                cursor: None,
                page_size: 50,
            }),
            GithubTab::PullRequests | GithubTab::Issues => self.request_queue(None),
            GithubTab::Actions => {
                if let Some(repository) = self.repository.clone() {
                    self.enqueue(GithubRequest::Runs {
                        repository,
                        head_sha: self.run_sha.clone(),
                        cursor: None,
                        page_size: 50,
                    });
                } else if self.scope.repositories.is_empty() {
                    self.enqueue(GithubRequest::Repositories {
                        cursor: None,
                        page_size: 50,
                    });
                } else {
                    self.runs_backlog.extend(
                        self.scope
                            .repositories
                            .iter()
                            .cloned()
                            .map(|repository| (repository, None)),
                    );
                    self.fill_runs_requests();
                }
            }
        }
    }
    pub(super) fn request_queue(&mut self, cursor: Option<String>) {
        self.enqueue(GithubRequest::Queue(QueueRequest {
            kind: if self.tab == GithubTab::Issues {
                ItemKind::Issue
            } else {
                ItemKind::PullRequest
            },
            queue: self.queue,
            repository: self.repository.clone(),
            cursor,
            page_size: 50,
        }));
    }
    pub(super) fn more(&mut self) {
        if self.loading() {
            return;
        }
        if self.tab == GithubTab::Actions && self.repository.is_none() {
            self.more_scoped_runs();
            return;
        }
        if self.tab == GithubTab::Overview {
            for (queue, kind, cursor) in self.overview_cursors.clone() {
                if cursor.is_some() {
                    self.enqueue(GithubRequest::Queue(QueueRequest {
                        kind,
                        queue,
                        repository: self.repository.clone(),
                        cursor,
                        page_size: 30,
                    }));
                }
            }
        } else if let Some(cursor) = self.next_cursor.clone() {
            match self.tab {
                GithubTab::Repositories => self.enqueue(GithubRequest::Repositories {
                    cursor: Some(cursor),
                    page_size: 50,
                }),
                GithubTab::PullRequests | GithubTab::Issues => self.request_queue(Some(cursor)),
                GithubTab::Actions => {
                    if let Some(repository) = self.repository.clone() {
                        self.enqueue(GithubRequest::Runs {
                            repository,
                            head_sha: self.run_sha.clone(),
                            cursor: Some(cursor),
                            page_size: 50,
                        });
                    }
                }
                GithubTab::Overview => {}
            }
        }
    }
    pub fn has_more(&self) -> bool {
        self.next_cursor.is_some()
            || self.overview_cursors.iter().any(|(_, _, c)| c.is_some())
            || self.catalog_cursor.is_some()
            || !self.runs_cursors.is_empty()
            || !self.runs_backlog.is_empty()
    }
    pub fn visible_entries(&self) -> &[usize] {
        &self.visible
    }
    pub(super) fn update_visible_entries(&mut self) {
        if !self.entries_dirty {
            return;
        }
        self.entries_dirty = false;
        let filter = self.filter.to_lowercase();
        self.visible = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                let matches_run = match entry {
                    Entry::Run(_, run) => match self.run_filter {
                        RunFilter::All => true,
                        RunFilter::Failed => run.conclusion.as_deref().is_some_and(is_failure),
                        RunFilter::Running => run.status != "completed",
                    },
                    _ => true,
                };
                matches_run && (filter.is_empty() || entry.label().to_lowercase().contains(&filter))
            })
            .map(|(index, _)| index)
            .collect();
    }
    pub(super) fn open_selected(&mut self) {
        let Some(entry) = self
            .visible_entries()
            .get(self.selected)
            .and_then(|index| self.entries.get(*index))
            .cloned()
        else {
            return;
        };
        self.invalidate();
        self.clear_detail();
        self.detail_tab = DetailTab::Description;
        self.focus = Focus::Detail;
        if self.viewer.is_none() {
            self.enqueue(GithubRequest::Viewer);
        }
        match entry {
            Entry::Item(item) => {
                self.selected_key = Some(item.key.clone());
                self.enqueue(GithubRequest::Details(item.key));
            }
            Entry::Repository(repository) => {
                self.repository = Some(repository.clone());
                self.enqueue(GithubRequest::Repository(repository));
            }
            Entry::Run(repository, run) => {
                self.selected_run = Some((repository.clone(), run.id));
                self.enqueue(GithubRequest::Run {
                    repository,
                    run_id: run.id,
                });
            }
            Entry::Heading(_) => {
                self.focus = Focus::List;
            }
        }
    }
    pub fn apply(&mut self, request_id: u64, result: Result<GithubResponse, String>) {
        let Some(tracked) = self.pending.remove(&request_id) else {
            return;
        };
        if tracked.generation != self.generation {
            return;
        }
        let mutation = matches!(tracked.request, GithubRequest::Mutate(_));
        if mutation {
            self.submitting = false;
        }
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                match tracked.request {
                    GithubRequest::Runs {
                        repository, cursor, ..
                    } if self.repository.is_none() => {
                        self.runs_backlog.push_front((repository, cursor))
                    }
                    GithubRequest::Repositories { cursor, .. }
                        if self.tab == GithubTab::Actions =>
                    {
                        self.catalog_cursor = cursor
                    }
                    _ => {}
                }
                self.error = Some(error);
                self.compute(self.geometry.area);
                return;
            }
        };
        match response {
            GithubResponse::Viewer(viewer) => self.viewer = Some(viewer),
            GithubResponse::Repositories(page) => {
                if self.tab == GithubTab::Actions {
                    self.catalog_cursor = page.next_cursor;
                    self.runs_backlog
                        .extend(page.items.into_iter().map(|repository| (repository, None)));
                } else {
                    self.next_cursor = page.next_cursor;
                    self.entries
                        .extend(page.items.into_iter().map(Entry::Repository));
                }
            }
            GithubResponse::Repository(repository) => {
                if matches!(tracked.request, GithubRequest::Repository(_))
                    && self.selected_key.is_some()
                {
                    self.merge_repository = Some(repository);
                } else if self.tab == GithubTab::Repositories {
                    self.detail = Some(Detail::Repository(repository));
                }
            }
            GithubResponse::Overview(overview) => {
                self.entries.clear();
                for (heading, queue, kind, page) in [
                    (
                        "Authored pull requests",
                        Queue::Authored,
                        ItemKind::PullRequest,
                        overview.authored,
                    ),
                    (
                        "Review requested",
                        Queue::ReviewRequested,
                        ItemKind::PullRequest,
                        overview.review_requested,
                    ),
                    (
                        "Assigned issues",
                        Queue::Assigned,
                        ItemKind::Issue,
                        overview.assigned_issues,
                    ),
                ] {
                    self.entries.push(Entry::Heading(heading.into()));
                    self.entries.extend(page.items.into_iter().map(Entry::Item));
                    self.overview_cursors.push((queue, kind, page.next_cursor));
                }
            }
            GithubResponse::Queue(page) => {
                if self.tab == GithubTab::Overview {
                    if let GithubRequest::Queue(request) = tracked.request {
                        if let Some(slot) =
                            self.overview_cursors.iter_mut().find(|(queue, kind, _)| {
                                *queue == request.queue && *kind == request.kind
                            })
                        {
                            slot.2 = page.next_cursor;
                        }
                        let heading = match request.queue {
                            Queue::Authored => "Authored pull requests",
                            Queue::ReviewRequested => "Review requested",
                            _ => "Assigned issues",
                        };
                        let start = self
                            .entries
                            .iter()
                            .position(
                                |entry| matches!(entry, Entry::Heading(text) if text == heading),
                            )
                            .map_or(self.entries.len(), |i| i + 1);
                        let end = self.entries[start..]
                            .iter()
                            .position(|entry| matches!(entry, Entry::Heading(_)))
                            .map_or(self.entries.len(), |i| start + i);
                        self.entries
                            .splice(end..end, page.items.into_iter().map(Entry::Item));
                    }
                } else {
                    self.next_cursor = page.next_cursor;
                    self.entries.extend(page.items.into_iter().map(Entry::Item));
                }
            }
            GithubResponse::Details(details) => {
                if self.selected_key.as_ref() == Some(&details.summary.key) {
                    let previous_head = self
                        .item()
                        .and_then(|item| item.merge.as_ref())
                        .map(|merge| &merge.head_sha);
                    let next_head = details.merge.as_ref().map(|merge| &merge.head_sha);
                    if previous_head != next_head {
                        self.diff = None;
                    }
                    let selected = self
                        .selected_comment()
                        .map(|(comment, kind)| (comment.id, matches!(kind, CommentKind::Review)));
                    self.selected_row = selected.and_then(|(id, review)| {
                        let comments = if review {
                            &details.review_comments
                        } else {
                            &details.comments
                        };
                        comments
                            .iter()
                            .position(|comment| comment.id == id)
                            .map(|index| RowTarget::Comment { review, index })
                    });
                    self.detail = Some(Detail::Item(details));
                    if self.detail_tab == DetailTab::Diff && self.diff.is_none() {
                        self.request_diff();
                    }
                }
            }
            GithubResponse::Labels(labels) => self.labels = labels,
            GithubResponse::Runs(page) => {
                if let GithubRequest::Runs { repository, .. } = tracked.request {
                    self.apply_runs_page(repository, page);
                }
            }
            GithubResponse::Run(run) => {
                if self
                    .selected_run
                    .as_ref()
                    .is_some_and(|(_, id)| *id == run.run.id)
                {
                    self.detail = Some(Detail::Run(run));
                }
            }
            GithubResponse::Mutated => {
                self.dialog = None;
                self.error = None;
                self.notice = Some("Saved to GitHub.".into());
                if let Some(item) = self.selected_key.clone() {
                    self.enqueue(GithubRequest::Details(item));
                }
            }
            GithubResponse::Diff(files) => {
                if matches!(&tracked.request, GithubRequest::Diff { item, head_sha } if self.selected_key.as_ref() == Some(item) && self.item().and_then(|item| item.merge.as_ref()).is_some_and(|merge| merge.head_sha == *head_sha))
                {
                    self.diff = Some(DiffViewState::new(files));
                }
            }
        }
        self.rows_dirty = true;
        self.entries_dirty = true;
        if self.error.is_none() {
            self.fill_runs_requests();
        }
        self.compute(self.geometry.area);
    }
}
