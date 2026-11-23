//! Keyboard cursor navigation engine and DOM scroll utilities for diff review.

extern crate alloc;

use alloc::string::String;
use core::borrow::Borrow;
use std::collections::HashSet;

use tellit_types::{CommentSide, DiffFile, DiffLineKind};
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, KeyboardEvent, ScrollIntoViewOptions, ScrollLogicalPosition};

use crate::state::{self, DiffViewMode};

/// Focused row coordinates inside the active diff file list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CursorPosition {
    /// Index into `JjDiffResponse::files`.
    pub file_index: usize,
    /// Index into `DiffFile::chunks`.
    pub chunk_index: usize,
    /// Index into `DiffChunk::lines`.
    pub line_index: usize,
    /// Active diff pane side (`Old` on left split pane, `New` on right or unified).
    pub side: CommentSide,
}

/// Relative traversal direction for keyboard shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationDirection {
    /// Move forward (`j`, `n`, `]`, `Shift+N`).
    Next,
    /// Move backward (`k`, `p`, `[`, `Shift+P`).
    Prev,
}

/// Filters what kind of diff target a keyboard jump lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationTarget {
    /// Every visible diff line (on the active side in split mode).
    Line,
    /// First non-context line of each contiguous change run.
    Chunk,
    /// First line (or card header) of each file.
    File,
}

/// Returns `true` when `ev` comes from a form control or holds `Ctrl`/`Meta`/`Alt` modifiers.
#[must_use]
pub fn should_ignore_shortcut(ev: &KeyboardEvent) -> bool {
    if ev.ctrl_key() || ev.meta_key() || ev.alt_key() || ev.is_composing() {
        return true;
    }
    let Some(target) = ev.target() else {
        return false;
    };
    let Some(el) = target.dyn_ref::<web_sys::Element>() else {
        return false;
    };
    let tag = el.tag_name();
    if tag.eq_ignore_ascii_case("INPUT")
        || tag.eq_ignore_ascii_case("TEXTAREA")
        || tag.eq_ignore_ascii_case("SELECT")
    {
        return true;
    }
    el.dyn_ref::<HtmlElement>()
        .is_some_and(|html| html.is_content_editable())
}

/// Scrolls `element_id` into view smoothly inside the diff pane without jumping if already centered.
///
/// Returns `true` when the element was found in the document and scrolled.
pub fn scroll_to_element_id(element_id: &str, block: ScrollLogicalPosition) -> bool {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return false;
    };
    let Some(el) = doc.get_element_by_id(element_id) else {
        return false;
    };
    let opts = ScrollIntoViewOptions::new();
    opts.set_block(block);
    el.scroll_into_view_with_scroll_into_view_options(&opts);
    true
}

/// Adjusts `pos.side` when a split-pane row only has content on the opposite side.
#[must_use]
pub fn normalize_cursor_side<F: Borrow<DiffFile>>(
    mut pos: CursorPosition,
    files: &[F],
) -> CursorPosition {
    if let Some(line) = files
        .get(pos.file_index)
        .and_then(|f| f.borrow().chunks.get(pos.chunk_index))
        .and_then(|c| c.lines.get(pos.line_index))
    {
        match line.kind {
            DiffLineKind::Add => pos.side = CommentSide::New,
            DiffLineKind::Delete => pos.side = CommentSide::Old,
            DiffLineKind::Context => {}
        }
    }
    pos
}

fn has_content_on_side<F: Borrow<DiffFile>>(pos: CursorPosition, files: &[F]) -> bool {
    let Some(line) = files
        .get(pos.file_index)
        .and_then(|f| f.borrow().chunks.get(pos.chunk_index))
        .and_then(|c| c.lines.get(pos.line_index))
    else {
        return false;
    };
    match pos.side {
        CommentSide::Old => line.old_line_number.is_some(),
        CommentSide::New => line.new_line_number.is_some(),
    }
}

fn is_unhydrated_navigable_file(file: &DiffFile) -> bool {
    !file.chunks_loaded
        && (file.additions > 0 || file.deletions > 0)
        && !state::is_image_file(&file.path)
}

