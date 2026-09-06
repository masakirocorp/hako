use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use super::scrollbar::{render_pane_scrollbar, scrollbar_thumb, should_show_scrollbar};
use super::widgets::panel_contrast_fg;
use crate::app::state::Palette;
use crate::app::{AppState, ClientViewState, Mode};
use crate::layout::PaneInfo;
use crate::terminal::{TerminalRuntime, TerminalRuntimeRegistry};

pub(crate) fn pane_is_scrolled_back(rt: &TerminalRuntime) -> bool {
    rt.scroll_metrics()
        .is_some_and(|metrics| metrics.offset_from_bottom > 0)
}

fn truncate_label(text: &str, max_width: usize) -> String {
    let len = text.chars().count();
    if len <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }
    let prefix: String = text.chars().take(max_width.saturating_sub(1)).collect();
    format!("{prefix}…")
}

fn pane_border_title(label: &str, pane_width: u16) -> Option<String> {
    let label = label.trim();
    if label.is_empty() || pane_width <= 4 {
        return None;
    }
    let max_label_width = pane_width.saturating_sub(4) as usize;
    Some(format!(" {} ", truncate_label(label, max_label_width)))
}

fn terminal_inner_rect(pane_inner: Rect, pane_scrollbars: bool) -> Rect {
    if !pane_scrollbars || pane_inner.width <= 4 {
        return pane_inner;
    }

    Rect::new(
        pane_inner.x,
        pane_inner.y,
        pane_inner.width.saturating_sub(1),
        pane_inner.height,
    )
}

fn pane_border_label(
    app: &AppState,
    terminal: &crate::terminal::TerminalState,
    show_agent_info: crate::config::PaneBorderAgentInfoConfig,
    seen: bool,
) -> Option<String> {
    let label = terminal.border_label(show_agent_info, seen);
    let location = &terminal.location;
    let profile_name = (!location.is_local())
        .then(|| {
            app.ssh_connection_profiles
                .iter()
                .find(|profile| profile.execution_host_id() == location.execution_host_id)
                .map(|profile| profile.name().to_string())
        })
        .flatten();
    let host = app
        .host_label(crate::app::host_label::HostLabelTarget::ExecutionHost(
            &location.execution_host_id,
        ))
        .to_string();
    let health = if location.is_local() {
        None
    } else if profile_name.is_none() {
        Some("Unavailable")
    } else {
        match app.host_connection_states.get(&location.execution_host_id) {
            None => Some("Offline"),
            Some(crate::execution_host::ConnectionStatus::Disconnected) => Some("Offline"),
            Some(crate::execution_host::ConnectionStatus::Reconnecting { .. }) => Some("Lost"),
            Some(crate::execution_host::ConnectionStatus::AuthenticationRequired) => {
                Some("Unavailable")
            }
            Some(_) => None,
        }
    };
    let host = match health {
        Some(health) => format!("{host} · {health}"),
        None => host,
    };
    Some(match label {
        Some(label) => format!("{label} · {host}"),
        None => host,
    })
}

fn pane_inner_rect(area: Rect, framed: bool) -> Rect {
    if framed {
        Block::default().borders(Borders::ALL).inner(area)
    } else {
        area
    }
}

fn clip_rect(rect: Rect, bounds: Rect) -> Option<Rect> {
    let left = rect.x.max(bounds.x);
    let top = rect.y.max(bounds.y);
    let right = rect
        .x
        .saturating_add(rect.width)
        .min(bounds.x.saturating_add(bounds.width));
    let bottom = rect
        .y
        .saturating_add(rect.height)
        .min(bounds.y.saturating_add(bounds.height));
    (left < right && top < bottom).then(|| Rect::new(left, top, right - left, bottom - top))
}
fn clip_projected_rect(
    projected: crate::app::view_state::ProjectedRect,
    bounds: Rect,
) -> Option<crate::app::view_state::ProjectedRect> {
    let destination = clip_rect(projected.destination, bounds)?;
    let source = Rect::new(
        projected
            .source
            .x
            .saturating_add(destination.x.saturating_sub(projected.destination.x)),
        projected
            .source
            .y
            .saturating_add(destination.y.saturating_sub(projected.destination.y)),
        destination.width,
        destination.height,
    );
    Some(crate::app::view_state::ProjectedRect {
        source,
        destination,
    })
}

fn projected_cell(
    canvas: crate::app::view_state::TabCanvasViewport,
    frame_area: Rect,
    x: u16,
    y: u16,
) -> Option<(u16, u16)> {
    let projected = canvas.project_rect(Rect::new(x, y, 1, 1))?;
    let projected = clip_projected_rect(projected, frame_area)?;
    Some((projected.destination.x, projected.destination.y))
}

fn render_projected_pane_border(
    frame: &mut Frame,
    canvas: crate::app::view_state::TabCanvasViewport,
    info: &PaneInfo,
    frame_area: Rect,
    style: Style,
    thick: bool,
    title: Option<&str>,
) {
    let rect = info.rect;
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let (top_left, top_right, bottom_left, bottom_right, horizontal, vertical) = if thick {
        ("┏", "┓", "┗", "┛", "━", "┃")
    } else {
        ("┌", "┐", "└", "┘", "─", "│")
    };
    let mut set_cell = |x: u16, y: u16, symbol: &str| {
        if let Some((x, y)) = projected_cell(canvas, frame_area, x, y) {
            let cell = &mut frame.buffer_mut()[(x, y)];
            cell.set_symbol(symbol);
            cell.set_style(style);
        }
    };
    set_cell(rect.x, rect.y, top_left);
    if rect.width > 1 {
        for x in rect.x.saturating_add(1)..rect.x.saturating_add(rect.width).saturating_sub(1) {
            set_cell(x, rect.y, horizontal);
        }
        set_cell(
            rect.x.saturating_add(rect.width).saturating_sub(1),
            rect.y,
            top_right,
        );
    }
    if rect.height > 1 {
        let right = rect.x.saturating_add(rect.width).saturating_sub(1);
        for y in rect.y.saturating_add(1)..rect.y.saturating_add(rect.height).saturating_sub(1) {
            set_cell(rect.x, y, vertical);
            if rect.width > 1 {
                set_cell(right, y, vertical);
            }
        }
        if rect.width > 1 {
            let bottom = rect.y.saturating_add(rect.height).saturating_sub(1);
            set_cell(rect.x, bottom, bottom_left);
            for x in rect.x.saturating_add(1)..rect.x.saturating_add(rect.width).saturating_sub(1) {
                set_cell(x, bottom, horizontal);
            }
            set_cell(
                rect.x.saturating_add(rect.width).saturating_sub(1),
                bottom,
                bottom_right,
            );
        } else {
            set_cell(
                rect.x,
                rect.y.saturating_add(rect.height).saturating_sub(1),
                bottom_left,
            );
        }
    }

    let Some(title) = title else {
        return;
    };
    for (offset, ch) in title.chars().enumerate() {
        let Ok(offset) = u16::try_from(offset) else {
            break;
        };
        let x = rect.x.saturating_add(1).saturating_add(offset);
        if x >= rect.x.saturating_add(rect.width).saturating_sub(1) {
            break;
        }
        let mut symbol = [0; 4];
        set_cell(x, rect.y, ch.encode_utf8(&mut symbol));
    }
}

fn render_projected_scrollbar(
    app: &AppState,
    frame: &mut Frame,
    canvas: crate::app::view_state::TabCanvasViewport,
    frame_area: Rect,
    info: &PaneInfo,
    rt: &crate::terminal::TerminalRuntime,
) {
    let Some(metrics) = rt.scroll_metrics() else {
        return;
    };
    let Some(canonical_track) = info.scrollbar_rect else {
        return;
    };
    let Some(track) = canvas
        .project_rect(canonical_track)
        .and_then(|projected| clip_projected_rect(projected, frame_area))
    else {
        return;
    };
    let Some(thumb) = scrollbar_thumb(metrics, canonical_track) else {
        return;
    };
    let thumb = canvas
        .project_rect(Rect::new(
            canonical_track.x,
            thumb.top,
            canonical_track.width,
            thumb.len,
        ))
        .and_then(|projected| clip_projected_rect(projected, frame_area));
    let (track_color, thumb_color, thumb_symbol) = if info.is_focused {
        (app.palette.overlay0, app.palette.overlay1, "▐")
    } else {
        (app.palette.surface_dim, app.palette.overlay0, "▕")
    };
    let buf = frame.buffer_mut();
    for y in track.destination.y..track.destination.y + track.destination.height {
        for x in track.destination.x..track.destination.x + track.destination.width {
            let cell = &mut buf[(x, y)];
            cell.set_symbol("▕");
            cell.set_style(Style::default().fg(track_color));
        }
    }
    if let Some(thumb) = thumb {
        for y in thumb.destination.y..thumb.destination.y + thumb.destination.height {
            for x in thumb.destination.x..thumb.destination.x + thumb.destination.width {
                let cell = &mut buf[(x, y)];
                cell.set_symbol(thumb_symbol);
                cell.set_style(Style::default().fg(thumb_color));
            }
        }
    }
}

