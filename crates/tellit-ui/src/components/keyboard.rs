//! Keyboard cursor navigation engine and DOM scroll utilities for diff review.

extern crate alloc;

use alloc::string::String;
use std::collections::HashSet;

use tellit_types::{CommentSide, DiffFile, DiffLineKind};
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, KeyboardEvent, ScrollIntoViewOptions, ScrollLogicalPosition};

use crate::state::{self, DiffViewMode};

/// Focused row coordinates inside the active diff file list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
pub fn normalize_cursor_side(mut pos: CursorPosition, files: &[DiffFile]) -> CursorPosition {
    if let Some(line) = files
        .get(pos.file_index)
        .and_then(|f| f.chunks.get(pos.chunk_index))
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

fn has_content_on_side(pos: CursorPosition, files: &[DiffFile]) -> bool {
    let Some(line) = files
        .get(pos.file_index)
        .and_then(|f| f.chunks.get(pos.chunk_index))
        .and_then(|c| c.lines.get(pos.line_index))
    else {
        return false;
    };
    match pos.side {
        CommentSide::Old => line.old_line_number.is_some(),
        CommentSide::New => line.new_line_number.is_some(),
    }
}

fn advance_cursor(
    pos: CursorPosition,
    dir: NavigationDirection,
    files: &[DiffFile],
) -> Option<CursorPosition> {
    let total_files = files.len();
    if total_files == 0 {
        return None;
    }
    let mut file_idx = pos.file_index.min(total_files - 1);
    let mut chunk_idx = pos.chunk_index;
    let mut line_idx = isize::try_from(pos.line_index).unwrap_or(0);
    let mut wraps = 0usize;

    match dir {
        NavigationDirection::Next => {
            line_idx += 1;
            while wraps <= total_files {
                let valid = files
                    .get(file_idx)
                    .and_then(|f| f.chunks.get(chunk_idx))
                    .and_then(|c| {
                        usize::try_from(line_idx)
                            .ok()
                            .and_then(|li| c.lines.get(li))
                    })
                    .is_some();
                if valid {
                    return Some(CursorPosition {
                        file_index: file_idx,
                        chunk_index: chunk_idx,
                        line_index: usize::try_from(line_idx).unwrap_or(0),
                        side: pos.side,
                    });
                }

                chunk_idx += 1;
                line_idx = 0;
                let has_chunk = files
                    .get(file_idx)
                    .and_then(|f| f.chunks.get(chunk_idx))
                    .is_some();
                if !has_chunk {
                    file_idx += 1;
                    chunk_idx = 0;
                    line_idx = 0;
                    if file_idx >= total_files {
                        file_idx = 0;
                        wraps += 1;
                    }
                }
            }
        }
        NavigationDirection::Prev => {
            line_idx -= 1;
            while wraps <= total_files {
                if line_idx < 0 {
                    if chunk_idx == 0 {
                        if file_idx == 0 {
                            file_idx = total_files - 1;
                            wraps += 1;
                        } else {
                            file_idx -= 1;
                        }
                        let file = &files[file_idx];
                        if file.chunks.is_empty() {
                            chunk_idx = 0;
                            line_idx = -1;
                        } else {
                            chunk_idx = file.chunks.len() - 1;
                            line_idx = isize::try_from(file.chunks[chunk_idx].lines.len())
                                .unwrap_or(0)
                                - 1;
                        }
                    } else {
                        chunk_idx -= 1;
                        line_idx = isize::try_from(files[file_idx].chunks[chunk_idx].lines.len())
                            .unwrap_or(0)
                            - 1;
                    }
                }

                if line_idx >= 0
                    && let Ok(li) = usize::try_from(line_idx)
                    && files
                        .get(file_idx)
                        .and_then(|f| f.chunks.get(chunk_idx))
                        .and_then(|c| c.lines.get(li))
                        .is_some()
                {
                    return Some(CursorPosition {
                        file_index: file_idx,
                        chunk_index: chunk_idx,
                        line_index: li,
                        side: pos.side,
                    });
                }
                line_idx = -1;
            }
        }
    }

    None
}

fn matches_target(
    pos: CursorPosition,
    target: NavigationTarget,
    files: &[DiffFile],
    mode: DiffViewMode,
    reviewed_files: &HashSet<String>,
) -> bool {
    let Some(file) = files.get(pos.file_index) else {
        return false;
    };
    match target {
        NavigationTarget::File => pos.chunk_index == 0 && pos.line_index == 0,
        NavigationTarget::Line => {
            if reviewed_files.contains(&file.path) {
                return false;
            }
            mode == DiffViewMode::Unified || has_content_on_side(pos, files)
        }
        NavigationTarget::Chunk => {
            if reviewed_files.contains(&file.path) {
                return false;
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

/// Computes the next [`CursorPosition`] matching `target` in `dir`, wrapping around cleanly.
#[must_use]
pub fn find_next_position(
    start: Option<CursorPosition>,
    dir: NavigationDirection,
    target: NavigationTarget,
    files: &[DiffFile],
    mode: DiffViewMode,
    reviewed_files: &HashSet<String>,
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

    let origin = start.unwrap_or(CursorPosition {
        file_index: 0,
        chunk_index: 0,
        line_index: 0,
        side: CommentSide::New,
    });
    let mut current = if start.is_none() {
        if matches_target(origin, target, files, mode, reviewed_files) {
            return Some(normalize_cursor_side(origin, files));
        }
        origin
    } else {
        origin
    };

    for _ in 0..total_line_budget(files) {
        let next = advance_cursor(current, dir, files)?;
        if next.file_index == origin.file_index
            && next.chunk_index == origin.chunk_index
            && next.line_index == origin.line_index
            && start.is_some()
        {
            break;
        }
        current = next;
        if matches_target(current, target, files, mode, reviewed_files) {
            return Some(normalize_cursor_side(current, files));
        }
    }

    None
}

fn total_line_budget(files: &[DiffFile]) -> usize {
    files
        .iter()
        .map(|f| f.chunks.iter().map(|c| c.lines.len()).sum::<usize>())
        .sum::<usize>()
        .saturating_add(files.len())
        .max(1)
}

/// Returns the first unviewed file index strictly following `after_index`, wrapping around.
#[must_use]
pub fn next_unviewed_file_index(
    after_index: usize,
    files: &[DiffFile],
    reviewed_files: &HashSet<String>,
) -> Option<usize> {
    let total = files.len();
    for offset in 1..total {
        let idx = (after_index + offset) % total;
        if !reviewed_files.contains(&files[idx].path) {
            return Some(idx);
        }
    }
    None
}

/// Switches `cursor` between `Old` (left) and `New` (right) in split view across paired lines.
#[must_use]
pub fn switch_split_side(
    cursor: CursorPosition,
    side: CommentSide,
    files: &[DiffFile],
) -> CursorPosition {
    let Some(lines) = files
        .get(cursor.file_index)
        .and_then(|f| f.chunks.get(cursor.chunk_index))
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
pub fn cursor_scroll_id(
    pos: CursorPosition,
    files: &[DiffFile],
    mode: DiffViewMode,
) -> Option<String> {
    let file = files.get(pos.file_index)?;
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
            },
        ];

        let mut reviewed = HashSet::new();
        let first = find_next_position(
            None,
            NavigationDirection::Next,
            NavigationTarget::Line,
            &files,
            DiffViewMode::Unified,
            &reviewed,
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
        )
        .unwrap();
        assert_eq!(wrapped.file_index, 0);
        assert_eq!(wrapped.line_index, 0);
    }
}
