use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use super::{
    scrollbar::render_scrollbar,
    widgets::{
        fill_rect, panel_contrast_fg, render_action_button, render_modal_header_bar,
        secondary_action_style, ModalListViewport,
    },
};
use crate::{
    app::state::Palette,
    github::{
        diff::{DiffCell, DiffLineKind, DiffMode, DiffRow, DiffSide},
        screen::{
            DetailTab, Dialog, Entry, Focus, GithubAction, GithubScreen, ListRow, TextBuffer,
        },
    },
};

pub fn render(screen: &GithubScreen, palette: &Palette, frame: &mut Frame) {
    let geometry = &screen.geometry;
    if geometry.area.is_empty() {
        return;
    }
    fill_rect(
        frame,
        geometry.area,
        Style::default().bg(palette.panel_bg).fg(palette.text),
    );
    let scope = screen
        .repository
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            screen.scope.organization.as_ref().map_or_else(
                || match screen.scope.repositories.as_slice() {
                    [] => "Personal queues".into(),
                    [repository] => repository.to_string(),
                    repositories => format!("{} repositories", repositories.len()),
                },
                |organization| format!("org {}", organization.as_str()),
            )
        });
    let title = if geometry.scope.is_empty() {
        format!("GitHub · {scope}")
    } else {
        "GitHub".into()
    };
    render_modal_header_bar(frame, geometry.header, &title, palette, false);
    frame.render_widget(
        Paragraph::new(scope).style(Style::default().fg(palette.subtext0)),
        geometry.scope,
    );
    render_list(screen, palette, frame);
    render_detail(screen, palette, frame);
    render_files(screen, palette, frame);
    if screen.dialog.is_some() {
        render_dialog(screen, palette, frame);
    }
    for (index, control) in geometry.controls.iter().enumerate() {
        let active = match control.action {
            GithubAction::Tab(tab) => screen.tab == tab,
            GithubAction::Queue(queue) => screen.queue == queue,
            GithubAction::Runs(filter) => screen.run_filter == filter,
            GithubAction::Detail(tab) => screen.detail_tab == tab,
            _ => false,
        };
        let focused = screen.focus == Focus::Controls && screen.control_focus == index;
        let danger = matches!(
            control.action,
            GithubAction::DeleteComment | GithubAction::CloseItem
        ) || control.action == GithubAction::Submit
            && matches!(screen.dialog, Some(Dialog::Confirm { .. }));
        let style = if screen.submitting {
            Style::default()
                .fg(palette.overlay0)
                .bg(palette.surface_dim)
        } else if focused {
            Style::default()
                .fg(panel_contrast_fg(palette))
                .bg(palette.accent)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else if danger {
            Style::default().fg(palette.red).bg(palette.surface0)
        } else if active {
            Style::default()
                .fg(panel_contrast_fg(palette))
                .bg(palette.accent)
                .add_modifier(Modifier::BOLD)
        } else if matches!(control.action, GithubAction::Tab(_)) {
            Style::default().fg(palette.overlay1)
        } else {
            secondary_action_style(palette)
        };
        let hint = (control.action == GithubAction::CloseScreen).then_some("Esc");
        render_action_button(frame, control.area, hint, &control.label, style);
    }
    if screen.dialog.is_none() {
        let status = if screen.submitting {
            "Submitting to GitHub… Navigation is paused until the result arrives.".to_owned()
        } else if let Some(error) = &screen.error {
            format!("Error: {error}")
        } else if screen.loading() {
            "Loading from GitHub…".to_owned()
        } else if let Some(notice) = &screen.notice {
            notice.clone()
        } else if !screen.filter.is_empty() {
            format!(
                "Filter loaded results: {} · {} matches · Actions… contains all commands",
                screen.filter,
                screen.visible_entries().len()
            )
        } else {
            "Tab focus · ↑↓ select/scroll · Enter open · ? all actions · Esc back/close".to_owned()
        };
        frame.render_widget(
            Paragraph::new(status)
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(if screen.error.is_some() {
                    palette.red
                } else {
                    palette.subtext0
                })),
            geometry.status,
        );
    }
}