fn render_projected_search_highlights(
    app: &AppState,
    frame: &mut Frame,
    info: &PaneInfo,
    destination: Rect,
    source_col: u16,
    source_row: u16,
    top: u32,
    bottom: u32,
    matches: &[(usize, crate::pane::TerminalTextMatch)],
    current: Option<usize>,
    current_only: bool,
) {
    let style = if current_only {
        Style::default()
            .fg(panel_contrast_fg(&app.palette))
            .bg(app.active_workspace_accent_color())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(app.palette.text)
            .bg(app.palette.surface1)
    };
    let visible_top = top.saturating_add(u32::from(source_row));
    let visible_bottom =
        bottom.min(visible_top.saturating_add(u32::from(destination.height.saturating_sub(1))));
    let source_right = source_col.saturating_add(destination.width.saturating_sub(1));
    let canonical_right = info.inner_rect.width.saturating_sub(1);
    for &(index, text_match) in matches {
        if (current == Some(index)) != current_only {
            continue;
        }
        let start_row = text_match.start.row.max(visible_top);
        let end_row = text_match.end.row.min(visible_bottom);
        if start_row > end_row {
            continue;
        }
        for absolute_row in start_row..=end_row {
            let start_col = (if absolute_row == text_match.start.row {
                text_match.start.col
            } else {
                0
            })
            .max(source_col);
            let end_col = (if absolute_row == text_match.end.row {
                text_match.end.col
            } else {
                canonical_right
            })
            .min(source_right);
            if start_col > end_col {
                continue;
            }
            let y = destination.y.saturating_add(
                absolute_row
                    .saturating_sub(visible_top)
                    .min(u32::from(destination.height.saturating_sub(1))) as u16,
            );
            for col in start_col..=end_col {
                let x = destination.x.saturating_add(col.saturating_sub(source_col));
                frame.buffer_mut()[(x, y)].set_style(style);
            }
        }
    }
}

fn render_projected_selection_highlight(
    selection: &Option<crate::selection::Selection>,
    frame: &mut Frame,
    pane_id: crate::layout::PaneId,
    destination: Rect,
    source_col: u16,
    source_row: u16,
    scroll_metrics: Option<crate::pane::ScrollMetrics>,
    p: &Palette,
    host_theme: crate::terminal_theme::TerminalTheme,
) {
    let Some(sel) = selection else {
        return;
    };
    if !sel.is_visible() || sel.pane_id != pane_id {
        return;
    }
    let style = automatic_selection_style(p, host_theme);
    let buf = frame.buffer_mut();
    for y in 0..destination.height {
        for x in 0..destination.width {
            if sel.contains(
                source_row.saturating_add(y),
                source_col.saturating_add(x),
                scroll_metrics,
            ) {
                buf[(destination.x + x, destination.y + y)].set_style(style);
            }
        }
    }
}

fn runtime_for_tab_pane<'a>(
    terminal_runtimes: &'a TerminalRuntimeRegistry,
    tab: &'a crate::workspace::Tab,
    pane_id: crate::layout::PaneId,
) -> Option<(&'a crate::terminal::TerminalId, &'a TerminalRuntime)> {
    let terminal_id = tab.terminal_id(pane_id)?;
    #[cfg(test)]
    if let Some(runtime) = tab.runtimes.get(&pane_id) {
        return Some((terminal_id, runtime));
    }
    terminal_runtimes
        .get(terminal_id)
        .map(|runtime| (terminal_id, runtime))
}

fn separate_split_panes(pane_infos: &mut [PaneInfo], splits: &[crate::layout::SplitBorder]) {
    for split in splits {
        match split.direction {
            ratatui::layout::Direction::Horizontal => {
                for info in pane_infos.iter_mut() {
                    let right = info.rect.x.saturating_add(info.rect.width);
                    if right == split.pos && info.rect.width > 1 {
                        info.rect.width -= 1;
                    }
                }
            }
            ratatui::layout::Direction::Vertical => {
                for info in pane_infos.iter_mut() {
                    let bottom = info.rect.y.saturating_add(info.rect.height);
                    if bottom == split.pos && info.rect.height > 1 {
                        info.rect.height -= 1;
                    }
                }
            }
        }
    }
}
fn stable_scrollbar_gutter(
    rt: &TerminalRuntime,
    pane_inner: Rect,
    pane_scrollbars: bool,
) -> (Rect, Option<Rect>) {
    let inner_rect = terminal_inner_rect(pane_inner, pane_scrollbars);
    if inner_rect == pane_inner {
        return (inner_rect, None);
    }
    let gutter = Rect::new(
        pane_inner.x + pane_inner.width.saturating_sub(1),
        pane_inner.y,
        1,
        pane_inner.height,
    );
    let scrollbar_rect = rt
        .scroll_metrics()
        .filter(|metrics| should_show_scrollbar(*metrics))
        .map(|_| gutter);

    (inner_rect, scrollbar_rect)
}

fn pane_theme_background(p: &Palette) -> Option<Color> {
    match p.panel_bg {
        Color::Reset => None,
        color => Some(color),
    }
}

