//! Scroll-anchor capture and restoration when switching Markdown `Preview`, `Full`, and `Code` tabs.

extern crate alloc;

use alloc::vec::Vec;

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::components::markdown_diff::MarkdownTab;
use crate::components::text_diff::expanded::MergedChunk;
use crate::components::text_diff::split::pair_side_by_side_rows;
use crate::components::text_diff::{CHUNK_WINDOW_STEP_LINES, LARGE_FILE_WINDOW_THRESHOLD};
use crate::state::DiffViewMode;

/// Fraction down the visible card body used as the scroll focal point while scrolled mid-file.
const FOCAL_VIEWPORT_FRACTION: f64 = 0.35;
/// Maximum line distance for snapping to the nearest neighbor before falling back to `fallback_ratio`.
pub const MAX_NEAREST_LINE_DISTANCE: u32 = 40;
/// Maximum line drift allowed before a line-targeted tab switch (such as pressing `c`) overrides the captured line.
pub const TARGET_LINE_OVERRIDE_THRESHOLD: u32 = 15;

/// Scroll anchor captured immediately before switching a Markdown card to `target_tab`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabScrollAnchor {
    /// Destination tab that should consume and restore this anchor.
    pub target_tab: MarkdownTab,
    /// Estimated 1-based source line at the viewport focal point, if any.
    pub source_line: Option<u32>,
    /// Target offset in pixels from `.diff-main`'s top edge (clamped below the sticky header).
    pub viewport_offset: f64,
    /// Fractional scroll position (`0.0..=1.0`) within `.diff-card__body` used as fallback.
    pub fallback_ratio: f64,
}

/// Viewport-coordinate bounds of `.diff-main`, the sticky card header, and `.diff-card__body`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewportGeometry {
    /// `.diff-main` top edge in viewport coordinates.
    pub main_top: f64,
    /// `.diff-main` bottom edge in viewport coordinates.
    pub main_bottom: f64,
    /// Bottom edge of `.diff-card__header` in viewport coordinates.
    pub header_bottom: f64,
    /// `.diff-card__body` top edge in viewport coordinates.
    pub body_top: f64,
    /// `.diff-card__body` bottom edge in viewport coordinates.
    pub body_bottom: f64,
}

/// Screen-space bounds and inclusive 1-based source line span of one candidate DOM element.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LineAnchorCandidate {
    /// Inclusive 1-based starting source line number.
    pub start_line: u32,
    /// Inclusive 1-based ending source line number (`>= start_line`).
    pub end_line: u32,
    /// Element top edge in viewport coordinates.
    pub top: f64,
    /// Element bottom edge in viewport coordinates.
    pub bottom: f64,
}

impl LineAnchorCandidate {
    fn height(self) -> f64 {
        (self.bottom - self.top).max(0.0)
    }

    fn line_span(self) -> u32 {
        self.end_line.saturating_sub(self.start_line)
    }

    fn contains_y(self, y: f64) -> bool {
        self.top <= y && y <= self.bottom
    }

    fn vertical_distance_to(self, y: f64) -> f64 {
        if y < self.top {
            self.top - y
        } else if y > self.bottom {
            y - self.bottom
        } else {
            0.0
        }
    }

    fn line_distance_to(self, line: u32) -> u32 {
        if line < self.start_line {
            self.start_line - line
        } else {
            line.saturating_sub(self.end_line)
        }
    }
}

/// Inclusive vertical viewport band (`top..=bottom`) in CSS pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VerticalBand {
    /// Top edge in viewport coordinates.
    pub top: f64,
    /// Bottom edge in viewport coordinates.
    pub bottom: f64,
}

impl VerticalBand {
    fn height(self) -> f64 {
        (self.bottom - self.top).max(0.0)
    }

    fn intersects(self, other: Self) -> bool {
        self.bottom > other.top && self.top < other.bottom
    }

    fn vertical_distance_to(self, y: f64) -> f64 {
        if y < self.top {
            self.top - y
        } else if y > self.bottom {
            y - self.bottom
        } else {
            0.0
        }
    }
}

/// Result of computing a destination `scroll_top` from a [`TabScrollAnchor`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RestoreScrollPlan {
    /// Target `.diff-main.scroll_top` in CSS pixels (`>= 0.0`).
    pub target_scroll_top: f64,
    /// Index into the candidate slice when a line element was matched (for post-layout correction).
    pub matched_index: Option<usize>,
}

/// Post-frame state returned when `Code` chunks were forced visible during restoration.
#[derive(Clone)]
pub struct CodeRestoreFollowUp {
    anchor: TabScrollAnchor,
    matched: Option<(web_sys::Element, u32, u32)>,
}

/// Interpolates a 1-based source line inside `cand` at viewport y-coordinate `focal_y`.
#[must_use]
pub fn interpolate_source_line(cand: LineAnchorCandidate, focal_y: f64) -> u32 {
    if cand.end_line <= cand.start_line {
        return cand.start_line;
    }
    let height = cand.height();
    if height <= 1.0 {
        return cand.start_line;
    }
    let frac = ((focal_y - cand.top) / height).clamp(0.0, 1.0);
    let span = f64::from(cand.end_line - cand.start_line);
    let offset = (frac * span).round() as u32;
    cand.start_line.saturating_add(offset).min(cand.end_line)
}

