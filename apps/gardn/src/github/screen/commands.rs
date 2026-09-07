use super::*;

impl GithubScreen {
    fn default_scope_label(&self) -> String {
        match self.scope_state.default.repositories.as_slice() {
            [] => "Space default".into(),
            [repository] => format!("Selected repositories ({repository})"),
            repositories => format!("{} selected repositories", repositories.len()),
        }
    }
    pub(super) fn scope_account_label(&self) -> String {
        if self.scope_state.account == AccountChoice::Default {
            if !self.scope_state.default.repositories.is_empty() {
                return "Selected repositories".into();
            }
            if let Some(organization) = &self.scope_state.default.organization {
                return organization.as_str().to_owned();
            }
        }
        match &self.scope_state.account {
            AccountChoice::Default | AccountChoice::Personal => self
                .viewer
                .as_ref()
                .map_or_else(|| "Personal".into(), |viewer| viewer.login.clone()),
            AccountChoice::Organization(organization) => organization.as_str().to_owned(),
        }
    }

    pub(super) fn scope_repository_label(&self) -> String {
        let Some(repository) = &self.repository else {
            if self.scope_state.account == AccountChoice::Default {
                return match self.scope_state.default.repositories.as_slice() {
                    [] => "All repositories".into(),
                    [repository] => repository.to_string(),
                    repositories => format!("{} selected repositories", repositories.len()),
                };
            }
            return "All repositories".into();
        };
        let label = repository.to_string();
        let account = match &self.scope_state.account {
            AccountChoice::Default if self.scope_state.default.repositories.is_empty() => self
                .scope_state
                .default
                .organization
                .as_ref()
                .map(|organization| organization.as_str()),
            AccountChoice::Personal => self.viewer.as_ref().map(|viewer| viewer.login.as_str()),
            AccountChoice::Organization(organization) => Some(organization.as_str()),
            AccountChoice::Default => None,
        };
        let Some((owner, name)) = label.split_once('/') else {
            return label;
        };
        if account.is_some_and(|account| owner.eq_ignore_ascii_case(account)) {
            name.to_owned()
        } else {
            label
        }
    }

    pub(super) fn open_scope_repositories(&mut self) {
        let mut items = vec![(GithubAction::ScopeRepository(0), "All repositories".into())];
        items.extend(self.scope_state.repositories.items.iter().enumerate().map(
            |(index, repository)| {
                (
                    GithubAction::ScopeRepository(index + 1),
                    repository.to_string(),
                )
            },
        ));
        if self.scope_state.repositories.loading {
            items.push((GithubAction::ScopeRetry, "Loading…".into()));
        } else if let Some(error) = &self.scope_state.repositories.error {
            items.push((GithubAction::ScopeRetry, format!("{error} · Retry")));
        } else if self.scope_state.repositories.cursor.is_some() {
            items.push((GithubAction::ScopeMore, "Load more".into()));
        }
        let selected = self
            .repository
            .as_ref()
            .and_then(|repository| {
                self.scope_state
                    .repositories
                    .items
                    .iter()
                    .position(|item| item == repository)
                    .map(|index| index + 1)
            })
            .unwrap_or(0);
        let mut list = ModalListState::hidden(selected);
        list.show();
        let trigger = self
            .menu
            .as_ref()
            .and_then(|menu| match (menu.kind, menu.trigger) {
                (LocalMenuKind::Repositories, trigger) => Some(trigger),
                (LocalMenuKind::Accounts, GithubAction::ChooseScope) => {
                    Some(GithubAction::ChooseScope)
                }
                _ => None,
            })
            .unwrap_or(GithubAction::ChooseRepository);
        self.menu = Some(LocalMenu {
            kind: LocalMenuKind::Repositories,
            trigger,
            items,
            list,
            scroll: 0,
        });
        if !self.scope_state.repositories.loaded
            && !self.scope_state.repositories.loading
            && self.scope_state.repositories.error.is_none()
        {
            self.scope_state.repositories.loading = true;
            self.enqueue(GithubRequest::ScopeRepositories {
                cursor: None,
                page_size: 50,
            });
            let items = self
                .menu
                .as_ref()
                .map(|menu| menu.items.clone())
                .unwrap_or_default();
            if let Some(menu) = &mut self.menu {
                menu.items = items
                    .into_iter()
                    .chain(std::iter::once((
                        GithubAction::ScopeRetry,
                        "Loading…".into(),
                    )))
                    .collect();
            }
        }
    }