/// Compute pane layout info and optionally resize pane runtimes to match.
pub(crate) fn compute_pane_infos(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
    resize_panes: bool,
    cell_size: crate::kitty_graphics::HostCellSize,
) -> Vec<PaneInfo> {
    let Some(ws_idx) = app.active else {
        return Vec::new();
    };
    let Some(ws) = app.workspaces.get(ws_idx) else {
        return Vec::new();
    };
    let Some(tab) = ws.active_tab() else {
        return Vec::new();
    };

    let multi_pane = tab.layout.pane_count() > 1;
    let terminal_active = matches!(app.mode, Mode::Terminal | Mode::Github);

    if tab.zoomed {
        let focused_id = tab.layout.focused();
        let pane_inner = pane_inner_rect(area, multi_pane);
        let mut inner_rect = pane_inner;
        let mut scrollbar_rect = None;
        if let Some(rt) = app.runtime_for_pane_in_workspace(terminal_runtimes, ws_idx, focused_id) {
            (inner_rect, scrollbar_rect) =
                stable_scrollbar_gutter(rt, pane_inner, app.pane_scrollbars);
            if resize_panes
                && ws.terminal_id(focused_id).is_some_and(|terminal_id| {
                    !app.direct_attach_resize_locks.contains(terminal_id)
                })
            {
                rt.resize(
                    inner_rect.height,
                    inner_rect.width,
                    cell_size.width_px,
                    cell_size.height_px,
                );
            }
        }
        return vec![PaneInfo {
            id: focused_id,
            rect: area,
            inner_rect,
            scrollbar_rect,
            is_focused: true,
        }];
    }

    let mut pane_infos = tab.layout.panes(area);
    if app.pane_gaps && multi_pane {
        separate_split_panes(&mut pane_infos, &tab.layout.splits(area));
    }
    for info in &mut pane_infos {
        let pane_inner = if multi_pane {
            let border_set = if info.is_focused && terminal_active {
                ratatui::symbols::border::THICK
            } else {
                ratatui::symbols::border::PLAIN
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_set(border_set);
            block.inner(info.rect)
        } else {
            area
        };

        let mut inner_rect = pane_inner;
        let mut scrollbar_rect = None;
        if let Some(rt) = app.runtime_for_pane_in_workspace(terminal_runtimes, ws_idx, info.id) {
            (inner_rect, scrollbar_rect) =
                stable_scrollbar_gutter(rt, pane_inner, app.pane_scrollbars);
            if resize_panes
                && ws.terminal_id(info.id).is_some_and(|terminal_id| {
                    !app.direct_attach_resize_locks.contains(terminal_id)
                })
            {
                rt.resize(
                    inner_rect.height,
                    inner_rect.width,
                    cell_size.width_px,
                    cell_size.height_px,
                );
            }
        }

        info.inner_rect = inner_rect;
        info.scrollbar_rect = scrollbar_rect;
    }

    pane_infos
}

fn layout_for_client_view(
    app: &AppState,
    client_view: &ClientViewState,
    tab: &crate::workspace::Tab,
) -> crate::layout::TileLayout {
    let mut layout = tab.layout.clone();
    let hidden = layout
        .pane_ids()
        .into_iter()
        .filter(|pane_id| {
            app.client_overlay_owners
                .get(pane_id)
                .is_some_and(|owner| *owner != client_view.id())
        })
        .collect::<Vec<_>>();
    for pane_id in hidden {
        let _ = layout.close_pane(pane_id);
    }

    layout
}

pub(super) fn compute_pane_infos_for_view(
    app: &AppState,
    client_view: &ClientViewState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
    resize_panes: bool,
    cell_size: crate::kitty_graphics::HostCellSize,
) -> Vec<PaneInfo> {
    let Some(ws_idx) = client_view.active_workspace else {
        return Vec::new();
    };
    let Some(ws) = app.workspaces.get(ws_idx) else {
        return Vec::new();
    };
    let Some(tab_idx) = client_view.active_tab_index_for_workspace(app, ws_idx) else {
        return Vec::new();
    };
    let Some(tab) = ws.tabs.get(tab_idx) else {
        return Vec::new();
    };
    let layout = layout_for_client_view(app, client_view, tab);
    let focused_id = client_view
        .focused_pane_for_tab(&ws.id, tab.number)
        .filter(|pane_id| layout.pane_ids().contains(pane_id))
        .unwrap_or_else(|| layout.focused());
    let multi_pane = layout.pane_count() > 1;
    let terminal_active = matches!(client_view.mode, Mode::Terminal | Mode::Github);

    if client_view.tab_is_zoomed(&ws.id, tab.number) {
        let pane_inner = pane_inner_rect(area, multi_pane);
        let mut inner_rect = pane_inner;
        let mut scrollbar_rect = None;
        if let Some((terminal_id, rt)) = runtime_for_tab_pane(terminal_runtimes, tab, focused_id) {
            (inner_rect, scrollbar_rect) =
                stable_scrollbar_gutter(rt, pane_inner, app.pane_scrollbars);
            if resize_panes && !app.direct_attach_resize_locks.contains(terminal_id) {
                rt.resize(
                    inner_rect.height,
                    inner_rect.width,
                    cell_size.width_px,
                    cell_size.height_px,
                );
            }
        }
        return vec![PaneInfo {
            id: focused_id,
            rect: area,
            inner_rect,
            scrollbar_rect,
            is_focused: true,
        }];
    }

    let mut pane_infos = layout.panes(area);
    if app.pane_gaps && multi_pane {
        separate_split_panes(&mut pane_infos, &layout.splits(area));
    }
    for info in &mut pane_infos {
        info.is_focused = info.id == focused_id;
        let pane_inner = if multi_pane {
            let border_set = if info.is_focused && terminal_active {
                ratatui::symbols::border::THICK
            } else {
                ratatui::symbols::border::PLAIN
            };
            let block = Block::default()
                .borders(Borders::ALL)
                .border_set(border_set);
            block.inner(info.rect)
        } else {
            area
        };

        let mut inner_rect = pane_inner;
        let mut scrollbar_rect = None;
        if let Some((terminal_id, rt)) = runtime_for_tab_pane(terminal_runtimes, tab, info.id) {
            (inner_rect, scrollbar_rect) =
                stable_scrollbar_gutter(rt, pane_inner, app.pane_scrollbars);
            if resize_panes && !app.direct_attach_resize_locks.contains(terminal_id) {
                rt.resize(
                    inner_rect.height,
                    inner_rect.width,
                    cell_size.width_px,
                    cell_size.height_px,
                );
            }
        }

        info.inner_rect = inner_rect;
        info.scrollbar_rect = scrollbar_rect;
    }
    pane_infos
}

pub(crate) fn popup_pane_rects_for_view(
    app: &AppState,
    view: &ClientViewState,
    area: Rect,
) -> Option<(Rect, Rect)> {
    let popup = app.popup_panes.get(&view.popup_pane?)?;
    crate::popup_size::resolve_popup_geometry(popup.width, popup.height, area)
        .map(|geometry| (geometry.outer, geometry.inner))
}

pub(super) fn resize_popup_pane_for_view(
    app: &AppState,
    view: &ClientViewState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
    cell_size: crate::kitty_graphics::HostCellSize,
) {
    let Some(pane_id) = view.popup_pane else {
        return;
    };
    let Some(popup) = app.popup_panes.get(&pane_id) else {
        return;
    };
    let Some((_, inner)) = popup_pane_rects_for_view(app, view, area) else {
        return;
    };
    if app.direct_attach_resize_locks.contains(&popup.terminal_id) {
        return;
    }
    if let Some(runtime) = terminal_runtimes.get(&popup.terminal_id) {
        runtime.resize(
            inner.height,
            inner.width,
            cell_size.width_px,
            cell_size.height_px,
        );
    }
}

pub(super) fn render_popup_pane_for_view(
    app: &AppState,
    view: &ClientViewState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
    area: Rect,
) {
    let Some(pane_id) = view.popup_pane else {
        return;
    };
    let Some(popup) = app.popup_panes.get(&pane_id) else {
        return;
    };
    let Some((outer, inner)) = popup_pane_rects_for_view(app, view, area) else {
        return;
    };
    let Some(runtime) = terminal_runtimes.get(&popup.terminal_id) else {
        return;
    };
    let title = app
        .terminals
        .get(&popup.terminal_id)
        .and_then(|terminal| terminal.manual_label.as_deref())
        .unwrap_or("Popup");
    let title = if outer.width > 4 {
        let max = outer.width.saturating_sub(4) as usize;
        let label: String = title.chars().take(max).collect();
        Some(Line::from(format!(" {label} ")))
    } else {
        None
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.palette.accent))
        .title(title.unwrap_or_else(|| Line::from("Popup")))
        .style(Style::default().bg(app.palette.panel_bg));
    frame.render_widget(Clear, outer);
    frame.render_widget(block, outer);
    runtime.render_with_theme_background(
        frame,
        inner,
        view.mode == Mode::Terminal && !pane_is_scrolled_back(runtime),
        pane_theme_background(&app.palette),
    );
}

