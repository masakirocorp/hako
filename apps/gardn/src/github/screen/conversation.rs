use super::*;

impl GithubScreen {
    pub fn selected_comment(&self) -> Option<(&Comment, CommentKind)> {
        let item = self.item()?;
        match self.selected_row? {
            RowTarget::Comment {
                review: true,
                index,
            } => item
                .review_comments
                .get(index)
                .map(|comment| (comment, CommentKind::Review)),
            RowTarget::Comment {
                review: false,
                index,
            } => item
                .comments
                .get(index)
                .map(|comment| (comment, CommentKind::General)),
            _ => None,
        }
    }
    pub(super) fn owns_selected_comment(&self) -> bool {
        self.selected_comment()
            .and_then(|(comment, _)| comment.user.as_ref())
            .zip(self.viewer.as_ref())
            .is_some_and(|(author, viewer)| author.login.eq_ignore_ascii_case(&viewer.login))
    }
    pub(super) fn compose(&mut self, kind: ComposerKind, title: &str, body: String) {
        if let Some(item) = self.selected_key.clone() {
            self.error = None;
            self.control_focus = 0;
            self.focus = Focus::Detail;
            self.dialog = Some(Dialog::Composer {
                title: title.into(),
                item,
                kind,
                text: TextBuffer::new(body),
            });
        }
    }
    pub(super) fn mutate(&mut self, mutation: GithubMutation) {
        if self.submitting {
            return;
        }
        self.submitting = true;
        self.error = None;
        self.enqueue(GithubRequest::Mutate(mutation));
    }
    pub(super) fn submit_dialog(&mut self) {
        let Some(dialog) = self.dialog.clone() else {
            return;
        };
        match dialog {
            Dialog::Filter(text) => {
                self.filter = text.value;
                self.entries_dirty = true;
                self.dialog = None;
                self.selected = 0;
                self.list_scroll = 0;
            }
            Dialog::FileSearch(text) => {
                self.file_filter = text.value;
                self.file_scroll = 0;
                self.dialog = None;
                self.show_files = true;
                if let Some(index) = self
                    .diff
                    .as_ref()
                    .and_then(|diff| diff.matching_files(&self.file_filter).first().copied())
                {
                    if let Some(diff) = &mut self.diff {
                        if let Err(error) = diff.select_file(index) {
                            self.error = Some(format!("Cannot select file: {error:?}"));
                        }
                    }
                    self.detail_scroll = 0;
                }
            }
            Dialog::Confirm { mutation, .. } => self.mutate(mutation),
            Dialog::Composer {
                item, kind, text, ..
            } => {
                if text.value.trim().is_empty() && !matches!(kind, ComposerKind::Review(_)) {
                    self.error = Some("Write a comment before submitting.".into());
                    return;
                }
                let mutation = match kind {
                    ComposerKind::Comment => GithubMutation::Comment {
                        item,
                        body: text.value,
                    },
                    ComposerKind::Reply(comment_id) => GithubMutation::Reply {
                        item,
                        comment_id,
                        body: text.value,
                    },
                    ComposerKind::Edit(kind, comment_id) => GithubMutation::EditComment {
                        item,
                        kind,
                        comment_id,
                        body: text.value,
                    },
                    ComposerKind::Inline(mut comment) => {
                        comment.body = text.value;
                        GithubMutation::InlineComment { item, comment }
                    }
                    ComposerKind::Review(event) => {
                        let Some(head_sha) = self
                            .item()
                            .and_then(|item| item.merge.as_ref())
                            .map(|merge| merge.head_sha.clone())
                        else {
                            self.error = Some("Refresh the pull request before reviewing.".into());
                            return;
                        };
                        GithubMutation::Review {
                            item,
                            head_sha,
                            event,
                            body: text.value,
                        }
                    }
                };
                self.mutate(mutation);
            }
            Dialog::Labels { selected } => {
                self.activate(GithubAction::ToggleLabel(selected));
            }
            Dialog::Merge => {}
        }
    }
}
