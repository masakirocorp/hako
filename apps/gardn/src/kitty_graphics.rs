use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as FmtWrite;
use std::fs::OpenOptions;
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine;
use ratatui::layout::Rect;

use crate::app::state::AppState;
use crate::app::{ClientViewState, Mode};
use crate::ghostty::{KittyImageDescriptor, KittyImageFormat, KittyImagePlacement};
use crate::layout::PaneId;
use crate::terminal::TerminalRuntimeRegistry;

const KITTY_CHUNK_BYTES: usize = 3072;
const HOST_IMAGE_ID_BASE: u32 = 10_000;
const DIRECT_UPLOAD_MAX_BYTES: usize = 8 * 1024;
const TRANSMIT_FILE_TTL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageTransmit {
    #[cfg(test)]
    Direct,
    TempFile,
}

struct PendingTransmitFile {
    path: PathBuf,
    written_at: Instant,
}

static PENDING_TRANSMIT_FILES: Mutex<Vec<PendingTransmitFile>> = Mutex::new(Vec::new());

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct HostCellSize {
    pub width_px: u32,
    pub height_px: u32,
}

impl HostCellSize {
    pub(crate) fn from_terminal(area: Rect) -> Self {
        let Ok(size) = crossterm::terminal::window_size() else {
            return Self::fallback_for_area(area);
        };
        if size.columns == 0 || size.rows == 0 {
            return Self::fallback_for_area(area);
        }
        if size.width == 0 || size.height == 0 {
            return Self::fallback_for_area(area);
        }
        Self {
            width_px: (size.width as u32 / size.columns as u32).max(1),
            height_px: (size.height as u32 / size.rows as u32).max(1),
        }
        .for_area(area)
    }

    pub(crate) fn is_known(self) -> bool {
        self.width_px > 0 && self.height_px > 0
    }

    fn fallback_for_area(area: Rect) -> Self {
        Self {
            width_px: 8,
            height_px: 16,
        }
        .for_area(area)
    }