pub(super) fn render_panes_for_view(
    app: &AppState,
    client_view: &ClientViewState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
    area: Rect,
) {
    let Some(ws_idx) = client_view.active_workspace else {
        render_empty_for_view(app, client_view, frame);
        return;
    };
    let Some(ws) = app.workspaces.get(ws_idx) else {
        render_empty_for_view(app, client_view, frame);
        return;
    };
    let Some(tab_idx) = client_view.active_tab_index_for_workspace(app, ws_idx) else {
        render_empty_for_view(app, client_view, frame);
        return;
    };
    let Some(tab) = ws.tabs.get(tab_idx) else {
        render_empty_for_view(app, client_view, frame);
        return;
    };
    let Some(canvas) = client_view.tab_canvas_view else {
        return;
    };
    let Some(frame_area) = clip_rect(area, frame.area()) else {
        return;
    };

    let multi_pane = tab.layout.pane_count() > 1;
    let active_accent = app.palette_for_workspace(ws_idx).accent;
    let terminal_active = matches!(client_view.mode, Mode::Terminal | Mode::Github);
    let watching = client_view.tab_control.is_watching();

    for info in &client_view.computed.pane_infos {
        let pane_state = tab.panes.get(&info.id);
        let Some((_, rt)) = runtime_for_tab_pane(terminal_runtimes, tab, info.id) else {
            continue;
        };

        if multi_pane {
            let (border_style, thick) = if watching {
                (Style::default().fg(app.palette.overlay0), false)
            } else if info.is_focused && terminal_active {
                (Style::default().fg(active_accent), true)
            } else if info.is_focused {
                (Style::default().fg(active_accent), false)
            } else {
                (Style::default().fg(app.palette.overlay0), false)
            };
            let title = pane_state
                .and_then(|pane| {
                    app.terminals
                        .get(&pane.attached_terminal_id)
                        .and_then(|terminal| {
                            pane_border_label(app, terminal, app.pane_border_agent_info, pane.seen)
                        })
                })
                .and_then(|label| pane_border_title(&label, info.rect.width));
            render_projected_pane_border(
                frame,
                canvas,
                info,
                frame_area,
                border_style,
                thick,
                title.as_deref(),
            );
        }

        let Some(projected_inner) = canvas
            .project_rect(info.inner_rect)
            .and_then(|projected| clip_projected_rect(projected, frame_area))
        else {
            // Border-only strips remain visible even when the terminal content is
            // entirely outside the observer's source window.
            if multi_pane {
                render_projected_scrollbar(app, frame, canvas, frame_area, info, rt);
            }
            continue;
        };
        let source_col = projected_inner.source.x.saturating_sub(info.inner_rect.x);
        let source_row = projected_inner.source.y.saturating_sub(info.inner_rect.y);
        let show_cursor = client_view.can_mutate_tab()
            && info.is_focused
            && terminal_active
            && !pane_is_scrolled_back(rt);
        rt.render_view_with_theme_background(
            frame,
            crate::pane::TerminalViewport {
                destination: projected_inner.destination,
                source_col,
                source_row,
            },
            show_cursor,
            pane_theme_background(&app.palette),
        );
        render_projected_scrollbar(app, frame, canvas, frame_area, info, rt);

        let should_dim = !info.is_focused && multi_pane && !terminal_active;
        let dim_area = if watching {
            canvas
                .project_rect(info.rect)
                .and_then(|projected| clip_projected_rect(projected, frame_area))
                .map(|projected| projected.destination)
        } else if should_dim {
            Some(projected_inner.destination)
        } else {
            None
        };
        if let Some(inner) = dim_area {
            let buf = frame.buffer_mut();
            for y in inner.y..inner.y + inner.height {
                for x in inner.x..inner.x + inner.width {
                    let cell = &mut buf[(x, y)];
                    cell.set_style(cell.style().add_modifier(Modifier::DIM));
                }
            }
        }

        let (copy_search_top, copy_search_bottom, copy_search_matches) =
            validated_copy_mode_search_matches(client_view.copy_mode.as_ref(), info, rt);
        let copy_search_current = client_view
            .copy_mode
            .as_ref()
            .and_then(|copy_mode| copy_mode.search.current);
        render_projected_search_highlights(
            app,
            frame,
            info,
            projected_inner.destination,
            source_col,
            source_row,
            copy_search_top,
            copy_search_bottom,
            &copy_search_matches,
            copy_search_current,
            false,
        );
        render_projected_selection_highlight(
            &client_view.selection,
            frame,
            info.id,
            projected_inner.destination,
            source_col,
            source_row,
            rt.scroll_metrics(),
            &app.palette,
            app.host_terminal_theme,
        );
        render_projected_search_highlights(
            app,
            frame,
            info,
            projected_inner.destination,
            source_col,
            source_row,
            copy_search_top,
            copy_search_bottom,
            &copy_search_matches,
            copy_search_current,
            true,
        );
        render_copy_mode_cursor_for_view(
            app,
            client_view,
            frame,
            info,
            projected_inner.destination,
            source_col,
            source_row,
        );
    }
}

pub(super) fn render_panes(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
    area: Rect,
) {
    let Some(ws_idx) = app.active else {
        render_empty(app, frame, area);
        return;
    };
    let Some(ws) = app.workspaces.get(ws_idx) else {
        render_empty(app, frame, area);
        return;
    };
    let Some(tab) = ws.active_tab() else {
        render_empty(app, frame, area);
        return;
    };

    let multi_pane = tab.layout.pane_count() > 1;
    let active_accent = app.active_workspace_accent_color();
    let terminal_active = matches!(app.mode, Mode::Terminal | Mode::Github);

    for info in &app.view.pane_infos {
        let pane_state = ws.pane_state(info.id);

        if let Some(rt) = app.runtime_for_pane_in_workspace(terminal_runtimes, ws_idx, info.id) {
            if multi_pane {
                let (border_style, border_set) = if info.is_focused && terminal_active {
                    (
                        Style::default().fg(active_accent),
                        ratatui::symbols::border::THICK,
                    )
                } else if info.is_focused {
                    (
                        Style::default().fg(active_accent),
                        ratatui::symbols::border::PLAIN,
                    )
                } else {
                    (
                        Style::default().fg(app.palette.overlay0),
                        ratatui::symbols::border::PLAIN,
                    )
                };

                let mut block = Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .border_set(border_set);
                if let Some(title) = pane_state
                    .and_then(|pane| {
                        app.terminals
                            .get(&pane.attached_terminal_id)
                            .and_then(|terminal| {
                                terminal.border_label(app.pane_border_agent_info, pane.seen)
                            })
                    })
                    .and_then(|label| pane_border_title(&label, info.rect.width))
                {
                    block = block.title(Line::from(Span::styled(title, border_style)));
                }
                frame.render_widget(block, info.rect);
            }

            let show_cursor = info.is_focused && terminal_active && !pane_is_scrolled_back(rt);
            rt.render_with_theme_background(
                frame,
                info.inner_rect,
                show_cursor,
                pane_theme_background(&app.palette),
            );
            render_pane_scrollbar(app, frame, info, rt);

            let should_dim = !info.is_focused && multi_pane && !terminal_active;
            if should_dim {
                let inner = info.inner_rect;
                let buf = frame.buffer_mut();
                for y in inner.y..inner.y + inner.height {
                    for x in inner.x..inner.x + inner.width {
                        let cell = &mut buf[(x, y)];
                        cell.set_style(cell.style().add_modifier(Modifier::DIM));
                    }
                }
            }

            let (copy_search_top, copy_search_bottom, copy_search_matches) =
                validated_copy_mode_search_matches(app.copy_mode.as_ref(), info, rt);
            let copy_search_current = app
                .copy_mode
                .as_ref()
                .and_then(|copy_mode| copy_mode.search.current);
            render_copy_mode_search_highlights(
                app,
                frame,
                info,
                copy_search_top,
                copy_search_bottom,
                &copy_search_matches,
                copy_search_current,
                false,
            );
            render_selection_highlight(
                &app.selection,
                frame,
                info.id,
                info.inner_rect,
                rt.scroll_metrics(),
                &app.palette,
                app.host_terminal_theme,
            );
            render_copy_mode_search_highlights(
                app,
                frame,
                info,
                copy_search_top,
                copy_search_bottom,
                &copy_search_matches,
                copy_search_current,
                true,
            );
            render_copy_mode_cursor(app, frame, info);
        }
    }
}

fn render_copy_mode_cursor(app: &AppState, frame: &mut Frame, info: &PaneInfo) {
    render_copy_mode_cursor_cell(app, app.mode, app.copy_mode.as_ref(), frame, info);
}

fn render_copy_mode_cursor_for_view(
    app: &AppState,
    client_view: &crate::app::ClientViewState,
    frame: &mut Frame,
    info: &PaneInfo,
    destination: Rect,
    source_col: u16,
    source_row: u16,
) {
    if client_view.mode != Mode::Copy {
        return;
    }
    let Some(copy_mode) = client_view.copy_mode.as_ref() else {
        return;
    };
    if copy_mode.pane_id != info.id
        || copy_mode.cursor_row >= info.inner_rect.height
        || copy_mode.cursor_col >= info.inner_rect.width
        || copy_mode.cursor_row < source_row
        || copy_mode.cursor_col < source_col
        || copy_mode.cursor_row >= source_row.saturating_add(destination.height)
        || copy_mode.cursor_col >= source_col.saturating_add(destination.width)
    {
        return;
    }
    let x = destination.x + copy_mode.cursor_col.saturating_sub(source_col);
    let y = destination.y + copy_mode.cursor_row.saturating_sub(source_row);
    let cell = &mut frame.buffer_mut()[(x, y)];
    cell.set_style(
        Style::default()
            .fg(panel_contrast_fg(&app.palette))
            .bg(app.active_workspace_accent_color())
            .add_modifier(Modifier::BOLD),
    );
}

fn render_copy_mode_cursor_cell(
    app: &AppState,
    mode: Mode,
    copy_mode: Option<&crate::app::state::CopyModeState>,
    frame: &mut Frame,
    info: &PaneInfo,
) {
    if mode != Mode::Copy {
        return;
    }
    let Some(copy_mode) = copy_mode else {
        return;
    };
    if copy_mode.pane_id != info.id
        || copy_mode.cursor_row >= info.inner_rect.height
        || copy_mode.cursor_col >= info.inner_rect.width
    {
        return;
    }

    let x = info.inner_rect.x + copy_mode.cursor_col;
    let y = info.inner_rect.y + copy_mode.cursor_row;
    let cell = &mut frame.buffer_mut()[(x, y)];
    cell.set_style(
        Style::default()
            .fg(panel_contrast_fg(&app.palette))
            .bg(app.active_workspace_accent_color())
            .add_modifier(Modifier::BOLD),
    );
}

