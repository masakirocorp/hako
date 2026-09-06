use super::*;

impl GithubScreen {
    pub(super) fn request_diff(&mut self) {
        if self
            .pending
            .values()
            .any(|tracked| matches!(tracked.request, GithubRequest::Diff { .. }))
            || self
                .queued
                .iter()
                .any(|request| matches!(request, GithubRequest::Diff { .. }))
        {
            return;
        }
        if let Some(item) = self.item() {
            if let Some(merge) = &item.merge {
                self.enqueue(GithubRequest::Diff {
                    item: item.summary.key.clone(),
                    head_sha: merge.head_sha.clone(),
                });
            }
        }
    }
    pub(super) fn select_diff_row(&mut self, row: usize, column: u16, extend: bool) {
        let Some(diff) = &mut self.diff else {
            return;
        };
        let Some(DiffRow::Line { left, right }) = diff.layout().rows.get(row) else {
            return;
        };
        let side = if diff.options().mode == DiffMode::Split {
            if column < self.geometry.detail.x + self.geometry.detail.width / 2 {
                DiffSide::Left
            } else {
                DiffSide::Right
            }
        } else if left.is_some() {
            DiffSide::Left
        } else {
            DiffSide::Right
        };
        if let Some(line) = left
            .as_ref()
            .and_then(|cell| cell.line_on(side))
            .or_else(|| right.as_ref().and_then(|cell| cell.line_on(side)))
        {
            if let Err(error) = diff.select(side, line, extend) {
                self.error = Some(format!(
                    "Select lines on the same side and within one hunk: {error:?}"
                ));
            } else {
                self.error = None;
                self.selected_row = None;
            }
        }
    }
    pub(super) fn move_diff(&mut self, forward: bool, extend: bool) {
        let Some(diff) = &mut self.diff else {
            return;
        };
        let current = diff
            .cursor()
            .and_then(|cursor| diff.layout().row_for(cursor.side, cursor.line));
        let start = current.unwrap_or(self.detail_scroll);
        let side = diff.cursor().map(|cursor| cursor.side);
        let count = if forward {
            diff.layout()
                .rows
                .len()
                .saturating_sub(start + usize::from(current.is_some()))
        } else {
            start
        };
        for offset in 0..count {
            let index = if forward {
                start + usize::from(current.is_some()) + offset
            } else {
                start - 1 - offset
            };
            if let DiffRow::Line { left, right } = &diff.layout().rows[index] {
                let candidates = [(DiffSide::Left, left), (DiffSide::Right, right)];
                let chosen = candidates
                    .iter()
                    .filter(|(candidate, _)| side.is_none_or(|side| !extend || *candidate == side))
                    .find_map(|(side, cell)| {
                        cell.as_ref()
                            .and_then(|cell| cell.line_on(*side))
                            .map(|line| (*side, line))
                            .filter(|(side, line)| {
                                diff.cursor().is_none_or(|cursor| {
                                    cursor.side != *side || cursor.line != *line
                                })
                            })
                    });
                if let Some((side, line)) = chosen {
                    if let Err(error) = diff.select(side, line, extend) {
                        self.error = Some(format!(
                            "Select lines on the same side and within one hunk: {error:?}"
                        ));
                    } else {
                        self.detail_scroll = ModalListViewport::new(
                            diff.layout().rows.len(),
                            self.geometry.detail.height as usize,
                            self.detail_scroll,
                        )
                        .ensure_visible(index, None);
                    }
                    break;
                }
            }
        }
    }
    pub(super) fn navigate_thread(&mut self, forward: bool) {
        let Some(item) = self.item() else {
            return;
        };
        let threads: Vec<_> = item
            .review_comments
            .iter()
            .enumerate()
            .filter_map(|(index, comment)| {
                Some((
                    index,
                    ThreadLocation {
                        path: comment.path.clone()?,
                        line: u32::try_from(comment.line?).ok()?,
                        side: match comment.side? {
                            Side::Left => DiffSide::Left,
                            Side::Right => DiffSide::Right,
                        },
                    },
                ))
            })
            .collect();
        let Some(diff) = &mut self.diff else {
            return;
        };
        let locations: Vec<_> = threads
            .iter()
            .map(|(_, location)| location.clone())
            .collect();
        if let Some(index) = diff.navigate_thread(&locations, forward) {
            self.selected_row = Some(RowTarget::Comment {
                review: true,
                index: threads[index].0,
            });
            if let Some(cursor) = diff.cursor() {
                self.detail_scroll = diff.layout().row_for(cursor.side, cursor.line).unwrap_or(0);
            }
            self.notice = self
                .selected_comment()
                .map(|(comment, _)| format!("Thread: {}", comment.body));
        } else {
            self.notice = Some("No current inline threads map to the loaded diff.".into());
        }
    }
}
