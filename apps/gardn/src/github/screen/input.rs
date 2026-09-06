use super::*;

impl GithubScreen {
    pub fn paste(&mut self, value: &str) {
        if self.submitting {
            return;
        }
        match &mut self.dialog {
            Some(Dialog::Composer { text, .. }) => text.insert(value),
            Some(Dialog::Filter(text) | Dialog::FileSearch(text)) => {
                text.insert(&value.replace(['\r', '\n'], " "))
            }
            _ => {}
        }
    }
    pub fn clear_hover(&mut self) {
        self.mouse_position = None;
        if let Some(menu) = &mut self.menu {
            menu.list.hover(None);
        }
    }

    pub fn hovered(&self, area: Rect) -> bool {
        !self.submitting
            && self.menu.is_none()
            && self
                .mouse_position
                .is_some_and(|point| contains(area, point))
    }

    pub fn hovered_row(&self, area: Rect, total: usize, scroll: usize) -> Option<usize> {
        if self.submitting || self.menu.is_some() {
            return None;
        }
        let (x, y) = self.mouse_position?;
        ModalListViewport::new(total, area.height as usize, scroll).hit_visual_row(area, x, y)
    }

    pub(super) fn link_url(&self, (row, span): (usize, usize)) -> Option<&str> {
        let url = self
            .detail_rows
            .get(row)?
            .spans
            .get(span)?
            .link
            .as_deref()?;
        if url.chars().any(char::is_control) {
            return None;
        }
        crate::app::actions::safe_web_url(url)
    }

    pub fn hovered_link(&self) -> Option<(usize, usize)> {
        if self.dialog.is_some() || self.detail_tab == DetailTab::Diff {
            return None;
        }
        let area = self.geometry.detail;
        let row = self.hovered_row(area, self.detail_rows.len(), self.detail_scroll)?;
        let column = usize::from(self.mouse_position?.0.saturating_sub(area.x));
        let mut end = 0;
        for (span, content) in self.detail_rows[row].spans.iter().enumerate() {
            end += UnicodeWidthStr::width(content.text.as_str());
            if column < end {
                return self.link_url((row, span)).map(|_| (row, span));
            }
        }
        None
    }

