use super::*;
use crate::github::rich_text::{self, TextRole};

impl GithubScreen {
    pub fn compute(&mut self, area: Rect) {
        use GithubAction as A;
        self.update_visible_entries();
        let mut geometry = Geometry {
            area,
            list_rows: std::mem::take(&mut self.geometry.list_rows),
            ..Geometry::default()
        };
        geometry.list_rows.clear();
        if area.is_empty() {
            self.geometry = geometry;
            return;
        }
        let roomy = area.height >= 18;
        let horizontal = if area.width >= 50 { 2 } else { 1 };
        let vertical = u16::from(roomy);
        let inner = Rect::new(
            area.x + horizontal.min(area.width),
            area.y + vertical.min(area.height),
            area.width.saturating_sub(horizontal * 2),
            area.height.saturating_sub(vertical * 2),
        );
        let stack =
            crate::ui::modal_stack_areas(inner, if roomy { 6 } else { 3 }, 1, 0, u16::from(roomy));
        let header = stack.header;
        let close_width = 11.min(header.width);
        geometry.header = Rect::new(
            header.x,
            header.y,
            header.width.saturating_sub(close_width + 1),
            u16::from(!header.is_empty()),
        );
        if !header.is_empty() {
            geometry.controls.push(Control {
                area: Rect::new(header.right() - close_width, header.y, close_width, 1),
                action: A::CloseScreen,
                label: "Close".into(),
            });
        }
        let row = |offset: u16| {
            Rect::new(
                header.x,
                (header.y + offset).min(header.bottom()),
                header.width,
                u16::from(offset < header.height),
            )
        };
        if roomy {
            geometry.scope = row(1);
        }
        let tabs: Vec<_> = GithubTab::ALL
            .into_iter()
            .enumerate()
            .map(|(index, tab)| {
                let label = if inner.width >= 66 {
                    tab.label().to_owned()
                } else if inner.width >= 46 || tab == self.tab {
                    match tab {
                        GithubTab::Repositories => "Repos",
                        GithubTab::PullRequests => "PRs",
                        _ => tab.label(),
                    }
                    .to_owned()
                } else {
                    (index + 1).to_string()
                };
                (A::Tab(tab), label)
            })
            .collect();
        place_controls(
            &mut geometry.controls,
            row(if roomy { 3 } else { 1 }),
            &tabs,
        );
        let has_detail =
            self.detail.is_some() || self.selected_key.is_some() || self.selected_run.is_some();
        let mut actions = Vec::new();
        if !self.available_queues().is_empty() {
            actions.push((
                A::ChooseQueue,
                format!("Queue: {} ▾", queue_label(self.queue)),
            ));
        }
        actions.push((A::Palette, "Actions…".into()));
        if has_detail {
            actions.push((A::Back, "Back".into()));
            if self.item().is_some() {
                actions.extend([
                    (A::Detail(DetailTab::Description), "Description".into()),
                    (A::Detail(DetailTab::Conversation), "Conversation".into()),
                ]);
                if self
                    .item()
                    .is_some_and(|item| item.summary.key.kind == ItemKind::PullRequest)
                {
                    actions.push((A::Detail(DetailTab::Diff), "Diff".into()));
                }
            }
        } else {
            actions.extend([(A::Refresh, "Refresh".into()), (A::Filter, "Filter".into())]);
            if self.has_more() {
                actions.push((A::More, "More".into()));
            }
            if self.repository.is_some() {
                actions.push((A::ResetRepository, "Reset repo".into()));
            }
        }
        place_controls(
            &mut geometry.controls,
            row(if roomy { 5 } else { 2 }),
            &actions,
        );
        if self.queue_menu.is_some() {
            if let Some(trigger) = geometry
                .controls
                .iter()
                .find(|control| control.action == A::ChooseQueue)
            {
                let width = 24.min(area.width);
                let height = (self.available_queues().len() as u16 + 2).min(area.height);
                let below = trigger.area.bottom();
                let y = if below + height <= area.bottom() {
                    below
                } else {
                    trigger.area.y.saturating_sub(height).max(area.y)
                };
                geometry.queue_menu = Rect::new(
                    trigger.area.x.min(area.right().saturating_sub(width)),
                    y,
                    width,
                    height,
                );
            } else {
                self.queue_menu = None;
            }
        }
        let content = stack.content;
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
        let visible = self.visible_entries();
        let total = visible.len();
        for (index, entry) in visible.iter().enumerate() {
            let entry = &self.entries[*entry];
            if matches!(entry, Entry::Heading(_)) && index > 0 {
                geometry.list_rows.push(ListRow::Gap);
            }
            geometry.list_rows.push(ListRow::Entry(index));
            if roomy && matches!(entry, Entry::Item(_) | Entry::Run(_, _)) {
                geometry.list_rows.push(ListRow::Metadata(index));
            }
        }
        self.selected = self.selected.min(total.saturating_sub(1));
        self.list_scroll = ModalListViewport::new(
            geometry.list_rows.len(),
            geometry.list.height as usize,
            self.list_scroll,
        )
        .scroll();
        self.detail_scroll = ModalListViewport::new(
            self.detail_len(),
            geometry.detail.height as usize,
            self.detail_scroll,
        )
        .scroll();
        geometry.status = stack.footer.unwrap_or_default();
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
        self.selected_link = None;
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
                    let width = width.min(80);
                    let mut append =
                        |text: &str, role| {
                            rows.extend(rich_text::plain(text, role, width).into_iter().map(
                                |spans| TextRow {
                                    spans,
                                    target: None,
                                    failure: false,
                                },
                            ));
                        };
                    append(&item.summary.title, TextRole::Title);
                    append(
                        &format!(
                            "{} #{} · @{}",
                            item.summary.key.repository,
                            item.summary.key.number,
                            item.summary.author.as_deref().unwrap_or("unknown"),
                        ),
                        TextRole::Muted,
                    );
                    append("", TextRole::Body);
                    let (state, role) = description_status(&item.summary.state);
                    append(
                        &format!(
                            "{state}{}",
                            if item.summary.is_draft {
                                " · Draft"
                            } else {
                                ""
                            },
                        ),
                        if item.summary.is_draft {
                            TextRole::Muted
                        } else {
                            role
                        },
                    );
                    if let Some(merge) = &item.merge {
                        if let Some(review) =
                            merge.review_decision.as_deref().filter(|s| !s.is_empty())
                        {
                            let (label, role) = description_status(review);
                            append(&format!("Review · {label}"), role);
                        }
                        let (label, role) = description_status(&merge.merge_state_status);
                        append(&format!("Merge · {label}"), role);
                        if merge.queue_enabled || merge.auto_merge_enabled {
                            append(
                                match (merge.queue_enabled, merge.auto_merge_enabled) {
                                    (true, true) => "Merge queue enabled · Auto-merge enabled",
                                    (true, false) => "Merge queue enabled",
                                    _ => "Auto-merge enabled",
                                },
                                TextRole::Muted,
                            );
                        }
                        append("", TextRole::Body);
                        append(
                            &format!("{} → {}", merge.head_branch, merge.base_branch),
                            TextRole::Code,
                        );
                        let mut changes = Vec::new();
                        if let Some(files) = item.changed_files {
                            changes.push(format!(
                                "{files} {}",
                                if files == 1 { "file" } else { "files" }
                            ));
                        }
                        if let Some(additions) = item.additions {
                            changes.push(format!("+{additions}"));
                        }
                        if let Some(deletions) = item.deletions {
                            changes.push(format!("−{deletions}"));
                        }
                        if !changes.is_empty() {
                            append(&changes.join(" · "), TextRole::Muted);
                        }
                        if !merge.head_sha.is_empty() {
                            append(
                                &format!(
                                    "Head {}",
                                    merge.head_sha.chars().take(7).collect::<String>()
                                ),
                                TextRole::Muted,
                            );
                        }
                    }
                    if !item.labels.is_empty() {
                        append(
                            &format!(
                                "Labels · {}",
                                item.labels
                                    .iter()
                                    .map(|label| label.name.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", "),
                            ),
                            TextRole::Muted,
                        );
                    }
                    if !item.assignees.is_empty() {
                        append(
                            &format!(
                                "Assignees · {}",
                                item.assignees
                                    .iter()
                                    .map(|user| format!("@{}", user.login))
                                    .collect::<Vec<_>>()
                                    .join(", "),
                            ),
                            TextRole::Muted,
                        );
                    }
                    append("", TextRole::Body);
                    append("Description", TextRole::Heading);
                    append("", TextRole::Body);
                    let body = item.body.as_deref().filter(|body| !body.trim().is_empty());
                    let lines = match body {
                        Some(body) => rich_text::markdown(body, width),
                        None => rich_text::plain("No description.", TextRole::Muted, width),
                    };
                    rows.extend(lines.into_iter().map(|spans| TextRow {
                        spans,
                        target: None,
                        failure: false,
                    }));
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

fn description_status(status: &str) -> (&str, TextRole) {
    match status {
        "OPEN" | "open" => ("Open", TextRole::Success),
        "CLOSED" | "closed" => ("Closed", TextRole::Muted),
        "MERGED" | "merged" => ("Merged", TextRole::Success),
        "APPROVED" => ("Approved", TextRole::Success),
        "CHANGES_REQUESTED" => ("Changes requested", TextRole::Warning),
        "REVIEW_REQUIRED" => ("Review required", TextRole::Warning),
        "CLEAN" => ("Ready to merge", TextRole::Success),
        "BLOCKED" => ("Blocked", TextRole::Warning),
        "BEHIND" => ("Branch out of date", TextRole::Warning),
        "DIRTY" => ("Conflicts", TextRole::Danger),
        "DRAFT" => ("Draft", TextRole::Muted),
        "HAS_HOOKS" => ("Merge hooks present", TextRole::Body),
        "UNSTABLE" => ("Checks not passing", TextRole::Warning),
        "UNKNOWN" => ("Unknown", TextRole::Muted),
        _ => (status, TextRole::Body),
    }
}