fn is_file_skipped(
    file: &DiffFile,
    reviewed_files: &HashSet<String>,
    collapsed_files: &HashSet<String>,
) -> bool {
    let has_navigable_lines = !file.chunks.is_empty() || is_unhydrated_navigable_file(file);
    !has_navigable_lines
        || reviewed_files.contains(&file.path)
        || collapsed_files.contains(&file.path)
}

fn next_unskipped_file_from<F: Borrow<DiffFile>>(
    mut file_idx: usize,
    dir: NavigationDirection,
    files: &[F],
    reviewed_files: &HashSet<String>,
    collapsed_files: &HashSet<String>,
) -> Option<usize> {
    let total = files.len();
    for _ in 0..total {
        if !is_file_skipped(files[file_idx].borrow(), reviewed_files, collapsed_files) {
            return Some(file_idx);
        }
        file_idx = match dir {
            NavigationDirection::Next => (file_idx + 1) % total,
            NavigationDirection::Prev => (file_idx + total - 1) % total,
        };
    }
    None
}

fn last_line_in_file(file: &DiffFile) -> Option<(usize, usize)> {
    if is_unhydrated_navigable_file(file) {
        return Some((0, 0));
    }
    for (ci, chunk) in file.chunks.iter().enumerate().rev() {
        if !chunk.lines.is_empty() {
            return Some((ci, chunk.lines.len() - 1));
        }
    }
    None
}

fn first_line_in_file(file: &DiffFile) -> Option<(usize, usize)> {
    if is_unhydrated_navigable_file(file) {
        return Some((0, 0));
    }
    for (ci, chunk) in file.chunks.iter().enumerate() {
        if !chunk.lines.is_empty() {
            return Some((ci, 0));
        }
    }
    None
}

/// Steps one line forward or backward while skipping reviewed and collapsed files in `O(1)` per file.
fn advance_cursor<F: Borrow<DiffFile>>(
    pos: CursorPosition,
    dir: NavigationDirection,
    files: &[F],
    reviewed_files: &HashSet<String>,
    collapsed_files: &HashSet<String>,
) -> Option<CursorPosition> {
    let total_files = files.len();
    if total_files == 0 {
        return None;
    }

    let cur_file_idx = pos.file_index.min(total_files - 1);
    let cur_file = files[cur_file_idx].borrow();

    // If the cursor sits on a skipped file, jump directly to the next open file without
    // scanning any lines inside skipped files.
    if is_file_skipped(cur_file, reviewed_files, collapsed_files) {
        let start_search = match dir {
            NavigationDirection::Next => (cur_file_idx + 1) % total_files,
            NavigationDirection::Prev => (cur_file_idx + total_files - 1) % total_files,
        };
        let next_file_idx =
            next_unskipped_file_from(start_search, dir, files, reviewed_files, collapsed_files)?;
        let target_file = files[next_file_idx].borrow();
        let (ci, li) = match dir {
            NavigationDirection::Next => first_line_in_file(target_file)?,
            NavigationDirection::Prev => last_line_in_file(target_file)?,
        };
        return Some(CursorPosition {
            file_index: next_file_idx,
            chunk_index: ci,
            line_index: li,
            side: pos.side,
        });
    }

    match dir {
        NavigationDirection::Next => {
            if let Some(chunk) = cur_file.chunks.get(pos.chunk_index)
                && pos.line_index + 1 < chunk.lines.len()
            {
                return Some(CursorPosition {
                    file_index: cur_file_idx,
                    chunk_index: pos.chunk_index,
                    line_index: pos.line_index + 1,
                    side: pos.side,
                });
            }
            for ci in (pos.chunk_index + 1)..cur_file.chunks.len() {
                if !cur_file.chunks[ci].lines.is_empty() {
                    return Some(CursorPosition {
                        file_index: cur_file_idx,
                        chunk_index: ci,
                        line_index: 0,
                        side: pos.side,
                    });
                }
            }
            let next_file_idx = next_unskipped_file_from(
                (cur_file_idx + 1) % total_files,
                NavigationDirection::Next,
                files,
                reviewed_files,
                collapsed_files,
            )?;
            let (ci, li) = first_line_in_file(files[next_file_idx].borrow())?;
            Some(CursorPosition {
                file_index: next_file_idx,
                chunk_index: ci,
                line_index: li,
                side: pos.side,
            })
        }
        NavigationDirection::Prev => {
            if pos.line_index > 0
                && cur_file
                    .chunks
                    .get(pos.chunk_index)
                    .is_some_and(|c| pos.line_index - 1 < c.lines.len())
            {
                return Some(CursorPosition {
                    file_index: cur_file_idx,
                    chunk_index: pos.chunk_index,
                    line_index: pos.line_index - 1,
                    side: pos.side,
                });
            }
            for ci in (0..pos.chunk_index.min(cur_file.chunks.len())).rev() {
                if !cur_file.chunks[ci].lines.is_empty() {
                    return Some(CursorPosition {
                        file_index: cur_file_idx,
                        chunk_index: ci,
                        line_index: cur_file.chunks[ci].lines.len() - 1,
                        side: pos.side,
                    });
                }
            }
            let prev_file_idx = next_unskipped_file_from(
                (cur_file_idx + total_files - 1) % total_files,
                NavigationDirection::Prev,
                files,
                reviewed_files,
                collapsed_files,
            )?;
            let (ci, li) = last_line_in_file(files[prev_file_idx].borrow())?;
            Some(CursorPosition {
                file_index: prev_file_idx,
                chunk_index: ci,
                line_index: li,
                side: pos.side,
            })
        }
    }
}