fn render_list(screen: &GithubScreen, palette: &Palette, frame: &mut Frame) {
    let area = screen.geometry.list;
    if area.is_empty() {
        return;
    }
    let entries = screen.visible_entries();
    let viewport = ModalListViewport::new(
        screen.geometry.list_rows.len(),
        area.height as usize,
        screen.list_scroll,
    );
    let scroll = viewport.scroll_area(area);
    if entries.is_empty() {
        let text = if screen.loading() {
            "Loading…"
        } else if screen.error.is_some() {
            "Could not load results. Use Refresh to retry."
        } else if screen.filter.is_empty() {
            "No results in this scope. Change queue or choose a repository."
        } else {
            "No loaded results match this filter. Clear Filter or load More."
        };
        frame.render_widget(
            Paragraph::new(text)
                .style(Style::default().fg(palette.subtext0))
                .wrap(Wrap { trim: false }),
            scroll.body,
        );
    }
    for (row, visual) in viewport.visible_range().enumerate() {
        let list_row = screen.geometry.list_rows[visual];
        let Some(visible) = list_row.entry() else {
            continue;
        };
        let entry = &screen.entries[entries[visible]];
        let metadata = matches!(list_row, ListRow::Metadata(_));
        let selected = visible == screen.selected;
        let heading = matches!(entry, Entry::Heading(_));
        let style = if heading {
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD)
        } else if selected {
            Style::default()
                .fg(palette.text)
                .bg(if screen.focus == Focus::List {
                    palette.surface1
                } else {
                    palette.surface0
                })
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(palette.text)
        };
        let text = match (entry, metadata) {
            (Entry::Item(item), false) => item.title.clone(),
            (Entry::Item(item), true) => format!(
                "{} #{} · {}{}",
                item.key.repository,
                item.key.number,
                item.state,
                if item.is_draft { " · draft" } else { "" }
            ),
            (Entry::Run(_, run), false) => run.display_title.clone(),
            (Entry::Run(repo, run), true) => format!(
                "{} #{} · {}",
                repo,
                run.run_number,
                run.conclusion.as_deref().unwrap_or(&run.status)
            ),
            _ => entry.label(),
        };
        let text = super::text::truncate_end(&text, scroll.body.width.saturating_sub(2) as usize);
        frame.render_widget(
            Paragraph::new(format!(
                "{}{text}",
                if selected && !heading && !metadata {
                    "› "
                } else {
                    "  "
                },
            ))
            .style(if metadata {
                style.fg(palette.subtext0).remove_modifier(Modifier::BOLD)
            } else {
                style
            }),
            Rect::new(
                scroll.body.x,
                scroll.body.y + row as u16,
                scroll.body.width,
                1,
            ),
        );
    }
    if let Some(track) = scroll.track {
        render_scrollbar(
            frame,
            viewport.metrics(),
            track,
            palette.surface1,
            palette.overlay1,
            "▐",
        );
    }
}

fn render_detail(screen: &GithubScreen, palette: &Palette, frame: &mut Frame) {
    let area = screen.geometry.detail;
    if area.is_empty() {
        return;
    }
    if screen.detail_tab == DetailTab::Diff && screen.item().is_some() {
        render_diff(screen, palette, frame);
        return;
    }
    let viewport = ModalListViewport::new(
        screen.detail_rows.len(),
        area.height as usize,
        screen.detail_scroll,
    );
    let scroll = viewport.scroll_area(area);
    if screen.detail_rows.is_empty() {
        frame.render_widget(
            Paragraph::new(if screen.loading() {
                "Loading details…"
            } else if screen.error.is_some() {
                "Details unavailable. Refresh to retry."
            } else {
                "Select an item to view details."
            })
            .style(Style::default().fg(palette.subtext0)),
            scroll.body,
        );
    }
    for (row, index) in viewport.visible_range().enumerate() {
        let line = &screen.detail_rows[index];
        let selected = line.target.is_some() && line.target == screen.selected_row;
        let style = Style::default()
            .fg(if line.failure {
                palette.red
            } else {
                palette.text
            })
            .bg(if selected {
                palette.surface1
            } else {
                palette.panel_bg
            });
        frame.render_widget(
            Paragraph::new(line.text.as_str()).style(style),
            Rect::new(
                scroll.body.x,
                scroll.body.y + row as u16,
                scroll.body.width,
                1,
            ),
        );
    }
    if let Some(track) = scroll.track {
        render_scrollbar(
            frame,
            viewport.metrics(),
            track,
            palette.surface1,
            palette.overlay1,
            "▐",
        );
    }
}