fn validated_copy_mode_search_matches(
    copy_mode: Option<&crate::app::state::CopyModeState>,
    info: &PaneInfo,
    rt: &crate::terminal::TerminalRuntime,
) -> (u32, u32, Vec<(usize, crate::pane::TerminalTextMatch)>) {
    let Some(copy_mode) = copy_mode else {
        return (0, 0, Vec::new());
    };
    if copy_mode.pane_id != info.id {
        return (0, 0, Vec::new());
    }
    let Some(metrics) = rt.scroll_metrics() else {
        return (0, 0, Vec::new());
    };
    let top = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom)
        .min(u32::MAX as usize) as u32;
    let bottom = top.saturating_add(u32::from(info.inner_rect.height.saturating_sub(1)));
    let first_visible = copy_mode
        .search
        .matches
        .partition_point(|text_match| text_match.end.row < top);
    let visible = &copy_mode.search.matches[first_visible..];
    let visible_len = visible.partition_point(|text_match| text_match.start.row <= bottom);
    let candidates = visible[..visible_len].to_vec();
    let validity = rt.text_matches_are_current(&candidates);
    let matches = candidates
        .into_iter()
        .zip(validity)
        .enumerate()
        .filter_map(|(offset, (text_match, is_current))| {
            is_current.then_some((first_visible + offset, text_match))
        })
        .collect();
    (top, bottom, matches)
}

fn render_copy_mode_search_highlights(
    app: &AppState,
    frame: &mut Frame,
    info: &PaneInfo,
    top: u32,
    bottom: u32,
    matches: &[(usize, crate::pane::TerminalTextMatch)],
    current: Option<usize>,
    current_only: bool,
) {
    let style = if current_only {
        Style::default()
            .fg(panel_contrast_fg(&app.palette))
            .bg(app.active_workspace_accent_color())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(app.palette.text)
            .bg(app.palette.surface1)
    };
    for &(index, text_match) in matches {
        if (current == Some(index)) != current_only {
            continue;
        }
        let start_row = text_match.start.row.max(top);
        let end_row = text_match.end.row.min(bottom);
        for absolute_row in start_row..=end_row {
            let viewport_row = absolute_row.saturating_sub(top) as u16;
            let start_col = if absolute_row == text_match.start.row {
                text_match.start.col
            } else {
                0
            };
            let end_col = if absolute_row == text_match.end.row {
                text_match.end.col
            } else {
                info.inner_rect.width.saturating_sub(1)
            };
            for col in start_col..=end_col.min(info.inner_rect.width.saturating_sub(1)) {
                let x = info.inner_rect.x.saturating_add(col);
                let y = info.inner_rect.y.saturating_add(viewport_row);
                frame.buffer_mut()[(x, y)].set_style(style);
            }
        }
    }
}

fn render_selection_highlight(
    selection: &Option<crate::selection::Selection>,
    frame: &mut Frame,
    pane_id: crate::layout::PaneId,
    inner: Rect,
    scroll_metrics: Option<crate::pane::ScrollMetrics>,
    p: &Palette,
    host_theme: crate::terminal_theme::TerminalTheme,
) {
    if let Some(sel) = selection {
        if sel.is_visible() && sel.pane_id == pane_id {
            let buf = frame.buffer_mut();
            let style = automatic_selection_style(p, host_theme);
            for y in 0..inner.height {
                for x in 0..inner.width {
                    if sel.contains(y, x, scroll_metrics) {
                        let cell = &mut buf[(inner.x + x, inner.y + y)];
                        cell.set_style(style);
                    }
                }
            }
        }
    }
}

type Rgb = (u8, u8, u8);

fn automatic_selection_style(
    p: &Palette,
    host_theme: crate::terminal_theme::TerminalTheme,
) -> Style {
    let bg = automatic_selection_bg(p, host_theme);
    Style::reset().fg(selection_fg_for_bg(bg, p)).bg(bg)
}

fn automatic_selection_bg(p: &Palette, host_theme: crate::terminal_theme::TerminalTheme) -> Color {
    let Some(background) = host_theme.background.map(terminal_theme_to_rgb) else {
        return selection_palette_background(p);
    };

    let target = if relative_luminance(background) < 0.5 {
        (255, 255, 255)
    } else {
        (0, 0, 0)
    };
    let selected = mix_rgb(background, target, 0.28);
    Color::Rgb(selected.0, selected.1, selected.2)
}

fn selection_palette_background(p: &Palette) -> Color {
    if p.panel_bg == Color::Reset {
        p.surface_dim
    } else {
        p.panel_bg
    }
}

fn terminal_theme_to_rgb(color: crate::terminal_theme::RgbColor) -> Rgb {
    (color.r, color.g, color.b)
}

fn selection_fg_for_bg(bg: Color, p: &Palette) -> Color {
    color_to_rgb(bg)
        .map(|bg| {
            if relative_luminance(bg) < 0.5 {
                Color::White
            } else {
                Color::Black
            }
        })
        .unwrap_or_else(|| panel_contrast_fg(p))
}

fn mix_rgb(base: Rgb, target: Rgb, amount: f32) -> Rgb {
    fn channel(base: u8, target: u8, amount: f32) -> u8 {
        (f32::from(base) + (f32::from(target) - f32::from(base)) * amount).round() as u8
    }
    (
        channel(base.0, target.0, amount),
        channel(base.1, target.1, amount),
        channel(base.2, target.2, amount),
    )
}

fn relative_luminance(color: Rgb) -> f32 {
    fn channel(value: u8) -> f32 {
        let value = f32::from(value) / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(color.0) + 0.7152 * channel(color.1) + 0.0722 * channel(color.2)
}

fn color_to_rgb(color: Color) -> Option<Rgb> {
    match color {
        Color::Reset => None,
        Color::Black => Some((0, 0, 0)),
        Color::Red => Some((128, 0, 0)),
        Color::Green => Some((0, 128, 0)),
        Color::Yellow => Some((128, 128, 0)),
        Color::Blue => Some((0, 0, 128)),
        Color::Magenta => Some((128, 0, 128)),
        Color::Cyan => Some((0, 128, 128)),
        Color::Gray => Some((192, 192, 192)),
        Color::DarkGray => Some((128, 128, 128)),
        Color::LightRed => Some((255, 0, 0)),
        Color::LightGreen => Some((0, 255, 0)),
        Color::LightYellow => Some((255, 255, 0)),
        Color::LightBlue => Some((0, 0, 255)),
        Color::LightMagenta => Some((255, 0, 255)),
        Color::LightCyan => Some((0, 255, 255)),
        Color::White => Some((255, 255, 255)),
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Indexed(_) => None,
    }
}

pub(super) fn wash_rect(frame: &mut Frame, area: Rect, palette: &Palette) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let Some(mute) = color_to_rgb(palette.overlay1) else {
        return;
    };
    let panel = color_to_rgb(palette.panel_bg);
    let default_fg = color_to_rgb(palette.text).unwrap_or(mute);
    let buf = frame.buffer_mut();
    let right = area.x.saturating_add(area.width);
    let bottom = area.y.saturating_add(area.height);
    for y in area.y..bottom {
        for x in area.x..right {
            let cell = &mut buf[(x, y)];
            let style = cell.style();
            let mut next = style.remove_modifier(Modifier::BOLD);
            let fg = style.fg.and_then(color_to_rgb).unwrap_or(default_fg);
            let (r, g, b) = mix_rgb(fg, mute, 0.32);
            next = next.fg(Color::Rgb(r, g, b));
            if let (Some(bg), Some(panel)) = (style.bg.and_then(color_to_rgb), panel) {
                let (r, g, b) = mix_rgb(bg, panel, 0.18);
                next = next.bg(Color::Rgb(r, g, b));
            }
            cell.set_style(next);
        }
    }
}

fn render_empty_for_view(app: &AppState, client_view: &ClientViewState, frame: &mut Frame) {
    render_empty_with_context(
        app,
        client_view.active_workspace,
        client_view.group_filter_enabled,
        client_view.active_group,
        frame,
        client_view.computed.terminal_area,
    );
}

fn render_empty(app: &AppState, frame: &mut Frame, area: Rect) {
    render_empty_with_context(
        app,
        app.active,
        app.group_filter_enabled,
        app.active_group,
        frame,
        area,
    );
}