    fn for_area(self, area: Rect) -> Self {
        if area.width == 0 || area.height == 0 {
            return Self::default();
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HostViewKey {
    workspace_index: usize,
    tab_index: usize,
}

#[derive(Debug)]
struct HostPlacement {
    pane_id: PaneId,
    area: Rect,
    cell_size: HostCellSize,
    placement: KittyImagePlacement,
    scrollback_offset: u32,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
struct ImageSignature {
    image_width: u32,
    image_height: u32,
    format_code: u32,
    data_len: usize,
    data_fingerprint: u64,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
struct PlacementSignature {
    x: u16,
    y: u16,
    cols: u32,
    rows: u32,
    source_x: u32,
    source_y: u32,
    source_width: u32,
    source_height: u32,
    x_offset: u32,
    y_offset: u32,
    z: i32,
    scrollback_offset: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ClippedPlacement {
    x: u16,
    y: u16,
    cols: u32,
    rows: u32,
    source_x: u32,
    source_y: u32,
    source_width: u32,
    source_height: u32,
    x_offset: u32,
    y_offset: u32,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct HostGraphicsCache {
    images: HashMap<u32, ImageSignature>,
    placements: HashMap<(u32, u32), PlacementSignature>,
    sources: HashMap<(PaneId, u32), u32>,
    view: Option<HostViewKey>,
}

static KITTY_GRAPHICS_ENABLED: AtomicBool = AtomicBool::new(false);
static LOCAL_HOST_GRAPHICS: OnceLock<Mutex<HostGraphicsCache>> = OnceLock::new();

pub(crate) fn set_enabled(enabled: bool) {
    KITTY_GRAPHICS_ENABLED.store(enabled, Ordering::Release);
}

pub(crate) fn is_enabled() -> bool {
    KITTY_GRAPHICS_ENABLED.load(Ordering::Acquire)
}

pub(crate) fn paint_local_pane_graphics(
    app: &AppState,
    obscured_pane: Option<PaneId>,
    graphics: &mut crate::app::pane_graphics::Runtime,
    terminal_runtimes: &TerminalRuntimeRegistry,
    cell_size: HostCellSize,
) -> io::Result<()> {
    let cache = LOCAL_HOST_GRAPHICS.get_or_init(|| Mutex::new(HostGraphicsCache::default()));
    let mut bytes = Vec::new();
    if let Ok(mut cache) = cache.lock() {
        bytes = encode_local_pane_graphics(
            app,
            obscured_pane,
            graphics,
            terminal_runtimes,
            cell_size,
            &mut cache,
        );
    }
    if bytes.is_empty() {
        return Ok(());
    }

    let mut framed = Vec::with_capacity(bytes.len() + 8);
    framed.extend_from_slice(b"\x1b7");
    framed.extend_from_slice(&bytes);
    framed.extend_from_slice(b"\x1b8");

    let mut stdout = io::stdout().lock();
    stdout.write_all(&framed)?;
    stdout.flush()
}

pub(crate) fn encode_local_pane_graphics(
    app: &AppState,
    obscured_pane: Option<PaneId>,
    graphics: &mut crate::app::pane_graphics::Runtime,
    terminal_runtimes: &TerminalRuntimeRegistry,
    cell_size: HostCellSize,
    cache: &mut HostGraphicsCache,
) -> Vec<u8> {
    let mode_ok = matches!(app.mode, Mode::Terminal | Mode::Github);
    let cell_ok = cell_size.is_known();
    tracing::debug!(
        mode_ok,
        cell_ok,
        cell_width_px = cell_size.width_px,
        cell_height_px = cell_size.height_px,
        active = ?app.active,
        pane_infos_len = app.view.pane_infos.len(),
        "paint_local_pane_graphics entry"
    );
    if !mode_ok || !cell_ok {
        tracing::debug!(
            reason = if !mode_ok {
                "not terminal mode"
            } else {
                "cell size unknown"
            },
            "paint_local_pane_graphics early return"
        );
        return cache.clear_bytes();
    }

    let view_key = active_view_key(app);
    let blit_pane = focused_graphics_blit_pane(app);
    let placements = collect_visible_placements(
        app,
        graphics,
        terminal_runtimes,
        cell_size,
        &cache.images,
        blit_pane,
        obscured_pane,
    );
    tracing::debug!(
        placements_collected = placements.len(),
        "collect_visible_placements result"
    );

    let mut bytes = Vec::new();
    let view_changed = cache.update_view(view_key);
    encode_graphics_update_with(
        &mut bytes,
        &placements,
        view_changed,
        &mut cache.images,
        &mut cache.placements,
        &mut cache.sources,
        ImageTransmit::TempFile,
    );
    tracing::debug!(
        placements = placements.len(),
        bytes = bytes.len(),
        cell_width_px = cell_size.width_px,
        cell_height_px = cell_size.height_px,
        "painting kitty graphics placements"
    );
    bytes
}

pub(crate) fn encode_local_pane_graphics_for_view(
    app: &AppState,
    view: &ClientViewState,
    graphics: &mut crate::app::pane_graphics::Runtime,
    terminal_runtimes: &TerminalRuntimeRegistry,
    cell_size: HostCellSize,
    cache: &mut HostGraphicsCache,
) -> Vec<u8> {
    let mode_ok = matches!(view.mode, Mode::Terminal | Mode::Github);
    let cell_ok = cell_size.is_known();
    tracing::debug!(
        mode_ok,
        cell_ok,
        cell_width_px = cell_size.width_px,
        cell_height_px = cell_size.height_px,
        active = ?view.active_workspace,
        pane_infos_len = view.computed.pane_infos.len(),
        "paint_local_pane_graphics_for_view entry"
    );
    if !mode_ok || !cell_ok {
        return cache.clear_bytes();
    }

    let view_key = active_view_key_for_view(app, view);
    let blit_pane = focused_graphics_blit_pane_for_view(app, view);
    let placements = collect_visible_placements_for_view(
        app,
        view,
        graphics,
        terminal_runtimes,
        cell_size,
        &cache.images,
        blit_pane,
    );

    let mut bytes = Vec::new();
    let view_changed = cache.update_view(view_key);
    encode_graphics_update_with(
        &mut bytes,
        &placements,
        view_changed,
        &mut cache.images,
        &mut cache.placements,
        &mut cache.sources,
        ImageTransmit::TempFile,
    );
    bytes
}

#[cfg(test)]
fn encode_graphics_update(
    bytes: &mut Vec<u8>,
    placements: &[HostPlacement],
    view_changed: bool,
    host_images: &mut HashMap<u32, ImageSignature>,
    host_placements: &mut HashMap<(u32, u32), PlacementSignature>,
    sources: &mut HashMap<(PaneId, u32), u32>,
) {
    encode_graphics_update_with(
        bytes,
        placements,
        view_changed,
        host_images,
        host_placements,
        sources,
        ImageTransmit::Direct,
    );
}

fn encode_graphics_update_with(
    bytes: &mut Vec<u8>,
    placements: &[HostPlacement],
    view_changed: bool,
    host_images: &mut HashMap<u32, ImageSignature>,
    host_placements: &mut HashMap<(u32, u32), PlacementSignature>,
    sources: &mut HashMap<(PaneId, u32), u32>,
    transmit: ImageTransmit,
) {
    let current_sources: HashSet<(PaneId, u32)> = placements
        .iter()
        .map(|placement| (placement.pane_id, placement.placement.image_id))
        .collect();
    sources.retain(|source, _| current_sources.contains(source));

    let mut current_placements = HashSet::new();
    for placement in placements {
        let clipped = clipped_placement(placement);
        tracing::debug!(
            pane_id = ?placement.pane_id,
            has_clipped = clipped.is_some(),
            grid_cols = placement.placement.render.grid_cols,
            grid_rows = placement.placement.render.grid_rows,
            viewport_col = placement.placement.render.viewport_col,
            viewport_row = placement.placement.render.viewport_row,
            area_w = placement.area.width,
            area_h = placement.area.height,
            "clipped_placement result"
        );
        let Some((clipped, format_code)) = clipped else {
            continue;
        };
        let host_id = host_image_id(placement.pane_id, &placement.placement);
        let host_placement_id = host_placement_id(placement.pane_id, &placement.placement);
        let image_signature = image_signature(placement, format_code);
        let placement_signature =
            placement_signature(clipped, placement.placement.z, placement.scrollback_offset);
        let placement_key = (host_id, host_placement_id);
        current_placements.insert(placement_key);

        match host_images.get(&host_id).copied() {
            Some(existing) if existing == image_signature => {}
            Some(_) => {
                encode_delete_image(bytes, host_id);
                host_placements.retain(|(image_id, placement_id), _| {
                    if *image_id == host_id {
                        current_placements.remove(&(*image_id, *placement_id));
                        false
                    } else {
                        true
                    }
                });
                if !encode_upload_image(bytes, placement, format_code, host_id, transmit) {
                    continue;
                }
                host_images.insert(host_id, image_signature);
            }
            None => {
                if !encode_upload_image(bytes, placement, format_code, host_id, transmit) {
                    continue;
                }
                host_images.insert(host_id, image_signature);
            }
        }

        release_superseded_source_image(
            bytes,
            sources,
            host_images,
            host_placements,
            &mut current_placements,
            (placement.pane_id, placement.placement.image_id),
            host_id,
        );

        // A different view can repaint the same cells with text or overlays and
        // leave the host-side Kitty placement state out of sync with this cache.
        // Re-emit the placement even when its geometry signature is unchanged.
        match host_placements.get_mut(&placement_key) {
            Some(existing) if !view_changed && *existing == placement_signature => {}
            Some(existing) => {
                encode_display_placement(
                    bytes,
                    clipped,
                    host_id,
                    host_placement_id,
                    placement.placement.z,
                );
                *existing = placement_signature;
            }
            None => {
                encode_display_placement(
                    bytes,
                    clipped,
                    host_id,
                    host_placement_id,
                    placement.placement.z,
                );
                host_placements.insert(placement_key, placement_signature);
            }
        }
    }

    let mut stale_placements = Vec::new();
    for key in host_placements.keys() {
        if current_placements.contains(key) {
            continue;
        }
        stale_placements.push(*key);
    }
    for (host_id, host_placement_id) in stale_placements {
        encode_delete_placement(bytes, host_id, host_placement_id);
        host_placements.remove(&(host_id, host_placement_id));
    }
}

/// Records that `source` is now backed by `host_id` and deletes the host
/// image it previously pointed at once no other source references it.
fn release_superseded_source_image(
    bytes: &mut Vec<u8>,
    sources: &mut HashMap<(PaneId, u32), u32>,
    host_images: &mut HashMap<u32, ImageSignature>,
    host_placements: &mut HashMap<(u32, u32), PlacementSignature>,
    current_placements: &mut HashSet<(u32, u32)>,
    source: (PaneId, u32),
    host_id: u32,
) {
    let Some(previous) = sources.insert(source, host_id) else {
        return;
    };
    if previous == host_id || sources.values().any(|id| *id == previous) {
        return;
    }
    encode_delete_image(bytes, previous);
    host_images.remove(&previous);
    host_placements.retain(|(image_id, placement_id), _| {
        if *image_id == previous {
            current_placements.remove(&(*image_id, *placement_id));
            false
        } else {
            true
        }
    });
}

pub(crate) fn clear_all_host_graphics() -> io::Result<()> {
    let cache = LOCAL_HOST_GRAPHICS.get_or_init(|| Mutex::new(HostGraphicsCache::default()));
    let mut bytes = Vec::new();
    if let Ok(mut cache) = cache.lock() {
        bytes = cache.clear_bytes();
    }
    if bytes.is_empty() {
        return Ok(());
    }
    let mut stdout = io::stdout().lock();
    stdout.write_all(&bytes)?;
    stdout.flush()
}

impl HostGraphicsCache {
    pub(crate) fn clear_bytes(&mut self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for id in self.images.keys().copied().collect::<Vec<_>>() {
            encode_delete_image(&mut bytes, id);
        }
        self.images.clear();
        self.placements.clear();
        self.sources.clear();
        self.view = None;
        bytes
    }

    fn update_view(&mut self, view_key: Option<HostViewKey>) -> bool {
        if self.view == view_key {
            return false;
        }
        self.view = view_key;
        true
    }
}

fn active_view_key(app: &AppState) -> Option<HostViewKey> {
    let ws_idx = app.active?;
    let ws = app.workspaces.get(ws_idx)?;
    Some(HostViewKey {
        workspace_index: ws_idx,
        tab_index: ws.active_tab_index(),
    })
}

fn active_view_key_for_view(app: &AppState, view: &ClientViewState) -> Option<HostViewKey> {
    let ws_idx = view.active_workspace?;
    let ws = app.workspaces.get(ws_idx)?;
    Some(HostViewKey {
        workspace_index: ws_idx,
        tab_index: view
            .active_tab_for_workspace(&ws.id)
            .unwrap_or_else(|| ws.active_tab_index()),
    })
}

fn focused_graphics_blit_pane(app: &AppState) -> Option<PaneId> {
    if app.mode != Mode::Terminal {
        return None;
    }
    let workspace = app.workspaces.get(app.active?)?;
    let tab = workspace.active_tab()?;
    if tab.layout.pane_ids().len() != 1 && !tab.zoomed {
        return None;
    }
    Some(tab.layout.focused())
}

fn focused_graphics_blit_pane_for_view(app: &AppState, view: &ClientViewState) -> Option<PaneId> {
    if view.mode != Mode::Terminal {
        return None;
    }
    let ws_idx = view.active_workspace?;
    let workspace = app.workspaces.get(ws_idx)?;
    let tab_idx = view.active_tab_for_workspace(&workspace.id)?;
    let tab = workspace.tabs.get(tab_idx)?;
    if tab.layout.pane_ids().len() != 1 && !tab.zoomed {
        return None;
    }
    Some(
        view.focused_pane_for_tab(&workspace.id, tab.number)
            .unwrap_or_else(|| tab.layout.focused()),
    )
}

fn project_host_placement(
    pane_id: PaneId,
    canonical_area: Rect,
    source: Rect,
    destination: Rect,
    cell_size: HostCellSize,
    mut placement: KittyImagePlacement,
    scrollback_offset: u32,
) -> Option<HostPlacement> {
    if canonical_area.width == 0
        || canonical_area.height == 0
        || source.width == 0
        || source.height == 0
        || destination.width == 0
        || destination.height == 0
        || source.width != destination.width
        || source.height != destination.height
    {
        return None;
    }

    let source_col = source.x.checked_sub(canonical_area.x)?;
    let source_row = source.y.checked_sub(canonical_area.y)?;
    if source_col >= canonical_area.width
        || source_row >= canonical_area.height
        || (source_col as u32).saturating_add(source.width as u32) > canonical_area.width as u32
        || (source_row as u32).saturating_add(source.height as u32) > canonical_area.height as u32
    {
        return None;
    }

    placement.render.viewport_col = placement
        .render
        .viewport_col
        .saturating_sub(source_col as i32);
    placement.render.viewport_row = placement
        .render
        .viewport_row
        .saturating_sub(source_row as i32);

    Some(HostPlacement {
        pane_id,
        area: destination,
        cell_size,
        placement,
        scrollback_offset,
    })
}

fn collect_visible_placements_for_view(
    app: &AppState,
    view: &ClientViewState,
    graphics: &mut crate::app::pane_graphics::Runtime,
    terminal_runtimes: &TerminalRuntimeRegistry,
    cell_size: HostCellSize,
    uploaded_images: &HashMap<u32, ImageSignature>,
    blit_pane: Option<PaneId>,
) -> Vec<HostPlacement> {
    let Some(ws_idx) = view.active_workspace else {
        return Vec::new();
    };
    if app.workspaces.get(ws_idx).is_none() {
        return Vec::new();
    }

    let mut placements = Vec::new();
    let Some(workspace) = app.workspaces.get(ws_idx) else {
        return Vec::new();
    };
    for info in &view.computed.pane_infos {
        if view.github.is_some() && view.github_pane_id == Some(info.id) {
            continue;
        }
        if blit_pane.is_some_and(|pane_id| pane_id != info.id) {
            continue;
        }
        let Some(terminal_id) = workspace.terminal_id(info.id) else {
            continue;
        };
        let Some(runtime) = terminal_runtimes.get(terminal_id) else {
            continue;
        };
        let mut copied_images = HashSet::new();
        for placement in runtime.kitty_image_placements_with_data_filter(|descriptor| {
            needs_host_image_data(info.id, descriptor, uploaded_images, &mut copied_images)
        }) {
            let scrollback_offset = view
                .terminal_offsets_from_bottom
                .get(terminal_id)
                .map(|offset| offset.offset_from_bottom)
                .or_else(|| runtime.scroll_metrics().map(|m| m.offset_from_bottom))
                .map(|offset| offset as u32)
                .unwrap_or(0);
            let Some(projected) = view
                .tab_canvas_view
                .as_ref()
                .and_then(|canvas| canvas.project_rect(info.inner_rect))
            else {
                continue;
            };
            let Some(placement) = project_host_placement(
                info.id,
                info.inner_rect,
                projected.source,
                projected.destination,
                cell_size,
                placement,
                scrollback_offset,
            ) else {
                continue;
            };
            placements.push(placement);
        }
        append_pane_graphics_placements(
            graphics,
            info.id,
            info.inner_rect,
            cell_size,
            uploaded_images,
            &mut placements,
        );
    }
    placements
}

fn collect_visible_placements(
    app: &AppState,
    graphics: &mut crate::app::pane_graphics::Runtime,
    terminal_runtimes: &TerminalRuntimeRegistry,
    cell_size: HostCellSize,
    uploaded_images: &HashMap<u32, ImageSignature>,
    blit_pane: Option<PaneId>,
    obscured_pane: Option<PaneId>,
) -> Vec<HostPlacement> {
    let ws_idx = match app.active {
        Some(idx) => idx,
        None => {
            tracing::debug!("collect_visible_placements: no active workspace");
            return Vec::new();
        }
    };
    if app
        .workspaces
        .get(ws_idx)
        .and_then(crate::workspace::Workspace::active_tab)
        .is_none()
    {
        tracing::debug!(ws_idx, "collect_visible_placements: no active tab");
        return Vec::new();
    }

    tracing::debug!(
        ws_idx,
        terminal_runtimes_len = terminal_runtimes.len(),
        pane_infos_len = app.view.pane_infos.len(),
        "collect_visible_placements: starting iteration"
    );
    let mut placements = Vec::new();
    for info in &app.view.pane_infos {
        if obscured_pane == Some(info.id) {
            continue;
        }
        if blit_pane.is_some_and(|pane_id| pane_id != info.id) {
            continue;
        }
        let runtime = match app.runtime_for_pane_in_workspace(terminal_runtimes, ws_idx, info.id) {
            Some(rt) => rt,
            None => {
                tracing::debug!(pane_id = ?info.id, "collect_visible_placements: runtime not found");
                continue;
            }
        };
        let mut copied_images = HashSet::new();
        for placement in runtime.kitty_image_placements_with_data_filter(|descriptor| {
            needs_host_image_data(info.id, descriptor, uploaded_images, &mut copied_images)
        }) {
            let scrollback_offset = runtime
                .scroll_metrics()
                .map(|m| m.offset_from_bottom as u32)
                .unwrap_or(0);
            placements.push(HostPlacement {
                pane_id: info.id,
                area: info.inner_rect,
                cell_size,
                placement,
                scrollback_offset,
            });
        }
        append_pane_graphics_placements(
            graphics,
            info.id,
            info.inner_rect,
            cell_size,
            uploaded_images,
            &mut placements,
        );
    }
    tracing::debug!(
        placements_len = placements.len(),
        "collect_visible_placements: done"
    );
    placements
}

fn append_pane_graphics_placements(
    graphics: &mut crate::app::pane_graphics::Runtime,
    pane_id: PaneId,
    area: Rect,
    cell_size: HostCellSize,
    uploaded_images: &HashMap<u32, ImageSignature>,
    placements: &mut Vec<HostPlacement>,
) {
    if !graphics.active_for_pane(pane_id) {
        return;
    }
    let _ = graphics.revision();
    let mut keys: Vec<_> = graphics
        .slots
        .iter()
        .filter(|((id, _), slot)| *id == pane_id && slot.layer.is_some())
        .map(|(key, _)| key.clone())
        .collect();
    keys.sort_by(|left, right| {
        let left_z = graphics.slots[left]
            .layer
            .as_ref()
            .map(|layer| layer.z_index);
        let right_z = graphics.slots[right]
            .layer
            .as_ref()
            .map(|layer| layer.z_index);
        left_z.cmp(&right_z)
    });
    for key in keys {
        let Some(slot) = graphics.slots.get_mut(&key) else {
            continue;
        };
        let host_image_id = slot.host_image_id;
        let Some(layer) = slot.layer.as_mut() else {
            continue;
        };
        let Some(placement) = pane_graphics_host_placement(
            pane_id,
            area,
            cell_size,
            host_image_id,
            layer,
            uploaded_images,
        ) else {
            continue;
        };
        if let Some(lease) = layer.direct_lease() {
            let _ = (lease.path(), lease.len());
            let _ = layer.mark_resident(0);
        }
        placements.push(placement);
    }
}

fn pane_graphics_host_placement(
    pane_id: PaneId,
    area: Rect,
    cell_size: HostCellSize,
    host_image_id: u32,
    layer: &crate::app::pane_graphics::Layer,
    uploaded_images: &HashMap<u32, ImageSignature>,
) -> Option<HostPlacement> {
    let format = pane_graphics_kitty_format(layer.format)?;
    let format_code = kitty_format_code(format);
    let signature = ImageSignature {
        image_width: layer.image_width,
        image_height: layer.image_height,
        format_code,
        data_len: layer.data_len(),
        data_fingerprint: layer.data_fingerprint,
    };
    let data = if uploaded_images.get(&host_image_id).copied() == Some(signature) {
        Vec::new()
    } else if let Some(inline) = layer.inline_data() {
        inline.to_vec()
    } else if let Some(lease) = layer.direct_lease() {
        lease.copy_rgba().unwrap_or_default()
    } else {
        Vec::new()
    };
    Some(HostPlacement {
        pane_id,
        area,
        cell_size,
        placement: KittyImagePlacement {
            image_id: host_image_id,
            placement_id: 1,
            z: layer.z_index,
            x_offset: 0,
            y_offset: 0,
            image_width: layer.image_width,
            image_height: layer.image_height,
            format,
            data_len: layer.data_len(),
            data_fingerprint: layer.data_fingerprint,
            data,
            render: crate::ghostty::KittyPlacementRenderInfo {
                pixel_width: layer.image_width,
                pixel_height: layer.image_height,
                grid_cols: layer.render.grid_cols,
                grid_rows: layer.render.grid_rows,
                viewport_col: layer.render.viewport_col,
                viewport_row: layer.render.viewport_row,
                source_x: 0,
                source_y: 0,
                source_width: 0,
                source_height: 0,
            },
        },
        scrollback_offset: 0,
    })
}

fn pane_graphics_kitty_format(
    format: crate::api::schema::PaneGraphicsFormat,
) -> Option<KittyImageFormat> {
    match format {
        crate::api::schema::PaneGraphicsFormat::Png => Some(KittyImageFormat::Png),
        crate::api::schema::PaneGraphicsFormat::Rgb => Some(KittyImageFormat::Rgb),
        crate::api::schema::PaneGraphicsFormat::Rgba
        | crate::api::schema::PaneGraphicsFormat::Bgra => Some(KittyImageFormat::Rgba),
    }
}

fn host_image_id(pane_id: PaneId, placement: &KittyImagePlacement) -> u32 {
    let format_code = kitty_format_code(placement.format);
    host_image_id_for_signature(
        pane_id,
        ImageSignature {
            image_width: placement.image_width,
            image_height: placement.image_height,
            format_code,
            data_len: placement.data_len,
            data_fingerprint: placement.data_fingerprint,
        },
    )
}

fn host_image_id_for_signature(pane_id: PaneId, signature: ImageSignature) -> u32 {
    let mut hasher = DefaultHasher::new();
    pane_id.raw().hash(&mut hasher);
    signature.hash(&mut hasher);
    HOST_IMAGE_ID_BASE + ((hasher.finish() as u32) % 900_000)
}

fn host_placement_id(pane_id: PaneId, placement: &KittyImagePlacement) -> u32 {
    let mut hasher = DefaultHasher::new();
    pane_id.raw().hash(&mut hasher);
    placement.image_id.hash(&mut hasher);
    placement.placement_id.hash(&mut hasher);
    1 + ((hasher.finish() as u32) % 900_000)
}

pub(crate) fn encode_kitty_regular_file(
    out: &mut Vec<u8>,
    leading: &[u8],
    control: &str,
    path: &str,
) {
    let payload = base64::engine::general_purpose::STANDARD.encode(path.as_bytes());
    out.extend_from_slice(b"\x1b7");
    out.extend_from_slice(leading);
    let _ = write!(out, "\x1b_G{control},t=f;{payload}\x1b\\");
    out.extend_from_slice(b"\x1b8");
}

fn encode_delete_image(out: &mut Vec<u8>, id: u32) {
    let _ = write!(out, "\x1b_Ga=d,d=I,i={id},q=2;\x1b\\");
}

fn encode_delete_placement(out: &mut Vec<u8>, host_id: u32, host_placement_id: u32) {
    let _ = write!(
        out,
        "\x1b_Ga=d,d=i,i={host_id},p={host_placement_id},q=2;\x1b\\"
    );
}

fn encode_upload_image(
    out: &mut Vec<u8>,
    placement: &HostPlacement,
    format_code: u32,
    host_id: u32,
    transmit: ImageTransmit,
) -> bool {
    if placement.placement.data.is_empty() {
        return false;
    }

    if transmit == ImageTransmit::TempFile
        && placement.placement.data.len() > DIRECT_UPLOAD_MAX_BYTES
    {
        match write_temp_image(&placement.placement.data) {
            Ok(path) => {
                if let Some(path_str) = path.to_str() {
                    let control = format!(
                        "a=t,t=t,f={format_code},s={},v={},i={host_id},q=2",
                        placement.placement.image_width, placement.placement.image_height,
                    );
                    encode_kitty_data(out, &control, path_str.as_bytes());
                    return true;
                }
            }
            Err(err) => {
                tracing::warn!(
                    err = %err,
                    host_id,
                    bytes = placement.placement.data.len(),
                    "falling back to direct Kitty upload after temp file write failed"
                );
            }
        }
    }

    let control = format!(
        "a=t,t=d,f={format_code},s={},v={},i={host_id},q=2",
        placement.placement.image_width, placement.placement.image_height,
    );
    encode_kitty_data(out, &control, &placement.placement.data);
    true
}

fn write_temp_image(data: &[u8]) -> io::Result<PathBuf> {
    sweep_stale_transmit_files();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "tty-graphics-protocol-gardn-{}-{nanos}.img",
        std::process::id()
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(&path)?.write_all(data)?;
    if let Ok(mut pending) = PENDING_TRANSMIT_FILES.lock() {
        pending.push(PendingTransmitFile {
            path: path.clone(),
            written_at: Instant::now(),
        });
    }
    Ok(path)
}

fn sweep_stale_transmit_files() {
    let Ok(mut pending) = PENDING_TRANSMIT_FILES.lock() else {
        return;
    };
    let now = Instant::now();
    pending.retain(|file| {
        if now.saturating_duration_since(file.written_at) < TRANSMIT_FILE_TTL {
            return true;
        }
        let _ = std::fs::remove_file(&file.path);
        false
    });
}

fn encode_display_placement(
    out: &mut Vec<u8>,
    clipped: ClippedPlacement,
    host_id: u32,
    host_placement_id: u32,
    z: i32,
) {
    let _ = write!(out, "\x1b[{};{}H", clipped.y + 1, clipped.x + 1);
    let mut control = format!(
        "a=p,i={host_id},p={host_placement_id},c={},r={},z={z},C=1,q=2",
        clipped.cols, clipped.rows,
    );
    if clipped.source_x > 0 {
        let _ = write!(control, ",x={}", clipped.source_x);
    }
    if clipped.source_y > 0 {
        let _ = write!(control, ",y={}", clipped.source_y);
    }
    if clipped.source_width > 0 {
        let _ = write!(control, ",w={}", clipped.source_width);
    }
    if clipped.source_height > 0 {
        let _ = write!(control, ",h={}", clipped.source_height);
    }
    if clipped.x_offset > 0 {
        let _ = write!(control, ",X={}", clipped.x_offset);
    }
    if clipped.y_offset > 0 {
        let _ = write!(control, ",Y={}", clipped.y_offset);
    }

    let _ = write!(out, "\x1b_G{control};\x1b\\");
}

fn clipped_placement(placement: &HostPlacement) -> Option<(ClippedPlacement, u32)> {
    if placement.area.width == 0 || placement.area.height == 0 {
        tracing::debug!(
            area_w = placement.area.width,
            area_h = placement.area.height,
            "clipped_placement: area zero"
        );
        return None;
    }
    let render = placement.placement.render;
    if render.grid_cols == 0 || render.grid_rows == 0 {
        tracing::debug!(
            grid_cols = render.grid_cols,
            grid_rows = render.grid_rows,
            "clipped_placement: grid zero"
        );
        return None;
    }
    let format_code = kitty_format_code(placement.placement.format);

    let left_clip_cells = if render.viewport_col < 0 {
        render.viewport_col.saturating_neg() as u32
    } else {
        0
    };
    let viewport_row = render
        .viewport_row
        .saturating_add(placement.scrollback_offset.min(i32::MAX as u32) as i32);
    let top_clip_cells = if viewport_row < 0 {
        viewport_row.saturating_neg() as u32
    } else {
        0
    };
    let viewport_col = render.viewport_col.max(0) as u32;
    let viewport_row = viewport_row.max(0) as u32;
    tracing::debug!(
        viewport_col = viewport_col,
        viewport_row = viewport_row,
        area_w = placement.area.width,
        area_h = placement.area.height,
        scrollback_offset = placement.scrollback_offset,
        raw_viewport_row = render.viewport_row,
        cond1 = viewport_col >= placement.area.width as u32,
        cond2 = viewport_row >= placement.area.height as u32,
        "clipped_placement: viewport check"
    );
    if viewport_col >= placement.area.width as u32 || viewport_row >= placement.area.height as u32 {
        return None;
    }

    let visible_cols = render
        .grid_cols
        .saturating_sub(left_clip_cells)
        .min(placement.area.width as u32 - viewport_col);
    let visible_rows = render
        .grid_rows
        .saturating_sub(top_clip_cells)
        .min(placement.area.height as u32 - viewport_row);
    tracing::debug!(
        visible_cols = visible_cols,
        visible_rows = visible_rows,
        left_clip_cells = left_clip_cells,
        top_clip_cells = top_clip_cells,
        "clipped_placement: visible dims check"
    );
    if visible_cols == 0 || visible_rows == 0 {
        return None;
    }

    let source_width = if render.source_width == 0 {
        placement.placement.image_width
    } else {
        render.source_width
    };
    let source_height = if render.source_height == 0 {
        placement.placement.image_height
    } else {
        render.source_height
    };
    let pixel_width = render
        .pixel_width
        .max(
            render
                .grid_cols
                .saturating_mul(placement.cell_size.width_px),
        )
        .max(1);
    let pixel_height = render
        .pixel_height
        .max(
            render
                .grid_rows
                .saturating_mul(placement.cell_size.height_px),
        )
        .max(1);

    let crop_left_px = left_clip_cells.saturating_mul(placement.cell_size.width_px);
    let crop_top_px = top_clip_cells.saturating_mul(placement.cell_size.height_px);
    let visible_width_px = visible_cols.saturating_mul(placement.cell_size.width_px);
    let visible_height_px = visible_rows.saturating_mul(placement.cell_size.height_px);

    let source_x = render.source_x + scale_pixels(crop_left_px, source_width, pixel_width);
    let source_y = render.source_y + scale_pixels(crop_top_px, source_height, pixel_height);
    let source_width = scale_pixels(visible_width_px, source_width, pixel_width)
        .max(1)
        .min(placement.placement.image_width.saturating_sub(source_x));
    let source_height = scale_pixels(visible_height_px, source_height, pixel_height)
        .max(1)
        .min(placement.placement.image_height.saturating_sub(source_y));

    if source_width == 0 || source_height == 0 {
        tracing::debug!(
            source_width = source_width,
            source_height = source_height,
            image_width = placement.placement.image_width,
            image_height = placement.placement.image_height,
            "clipped_placement: source dims zero"
        );
        return None;
    }

    tracing::debug!("clipped_placement: success");
    Some((
        ClippedPlacement {
            x: placement.area.x + viewport_col as u16,
            y: placement.area.y + viewport_row as u16,
            cols: visible_cols,
            rows: visible_rows,
            source_x,
            source_y,
            source_width,
            source_height,
            x_offset: if left_clip_cells == 0 {
                placement.placement.x_offset
            } else {
                0
            },
            y_offset: if top_clip_cells == 0 {
                placement.placement.y_offset
            } else {
                0
            },
        },
        format_code,
    ))
}

fn scale_pixels(value: u32, source: u32, dest: u32) -> u32 {
    ((value as u64).saturating_mul(source as u64) / dest.max(1) as u64).min(u32::MAX as u64) as u32
}

fn image_signature(placement: &HostPlacement, format_code: u32) -> ImageSignature {
    ImageSignature {
        image_width: placement.placement.image_width,
        image_height: placement.placement.image_height,
        format_code,
        data_len: placement.placement.data_len,
        data_fingerprint: placement.placement.data_fingerprint,
    }
}

fn image_signature_from_descriptor(
    descriptor: KittyImageDescriptor,
    format_code: u32,
) -> ImageSignature {
    ImageSignature {
        image_width: descriptor.image_width,
        image_height: descriptor.image_height,
        format_code,
        data_len: descriptor.data_len,
        data_fingerprint: descriptor.data_fingerprint,
    }
}

fn needs_host_image_data(
    pane_id: PaneId,
    descriptor: KittyImageDescriptor,
    uploaded_images: &HashMap<u32, ImageSignature>,
    copied_images: &mut HashSet<u32>,
) -> bool {
    let format_code = kitty_format_code(descriptor.format);
    let signature = image_signature_from_descriptor(descriptor, format_code);
    let host_id = host_image_id_for_signature(pane_id, signature);
    if uploaded_images.get(&host_id).copied() == Some(signature) {
        return false;
    }
    copied_images.insert(descriptor.image_id)
}

fn placement_signature(
    clipped: ClippedPlacement,
    z: i32,
    scrollback_offset: u32,
) -> PlacementSignature {
    PlacementSignature {
        x: clipped.x,
        y: clipped.y,
        cols: clipped.cols,
        rows: clipped.rows,
        source_x: clipped.source_x,
        source_y: clipped.source_y,
        source_width: clipped.source_width,
        source_height: clipped.source_height,
        x_offset: clipped.x_offset,
        y_offset: clipped.y_offset,
        z,
        scrollback_offset,
    }
}

fn kitty_format_code(format: KittyImageFormat) -> u32 {
    match format {
        KittyImageFormat::Rgb => 24,
        KittyImageFormat::Rgba => 32,
        KittyImageFormat::Png => 100,
    }
}

fn encode_kitty_data(out: &mut Vec<u8>, control: &str, data: &[u8]) {
    let mut chunks = data.chunks(KITTY_CHUNK_BYTES).peekable();
    let Some(first) = chunks.next() else {
        return;
    };
    let more = if chunks.peek().is_some() { 1 } else { 0 };
    let encoded = base64::engine::general_purpose::STANDARD.encode(first);
    let _ = write!(out, "\x1b_G{control},m={more};{encoded}\x1b\\");

    while let Some(chunk) = chunks.next() {
        let more = if chunks.peek().is_some() { 1 } else { 0 };
        let encoded = base64::engine::general_purpose::STANDARD.encode(chunk);
        let _ = write!(out, "\x1b_Gm={more};{encoded}\x1b\\");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ghostty::KittyPlacementRenderInfo;

    fn test_placement(viewport_col: i32, viewport_row: i32) -> HostPlacement {
        HostPlacement {
            pane_id: PaneId::from_raw(1),
            area: Rect::new(0, 0, 20, 10),
            cell_size: HostCellSize {
                width_px: 10,
                height_px: 10,
            },
            scrollback_offset: 0,
            placement: KittyImagePlacement {
                image_id: 7,
                placement_id: 3,
                z: 0,
                x_offset: 0,
                y_offset: 0,
                image_width: 30,
                image_height: 30,
                format: KittyImageFormat::Rgba,
                data_len: 30 * 30 * 4,
                data_fingerprint: 42,
                data: vec![255; 30 * 30 * 4],
                render: KittyPlacementRenderInfo {
                    pixel_width: 0,
                    pixel_height: 0,
                    grid_cols: 3,
                    grid_rows: 3,
                    viewport_col,
                    viewport_row,
                    source_x: 0,
                    source_y: 0,
                    source_width: 0,
                    source_height: 0,
                },
            },
        }
    }

    #[test]
    fn projected_partial_pan_crops_at_terminal_source_and_nonzero_origin() {
        let placement = test_placement(2, 2);
        let projected = project_host_placement(
            placement.pane_id,
            placement.area,
            Rect::new(3, 3, 17, 7),
            Rect::new(30, 7, 17, 7),
            placement.cell_size,
            placement.placement,
            0,
        )
        .expect("partially visible placement");

        assert_eq!(projected.area, Rect::new(30, 7, 17, 7));
        assert_eq!(projected.placement.render.viewport_col, -1);
        assert_eq!(projected.placement.render.viewport_row, -1);

        let (clipped, _) = clipped_placement(&projected).expect("visible crop");
        assert_eq!(clipped.x, 30);
        assert_eq!(clipped.y, 7);
        assert_eq!(clipped.cols, 2);
        assert_eq!(clipped.rows, 2);
        assert_eq!(clipped.source_x, 10);
        assert_eq!(clipped.source_y, 10);
    }

    #[test]
    fn fully_hidden_pan_deletes_then_reappearance_redisplays_without_upload() {
        let mut images = HashMap::new();
        let mut placements = HashMap::new();
        let mut sources = HashMap::new();
        let mut bytes = Vec::new();

        let initial = test_placement(0, 0);
        let initial = project_host_placement(
            initial.pane_id,
            initial.area,
            Rect::new(0, 0, 20, 10),
            Rect::new(30, 7, 20, 10),
            initial.cell_size,
            initial.placement,
            0,
        )
        .expect("visible projected placement");
        encode_graphics_update(
            &mut bytes,
            &[initial],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );
        assert!(String::from_utf8_lossy(&bytes).contains("a=t"));
        bytes.clear();

        encode_graphics_update(
            &mut bytes,
            &[],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );
        assert!(String::from_utf8_lossy(&bytes).contains("a=d,d=i"));
        assert!(placements.is_empty());
        bytes.clear();

        let reappeared = test_placement(0, 0);
        let reappeared = project_host_placement(
            reappeared.pane_id,
            reappeared.area,
            Rect::new(0, 0, 20, 10),
            Rect::new(30, 7, 20, 10),
            reappeared.cell_size,
            reappeared.placement,
            0,
        )
        .expect("visible projected placement");
        encode_graphics_update(
            &mut bytes,
            &[reappeared],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );
        let output = String::from_utf8_lossy(&bytes);
        assert!(!output.contains("a=t"));
        assert!(output.contains("a=p"));
    }

    #[test]
    fn clipped_placement_handles_positive_viewport_without_wrapping() {
        let placement = test_placement(2, 2);
        let (clipped, _) = clipped_placement(&placement).expect("visible placement");

        assert_eq!(clipped.x, 2);
        assert_eq!(clipped.y, 2);
        assert_eq!(clipped.cols, 3);
        assert_eq!(clipped.rows, 3);
        assert_eq!(clipped.source_x, 0);
        assert_eq!(clipped.source_y, 0);
    }

    #[test]
    fn clipped_placement_crops_negative_viewport_offsets() {
        let placement = test_placement(-1, -1);
        let (clipped, _) = clipped_placement(&placement).expect("partially visible placement");

        assert_eq!(clipped.x, 0);
        assert_eq!(clipped.y, 0);
        assert_eq!(clipped.cols, 2);
        assert_eq!(clipped.rows, 2);
        assert_eq!(clipped.source_x, 10);
        assert_eq!(clipped.source_y, 10);
    }

    #[test]
    fn graphics_update_uploads_once_then_repositions_only() {
        let mut images = HashMap::new();
        let mut placements = HashMap::new();
        let mut sources = HashMap::new();
        let mut bytes = Vec::new();
        let placement = test_placement(0, 0);

        encode_graphics_update(
            &mut bytes,
            &[placement],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );
        let first = String::from_utf8_lossy(&bytes);
        assert!(first.contains("a=t"));
        assert!(first.contains("a=p"));
        assert!(first.contains("\x1b[1;1H"));

        bytes.clear();
        let same = test_placement(0, 0);
        encode_graphics_update(
            &mut bytes,
            &[same],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );
        assert!(bytes.is_empty());

        let mut z_changed = test_placement(0, 0);
        z_changed.placement.z = 1;
        encode_graphics_update(
            &mut bytes,
            &[z_changed],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );
        let z_changed_bytes = String::from_utf8_lossy(&bytes);
        assert!(!z_changed_bytes.contains("a=t"));
        assert!(z_changed_bytes.contains("a=p"));

        bytes.clear();
        let moved = test_placement(0, 1);
        encode_graphics_update(
            &mut bytes,
            &[moved],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );
        let moved_bytes = String::from_utf8_lossy(&bytes);
        assert!(!moved_bytes.contains("a=t"));
        assert!(moved_bytes.contains("a=p"));
    }

    #[test]
    fn view_change_redisplays_unchanged_visible_placement() {
        let mut images = HashMap::new();
        let mut placements = HashMap::new();
        let mut sources = HashMap::new();
        let mut bytes = Vec::new();
        let placement = test_placement(0, 0);

        encode_graphics_update(
            &mut bytes,
            &[placement],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );
        assert_eq!(placements.len(), 1);

        bytes.clear();
        let same = test_placement(0, 0);
        encode_graphics_update(
            &mut bytes,
            &[same],
            true,
            &mut images,
            &mut placements,
            &mut sources,
        );
        let redisplay = String::from_utf8_lossy(&bytes);
        assert!(!redisplay.contains("a=t"));
        assert!(redisplay.contains("a=p"));
        assert_eq!(placements.len(), 1);
    }

    #[test]
    fn surface_reset_deletes_then_reuploads_and_redisplays_placement() {
        let mut cache = HostGraphicsCache::default();
        let mut bytes = Vec::new();
        let placement = test_placement(0, 0);

        encode_graphics_update(
            &mut bytes,
            &[placement],
            false,
            &mut cache.images,
            &mut cache.placements,
            &mut cache.sources,
        );
        assert_eq!(cache.images.len(), 1);
        assert_eq!(cache.placements.len(), 1);

        bytes = cache.clear_bytes();
        let same = test_placement(0, 0);
        encode_graphics_update(
            &mut bytes,
            &[same],
            false,
            &mut cache.images,
            &mut cache.placements,
            &mut cache.sources,
        );

        let redisplay = String::from_utf8_lossy(&bytes);
        assert!(redisplay.contains("a=d,d=I"));
        assert!(redisplay.contains("a=t"));
        assert!(redisplay.contains("a=p"));
        assert_eq!(cache.images.len(), 1);
        assert_eq!(cache.placements.len(), 1);
    }

    #[test]
    fn scrollback_offset_change_redisplays_placement() {
        let mut images = HashMap::new();
        let mut placements = HashMap::new();
        let mut sources = HashMap::new();
        let mut bytes = Vec::new();
        let placement = test_placement(0, 0);

        encode_graphics_update(
            &mut bytes,
            &[placement],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );

        bytes.clear();
        let mut scrolled = test_placement(0, 0);
        scrolled.scrollback_offset = 3;
        encode_graphics_update(
            &mut bytes,
            &[scrolled],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );
        let redisplay = String::from_utf8_lossy(&bytes);
        assert!(!redisplay.contains("a=t"));
        assert!(redisplay.contains("a=p"));
        assert!(redisplay.contains("\x1b[4;1H"));
    }

    #[test]
    fn scrollback_offset_hides_placement_below_viewport() {
        let mut images = HashMap::new();
        let mut placements = HashMap::new();
        let mut sources = HashMap::new();
        let mut bytes = Vec::new();
        let placement = test_placement(0, 0);

        encode_graphics_update(
            &mut bytes,
            &[placement],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );

        bytes.clear();
        let mut scrolled = test_placement(0, 0);
        scrolled.scrollback_offset = 10;
        encode_graphics_update(
            &mut bytes,
            &[scrolled],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );

        let redisplay = String::from_utf8_lossy(&bytes);
        assert!(redisplay.contains("a=d,d=i"));
        assert!(!redisplay.contains("a=p"));
    }

    #[test]
    fn empty_image_data_does_not_mark_image_uploaded() {
        let mut images = HashMap::new();
        let mut placements = HashMap::new();
        let mut sources = HashMap::new();
        let mut bytes = Vec::new();
        let mut placement = test_placement(0, 0);
        placement.placement.data.clear();

        encode_graphics_update(
            &mut bytes,
            &[placement],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );

        assert!(bytes.is_empty());
        assert!(images.is_empty());
        assert!(placements.is_empty());
    }

    #[test]
    fn same_image_signature_reuses_host_upload_across_source_image_ids() {
        let mut images = HashMap::new();
        let mut placements = HashMap::new();
        let mut sources = HashMap::new();
        let mut bytes = Vec::new();
        let first = test_placement(0, 0);

        encode_graphics_update(
            &mut bytes,
            &[first],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );
        assert_eq!(images.len(), 1);
        assert_eq!(placements.len(), 1);

        bytes.clear();
        let mut same_image_new_source_id = test_placement(0, 0);
        same_image_new_source_id.placement.image_id = 8;
        same_image_new_source_id.placement.placement_id = 4;
        same_image_new_source_id.placement.data.clear();
        encode_graphics_update(
            &mut bytes,
            &[same_image_new_source_id],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );

        let reused = String::from_utf8_lossy(&bytes);
        assert!(!reused.contains("a=t"));
        assert!(reused.contains("a=p"));
        assert_eq!(images.len(), 1);
        assert_eq!(placements.len(), 1);
    }

    #[test]
    fn stale_placement_deletes_placement_not_image_immediately() {
        let mut images = HashMap::new();
        let mut placements = HashMap::new();
        let mut sources = HashMap::new();
        let mut bytes = Vec::new();
        let placement = test_placement(0, 0);

        encode_graphics_update(
            &mut bytes,
            &[placement],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );
        assert_eq!(placements.len(), 1);

        bytes.clear();
        encode_graphics_update(
            &mut bytes,
            &[],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );
        let delete = String::from_utf8_lossy(&bytes);
        assert!(delete.contains("a=d,d=i"));
        assert!(!delete.contains("d=I"));
        assert!(placements.is_empty());
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn view_change_deletes_stale_placement_immediately() {
        let mut images = HashMap::new();
        let mut placements = HashMap::new();
        let mut sources = HashMap::new();
        let mut bytes = Vec::new();
        let placement = test_placement(0, 0);

        encode_graphics_update(
            &mut bytes,
            &[placement],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );
        bytes.clear();
        encode_graphics_update(
            &mut bytes,
            &[],
            true,
            &mut images,
            &mut placements,
            &mut sources,
        );

        let delete = String::from_utf8_lossy(&bytes);
        assert!(delete.contains("a=d,d=i"));
        assert!(placements.is_empty());
    }
    #[test]
    fn changed_source_image_deletes_superseded_host_image() {
        let mut images = HashMap::new();
        let mut placements = HashMap::new();
        let mut sources = HashMap::new();
        let mut bytes = Vec::new();
        let first = test_placement(0, 0);
        encode_graphics_update(
            &mut bytes,
            &[first],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );
        bytes.clear();
        let mut changed = test_placement(0, 0);
        changed.placement.data_fingerprint = 99;
        encode_graphics_update(
            &mut bytes,
            &[changed],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );
        let output = String::from_utf8_lossy(&bytes);
        assert!(output.contains("a=d,d=I"));
        assert_eq!(images.len(), 1);
        assert_eq!(sources.len(), 1);
    }

    #[test]
    fn focused_blit_uses_the_only_pane_on_a_terminal_tab() {
        let mut app = crate::app::state::AppState::test_new();
        app.mode = Mode::Terminal;
        app.workspaces = vec![crate::workspace::Workspace::test_new("tb")];
        app.active = Some(0);
        let pane_id = app.workspaces[0].tabs[0].root_pane;

        assert_eq!(focused_graphics_blit_pane(&app), Some(pane_id));
    }

    #[test]
    fn focused_blit_skips_split_tabs_until_zoomed() {
        let mut app = crate::app::state::AppState::test_new();
        app.mode = Mode::Terminal;
        let mut workspace = crate::workspace::Workspace::test_new("tb");
        workspace.test_split(ratatui::layout::Direction::Vertical);
        app.workspaces = vec![workspace];
        app.active = Some(0);

        assert_eq!(focused_graphics_blit_pane(&app), None);

        app.workspaces[0].tabs[0].zoomed = true;
        let focused = app.workspaces[0].tabs[0].layout.focused();
        assert_eq!(focused_graphics_blit_pane(&app), Some(focused));
    }

    fn large_placement() -> HostPlacement {
        let mut placement = test_placement(0, 0);
        placement.placement.data = vec![7; DIRECT_UPLOAD_MAX_BYTES + 1];
        placement.placement.data_len = placement.placement.data.len();
        placement
    }

    #[test]
    fn large_local_upload_uses_a_temp_file_instead_of_pixel_bytes() {
        let mut images = HashMap::new();
        let mut placements = HashMap::new();
        let mut sources = HashMap::new();
        let mut bytes = Vec::new();
        let placement = large_placement();

        encode_graphics_update_with(
            &mut bytes,
            &[placement],
            false,
            &mut images,
            &mut placements,
            &mut sources,
            ImageTransmit::TempFile,
        );

        let output = String::from_utf8_lossy(&bytes);
        assert!(output.contains("t=t"));
        assert!(!output.contains("t=d"));
        let payload = output
            .split(';')
            .nth(1)
            .and_then(|value| value.split('\u{1b}').next())
            .expect("temp-file upload should carry a path payload");
        let path = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(payload)
                .expect("temp-file path should be base64"),
        )
        .expect("temp-file path should be utf8");
        assert!(
            path.contains("tty-graphics-protocol"),
            "Ghostty only reads temp files whose path contains tty-graphics-protocol, got {path}"
        );
        assert!(bytes.len() < DIRECT_UPLOAD_MAX_BYTES);
    }

    #[test]
    fn remote_upload_keeps_direct_pixel_bytes() {
        let mut images = HashMap::new();
        let mut placements = HashMap::new();
        let mut sources = HashMap::new();
        let mut bytes = Vec::new();

        encode_graphics_update(
            &mut bytes,
            &[large_placement()],
            false,
            &mut images,
            &mut placements,
            &mut sources,
        );

        let output = String::from_utf8_lossy(&bytes);
        assert!(output.contains("t=d"));
        assert!(!output.contains("t=t"));
        assert!(bytes.len() > DIRECT_UPLOAD_MAX_BYTES);
    }

    #[test]
    fn host_image_data_is_requested_once_per_image_id() {
        let descriptor = crate::ghostty::KittyImageDescriptor {
            image_id: 9,
            placement_id: 1,
            image_width: 10,
            image_height: 10,
            format: KittyImageFormat::Rgba,
            data_len: 400,
            data_fingerprint: 1,
        };
        let uploaded = HashMap::new();
        let mut copied = HashSet::new();
        let pane_id = PaneId::from_raw(1);

        assert!(needs_host_image_data(
            pane_id,
            descriptor,
            &uploaded,
            &mut copied
        ));
        assert!(!needs_host_image_data(
            pane_id,
            descriptor,
            &uploaded,
            &mut copied
        ));
    }
}
