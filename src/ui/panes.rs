use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use super::scrollbar::{render_pane_scrollbar, should_show_scrollbar};
#[cfg(test)]
use super::text::display_width;
use super::text::truncate_end;
use super::widgets::panel_contrast_fg;
use crate::app::state::Palette;
use crate::app::{AppState, Mode};
use crate::layout::PaneInfo;
use crate::popup_size::resolve_popup_geometry;
use crate::terminal::{TerminalRuntime, TerminalRuntimeRegistry};

pub(crate) fn pane_is_scrolled_back(rt: &TerminalRuntime) -> bool {
    rt.scroll_metrics()
        .is_some_and(|metrics| metrics.offset_from_bottom > 0)
}

fn pane_border_title(label: &str, pane_width: u16, _focused: bool) -> Option<String> {
    let label = label.trim();
    if label.is_empty() || pane_width <= 4 {
        return None;
    }
    let max_label_width = pane_width.saturating_sub(4) as usize;
    Some(format!(" {} ", truncate_end(label, max_label_width)))
}

/// Workspace-level facts a border title can name.
///
/// Every pane on screen belongs to one tab of one workspace, so these are the
/// same for all of them and are resolved once per render rather than per pane.
struct BorderTitleContext {
    workspace: Option<String>,
    tab: Option<String>,
    branch: Option<String>,
    indicator_style: crate::config::StatusIndicatorStyle,
    /// `ui.show_agent_labels_on_pane_borders`. The older switch for the same
    /// surface, so it still decides whether a detected agent may be named —
    /// it just governs the `agent` token now instead of the whole title.
    show_agent_labels: bool,
}

/// Builds a border title from the configured tokens.
///
/// Returns `None` when nothing resolved, so the caller leaves the border an
/// unbroken line instead of drawing a lone separator.
///
/// Every value here is a field or a cached lookup. Nothing in this path may
/// reach a `TerminalRuntime`: its `cwd()` reads `/proc` per call, and this runs
/// per render × per pane × per attached client.
fn pane_border_title_from_tokens(
    tokens: &[crate::config::PaneBorderToken],
    terminal: &crate::terminal::TerminalState,
    seen: bool,
    pane_number: Option<usize>,
    context: &BorderTitleContext,
) -> Option<String> {
    use crate::config::PaneBorderToken as Token;

    // values() builds a fresh map, so it is built once per pane and only when a
    // custom token is actually configured — never once per token.
    let metadata = tokens
        .iter()
        .any(|token| matches!(token, Token::Custom(_)))
        .then(|| terminal.metadata_tokens.values());

    let mut parts: Vec<String> = Vec::new();
    for token in tokens {
        let value = match token {
            Token::Cwd => shorten_home(&terminal.cwd),
            Token::Agent => context
                .show_agent_labels
                .then(|| {
                    terminal
                        .effective_display_agent()
                        .or_else(|| terminal.effective_agent_label().map(str::to_string))
                })
                .flatten(),
            Token::StateIcon => Some(
                super::status::state_icon_symbol(terminal.state, seen, context.indicator_style)
                    .to_string(),
            ),
            Token::StateText => Some(super::status::state_label(terminal.state, seen).to_string()),
            Token::Branch => context.branch.clone(),
            Token::Pane => pane_number.map(|number| number.to_string()),
            Token::Workspace => context.workspace.clone(),
            Token::Tab => context.tab.clone(),
            Token::TerminalTitle => terminal.terminal_title.clone(),
            Token::TerminalTitleStripped => terminal.terminal_title_stripped(),
            Token::Custom(name) => metadata
                .as_ref()
                .and_then(|values| values.get(name).cloned()),
        };
        if let Some(value) = value {
            let value = value.trim();
            if !value.is_empty() {
                parts.push(value.to_string());
            }
        }
    }

    (!parts.is_empty()).then(|| parts.join(" "))
}

/// The home directory a border title abbreviates against.
///
/// Resolved once rather than per call: `shorten_home` runs per render × per
/// pane × per attached client, and reading the environment there would allocate
/// on every one of them.
fn home_dir() -> Option<&'static std::path::Path> {
    static HOME: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    HOME.get_or_init(|| {
        let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
        #[cfg(windows)]
        let home = home.or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from));
        home.filter(|home| !home.as_os_str().is_empty())
    })
    .as_deref()
}

/// Abbreviates a pane's directory against the home directory, so the part that
/// differs between panes gets the width instead of a prefix they all share.
///
/// Compares by path component, not by string prefix, so `/home/ann2` is not
/// read as living under `/home/ann` and the separator is whatever the platform
/// uses.
fn shorten_home(path: &std::path::Path) -> Option<String> {
    if path.as_os_str().is_empty() {
        return None;
    }
    let shortened = home_dir()
        .and_then(|home| path.strip_prefix(home).ok())
        .map(|rest| {
            if rest.as_os_str().is_empty() {
                // Joining an empty path would leave a trailing separator.
                "~".to_string()
            } else {
                // Joined rather than formatted so the separator is the platform's.
                std::path::Path::new("~").join(rest).display().to_string()
            }
        });
    Some(shortened.unwrap_or_else(|| path.to_string_lossy().into_owned()))
}

// Full view computation reaches this helper for active and background panes.
// Keep terminal queries narrow, allocation-free, and short under the core lock.
fn terminal_inner_rect(rt: &TerminalRuntime, pane_inner: Rect, pane_scrollbars: bool) -> Rect {
    if !pane_scrollbars || pane_inner.width <= 4 || rt.alternate_screen_active() {
        return pane_inner;
    }

    Rect::new(
        pane_inner.x,
        pane_inner.y,
        pane_inner.width.saturating_sub(1),
        pane_inner.height,
    )
}

pub(crate) fn pane_inner_rect(area: Rect, borders: Borders) -> Rect {
    if borders.is_empty() {
        area
    } else {
        Block::default().borders(borders).inner(area)
    }
}

fn ranges_overlap(a_start: u16, a_len: u16, b_start: u16, b_len: u16) -> bool {
    a_start < b_start.saturating_add(b_len) && b_start < a_start.saturating_add(a_len)
}

fn pane_to_right<'a>(info: &PaneInfo, panes: &'a [PaneInfo]) -> Option<&'a PaneInfo> {
    let right = info.rect.x.saturating_add(info.rect.width);
    panes.iter().find(|other| {
        other.id != info.id
            && other.rect.x == right
            && ranges_overlap(
                info.rect.y,
                info.rect.height,
                other.rect.y,
                other.rect.height,
            )
    })
}

