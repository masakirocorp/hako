use super::*;

impl GithubScreen {
    pub(super) fn merge_methods(&self) -> Vec<(MergeMethod, &'static str)> {
        let Some(repo) = &self.merge_repository else {
            return Vec::new();
        };
        [
            (repo.allow_squash_merge, MergeMethod::Squash, "Squash"),
            (repo.allow_merge_commit, MergeMethod::Merge, "Merge commit"),
            (repo.allow_rebase_merge, MergeMethod::Rebase, "Rebase"),
        ]
        .into_iter()
        .filter(|(allowed, _, _)| *allowed)
        .map(|(_, method, label)| (method, label))
        .collect()
    }
    pub(super) fn merge_actions(&self) -> Vec<(GithubAction, String)> {
        let Some(item) = self.item() else {
            return Vec::new();
        };
        let Some(merge) = &item.merge else {
            return Vec::new();
        };
        let Some(repo) = &self.merge_repository else {
            return Vec::new();
        };
        if !item.summary.state.eq_ignore_ascii_case("open") || item.summary.is_draft {
            return Vec::new();
        }
        let mut actions = Vec::new();
        if merge.auto_merge_enabled {
            actions.push((GithubAction::DisableAuto, "Disable auto-merge".into()));
        }
        let conflicting = merge.mergeable.eq_ignore_ascii_case("conflicting");
        let blocked = matches!(
            merge.review_decision.as_deref(),
            Some("CHANGES_REQUESTED" | "REVIEW_REQUIRED")
        ) || item.checks.iter().any(|check| {
            !check.status.eq_ignore_ascii_case("completed")
                || !matches!(
                    check.conclusion.as_deref(),
                    Some("success" | "neutral" | "skipped")
                )
        });
        for (index, (_, label)) in self.merge_methods().iter().enumerate() {
            if !conflicting
                && (merge.queue_enabled
                    || !blocked && merge.mergeable.eq_ignore_ascii_case("mergeable"))
            {
                actions.push((
                    GithubAction::MergeNow(index),
                    if merge.queue_enabled {
                        format!("Add to merge queue ({label})")
                    } else {
                        format!("{label} now")
                    },
                ));
            }
            if !conflicting && !merge.auto_merge_enabled && repo.allow_auto_merge == Some(true) {
                actions.push((
                    GithubAction::MergeAuto(index),
                    format!("{label} when ready"),
                ));
            }
        }
        actions
    }
}