    fn select_scope_account(&mut self, index: usize) {
        let account = match index {
            0 => AccountChoice::Default,
            1 => AccountChoice::Personal,
            index => {
                let Some(account) = self
                    .scope_state
                    .organizations
                    .items
                    .get(index - 2)
                    .and_then(|organization| {
                        crate::app::state::GithubOrganization::parse(&organization.login)
                            .ok()
                            .flatten()
                            .map(AccountChoice::Organization)
                    })
                else {
                    return;
                };
                account
            }
        };
        self.scope_state.account = account.clone();
        self.scope = match account {
            AccountChoice::Default => self.scope_state.default.clone(),
            AccountChoice::Personal => ResolvedGithubScope {
                repositories: Vec::new(),
                organization: None,
            },
            AccountChoice::Organization(organization) => ResolvedGithubScope {
                repositories: Vec::new(),
                organization: Some(organization),
            },
        };
        self.repository = None;
        self.scope_state.repositories = ScopeCatalog::default();
        self.refresh();
        self.open_scope_repositories();
    }

    pub(super) fn scope_account_items(&self) -> Vec<(GithubAction, String)> {
        let mut items = vec![
            (GithubAction::ScopeAccount(0), self.default_scope_label()),
            (
                GithubAction::ScopeAccount(1),
                self.viewer.as_ref().map_or_else(
                    || "Personal".into(),
                    |viewer| format!("Personal ({})", viewer.login),
                ),
            ),
        ];
        items.extend(self.scope_state.organizations.items.iter().enumerate().map(
            |(index, organization)| {
                (
                    GithubAction::ScopeAccount(index + 2),
                    format!("Organization {}", organization.login),
                )
            },
        ));
        if self.scope_state.organizations.loading {
            items.push((GithubAction::ScopeRetry, "Loading organizations…".into()));
        } else if let Some(error) = &self.scope_state.organizations.error {
            items.push((
                GithubAction::ScopeRetry,
                format!("{error} · Retry organizations"),
            ));
        } else if self.scope_state.organizations.cursor.is_some() {
            items.push((GithubAction::ScopeMore, "Load more organizations".into()));
        }
        items
    }

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
    pub(super) fn local_overflow_actions(&self) -> Vec<(GithubAction, String)> {
        use GithubAction as A;
        let queue_visible = self
            .geometry
            .controls
            .iter()
            .any(|control| control.action == A::ChooseQueue);
        let visible = |action: &A| {
            self.geometry
                .controls
                .iter()
                .any(|control| control.action == *action)
        };
        let mut actions = self
            .contextual_actions()
            .into_iter()
            .filter(|(action, _)| {
                !matches!(action, A::ChooseAction | A::CloseScreen | A::Queue(_))
                    && !visible(action)
            })
            .collect::<Vec<_>>();
        if !self.available_queues().is_empty() && !queue_visible {
            actions.push((A::ChooseQueue, "Choose queue".into()));
        }
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
                    | A::ChooseAction
            )
        {
            return Vec::new();
        }
        if !matches!(
            action,
            A::ChooseQueue
                | A::ChooseAction
                | A::ChooseAccount
                | A::ChooseRepository
                | A::ChooseScope
                | A::ScopeAccount(_)
                | A::ScopeRepository(_)
                | A::ScopeMore
                | A::ScopeRetry
        ) {
            self.menu = None;
        }
        let previous_dialog = self.dialog.as_ref().map(std::mem::discriminant);
        if matches!(action, A::ChooseAccount | A::ChooseScope) {
            let items = self.scope_account_items();
            let selected = match &self.scope_state.account {
                AccountChoice::Default => 0,
                AccountChoice::Personal => 1,
                AccountChoice::Organization(selected) => self
                    .scope_state
                    .organizations
                    .items
                    .iter()
                    .position(|organization| organization.login == selected.as_str())
                    .map_or(0, |index| index + 2),
            };
            let mut list = ModalListState::hidden(selected);
            list.show();
            self.menu = Some(LocalMenu {
                kind: LocalMenuKind::Accounts,
                trigger: action,
                items,
                list,
                scroll: 0,
            });
            if !self.scope_state.organizations.loaded
                && !self.scope_state.organizations.loading
                && self.scope_state.organizations.error.is_none()
            {
                self.scope_state.organizations.loading = true;
                self.enqueue(GithubRequest::Organizations {
                    cursor: None,
                    page_size: 50,
                });
                let items = self.scope_account_items();
                if let Some(menu) = &mut self.menu {
                    menu.items = items;
                }
            }
            return Vec::new();
        }
        if action == A::ChooseRepository {
            self.open_scope_repositories();
            return Vec::new();
        }
        if let A::ScopeAccount(index) = action {
            self.select_scope_account(index);
            return Vec::new();
        }
        if let A::ScopeRepository(index) = action {
            self.repository = if index == 0 {
                None
            } else {
                let Some(repository) = self.scope_state.repositories.items.get(index - 1).cloned()
                else {
                    return Vec::new();
                };
                Some(repository)
            };
            self.run_sha = None;
            self.menu = None;
            self.refresh();
            return Vec::new();
        }
        if action == A::ScopeMore {
            let accounts = self
                .menu
                .as_ref()
                .is_some_and(|menu| menu.kind == LocalMenuKind::Accounts);
            if accounts {
                if self.scope_state.organizations.loading {
                    return Vec::new();
                }
                if let Some(cursor) = self.scope_state.organizations.cursor.clone() {
                    self.scope_state.organizations.loading = true;
                    self.enqueue(GithubRequest::Organizations {
                        cursor: Some(cursor),
                        page_size: 50,
                    });
                    let items = self.scope_account_items();
                    if let Some(menu) = &mut self.menu {
                        menu.items = items;
                    }
                }
            } else {
                if self.scope_state.repositories.loading {
                    return Vec::new();
                }
                if let Some(cursor) = self.scope_state.repositories.cursor.clone() {
                    self.scope_state.repositories.loading = true;
                    self.enqueue(GithubRequest::ScopeRepositories {
                        cursor: Some(cursor),
                        page_size: 50,
                    });
                    self.open_scope_repositories();
                }
            }
            return Vec::new();
        }
        if action == A::ScopeRetry {
            let accounts = self
                .menu
                .as_ref()
                .is_some_and(|menu| menu.kind == LocalMenuKind::Accounts);
            if accounts {
                if self.scope_state.organizations.loading {
                    return Vec::new();
                }
                let cursor = self.scope_state.organizations.cursor.clone();
                self.scope_state.organizations.error = None;
                self.scope_state.organizations.loading = true;
                self.enqueue(GithubRequest::Organizations {
                    cursor,
                    page_size: 50,
                });
                let items = self.scope_account_items();
                if let Some(menu) = &mut self.menu {
                    menu.items = items;
                }
            } else {
                if self.scope_state.repositories.loading {
                    return Vec::new();
                }
                let cursor = self.scope_state.repositories.cursor.clone();
                self.scope_state.repositories.error = None;
                self.scope_state.repositories.loading = true;
                self.enqueue(GithubRequest::ScopeRepositories {
                    cursor,
                    page_size: 50,
                });
                self.open_scope_repositories();
            }
            return Vec::new();
        }
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
            A::ChooseQueue | A::ChooseAction => {
                if self
                    .menu
                    .as_ref()
                    .is_some_and(|menu| menu.trigger == action)
                {
                    self.menu = None;
                } else {
                    let items: Vec<_> = if action == A::ChooseQueue {
                        self.available_queues()
                            .iter()
                            .map(|&queue| (A::Queue(queue), queue_label(queue).into()))
                            .collect()
                    } else {
                        self.local_overflow_actions()
                    };
                    if !items.is_empty() {
                        let selected = items
                            .iter()
                            .position(|(action, _)| *action == A::Queue(self.queue))
                            .unwrap_or(0);
                        let mut list = ModalListState::hidden(selected);
                        list.show();
                        let trigger = if action == A::ChooseQueue
                            && self
                                .menu
                                .as_ref()
                                .is_some_and(|menu| menu.trigger == A::ChooseAction)
                        {
                            A::ChooseAction
                        } else {
                            action
                        };
                        self.menu = Some(LocalMenu {
                            kind: LocalMenuKind::Actions,
                            trigger,
                            items,
                            list,
                            scroll: 0,
                        });
                    }
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
            A::ChooseAccount
            | A::ChooseRepository
            | A::ChooseScope
            | A::ScopeAccount(_)
            | A::ScopeRepository(_)
            | A::ScopeMore
            | A::ScopeRetry => {}
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