fn render_files(screen: &GithubScreen, palette: &Palette, frame: &mut Frame) {
    let area = screen.geometry.files;
    if area.is_empty() {
        return;
    }
    let Some(diff) = &screen.diff else {
        return;
    };
    let files = diff.matching_files(&screen.file_filter);
    let viewport = ModalListViewport::new(files.len(), area.height as usize, screen.file_scroll);
    let scroll = viewport.scroll_area(area);
    if files.is_empty() {
        frame.render_widget(
            Paragraph::new("No matching files. Find file clears the filter.")
                .wrap(Wrap { trim: false })
                .style(Style::default().fg(palette.subtext0)),
            scroll.body,
        );
    }
    for (row, index) in viewport.visible_range().enumerate() {
        let file_index = files[index];
        let file = &diff.files()[file_index].file;
        let selected = diff.selected_file() == Some(file_index);
        frame.render_widget(
            Paragraph::new(format!(
                "{}{} [{}]",
                if selected { "› " } else { "  " },
                file.path,
                file.status
            ))
            .style(
                Style::default()
                    .fg(if selected {
                        palette.accent
                    } else {
                        palette.text
                    })
                    .bg(if selected {
                        palette.surface0
                    } else {
                        palette.panel_bg
                    }),
            ),
            Rect::new(
                scroll.body.x,
                scroll.body.y + row as u16,
                scroll.body.width,
                1,
            ),
        );
    }
    if let Some(track) = scroll.track {
        render_scrollbar(
            frame,
            viewport.metrics(),
            track,
            palette.surface1,
            palette.overlay1,
            "▐",
        );
    }
}

fn render_diff(screen: &GithubScreen, palette: &Palette, frame: &mut Frame) {
    let area = screen.geometry.detail;
    let Some(diff) = &screen.diff else {
        frame.render_widget(
            Paragraph::new(if screen.loading() {
                "Loading diff…"
            } else {
                "Diff unavailable. Refresh to retry."
            })
            .style(Style::default().fg(palette.subtext0)),
            area,
        );
        return;
    };
    let viewport = ModalListViewport::new(
        diff.layout().rows.len(),
        area.height as usize,
        screen.detail_scroll,
    );
    let scroll = viewport.scroll_area(area);
    if diff.layout().rows.is_empty() {
        frame.render_widget(
            Paragraph::new("No diff content available for this file.")
                .style(Style::default().fg(palette.subtext0)),
            scroll.body,
        );
    }
    let selection = diff.selection();
    for (row, index) in viewport.visible_range().enumerate() {
        let line_area = Rect::new(
            scroll.body.x,
            scroll.body.y + row as u16,
            scroll.body.width,
            1,
        );
        match &diff.layout().rows[index] {
            DiffRow::Hunk { header, .. } => frame.render_widget(
                Paragraph::new(header.as_str())
                    .style(Style::default().fg(palette.blue).bg(palette.surface_dim)),
                line_area,
            ),
            DiffRow::Notice(text) => frame.render_widget(
                Paragraph::new(text.as_str()).style(Style::default().fg(palette.subtext0)),
                line_area,
            ),
            DiffRow::Line { left, right } => {
                if diff.options().mode == DiffMode::Split {
                    let width = line_area.width / 2;
                    for (cell, side, rect) in [
                        (
                            left.as_ref(),
                            DiffSide::Left,
                            Rect::new(line_area.x, line_area.y, width, 1),
                        ),
                        (
                            right.as_ref(),
                            DiffSide::Right,
                            Rect::new(
                                line_area.x + width,
                                line_area.y,
                                line_area.width.saturating_sub(width),
                                1,
                            ),
                        ),
                    ] {
                        if let Some(cell) = cell {
                            render_diff_cell(frame, palette, cell, side, rect, selection.as_ref());
                        }
                    }
                } else if let Some(cell) = left.as_ref() {
                    render_diff_cell(
                        frame,
                        palette,
                        cell,
                        DiffSide::Left,
                        line_area,
                        selection.as_ref(),
                    );
                } else if let Some(cell) = right.as_ref() {
                    render_diff_cell(
                        frame,
                        palette,
                        cell,
                        DiffSide::Right,
                        line_area,
                        selection.as_ref(),
                    );
                }
            }
        }
    }
    if let Some(track) = scroll.track {
        render_scrollbar(
            frame,
            viewport.metrics(),
            track,
            palette.surface1,
            palette.overlay1,
            "▐",
        );
    }
}