fn matches_target<F: Borrow<DiffFile>>(
    pos: CursorPosition,
    target: NavigationTarget,
    files: &[F],
    mode: DiffViewMode,
    reviewed_files: &HashSet<String>,
    collapsed_files: &HashSet<String>,
) -> bool {
    let Some(file) = files.get(pos.file_index).map(Borrow::borrow) else {
        return false;
    };
    match target {
        NavigationTarget::File => pos.chunk_index == 0 && pos.line_index == 0,
        NavigationTarget::Line => {
            if is_file_skipped(file, reviewed_files, collapsed_files) {
                return false;
            }
            if is_unhydrated_navigable_file(file) {
                return pos.chunk_index == 0 && pos.line_index == 0;
            }
            mode == DiffViewMode::Unified || has_content_on_side(pos, files)
        }
        NavigationTarget::Chunk => {
            if is_file_skipped(file, reviewed_files, collapsed_files) {
                return false;
            }
            if is_unhydrated_navigable_file(file) {
                return pos.chunk_index == 0 && pos.line_index == 0;
            }
            let Some(chunk) = file.chunks.get(pos.chunk_index) else {
                return false;
            };
            let Some(line) = chunk.lines.get(pos.line_index) else {
                return false;
            };
            if line.kind == DiffLineKind::Context {
                return false;
            }
            pos.line_index == 0
                || chunk
                    .lines
                    .get(pos.line_index - 1)
                    .is_none_or(|prev| prev.kind == DiffLineKind::Context)
        }
    }
}