fn pane_below<'a>(info: &PaneInfo, panes: &'a [PaneInfo]) -> Option<&'a PaneInfo> {
    let bottom = info.rect.y.saturating_add(info.rect.height);
    panes.iter().find(|other| {
        other.id != info.id
            && other.rect.y == bottom
            && ranges_overlap(info.rect.x, info.rect.width, other.rect.x, other.rect.width)
    })
}

fn shrink_for_one_cell_gap(size: u16) -> u16 {
    if size > 1 {
        size - 1
    } else {
        size
    }
}

/// Borders for a pane that is the only one in its tab.
///
/// A top rule, not a box: it exists to carry the title, and the sides and
/// bottom would cost columns and a second row for nothing. `pane_inner_rect`
/// turns this into exactly one row off the top and no change in width.
///
/// `single_pane_border` comes from `AppState::single_pane_border_enabled()`;
/// every caller must pass that same value.
fn single_pane_borders(single_pane_border: bool) -> Borders {
    if single_pane_border {
        Borders::TOP
    } else {
        Borders::NONE
    }
}

/// Borders for the one pane a zoomed tab shows.
///
/// Zoom bypasses `apply_pane_chrome` entirely, so it has to reach the same
/// answer on its own — and a tab holding a single pane must agree with the
/// unzoomed path, or selecting the tab resizes its PTY.
fn zoomed_pane_borders(
    multi_pane: bool,
    pane_borders: bool,
    pane_outer_borders: bool,
    single_pane_border: bool,
) -> Borders {
    if !pane_borders {
        Borders::NONE
    } else if !multi_pane {
        single_pane_borders(single_pane_border)
    } else if pane_outer_borders {
        Borders::ALL
    } else {
        Borders::NONE
    }
}

pub(crate) fn apply_pane_chrome(
    panes: Vec<PaneInfo>,
    pane_borders: bool,
    pane_gaps: bool,
    pane_outer_borders: bool,
    single_pane_border: bool,
) -> Vec<PaneInfo> {
    let multi_pane = panes.len() > 1;
    let outer_left = panes.iter().map(|info| info.rect.x).min().unwrap_or(0);
    let outer_top = panes.iter().map(|info| info.rect.y).min().unwrap_or(0);
    let outer_right = panes
        .iter()
        .map(|info| info.rect.x.saturating_add(info.rect.width))
        .max()
        .unwrap_or(0);
    let outer_bottom = panes
        .iter()
        .map(|info| info.rect.y.saturating_add(info.rect.height))
        .max()
        .unwrap_or(0);
    panes
        .iter()
        .cloned()
        .map(|mut info| {
            let right_neighbor = multi_pane.then(|| pane_to_right(&info, &panes)).flatten();
            let below_neighbor = multi_pane.then(|| pane_below(&info, &panes)).flatten();

            if multi_pane && pane_gaps && !pane_borders {
                if right_neighbor.is_some() {
                    info.rect.width = shrink_for_one_cell_gap(info.rect.width);
                }
                if below_neighbor.is_some() {
                    info.rect.height = shrink_for_one_cell_gap(info.rect.height);
                }
            }

            info.borders = if !pane_borders {
                Borders::NONE
            } else if !multi_pane {
                single_pane_borders(single_pane_border)
            } else {
                let mut borders = Borders::ALL;
                if !pane_gaps {
                    if right_neighbor.is_some() {
                        borders.remove(Borders::RIGHT);
                    }
                    if below_neighbor.is_some() {
                        borders.remove(Borders::BOTTOM);
                    }
                }
                if !pane_outer_borders {
                    if info.rect.x == outer_left {
                        borders.remove(Borders::LEFT);
                    }
                    if info.rect.y == outer_top {
                        borders.remove(Borders::TOP);
                    }
                    if info.rect.x.saturating_add(info.rect.width) == outer_right {
                        borders.remove(Borders::RIGHT);
                    }
                    if info.rect.y.saturating_add(info.rect.height) == outer_bottom {
                        borders.remove(Borders::BOTTOM);
                    }
                }
                borders
            };
            info
        })
        .collect()
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

fn stable_scrollbar_gutter(
    rt: &TerminalRuntime,
    pane_inner: Rect,
    pane_scrollbars: bool,
) -> (Rect, Option<Rect>) {
    let inner_rect = terminal_inner_rect(rt, pane_inner, pane_scrollbars);
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

/// Resize every visible runtime in a tab to the geometry it would receive if the tab were selected.
pub(super) fn resize_tab_panes(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    tab: &crate::workspace::Tab,
    area: Rect,
    cell_size: crate::kitty_graphics::HostCellSize,
) {
    let multi_pane = tab.layout.pane_count() > 1;

    if tab.zoomed {
        let focused_id = tab.layout.focused();
        if let Some((terminal_id, rt)) = runtime_for_tab_pane(terminal_runtimes, tab, focused_id) {
            let borders = zoomed_pane_borders(
                multi_pane,
                app.pane_borders,
                app.pane_outer_borders,
                app.single_pane_border_enabled(),
            );
            let pane_inner = pane_inner_rect(area, borders);
            let inner_rect = terminal_inner_rect(rt, pane_inner, app.pane_scrollbars);
            if !app.direct_attach_resize_locks.contains(terminal_id) {
                rt.resize(
                    inner_rect.height,
                    inner_rect.width,
                    cell_size.width_px,
                    cell_size.height_px,
                );
            }
        }
        return;
    }

    for info in apply_pane_chrome(
        tab.layout.panes(area),
        app.pane_borders,
        app.pane_gaps,
        app.pane_outer_borders,
        app.single_pane_border_enabled(),
    ) {
        let pane_inner = pane_inner_rect(info.rect, info.borders);

        if let Some((terminal_id, rt)) = runtime_for_tab_pane(terminal_runtimes, tab, info.id) {
            let inner_rect = terminal_inner_rect(rt, pane_inner, app.pane_scrollbars);
            if !app.direct_attach_resize_locks.contains(terminal_id) {
                rt.resize(
                    inner_rect.height,
                    inner_rect.width,
                    cell_size.width_px,
                    cell_size.height_px,
                );
            }
        }
    }
}

/// Compute pane layout info and optionally resize pane runtimes to match.
pub(super) fn compute_pane_infos(
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

    let multi_pane = ws.layout.pane_count() > 1;

    if ws.zoomed {
        let focused_id = ws.layout.focused();
        let borders = zoomed_pane_borders(
            multi_pane,
            app.pane_borders,
            app.pane_outer_borders,
            app.single_pane_border_enabled(),
        );
        let pane_inner = pane_inner_rect(area, borders);
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
            borders,
            is_focused: true,
        }];
    }

    let mut pane_infos = apply_pane_chrome(
        ws.layout.panes(area),
        app.pane_borders,
        app.pane_gaps,
        app.pane_outer_borders,
        app.single_pane_border_enabled(),
    );

    for info in &mut pane_infos {
        let pane_inner = pane_inner_rect(info.rect, info.borders);

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

pub(super) fn render_panes(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
    pane_infos: &[PaneInfo],
    split_borders: &[crate::layout::SplitBorder],
) {
    let Some(ws_idx) = app.active else {
        return;
    };
    let Some(ws) = app.workspaces.get(ws_idx) else {
        return;
    };

    let multi_pane = ws.layout.pane_count() > 1;
    let terminal_active = app.mode == Mode::Terminal;

    for info in pane_infos {
        if let Some(rt) = app.runtime_for_pane_in_workspace(terminal_runtimes, ws_idx, info.id) {
            let show_cursor = info.is_focused
                && terminal_active
                && !pane_is_scrolled_back(rt)
                && app.pane_exposes_host_cursor(ws_idx, info.id);
            rt.render(frame, info.inner_rect, show_cursor);
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
                validated_copy_mode_search_matches(app, info, rt);
            render_copy_mode_search_highlights(
                app,
                frame,
                info,
                copy_search_top,
                copy_search_bottom,
                &copy_search_matches,
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
                true,
            );
            render_copy_mode_cursor(app, frame, info);
        }
    }

    render_pane_borders(app, ws, pane_infos, split_borders, frame);
}

pub(crate) fn popup_pane_rects(app: &AppState, area: Rect) -> Option<(Rect, Rect)> {
    let popup = app.popup_pane.as_ref()?;
    resolve_popup_geometry(popup.width, popup.height, area)
        .map(|geometry| (geometry.outer, geometry.inner))
}

pub(super) fn resize_popup_pane(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
    cell_size: crate::kitty_graphics::HostCellSize,
) {
    let Some(popup) = app.popup_pane.as_ref() else {
        return;
    };
    let Some((_outer, inner)) = popup_pane_rects(app, area) else {
        return;
    };
    if app.direct_attach_resize_locks.contains(&popup.terminal_id) {
        return;
    }
    if let Some(rt) = terminal_runtimes.get(&popup.terminal_id) {
        rt.resize(
            inner.height,
            inner.width,
            cell_size.width_px,
            cell_size.height_px,
        );
    }
}

pub(super) fn render_popup_pane(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
    area: Rect,
) {
    let Some(popup) = app.popup_pane.as_ref() else {
        return;
    };
    let Some((outer, inner)) = popup_pane_rects(app, area) else {
        return;
    };
    let Some(rt) = terminal_runtimes.get(&popup.terminal_id) else {
        return;
    };
    let title = app
        .terminals
        .get(&popup.terminal_id)
        .and_then(|terminal| terminal.manual_label.as_deref())
        .unwrap_or("popup");
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.palette.accent))
        .title(pane_border_title(title, outer.width, true).unwrap_or_default())
        .style(Style::default().bg(app.palette.panel_bg));
    frame.render_widget(Clear, outer);
    frame.render_widget(block, outer);
    rt.render(frame, inner, !pane_is_scrolled_back(rt));
}

