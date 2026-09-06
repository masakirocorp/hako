use super::*;

impl GithubScreen {
    pub(super) fn current_url(&self) -> Option<String> {
        if self.focus == Focus::Detail {
            if let Some(url) = self.selected_link.and_then(|key| self.link_url(key)) {
                return Some(url.to_owned());
            }
        }
        if let Some((comment, _)) = self.selected_comment() {
            return Some(comment.html_url.clone());
        }
        match &self.detail {
            Some(Detail::Item(item)) => {
                if let Some(RowTarget::Check(index)) = self.selected_row {
                    return item.checks.get(index).and_then(|check| check.url.clone());
                }
                Some(item.summary.url.clone())
            }
            Some(Detail::Repository(repo)) => Some(repo.html_url.clone()),
            Some(Detail::Run(run)) => {
                if let Some(RowTarget::Job(index)) = self.selected_row {
                    return run.jobs.get(index).map(|job| job.html_url.clone());
                }
                Some(run.run.html_url.clone())
            }
            None => self
                .visible_entries()
                .get(self.selected)
                .and_then(|index| self.entries.get(*index))
                .and_then(|entry| match entry {
                    Entry::Item(item) => Some(item.url.clone()),
                    Entry::Repository(repo) => Some(format!("https://github.com/{repo}")),
                    Entry::Run(_, run) => Some(run.html_url.clone()),
                    Entry::Heading(_) => None,
                }),
        }
    }
    pub fn available_queues(&self) -> &'static [Queue] {
        match self.tab {
            GithubTab::PullRequests => &[
                Queue::Authored,
                Queue::Assigned,
                Queue::Mentioned,
                Queue::All,
                Queue::ReviewRequested,
            ],
            GithubTab::Issues => &[
                Queue::Authored,
                Queue::Assigned,
                Queue::Mentioned,
                Queue::All,
            ],
            _ => &[],
        }
    }
    pub fn contextual_actions(&self) -> Vec<(GithubAction, String)> {
        use GithubAction as A;
        if self.submitting {
            return Vec::new();
        }
        if let Some(dialog) = &self.dialog {
            let mut actions = match dialog {
                Dialog::Filter(_) | Dialog::FileSearch(_) | Dialog::Composer { .. } => {
                    vec![(A::Submit, "Submit".into())]
                }
                Dialog::Confirm { .. } => vec![(A::Submit, "Confirm".into())],
                Dialog::Labels { .. } => self
                    .labels
                    .iter()
                    .enumerate()
                    .map(|(index, label)| {
                        let applied = self.item().is_some_and(|item| {
                            item.labels.iter().any(|current| current.name == label.name)
                        });
                        (
                            A::ToggleLabel(index),
                            format!("{} {}", if applied { "Remove" } else { "Add" }, label.name),
                        )
                    })
                    .collect(),
                Dialog::Merge => self.merge_actions(),
            };
            actions.push((A::Cancel, "Cancel".into()));
            return actions;
        }
        let mut actions = vec![
            (A::Refresh, "Refresh".into()),
            (A::Filter, "Filter loaded results".into()),
        ];
        if self.has_more() {
            actions.push((A::More, "Load more".into()));
        }
        if self.repository.is_some() {
            actions.push((A::ResetRepository, "Reset repository narrowing".into()));
        }
        actions.extend(
            GithubTab::ALL
                .into_iter()
                .map(|tab| (A::Tab(tab), tab.label().into())),
        );
        actions.extend(
            self.available_queues()
                .iter()
                .map(|&queue| (A::Queue(queue), queue_label(queue).into())),
        );
        if self.tab == GithubTab::Actions {
            actions.extend([
                (A::Runs(RunFilter::All), "All runs".into()),
                (A::Runs(RunFilter::Failed), "Failed runs".into()),
                (A::Runs(RunFilter::Running), "Running runs".into()),
            ]);
        }
        if self.current_url().is_some() {
            actions.extend([
                (A::Browser, "Open in browser".into()),
                (A::CopyUrl, "Copy URL".into()),
            ]);
        }
        actions.push((A::Editor, "Open repository in editor".into()));
        if self.detail.is_some() || self.selected_key.is_some() || self.selected_run.is_some() {
            actions.push((A::Back, "Back to list".into()));
        } else {
            actions.push((A::Open, "Open selected".into()));
        }
        if let Some(item) = self.item() {
            actions.extend([
                (A::Detail(DetailTab::Description), "Description".into()),
                (A::Detail(DetailTab::Conversation), "Conversation".into()),
                (A::Comment, "Write comment".into()),
                (A::Labels, "Edit labels".into()),
            ]);
            if self.selected_comment().is_some() {
                actions.push((A::Reply, "Reply to selected comment".into()));
            }
            if self.owns_selected_comment() {
                actions.extend([
                    (A::EditComment, "Edit your comment".into()),
                    (A::DeleteComment, "Delete your comment".into()),
                ]);
            }
            if item.summary.state.eq_ignore_ascii_case("open") {
                actions.push((A::CloseItem, "Close item".into()));
            }
            if item.summary.key.kind == ItemKind::PullRequest {
                actions.extend([
                    (A::Detail(DetailTab::Diff), "Diff".into()),
                    (A::Detail(DetailTab::Checks), "Checks".into()),
                    (A::PullRequestRuns, "Actions for pull request SHA".into()),
                    (A::ReviewComment, "Submit review comment".into()),
                    (A::Approve, "Approve pull request".into()),
                    (A::RequestChanges, "Request changes".into()),
                ]);
                if item.summary.state.eq_ignore_ascii_case("open") {
                    actions.push((
                        A::ToggleDraft,
                        if item.summary.is_draft {
                            "Mark ready"
                        } else {
                            "Convert to draft"
                        }
                        .into(),
                    ));
                    actions.push((A::Merge, "Merge options".into()));
                }
                if self.detail_tab == DetailTab::Diff {
                    actions.extend([
                        (A::ToggleSplit, "Toggle split/unified diff".into()),
                        (A::ToggleWrap, "Toggle diff wrapping".into()),
                        (A::ToggleWhitespace, "Toggle ignore whitespace".into()),
                        (A::ToggleFiles, "Toggle files panel".into()),
                        (A::FindFile, "Find file".into()),
                        (A::NextThread, "Next inline thread".into()),
                        (A::PreviousThread, "Previous inline thread".into()),
                    ]);
                    if self
                        .diff
                        .as_ref()
                        .and_then(DiffViewState::selection)
                        .is_some()
                    {
                        actions.push((A::InlineComment, "Comment on selected lines".into()));
                    }
                    if let Some(diff) = &self.diff {
                        actions.extend(diff.matching_files(&self.file_filter).into_iter().map(
                            |index| {
                                (
                                    A::SelectFile(index),
                                    format!("File {}", diff.files()[index].file.path),
                                )
                            },
                        ));
                    }
                }
            }
        }
        if matches!(self.detail, Some(Detail::Run(_))) || self.detail_tab == DetailTab::Checks {
            actions.extend([
                (A::NextFailure, "Next failure".into()),
                (A::PreviousFailure, "Previous failure".into()),
            ]);
        }
        actions.push((A::CloseScreen, "Close GitHub".into()));
        actions
    }
    pub fn activate(&mut self, action: GithubAction) -> Vec<GithubEffect> {
        use GithubAction as A;
        if self.submitting {
            return Vec::new();
        }
        if self.dialog.is_some()
            && !matches!(
                action,
                A::Submit
                    | A::Cancel
                    | A::ToggleLabel(_)
                    | A::MergeNow(_)
                    | A::MergeAuto(_)
                    | A::DisableAuto
                    | A::Palette
            )
        {
            return Vec::new();
        }
        if action != A::ChooseQueue {
            self.queue_menu = None;
        }
        let previous_dialog = self.dialog.as_ref().map(std::mem::discriminant);
        match action {
            A::Palette => return vec![GithubEffect::OpenPalette],
            A::CloseScreen => {
                self.invalidate();
                return vec![GithubEffect::Close];
            }
            A::Browser => {
                return self
                    .current_url()
                    .map(|url| vec![GithubEffect::OpenUrl(url)])
                    .unwrap_or_default()
            }
            A::CopyUrl => {
                return self
                    .current_url()
                    .map(|url| vec![GithubEffect::Copy(url)])
                    .unwrap_or_default()
            }
            A::Editor => return vec![GithubEffect::OpenEditor],
            A::Tab(tab) => {
                self.tab = tab;
                if tab == GithubTab::Issues && self.queue == Queue::ReviewRequested {
                    self.queue = Queue::Authored;
                }
                self.run_sha = None;
                self.focus = Focus::List;
                self.refresh();
            }
            A::ChooseQueue => {
                if self.queue_menu.is_some() {
                    self.queue_menu = None;
                } else if let Some(index) = self
                    .available_queues()
                    .iter()
                    .position(|&queue| queue == self.queue)
                {
                    let mut menu = ModalListState::hidden(index);
                    menu.show();
                    self.queue_menu = Some(menu);
                }
            }
            A::Queue(queue) => {
                self.queue = queue;
                self.refresh();
            }
            A::Runs(filter) => {
                self.run_filter = filter;
                self.entries_dirty = true;
                self.selected = 0;
                self.list_scroll = 0;
            }
            A::Refresh => {
                if let Some(item) = self.selected_key.clone() {
                    self.invalidate();
                    self.diff = None;
                    self.enqueue(GithubRequest::Details(item));
                } else if let Some((repository, run_id)) = self.selected_run.clone() {
                    self.invalidate();
                    self.enqueue(GithubRequest::Run { repository, run_id });
                } else {
                    self.refresh();
                }
                self.force_refresh = true;
                self.refresh_generation = true;
            }
            A::More => self.more(),
            A::Filter => self.dialog = Some(Dialog::Filter(TextBuffer::new(self.filter.clone()))),
            A::ResetRepository => {
                self.repository = None;
                self.run_sha = None;
                self.refresh();
            }
            A::Back => {
                self.invalidate();
                self.clear_detail();
                self.focus = Focus::List;
                self.fill_runs_requests();
            }
            A::Open => self.open_selected(),
            A::Detail(tab) => {
                self.detail_tab = tab;
                self.detail_scroll = 0;
                self.selected_row = None;
                self.rows_dirty = true;
                if tab == DetailTab::Diff && self.diff.is_none() {
                    self.request_diff();
                }
            }
            A::Comment => self.compose(ComposerKind::Comment, "Write comment", String::new()),
            A::Reply => {
                if let Some((comment, kind)) = self.selected_comment() {
                    let reply = match kind {
                        CommentKind::Review => {
                            ComposerKind::Reply(comment.in_reply_to_id.unwrap_or(comment.id))
                        }
                        CommentKind::General => ComposerKind::Comment,
                    };
                    let body = if matches!(kind, CommentKind::General) {
                        format!(
                            "{}\n\n",
                            comment
                                .body
                                .lines()
                                .map(|line| format!("> {line}"))
                                .collect::<Vec<_>>()
                                .join("\n")
                        )
                    } else {
                        String::new()
                    };
                    self.compose(reply, "Reply to comment", body);
                }
            }
            A::EditComment => {
                if self.owns_selected_comment() {
                    if let Some((comment, kind)) = self.selected_comment() {
                        self.compose(
                            ComposerKind::Edit(kind, comment.id),
                            "Edit your comment",
                            comment.body.clone(),
                        );
                    }
                }
            }
            A::DeleteComment => {
                if self.owns_selected_comment() {
                    if let (Some(item), Some((comment, kind))) =
                        (self.selected_key.clone(), self.selected_comment())
                    {
                        self.dialog = Some(Dialog::Confirm {
                            title: "Delete your comment?".into(),
                            description:
                                "This permanently deletes the selected comment from GitHub.".into(),
                            mutation: GithubMutation::DeleteComment {
                                item,
                                kind,
                                comment_id: comment.id,
                            },
                        });
                    }
                }
            }
            A::Labels => {
                if let Some(item) = self.selected_key.clone() {
                    self.labels.clear();
                    self.enqueue(GithubRequest::Labels(item.repository));
                    self.dialog = Some(Dialog::Labels { selected: 0 });
                }
            }
            A::ToggleLabel(index) => {
                if let (Some(item), Some(label)) = (self.item(), self.labels.get(index)) {
                    let applied = item.labels.iter().any(|current| current.name == label.name);
                    self.mutate(GithubMutation::Labels {
                        item: item.summary.key.clone(),
                        add: if applied {
                            Vec::new()
                        } else {
                            vec![label.name.clone()]
                        },
                        remove: if applied {
                            vec![label.name.clone()]
                        } else {
                            Vec::new()
                        },
                    });
                }
            }
            A::ToggleDraft => {
                if let Some(item) = self.item() {
                    if let Some(merge) = &item.merge {
                        self.mutate(GithubMutation::Draft {
                            item: item.summary.key.clone(),
                            head_sha: merge.head_sha.clone(),
                            draft: !item.summary.is_draft,
                        });
                    }
                }
            }
            A::CloseItem => {
                if let Some(item) = self.selected_key.clone() {
                    self.dialog = Some(Dialog::Confirm {
                        title: format!("Close #{}?", item.number),
                        description:
                            "The item will be closed on GitHub. No branch will be deleted.".into(),
                        mutation: GithubMutation::Close { item },
                    });
                }
            }
            A::ReviewComment => self.compose(
                ComposerKind::Review(ReviewEvent::Comment),
                "Review comment (body optional)",
                String::new(),
            ),
            A::Approve => self.compose(
                ComposerKind::Review(ReviewEvent::Approve),
                "Approve pull request (body optional)",
                String::new(),
            ),
            A::RequestChanges => self.compose(
                ComposerKind::Review(ReviewEvent::RequestChanges),
                "Request changes (body optional)",
                String::new(),
            ),
            A::Merge => {
                if let Some(item) = self.selected_key.clone() {
                    self.merge_repository = None;
                    self.dialog = Some(Dialog::Merge);
                    self.enqueue(GithubRequest::Repository(item.repository));
                }
            }
            A::MergeNow(index) | A::MergeAuto(index) => {
                if !self
                    .merge_actions()
                    .iter()
                    .any(|(candidate, _)| *candidate == action)
                {
                    return Vec::new();
                }
                if let (Some(item), Some((method, label))) =
                    (self.item(), self.merge_methods().get(index).copied())
                {
                    if let Some(merge) = &item.merge {
                        self.dialog = Some(Dialog::Confirm {
                            title: if merge.queue_enabled {
                                "Add to merge queue?".into()
                            } else {
                                format!("{label} pull request?")
                            },
                            description: format!(
                                "{} #{}\nHead {}\nNo branch will be deleted.",
                                item.summary.key.repository,
                                item.summary.key.number,
                                merge.head_sha
                            ),
                            mutation: GithubMutation::Merge {
                                item: item.summary.key.clone(),
                                head_sha: merge.head_sha.clone(),
                                action: if matches!(action, A::MergeAuto(_)) {
                                    MergeAction::Auto(method)
                                } else {
                                    MergeAction::Now(method)
                                },
                            },
                        });
                    }
                }
            }
            A::DisableAuto => {
                if let Some(item) = self.item() {
                    if let Some(merge) = &item.merge {
                        self.mutate(GithubMutation::Merge {
                            item: item.summary.key.clone(),
                            head_sha: merge.head_sha.clone(),
                            action: MergeAction::DisableAuto,
                        });
                    }
                }
            }
            A::Submit => self.submit_dialog(),
            A::Cancel => {
                self.dialog = None;
                self.error = None;
            }
            A::ToggleSplit | A::ToggleWrap | A::ToggleWhitespace => {
                if let Some(diff) = &mut self.diff {
                    let mut options = diff.options();
                    match action {
                        A::ToggleSplit => {
                            options.mode = if options.mode == DiffMode::Split {
                                DiffMode::Unified
                            } else {
                                DiffMode::Split
                            }
                        }
                        A::ToggleWrap => options.wrap = !options.wrap,
                        _ => options.ignore_whitespace = !options.ignore_whitespace,
                    }
                    diff.set_options(options);
                }
            }
            A::ToggleFiles => self.show_files = !self.show_files,
            A::FindFile => {
                self.dialog = Some(Dialog::FileSearch(TextBuffer::new(
                    self.file_filter.clone(),
                )))
            }
            A::SelectFile(index) => {
                if let Some(diff) = &mut self.diff {
                    if let Err(error) = diff.select_file(index) {
                        self.error = Some(format!("Cannot select file: {error:?}"));
                    } else {
                        self.detail_scroll = 0;
                        self.focus = Focus::Detail;
                    }
                }
            }
            A::NextThread | A::PreviousThread => self.navigate_thread(action == A::NextThread),
            A::InlineComment => {
                if let (Some(selection), Some(item)) = (
                    self.diff.as_ref().and_then(DiffViewState::selection),
                    self.item(),
                ) {
                    if let Some(merge) = &item.merge {
                        let side = match selection.side {
                            DiffSide::Left => Side::Left,
                            DiffSide::Right => Side::Right,
                        };
                        self.compose(
                            ComposerKind::Inline(InlineComment {
                                body: String::new(),
                                commit_id: merge.head_sha.clone(),
                                path: selection.path.clone(),
                                line: u64::from(selection.end_line),
                                side,
                                start: (selection.start_line != selection.end_line)
                                    .then_some((u64::from(selection.start_line), side)),
                            }),
                            &format!(
                                "Comment on {} {}:{}–{}",
                                selection.path,
                                selection.side.as_str(),
                                selection.start_line,
                                selection.end_line
                            ),
                            String::new(),
                        );
                    }
                } else {
                    self.error = Some("Select a diff line or drag a same-side range first.".into());
                }
            }
            A::NextFailure | A::PreviousFailure => self.navigate_failure(action == A::NextFailure),
            A::PullRequestRuns => {
                let target = self.item().and_then(|item| {
                    item.merge
                        .as_ref()
                        .map(|merge| (item.summary.key.repository.clone(), merge.head_sha.clone()))
                });
                if let Some((repository, sha)) = target {
                    self.repository = Some(repository);
                    self.run_sha = Some(sha);
                    self.tab = GithubTab::Actions;
                    self.refresh();
                }
            }
        }
        if self.dialog.as_ref().map(std::mem::discriminant) != previous_dialog {
            self.control_focus = 0;
            self.focus = if self.dialog.is_some() || self.detail.is_some() {
                Focus::Detail
            } else {
                Focus::List
            };
        }
        self.compute(self.geometry.area);
        Vec::new()
    }
}