/// Computes the y-coordinate inside `cand` corresponding to `target_line`.
#[must_use]
pub fn compute_restore_target_y(cand: LineAnchorCandidate, target_line: u32) -> f64 {
    if cand.end_line <= cand.start_line || target_line <= cand.start_line {
        return cand.top;
    }
    if target_line >= cand.end_line {
        return cand.top + cand.height();
    }
    let frac =
        f64::from(target_line - cand.start_line) / f64::from(cand.end_line - cand.start_line);
    cand.top + frac * cand.height()
}

/// Selects the best capture candidate for `focal_y`, preferring the narrowest containing block.
#[must_use]
pub fn select_capture_candidate(
    candidates: &[LineAnchorCandidate],
    focal_y: f64,
) -> Option<LineAnchorCandidate> {
    let mut best_containing: Option<LineAnchorCandidate> = None;
    let mut best_nearest: Option<(f64, LineAnchorCandidate)> = None;

    for &cand in candidates {
        if cand.height() <= 0.0 {
            continue;
        }
        if cand.contains_y(focal_y) {
            let better = match best_containing {
                None => true,
                Some(cur) => {
                    cand.line_span() < cur.line_span()
                        || (cand.line_span() == cur.line_span()
                            && cand.height().total_cmp(&cur.height()).is_lt())
                }
            };
            if better {
                best_containing = Some(cand);
            }
        } else if best_containing.is_none() {
            let dist = cand.vertical_distance_to(focal_y);
            let better = match best_nearest {
                None => true,
                Some((cur_dist, cur)) => {
                    dist.total_cmp(&cur_dist).is_lt()
                        || (dist.total_cmp(&cur_dist).is_eq()
                            && (cand.line_span() < cur.line_span()
                                || (cand.line_span() == cur.line_span()
                                    && cand.height().total_cmp(&cur.height()).is_lt())))
                }
            };
            if better {
                best_nearest = Some((dist, cand));
            }
        }
    }

    best_containing.or_else(|| best_nearest.map(|(_, c)| c))
}

struct VisibleCardFocus {
    visible_top: f64,
    card_band: VerticalBand,
    focal_y: f64,
}

fn compute_visible_card_focus(geom: ViewportGeometry) -> Option<VisibleCardFocus> {
    let visible_top = geom.main_top.max(geom.header_bottom);
    let card_vis_top = visible_top.max(geom.body_top);
    let card_vis_bottom = geom.main_bottom.min(geom.body_bottom);
    if card_vis_bottom <= card_vis_top {
        return None;
    }

    let scrolled_into_card = geom.body_top < visible_top - 1.0;
    let focal_y = if scrolled_into_card {
        card_vis_top + (card_vis_bottom - card_vis_top) * FOCAL_VIEWPORT_FRACTION
    } else {
        card_vis_top
    };
    Some(VisibleCardFocus {
        visible_top,
        card_band: VerticalBand {
            top: card_vis_top,
            bottom: card_vis_bottom,
        },
        focal_y,
    })
}

/// Selects indices of `.diff-chunk-wrapper` bands that intersect `visible_band`, falling back to
/// the single band nearest `focal_y` when none intersect.
#[must_use]
pub fn select_visible_chunk_indices(
    chunk_bands: &[VerticalBand],
    visible_band: VerticalBand,
    focal_y: f64,
) -> Vec<usize> {
    let mut intersecting = Vec::new();
    let mut nearest: Option<(f64, usize)> = None;

    for (idx, &band) in chunk_bands.iter().enumerate() {
        if band.height() <= 0.0 {
            continue;
        }
        if band.intersects(visible_band) {
            intersecting.push(idx);
        } else if intersecting.is_empty() {
            let dist = band.vertical_distance_to(focal_y);
            if nearest.is_none_or(|(best_dist, _)| dist.total_cmp(&best_dist).is_lt()) {
                nearest = Some((dist, idx));
            }
        }
    }

    if !intersecting.is_empty() {
        return intersecting;
    }
    nearest.map_or_else(Vec::new, |(_, idx)| alloc::vec![idx])
}

/// Returns whether a code row's bounding box lies inside its enclosing chunk wrapper's band.
#[must_use]
pub fn row_within_chunk_band(row_top: f64, row_bottom: f64, chunk_band: VerticalBand) -> bool {
    const EDGE_EPSILON: f64 = 1.0;
    row_bottom > row_top
        && row_bottom >= chunk_band.top - EDGE_EPSILON
        && row_top <= chunk_band.bottom + EDGE_EPSILON
}

/// Returns whether scroll restoration should nudge the viewport to keep `active_element` visible.
#[must_use]
pub fn should_nudge_focused_element(in_card: bool, in_header: bool, in_comment_form: bool) -> bool {
    in_card && !in_header && in_comment_form
}

