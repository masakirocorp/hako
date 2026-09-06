use super::*;

impl GithubScreen {
    pub(super) fn fill_runs_requests(&mut self) {
        if self.tab != GithubTab::Actions
            || self.repository.is_some()
            || self.selected_run.is_some()
        {
            return;
        }
        while self.pending.len() + self.queued.len() + self.awaiting_ids.len() < 4 {
            let Some((repository, cursor)) = self.runs_backlog.pop_front() else {
                break;
            };
            self.enqueue(GithubRequest::Runs {
                repository,
                head_sha: None,
                cursor,
                page_size: 50,
            });
        }
    }
    pub(super) fn more_scoped_runs(&mut self) {
        if self.selected_run.is_some() {
            self.notice = Some("Return to the run list to load more results.".into());
            return;
        }
        self.error = None;
        if let Some(cursor) = self.catalog_cursor.take() {
            self.enqueue(GithubRequest::Repositories {
                cursor: Some(cursor),
                page_size: 50,
            });
        }
        self.runs_backlog.extend(
            self.runs_cursors
                .drain(..)
                .map(|(repository, cursor)| (repository, Some(cursor))),
        );
        self.fill_runs_requests();
    }
    pub(super) fn apply_runs_page(
        &mut self,
        repository: GithubRepository,
        page: Page<WorkflowRun>,
    ) {
        let selected = self
            .visible_entries()
            .get(self.selected)
            .and_then(|index| self.entries.get(*index))
            .and_then(|entry| match entry {
                Entry::Run(repository, run) => Some((repository.clone(), run.id)),
                _ => None,
            });
        if self.repository.is_none() {
            if let Some(cursor) = page.next_cursor {
                self.runs_cursors.push((repository.clone(), cursor));
            }
        } else {
            self.next_cursor = page.next_cursor;
        }
        self.entries.extend(
            page.items
                .into_iter()
                .map(|run| Entry::Run(repository.clone(), run)),
        );
        self.entries.sort_by(|a, b| match (a, b) {
            (Entry::Run(a_repo, a), Entry::Run(b_repo, b)) => b
                .created_at
                .cmp(&a.created_at)
                .then(a_repo.cmp(b_repo))
                .then(b.id.cmp(&a.id)),
            _ => std::cmp::Ordering::Equal,
        });
        self.entries_dirty = true;
        self.update_visible_entries();
        if let Some((repository, id)) = selected {
            if let Some(index) = self.visible_entries().iter().position(|index| matches!(&self.entries[*index], Entry::Run(repo, run) if *repo == repository && run.id == id)) { self.selected = index; }
        }
    }
}