#[derive(Clone, Copy, Default)]
struct LineCell {
    up: bool,
    down: bool,
    left: bool,
    right: bool,
}

fn render_pane_borders(
    app: &AppState,
    ws: &crate::workspace::Workspace,
    pane_infos: &[PaneInfo],
    split_borders: &[crate::layout::SplitBorder],
    frame: &mut Frame,
) {
    if !app.pane_borders || pane_infos.iter().all(|info| info.borders.is_empty()) {
        return;
    }

    let mut cells = std::collections::HashMap::<(u16, u16), LineCell>::new();
    for info in pane_infos {
        add_pane_border_cells(&mut cells, info);
    }
    add_split_border_cells(app.pane_gaps, split_borders, &mut cells);

    let buf = frame.buffer_mut();
    let area = buf.area;
    for ((x, y), line) in cells {
        if x < area.x
            || x >= area.x.saturating_add(area.width)
            || y < area.y
            || y >= area.y.saturating_add(area.height)
        {
            continue;
        }
        let focused = pane_infos
            .iter()
            .any(|info| info.is_focused && line_touches_pane(x, y, info, app.pane_gaps));
        let symbol = line_cell_symbol(line);
        if symbol.is_empty() {
            continue;
        }
        let cell = &mut buf[(x, y)];
        cell.set_symbol(symbol);
        let color = if focused {
            app.palette.accent
        } else {
            app.palette.overlay0
        };
        cell.set_style(Style::default().fg(color));
    }

    render_pane_border_titles(app, ws, pane_infos, frame);
}

fn add_split_border_cells(
    pane_gaps: bool,
    split_borders: &[crate::layout::SplitBorder],
    cells: &mut std::collections::HashMap<(u16, u16), LineCell>,
) {
    if pane_gaps {
        return;
    }

    for split in split_borders {
        match split.direction {
            ratatui::layout::Direction::Horizontal => {
                let x = split.pos;
                let end = split.area.y.saturating_add(split.area.height);
                for y in split.area.y..=end {
                    if !cells.contains_key(&(x, y)) {
                        continue;
                    }
                    let left = x
                        .checked_sub(1)
                        .and_then(|left_x| cells.get(&(left_x, y)))
                        .is_some_and(|cell| cell.left || cell.right);
                    let right = cells
                        .get(&(x.saturating_add(1), y))
                        .is_some_and(|cell| cell.left || cell.right);
                    let cell = cells.entry((x, y)).or_default();
                    cell.up |= y > split.area.y;
                    cell.down |= y + 1 < end;
                    cell.left |= left;
                    cell.right |= right;
                }
            }
            ratatui::layout::Direction::Vertical => {
                let y = split.pos;
                let end = split.area.x.saturating_add(split.area.width);
                for x in split.area.x..=end {
                    if !cells.contains_key(&(x, y)) {
                        continue;
                    }
                    let up = y
                        .checked_sub(1)
                        .and_then(|up_y| cells.get(&(x, up_y)))
                        .is_some_and(|cell| cell.up || cell.down);
                    let down = cells
                        .get(&(x, y.saturating_add(1)))
                        .is_some_and(|cell| cell.up || cell.down);
                    let cell = cells.entry((x, y)).or_default();
                    cell.left |= x > split.area.x;
                    cell.right |= x + 1 < end;
                    cell.up |= up;
                    cell.down |= down;
                }
            }
        }
    }
}