fn render_diff_cell(
    frame: &mut Frame,
    palette: &Palette,
    cell: &DiffCell,
    side: DiffSide,
    area: Rect,
    selection: Option<&crate::github::diff::DiffSelection>,
) {
    let (marker, color) = match cell.kind {
        DiffLineKind::Context => (" ", palette.text),
        DiffLineKind::Added => ("+", palette.green),
        DiffLineKind::Deleted => ("-", palette.red),
    };
    let selected = selection.is_some_and(|selection| {
        selection.side == side
            && cell
                .line_on(side)
                .is_some_and(|line| line >= selection.start_line && line <= selection.end_line)
    });
    let number = if cell.continuation {
        "     ↳".into()
    } else {
        cell.line_on(side)
            .map(|line| format!("{line:>6}"))
            .unwrap_or_else(|| "      ".into())
    };
    let style = Style::default().fg(color).bg(if selected {
        palette.surface1
    } else {
        palette.panel_bg
    });
    let spans = vec![
        Span::styled(
            format!("{number}{marker} "),
            Style::default().fg(if selected {
                palette.accent
            } else {
                palette.overlay1
            }),
        ),
        Span::raw(cell.text.as_str()),
    ];
    frame.render_widget(Paragraph::new(Line::from(spans)).style(style), area);
}