/// Computes the next [`CursorPosition`] matching `target` in `dir`, skipping collapsed and
/// reviewed files in `O(1)` per file and wrapping around cleanly.
#[must_use]
pub fn find_next_position<F: Borrow<DiffFile>>(
    start: Option<CursorPosition>,
    dir: NavigationDirection,
    target: NavigationTarget,
    files: &[F],
    mode: DiffViewMode,
    reviewed_files: &HashSet<String>,
    collapsed_files: &HashSet<String>,
) -> Option<CursorPosition> {
    if files.is_empty() {
        return None;
    }

    if target == NavigationTarget::File {
        let cur_file = start.map_or_else(
            || match dir {
                NavigationDirection::Next => files.len() - 1,
                NavigationDirection::Prev => 0,
            },
            |s| s.file_index.min(files.len() - 1),
        );
        let next_file = match dir {
            NavigationDirection::Next => (cur_file + 1) % files.len(),
            NavigationDirection::Prev => (cur_file + files.len() - 1) % files.len(),
        };
        let default_side = if mode == DiffViewMode::Split {
            CommentSide::Old
        } else {
            CommentSide::New
        };
        return Some(normalize_cursor_side(
            CursorPosition {
                file_index: next_file,
                chunk_index: 0,
                line_index: 0,
                side: default_side,
            },
            files,
        ));
    }

    let origin = match start {
        Some(pos) => pos,
        None => {
            let start_idx = match dir {
                NavigationDirection::Next => 0,
                NavigationDirection::Prev => 0,
            };
            let first_open =
                next_unskipped_file_from(start_idx, dir, files, reviewed_files, collapsed_files)?;
            let first_file = files[first_open].borrow();
            let (ci, li) = match dir {
                NavigationDirection::Next => first_line_in_file(first_file)?,
                NavigationDirection::Prev => {
                    if first_open == 0 {
                        first_line_in_file(first_file)?
                    } else {
                        last_line_in_file(first_file)?
                    }
                }
            };
            let candidate = CursorPosition {
                file_index: first_open,
                chunk_index: ci,
                line_index: li,
                side: CommentSide::New,
            };
            if matches_target(
                candidate,
                target,
                files,
                mode,
                reviewed_files,
                collapsed_files,
            ) {
                return Some(normalize_cursor_side(candidate, files));
            }
            candidate
        }
    };

    let mut current = origin;
    let budget = open_line_budget(files, reviewed_files, collapsed_files);
    for _ in 0..budget {
        let next = advance_cursor(current, dir, files, reviewed_files, collapsed_files)?;
        if next.file_index == origin.file_index
            && next.chunk_index == origin.chunk_index
            && next.line_index == origin.line_index
            && start.is_some()
        {
            break;
        }
        current = next;
        if matches_target(
            current,
            target,
            files,
            mode,
            reviewed_files,
            collapsed_files,
        ) {
            return Some(normalize_cursor_side(current, files));
        }
    }

    None
}

fn open_line_budget<F: Borrow<DiffFile>>(
    files: &[F],
    reviewed_files: &HashSet<String>,
    collapsed_files: &HashSet<String>,
) -> usize {
    files
        .iter()
        .map(Borrow::borrow)
        .filter(|f| !is_file_skipped(f, reviewed_files, collapsed_files))
        .map(|f| {
            if is_unhydrated_navigable_file(f) {
                1
            } else {
                f.chunks.iter().map(|c| c.lines.len()).sum::<usize>()
            }
        })
        .sum::<usize>()
        .saturating_add(1)
}

/// Returns the adjacent file index in `0..total` without wrapping (for single-file stepping).
#[must_use]
pub fn adjacent_file_index(
    current: usize,
    total: usize,
    dir: NavigationDirection,
) -> Option<usize> {
    if total == 0 || current >= total {
        return None;
    }
    match dir {
        NavigationDirection::Next => (current + 1 < total).then_some(current + 1),
        NavigationDirection::Prev => current.checked_sub(1),
    }
}

/// Returns the first unviewed file index strictly following `after_index`, wrapping around.
#[must_use]
pub fn next_unviewed_file_index<F: Borrow<DiffFile>>(
    after_index: usize,
    files: &[F],
    reviewed_files: &HashSet<String>,
) -> Option<usize> {
    let total = files.len();
    for offset in 1..total {
        let idx = (after_index + offset) % total;
        if !reviewed_files.contains(&files[idx].borrow().path) {
            return Some(idx);
        }
    }
    None
}