    fn move_link_focus(&mut self, forward: bool) -> bool {
        if self.detail_tab == DetailTab::Diff {
            return false;
        }
        let mut previous_url = None;
        let mut links = self
            .detail_rows
            .iter()
            .enumerate()
            .flat_map(|(row, line)| {
                line.spans
                    .iter()
                    .enumerate()
                    .map(move |(span, _)| (row, span))
            })
            .filter(|&key| {
                let url = self.link_url(key);
                let distinct = url.is_some() && url != previous_url;
                previous_url = url;
                distinct
            });
        let next = if forward {
            links.find(|key| self.selected_link.is_none_or(|current| *key > current))
        } else {
            links
                .filter(|key| self.selected_link.is_none_or(|current| *key < current))
                .last()
        };
        let Some(key) = next else {
            self.selected_link = None;
            return false;
        };
        self.selected_link = Some(key);
        self.detail_scroll = ModalListViewport::new(
            self.detail_rows.len(),
            self.geometry.detail.height as usize,
            self.detail_scroll,
        )
        .ensure_visible(key.0, None);
        true
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Vec<GithubEffect> {
        if key.kind == KeyEventKind::Release {
            return Vec::new();
        }
        if self.submitting {
            return Vec::new();
        }
        self.clear_hover();
        if let Some(menu) = &mut self.menu {
            let count = menu.items.len();
            match key.code {
                KeyCode::Esc => self.menu = None,
                KeyCode::Up | KeyCode::BackTab => menu.list.move_prev(),
                KeyCode::Down | KeyCode::Tab => menu.list.move_next(count),
                KeyCode::Home => menu.list.select(0),
                KeyCode::End => menu.list.select(count.saturating_sub(1)),
                KeyCode::Enter | KeyCode::Char(' ') => {
                    if let Some(&(action, _)) = menu.items.get(menu.list.selected) {
                        return self.activate(action);
                    }
                }
                _ => {}
            }
            self.compute(self.geometry.area);
            return Vec::new();
        }
        if self.dialog.is_some() {
            if key.code == KeyCode::Esc {
                return self.activate(GithubAction::Cancel);
            }
            if key.code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
                return self.activate(GithubAction::Submit);
            }
            if key.code == KeyCode::Tab || key.code == KeyCode::BackTab {
                let count = self.geometry.controls.len();
                if count > 0 {
                    self.control_focus = if key.code == KeyCode::BackTab {
                        (self.control_focus + count - 1) % count
                    } else {
                        (self.control_focus + 1) % count
                    };
                    self.focus = Focus::Controls;
                }
                return Vec::new();
            }
            if self.focus == Focus::Controls && key.code == KeyCode::Enter {
                if let Some(control) = self.geometry.controls.get(self.control_focus) {
                    return self.activate(control.action);
                }
            }
            match &mut self.dialog {
                Some(Dialog::Composer { text, .. }) => {
                    text.key(key, true);
                    self.focus = Focus::Detail;
                }
                Some(Dialog::Filter(text) | Dialog::FileSearch(text)) => {
                    if key.code == KeyCode::Enter {
                        return self.activate(GithubAction::Submit);
                    }
                    text.key(key, false);
                    self.focus = Focus::Detail;
                }
                Some(Dialog::Labels { selected }) => {
                    if key.code == KeyCode::Down {
                        *selected = selected
                            .saturating_add(1)
                            .min(self.labels.len().saturating_sub(1));
                    }
                    if key.code == KeyCode::Up {
                        *selected = selected.saturating_sub(1);
                    }
                    if key.code == KeyCode::Enter {
                        return self.activate(GithubAction::Submit);
                    }
                }
                Some(Dialog::Confirm { .. } | Dialog::Merge) => {
                    if matches!(
                        key.code,
                        KeyCode::Left | KeyCode::Up | KeyCode::Right | KeyCode::Down
                    ) {
                        let count = self.geometry.controls.len();
                        if count > 0 {
                            self.control_focus = if matches!(key.code, KeyCode::Left | KeyCode::Up)
                            {
                                (self.control_focus + count - 1) % count
                            } else {
                                (self.control_focus + 1) % count
                            };
                            self.focus = Focus::Controls;
                        }
                    }
                    if key.code == KeyCode::Enter {
                        if let Some(control) = self.geometry.controls.get(self.control_focus) {
                            return self.activate(control.action);
                        }
                    }
                }
                None => {}
            }
            self.compute(self.geometry.area);
            return Vec::new();
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('p') {
            return self.activate(GithubAction::Palette);
        }
        if self.focus == Focus::Detail && key.code == KeyCode::Enter {
            if let Some(url) = self.selected_link.and_then(|key| self.link_url(key)) {
                return vec![GithubEffect::OpenUrl(url.to_owned())];
            }
        }
        let action = match key.code {
            KeyCode::Esc => Some(
                if self.detail.is_some()
                    || self.selected_key.is_some()
                    || self.selected_run.is_some()
                {
                    GithubAction::Back
                } else {
                    GithubAction::CloseScreen
                },
            ),
            KeyCode::Char('1') => Some(GithubAction::Tab(GithubTab::Overview)),
            KeyCode::Char('2') => Some(GithubAction::Tab(GithubTab::Repositories)),
            KeyCode::Char('3') => Some(GithubAction::Tab(GithubTab::PullRequests)),
            KeyCode::Char('4') => Some(GithubAction::Tab(GithubTab::Issues)),
            KeyCode::Char('5') => Some(GithubAction::Tab(GithubTab::Actions)),
            KeyCode::Char('/') => Some(GithubAction::Filter),
            KeyCode::Char('r') => Some(GithubAction::Refresh),
            KeyCode::Char('b') => Some(GithubAction::Browser),
            KeyCode::Char('y') => Some(GithubAction::CopyUrl),
            KeyCode::Char('e') => Some(GithubAction::Editor),
            KeyCode::Char('c') => Some(if self.detail_tab == DetailTab::Diff {
                GithubAction::InlineComment
            } else {
                GithubAction::Comment
            }),
            KeyCode::Char('m') => Some(GithubAction::Merge),
            KeyCode::Char('n') => Some(if self.detail_tab == DetailTab::Diff {
                GithubAction::NextThread
            } else {
                GithubAction::NextFailure
            }),
            KeyCode::Char('N') => Some(if self.detail_tab == DetailTab::Diff {
                GithubAction::PreviousThread
            } else {
                GithubAction::PreviousFailure
            }),
            KeyCode::Char('f') if self.detail_tab == DetailTab::Diff => {
                Some(GithubAction::FindFile)
            }
            KeyCode::Char('w') if self.detail_tab == DetailTab::Diff => {
                Some(GithubAction::ToggleWrap)
            }
            KeyCode::Char('s') if self.detail_tab == DetailTab::Diff => {
                Some(GithubAction::ToggleSplit)
            }
            KeyCode::Char('?') => Some(GithubAction::Palette),
            KeyCode::Enter | KeyCode::Char(' ') if self.focus == Focus::Controls => self
                .geometry
                .controls
                .get(self.control_focus)
                .map(|control| control.action),
            KeyCode::Enter if self.focus == Focus::List => Some(GithubAction::Open),
            KeyCode::Enter if self.selected_comment().is_some() => Some(GithubAction::Reply),
            _ => None,
        };
        if let Some(action) = action {
            return self.activate(action);
        }
        match key.code {
            KeyCode::Tab => {
                if self.focus == Focus::Detail && self.move_link_focus(true) {
                    self.compute(self.geometry.area);
                    return Vec::new();
                }
                self.focus = match self.focus {
                    Focus::List if self.detail.is_some() => Focus::Detail,
                    Focus::List | Focus::Detail => Focus::Controls,
                    Focus::Controls => Focus::List,
                };
                if self.focus == Focus::Detail {
                    self.move_link_focus(true);
                }
            }
            KeyCode::BackTab => {
                if self.focus == Focus::Detail && self.move_link_focus(false) {
                    self.compute(self.geometry.area);
                    return Vec::new();
                }
                self.focus = match self.focus {
                    Focus::List => Focus::Controls,
                    Focus::Detail => Focus::List,
                    Focus::Controls if self.detail.is_some() => Focus::Detail,
                    Focus::Controls => Focus::List,
                };
                if self.focus == Focus::Detail {
                    self.move_link_focus(false);
                }
            }
            KeyCode::Up | KeyCode::Down | KeyCode::Char('j' | 'k') => {
                self.selected_link = None;
                let forward = matches!(key.code, KeyCode::Down | KeyCode::Char('j'));
                if self.focus == Focus::Controls {
                    let count = self.geometry.controls.len();
                    if count > 0 {
                        self.control_focus = if forward {
                            (self.control_focus + 1) % count
                        } else {
                            (self.control_focus + count - 1) % count
                        };
                    }
                } else if self.focus == Focus::List {
                    self.move_list(forward);
                } else if self.detail_tab == DetailTab::Diff && self.diff.is_some() {
                    self.move_diff(forward, key.modifiers.contains(KeyModifiers::SHIFT));
                } else {
                    self.scroll(Focus::Detail, if forward { 1 } else { -1 });
                    self.selected_row = self
                        .detail_rows
                        .get(self.detail_scroll)
                        .and_then(|row| row.target);
                }
            }
            KeyCode::Left | KeyCode::Right if self.focus == Focus::Controls => {
                let count = self.geometry.controls.len();
                if count > 0 {
                    self.control_focus = if key.code == KeyCode::Right {
                        (self.control_focus + 1) % count
                    } else {
                        (self.control_focus + count - 1) % count
                    };
                }
            }
            KeyCode::Left | KeyCode::Right if self.detail_tab == DetailTab::Diff => {
                if let Some(diff) = &mut self.diff {
                    let side = if key.code == KeyCode::Left {
                        DiffSide::Left
                    } else {
                        DiffSide::Right
                    };
                    if let Some(cursor) = diff.cursor() {
                        if let Some(DiffRow::Line { left, right }) = diff
                            .layout()
                            .row_for(cursor.side, cursor.line)
                            .and_then(|row| diff.layout().rows.get(row))
                        {
                            if let Some(line) = left
                                .as_ref()
                                .and_then(|cell| cell.line_on(side))
                                .or_else(|| right.as_ref().and_then(|cell| cell.line_on(side)))
                            {
                                if let Err(error) = diff.select(
                                    side,
                                    line,
                                    key.modifiers.contains(KeyModifiers::SHIFT),
                                ) {
                                    self.error = Some(format!("Selection must stay on one side and within one hunk: {error:?}"));
                                }
                            }
                        }
                    }
                }
            }
            KeyCode::PageDown => self.scroll(self.focus, 10),
            KeyCode::PageUp => self.scroll(self.focus, -10),
            KeyCode::Home => {
                if self.focus == Focus::List {
                    self.selected = 0;
                    self.list_scroll = 0;
                } else {
                    self.detail_scroll = 0;
                }
            }
            KeyCode::End => {
                if self.focus == Focus::List {
                    self.selected = self.visible_entries().len().saturating_sub(1);
                    self.list_scroll = self.geometry.list_rows.len();
                } else {
                    self.detail_scroll = self.detail_len();
                }
            }
            _ => {}
        }
        self.compute(self.geometry.area);
        Vec::new()
    }
    pub(super) fn move_list(&mut self, forward: bool) {
        let visible = self.visible_entries();
        if visible.is_empty() {
            return;
        }
        let mut index = self.selected;
        loop {
            let next = if forward {
                index.saturating_add(1).min(visible.len() - 1)
            } else {
                index.saturating_sub(1)
            };
            if next == index {
                break;
            }
            index = next;
            if self.entries[visible[index]].selectable() {
                break;
            }
        }
        if self.entries[visible[index]].selectable() {
            self.selected = index;
        }
        if let Some(row) = self
            .geometry
            .list_rows
            .iter()
            .position(|row| row.entry() == Some(self.selected))
        {
            self.list_scroll = ModalListViewport::new(
                self.geometry.list_rows.len(),
                self.geometry.list.height as usize,
                self.list_scroll,
            )
            .ensure_visible(row, None);
        }
    }
    pub(super) fn scroll(&mut self, focus: Focus, delta: i16) {
        let offset = if focus == Focus::List {
            &mut self.list_scroll
        } else {
            &mut self.detail_scroll
        };
        *offset = if delta < 0 {
            offset.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            offset.saturating_add(delta as usize)
        };
    }
    pub fn detail_len(&self) -> usize {
        if self.detail_tab == DetailTab::Diff && self.item().is_some() {
            self.diff
                .as_ref()
                .map_or(0, |diff| diff.layout().rows.len())
        } else {
            self.detail_rows.len()
        }
    }
    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> Vec<GithubEffect> {
        let point = (mouse.column, mouse.row);
        self.mouse_position = contains(self.geometry.area, point).then_some(point);
        if self.submitting {
            return Vec::new();
        }
        if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
            self.scrollbar_drag = None;
            self.file_scrollbar_drag = None;
            self.diff_drag = false;
            return Vec::new();
        }
        if let Some(menu) = &mut self.menu {
            let inner = inset(self.geometry.menu);
            let viewport =
                ModalListViewport::new(menu.items.len(), inner.height as usize, menu.scroll);
            let hit =
                viewport.hit_visual_row(viewport.scroll_area(inner).body, mouse.column, mouse.row);
            menu.list.hover(hit);
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    if let Some(index) = hit {
                        let action = menu.items[index].0;
                        return self.activate(action);
                    }
                    self.menu = None;
                }
                MouseEventKind::ScrollDown if contains(inner, point) => {
                    menu.list.hover(None);
                    menu.list.move_next(menu.items.len());
                }
                MouseEventKind::ScrollUp if contains(inner, point) => {
                    menu.list.hover(None);
                    menu.list.move_prev();
                }
                _ => {}
            }
            self.compute(self.geometry.area);
            return Vec::new();
        }
        if mouse.kind == MouseEventKind::Moved {
            return Vec::new();
        }
        if !contains(self.geometry.area, point) {
            return Vec::new();
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            if let Some((index, control)) = self
                .geometry
                .controls
                .iter()
                .enumerate()
                .find(|(_, control)| contains(control.area, point))
            {
                let action = control.action;
                self.control_focus = index;
                self.focus = Focus::Controls;
                return self.activate(action);
            }
        }
        if self.dialog.is_some() {
            if contains(self.geometry.input, point) {
                if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                    self.focus = Focus::Detail;
                    if let Some(
                        Dialog::Composer { text, .. }
                        | Dialog::Filter(text)
                        | Dialog::FileSearch(text),
                    ) = &mut self.dialog
                    {
                        text.click(self.geometry.input, mouse.column, mouse.row);
                    }
                }
                if let Some(Dialog::Labels { selected }) = &mut self.dialog {
                    if mouse.kind == MouseEventKind::ScrollDown {
                        *selected = selected
                            .saturating_add(1)
                            .min(self.labels.len().saturating_sub(1));
                    }
                    if mouse.kind == MouseEventKind::ScrollUp {
                        *selected = selected.saturating_sub(1);
                    }
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                        let top = selected
                            .saturating_sub(self.geometry.input.height.saturating_sub(1) as usize);
                        let index = top + mouse.row.saturating_sub(self.geometry.input.y) as usize;
                        return self.activate(GithubAction::ToggleLabel(index));
                    }
                }
            }
            self.compute(self.geometry.area);
            return Vec::new();
        }
        if let Some((focus, grab)) = self.scrollbar_drag {
            if matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left)) {
                let area = if focus == Focus::List {
                    self.geometry.list
                } else {
                    self.geometry.detail
                };
                let total = if focus == Focus::List {
                    self.geometry.list_rows.len()
                } else {
                    self.detail_len()
                };
                let offset = if focus == Focus::List {
                    self.list_scroll
                } else {
                    self.detail_scroll
                };
                let viewport = ModalListViewport::new(total, area.height as usize, offset);
                if let Some(track) = viewport.scroll_area(area).track {
                    let bottom = crate::ui::scrollbar_offset_from_drag_row(
                        viewport.metrics(),
                        track,
                        mouse.row,
                        grab,
                    );
                    let scroll = viewport.scroll_from_offset_from_bottom(bottom);
                    if focus == Focus::List {
                        self.list_scroll = scroll;
                    } else {
                        self.detail_scroll = scroll;
                    }
                }
                return Vec::new();
            }
        }
        if let Some(grab) = self.file_scrollbar_drag {
            if matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left)) {
                if let Some(diff) = &self.diff {
                    let viewport = ModalListViewport::new(
                        diff.matching_files(&self.file_filter).len(),
                        self.geometry.files.height as usize,
                        self.file_scroll,
                    );
                    if let Some(track) = viewport.scroll_area(self.geometry.files).track {
                        let bottom = crate::ui::scrollbar_offset_from_drag_row(
                            viewport.metrics(),
                            track,
                            mouse.row,
                            grab,
                        );
                        self.file_scroll = viewport.scroll_from_offset_from_bottom(bottom);
                    }
                }
                return Vec::new();
            }
        }
        if contains(self.geometry.files, point) {
            if mouse.kind == MouseEventKind::ScrollDown {
                self.file_scroll = self.file_scroll.saturating_add(3);
            }
            if mouse.kind == MouseEventKind::ScrollUp {
                self.file_scroll = self.file_scroll.saturating_sub(3);
            }
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                if let Some(diff) = &self.diff {
                    let files = diff.matching_files(&self.file_filter);
                    let viewport = ModalListViewport::new(
                        files.len(),
                        self.geometry.files.height as usize,
                        self.file_scroll,
                    );
                    if let Some(track) = viewport
                        .scroll_area(self.geometry.files)
                        .track
                        .filter(|track| contains(*track, point))
                    {
                        self.file_scrollbar_drag = Some(
                            crate::ui::scrollbar_thumb_grab_offset(
                                viewport.metrics(),
                                track,
                                mouse.row,
                            )
                            .unwrap_or(0),
                        );
                        let bottom = crate::ui::scrollbar_offset_from_row(
                            viewport.metrics(),
                            track,
                            mouse.row,
                        );
                        self.file_scroll = viewport.scroll_from_offset_from_bottom(bottom);
                    } else if let Some(row) =
                        viewport.hit_visual_row(self.geometry.files, mouse.column, mouse.row)
                    {
                        return self.activate(GithubAction::SelectFile(files[row]));
                    }
                }
            }
            self.compute(self.geometry.area);
            return Vec::new();
        }
        for (focus, area, total, scroll) in [
            (
                Focus::List,
                self.geometry.list,
                self.geometry.list_rows.len(),
                self.list_scroll,
            ),
            (
                Focus::Detail,
                self.geometry.detail,
                self.detail_len(),
                self.detail_scroll,
            ),
        ] {
            if !contains(area, point) {
                continue;
            }
            self.focus = focus;
            let viewport = ModalListViewport::new(total, area.height as usize, scroll);
            if mouse.kind == MouseEventKind::ScrollDown {
                self.scroll(focus, 3);
            }
            if mouse.kind == MouseEventKind::ScrollUp {
                self.scroll(focus, -3);
            }
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                if let Some(track) = viewport
                    .scroll_area(area)
                    .track
                    .filter(|track| contains(*track, point))
                {
                    let grab = crate::ui::scrollbar_thumb_grab_offset(
                        viewport.metrics(),
                        track,
                        mouse.row,
                    );
                    let bottom =
                        crate::ui::scrollbar_offset_from_row(viewport.metrics(), track, mouse.row);
                    let scroll = viewport.scroll_from_offset_from_bottom(bottom);
                    if focus == Focus::List {
                        self.list_scroll = scroll;
                    } else {
                        self.detail_scroll = scroll;
                    }
                    self.scrollbar_drag = Some((focus, grab.unwrap_or(0)));
                } else if let Some(row) = viewport.hit_visual_row(area, mouse.column, mouse.row) {
                    if focus == Focus::List {
                        if let Some(index) = self.geometry.list_rows[row].entry() {
                            if self.entries[self.visible_entries()[index]].selectable() {
                                self.selected = index;
                                return self.activate(GithubAction::Open);
                            }
                        }
                        return Vec::new();
                    }
                    if self.detail_tab == DetailTab::Diff && self.diff.is_some() {
                        self.select_diff_row(
                            row,
                            mouse.column,
                            mouse.modifiers.contains(KeyModifiers::SHIFT),
                        );
                        self.diff_drag = true;
                    } else {
                        self.selected_link = self.hovered_link();
                        if let Some(url) = self.selected_link.and_then(|key| self.link_url(key)) {
                            return vec![GithubEffect::OpenUrl(url.to_owned())];
                        }
                        self.selected_row = self.detail_rows.get(row).and_then(|row| row.target);
                    }
                }
            }
            if matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left))
                && self.diff_drag
                && focus == Focus::Detail
            {
                if let Some(row) = viewport.hit_visual_row(area, mouse.column, mouse.row) {
                    self.select_diff_row(row, mouse.column, true);
                }
            }
            break;
        }
        self.compute(self.geometry.area);
        Vec::new()
    }
    pub(super) fn navigate_failure(&mut self, forward: bool) {
        let rows: Vec<_> = self
            .detail_rows
            .iter()
            .enumerate()
            .filter(|(_, row)| row.failure)
            .map(|(index, _)| index)
            .collect();
        let next = if forward {
            rows.iter()
                .copied()
                .find(|index| *index > self.detail_scroll)
                .or_else(|| rows.first().copied())
        } else {
            rows.iter()
                .rev()
                .copied()
                .find(|index| *index < self.detail_scroll)
                .or_else(|| rows.last().copied())
        };
        if let Some(index) = next {
            self.detail_scroll = index;
            self.selected_row = self.detail_rows[index].target;
        } else {
            self.notice = Some("No failures in this view.".into());
        }
    }
}