fn render_dialog(screen: &GithubScreen, palette: &Palette, frame: &mut Frame) {
    let Some(dialog) = &screen.dialog else {
        return;
    };
    let area = screen.geometry.modal;
    frame.render_widget(Clear, area);
    let title = match dialog {
        Dialog::Filter(_) => "Filter loaded results",
        Dialog::FileSearch(_) => "Find changed file",
        Dialog::Composer { title, .. } | Dialog::Confirm { title, .. } => title,
        Dialog::Labels { .. } => "Labels",
        Dialog::Merge => "Merge options",
    };
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(palette.accent))
            .style(Style::default().bg(palette.panel_bg).fg(palette.text)),
        area,
    );
    let error_area = Rect::new(
        area.x.saturating_add(1),
        area.y.saturating_add(1),
        area.width.saturating_sub(2),
        2.min(area.height.saturating_sub(2)),
    );
    let hint = if screen.submitting {
        "Submitting… Please wait."
    } else if let Some(error) = &screen.error {
        error
    } else {
        match dialog {
            Dialog::Composer { .. } => {
                "Enter newline · Ctrl+Enter submit · Esc cancel · Paste supported"
            }
            Dialog::Filter(_) => {
                "Matches loaded titles, repositories and status. Load More for older results."
            }
            Dialog::FileSearch(_) => "Matches current and previous file names.",
            Dialog::Labels { .. } => "Click a label to toggle it · ↑↓ select · Enter toggle",
            Dialog::Confirm { .. } => "Review the action, then choose Confirm.",
            Dialog::Merge => "Only repository-allowed methods. No admin merge or branch deletion.",
        }
    };
    frame.render_widget(
        Paragraph::new(hint)
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(if screen.error.is_some() {
                palette.red
            } else {
                palette.subtext0
            })),
        error_area,
    );
    let input = screen.geometry.input;
    match dialog {
        Dialog::Filter(text) | Dialog::FileSearch(text) | Dialog::Composer { text, .. } => {
            render_text_buffer(
                frame,
                palette,
                input,
                text,
                screen.focus != Focus::Controls && !screen.submitting,
            )
        }
        Dialog::Confirm { description, .. } => frame.render_widget(
            Paragraph::new(description.as_str()).wrap(Wrap { trim: false }),
            input,
        ),
        Dialog::Labels { selected } => {
            if screen.labels.is_empty() {
                frame.render_widget(
                    Paragraph::new(if screen.loading() {
                        "Loading labels…"
                    } else {
                        "No labels available."
                    }),
                    input,
                );
            }
            let top = selected.saturating_sub(input.height.saturating_sub(1) as usize);
            for (row, (index, label)) in screen
                .labels
                .iter()
                .enumerate()
                .skip(top)
                .take(input.height as usize)
                .enumerate()
            {
                let applied = screen.item().is_some_and(|item| {
                    item.labels.iter().any(|current| current.name == label.name)
                });
                let style = Style::default().fg(palette.text).bg(if index == *selected {
                    palette.surface1
                } else {
                    palette.panel_bg
                });
                frame.render_widget(
                    Paragraph::new(format!(
                        "[{}] {}",
                        if applied { "x" } else { " " },
                        label.name
                    ))
                    .style(style),
                    Rect::new(input.x, input.y + row as u16, input.width, 1),
                );
            }
        }
        Dialog::Merge => {
            let text = if let Some(item) = screen.item() {
                if let Some(merge) = &item.merge {
                    format!("{} #{}\n{}{}\nMergeable {} · {}\nReview {}\nMerge queue {} · Auto-merge {}\n{}", item.summary.key.repository, item.summary.key.number, item.summary.state, if item.summary.is_draft { " · Draft: mark ready before merging" } else { "" }, merge.mergeable, merge.merge_state_status, merge.review_decision.as_deref().unwrap_or("none"), if merge.queue_enabled { "enabled" } else { "off" }, if merge.auto_merge_enabled { "enabled" } else { "off" }, if screen.merge_repository.is_none() { "Loading allowed methods…" } else { "Unavailable methods are hidden. GitHub rechecks permissions and the head SHA before merging." })
                } else {
                    "Merge state unavailable. Cancel and refresh the pull request.".into()
                }
            } else {
                "Select a pull request first.".into()
            };
            frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), input);
        }
    }
}

fn render_text_buffer(
    frame: &mut Frame,
    palette: &Palette,
    area: Rect,
    text: &TextBuffer,
    focused: bool,
) {
    if area.is_empty() {
        return;
    }
    let layout = text.layout(area.width as usize);
    let top = layout
        .cursor_row
        .saturating_sub(area.height.saturating_sub(1) as usize);
    fill_rect(
        frame,
        area,
        Style::default().fg(palette.text).bg(palette.surface0),
    );
    for (row, range) in layout
        .lines
        .iter()
        .skip(top)
        .take(area.height as usize)
        .enumerate()
    {
        frame.render_widget(
            Paragraph::new(&text.value[range.clone()])
                .style(Style::default().fg(palette.text).bg(palette.surface0)),
            Rect::new(area.x, area.y + row as u16, area.width, 1),
        );
    }
    if focused {
        frame.set_cursor_position((
            area.x
                + layout
                    .cursor_column
                    .min(area.width.saturating_sub(1) as usize) as u16,
            area.y
                + layout
                    .cursor_row
                    .saturating_sub(top)
                    .min(area.height.saturating_sub(1) as usize) as u16,
        ));
    }
}