/// Switches `cursor` between `Old` (left) and `New` (right) in split view across paired lines.
#[must_use]
pub fn switch_split_side<F: Borrow<DiffFile>>(
    cursor: CursorPosition,
    side: CommentSide,
    files: &[F],
) -> CursorPosition {
    let Some(lines) = files
        .get(cursor.file_index)
        .and_then(|f| f.borrow().chunks.get(cursor.chunk_index))
        .map(|c| c.lines.as_slice())
    else {
        return cursor;
    };
    let Some(cur_line) = lines.get(cursor.line_index) else {
        return cursor;
    };

    if side == CommentSide::Old
        && cur_line.kind == DiffLineKind::Add
        && let Some(prev_idx) = cursor.line_index.checked_sub(1)
        && lines
            .get(prev_idx)
            .is_some_and(|l| l.kind == DiffLineKind::Delete)
    {
        return CursorPosition {
            line_index: prev_idx,
            side,
            ..cursor
        };
    }

    if side == CommentSide::New
        && cur_line.kind == DiffLineKind::Delete
        && lines
            .get(cursor.line_index + 1)
            .is_some_and(|l| l.kind == DiffLineKind::Add)
    {
        return CursorPosition {
            line_index: cursor.line_index + 1,
            side,
            ..cursor
        };
    }

    let candidate = CursorPosition { side, ..cursor };
    if has_content_on_side(candidate, files) {
        return candidate;
    }

    for i in (cursor.line_index + 1)..lines.len() {
        let probe = CursorPosition {
            line_index: i,
            side,
            ..cursor
        };
        if has_content_on_side(probe, files) {
            return probe;
        }
    }
    for i in (0..cursor.line_index).rev() {
        let probe = CursorPosition {
            line_index: i,
            side,
            ..cursor
        };
        if has_content_on_side(probe, files) {
            return probe;
        }
    }

    cursor
}

