mod commands;
mod conversation;
mod diff;
mod input;
mod merge;
mod navigation;
mod runs;
#[cfg(test)]
mod tests;
mod view;

use std::collections::{BTreeMap, VecDeque};

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::diff::{DiffMode, DiffRow, DiffSide, DiffViewState, ThreadLocation};
use super::{domain::*, GithubRepository, ResolvedGithubScope};
use crate::ui::ModalListViewport;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubTab {
    Overview,
    Repositories,
    PullRequests,
    Issues,
    Actions,
}
impl GithubTab {
    pub const ALL: [Self; 5] = [
        Self::Overview,
        Self::Repositories,
        Self::PullRequests,
        Self::Issues,
        Self::Actions,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Repositories => "Repositories",
            Self::PullRequests => "Pull requests",
            Self::Issues => "Issues",
            Self::Actions => "Actions",
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailTab {
    Description,
    Conversation,
    Diff,
    Checks,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunFilter {
    All,
    Failed,
    Running,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GithubAction {
    Tab(GithubTab),
    Queue(Queue),
    Runs(RunFilter),
    Refresh,
    More,
    Filter,
    ResetRepository,
    Back,
    Open,
    Browser,
    CopyUrl,
    Editor,
    Palette,
    CloseScreen,
    Detail(DetailTab),
    Comment,
    Reply,
    EditComment,
    DeleteComment,
    Labels,
    ToggleLabel(usize),
    ToggleDraft,
    CloseItem,
    ReviewComment,
    Approve,
    RequestChanges,
    Merge,
    MergeNow(usize),
    MergeAuto(usize),
    DisableAuto,
    Submit,
    Cancel,
    ToggleSplit,
    ToggleWrap,
    ToggleWhitespace,
    ToggleFiles,
    FindFile,
    NextThread,
    PreviousThread,
    InlineComment,
    NextFailure,
    PreviousFailure,
    PullRequestRuns,
    SelectFile(usize),
}
#[derive(Debug, Clone)]
pub enum GithubEffect {
    Close,
    OpenPalette,
    OpenUrl(String),
    Copy(String),
    OpenEditor,
}
#[derive(Debug, Clone)]
pub enum Entry {
    Heading(String),
    Item(Summary),
    Repository(GithubRepository),
    Run(GithubRepository, WorkflowRun),
}
impl Entry {
    pub fn label(&self) -> String {
        match self {
            Self::Heading(text) => text.clone(),
            Self::Item(item) => format!(
                "{} #{}  {}  [{}{}]",
                item.key.repository,
                item.key.number,
                item.title,
                item.state,
                if item.is_draft { ", draft" } else { "" }
            ),
            Self::Repository(repo) => repo.to_string(),
            Self::Run(repo, run) => format!(
                "{} #{}  {}  {}",
                repo,
                run.run_number,
                run.display_title,
                run.conclusion.as_deref().unwrap_or(&run.status)
            ),
        }
    }
    fn selectable(&self) -> bool {
        !matches!(self, Self::Heading(_))
    }
}
#[derive(Debug, Clone)]
pub enum Detail {
    Item(Box<ItemDetails>),
    Repository(RepositoryDetails),
    Run(RunDetails),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Detail,
    Controls,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowTarget {
    Comment { review: bool, index: usize },
    Job(usize),
    Check(usize),
}
#[derive(Debug, Clone)]
pub struct TextRow {
    pub text: String,
    pub target: Option<RowTarget>,
    pub failure: bool,
}
#[derive(Debug, Clone)]
pub struct Control {
    pub area: Rect,
    pub action: GithubAction,
    pub label: String,
}
#[derive(Debug, Clone, Copy)]
pub enum ListRow {
    Entry(usize),
    Metadata(usize),
    Gap,
}
impl ListRow {
    pub fn entry(self) -> Option<usize> {
        match self {
            Self::Entry(index) | Self::Metadata(index) => Some(index),
            Self::Gap => None,
        }
    }
}
#[derive(Debug, Clone, Default)]
pub struct Geometry {
    pub area: Rect,
    pub header: Rect,
    pub scope: Rect,
    pub list_rows: Vec<ListRow>,
    pub list: Rect,
    pub detail: Rect,
    pub status: Rect,
    pub controls: Vec<Control>,
    pub modal: Rect,
    pub input: Rect,
    pub files: Rect,
}
#[derive(Debug, Clone)]
pub struct TextBuffer {
    pub value: String,
    pub cursor: usize,
}
pub struct TextLayout {
    pub lines: Vec<std::ops::Range<usize>>,
    pub cursor_row: usize,
    pub cursor_column: usize,
}
impl TextBuffer {
    pub fn layout(&self, width: usize) -> TextLayout {
        let mut lines = Vec::new();
        let mut start = 0;
        let mut columns = 0;
        for (offset, character) in self.value.char_indices() {
            if character == '\n' {
                lines.push(start..offset);
                start = offset + 1;
                columns = 0;
            } else {
                let next = character.width().unwrap_or(0);
                if columns + next > width.max(1) && offset > start {
                    lines.push(start..offset);
                    start = offset;
                    columns = 0;
                }
                columns += next;
            }
        }
        lines.push(start..self.value.len());
        let cursor_row = lines
            .iter()
            .rposition(|line| line.start <= self.cursor)
            .unwrap_or(0);
        let cursor_column =
            UnicodeWidthStr::width(&self.value[lines[cursor_row].start..self.cursor]);
        TextLayout {
            lines,
            cursor_row,
            cursor_column,
        }
    }
    fn click(&mut self, area: Rect, column: u16, row: u16) {
        let layout = self.layout(area.width as usize);
        let top = layout
            .cursor_row
            .saturating_sub(area.height.saturating_sub(1) as usize);
        let target_row = top + row.saturating_sub(area.y) as usize;
        if let Some(line) = layout.lines.get(target_row) {
            let target_column = column.saturating_sub(area.x) as usize;
            let mut columns = 0;
            self.cursor = line.end;
            for (offset, character) in self.value[line.clone()].char_indices() {
                if columns + character.width().unwrap_or(0) > target_column {
                    self.cursor = line.start + offset;
                    break;
                }
                columns += character.width().unwrap_or(0);
            }
        }
    }
    fn new(value: String) -> Self {
        let cursor = value.len();
        Self { value, cursor }
    }
    fn insert(&mut self, text: &str) {
        self.value.insert_str(self.cursor, text);
        self.cursor += text.len();
    }
    fn key(&mut self, key: KeyEvent, multiline: bool) {
        match key.code {
            KeyCode::Char(c)
                if !key.modifiers.intersects(
                    KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                ) =>
            {
                self.insert(&c.to_string())
            }
            KeyCode::Enter if multiline => self.insert("\n"),
            KeyCode::Left => {
                self.cursor = self.value[..self.cursor]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(i, _)| i)
            }
            KeyCode::Right => {
                self.cursor += self.value[self.cursor..]
                    .chars()
                    .next()
                    .map_or(0, char::len_utf8)
            }
            KeyCode::Home => {
                self.cursor = self.value[..self.cursor].rfind('\n').map_or(0, |i| i + 1)
            }
            KeyCode::End => {
                self.cursor += self.value[self.cursor..]
                    .find('\n')
                    .unwrap_or(self.value.len() - self.cursor)
            }
            KeyCode::Backspace => {
                if let Some((start, _)) = self.value[..self.cursor].char_indices().next_back() {
                    self.value.drain(start..self.cursor);
                    self.cursor = start;
                }
            }
            KeyCode::Delete => {
                if let Some(c) = self.value[self.cursor..].chars().next() {
                    self.value.drain(self.cursor..self.cursor + c.len_utf8());
                }
            }
            KeyCode::Up | KeyCode::Down if multiline => {
                let start = self.value[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
                let column = self.value[start..self.cursor].chars().count();
                let target = if key.code == KeyCode::Up {
                    start
                        .checked_sub(1)
                        .map(|end| (self.value[..end].rfind('\n').map_or(0, |i| i + 1), end))
                } else {
                    self.value[self.cursor..].find('\n').map(|offset| {
                        let next = self.cursor + offset + 1;
                        (
                            next,
                            self.value[next..]
                                .find('\n')
                                .map_or(self.value.len(), |n| next + n),
                        )
                    })
                };
                if let Some((start, end)) = target {
                    self.cursor = self.value[start..end]
                        .char_indices()
                        .nth(column)
                        .map_or(end, |(offset, _)| start + offset);
                }
            }
            _ => {}
        }
    }
}
#[derive(Debug, Clone)]
pub enum ComposerKind {
    Comment,
    Reply(u64),
    Edit(CommentKind, u64),
    Review(ReviewEvent),
    Inline(InlineComment),
}
#[derive(Debug, Clone)]
pub enum Dialog {
    Filter(TextBuffer),
    FileSearch(TextBuffer),
    Composer {
        title: String,
        item: ItemKey,
        kind: ComposerKind,
        text: TextBuffer,
    },
    Confirm {
        title: String,
        description: String,
        mutation: GithubMutation,
    },
    Labels {
        selected: usize,
    },
    Merge,
}
#[derive(Debug, Clone)]
struct Tracked {
    generation: u64,
    request: GithubRequest,
}
#[derive(Debug, Clone)]
pub struct GithubScreen {
    pub scope: ResolvedGithubScope,
    pub tab: GithubTab,
    pub repository: Option<GithubRepository>,
    pub queue: Queue,
    pub run_filter: RunFilter,
    pub filter: String,
    pub entries: Vec<Entry>,
    visible: Vec<usize>,
    entries_dirty: bool,
    pub selected: usize,
    pub list_scroll: usize,
    pub detail_scroll: usize,
    pub detail: Option<Detail>,
    pub detail_tab: DetailTab,
    pub detail_rows: Vec<TextRow>,
    pub selected_row: Option<RowTarget>,
    pub focus: Focus,
    pub control_focus: usize,
    pub geometry: Geometry,
    pub dialog: Option<Dialog>,
    pub error: Option<String>,
    pub notice: Option<String>,
    pub viewer: Option<Viewer>,
    pub diff: Option<DiffViewState>,
    pub show_files: bool,
    pub file_filter: String,
    pub file_scroll: usize,
    pub diff_drag: bool,
    pub labels: Vec<Label>,
    pub merge_repository: Option<RepositoryDetails>,
    pub submitting: bool,
    force_refresh: bool,
    refresh_generation: bool,
    generation: u64,
    pending: BTreeMap<u64, Tracked>,
    queued: VecDeque<GithubRequest>,
    awaiting_ids: VecDeque<Tracked>,
    obsolete: Vec<u64>,
    next_cursor: Option<String>,
    overview_cursors: Vec<(Queue, ItemKind, Option<String>)>,
    runs_backlog: VecDeque<(GithubRepository, Option<String>)>,
    runs_cursors: Vec<(GithubRepository, String)>,
    catalog_cursor: Option<String>,
    selected_key: Option<ItemKey>,
    selected_run: Option<(GithubRepository, u64)>,
    run_sha: Option<String>,
    rows_dirty: bool,
    rows_width: u16,
    scrollbar_drag: Option<(Focus, u16)>,
    file_scrollbar_drag: Option<u16>,
}
impl GithubScreen {
    pub fn new(scope: ResolvedGithubScope) -> Self {
        let mut screen = Self {
            scope,
            tab: GithubTab::Overview,
            repository: None,
            queue: Queue::Authored,
            run_filter: RunFilter::All,
            filter: String::new(),
            entries: Vec::new(),
            selected: 0,
            visible: Vec::new(),
            entries_dirty: true,
            list_scroll: 0,
            detail_scroll: 0,
            detail: None,
            detail_tab: DetailTab::Description,
            detail_rows: Vec::new(),
            selected_row: None,
            focus: Focus::List,
            control_focus: 0,
            geometry: Geometry::default(),
            dialog: None,
            error: None,
            notice: None,
            viewer: None,
            diff: None,
            show_files: true,
            file_filter: String::new(),
            file_scroll: 0,
            diff_drag: false,
            labels: Vec::new(),
            merge_repository: None,
            submitting: false,
            generation: 0,
            force_refresh: false,
            refresh_generation: false,
            pending: BTreeMap::new(),
            queued: VecDeque::new(),
            awaiting_ids: VecDeque::new(),
            obsolete: Vec::new(),
            next_cursor: None,
            overview_cursors: Vec::new(),
            selected_key: None,
            selected_run: None,
            runs_backlog: VecDeque::new(),
            runs_cursors: Vec::new(),
            catalog_cursor: None,
            run_sha: None,
            rows_dirty: true,
            rows_width: 0,
            scrollbar_drag: None,
            file_scrollbar_drag: None,
        };
        screen.refresh();
        screen
    }
    fn enqueue(&mut self, request: GithubRequest) {
        self.force_refresh |= self.refresh_generation;
        self.queued.push_back(request);
    }
    pub fn take_force_refresh(&mut self) -> bool {
        std::mem::take(&mut self.force_refresh)
    }
    pub fn drain_requests(&mut self) -> Vec<GithubRequest> {
        let requests: Vec<_> = self.queued.drain(..).collect();
        self.awaiting_ids
            .extend(requests.iter().cloned().map(|request| Tracked {
                generation: self.generation,
                request,
            }));
        requests
    }
    pub fn track_request(&mut self, id: u64) {
        if let Some(tracked) = self.awaiting_ids.pop_front() {
            self.pending.insert(id, tracked);
        } else {
            self.obsolete.push(id);
        }
    }
    pub fn pending_requests(&self) -> Vec<u64> {
        self.pending.keys().copied().collect()
    }
    pub fn cancel_requests(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.obsolete)
    }
    pub fn loading(&self) -> bool {
        !self.pending.is_empty() || !self.queued.is_empty() || !self.awaiting_ids.is_empty()
    }
}

fn is_failure(conclusion: &str) -> bool {
    matches!(
        conclusion,
        "failure" | "timed_out" | "cancelled" | "action_required" | "startup_failure" | "stale"
    )
}

impl GithubScreen {
    pub fn item(&self) -> Option<&ItemDetails> {
        match &self.detail {
            Some(Detail::Item(item)) => Some(item),
            _ => None,
        }
    }
}

fn queue_label(queue: Queue) -> &'static str {
    match queue {
        Queue::Authored => "Authored",
        Queue::ReviewRequested => "Review requested",
        Queue::Assigned => "Assigned",
        Queue::Mentioned => "Mentioned",
        Queue::All => "All",
    }
}

fn contains(area: Rect, (x, y): (u16, u16)) -> bool {
    area.width > 0
        && area.height > 0
        && x >= area.x
        && x < area.right()
        && y >= area.y
        && y < area.bottom()
}

fn place_controls(
    controls: &mut Vec<Control>,
    area: Rect,
    actions: &[(GithubAction, String)],
) -> u16 {
    if area.is_empty() {
        return area.y;
    }
    let mut x = area.x;
    let mut y = area.y;
    for (action, label) in actions {
        let width = (UnicodeWidthStr::width(label.as_str())
            .saturating_add(2)
            .min(u16::MAX as usize) as u16)
            .min(area.width);
        if x.saturating_add(width) > area.right() {
            x = area.x;
            y = y.saturating_add(1);
        }
        if y >= area.bottom() {
            break;
        }
        controls.push(Control {
            area: Rect::new(x, y, width, 1),
            action: *action,
            label: label.clone(),
        });
        x = x.saturating_add(width).saturating_add(1);
    }
    y.saturating_add(1).min(area.bottom())
}
fn inset(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(u16::from(area.width > 0)),
        area.y.saturating_add(u16::from(area.height > 0)),
        area.width.saturating_sub(2),
        area.height.saturating_sub(2),
    )
}
fn append_rows(
    rows: &mut Vec<TextRow>,
    text: &str,
    target: Option<RowTarget>,
    failure: bool,
    width: usize,
) {
    for line in text.split('\n') {
        let mut chunk = String::new();
        let mut columns = 0;
        for c in line.chars() {
            let c = if c == '\t' {
                ' '
            } else if c.is_control() {
                continue;
            } else {
                c
            };
            let next = c.width().unwrap_or(0);
            if columns + next > width && !chunk.is_empty() {
                rows.push(TextRow {
                    text: std::mem::take(&mut chunk),
                    target,
                    failure,
                });
                columns = 0;
            }
            chunk.push(c);
            columns += next;
        }
        rows.push(TextRow {
            text: chunk,
            target,
            failure,
        });
    }
}
fn duration(start: Option<&str>, end: Option<&str>) -> String {
    let Some(start) = start.and_then(timestamp_seconds) else {
        return "not started".into();
    };
    let end = end.and_then(timestamp_seconds).or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|duration| duration.as_secs() as i64)
    });
    let Some(end) = end else {
        return "duration unavailable".into();
    };
    let seconds = end.saturating_sub(start).max(0);
    format!("{}m {}s", seconds / 60, seconds % 60)
}
fn timestamp_seconds(value: &str) -> Option<i64> {
    let year: i64 = value.get(0..4)?.parse().ok()?;
    let month: i64 = value.get(5..7)?.parse().ok()?;
    let day: i64 = value.get(8..10)?.parse().ok()?;
    let hour: i64 = value.get(11..13)?.parse().ok()?;
    let minute: i64 = value.get(14..16)?.parse().ok()?;
    let second: i64 = value.get(17..19)?.parse().ok()?;
    if year < 1970 || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = year - i64::from(month <= 2);
    let era = year / 400;
    let yoe = year - era * 400;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let days = era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468;
    Some(days * 86400 + hour * 3600 + minute * 60 + second)
}