fn add_pane_border_cells(
    cells: &mut std::collections::HashMap<(u16, u16), LineCell>,
    info: &PaneInfo,
) {
    let rect = info.rect;
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let right = rect.x.saturating_add(rect.width).saturating_sub(1);
    let bottom = rect.y.saturating_add(rect.height).saturating_sub(1);

    if info.borders.contains(Borders::TOP) {
        for x in rect.x..=right {
            let cell = cells.entry((x, rect.y)).or_default();
            cell.left |= x > rect.x;
            cell.right |= x < right;
        }
    }
    if info.borders.contains(Borders::BOTTOM) {
        for x in rect.x..=right {
            let cell = cells.entry((x, bottom)).or_default();
            cell.left |= x > rect.x;
            cell.right |= x < right;
        }
    }
    if info.borders.contains(Borders::LEFT) {
        for y in rect.y..=bottom {
            let cell = cells.entry((rect.x, y)).or_default();
            cell.up |= y > rect.y;
            cell.down |= y < bottom;
        }
    }
    if info.borders.contains(Borders::RIGHT) {
        for y in rect.y..=bottom {
            let cell = cells.entry((right, y)).or_default();
            cell.up |= y > rect.y;
            cell.down |= y < bottom;
        }
    }
}

fn line_touches_pane(x: u16, y: u16, info: &PaneInfo, pane_gaps: bool) -> bool {
    let rect = info.rect;
    if rect.width == 0 || rect.height == 0 {
        return false;
    }
    let right = rect.x.saturating_add(rect.width).saturating_sub(1);
    let bottom = rect.y.saturating_add(rect.height).saturating_sub(1);
    let in_rows = y >= rect.y && y <= bottom;
    let in_cols = x >= rect.x && x <= right;
    let own_border =
        (in_rows && (x == rect.x || x == right)) || (in_cols && (y == rect.y || y == bottom));

    if pane_gaps {
        return own_border;
    }

    let shared_right = rect.x.saturating_add(rect.width);
    let shared_bottom = rect.y.saturating_add(rect.height);
    own_border
        || (in_rows && x == shared_right)
        || (in_cols && y == shared_bottom)
        || (x == shared_right && y == shared_bottom)
}

fn render_pane_border_titles(
    app: &AppState,
    ws: &crate::workspace::Workspace,
    pane_infos: &[PaneInfo],
    frame: &mut Frame,
) {
    // Workspace-level tokens are the same on every pane here — the tab surface
    // draws one tab of one workspace — so they are resolved once rather than
    // cloned per pane inside the loop.
    let tokens = &app.pane_border_title;
    let context = BorderTitleContext {
        workspace: tokens
            .iter()
            .any(|token| matches!(token, crate::config::PaneBorderToken::Workspace))
            .then(|| ws.display_name_from_terminals(&app.terminals)),
        tab: tokens
            .iter()
            .any(|token| matches!(token, crate::config::PaneBorderToken::Tab))
            .then(|| ws.active_tab_display_name())
            .flatten(),
        branch: tokens
            .iter()
            .any(|token| matches!(token, crate::config::PaneBorderToken::Branch))
            .then(|| ws.branch())
            .flatten(),
        indicator_style: app.status_indicators,
        show_agent_labels: app.show_agent_labels_on_pane_borders,
    };

    let buf = frame.buffer_mut();
    let area = buf.area;
    for info in pane_infos {
        if !info.borders.contains(Borders::TOP) || info.rect.width <= 4 {
            continue;
        }
        let Some(pane) = ws.pane_state(info.id) else {
            continue;
        };
        let Some(terminal) = app.terminals.get(&pane.attached_terminal_id) else {
            continue;
        };
        // An explicit name wins over anything generated: the agent's reported
        // title first, then a name the user typed. Tokens fill the silence that
        // used to leave the border blank.
        //
        // A *detected* agent is not an explicit name, so it does not short
        // circuit the tokens — it reaches the border through the `agent` token,
        // which `show_agent_labels_on_pane_borders` still gates.
        let Some(label) = terminal.explicit_border_label().or_else(|| {
            pane_border_title_from_tokens(
                tokens,
                terminal,
                pane.seen,
                ws.public_pane_number(info.id),
                &context,
            )
        }) else {
            continue;
        };
        let Some(title) = pane_border_title(&label, info.rect.width, info.is_focused) else {
            continue;
        };
        let y = info.rect.y;
        if y < area.y || y >= area.y.saturating_add(area.height) {
            continue;
        }
        let start_x = info.rect.x.saturating_add(1);
        let end_x = info
            .rect
            .x
            .saturating_add(info.rect.width)
            .saturating_sub(1)
            .min(area.x.saturating_add(area.width));
        if start_x >= end_x {
            continue;
        }
        let color = if info.is_focused {
            app.palette.accent
        } else {
            app.palette.overlay0
        };
        let mut style = Style::default().fg(color);
        if info.is_focused {
            style = style.add_modifier(Modifier::BOLD);
        }
        buf.set_stringn(
            start_x,
            y,
            title,
            end_x.saturating_sub(start_x) as usize,
            style,
        );
    }
}

fn line_cell_symbol(line: LineCell) -> &'static str {
    match (line.up, line.down, line.left, line.right) {
        (true, true, true, true) => "┼",
        (true, true, true, false) => "┤",
        (true, true, false, true) => "├",
        (true, false, true, true) => "┴",
        (false, true, true, true) => "┬",
        (true, true, false, false) | (true, false, false, false) | (false, true, false, false) => {
            "│"
        }
        (false, false, true, true) | (false, false, true, false) | (false, false, false, true) => {
            "─"
        }
        (false, true, false, true) => "┌",
        (false, true, true, false) => "┐",
        (true, false, false, true) => "└",
        (true, false, true, false) => "┘",
        _ => "",
    }
}

fn render_copy_mode_cursor(app: &AppState, frame: &mut Frame, info: &PaneInfo) {
    if app.mode != Mode::Copy {
        return;
    }
    let Some(copy_mode) = app.copy_mode.as_ref() else {
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
            .bg(app.palette.accent)
            .add_modifier(Modifier::BOLD),
    );
}