/// Captures a [`TabScrollAnchor`] from the current viewport geometry and line candidates.
#[must_use]
pub fn capture_anchor_from_candidates(
    geom: ViewportGeometry,
    candidates: &[LineAnchorCandidate],
    target_tab: MarkdownTab,
) -> Option<TabScrollAnchor> {
    let focus = compute_visible_card_focus(geom)?;
    let visible_top = focus.visible_top;
    let focal_y = focus.focal_y;

    let body_height = geom.body_bottom - geom.body_top;
    let fallback_ratio = if body_height > 1.0 {
        ((focal_y - geom.body_top) / body_height).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let min_offset = (visible_top - geom.main_top).max(0.0);
    let max_offset = (geom.main_bottom - geom.main_top - 24.0).max(min_offset);

    if let Some(cand) = select_capture_candidate(candidates, focal_y) {
        let source_line = interpolate_source_line(cand, focal_y);
        let raw_y = if cand.end_line > cand.start_line && cand.contains_y(focal_y) {
            focal_y
        } else {
            cand.top
        };
        let viewport_offset = (raw_y - geom.main_top).clamp(min_offset, max_offset);
        return Some(TabScrollAnchor {
            target_tab,
            source_line: Some(source_line),
            viewport_offset,
            fallback_ratio,
        });
    }

    let viewport_offset = (focal_y - geom.main_top).clamp(min_offset, max_offset);
    Some(TabScrollAnchor {
        target_tab,
        source_line: None,
        viewport_offset,
        fallback_ratio,
    })
}

/// Selects the destination candidate matching `target_line`, returning `(index, candidate, line_distance)`.
#[must_use]
pub fn select_restore_candidate(
    candidates: &[LineAnchorCandidate],
    target_line: u32,
) -> Option<(usize, LineAnchorCandidate, u32)> {
    let mut best: Option<(usize, LineAnchorCandidate, u32)> = None;

    for (idx, &cand) in candidates.iter().enumerate() {
        if cand.height() <= 0.0 {
            continue;
        }
        let dist = cand.line_distance_to(target_line);
        let better = match best {
            None => true,
            Some((_, cur, cur_dist)) => {
                dist < cur_dist
                    || (dist == cur_dist
                        && (cand.line_span() < cur.line_span()
                            || (cand.line_span() == cur.line_span()
                                && cand.height().total_cmp(&cur.height()).is_lt())))
            }
        };
        if better {
            best = Some((idx, cand, dist));
        }
    }

    best
}

/// Computes the target `.diff-main.scroll_top` to restore `anchor` in the destination view.
#[must_use]
pub fn compute_restore_scroll_plan(
    anchor: TabScrollAnchor,
    current_scroll_top: f64,
    geom: ViewportGeometry,
    candidates: &[LineAnchorCandidate],
) -> Option<RestoreScrollPlan> {
    let body_height = geom.body_bottom - geom.body_top;
    if body_height <= 0.0 {
        return None;
    }

    let desired_y = geom.main_top + anchor.viewport_offset;

    if let Some(target_line) = anchor.source_line
        && let Some((idx, cand, dist)) = select_restore_candidate(candidates, target_line)
        && dist <= MAX_NEAREST_LINE_DISTANCE
    {
        let target_y = compute_restore_target_y(cand, target_line);
        let target_scroll_top = (current_scroll_top + (target_y - desired_y)).max(0.0);
        return Some(RestoreScrollPlan {
            target_scroll_top,
            matched_index: Some(idx),
        });
    }

    let fallback_y = geom.body_top + anchor.fallback_ratio.clamp(0.0, 1.0) * body_height;
    let target_scroll_top = (current_scroll_top + (fallback_y - desired_y)).max(0.0);
    Some(RestoreScrollPlan {
        target_scroll_top,
        matched_index: None,
    })
}

/// Returns the expanded `visible_line_limit` required to render `target_line` in `Code` mode,
/// or `None` if windowing is inactive or `target_line` already lies inside `current_limit`.
#[must_use]
pub fn required_window_limit_for_line(
    merged_chunks: &[MergedChunk],
    mode: DiffViewMode,
    current_limit: usize,
    target_line: u32,
) -> Option<usize> {
    let total_merged_lines: usize = merged_chunks.iter().map(|mc| mc.lines.len()).sum();
    if total_merged_lines <= LARGE_FILE_WINDOW_THRESHOLD {
        return None;
    }

    let mut row_idx = 0usize;
    let mut best_match: Option<(u32, usize)> = None;

    let mut consider = |line_opt: Option<u32>, idx: usize| {
        let Some(ln) = line_opt else {
            return;
        };
        let dist = ln.abs_diff(target_line);
        if best_match.is_none_or(|(best_dist, _)| dist < best_dist) {
            best_match = Some((dist, idx));
        }
    };

    for mc in merged_chunks {
        if mode == DiffViewMode::Split {
            for row in pair_side_by_side_rows(mc) {
                let new_ln = row
                    .new_line_idx
                    .and_then(|i| mc.line(i))
                    .and_then(|l| l.new_line_number.or(l.old_line_number));
                let old_ln = row
                    .old_line_idx
                    .and_then(|i| mc.line(i))
                    .and_then(|l| l.old_line_number.or(l.new_line_number));
                consider(new_ln.or(old_ln), row_idx);
                if new_ln.is_some() && old_ln.is_some() {
                    consider(old_ln, row_idx);
                }
                row_idx += 1;
            }
        } else {
            for line in mc.iter_lines() {
                let primary = line.new_line_number.or(line.old_line_number);
                consider(primary, row_idx);
                if line.new_line_number.is_some() && line.old_line_number.is_some() {
                    consider(line.old_line_number, row_idx);
                }
                row_idx += 1;
            }
        }
    }

    let total_rows = row_idx;
    let (dist, best_row) = best_match?;
    if dist > MAX_NEAREST_LINE_DISTANCE {
        return None;
    }
    let lookahead = CHUNK_WINDOW_STEP_LINES / 4;
    let needed = best_row
        .saturating_add(lookahead)
        .max(best_row + 1)
        .min(total_rows);
    (needed > current_limit).then_some(needed)
}

fn measure_card_geometry(
    card_el: &web_sys::Element,
) -> Option<(web_sys::Element, ViewportGeometry)> {
    let main_el = card_el.closest(".diff-main").ok().flatten()?;
    let body_el = card_el.query_selector(".diff-card__body").ok().flatten()?;
    let main_rect = main_el.get_bounding_client_rect();
    let body_rect = body_el.get_bounding_client_rect();
    let header_bottom = card_el
        .query_selector(".diff-card__header")
        .ok()
        .flatten()
        .map_or(main_rect.top(), |h| h.get_bounding_client_rect().bottom());
    Some((
        main_el,
        ViewportGeometry {
            main_top: main_rect.top(),
            main_bottom: main_rect.bottom(),
            header_bottom,
            body_top: body_rect.top(),
            body_bottom: body_rect.bottom(),
        },
    ))
}

fn parse_candidate_line_span(el: &web_sys::Element, tab: MarkdownTab) -> Option<(u32, u32)> {
    match tab {
        MarkdownTab::Code => el
            .get_attribute("data-new-line")
            .and_then(|s| s.parse::<u32>().ok())
            .or_else(|| {
                el.get_attribute("data-old-line")
                    .and_then(|s| s.parse::<u32>().ok())
            })
            .map(|ln| (ln, ln)),
        MarkdownTab::Preview | MarkdownTab::Full => el
            .get_attribute("data-md-line")
            .and_then(|s| s.parse::<u32>().ok())
            .map(|start| {
                let end = el
                    .get_attribute("data-md-line-end")
                    .and_then(|s| s.parse::<u32>().ok())
                    .map_or(start, |e| e.max(start));
                (start, end)
            }),
    }
}

fn collect_dom_candidates_within(
    root_el: &web_sys::Element,
    tab: MarkdownTab,
    chunk_band: Option<VerticalBand>,
    out: &mut Vec<(web_sys::Element, LineAnchorCandidate)>,
) {
    let selector = match tab {
        MarkdownTab::Code => "tr[data-new-line], tr[data-old-line]",
        MarkdownTab::Preview | MarkdownTab::Full => "[data-md-line]",
    };
    let Ok(list) = root_el.query_selector_all(selector) else {
        return;
    };
    let len = list.length();
    out.reserve(len as usize);

    for i in 0..len {
        let Some(node) = list.item(i) else {
            continue;
        };
        let Ok(el) = node.dyn_into::<web_sys::Element>() else {
            continue;
        };
        let rect = el.get_bounding_client_rect();
        if rect.height() <= 0.0 {
            continue;
        }
        if let Some(band) = chunk_band
            && !row_within_chunk_band(rect.top(), rect.bottom(), band)
        {
            continue;
        }
        if let Some((start_line, end_line)) = parse_candidate_line_span(&el, tab) {
            out.push((
                el,
                LineAnchorCandidate {
                    start_line,
                    end_line,
                    top: rect.top(),
                    bottom: rect.bottom(),
                },
            ));
        }
    }
}

fn collect_dom_candidates(
    card_el: &web_sys::Element,
    tab: MarkdownTab,
) -> Vec<(web_sys::Element, LineAnchorCandidate)> {
    let mut out = Vec::new();
    collect_dom_candidates_within(card_el, tab, None, &mut out);
    out
}

fn collect_code_capture_candidates(
    card_el: &web_sys::Element,
    focus: VisibleCardFocus,
) -> Vec<(web_sys::Element, LineAnchorCandidate)> {
    let Ok(wrappers) = card_el.query_selector_all(".diff-chunk-wrapper") else {
        return collect_dom_candidates(card_el, MarkdownTab::Code);
    };
    let wrapper_count = wrappers.length();
    if wrapper_count == 0 {
        return collect_dom_candidates(card_el, MarkdownTab::Code);
    }

    let mut wrapper_els = Vec::with_capacity(wrapper_count as usize);
    let mut wrapper_bands = Vec::with_capacity(wrapper_count as usize);
    for i in 0..wrapper_count {
        let Some(node) = wrappers.item(i) else {
            continue;
        };
        let Ok(el) = node.dyn_into::<web_sys::Element>() else {
            continue;
        };
        let rect = el.get_bounding_client_rect();
        wrapper_bands.push(VerticalBand {
            top: rect.top(),
            bottom: rect.bottom(),
        });
        wrapper_els.push(el);
    }

    // Scope Code capture to wrappers near the viewport and clip rows to each wrapper's own box.
    // Under `content-visibility: auto`, Firefox can still report overflowing `<tr>` coordinates
    // from skipped offscreen wrappers above the viewport.
    let selected = select_visible_chunk_indices(&wrapper_bands, focus.card_band, focus.focal_y);
    let mut out = Vec::new();
    for idx in selected {
        collect_dom_candidates_within(
            &wrapper_els[idx],
            MarkdownTab::Code,
            Some(wrapper_bands[idx]),
            &mut out,
        );
    }
    out
}

/// Temporarily overrides `content-visibility: auto` on `.diff-chunk-wrapper` elements inside
/// `card_el` so newly mounted `Code` chunks lay out to their full height before `scroll_top` is
/// set, avoiding `contain-intrinsic-size` scroll clamping and skipped zero-height rows.
fn force_chunks_content_visibility_visible(card_el: &web_sys::Element) -> bool {
    let Ok(wrappers) = card_el.query_selector_all(".diff-chunk-wrapper") else {
        return false;
    };
    let mut forced_any = false;
    for i in 0..wrappers.length() {
        let Some(node) = wrappers.item(i) else {
            continue;
        };
        let Ok(html_el) = node.dyn_into::<web_sys::HtmlElement>() else {
            continue;
        };
        if html_el
            .style()
            .set_property("content-visibility", "visible")
            .is_ok()
        {
            forced_any = true;
        }
    }
    forced_any
}

/// Removes temporary inline `content-visibility` overrides on `.diff-chunk-wrapper` elements in `card_el`.
pub fn clear_chunk_content_visibility_overrides(card_el: &web_sys::Element) {
    let Ok(wrappers) = card_el.query_selector_all(".diff-chunk-wrapper") else {
        return;
    };
    for i in 0..wrappers.length() {
        let Some(node) = wrappers.item(i) else {
            continue;
        };
        let Ok(html_el) = node.dyn_into::<web_sys::HtmlElement>() else {
            continue;
        };
        let _ = html_el.style().remove_property("content-visibility");
    }
}

/// Captures the current scroll anchor for `card_el` before transitioning `current_tab -> target_tab`.
#[must_use]
pub fn capture_tab_scroll_anchor(
    card_el: &web_sys::Element,
    current_tab: MarkdownTab,
    target_tab: MarkdownTab,
) -> Option<TabScrollAnchor> {
    let (_, geom) = measure_card_geometry(card_el)?;
    let focus = compute_visible_card_focus(geom)?;
    let dom_candidates = match current_tab {
        MarkdownTab::Code => collect_code_capture_candidates(card_el, focus),
        MarkdownTab::Preview | MarkdownTab::Full => collect_dom_candidates(card_el, current_tab),
    };
    let candidates: Vec<LineAnchorCandidate> = dom_candidates.into_iter().map(|(_, c)| c).collect();
    capture_anchor_from_candidates(geom, &candidates, target_tab)
}

/// Retargets `anchor.source_line` to `target_line` when `source_line` is missing or more than
/// `max_drift` lines away from `target_line`.
pub fn ensure_anchor_near_line(anchor: &mut TabScrollAnchor, target_line: u32, max_drift: u32) {
    if anchor
        .source_line
        .is_none_or(|src| src.abs_diff(target_line) > max_drift)
    {
        anchor.source_line = Some(target_line);
    }
}

/// Computes an adjusted `scroll_top` so a focused element inside `[active_top, active_bottom]`
/// remains visible between `visible_top` and `main_bottom - 16.0`, or `None` if already visible.
#[must_use]
pub fn compute_focused_element_scroll_nudge(
    current_scroll_top: f64,
    visible_top: f64,
    main_bottom: f64,
    active_top: f64,
    active_bottom: f64,
) -> Option<f64> {
    const BOTTOM_MARGIN: f64 = 16.0;
    let safe_bottom = (main_bottom - BOTTOM_MARGIN).max(visible_top);
    if active_bottom > safe_bottom {
        let needed_down = active_bottom - safe_bottom;
        let max_down = (active_top - visible_top).max(0.0);
        let delta = needed_down.min(max_down);
        if delta > 1.0 {
            return Some((current_scroll_top + delta).max(0.0));
        }
    }
    if active_top < visible_top {
        let delta = active_top - visible_top;
        if delta < -1.0 {
            return Some((current_scroll_top + delta).max(0.0));
        }
    }
    None
}

fn align_matched_element(
    main_el: &web_sys::Element,
    matched_el: &web_sys::Element,
    start_line: u32,
    end_line: u32,
    anchor: TabScrollAnchor,
) {
    let Some(target_line) = anchor.source_line else {
        return;
    };
    if !matched_el.is_connected() {
        return;
    }
    let refreshed_rect = matched_el.get_bounding_client_rect();
    if refreshed_rect.height() <= 0.0 {
        return;
    }
    let refreshed_cand = LineAnchorCandidate {
        start_line,
        end_line,
        top: refreshed_rect.top(),
        bottom: refreshed_rect.bottom(),
    };
    let main_top = main_el.get_bounding_client_rect().top();
    let desired_y = main_top + anchor.viewport_offset;
    let actual_y = compute_restore_target_y(refreshed_cand, target_line);
    let residual = actual_y - desired_y;
    if residual.abs() > 1.0 {
        let corrected = (f64::from(main_el.scroll_top()) + residual).max(0.0);
        main_el.set_scroll_top(corrected.round() as i32);
    }
}

fn nudge_focused_comment_form(card_el: &web_sys::Element, main_el: &web_sys::Element) {
    let Some(active_el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.active_element())
    else {
        return;
    };
    let in_card = card_el.contains(Some(active_el.as_ref()));
    let in_header = active_el
        .closest(".diff-card__header")
        .ok()
        .flatten()
        .is_some();
    let comment_form = active_el.closest(".comment-form").ok().flatten();
    if !should_nudge_focused_element(in_card, in_header, comment_form.is_some()) {
        return;
    }
    let Some(focus_target) = comment_form else {
        return;
    };
    let focus_rect = focus_target.get_bounding_client_rect();
    if focus_rect.height() <= 0.0 {
        return;
    }
    let main_rect = main_el.get_bounding_client_rect();
    let header_bottom = card_el
        .query_selector(".diff-card__header")
        .ok()
        .flatten()
        .map_or(main_rect.top(), |h| h.get_bounding_client_rect().bottom());
    let visible_top = main_rect.top().max(header_bottom);
    if let Some(nudged) = compute_focused_element_scroll_nudge(
        f64::from(main_el.scroll_top()),
        visible_top,
        main_rect.bottom(),
        focus_rect.top(),
        focus_rect.bottom(),
    ) {
        main_el.set_scroll_top(nudged.round() as i32);
    }
}

/// Applies `anchor` to `.diff-main.scroll_top` once the destination tab DOM has mounted.
///
/// Returns [`Some`] when `Code` chunks had `content-visibility: auto` temporarily overridden so
/// the caller can release the override and re-verify alignment on the next animation frame.
#[must_use]
pub fn restore_tab_scroll_anchor(
    card_el: &web_sys::Element,
    anchor: TabScrollAnchor,
) -> Option<CodeRestoreFollowUp> {
    let forced_chunks =
        anchor.target_tab == MarkdownTab::Code && force_chunks_content_visibility_visible(card_el);

    let Some((main_el, geom)) = measure_card_geometry(card_el) else {
        if forced_chunks {
            clear_chunk_content_visibility_overrides(card_el);
        }
        return None;
    };
    let dom_candidates = collect_dom_candidates(card_el, anchor.target_tab);
    let candidates: Vec<LineAnchorCandidate> = dom_candidates.iter().map(|(_, c)| *c).collect();
    let current_scroll_top = f64::from(main_el.scroll_top());

    let Some(plan) = compute_restore_scroll_plan(anchor, current_scroll_top, geom, &candidates)
    else {
        if forced_chunks {
            clear_chunk_content_visibility_overrides(card_el);
        }
        return None;
    };
    main_el.set_scroll_top(plan.target_scroll_top.round() as i32);

    let matched = plan.matched_index.and_then(|idx| {
        dom_candidates
            .get(idx)
            .map(|(el, cand)| (el.clone(), cand.start_line, cand.end_line))
    });

    if let Some((ref matched_el, start_line, end_line)) = matched {
        align_matched_element(&main_el, matched_el, start_line, end_line, anchor);
    }

    nudge_focused_comment_form(card_el, &main_el);

    forced_chunks.then_some(CodeRestoreFollowUp { anchor, matched })
}

/// Releases temporary `Code` chunk `content-visibility` overrides after the restore frame paints
/// (so `contain-intrinsic-size: auto` has recorded true heights) and corrects any residual drift.
pub fn finish_code_scroll_restore(card_el: &web_sys::Element, follow_up: &CodeRestoreFollowUp) {
    clear_chunk_content_visibility_overrides(card_el);
    let Ok(Some(main_el)) = card_el.closest(".diff-main") else {
        return;
    };
    if let Some((ref matched_el, start_line, end_line)) = follow_up.matched {
        align_matched_element(&main_el, matched_el, start_line, end_line, follow_up.anchor);
    }
    nudge_focused_comment_form(card_el, &main_el);
}

/// Captures the viewport anchor on `card_el` and switches `md_tab` to `next_tab`.
pub fn switch_markdown_tab(
    card_el: Option<&web_sys::Element>,
    md_tab: RwSignal<MarkdownTab>,
    pending_md_scroll: RwSignal<Option<TabScrollAnchor>>,
    next_tab: MarkdownTab,
) {
    let current_tab = md_tab.get_untracked();
    if current_tab == next_tab {
        return;
    }
    let anchor = card_el.and_then(|el| capture_tab_scroll_anchor(el, current_tab, next_tab));
    pending_md_scroll.set(anchor);
    md_tab.set(next_tab);
}

/// Captures the viewport anchor, switches `md_tab`, and ensures restoration stays near `target_line`.
pub fn switch_markdown_tab_for_line(
    card_el: Option<&web_sys::Element>,
    md_tab: RwSignal<MarkdownTab>,
    pending_md_scroll: RwSignal<Option<TabScrollAnchor>>,
    next_tab: MarkdownTab,
    target_line: u32,
) {
    let current_tab = md_tab.get_untracked();
    if current_tab == next_tab {
        return;
    }
    let mut anchor = card_el.and_then(|el| capture_tab_scroll_anchor(el, current_tab, next_tab));
    if let Some(ref mut a) = anchor {
        ensure_anchor_near_line(a, target_line, TARGET_LINE_OVERRIDE_THRESHOLD);
    }
    pending_md_scroll.set(anchor);
    md_tab.set(next_tab);
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use alloc::sync::Arc;
    use tellit_types::{DiffChunk, DiffFile, DiffLine, DiffLineKind, FileStatus};

    fn base_geom(body_top: f64, body_bottom: f64) -> ViewportGeometry {
        ViewportGeometry {
            main_top: 50.0,
            main_bottom: 850.0,
            header_bottom: 90.0,
            body_top,
            body_bottom,
        }
    }

    #[test]
    fn interpolates_lines_proportionally_across_multiline_blocks() {
        let block = LineAnchorCandidate {
            start_line: 100,
            end_line: 150,
            top: -200.0,
            bottom: 800.0,
        };
        assert_eq!(interpolate_source_line(block, -200.0), 100);
        assert_eq!(interpolate_source_line(block, 300.0), 125);
        assert_eq!(interpolate_source_line(block, 800.0), 150);
        // Out-of-bounds focal_y clamps cleanly.
        assert_eq!(interpolate_source_line(block, -500.0), 100);
        assert_eq!(interpolate_source_line(block, 1200.0), 150);

        let single = LineAnchorCandidate {
            start_line: 42,
            end_line: 42,
            top: 200.0,
            bottom: 220.0,
        };
        assert_eq!(interpolate_source_line(single, 210.0), 42);
    }

    #[test]
    fn prefers_narrowest_containing_block_on_capture_and_restore() {
        let candidates = [
            // Outer diff wrapper: lines 10..=40
            LineAnchorCandidate {
                start_line: 10,
                end_line: 40,
                top: 100.0,
                bottom: 500.0,
            },
            // List: lines 12..=25
            LineAnchorCandidate {
                start_line: 12,
                end_line: 25,
                top: 150.0,
                bottom: 350.0,
            },
            // Narrowest list item: lines 16..=17
            LineAnchorCandidate {
                start_line: 16,
                end_line: 17,
                top: 220.0,
                bottom: 260.0,
            },
        ];

        let captured = select_capture_candidate(&candidates, 240.0).unwrap();
        assert_eq!(captured.start_line, 16);
        assert_eq!(captured.end_line, 17);

        let (idx, restored, dist) = select_restore_candidate(&candidates, 17).unwrap();
        assert_eq!(idx, 2);
        assert_eq!(dist, 0);
        assert_eq!(restored.start_line, 16);
        assert_eq!(restored.end_line, 17);
    }

    #[test]
    fn capture_and_restore_roundtrip_preserves_multiline_code_fence_midpoint() {
        let geom = base_geom(-1000.0, 2000.0);
        // Visible card region is 90..=850 (height 760), focal_y = 90 + 0.35 * 760 = 356.
        let fence = LineAnchorCandidate {
            start_line: 50,
            end_line: 150,
            top: -144.0,
            bottom: 856.0,
        };
        // focal_y = 356 is exactly halfway between -144 and 856 -> line 100.
        let anchor = capture_anchor_from_candidates(geom, &[fence], MarkdownTab::Code).unwrap();
        assert_eq!(anchor.source_line, Some(100));
        assert!((anchor.viewport_offset - (356.0 - 50.0)).abs() < 1e-6);

        // Restore in Code mode where row 100 currently sits at top=600 with scroll_top=400.
        let code_row = LineAnchorCandidate {
            start_line: 100,
            end_line: 100,
            top: 600.0,
            bottom: 620.0,
        };
        let code_plan = compute_restore_scroll_plan(anchor, 400.0, geom, &[code_row]).unwrap();
        // desired_y = 50 + 306 = 356, delta = 600 - 356 = +244 -> scroll_top = 644.
        assert_eq!(code_plan.matched_index, Some(0));
        assert!((code_plan.target_scroll_top - 644.0).abs() < 1e-6);

        // Roundtrip back to Full mode: fence currently starts at top=200 (height 1000).
        let dest_fence = LineAnchorCandidate {
            start_line: 50,
            end_line: 150,
            top: 200.0,
            bottom: 1200.0,
        };
        let full_anchor = TabScrollAnchor {
            target_tab: MarkdownTab::Full,
            ..anchor
        };
        let full_plan =
            compute_restore_scroll_plan(full_anchor, 300.0, geom, &[dest_fence]).unwrap();
        // Midpoint of dest_fence is 200 + 500 = 700, desired_y = 356, delta = +344 -> 644.
        assert_eq!(full_plan.matched_index, Some(0));
        assert!((full_plan.target_scroll_top - 644.0).abs() < 1e-6);
    }

    #[test]
    fn restore_falls_back_to_ratio_when_target_line_is_far_from_diff_hunks() {
        let geom = base_geom(-400.0, 1600.0);
        // Only diff hunk in Preview mode is at lines 5..=12, while user was on line 500 in Full mode.
        let preview_hunk = LineAnchorCandidate {
            start_line: 5,
            end_line: 12,
            top: 100.0,
            bottom: 200.0,
        };
        let anchor = TabScrollAnchor {
            target_tab: MarkdownTab::Preview,
            source_line: Some(500),
            viewport_offset: 250.0,
            fallback_ratio: 0.5,
        };
        let plan = compute_restore_scroll_plan(anchor, 1000.0, geom, &[preview_hunk]).unwrap();
        // fallback_y = -400 + 0.5 * 2000 = 600; desired_y = 50 + 250 = 300; delta = +300 -> 1300.
        assert_eq!(plan.matched_index, None);
        assert!((plan.target_scroll_top - 1300.0).abs() < 1e-6);
    }

    #[test]
    fn capture_at_card_top_keeps_zero_fallback_ratio() {
        let geom = base_geom(120.0, 920.0);
        let first_heading = LineAnchorCandidate {
            start_line: 1,
            end_line: 1,
            top: 130.0,
            bottom: 162.0,
        };
        let anchor =
            capture_anchor_from_candidates(geom, &[first_heading], MarkdownTab::Full).unwrap();
        assert_eq!(anchor.source_line, Some(1));
        assert_eq!(anchor.fallback_ratio, 0.0);
        assert!((anchor.viewport_offset - (130.0 - 50.0)).abs() < 1e-6);
    }

    #[test]
    fn computes_required_window_limit_for_large_markdown_code_tab() {
        let lines: Vec<DiffLine> = (1..=600)
            .map(|n| DiffLine {
                kind: DiffLineKind::Add,
                content: alloc::format!("line {n}"),
                old_line_number: None,
                new_line_number: Some(n),
                syntax_spans: Vec::new(),
                word_diff: Vec::new(),
            })
            .collect();
        let file = Arc::new(DiffFile {
            path: String::from("LARGE.md"),
            old_path: None,
            status: FileStatus::Added,
            additions: 600,
            deletions: 0,
            is_generated: false,
            chunks: alloc::vec![DiffChunk {
                old_start: 0,
                old_lines: 0,
                new_start: 1,
                new_lines: 600,
                header: String::from("@@ -0,0 +1,600 @@"),
                lines,
            }],
            chunks_loaded: true,
        });
        let chunks = crate::components::text_diff::expanded::merge_chunks_for_file(&file, None);

        // Line 50 is already inside the initial 150-line window.
        assert_eq!(
            required_window_limit_for_line(&chunks, DiffViewMode::Unified, 150, 50),
            None
        );
        // Line 400 (row index 399) requires expanding past 150.
        let needed_unified =
            required_window_limit_for_line(&chunks, DiffViewMode::Unified, 150, 400).unwrap();
        assert!((400..=600).contains(&needed_unified));

        let needed_split =
            required_window_limit_for_line(&chunks, DiffViewMode::Split, 150, 400).unwrap();
        assert!((400..=600).contains(&needed_split));

        // Once expanded to `needed_unified`, no further expansion is needed.
        assert_eq!(
            required_window_limit_for_line(&chunks, DiffViewMode::Unified, needed_unified, 400),
            None
        );
    }

    #[test]
    fn retargets_far_anchor_and_nudges_offscreen_focused_composer() {
        let mut far = TabScrollAnchor {
            target_tab: MarkdownTab::Code,
            source_line: Some(20),
            viewport_offset: 200.0,
            fallback_ratio: 0.1,
        };
        ensure_anchor_near_line(&mut far, 300, TARGET_LINE_OVERRIDE_THRESHOLD);
        assert_eq!(far.source_line, Some(300));

        let mut close = TabScrollAnchor {
            target_tab: MarkdownTab::Code,
            source_line: Some(292),
            viewport_offset: 200.0,
            fallback_ratio: 0.5,
        };
        ensure_anchor_near_line(&mut close, 300, TARGET_LINE_OVERRIDE_THRESHOLD);
        assert_eq!(close.source_line, Some(292));

        // Composer already visible inside [90, 834] needs no nudge.
        assert_eq!(
            compute_focused_element_scroll_nudge(500.0, 90.0, 850.0, 300.0, 460.0),
            None
        );
        // Composer clipped below main_bottom - 16 (834) nudges down by 66px.
        let down = compute_focused_element_scroll_nudge(500.0, 90.0, 850.0, 740.0, 900.0).unwrap();
        assert!((down - 566.0).abs() < 1e-6);
        // Composer tucked behind sticky header (active_top 40 < visible_top 90) nudges up by 50px.
        let up = compute_focused_element_scroll_nudge(500.0, 90.0, 850.0, 40.0, 200.0).unwrap();
        assert!((up - 450.0).abs() < 1e-6);
    }

    #[test]
    fn scopes_code_capture_to_visible_chunks_and_clips_overflowing_rows() {
        let visible = VerticalBand {
            top: 90.0,
            bottom: 650.0,
        };
        let chunks = [
            // Offscreen chunk above viewport (e.g., collapsed to 240px under content-visibility: auto).
            VerticalBand {
                top: -800.0,
                bottom: -560.0,
            },
            // Zero-height hidden chunk.
            VerticalBand {
                top: 100.0,
                bottom: 100.0,
            },
            // Intersecting chunks in viewport.
            VerticalBand {
                top: 120.0,
                bottom: 520.0,
            },
            VerticalBand {
                top: 520.0,
                bottom: 920.0,
            },
            // Offscreen chunk below viewport.
            VerticalBand {
                top: 920.0,
                bottom: 1160.0,
            },
        ];

        assert_eq!(
            select_visible_chunk_indices(&chunks, visible, 286.0),
            [2, 3]
        );

        // When viewport sits in a gap with no intersecting chunks, pick the nearest non-empty chunk.
        let gap_chunks = [
            VerticalBand {
                top: -200.0,
                bottom: 40.0,
            },
            VerticalBand {
                top: 700.0,
                bottom: 940.0,
            },
        ];
        assert_eq!(
            select_visible_chunk_indices(&gap_chunks, visible, 250.0),
            [0]
        );
        assert_eq!(
            select_visible_chunk_indices(&gap_chunks, visible, 600.0),
            [1]
        );

        // Reject overflowing rows reported outside a collapsed 240px chunk wrapper.
        let collapsed_chunk = VerticalBand {
            top: 0.0,
            bottom: 240.0,
        };
        assert!(row_within_chunk_band(20.0, 42.0, collapsed_chunk));
        assert!(row_within_chunk_band(240.5, 260.0, collapsed_chunk));
        assert!(!row_within_chunk_band(350.0, 372.0, collapsed_chunk));
        assert!(!row_within_chunk_band(50.0, 50.0, collapsed_chunk));
    }

    #[test]
    fn focus_nudge_ignores_sticky_header_tab_buttons() {
        // Clicking Preview / Full / Code focuses a button inside .diff-card__header.
        assert!(!should_nudge_focused_element(true, true, false));
        assert!(!should_nudge_focused_element(true, true, true));
        // Non-composer controls inside the card body or outside the card do not nudge.
        assert!(!should_nudge_focused_element(true, false, false));
        assert!(!should_nudge_focused_element(false, false, true));
        // Active comment composer inside the card body does nudge.
        assert!(should_nudge_focused_element(true, false, true));
    }
}
