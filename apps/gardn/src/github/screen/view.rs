use super::*;

impl GithubScreen {
    pub fn compute(&mut self, area: Rect) {
        use GithubAction as A;
        self.update_visible_entries();
        let mut geometry = Geometry {
            area,
            ..Geometry::default()
        };
        if area.is_empty() {
            self.geometry = geometry;
            return;
        }
        geometry.header = Rect::new(area.x, area.y, area.width.saturating_sub(7), 1);
        geometry.controls.push(Control {
            area: Rect::new(
                area.right().saturating_sub(7).max(area.x),
                area.y,
                7.min(area.width),
                1,
            ),
            action: A::CloseScreen,
            label: "Close".into(),
        });
        let mut y = area.y.saturating_add(1);
        let bottom = area.bottom().saturating_sub(2).max(y);
        let chrome_bottom = bottom
            .saturating_sub(3)
            .max(y.saturating_add(2).min(bottom));
        let tabs: Vec<_> = GithubTab::ALL
            .into_iter()
            .map(|tab| (A::Tab(tab), tab.label().to_owned()))
            .collect();
        if area.height >= 12 {
            y = place_controls(
                &mut geometry.controls,
                Rect::new(
                    area.x,
                    y,
                    area.width,
                    chrome_bottom.saturating_sub(y).saturating_sub(1).min(3),
                ),
                &tabs,
            );
        } else {
            let short_tabs = [
                (A::Tab(GithubTab::Overview), "1".into()),
                (A::Tab(GithubTab::Repositories), "2".into()),
                (A::Tab(GithubTab::PullRequests), "3".into()),
                (A::Tab(GithubTab::Issues), "4".into()),
                (A::Tab(GithubTab::Actions), "5".into()),
            ];
            y = place_controls(
                &mut geometry.controls,
                Rect::new(area.x, y, area.width, u16::from(y < chrome_bottom)),
                &short_tabs,
            );
        }
        let mut toolbar = vec![
            (A::Palette, "Actions…".into()),
            (A::Refresh, "Refresh".into()),
            (A::Filter, "Filter".into()),
        ];
        if self.has_more() {
            toolbar.push((A::More, "More".into()));
        }
        if self.repository.is_some() {
            toolbar.push((A::ResetRepository, "Reset repo".into()));
        }
        y = place_controls(
            &mut geometry.controls,
            Rect::new(area.x, y, area.width, u16::from(y < chrome_bottom)),
            &toolbar,
        );
        if matches!(self.tab, GithubTab::PullRequests | GithubTab::Issues) {
            let queues: Vec<_> = [
                Queue::Authored,
                Queue::ReviewRequested,
                Queue::Assigned,
                Queue::Mentioned,
                Queue::All,
            ]
            .into_iter()
            .filter(|queue| self.tab != GithubTab::Issues || *queue != Queue::ReviewRequested)
            .map(|queue| (A::Queue(queue), queue_label(queue).to_owned()))
            .collect();
            y = place_controls(
                &mut geometry.controls,
                Rect::new(area.x, y, area.width, u16::from(y < chrome_bottom)),
                &queues,
            );
        }
        if self.tab == GithubTab::Actions {
            y = place_controls(
                &mut geometry.controls,
                Rect::new(area.x, y, area.width, u16::from(y < chrome_bottom)),
                &[
                    (A::Runs(RunFilter::All), "All".into()),
                    (A::Runs(RunFilter::Failed), "Failed".into()),
                    (A::Runs(RunFilter::Running), "Running".into()),
                ],
            );
        }
        let has_detail =
            self.detail.is_some() || self.selected_key.is_some() || self.selected_run.is_some();
        if has_detail {
            let mut actions = vec![
                (A::Back, "Back".into()),
                (A::Browser, "Browser".into()),
                (A::CopyUrl, "Copy".into()),
            ];
            if self.item().is_some() {
                actions.extend([
                    (A::Detail(DetailTab::Description), "Description".into()),
                    (A::Detail(DetailTab::Conversation), "Conversation".into()),
                    (A::Comment, "Comment".into()),
                ]);
                if self
                    .item()
                    .is_some_and(|item| item.summary.key.kind == ItemKind::PullRequest)
                {
                    actions.extend([
                        (A::Detail(DetailTab::Diff), "Diff".into()),
                        (A::Detail(DetailTab::Checks), "Checks".into()),
                        (A::Merge, "Merge".into()),
                    ]);
                }
                if self.selected_comment().is_some() {
                    actions.insert(1, (A::Reply, "Reply".into()));
                }
                if self.owns_selected_comment() {
                    actions.insert(2, (A::EditComment, "Edit".into()));
                    actions.insert(3, (A::DeleteComment, "Delete".into()));
                }
            }
            y = place_controls(
                &mut geometry.controls,
                Rect::new(
                    area.x,
                    y,
                    area.width,
                    chrome_bottom
                        .saturating_sub(y)
                        .min(if area.height >= 18 { 2 } else { 1 }),
                ),
                &actions,
            );
        }
        if self.detail_tab == DetailTab::Diff && self.item().is_some() {
            let options = self
                .diff
                .as_ref()
                .map(DiffViewState::options)
                .unwrap_or_default();
            let actions = [
                (
                    A::ToggleFiles,
                    if self.show_files {
                        "Hide files"
                    } else {
                        "Show files"
                    }
                    .into(),
                ),
                (A::FindFile, "Find file".into()),
                (
                    A::ToggleSplit,
                    if options.mode == DiffMode::Split {
                        "Split"
                    } else {
                        "Unified"
                    }
                    .into(),
                ),
                (
                    A::ToggleWrap,
                    format!("Wrap {}", if options.wrap { "on" } else { "off" }),
                ),
                (
                    A::ToggleWhitespace,
                    format!(
                        "Whitespace {}",
                        if options.ignore_whitespace {
                            "ignored"
                        } else {
                            "shown"
                        }
                    ),
                ),
                (A::InlineComment, "Comment lines".into()),
                (A::NextThread, "Next thread".into()),
            ];
            y = place_controls(
                &mut geometry.controls,
                Rect::new(
                    area.x,
                    y,
                    area.width,
                    chrome_bottom.saturating_sub(y).min(2),
                ),
                &actions,
            );
        }
        y = y.min(bottom);
        let content = Rect::new(area.x, y, area.width, bottom.saturating_sub(y));
        if has_detail && area.width >= 110 {
            let list_width = (area.width / 3).min(45);
            geometry.list = Rect::new(content.x, content.y, list_width, content.height);
            geometry.detail = Rect::new(
                content.x + list_width + 1,
                content.y,
                content.width.saturating_sub(list_width + 1),
                content.height,
            );
        } else if has_detail && self.focus != Focus::List {
            geometry.detail = content;
        } else {
            geometry.list = content;
        }
        if self.detail_tab == DetailTab::Diff
            && self.item().is_some()
            && self.show_files
            && geometry.detail.width > 0
        {
            if geometry.detail.width >= 85 {
                let width = (geometry.detail.width / 4).min(30);
                geometry.files = Rect::new(
                    geometry.detail.x,
                    geometry.detail.y,
                    width,
                    geometry.detail.height,
                );
                geometry.detail.x += width + 1;
                geometry.detail.width = geometry.detail.width.saturating_sub(width + 1);
            } else {
                let height = (geometry.detail.height / 3).min(5);
                geometry.files = Rect::new(
                    geometry.detail.x,
                    geometry.detail.y,
                    geometry.detail.width,
                    height,
                );
                geometry.detail.y += height;
                geometry.detail.height = geometry.detail.height.saturating_sub(height);
            }
        }
        if let Some(diff) = &mut self.diff {
            let mut options = diff.options();
            let columns = usize::from(geometry.detail.width.saturating_sub(1));
            options.width = if options.mode == DiffMode::Split {
                columns / 2
            } else {
                columns
            }
            .saturating_sub(8)
            .max(1);
            diff.set_options(options);
            self.file_scroll = ModalListViewport::new(
                diff.matching_files(&self.file_filter).len(),
                geometry.files.height as usize,
                self.file_scroll,
            )
            .scroll();
        }
        if self.rows_dirty || self.rows_width != geometry.detail.width {
            self.rows_width = geometry.detail.width;
            self.rebuild_detail_rows(geometry.detail.width.saturating_sub(1).max(1) as usize);
            self.rows_dirty = false;
        }
        let total = self.visible_entries().len();
        self.selected = self.selected.min(total.saturating_sub(1));
        self.list_scroll =
            ModalListViewport::new(total, geometry.list.height as usize, self.list_scroll).scroll();
        self.detail_scroll = ModalListViewport::new(
            self.detail_len(),
            geometry.detail.height as usize,
            self.detail_scroll,
        )
        .scroll();
        geometry.status = Rect::new(
            area.x,
            bottom.min(area.bottom()),
            area.width,
            area.bottom().saturating_sub(bottom),
        );
        if self.dialog.is_some() {
            geometry.controls.clear();
            let width = area.width.min(86);
            let height = area.height.min(
                if matches!(
                    self.dialog,
                    Some(Dialog::Composer { .. } | Dialog::Labels { .. })
                ) {
                    22
                } else {
                    16
                },
            );
            geometry.modal = Rect::new(
                area.x + area.width.saturating_sub(width) / 2,
                area.y + area.height.saturating_sub(height) / 2,
                width,
                height,
            );
            let inner = inset(geometry.modal);
            let input_y = inner.y.saturating_add(2).min(inner.bottom());
            let footer_height = if matches!(self.dialog, Some(Dialog::Merge)) {
                5
            } else {
                2
            };
            let input_bottom = inner
                .bottom()
                .saturating_sub(footer_height)
                .max(input_y)
                .min(inner.bottom());
            geometry.input = Rect::new(
                inner.x,
                input_y,
                inner.width,
                input_bottom.saturating_sub(input_y),
            );
            let mut actions = vec![(A::Cancel, "Cancel".into())];
            match &self.dialog {
                Some(Dialog::Merge) => {
                    actions.push((A::Palette, "Actions…".into()));
                    actions.extend(self.merge_actions());
                }
                Some(Dialog::Labels { .. }) => {
                    actions.push((A::Submit, "Toggle selected".into()));
                }
                Some(Dialog::Confirm { .. }) => actions.push((A::Submit, "Confirm".into())),
                _ => actions.push((
                    A::Submit,
                    if self.submitting {
                        "Submitting…"
                    } else {
                        "Submit Ctrl+Enter"
                    }
                    .into(),
                )),
            }
            place_controls(
                &mut geometry.controls,
                Rect::new(
                    inner.x,
                    input_bottom,
                    inner.width,
                    inner.bottom().saturating_sub(input_bottom),
                ),
                &actions,
            );
        }
        self.control_focus = self
            .control_focus
            .min(geometry.controls.len().saturating_sub(1));
        self.geometry = geometry;
    }
    pub(super) fn rebuild_detail_rows(&mut self, width: usize) {
        let mut rows = Vec::new();
        match &self.detail {
            Some(Detail::Repository(repo)) => {
                append_rows(&mut rows, &repo.repository.to_string(), None, false, width);
                append_rows(
                    &mut rows,
                    repo.description
                        .as_deref()
                        .unwrap_or("No repository description."),
                    None,
                    false,
                    width,
                );
                append_rows(&mut rows, &format!("{}{} · {} stars · {} forks · {} open issues\nDefault branch {}\nRepository narrowing is active. Choose Pull requests, Issues or Actions above.", if repo.private { "Private" } else { "Public" }, if repo.archived { " · Archived" } else { "" }, repo.stargazers_count, repo.forks_count, repo.open_issues_count, repo.default_branch.as_deref().unwrap_or("none")), None, false, width);
            }
            Some(Detail::Item(item)) => match self.detail_tab {
                DetailTab::Description => {
                    append_rows(
                        &mut rows,
                        &format!(
                            "{} #{}\n{}\n{}{} · @{}",
                            item.summary.key.repository,
                            item.summary.key.number,
                            item.summary.title,
                            item.summary.state,
                            if item.summary.is_draft {
                                " · Draft"
                            } else {
                                ""
                            },
                            item.summary.author.as_deref().unwrap_or("unknown")
                        ),
                        None,
                        false,
                        width,
                    );
                    append_rows(
                        &mut rows,
                        &format!(
                            "Labels: {}\nAssignees: {}",
                            item.labels
                                .iter()
                                .map(|label| label.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", "),
                            item.assignees
                                .iter()
                                .map(|user| user.login.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        None,
                        false,
                        width,
                    );
                    if let Some(merge) = &item.merge {
                        append_rows(&mut rows, &format!("{} → {}  +{} −{}  {} files\nReview {} · Merge {} · Queue {} · Auto-merge {}\nHead {}", merge.head_branch, merge.base_branch, item.additions.unwrap_or(0), item.deletions.unwrap_or(0), item.changed_files.unwrap_or(0), merge.review_decision.as_deref().unwrap_or("none"), merge.merge_state_status, if merge.queue_enabled { "enabled" } else { "off" }, if merge.auto_merge_enabled { "enabled" } else { "off" }, merge.head_sha), None, false, width);
                    }
                    append_rows(&mut rows, "", None, false, width);
                    append_rows(
                        &mut rows,
                        item.body
                            .as_deref()
                            .filter(|body| !body.is_empty())
                            .unwrap_or("No description."),
                        None,
                        false,
                        width,
                    );
                }
                DetailTab::Conversation => {
                    let mut comments: Vec<_> =
                        item.comments
                            .iter()
                            .enumerate()
                            .map(|(index, comment)| {
                                (
                                    comment,
                                    RowTarget::Comment {
                                        review: false,
                                        index,
                                    },
                                )
                            })
                            .chain(item.review_comments.iter().enumerate().map(
                                |(index, comment)| {
                                    (
                                        comment,
                                        RowTarget::Comment {
                                            review: true,
                                            index,
                                        },
                                    )
                                },
                            ))
                            .collect();
                    comments.sort_by(|(a, _), (b, _)| {
                        a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id))
                    });
                    for (comment, target) in comments {
                        append_rows(
                            &mut rows,
                            &format!(
                                "@{} · {}{}",
                                comment
                                    .user
                                    .as_ref()
                                    .map_or("unknown", |user| user.login.as_str()),
                                comment.created_at,
                                if comment.line.is_none() && comment.path.is_some() {
                                    " · Outdated inline comment"
                                } else {
                                    ""
                                }
                            ),
                            Some(target),
                            false,
                            width,
                        );
                        if let Some(path) = &comment.path {
                            append_rows(
                                &mut rows,
                                &format!(
                                    "{} {:?}:{}{}",
                                    path,
                                    comment.side,
                                    comment.line.or(comment.original_line).unwrap_or(0),
                                    comment
                                        .in_reply_to_id
                                        .map(|id| format!(" · reply to {id}"))
                                        .unwrap_or_default()
                                ),
                                Some(target),
                                false,
                                width,
                            );
                        }
                        append_rows(&mut rows, &comment.body, Some(target), false, width);
                        append_rows(&mut rows, "", Some(target), false, width);
                    }
                    for review in &item.reviews {
                        append_rows(
                            &mut rows,
                            &format!(
                                "Review {} · @{}\n{}",
                                review.state,
                                review
                                    .user
                                    .as_ref()
                                    .map_or("unknown", |user| user.login.as_str()),
                                review.body
                            ),
                            None,
                            review.state == "CHANGES_REQUESTED",
                            width,
                        );
                    }
                    if rows.is_empty() {
                        append_rows(
                            &mut rows,
                            "No comments yet. Use Comment to start the conversation.",
                            None,
                            false,
                            width,
                        );
                    }
                }
                DetailTab::Checks => {
                    for (index, check) in item.checks.iter().enumerate() {
                        append_rows(
                            &mut rows,
                            &format!(
                                "{} · {} · {}",
                                check.name,
                                check.status,
                                check.conclusion.as_deref().unwrap_or("pending")
                            ),
                            Some(RowTarget::Check(index)),
                            check
                                .conclusion
                                .as_deref()
                                .is_some_and(|value| is_failure(&value.to_lowercase())),
                            width,
                        );
                    }
                    if rows.is_empty() {
                        append_rows(
                            &mut rows,
                            "No checks reported for this pull request.",
                            None,
                            false,
                            width,
                        );
                    }
                }
                DetailTab::Diff => {}
            },
            Some(Detail::Run(details)) => {
                let run = &details.run;
                append_rows(
                    &mut rows,
                    &format!(
                        "{} #{} attempt {}\n{}\n{} · {} · {}\n{} · {}\nHead {}",
                        run.name.as_deref().unwrap_or("Workflow"),
                        run.run_number,
                        run.run_attempt,
                        run.display_title,
                        run.status,
                        run.conclusion.as_deref().unwrap_or("pending"),
                        duration(
                            run.run_started_at.as_deref(),
                            (run.status == "completed").then_some(run.updated_at.as_str())
                        ),
                        run.event,
                        run.head_branch.as_deref().unwrap_or("no branch"),
                        run.head_sha
                    ),
                    None,
                    false,
                    width,
                );
                for (index, job) in details.jobs.iter().enumerate() {
                    append_rows(
                        &mut rows,
                        &format!(
                            "\n{} · {} · {} · {}",
                            job.name,
                            job.status,
                            job.conclusion.as_deref().unwrap_or("pending"),
                            duration(job.started_at.as_deref(), job.completed_at.as_deref())
                        ),
                        Some(RowTarget::Job(index)),
                        job.conclusion.as_deref().is_some_and(is_failure),
                        width,
                    );
                    for step in &job.steps {
                        append_rows(
                            &mut rows,
                            &format!(
                                "  {}. {} · {} · {} · {}",
                                step.number,
                                step.name,
                                step.status,
                                step.conclusion.as_deref().unwrap_or("pending"),
                                duration(step.started_at.as_deref(), step.completed_at.as_deref())
                            ),
                            Some(RowTarget::Job(index)),
                            step.conclusion.as_deref().is_some_and(is_failure),
                            width,
                        );
                    }
                }
                if details.jobs.is_empty() {
                    append_rows(
                        &mut rows,
                        "No jobs available yet. Refresh to check again.",
                        None,
                        false,
                        width,
                    );
                }
            }
            None => {}
        }
        self.detail_rows = rows;
    }
}
