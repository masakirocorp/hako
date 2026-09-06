use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffFile {
    pub path: String,
    pub previous_path: Option<String>,
    pub status: String,
    pub patch: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiffSide {
    Left,
    Right,
}

impl DiffSide {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Left => "LEFT",
            Self::Right => "RIGHT",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffLineKind {
    Context,
    Added,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub text: String,
    pub kind: DiffLineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub no_newline: bool,
}

impl DiffLine {
    pub fn line_on(&self, side: DiffSide) -> Option<u32> {
        match side {
            DiffSide::Left => self.old_line,
            DiffSide::Right => self.new_line,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchState {
    Available,
    Unavailable,
    Binary,
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDiffFile {
    pub file: DiffFile,
    pub hunks: Vec<DiffHunk>,
    pub state: PatchState,
}

impl ParsedDiffFile {
    pub fn parse(file: DiffFile) -> Self {
        let (hunks, state) = match file.patch.as_deref() {
            None | Some("") => (Vec::new(), PatchState::Unavailable),
            Some(patch)
                if patch.lines().any(|line| {
                    line.starts_with("Binary files ") || line == "GIT binary patch"
                }) =>
            {
                (Vec::new(), PatchState::Binary)
            }
            Some(patch) => match parse_hunks(patch) {
                Ok(hunks) => (hunks, PatchState::Available),
                Err(error) => (Vec::new(), PatchState::Invalid(error)),
            },
        };
        Self { file, hunks, state }
    }

    pub fn hunk_for(&self, side: DiffSide, line: u32) -> Option<usize> {
        self.hunks.iter().position(|hunk| {
            hunk.lines
                .iter()
                .any(|source| source.line_on(side) == Some(line))
        })
    }
}

fn parse_range(value: &str, prefix: char) -> Option<(u32, u32)> {
    let value = value.strip_prefix(prefix)?;
    let (start, count) = match value.split_once(',') {
        Some((start, count)) => (start.parse::<u32>().ok()?, count.parse::<u32>().ok()?),
        None => (value.parse::<u32>().ok()?, 1),
    };
    if count > 0 && start == 0 {
        return None;
    }
    start.checked_add(count)?;
    Some((start, count))
}

fn parse_hunks(patch: &str) -> Result<Vec<DiffHunk>, String> {
    let mut hunks = Vec::new();
    let mut input = patch.split_terminator('\n').peekable();
    let mut previous_old_end = 0;
    let mut previous_new_end = 0;
    while let Some(header) = input.next() {
        let mut parts = header.split_whitespace();
        let parsed = (|| {
            if parts.next()? != "@@" {
                return None;
            }
            let old = parse_range(parts.next()?, '-')?;
            let new = parse_range(parts.next()?, '+')?;
            if parts.next()? != "@@" {
                return None;
            }
            Some((old, new))
        })();
        let Some(((mut old, old_count), (mut new, new_count))) = parsed else {
            return Err("Invalid patch hunk header".into());
        };
        if old < previous_old_end || new < previous_new_end {
            return Err("Overlapping patch hunks".into());
        }
        let old_end = old + old_count;
        let new_end = new + new_count;
        let mut lines: Vec<DiffLine> = Vec::new();
        while let Some(&raw) = input.peek() {
            if raw.starts_with("@@") {
                break;
            }
            input.next();
            if raw == "\\ No newline at end of file" {
                let Some(last) = lines.last_mut() else {
                    return Err("Newline marker has no source line".into());
                };
                last.no_newline = true;
                continue;
            }
            let (kind, old_line, new_line) = match raw.as_bytes().first() {
                Some(b' ') if old < old_end && new < new_end => {
                    let coordinates = (DiffLineKind::Context, Some(old), Some(new));
                    old += 1;
                    new += 1;
                    coordinates
                }
                Some(b'-') if old < old_end => {
                    let coordinates = (DiffLineKind::Deleted, Some(old), None);
                    old += 1;
                    coordinates
                }
                Some(b'+') if new < new_end => {
                    let coordinates = (DiffLineKind::Added, None, Some(new));
                    new += 1;
                    coordinates
                }
                _ => return Err("Patch line does not match its hunk ranges".into()),
            };
            lines.push(DiffLine {
                text: raw[1..].to_string(),
                kind,
                old_line,
                new_line,
                no_newline: false,
            });
        }
        if old != old_end || new != new_end {
            return Err("Patch hunk is incomplete".into());
        }
        previous_old_end = old_end;
        previous_new_end = new_end;
        hunks.push(DiffHunk {
            header: header.to_string(),
            lines,
        });
    }
    if hunks.is_empty() {
        return Err("Patch has no hunks".into());
    }
    Ok(hunks)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffMode {
    Unified,
    Split,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffOptions {
    pub mode: DiffMode,
    pub wrap: bool,
    pub ignore_whitespace: bool,
    /// Content columns per cell, excluding gutters and borders.
    pub width: usize,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self {
            mode: DiffMode::Unified,
            wrap: false,
            ignore_whitespace: false,
            width: 80,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffCell {
    pub text: String,
    pub kind: DiffLineKind,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
    pub continuation: bool,
    pub hunk: usize,
    pub no_newline: bool,
}

impl DiffCell {
    pub fn line_on(&self, side: DiffSide) -> Option<u32> {
        match side {
            DiffSide::Left => self.old_line,
            DiffSide::Right => self.new_line,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffRow {
    Hunk {
        index: usize,
        header: String,
    },
    Line {
        left: Option<DiffCell>,
        right: Option<DiffCell>,
    },
    Notice(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiffLayout {
    pub rows: Vec<DiffRow>,
}

impl DiffLayout {
    pub fn build(file: &ParsedDiffFile, options: DiffOptions) -> Self {
        let mut layout = Self::default();
        let notice = match &file.state {
            PatchState::Available => None,
            PatchState::Unavailable => Some(
                "Patch unavailable. The file may be binary, unchanged, or too large.".to_string(),
            ),
            PatchState::Binary => Some("Binary file".to_string()),
            PatchState::Invalid(error) => Some(format!("Cannot display patch: {error}")),
        };
        if let Some(notice) = notice {
            layout.rows.push(DiffRow::Notice(notice));
        }
        for (hunk_index, hunk) in file.hunks.iter().enumerate() {
            layout.rows.push(DiffRow::Hunk {
                index: hunk_index,
                header: hunk.header.clone(),
            });
            let mut index = 0;
            while index < hunk.lines.len() {
                let line = &hunk.lines[index];
                if line.kind == DiffLineKind::Context {
                    layout.push_lines(Some(line), Some(line), hunk_index, options);
                    index += 1;
                    continue;
                }
                let start = index;
                while index < hunk.lines.len() && hunk.lines[index].kind != DiffLineKind::Context {
                    index += 1;
                }
                let changes = &hunk.lines[start..index];
                let deleted: Vec<_> = changes
                    .iter()
                    .filter(|line| line.kind == DiffLineKind::Deleted)
                    .collect();
                let added: Vec<_> = changes
                    .iter()
                    .filter(|line| line.kind == DiffLineKind::Added)
                    .collect();
                if options.mode == DiffMode::Split || options.ignore_whitespace {
                    for offset in 0..deleted.len().max(added.len()) {
                        let old = deleted.get(offset).copied();
                        let new = added.get(offset).copied();
                        if let (Some(old), Some(new)) = (old, new) {
                            if options.ignore_whitespace
                                && old.no_newline == new.no_newline
                                && old
                                    .text
                                    .chars()
                                    .filter(|ch| !ch.is_whitespace())
                                    .eq(new.text.chars().filter(|ch| !ch.is_whitespace()))
                            {
                                let context = DiffLine {
                                    text: new.text.clone(),
                                    kind: DiffLineKind::Context,
                                    old_line: old.old_line,
                                    new_line: new.new_line,
                                    no_newline: new.no_newline,
                                };
                                layout.push_lines(
                                    Some(&context),
                                    Some(&context),
                                    hunk_index,
                                    options,
                                );
                                continue;
                            }
                        }
                        if options.mode == DiffMode::Split {
                            layout.push_lines(old, new, hunk_index, options);
                        } else {
                            layout.push_lines(old, None, hunk_index, options);
                            layout.push_lines(None, new, hunk_index, options);
                        }
                    }
                } else {
                    for change in changes {
                        match change.kind {
                            DiffLineKind::Deleted => {
                                layout.push_lines(Some(change), None, hunk_index, options)
                            }
                            DiffLineKind::Added => {
                                layout.push_lines(None, Some(change), hunk_index, options)
                            }
                            DiffLineKind::Context => {
                                layout.push_lines(Some(change), Some(change), hunk_index, options)
                            }
                        }
                    }
                }
            }
        }
        layout
    }

    fn push_lines(
        &mut self,
        left: Option<&DiffLine>,
        right: Option<&DiffLine>,
        hunk: usize,
        options: DiffOptions,
    ) {
        if options.mode == DiffMode::Unified {
            let source = right.or(left);
            let cells = source
                .map(|line| wrap_cells(line, hunk, options))
                .unwrap_or_default();
            for cell in cells {
                if right.is_some() {
                    self.rows.push(DiffRow::Line {
                        left: None,
                        right: Some(cell),
                    });
                } else {
                    self.rows.push(DiffRow::Line {
                        left: Some(cell),
                        right: None,
                    });
                }
            }
            return;
        }
        let mut left = left
            .map(|line| wrap_cells(line, hunk, options))
            .unwrap_or_default()
            .into_iter();
        let mut right = right
            .map(|line| wrap_cells(line, hunk, options))
            .unwrap_or_default()
            .into_iter();
        loop {
            let left = left.next();
            let right = right.next();
            if left.is_none() && right.is_none() {
                break;
            }
            self.rows.push(DiffRow::Line { left, right });
        }
    }

    pub fn row_for(&self, side: DiffSide, line: u32) -> Option<usize> {
        self.rows.iter().position(|row| match row {
            DiffRow::Line { left, right } => left
                .iter()
                .chain(right.iter())
                .any(|cell| cell.line_on(side) == Some(line)),
            DiffRow::Hunk { .. } | DiffRow::Notice(_) => false,
        })
    }
}

fn wrap_cells(line: &DiffLine, hunk: usize, options: DiffOptions) -> Vec<DiffCell> {
    let width = options.width.max(1);
    let mut chunks = Vec::new();
    let mut text = String::new();
    let mut columns = 0;
    let mut source_column = 0;
    for character in line.text.chars() {
        let (character, repeat) = if character == '\t' {
            (' ', 4 - source_column % 4)
        } else if character.is_control() {
            ('\u{fffd}', 1)
        } else {
            (character, 1)
        };
        let character_width = character.width().unwrap_or(0);
        for _ in 0..repeat {
            if options.wrap && columns > 0 && columns + character_width > width {
                chunks.push(std::mem::take(&mut text));
                columns = 0;
            }
            text.push(character);
            columns += character_width;
            source_column += character_width;
        }
    }
    chunks.push(text);
    chunks
        .into_iter()
        .enumerate()
        .map(|(index, text)| DiffCell {
            text,
            kind: line.kind,
            old_line: line.old_line,
            new_line: line.new_line,
            continuation: index > 0,
            hunk,
            no_newline: line.no_newline,
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadLocation {
    pub path: String,
    pub line: u32,
    pub side: DiffSide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DiffLocation {
    pub file: usize,
    pub hunk: usize,
    pub side: DiffSide,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffSelection {
    pub path: String,
    pub side: DiffSide,
    pub start_line: u32,
    pub end_line: u32,
    pub hunk: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionError {
    MissingFile,
    MissingLine,
    DifferentSide,
    DifferentHunk,
}

#[derive(Debug, Clone)]
pub struct DiffViewState {
    files: Vec<ParsedDiffFile>,
    selected_file: Option<usize>,
    cursor: Option<DiffLocation>,
    anchor: Option<DiffLocation>,
    options: DiffOptions,
    layout: DiffLayout,
}

impl DiffViewState {
    pub fn new(files: Vec<DiffFile>) -> Self {
        let files: Vec<_> = files.into_iter().map(ParsedDiffFile::parse).collect();
        let options = DiffOptions::default();
        let selected_file = if files.is_empty() { None } else { Some(0) };
        let layout = files
            .first()
            .map(|file| DiffLayout::build(file, options))
            .unwrap_or_default();
        Self {
            files,
            selected_file,
            cursor: None,
            anchor: None,
            options,
            layout,
        }
    }

    pub fn files(&self) -> &[ParsedDiffFile] {
        &self.files
    }
    pub fn selected_file(&self) -> Option<usize> {
        self.selected_file
    }
    pub fn cursor(&self) -> Option<DiffLocation> {
        self.cursor
    }
    pub fn options(&self) -> DiffOptions {
        self.options
    }
    pub fn layout(&self) -> &DiffLayout {
        &self.layout
    }

    pub fn matching_files(&self, query: &str) -> Vec<usize> {
        let query = query.to_lowercase();
        self.files
            .iter()
            .enumerate()
            .filter_map(|(index, file)| {
                (file.file.path.to_lowercase().contains(&query)
                    || file
                        .file
                        .previous_path
                        .as_ref()
                        .is_some_and(|path| path.to_lowercase().contains(&query)))
                .then_some(index)
            })
            .collect()
    }

    pub fn select_file(&mut self, index: usize) -> Result<(), SelectionError> {
        let file = self.files.get(index).ok_or(SelectionError::MissingFile)?;
        if self.selected_file != Some(index) {
            self.layout = DiffLayout::build(file, self.options);
            self.selected_file = Some(index);
            self.cursor = None;
            self.anchor = None;
        }
        Ok(())
    }

    /// Call during update/compute, never from rendering.
    pub fn set_options(&mut self, options: DiffOptions) {
        if self.options != options {
            self.options = options;
            self.layout = self
                .selected_file
                .and_then(|index| self.files.get(index))
                .map(|file| DiffLayout::build(file, options))
                .unwrap_or_default();
        }
    }

    pub fn select(
        &mut self,
        side: DiffSide,
        line: u32,
        extend: bool,
    ) -> Result<(), SelectionError> {
        let file = self.selected_file.ok_or(SelectionError::MissingFile)?;
        let hunk = self.files[file]
            .hunk_for(side, line)
            .ok_or(SelectionError::MissingLine)?;
        let location = DiffLocation {
            file,
            hunk,
            side,
            line,
        };
        if extend {
            let anchor = self.anchor.or(self.cursor).unwrap_or(location);
            if anchor.file != file {
                return Err(SelectionError::MissingFile);
            }
            if anchor.side != side {
                return Err(SelectionError::DifferentSide);
            }
            if anchor.hunk != hunk {
                return Err(SelectionError::DifferentHunk);
            }
            self.anchor = Some(anchor);
        } else {
            self.anchor = None;
        }
        self.cursor = Some(location);
        Ok(())
    }

    pub fn selection(&self) -> Option<DiffSelection> {
        let cursor = self.cursor?;
        let anchor = self.anchor.unwrap_or(cursor);
        Some(DiffSelection {
            path: self.files[cursor.file].file.path.clone(),
            side: cursor.side,
            start_line: anchor.line.min(cursor.line),
            end_line: anchor.line.max(cursor.line),
            hunk: cursor.hunk,
        })
    }

    pub fn locate_thread(&self, thread: &ThreadLocation) -> Option<DiffLocation> {
        let file = self
            .files
            .iter()
            .position(|file| file.file.path == thread.path)
            .or_else(|| {
                self.files.iter().position(|file| {
                    thread.side == DiffSide::Left
                        && file.file.previous_path.as_deref() == Some(thread.path.as_str())
                })
            })?;
        let hunk = self.files[file].hunk_for(thread.side, thread.line)?;
        Some(DiffLocation {
            file,
            hunk,
            side: thread.side,
            line: thread.line,
        })
    }

    /// Returns the caller's thread index; missing and outdated coordinates are skipped.
    pub fn navigate_thread(&mut self, threads: &[ThreadLocation], forward: bool) -> Option<usize> {
        let mut located: Vec<_> = threads
            .iter()
            .enumerate()
            .filter_map(|(index, thread)| {
                self.locate_thread(thread).map(|location| (location, index))
            })
            .collect();
        located.sort_by_key(|(location, index)| {
            (
                location.file,
                location.hunk,
                location.line,
                location.side,
                *index,
            )
        });
        let key =
            |location: DiffLocation| (location.file, location.hunk, location.line, location.side);
        let selected = if forward {
            located
                .iter()
                .find(|(location, _)| {
                    self.cursor
                        .is_none_or(|cursor| key(*location) > key(cursor))
                })
                .or_else(|| located.first())
        } else {
            located
                .iter()
                .rev()
                .find(|(location, _)| {
                    self.cursor
                        .is_none_or(|cursor| key(*location) < key(cursor))
                })
                .or_else(|| located.last())
        };
        let &(location, index) = selected?;
        self.select_file(location.file).ok()?;
        self.select(location.side, location.line, false).ok()?;
        Some(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, patch: &str) -> DiffFile {
        DiffFile {
            path: path.into(),
            previous_path: None,
            status: "modified".into(),
            patch: Some(patch.into()),
        }
    }

    #[test]
    fn deleted_lines_remain_left_side_comment_targets_in_both_modes() {
        let mut view = DiffViewState::new(vec![file(
            "code.rs",
            "@@ -10,2 +20,1 @@\n-deleted\n context",
        )]);
        for mode in [DiffMode::Unified, DiffMode::Split] {
            view.set_options(DiffOptions {
                mode,
                ..DiffOptions::default()
            });
            assert!(view.layout().row_for(DiffSide::Left, 10).is_some());
            assert_eq!(view.layout().row_for(DiffSide::Right, 10), None);
            assert_eq!(view.select(DiffSide::Left, 10, false), Ok(()));
            assert_eq!(
                view.selection(),
                Some(DiffSelection {
                    path: "code.rs".into(),
                    side: DiffSide::Left,
                    start_line: 10,
                    end_line: 10,
                    hunk: 0,
                })
            );
            assert_eq!(
                view.select(DiffSide::Right, 10, false),
                Err(SelectionError::MissingLine)
            );
        }
    }

    #[test]
    fn range_extension_rejects_other_sides_and_hunks_without_changing_selection() {
        let patch = "@@ -1,2 +1,2 @@\n first\n second\n@@ -9 +9 @@\n later";
        let mut view = DiffViewState::new(vec![file("a", patch), file("b", patch)]);
        assert_eq!(view.select(DiffSide::Right, 2, false), Ok(()));
        assert_eq!(view.select(DiffSide::Right, 1, true), Ok(()));
        let range = view.selection();
        assert_eq!(
            range
                .as_ref()
                .map(|range| (range.start_line, range.end_line)),
            Some((1, 2))
        );
        assert_eq!(
            view.select(DiffSide::Left, 1, true),
            Err(SelectionError::DifferentSide)
        );
        assert_eq!(
            view.select(DiffSide::Right, 9, true),
            Err(SelectionError::DifferentHunk)
        );
        assert_eq!(view.selection(), range);
        assert_eq!(view.select_file(1), Ok(()));
        assert_eq!(view.selection(), None);
        assert_eq!(view.select(DiffSide::Right, 9, true), Ok(()));
        assert_eq!(
            view.selection()
                .map(|range| (range.path, range.start_line, range.end_line)),
            Some(("b".into(), 9, 9))
        );
    }

    #[test]
    fn wrapped_unicode_and_tabs_keep_source_coordinates() {
        let mut view = DiffViewState::new(vec![file("a", "@@ -7 +9 @@\n-界\tab\n+replacement")]);
        view.set_options(DiffOptions {
            mode: DiffMode::Split,
            wrap: true,
            width: 2,
            ..DiffOptions::default()
        });
        let left: Vec<_> = view
            .layout()
            .rows
            .iter()
            .filter_map(|row| match row {
                DiffRow::Line { left, .. } => left.as_ref(),
                _ => None,
            })
            .collect();
        assert_eq!(
            left.iter()
                .map(|cell| cell.text.as_str())
                .collect::<Vec<_>>(),
            vec!["界", "  ", "ab"]
        );
        assert!(left
            .iter()
            .all(|cell| cell.old_line == Some(7) && cell.new_line.is_none()));
        assert_eq!(
            left.iter()
                .map(|cell| cell.continuation)
                .collect::<Vec<_>>(),
            vec![false, true, true]
        );
        assert_eq!(view.select(DiffSide::Left, 7, false), Ok(()));
        assert_eq!(view.selection().map(|range| range.start_line), Some(7));
    }

    #[test]
    fn ignored_whitespace_preserves_both_coordinates_and_following_lines() {
        let mut view = DiffViewState::new(vec![file("a", "@@ -5,2 +8,2 @@\n- a b\n+ab\n next")]);
        view.set_options(DiffOptions {
            ignore_whitespace: true,
            wrap: true,
            width: 1,
            ..DiffOptions::default()
        });
        let old_row = view.layout().row_for(DiffSide::Left, 5);
        assert_eq!(old_row, view.layout().row_for(DiffSide::Right, 8));
        assert!(old_row.is_some());
        assert!(view.layout().row_for(DiffSide::Left, 6).is_some());
        assert!(view.layout().row_for(DiffSide::Right, 9).is_some());
        assert_eq!(view.select(DiffSide::Left, 5, false), Ok(()));
        assert_eq!(view.select(DiffSide::Left, 6, true), Ok(()));
        assert_eq!(
            view.selection()
                .map(|range| (range.start_line, range.end_line)),
            Some((5, 6))
        );
    }

    #[test]
    fn invalid_and_binary_patches_cannot_be_selected() {
        let mut unavailable = file("missing", "");
        unavailable.patch = None;
        let mut view = DiffViewState::new(vec![
            unavailable,
            file("binary", "Binary files a/image and b/image differ"),
            file("truncated", "@@ -1,2 +1,2 @@\n only one"),
            file("overflow", "@@ -4294967295 +1 @@\n-old\n+new"),
        ]);
        for index in 0..view.files().len() {
            assert_eq!(view.select_file(index), Ok(()));
            assert_eq!(
                view.select(DiffSide::Right, 1, false),
                Err(SelectionError::MissingLine)
            );
        }
        assert_eq!(view.files()[0].state, PatchState::Unavailable);
        assert_eq!(view.files()[1].state, PatchState::Binary);
        assert!(matches!(view.files()[2].state, PatchState::Invalid(_)));
        assert!(matches!(view.files()[3].state, PatchState::Invalid(_)));
    }

    #[test]
    fn thread_navigation_handles_renames_deleted_lines_and_missing_locations() {
        let mut renamed = file("new.rs", "@@ -4 +4 @@\n-old\n+new");
        renamed.previous_path = Some("old.rs".into());
        renamed.status = "renamed".into();
        let mut view = DiffViewState::new(vec![renamed, file("second.rs", "@@ -1 +1 @@\n same")]);
        let threads = vec![
            ThreadLocation {
                path: "absent".into(),
                side: DiffSide::Right,
                line: 1,
            },
            ThreadLocation {
                path: "old.rs".into(),
                side: DiffSide::Left,
                line: 4,
            },
            ThreadLocation {
                path: "second.rs".into(),
                side: DiffSide::Right,
                line: 1,
            },
        ];
        assert_eq!(view.matching_files("OLD"), vec![0]);
        assert_eq!(view.navigate_thread(&threads, true), Some(1));
        assert_eq!(
            view.selection()
                .map(|range| (range.path, range.side, range.start_line)),
            Some(("new.rs".into(), DiffSide::Left, 4))
        );
        assert_eq!(view.navigate_thread(&threads, true), Some(2));
        assert_eq!(view.navigate_thread(&threads, true), Some(1));
        assert_eq!(view.navigate_thread(&threads, false), Some(2));
    }

    #[test]
    fn ignoring_whitespace_keeps_substantive_changes_in_mixed_blocks() {
        let parsed = ParsedDiffFile::parse(file("a", "@@ -1,2 +1,2 @@\n- a b\n-old\n+ab\n+new"));
        let layout = DiffLayout::build(
            &parsed,
            DiffOptions {
                ignore_whitespace: true,
                ..DiffOptions::default()
            },
        );
        assert_eq!(
            layout.row_for(DiffSide::Left, 1),
            layout.row_for(DiffSide::Right, 1)
        );
        assert_ne!(
            layout.row_for(DiffSide::Left, 2),
            layout.row_for(DiffSide::Right, 2)
        );
        assert!(layout.rows.iter().any(|row| matches!(row,
            DiffRow::Line { right: Some(cell), .. }
                if cell.text == "new" && cell.kind == DiffLineKind::Added && cell.new_line == Some(2)
        )));
    }

    #[test]
    fn zero_count_hunks_and_newline_markers_do_not_invent_source_lines() {
        let parsed = ParsedDiffFile::parse(file(
            "a",
            "@@ -0,0 +1 @@\n+new\n\\ No newline at end of file",
        ));
        assert_eq!(parsed.state, PatchState::Available);
        assert_eq!(parsed.hunk_for(DiffSide::Left, 0), None);
        assert_eq!(parsed.hunk_for(DiffSide::Right, 1), Some(0));
        let layout = DiffLayout::build(&parsed, DiffOptions::default());
        assert!(layout.rows.iter().any(|row| matches!(row, DiffRow::Line { right: Some(cell), .. } if cell.no_newline && cell.new_line == Some(1))));
    }
}