/// Computes the DOM element ID to scroll to for a given [`CursorPosition`].
#[must_use]
pub fn cursor_scroll_id<F: Borrow<DiffFile>>(
    pos: CursorPosition,
    files: &[F],
    mode: DiffViewMode,
) -> Option<String> {
    let file = files.get(pos.file_index)?.borrow();
    let line = file
        .chunks
        .get(pos.chunk_index)
        .and_then(|c| c.lines.get(pos.line_index))?;
    let side = if mode == DiffViewMode::Unified {
        if line.kind == DiffLineKind::Delete {
            CommentSide::Old
        } else {
            CommentSide::New
        }
    } else {
        pos.side
    };
    Some(state::diff_line_element_id(
        &file.path,
        pos.chunk_index,
        pos.line_index,
        side,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tellit_types::{DiffChunk, DiffLine, FileStatus};

    fn mk_line(kind: DiffLineKind, old_n: Option<u32>, new_n: Option<u32>) -> DiffLine {
        DiffLine {
            kind,
            content: String::from("line"),
            old_line_number: old_n,
            new_line_number: new_n,
            syntax_spans: Vec::new(),
            word_diff: Vec::new(),
        }
    }

    fn mk_file(path: &str, lines: alloc::vec::Vec<DiffLine>) -> DiffFile {
        let len = u32::try_from(lines.len()).unwrap_or(u32::MAX);
        DiffFile {
            path: String::from(path),
            old_path: None,
            status: FileStatus::Modified,
            additions: len,
            deletions: 0,
            is_generated: false,
            chunks: alloc::vec![DiffChunk {
                old_start: 1,
                old_lines: len,
                new_start: 1,
                new_lines: len,
                header: String::from("@@ -1,1 +1,1 @@"),
                lines,
            }],
            chunks_loaded: true,
        }
    }

    #[test]
    fn navigates_lines_chunks_and_skips_reviewed_files() {
        let files = alloc::vec![
            DiffFile {
                path: String::from("a.rs"),
                old_path: None,
                status: FileStatus::Modified,
                additions: 1,
                deletions: 1,
                is_generated: false,
                chunks: alloc::vec![DiffChunk {
                    old_start: 1,
                    old_lines: 2,
                    new_start: 1,
                    new_lines: 2,
                    header: String::from("@@ -1,2 +1,2 @@"),
                    lines: alloc::vec![
                        mk_line(DiffLineKind::Context, Some(1), Some(1)),
                        mk_line(DiffLineKind::Delete, Some(2), None),
                        mk_line(DiffLineKind::Add, None, Some(2)),
                    ],
                }],
                chunks_loaded: true,
            },
            DiffFile {
                path: String::from("b.rs"),
                old_path: None,
                status: FileStatus::Added,
                additions: 1,
                deletions: 0,
                is_generated: false,
                chunks: alloc::vec![DiffChunk {
                    old_start: 0,
                    old_lines: 0,
                    new_start: 1,
                    new_lines: 1,
                    header: String::from("@@ -0,0 +1,1 @@"),
                    lines: alloc::vec![mk_line(DiffLineKind::Add, None, Some(1))],
                }],
                chunks_loaded: true,
            },
        ];

        let mut reviewed = HashSet::new();
        let collapsed = HashSet::new();
        let first = find_next_position(
            None,
            NavigationDirection::Next,
            NavigationTarget::Line,
            &files,
            DiffViewMode::Unified,
            &reviewed,
            &collapsed,
        )
        .unwrap();
        assert_eq!(first.file_index, 0);
        assert_eq!(first.line_index, 0);

        let chunk = find_next_position(
            Some(first),
            NavigationDirection::Next,
            NavigationTarget::Chunk,
            &files,
            DiffViewMode::Unified,
            &reviewed,
            &collapsed,
        )
        .unwrap();
        assert_eq!(chunk.line_index, 1);
        assert_eq!(chunk.side, CommentSide::Old);

        // Switching from Delete (line_index 1) to New hops directly to paired Add (line_index 2).
        let switched = switch_split_side(chunk, CommentSide::New, &files);
        assert_eq!(switched.line_index, 2);
        assert_eq!(switched.side, CommentSide::New);

        // Reviewed file "b.rs" is skipped when advancing lines.
        reviewed.insert(String::from("b.rs"));
        let wrapped = find_next_position(
            Some(switched),
            NavigationDirection::Next,
            NavigationTarget::Line,
            &files,
            DiffViewMode::Unified,
            &reviewed,
            &collapsed,
        )
        .unwrap();
        assert_eq!(wrapped.file_index, 0);
        assert_eq!(wrapped.line_index, 0);
    }

    #[test]
    fn skips_collapsed_files_in_both_directions_and_returns_none_when_all_collapsed() {
        let huge_lines: alloc::vec::Vec<DiffLine> = (1..=500)
            .map(|n| mk_line(DiffLineKind::Add, None, Some(n)))
            .collect();
        let files = alloc::vec![
            mk_file(
                "a.rs",
                alloc::vec![mk_line(DiffLineKind::Add, None, Some(1))]
            ),
            mk_file("big_collapsed.rs", huge_lines),
            mk_file(
                "c.rs",
                alloc::vec![mk_line(DiffLineKind::Add, None, Some(1))]
            ),
        ];

        let reviewed = HashSet::new();
        let mut collapsed = HashSet::new();
        collapsed.insert(String::from("big_collapsed.rs"));

        let at_a = CursorPosition {
            file_index: 0,
            chunk_index: 0,
            line_index: 0,
            side: CommentSide::New,
        };
        let next = find_next_position(
            Some(at_a),
            NavigationDirection::Next,
            NavigationTarget::Line,
            &files,
            DiffViewMode::Unified,
            &reviewed,
            &collapsed,
        )
        .unwrap();
        assert_eq!(next.file_index, 2);

        let prev = find_next_position(
            Some(next),
            NavigationDirection::Prev,
            NavigationTarget::Line,
            &files,
            DiffViewMode::Unified,
            &reviewed,
            &collapsed,
        )
        .unwrap();
        assert_eq!(prev.file_index, 0);

        // Collapsing every file returns None immediately.
        collapsed.insert(String::from("a.rs"));
        collapsed.insert(String::from("c.rs"));
        assert_eq!(
            find_next_position(
                None,
                NavigationDirection::Next,
                NavigationTarget::Line,
                &files,
                DiffViewMode::Unified,
                &reviewed,
                &collapsed,
            ),
            None
        );
    }

    #[test]
    fn advances_onto_uncollapsed_unhydrated_text_files_instead_of_wrapping() {
        let mk_summary = |path: &str, additions: u32, deletions: u32| DiffFile {
            path: String::from(path),
            old_path: None,
            status: FileStatus::Modified,
            additions,
            deletions,
            is_generated: false,
            chunks: Vec::new(),
            chunks_loaded: false,
        };

        let files = alloc::vec![
            mk_file(
                "Commit Message",
                alloc::vec![mk_line(DiffLineKind::Add, None, Some(1))],
            ),
            mk_summary("logo.png", 1, 0),
            mk_summary("src/lazy_collapsed.rs", 10, 2),
            mk_summary("src/lazy_open.rs", 5, 0),
        ];

        let reviewed = HashSet::new();
        let mut collapsed = HashSet::new();
        collapsed.insert(String::from("src/lazy_collapsed.rs"));

        let at_commit_msg = CursorPosition {
            file_index: 0,
            chunk_index: 0,
            line_index: 0,
            side: CommentSide::New,
        };

        // `j` (Line) skips image + collapsed summary and lands on virtual (0, 0) of `src/lazy_open.rs`.
        let next_line = find_next_position(
            Some(at_commit_msg),
            NavigationDirection::Next,
            NavigationTarget::Line,
            &files,
            DiffViewMode::Split,
            &reviewed,
            &collapsed,
        )
        .unwrap();
        assert_eq!(next_line.file_index, 3);
        assert_eq!(next_line.chunk_index, 0);
        assert_eq!(next_line.line_index, 0);

        // `n` (Chunk) also lands on `src/lazy_open.rs` instead of wrapping to file 0.
        let next_chunk = find_next_position(
            Some(at_commit_msg),
            NavigationDirection::Next,
            NavigationTarget::Chunk,
            &files,
            DiffViewMode::Unified,
            &reviewed,
            &collapsed,
        )
        .unwrap();
        assert_eq!(next_chunk.file_index, 3);
        assert_eq!(next_chunk.chunk_index, 0);
        assert_eq!(next_chunk.line_index, 0);

        // Pressing `j` again from the unhydrated file wraps cleanly back to file 0.
        let wrapped = find_next_position(
            Some(next_line),
            NavigationDirection::Next,
            NavigationTarget::Line,
            &files,
            DiffViewMode::Split,
            &reviewed,
            &collapsed,
        )
        .unwrap();
        assert_eq!(wrapped.file_index, 0);
    }

    #[test]
    fn adjacent_file_index_and_next_unviewed_cover_single_file_transitions() {
        assert_eq!(adjacent_file_index(0, 0, NavigationDirection::Next), None);
        assert_eq!(adjacent_file_index(0, 3, NavigationDirection::Prev), None);
        assert_eq!(
            adjacent_file_index(0, 3, NavigationDirection::Next),
            Some(1)
        );
        assert_eq!(
            adjacent_file_index(2, 3, NavigationDirection::Prev),
            Some(1)
        );
        assert_eq!(adjacent_file_index(2, 3, NavigationDirection::Next), None);

        let files = alloc::vec![
            mk_file("Commit Message", Vec::new()),
            mk_file("src/a.rs", Vec::new()),
            mk_file("src/b.rs", Vec::new()),
        ];
        let mut reviewed = HashSet::new();
        reviewed.insert(String::from("Commit Message"));
        assert_eq!(next_unviewed_file_index(0, &files, &reviewed), Some(1));
        reviewed.insert(String::from("src/a.rs"));
        assert_eq!(next_unviewed_file_index(1, &files, &reviewed), Some(2));
        reviewed.insert(String::from("src/b.rs"));
        // All viewed -> returns None so Shift+V returns cleanly to Change Overview.
        assert_eq!(next_unviewed_file_index(2, &files, &reviewed), None);
    }
}