fn validated_copy_mode_search_matches(
    app: &AppState,
    info: &PaneInfo,
    rt: &crate::terminal::TerminalRuntime,
) -> (u32, u32, Vec<(usize, crate::pane::TerminalTextMatch)>) {
    let Some(copy_mode) = app.copy_mode.as_ref() else {
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
    current_only: bool,
) {
    let Some(copy_mode) = app.copy_mode.as_ref() else {
        return;
    };
    let current = copy_mode.search.current;
    let style = if current_only {
        Style::default()
            .fg(panel_contrast_fg(&app.palette))
            .bg(app.palette.accent)
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

pub(super) fn render_empty(app: &AppState, frame: &mut Frame, area: Rect) {
    let p = &app.palette;
    let lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "  No workspaces yet",
            Style::default().fg(p.overlay0),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  A workspace is one project context.",
            Style::default().fg(p.overlay1),
        )),
        Line::from(Span::styled(
            "  Its root pane (top-left) sets the default repo or folder name.",
            Style::default().fg(p.overlay1),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Press ", Style::default().fg(p.overlay0)),
            Span::styled(
                app.keybinds
                    .new_workspace
                    .label()
                    .unwrap_or_else(|| "unset".to_string()),
                Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
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
    use crate::detect::{Agent, AgentState};
    use crate::layout::PaneId;
    use crate::selection::Selection;
    use crate::terminal::TerminalRuntime;
    use crate::terminal::TerminalState;
    use crate::workspace::Workspace;

    fn render_view_pane_borders(app: &AppState, ws: &Workspace, frame: &mut Frame) {
        render_pane_borders(
            app,
            ws,
            &app.view.pane_infos,
            &app.view.split_borders,
            frame,
        );
    }

    #[test]
    fn pane_border_title_trims_and_truncates() {
        assert_eq!(
            pane_border_title(" claude ", 20, false).as_deref(),
            Some(" claude ")
        );
        assert_eq!(
            pane_border_title(" claude ", 20, true).as_deref(),
            Some(" claude ")
        );
        assert_eq!(pane_border_title("", 20, false), None);
        assert_eq!(
            pane_border_title("abcdef", 8, false).as_deref(),
            Some(" abc… ")
        );
        assert_eq!(
            pane_border_title("abcdef", 8, true).as_deref(),
            Some(" abc… ")
        );
        assert_eq!(pane_border_title("abcdef", 4, false), None);
    }

    #[test]
    fn pane_border_title_truncates_cjk_by_display_width() {
        let title = pane_border_title("1 模块组织（已定）", 12, false).unwrap();

        assert_eq!(title, " 1 模块… ");
        assert!(display_width(title.as_str()) <= 10);
    }

    #[test]
    fn pane_border_renderer_places_adjacent_cjk_by_display_width() {
        let mut app = AppState::test_new();
        app.mode = Mode::Terminal;
        app.view.terminal_area = Rect::new(0, 0, 12, 3);
        let ws = Workspace::test_new("test");
        let pane_id = ws.tabs[0].root_pane;
        app.view.pane_infos = vec![PaneInfo {
            id: pane_id,
            rect: Rect::new(0, 0, 12, 3),
            inner_rect: Rect::default(),
            scrollbar_rect: None,
            borders: Borders::ALL,
            is_focused: false,
        }];

        let terminal_id = ws.tabs[0].panes[&pane_id].attached_terminal_id.clone();
        let mut terminal_state = TerminalState::new(terminal_id.clone(), "/tmp".into());
        terminal_state.set_manual_label("1 模块组织（已定）".into());
        app.terminals.insert(terminal_id, terminal_state);

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(12, 3)).unwrap();
        terminal
            .draw(|frame| render_view_pane_borders(&app, &ws, frame))
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(4, 0)].symbol(), "模");
        assert_eq!(buffer[(5, 0)].symbol(), " ");
        assert_eq!(buffer[(6, 0)].symbol(), "块");
    }

    /// Builds a workspace with one bordered pane and renders its border.
    ///
    /// Returns the top border row as a string so a test can read the title the
    /// way a user sees it, rather than cell by cell.
    fn rendered_border_top(
        tokens: Vec<crate::config::PaneBorderToken>,
        show_agent_labels: bool,
        prepare: impl FnOnce(&mut TerminalState),
    ) -> String {
        let mut app = AppState::test_new();
        app.mode = Mode::Terminal;
        app.pane_border_title = tokens;
        app.show_agent_labels_on_pane_borders = show_agent_labels;
        app.view.terminal_area = Rect::new(0, 0, 40, 3);
        let ws = Workspace::test_new("test");
        let pane_id = ws.tabs[0].root_pane;
        app.view.pane_infos = vec![PaneInfo {
            id: pane_id,
            rect: Rect::new(0, 0, 40, 3),
            inner_rect: Rect::default(),
            scrollbar_rect: None,
            borders: Borders::ALL,
            is_focused: false,
        }];

        let terminal_id = ws.tabs[0].panes[&pane_id].attached_terminal_id.clone();
        let mut terminal_state = TerminalState::new(terminal_id.clone(), "/tmp/project".into());
        prepare(&mut terminal_state);
        app.terminals.insert(terminal_id, terminal_state);

        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 3)).unwrap();
        terminal
            .draw(|frame| render_view_pane_borders(&app, &ws, frame))
            .unwrap();

        let buffer = terminal.backend().buffer();
        (0..40)
            .map(|x| buffer[(x, 0)].symbol().to_string())
            .collect()
    }

    // The point of the feature: a pane nobody has titled says where it is
    // working instead of leaving the border blank.
    #[test]
    fn border_tokens_name_an_untitled_pane() {
        let top = rendered_border_top(vec![crate::config::PaneBorderToken::Cwd], false, |_| {});
        assert!(top.contains("/tmp/project"), "{top}");
    }

    /// Computes the chrome a lone pane gets, the way the live view does.
    fn lone_pane_infos(prepare: impl FnOnce(&mut AppState)) -> Vec<PaneInfo> {
        let mut app = AppState::test_new();
        app.pane_border_title = vec![crate::config::PaneBorderToken::Cwd];
        prepare(&mut app);

        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(40, 8, 1024, b"ready\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        compute_pane_infos(
            &app,
            &TerminalRuntimeRegistry::new(),
            Rect::new(10, 3, 40, 8),
            false,
            crate::kitty_graphics::HostCellSize::default(),
        )
    }

    // An unsplit pane is where a session spends most of its time, so it says
    // where it is working too — as a rule under the tab row, not a box.
    #[tokio::test]
    async fn a_lone_pane_border_is_a_top_rule_that_costs_one_row_and_no_columns() {
        let info = lone_pane_infos(|_| {}).remove(0);

        assert_eq!(info.borders, Borders::TOP, "a box would cost columns too");
        // Same x and width as the pane; one row off the top; the scrollbar
        // gutter still takes its column.
        assert_eq!(info.rect, Rect::new(10, 3, 40, 8));
        assert_eq!(info.inner_rect, Rect::new(10, 4, 39, 7));
    }

    // The documented way to turn titles off must not leave a blank rule behind
    // that costs a row and says nothing.
    #[tokio::test]
    async fn an_empty_title_leaves_a_lone_pane_borderless() {
        let info = lone_pane_infos(|app| app.pane_border_title.clear()).remove(0);

        assert_eq!(info.borders, Borders::NONE);
        assert_eq!(info.inner_rect, Rect::new(10, 3, 39, 8));
    }

    // A lone pane's top edge *is* the outer frame, so the switch that removes
    // the outer frame removes this too. Same for pane borders entirely.
    #[tokio::test]
    async fn a_lone_pane_border_obeys_the_existing_border_switches() {
        for prepare in [
            |app: &mut AppState| app.pane_outer_borders = false,
            |app: &mut AppState| app.pane_borders = false,
            |app: &mut AppState| app.pane_border_show_when_single_pane = false,
        ] {
            let info = lone_pane_infos(prepare).remove(0);
            assert_eq!(info.borders, Borders::NONE);
        }
    }

    // The point of the feature, drawn: the rule under the tab row reads as an
    // unbroken line with the title inside it, no dangling corners at the ends.
    #[test]
    fn a_lone_pane_draws_its_title_on_an_unbroken_rule() {
        let mut app = AppState::test_new();
        app.mode = Mode::Terminal;
        app.pane_border_title = vec![crate::config::PaneBorderToken::Cwd];

        let ws = Workspace::test_new("test");
        let pane_id = ws.tabs[0].root_pane;
        let area = Rect::new(0, 0, 40, 3);
        app.view.terminal_area = area;
        app.view.pane_infos =
            apply_pane_chrome(ws.tabs[0].layout.panes(area), true, false, true, true);

        let terminal_id = ws.tabs[0].panes[&pane_id].attached_terminal_id.clone();
        app.terminals.insert(
            terminal_id.clone(),
            TerminalState::new(terminal_id, "/tmp/project".into()),
        );

        let mut backend =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(40, 3)).unwrap();
        backend
            .draw(|frame| render_view_pane_borders(&app, &ws, frame))
            .unwrap();
        let buffer = backend.backend().buffer();
        let top: String = (0..40)
            .map(|x| buffer[(x, 0)].symbol().to_string())
            .collect();

        assert!(top.contains("/tmp/project"), "{top}");
        assert!(
            !top.contains('┌'),
            "a lone rule must not grow corners: {top}"
        );
        assert!(
            !top.contains('┐'),
            "a lone rule must not grow corners: {top}"
        );
        // The row below stays terminal content — no side borders were drawn.
        let second: String = (0..40)
            .map(|x| buffer[(x, 1)].symbol().to_string())
            .collect();
        assert!(
            !second.contains('│'),
            "a box was drawn, not a rule: {second}"
        );
    }

    // The real hazard of this feature: three places decide pane borders, and a
    // lone pane must get the same answer from all of them. If the selected and
    // background paths disagree by a row, every tab switch reflows the PTY.
    #[tokio::test]
    async fn selected_and_background_paths_agree_on_a_lone_pane() {
        for zoomed in [false, true] {
            let mut app = AppState::test_new();
            app.pane_border_title = vec![crate::config::PaneBorderToken::Cwd];

            let mut workspace = Workspace::test_new("test");
            workspace.tabs[0].zoomed = zoomed;
            let root_pane = workspace.tabs[0].root_pane;
            workspace.tabs[0].runtimes.insert(
                root_pane,
                TerminalRuntime::test_with_scrollback_bytes(40, 8, 1024, b"ready\n"),
            );
            app.workspaces = vec![workspace];
            app.active = Some(0);

            let area = Rect::new(10, 3, 40, 8);
            let registry = TerminalRuntimeRegistry::new();
            let cell_size = crate::kitty_graphics::HostCellSize::default();

            let selected =
                compute_pane_infos(&app, &registry, area, false, cell_size)[0].inner_rect;
            resize_tab_panes(&app, &registry, &app.workspaces[0].tabs[0], area, cell_size);
            let background = app.workspaces[0].tabs[0].runtimes[&root_pane].current_size();

            assert_eq!(
                (selected.height, selected.width),
                background,
                "selected and background disagree (zoomed={zoomed})"
            );
        }
    }

    // The reported case: side-by-side panes, an agent detected in the right
    // one, and the default title. Both borders must name their own directory —
    // the right pane keeps its own TOP border across the shared divider, and a
    // detected agent adds to its title rather than replacing it.
    #[test]
    fn a_split_names_both_panes_including_the_one_running_an_agent() {
        let mut app = AppState::test_new();
        app.mode = Mode::Terminal;
        app.show_agent_labels_on_pane_borders = true;
        app.pane_border_title = vec![
            crate::config::PaneBorderToken::Cwd,
            crate::config::PaneBorderToken::Agent,
        ];

        let mut ws = Workspace::test_new("test");
        let left_id = ws.tabs[0].root_pane;
        let right_id = ws.test_split(ratatui::layout::Direction::Horizontal);
        ws.tabs[0].layout.focus_pane(left_id);

        let area = Rect::new(0, 0, 80, 6);
        app.view.terminal_area = area;
        app.view.pane_infos =
            apply_pane_chrome(ws.tabs[0].layout.panes(area), true, false, true, false);

        for (pane_id, cwd, agent) in [
            (left_id, "/tmp/left", None),
            (right_id, "/tmp/right", Some(Agent::Claude)),
        ] {
            let terminal_id = ws.tabs[0].panes[&pane_id].attached_terminal_id.clone();
            let mut terminal = TerminalState::new(terminal_id.clone(), cwd.into());
            if let Some(agent) = agent {
                terminal.set_detected_state(Some(agent), AgentState::Idle);
            }
            app.terminals.insert(terminal_id, terminal);
        }

        let mut backend =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(area.width, area.height))
                .unwrap();
        backend
            .draw(|frame| render_view_pane_borders(&app, &ws, frame))
            .unwrap();
        let buffer = backend.backend().buffer();

        let read_top = |info: &PaneInfo| -> String {
            (info.rect.x..info.rect.x + info.rect.width)
                .map(|x| buffer[(x, info.rect.y)].symbol().to_string())
                .collect()
        };
        let left = read_top(
            app.view
                .pane_infos
                .iter()
                .find(|info| info.id == left_id)
                .unwrap(),
        );
        let right = read_top(
            app.view
                .pane_infos
                .iter()
                .find(|info| info.id == right_id)
                .unwrap(),
        );

        assert!(left.contains("/tmp/left"), "left border: {left}");
        assert!(!left.contains("claude"), "left border: {left}");
        assert!(right.contains("/tmp/right"), "right border: {right}");
        assert!(right.contains("claude"), "right border: {right}");
    }

    // Two panes on one screen differ by where they are, which is the whole
    // reason the default names cwd rather than the workspace.
    #[test]
    fn border_tokens_join_with_spaces_and_skip_what_is_absent() {
        let top = rendered_border_top(
            vec![
                crate::config::PaneBorderToken::Cwd,
                crate::config::PaneBorderToken::Agent,
            ],
            true,
            |_| {},
        );
        // No agent is running, so only the cwd shows — and no stray separator.
        assert!(top.contains("/tmp/project"), "{top}");
        assert!(!top.contains("  ─"), "a missing token left a gap: {top}");
    }

    // An explicit name outranks anything generated. Overwriting a name the user
    // typed with an automatic string would be a regression.
    #[test]
    fn a_manual_label_still_wins_over_tokens() {
        let top = rendered_border_top(
            vec![crate::config::PaneBorderToken::Cwd],
            false,
            |terminal| {
                terminal.set_manual_label("build".into());
            },
        );
        assert!(top.contains("build"), "{top}");
        assert!(!top.contains("/tmp/project"), "{top}");
    }

    // The documented way to turn the title off leaves the border as it was.
    #[test]
    fn no_tokens_leaves_the_border_unbroken() {
        let top = rendered_border_top(Vec::new(), false, |_| {});
        assert!(
            top.trim_matches(|ch| ch == '─' || ch == '┌' || ch == '┐')
                .is_empty(),
            "border should be an unbroken line: {top}"
        );
    }

    // The border is narrow, so the prefix every pane shares is the first thing
    // worth spending. Compared by component, so a sibling of home that merely
    // starts with the same text is left alone.
    #[test]
    fn shorten_home_abbreviates_only_real_children_of_home() {
        let Some(home) = home_dir() else {
            return;
        };

        assert_eq!(shorten_home(home).as_deref(), Some("~"));
        assert_eq!(
            shorten_home(&home.join("dev").join("herdr")),
            Some(
                std::path::Path::new("~")
                    .join("dev")
                    .join("herdr")
                    .display()
                    .to_string()
            )
        );

        let sibling = std::path::PathBuf::from(format!("{}2", home.display()));
        assert_eq!(
            shorten_home(&sibling).as_deref(),
            Some(sibling.to_string_lossy().as_ref()),
            "a sibling sharing home's text prefix must not be abbreviated"
        );

        assert_eq!(shorten_home(std::path::Path::new("")), None);
    }

    // A detected agent is a guess about what is running, not a name anyone
    // chose. Letting it short circuit the title is what kept the cwd off the
    // border for everyone who had the older switch turned on.
    #[test]
    fn a_detected_agent_does_not_displace_the_tokens() {
        let top = rendered_border_top(
            vec![
                crate::config::PaneBorderToken::Cwd,
                crate::config::PaneBorderToken::Agent,
            ],
            true,
            |terminal| {
                terminal.set_detected_state(Some(Agent::Claude), AgentState::Idle);
            },
        );
        assert!(top.contains("/tmp/project"), "cwd was displaced: {top}");
        assert!(top.contains("claude"), "{top}");
    }

    // Turning agent labels off is an existing setting, and the settings modal
    // still offers it. It now governs the `agent` token rather than the whole
    // title, so the rest of the title must survive it.
    #[test]
    fn agent_labels_off_suppresses_only_the_agent_token() {
        let top = rendered_border_top(
            vec![
                crate::config::PaneBorderToken::Cwd,
                crate::config::PaneBorderToken::Agent,
            ],
            false,
            |terminal| {
                terminal.set_detected_state(Some(Agent::Claude), AgentState::Idle);
            },
        );
        assert!(top.contains("/tmp/project"), "{top}");
        assert!(
            !top.contains("claude"),
            "agent token was not suppressed: {top}"
        );
    }

    #[test]
    fn default_horizontal_split_uses_one_shared_divider_column() {
        let mut workspace = Workspace::test_new("test");
        let root = workspace.tabs[0].root_pane;
        let right = workspace.test_split(ratatui::layout::Direction::Horizontal);
        workspace.tabs[0].layout.focus_pane(root);

        let infos = apply_pane_chrome(
            workspace.tabs[0].layout.panes(Rect::new(0, 0, 100, 20)),
            true,
            false,
            true,
            true,
        );
        let left = infos.iter().find(|info| info.id == root).unwrap();
        let right = infos.iter().find(|info| info.id == right).unwrap();

        assert_eq!(left.rect.x + left.rect.width, right.rect.x);
        assert!(!left.borders.contains(Borders::RIGHT));
        assert!(right.borders.contains(Borders::LEFT));
    }

    #[test]
    fn default_vertical_split_uses_one_shared_divider_row() {
        let mut workspace = Workspace::test_new("test");
        let root = workspace.tabs[0].root_pane;
        let bottom = workspace.test_split(ratatui::layout::Direction::Vertical);
        workspace.tabs[0].layout.focus_pane(root);

        let infos = apply_pane_chrome(
            workspace.tabs[0].layout.panes(Rect::new(0, 0, 100, 20)),
            true,
            false,
            true,
            true,
        );
        let top = infos.iter().find(|info| info.id == root).unwrap();
        let bottom = infos.iter().find(|info| info.id == bottom).unwrap();

        assert_eq!(top.rect.y + top.rect.height, bottom.rect.y);
        assert!(!top.borders.contains(Borders::BOTTOM));
        assert!(bottom.borders.contains(Borders::TOP));
    }

    #[test]
    fn disabled_outer_borders_keep_only_shared_pane_dividers() {
        let mut workspace = Workspace::test_new("test");
        let root = workspace.tabs[0].root_pane;
        let right = workspace.test_split(ratatui::layout::Direction::Horizontal);
        workspace.tabs[0].layout.focus_pane(root);

        let infos = apply_pane_chrome(
            workspace.tabs[0].layout.panes(Rect::new(0, 0, 100, 20)),
            true,
            false,
            false,
            true,
        );
        let left = infos.iter().find(|info| info.id == root).unwrap();
        let right = infos.iter().find(|info| info.id == right).unwrap();

        assert_eq!(left.borders, Borders::NONE);
        assert_eq!(right.borders, Borders::LEFT);
    }

    #[test]
    fn pane_gaps_keep_independent_bordered_panes() {
        let mut workspace = Workspace::test_new("test");
        let root = workspace.tabs[0].root_pane;
        let right = workspace.test_split(ratatui::layout::Direction::Horizontal);
        workspace.tabs[0].layout.focus_pane(root);

        let infos = apply_pane_chrome(
            workspace.tabs[0].layout.panes(Rect::new(0, 0, 100, 20)),
            true,
            true,
            true,
            true,
        );
        let left = infos.iter().find(|info| info.id == root).unwrap();
        let right = infos.iter().find(|info| info.id == right).unwrap();

        assert_eq!(left.rect.x + left.rect.width, right.rect.x);
        assert_eq!(left.borders, Borders::ALL);
        assert_eq!(right.borders, Borders::ALL);
    }

    #[test]
    fn borderless_pane_gaps_add_one_empty_cell_between_panes() {
        let mut workspace = Workspace::test_new("test");
        let root = workspace.tabs[0].root_pane;
        let right = workspace.test_split(ratatui::layout::Direction::Horizontal);
        workspace.tabs[0].layout.focus_pane(root);

        let infos = apply_pane_chrome(
            workspace.tabs[0].layout.panes(Rect::new(0, 0, 100, 20)),
            false,
            true,
            true,
            true,
        );
        let left = infos.iter().find(|info| info.id == root).unwrap();
        let right = infos.iter().find(|info| info.id == right).unwrap();

        assert_eq!(left.rect, Rect::new(0, 0, 49, 20));
        assert_eq!(right.rect, Rect::new(50, 0, 50, 20));
        assert!(left.borders.is_empty());
        assert!(right.borders.is_empty());
    }

    #[test]
    fn disabled_pane_borders_make_inner_rect_equal_visual_rect() {
        let mut workspace = Workspace::test_new("test");
        workspace.test_split(ratatui::layout::Direction::Horizontal);

        let infos = apply_pane_chrome(
            workspace.tabs[0].layout.panes(Rect::new(0, 0, 100, 20)),
            false,
            false,
            true,
            true,
        );

        for info in infos {
            assert!(info.borders.is_empty());
            assert_eq!(pane_inner_rect(info.rect, info.borders), info.rect);
        }
    }

    #[test]
    fn global_pane_border_renderer_composes_junctions_and_focus_style() {
        let mut app = AppState::test_new();
        app.mode = Mode::Terminal;
        app.view.terminal_area = Rect::new(0, 0, 4, 4);
        app.view.pane_infos = vec![
            PaneInfo {
                id: PaneId::from_raw(1),
                rect: Rect::new(0, 0, 2, 2),
                inner_rect: Rect::default(),
                scrollbar_rect: None,
                borders: Borders::TOP | Borders::LEFT,
                is_focused: true,
            },
            PaneInfo {
                id: PaneId::from_raw(2),
                rect: Rect::new(2, 0, 2, 2),
                inner_rect: Rect::default(),
                scrollbar_rect: None,
                borders: Borders::TOP | Borders::LEFT | Borders::RIGHT,
                is_focused: false,
            },
            PaneInfo {
                id: PaneId::from_raw(3),
                rect: Rect::new(0, 2, 2, 2),
                inner_rect: Rect::default(),
                scrollbar_rect: None,
                borders: Borders::TOP | Borders::LEFT | Borders::BOTTOM,
                is_focused: false,
            },
            PaneInfo {
                id: PaneId::from_raw(4),
                rect: Rect::new(2, 2, 2, 2),
                inner_rect: Rect::default(),
                scrollbar_rect: None,
                borders: Borders::ALL,
                is_focused: false,
            },
        ];
        app.view.split_borders = vec![
            crate::layout::SplitBorder {
                pos: 2,
                direction: ratatui::layout::Direction::Horizontal,
                ratio: 0.5,
                area: Rect::new(0, 0, 4, 4),
                path: vec![],
            },
            crate::layout::SplitBorder {
                pos: 2,
                direction: ratatui::layout::Direction::Vertical,
                ratio: 0.5,
                area: Rect::new(0, 0, 4, 4),
                path: vec![false],
            },
        ];
        let ws = Workspace::test_new("test");
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(4, 4)).unwrap();

        terminal
            .draw(|frame| render_view_pane_borders(&app, &ws, frame))
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(2, 2)].symbol(), "┼");
        assert_eq!(buffer[(2, 2)].style().fg, Some(app.palette.accent));
        assert_eq!(buffer[(2, 1)].symbol(), "│");
        assert_eq!(buffer[(2, 1)].style().fg, Some(app.palette.accent));
    }

    #[test]
    fn gapped_pane_focus_does_not_color_neighbor_border() {
        let mut app = AppState::test_new();
        app.mode = Mode::Terminal;
        app.pane_gaps = true;
        app.view.terminal_area = Rect::new(0, 0, 4, 3);
        app.view.pane_infos = vec![
            PaneInfo {
                id: PaneId::from_raw(1),
                rect: Rect::new(0, 0, 2, 3),
                inner_rect: Rect::default(),
                scrollbar_rect: None,
                borders: Borders::ALL,
                is_focused: true,
            },
            PaneInfo {
                id: PaneId::from_raw(2),
                rect: Rect::new(2, 0, 2, 3),
                inner_rect: Rect::default(),
                scrollbar_rect: None,
                borders: Borders::ALL,
                is_focused: false,
            },
        ];
        let ws = Workspace::test_new("test");
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(4, 3)).unwrap();

        terminal
            .draw(|frame| render_view_pane_borders(&app, &ws, frame))
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(1, 1)].style().fg, Some(app.palette.accent));
        assert_eq!(buffer[(2, 1)].style().fg, Some(app.palette.overlay0));
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
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, None);
        assert_eq!(info.inner_rect, Rect::new(10, 3, 39, 8));
    }

    #[tokio::test]
    async fn alternate_screen_reclaims_scrollbar_gutter_and_restores_it_on_exit() {
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
        let assert_geometry = |expected_width, has_scrollbar| {
            let infos = compute_pane_infos(
                &app,
                &terminal_runtimes,
                area,
                true,
                crate::kitty_graphics::HostCellSize::default(),
            );
            assert_eq!(
                infos[0].inner_rect,
                Rect::new(area.x, area.y, expected_width, area.height)
            );
            assert_eq!(infos[0].scrollbar_rect.is_some(), has_scrollbar);
            assert_eq!(
                app.workspaces[0].tabs[0].runtimes[&root_pane].current_size(),
                (area.height, expected_width)
            );
        };

        assert_geometry(39, true);
        app.workspaces[0].tabs[0].runtimes[&root_pane].test_process_pty_bytes(b"\x1b[?1049h");
        assert_geometry(40, false);
        app.workspaces[0].tabs[0].runtimes[&root_pane].test_process_pty_bytes(b"\x1b[?1049l");
        assert_geometry(39, true);
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
    async fn pane_scrollbar_setting_controls_reserved_column() {
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

        app.pane_scrollbars = false;
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
            ..Default::default()
        };
        let expected_style = automatic_selection_style(&palette, host_theme);
        let selection = Some(Selection::range(PaneId::from_raw(1), 0, 0, 2, None));
        let backend = ratatui::backend::TestBackend::new(4, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

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
            .unwrap();

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
                ..Default::default()
            },
        );

        let Color::Rgb(r, g, b) = bg else {
            panic!("selection background should resolve to rgb");
        };
        assert!(relative_luminance((r, g, b)) > relative_luminance((12, 14, 16)));
    }
}