fn render_empty_with_context(
    app: &AppState,
    active_workspace: Option<usize>,
    group_filter_enabled: bool,
    active_group: usize,
    frame: &mut Frame,
    area: Rect,
) {
    let p = &app.palette;
    let active_workspace_has_no_tabs = active_workspace
        .and_then(|idx| app.workspaces.get(idx))
        .is_some_and(|ws| ws.tabs.is_empty());
    let (title, detail, context, action_label) = if active_workspace_has_no_tabs {
        (
            "  No Tabs in This Space",
            "  The space is still here.",
            "  Create a tab to keep working in this context.",
            app.keybinds
                .new_tab
                .label()
                .unwrap_or_else(|| "Unset".to_string()),
        )
    } else if app.workspaces.is_empty() {
        (
            "  No Spaces Yet",
            "  A space is one project context.",
            "  Its root pane sets the default repo or folder name.",
            app.keybinds
                .new_workspace
                .label()
                .unwrap_or_else(|| "Unset".to_string()),
        )
    } else if group_filter_enabled {
        (
            "  No Spaces in This Group",
            "  Switch groups or create one here.",
            "  Hidden spaces stay in the group menu.",
            app.keybinds
                .new_workspace
                .label()
                .unwrap_or_else(|| "Unset".to_string()),
        )
    } else {
        (
            "  No Active Space",
            "  Select a space from the sidebar.",
            "  Create one if you want a fresh context.",
            app.keybinds
                .new_workspace
                .label()
                .unwrap_or_else(|| "Unset".to_string()),
        )
    };
    let accent = if !group_filter_enabled {
        active_workspace
            .and_then(|ws_idx| app.workspaces.get(ws_idx))
            .and_then(|workspace| app.group_index_by_id(&workspace.group_id))
            .map(|group_idx| app.group_accent_color(group_idx))
            .unwrap_or_else(|| app.group_accent_color(active_group))
    } else {
        app.group_accent_color(active_group)
    };
    let lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(title, Style::default().fg(p.overlay0))),
        Line::from(""),
        Line::from(Span::styled(detail, Style::default().fg(p.overlay1))),
        Line::from(Span::styled(context, Style::default().fg(p.overlay1))),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Press ", Style::default().fg(p.overlay0)),
            Span::styled(
                action_label,
                Style::default().fg(accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to create one", Style::default().fg(p.overlay0)),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(p.surface_dim)),
        ),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect::AgentState;
    use crate::layout::PaneId;
    use crate::selection::Selection;
    use crate::terminal::TerminalRuntime;
    use crate::workspace::Workspace;
    use ratatui::{backend::TestBackend, buffer::Buffer, Terminal};

    #[test]
    fn remote_pane_border_shows_connection_name_and_offline_state() {
        let mut app = AppState::test_new();
        let profile = crate::persist::ssh_profiles::SshConnectionProfile::new(
            "workbox",
            "Work box",
            "alice@workbox",
            None,
        )
        .expect("valid profile");
        let location = crate::execution_host::ResourceLocation::new(
            profile.execution_host_id(),
            crate::execution_host::HostPath::new("/srv/app").expect("valid remote path"),
        );
        app.ssh_connection_profiles.push(profile);
        let terminal =
            crate::terminal::TerminalState::new_at(crate::terminal::TerminalId::alloc(), location);

        assert_eq!(
            pane_border_label(&app, &terminal, Default::default(), false),
            Some("Work box · Offline".to_string())
        );
    }

    #[test]
    fn remote_pane_border_marks_missing_profile_unavailable() {
        let app = AppState::test_new();
        let terminal = crate::terminal::TerminalState::new_at(
            crate::terminal::TerminalId::alloc(),
            crate::execution_host::ResourceLocation::new(
                crate::execution_host::ExecutionHostId::new("ssh:missing:1")
                    .expect("valid host id"),
                crate::execution_host::HostPath::new("/srv/app").expect("valid remote path"),
            ),
        );

        assert_eq!(
            pane_border_label(&app, &terminal, Default::default(), false),
            Some("ssh:missing:1 · Unavailable".to_string())
        );
    }

    #[test]
    fn local_pane_border_identifies_local_in_a_mixed_workspace() {
        let app = AppState::test_new();
        let terminal = crate::terminal::TerminalState::new_at(
            crate::terminal::TerminalId::alloc(),
            crate::execution_host::ResourceLocation::local("/tmp").expect("valid local path"),
        );

        assert_eq!(
            pane_border_label(&app, &terminal, Default::default(), false),
            Some("test-host".to_string())
        );
    }

    #[test]
    fn client_overlay_projection_is_owner_only_and_collapses_hidden_geometry() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root = workspace.tabs[0].root_pane;
        let overlay = workspace.test_split(ratatui::layout::Direction::Horizontal);
        workspace.tabs[0].layout.focus_pane(root);
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let mut owner = ClientViewState::from_default_client_state(&app);
        let mut other = ClientViewState::from_default_client_state(&app);
        owner.reconcile(&app);
        other.reconcile(&app);
        app.client_overlay_owners.insert(overlay, owner.id());
        assert!(owner.focus_pane_in_workspace(&app, 0, 0, overlay));
        owner.set_tab_zoomed(&app.workspaces[0].id, 1, true);

        let area = Rect::new(3, 2, 80, 24);
        let runtimes = TerminalRuntimeRegistry::new();
        let owner_infos = compute_pane_infos_for_view(
            &app,
            &owner,
            &runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let other_infos = compute_pane_infos_for_view(
            &app,
            &other,
            &runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );

        assert_eq!(owner_infos.len(), 1);
        assert_eq!(owner_infos[0].id, overlay);
        assert_eq!(other_infos.len(), 1);
        assert_eq!(other_infos[0].id, root);
        assert_eq!(other_infos[0].rect, area);
        assert!(!other.focus_pane_in_workspace(&app, 0, 0, overlay));
    }

    #[test]
    fn pane_border_title_trims_and_truncates() {
        assert_eq!(
            pane_border_title(" claude ", 20).as_deref(),
            Some(" claude ")
        );
        assert_eq!(pane_border_title("", 20), None);
        assert_eq!(pane_border_title("abcdef", 8).as_deref(), Some(" abc… "));
        assert_eq!(pane_border_title("abcdef", 4), None);
    }

    #[test]
    fn projected_border_keeps_canonical_edges_and_title_position() {
        let info = PaneInfo {
            id: PaneId::from_raw(1),
            rect: Rect::new(1, 1, 6, 4),
            inner_rect: Rect::new(2, 2, 4, 2),
            scrollbar_rect: None,
            is_focused: false,
        };
        let canvas = crate::app::view_state::TabCanvasViewport::new(
            ratatui::layout::Size::new(12, 8),
            Rect::new(0, 0, 4, 1),
            crate::app::view_state::CanvasOrigin { col: 2, row: 1 },
        );
        let backend = TestBackend::new(4, 1);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| {
                render_projected_pane_border(
                    frame,
                    canvas,
                    &info,
                    frame.area(),
                    Style::default().fg(Color::White),
                    false,
                    Some(" abc "),
                );
            })
            .expect("render projected title");
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].symbol(), " ");
        assert_eq!(buffer[(1, 0)].symbol(), "a");
        assert_eq!(buffer[(2, 0)].symbol(), "b");
        assert_eq!(buffer[(3, 0)].symbol(), "c");

        let canvas = crate::app::view_state::TabCanvasViewport::new(
            ratatui::layout::Size::new(12, 8),
            Rect::new(0, 0, 4, 3),
            crate::app::view_state::CanvasOrigin { col: 2, row: 2 },
        );
        let backend = TestBackend::new(4, 3);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| {
                render_projected_pane_border(
                    frame,
                    canvas,
                    &info,
                    frame.area(),
                    Style::default().fg(Color::White),
                    false,
                    None,
                );
            })
            .expect("render cropped projected border");
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(3, 0)].symbol(), " ");
        assert_eq!(buffer[(3, 1)].symbol(), " ");
        assert_eq!(buffer[(3, 2)].symbol(), "─");
    }

    #[test]
    fn projected_pane_border_draws_all_four_edges() {
        let info = PaneInfo {
            id: PaneId::from_raw(1),
            rect: Rect::new(0, 0, 8, 5),
            inner_rect: Rect::new(1, 1, 6, 3),
            scrollbar_rect: None,
            is_focused: true,
        };
        let canvas = crate::app::view_state::TabCanvasViewport::new(
            ratatui::layout::Size::new(8, 5),
            Rect::new(0, 0, 8, 5),
            crate::app::view_state::CanvasOrigin { col: 0, row: 0 },
        );
        let backend = TestBackend::new(8, 5);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| {
                render_projected_pane_border(
                    frame,
                    canvas,
                    &info,
                    frame.area(),
                    Style::default().fg(Color::White),
                    true,
                    None,
                );
            })
            .expect("render full projected border");
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].symbol(), "┏");
        assert_eq!(buffer[(7, 0)].symbol(), "┓");
        assert_eq!(buffer[(0, 4)].symbol(), "┗");
        assert_eq!(buffer[(7, 4)].symbol(), "┛");
        for x in 1..7 {
            assert_eq!(buffer[(x, 0)].symbol(), "━");
            assert_eq!(buffer[(x, 4)].symbol(), "━");
        }
        for y in 1..4 {
            assert_eq!(buffer[(0, y)].symbol(), "┃");
            assert_eq!(buffer[(7, y)].symbol(), "┃");
        }
    }

    #[test]
    fn client_split_panes_draw_closed_right_edges() {
        let mut app = AppState::test_new();
        app.zen_mode = true;
        app.pane_borders = true;
        app.pane_scrollbars = false;
        app.pane_gaps = true;
        let mut workspace = Workspace::test_new("split");
        let left = workspace.tabs[0].root_pane;
        let right = workspace.test_split(ratatui::layout::Direction::Horizontal);
        workspace.tabs[0].runtimes.insert(
            left,
            TerminalRuntime::test_with_scrollback_bytes(20, 8, 1024, b"left\n"),
        );
        workspace.tabs[0].runtimes.insert(
            right,
            TerminalRuntime::test_with_scrollback_bytes(20, 8, 1024, b"right\n"),
        );
        workspace.tabs[0].layout.focus_pane(left);
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(0, 0, 40, 12);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let mut client = ClientViewState::from_default_client_state(&app);
        client.zen_mode = true;
        crate::ui::compute_view_for_client_without_resizing_panes(
            &app,
            &mut client,
            &terminal_runtimes,
            area,
        );
        let backend = TestBackend::new(area.width, area.height);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| {
                crate::ui::render_with_runtime_registry_for_view(
                    &app,
                    &client,
                    &terminal_runtimes,
                    frame,
                );
            })
            .expect("render split panes");
        let buffer = terminal.backend().buffer();
        let dump = buffer_text(buffer, area.width, area.height);
        let mut panes = client.computed.pane_infos.clone();
        panes.sort_by_key(|info| info.rect.x);
        assert_eq!(panes.len(), 2, "expected a two-pane split:\n{dump}");
        assert_eq!(
            panes[0].rect.x + panes[0].rect.width + 1,
            panes[1].rect.x,
            "pane_gaps should leave a column between split boxes:\n{dump}"
        );
        let canvas = client.tab_canvas_view.expect("canvas");
        for info in &panes {
            let right_x = info.rect.x + info.rect.width - 1;
            let edge_y = info.rect.y.saturating_add(1);
            let (x, y) = canvas
                .canvas_to_screen(right_x, edge_y)
                .expect("right edge should be on screen");
            let symbol = buffer[(x, y)].symbol();
            assert!(
                symbol == "┃" || symbol == "│",
                "pane {} missing right border at canvas ({right_x},{edge_y}) screen ({x},{y}) got {symbol:?}\n{dump}",
                info.id.raw()
            );
        }
    }

    #[test]
    fn watching_client_washes_pane_contents() {
        let mut app = AppState::test_new();
        app.zen_mode = true;
        let mut workspace = Workspace::test_new("watch");
        let root = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root,
            TerminalRuntime::test_with_scrollback_bytes(20, 8, 1024, b"hello\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(0, 0, 40, 12);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let mut controller = ClientViewState::from_default_client_state(&app);
        controller.zen_mode = true;
        let mut watcher = controller.clone();
        watcher.set_tab_control(crate::app::ClientTabControl::WatchingControlled { epoch: 3 });
        crate::ui::compute_view_for_client_without_resizing_panes(
            &app,
            &mut controller,
            &terminal_runtimes,
            area,
        );
        crate::ui::compute_view_for_client_without_resizing_panes(
            &app,
            &mut watcher,
            &terminal_runtimes,
            area,
        );

        let mut controller_terminal =
            Terminal::new(TestBackend::new(area.width, area.height)).expect("controller backend");
        controller_terminal
            .draw(|frame| {
                crate::ui::render_with_runtime_registry_for_view(
                    &app,
                    &controller,
                    &terminal_runtimes,
                    frame,
                );
            })
            .expect("render controller");
        let mut watcher_terminal =
            Terminal::new(TestBackend::new(area.width, area.height)).expect("watcher backend");
        watcher_terminal
            .draw(|frame| {
                crate::ui::render_with_runtime_registry_for_view(
                    &app,
                    &watcher,
                    &terminal_runtimes,
                    frame,
                );
            })
            .expect("render watcher");

        let pane = watcher.computed.pane_infos.first().expect("pane");
        let canvas = watcher.tab_canvas_view.expect("canvas");
        let (x, y) = canvas
            .canvas_to_screen(pane.inner_rect.x, pane.inner_rect.y)
            .expect("inner cell on screen");
        let controller_cell = &controller_terminal.backend().buffer()[(x, y)];
        let watcher_cell = &watcher_terminal.backend().buffer()[(x, y)];
        assert_eq!(
            watcher_cell.symbol(),
            controller_cell.symbol(),
            "watching wash should not erase glyphs"
        );
        assert_ne!(watcher_cell.symbol(), " ", "washed pane still has content");
        assert_ne!(
            watcher_cell.style().fg,
            Some(app.palette.panel_bg),
            "washed text must not collapse into the background"
        );
        assert_ne!(
            watcher_cell.style().fg,
            controller_cell.style().fg,
            "watching panes should mix toward muted chrome"
        );
    }

    #[test]
    fn main_empty_state_mentions_empty_active_workspace() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("empty");
        workspace.tabs.clear();
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let backend = TestBackend::new(72, 14);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_empty(&app, frame, Rect::new(0, 0, 72, 14)))
            .expect("render empty pane");

        let text = buffer_text(terminal.backend().buffer(), 72, 14);
        assert!(text.contains("No Tabs in This Space"));
        assert!(text.contains("The space is still here"));
        assert!(text.contains("Create a tab to keep working"));
    }

    #[test]
    fn main_empty_state_action_key_uses_empty_group_accent() {
        let mut app = AppState::test_new();
        app.palette.accent = Color::Rgb(1, 2, 3);
        app.keybinds.new_workspace = crate::config::ActionKeybinds::prefix("shift+n");
        app.workspaces = vec![Workspace::test_new("hidden")];
        let group_idx = app.create_group("work".to_string());
        app.set_group_accent(group_idx, Some(crate::config::TerminalAccent::Magenta));
        app.active_group = group_idx;
        app.group_filter_enabled = true;
        app.active = None;
        let expected_accent = app.group_accent_color(group_idx);
        assert_ne!(expected_accent, app.palette.accent);

        let backend = TestBackend::new(72, 14);
        let mut terminal = Terminal::new(backend).expect("test backend");
        terminal
            .draw(|frame| render_empty(&app, frame, Rect::new(0, 0, 72, 14)))
            .expect("render empty pane");

        let buffer = terminal.backend().buffer();
        let text = buffer_text(buffer, 72, 14);
        assert!(text.contains("No Spaces in This Group"));
        assert!(text.contains("Switch groups or create one here"));
        assert!(text.contains("Hidden spaces stay in the group menu"));
        let (x, y) = first_cell_with_text(buffer, 72, 14, "prefix+shift+n")
            .expect("new workspace action key");
        assert_eq!(buffer[(x, y)].style().fg, Some(expected_accent));
    }

    #[tokio::test]
    async fn pane_scrollbar_gutter_is_reserved_before_scrollback_exists() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(40, 8, 1024, b"ready\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(10, 3, 40, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            true,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, None);
        assert_eq!(
            app.workspaces[0].tabs[0].runtimes[&root_pane].current_size(),
            (area.height, area.width.saturating_sub(1))
        );
    }

    fn buffer_text(buffer: &Buffer, width: u16, height: u16) -> String {
        let mut text = String::new();
        for y in 0..height {
            for x in 0..width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    fn first_cell_with_text(
        buffer: &Buffer,
        width: u16,
        height: u16,
        text: &str,
    ) -> Option<(u16, u16)> {
        let target: Vec<char> = text.chars().collect();
        for y in 0..height {
            for x in 0..width.saturating_sub(target.len().saturating_sub(1) as u16) {
                let matches = target.iter().enumerate().all(|(idx, ch)| {
                    let mut encoded = [0; 4];
                    buffer[(x + idx as u16, y)].symbol() == ch.encode_utf8(&mut encoded)
                });
                if matches {
                    return Some((x, y));
                }
            }
        }
        None
    }

    #[tokio::test]
    async fn zoomed_pane_scrollbar_gutter_is_reserved_before_scrollback_exists() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        workspace.zoomed = true;
        let root_pane = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(40, 8, 1024, b"ready\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(10, 3, 40, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, None);
        assert_eq!(info.inner_rect, Rect::new(10, 3, 39, 8));
    }

    #[tokio::test]
    async fn zoomed_multi_pane_keeps_border_space() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let focused_pane = workspace.test_split(ratatui::layout::Direction::Horizontal);
        workspace.zoomed = true;
        workspace.tabs[0].runtimes.insert(
            focused_pane,
            TerminalRuntime::test_with_scrollback_bytes(40, 8, 1024, b"ready\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(10, 3, 40, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.id, focused_pane);
        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, None);
        assert_eq!(info.inner_rect, Rect::new(11, 4, 37, 6));
    }

    #[tokio::test]
    async fn tiny_pane_does_not_reserve_scrollbar_gutter() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(4, 8, 1024, b"ready\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(10, 3, 4, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, None);
        assert_eq!(info.inner_rect, area);
    }

    #[tokio::test]
    async fn pane_scrollbar_reserves_last_column_from_terminal_area() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(
                40,
                8,
                1024,
                b"one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
            ),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(10, 3, 40, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, Some(Rect::new(49, 3, 1, 8)));
        assert_eq!(info.inner_rect, Rect::new(10, 3, 39, 8));
    }

    #[tokio::test]
    async fn pane_scrollbar_setting_reclaims_reserved_column() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(
                40,
                8,
                1024,
                b"one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
            ),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.pane_scrollbars = false;

        let area = Rect::new(10, 3, 40, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, None);
        assert_eq!(info.inner_rect, area);
    }

    #[tokio::test]
    async fn scrollbar_gutter_and_pty_size_remain_stable_across_screen_transitions() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(
                40,
                8,
                1024,
                b"one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
            ),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(10, 3, 40, 8);
        let inner_rect = Rect::new(10, 3, 39, 8);
        let scrollbar_rect = Rect::new(49, 3, 1, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let primary = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            true,
            crate::kitty_graphics::HostCellSize::default(),
        );
        assert_eq!(primary[0].scrollbar_rect, Some(scrollbar_rect));
        assert_eq!(primary[0].inner_rect, inner_rect);
        assert_eq!(
            app.workspaces[0].tabs[0].runtimes[&root_pane].current_size(),
            (inner_rect.height, inner_rect.width)
        );

        for (screen_transition, expected_scrollbar) in [
            (b"\x1b[?1049h".as_slice(), None),
            (b"\x1b[?1049l".as_slice(), Some(scrollbar_rect)),
            (b"\x1b[?1049h".as_slice(), None),
            (b"\x1b[?1049l".as_slice(), Some(scrollbar_rect)),
        ] {
            app.workspaces[0].tabs[0]
                .runtimes
                .get(&root_pane)
                .expect("runtime")
                .test_process_pty_bytes(root_pane, screen_transition);
            let infos = compute_pane_infos(
                &app,
                &terminal_runtimes,
                area,
                true,
                crate::kitty_graphics::HostCellSize::default(),
            );

            assert_eq!(infos[0].scrollbar_rect, expected_scrollbar);
            assert_eq!(infos[0].inner_rect, inner_rect);
            assert_eq!(
                app.workspaces[0].tabs[0].runtimes[&root_pane].current_size(),
                (inner_rect.height, inner_rect.width)
            );
        }
    }

    #[tokio::test]
    async fn pane_border_renders_agent_status_with_focused_border_style() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let agent_pane = workspace.test_split(ratatui::layout::Direction::Horizontal);
        workspace.tabs[0].runtimes.insert(
            agent_pane,
            TerminalRuntime::test_with_scrollback_bytes(40, 8, 1024, b"ready\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.pane_border_agent_info = crate::config::PaneBorderAgentInfoConfig::NameAndStatus;
        app.ensure_test_terminals();

        let terminal_id = app.workspaces[0].tabs[0].panes[&agent_pane]
            .attached_terminal_id
            .clone();
        app.terminals
            .get_mut(&terminal_id)
            .expect("agent terminal state")
            .set_detected_state(Some(crate::detect::Agent::Claude), AgentState::Working);

        let width = 50;
        let height = 10;
        let area = Rect::new(0, 0, width, height);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        app.view.pane_infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let expected_accent = app.active_workspace_accent_color();
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test backend");

        terminal
            .draw(|frame| render_panes(&app, &terminal_runtimes, frame, area))
            .expect("render working agent pane");
        let (x, y) = first_cell_with_text(
            terminal.backend().buffer(),
            width,
            height,
            "claude · working",
        )
        .expect("working agent pane-border label");
        assert_eq!(
            terminal.backend().buffer()[(x, y)].style().fg,
            Some(expected_accent)
        );

        app.terminals
            .get_mut(&terminal_id)
            .expect("agent terminal state")
            .set_detected_state(Some(crate::detect::Agent::Claude), AgentState::Blocked);
        terminal
            .draw(|frame| render_panes(&app, &terminal_runtimes, frame, area))
            .expect("render blocked agent pane");
        let rendered = buffer_text(terminal.backend().buffer(), width, height);
        assert!(rendered.contains("claude · blocked"));
        assert!(!rendered.contains("claude · working"));
    }

    #[test]
    fn selection_highlight_uses_one_uniform_style() {
        let palette = Palette::catppuccin();
        let host_theme = crate::terminal_theme::TerminalTheme {
            foreground: None,
            background: Some(crate::terminal_theme::RgbColor {
                r: 12,
                g: 14,
                b: 16,
            }),
            ..crate::terminal_theme::TerminalTheme::default()
        };
        let expected_style = automatic_selection_style(&palette, host_theme);
        let selection = Some(Selection::range(PaneId::from_raw(1), 0, 0, 2, None));
        let backend = TestBackend::new(4, 1);
        let mut terminal = Terminal::new(backend).expect("test backend");

        terminal
            .draw(|frame| {
                let buf = frame.buffer_mut();
                buf[(0, 0)].set_style(
                    Style::default()
                        .fg(Color::Rgb(10, 220, 120))
                        .bg(Color::Black),
                );
                buf[(1, 0)].set_style(
                    Style::default()
                        .fg(Color::Rgb(220, 180, 40))
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                );
                buf[(2, 0)].set_style(Style::default().fg(Color::Blue).bg(Color::Reset));
                render_selection_highlight(
                    &selection,
                    frame,
                    PaneId::from_raw(1),
                    Rect::new(0, 0, 4, 1),
                    None,
                    &palette,
                    host_theme,
                );
            })
            .expect("render selection highlight");

        let buffer = terminal.backend().buffer();
        let first = buffer[(0, 0)].style();
        let second = buffer[(1, 0)].style();
        let third = buffer[(2, 0)].style();

        assert_eq!(first.fg, expected_style.fg);
        assert_eq!(second.fg, expected_style.fg);
        assert_eq!(third.fg, expected_style.fg);
        assert_eq!(first.bg, expected_style.bg);
        assert_eq!(second.bg, expected_style.bg);
        assert_eq!(third.bg, expected_style.bg);
        assert_eq!(first.add_modifier, expected_style.add_modifier);
        assert_eq!(second.add_modifier, expected_style.add_modifier);
        assert_eq!(third.add_modifier, expected_style.add_modifier);
        assert!(!second.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn automatic_selection_background_uses_host_background() {
        let bg = automatic_selection_bg(
            &Palette::terminal(),
            crate::terminal_theme::TerminalTheme {
                foreground: Some(crate::terminal_theme::RgbColor {
                    r: 230,
                    g: 230,
                    b: 230,
                }),
                background: Some(crate::terminal_theme::RgbColor {
                    r: 12,
                    g: 14,
                    b: 16,
                }),
                ..crate::terminal_theme::TerminalTheme::default()
            },
        );

        let Color::Rgb(r, g, b) = bg else {
            panic!("selection background should resolve to rgb");
        };
        assert!(relative_luminance((r, g, b)) > relative_luminance((12, 14, 16)));
    }
}
