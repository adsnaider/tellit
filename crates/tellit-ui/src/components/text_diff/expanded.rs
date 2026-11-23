//! Hidden context line expansion and adjacent hunk merging state.

extern crate alloc;

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use std::collections::HashMap;

use tellit_types::{DiffChunk, DiffFile, DiffLine, DiffLineKind, FileStatus};

/// Default number of context lines revealed per expansion click.
pub const DEFAULT_EXPAND_COUNT: u32 = 20;

/// Direction relative to an existing diff hunk in which hidden lines are expanded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpandDirection {
    /// Reveal lines immediately preceding the hunk.
    Up,
    /// Reveal lines immediately following the hunk.
    Down,
}

/// Recorded expansion count anchored to one original hunk index and direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpandedRange {
    /// Original index inside [`DiffFile::chunks`].
    pub chunk_index: usize,
    /// Expansion direction relative to `chunk_index`.
    pub direction: ExpandDirection,
    /// Cumulative number of lines expanded along that edge.
    pub count: u32,
}

/// Cached full file contents, line counts, and active expansion ranges for one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileExpandedState {
    /// Lines of the base revision (`old`) file when fetched.
    pub old_content: Option<Vec<String>>,
    /// Lines of the target revision (`new`) file when fetched.
    pub new_content: Option<Vec<String>>,
    /// Total lines in the base revision file.
    pub old_total_lines: Option<u32>,
    /// Total lines in the target revision file.
    pub new_total_lines: Option<u32>,
    /// Active line expansions anchored to original chunk indices.
    pub expanded_ranges: Vec<ExpandedRange>,
}

/// Reference to one line inside a [`MergedChunk`], avoiding cloning original [`DiffLine`] entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkLineRef {
    /// Original line indexed by `(chunk_index, line_index)` inside `DiffFile::chunks`.
    Original(usize, usize),
    /// Context line materialized on demand from `/api/blob`.
    Expanded(Arc<DiffLine>),
}

impl ChunkLineRef {
    /// Returns `true` when this line came from `/api/blob` context expansion.
    #[must_use]
    pub fn is_expanded(&self) -> bool {
        matches!(self, Self::Expanded(_))
    }

    /// Returns the original `(chunk_index, line_index)` coordinate when this line is not expanded.
    #[must_use]
    pub fn source(&self) -> Option<(usize, usize)> {
        match self {
            Self::Original(ci, li) => Some((*ci, *li)),
            Self::Expanded(_) => None,
        }
    }
}

/// Diff hunk combined with expanded context lines and merged adjacent hunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedChunk {
    /// Shared owning file whose original chunk lines are referenced by [`ChunkLineRef::Original`].
    pub file: Arc<DiffFile>,
    /// Start line on the old side after upward expansion.
    pub old_start: u32,
    /// Line span on the old side across merged hunks.
    pub old_lines: u32,
    /// Start line on the new side after upward expansion.
    pub new_start: u32,
    /// Line span on the new side across merged hunks.
    pub new_lines: u32,
    /// Original hunk header string.
    pub header: String,
    /// Ordered references to diff and expanded context lines.
    pub lines: Vec<ChunkLineRef>,
    /// Sorted original indices in [`DiffFile::chunks`] merged into this block.
    pub original_indices: Vec<usize>,
    /// Hidden lines still collapsed immediately above this block.
    pub hidden_lines_before: u32,
    /// Hidden lines still collapsed immediately below the last block (`None` when total file length is unknown).
    pub hidden_lines_after: Option<u32>,
}

impl MergedChunk {
    /// Resolves `line_ref` into a borrowed [`DiffLine`] without cloning.
    #[must_use]
    pub fn resolve_line<'a>(&'a self, line_ref: &'a ChunkLineRef) -> Option<&'a DiffLine> {
        match line_ref {
            ChunkLineRef::Original(ci, li) => self.file.chunks.get(*ci)?.lines.get(*li),
            ChunkLineRef::Expanded(line) => Some(line.as_ref()),
        }
    }

    /// Resolves the line at `idx` inside this merged chunk without cloning.
    #[must_use]
    pub fn line(&self, idx: usize) -> Option<&DiffLine> {
        self.resolve_line(self.lines.get(idx)?)
    }

    /// Iterates over every resolved [`DiffLine`] in order without cloning.
    pub fn iter_lines(&self) -> impl Iterator<Item = &DiffLine> {
        self.lines.iter().filter_map(|r| self.resolve_line(r))
    }

    /// Returns `true` when the line at `idx` came from context expansion.
    #[must_use]
    pub fn is_line_expanded(&self, idx: usize) -> bool {
        self.lines.get(idx).is_some_and(ChunkLineRef::is_expanded)
    }

    /// Returns the original `(chunk_index, line_index)` for the line at `idx`, if not expanded.
    #[must_use]
    pub fn line_source(&self, idx: usize) -> Option<(usize, usize)> {
        self.lines.get(idx).and_then(ChunkLineRef::source)
    }
}

/// Map of file paths to their cached [`FileExpandedState`].
pub type ExpandedLinesMap = HashMap<String, FileExpandedState>;

/// Splits text fetched from `/api/blob` into individual source lines without a trailing empty line.
#[must_use]
pub fn split_blob_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|line| String::from(line.strip_suffix('\r').unwrap_or(line)))
        .collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

/// Records an incremental line expansion in `state` for `file_path`.
pub fn record_line_expansion(
    map: &mut ExpandedLinesMap,
    file_path: &str,
    chunk_index: usize,
    direction: ExpandDirection,
    count: u32,
) {
    let entry = map.entry(String::from(file_path)).or_default();
    if let Some(existing) = entry
        .expanded_ranges
        .iter_mut()
        .find(|r| r.chunk_index == chunk_index && r.direction == direction)
    {
        existing.count = existing.count.saturating_add(count);
    } else {
        entry.expanded_ranges.push(ExpandedRange {
            chunk_index,
            direction,
            count,
        });
    }
}

/// Consolidates the remaining `hidden_lines` gap immediately before `chunk_index`.
pub fn record_expand_all_before(
    map: &mut ExpandedLinesMap,
    file_path: &str,
    chunk_index: usize,
    hidden_lines: u32,
) {
    let entry = map.entry(String::from(file_path)).or_default();
    let up_count = expanded_count_in(entry, chunk_index, ExpandDirection::Up);
    let prev_down_count = if chunk_index > 0 {
        expanded_count_in(entry, chunk_index - 1, ExpandDirection::Down)
    } else {
        0
    };
    let total = up_count
        .saturating_add(prev_down_count)
        .saturating_add(hidden_lines);

    if chunk_index > 0 {
        entry.expanded_ranges.retain(|r| {
            !(r.chunk_index == chunk_index - 1 && r.direction == ExpandDirection::Down)
        });
    }

    if let Some(existing) = entry
        .expanded_ranges
        .iter_mut()
        .find(|r| r.chunk_index == chunk_index && r.direction == ExpandDirection::Up)
    {
        existing.count = total;
    } else {
        entry.expanded_ranges.push(ExpandedRange {
            chunk_index,
            direction: ExpandDirection::Up,
            count: total,
        });
    }
}

struct ExpandedChunkLines {
    lines: Vec<ChunkLineRef>,
    old_start: u32,
    new_start: u32,
}

/// Computes merged, expansion-aware diff chunks for `file` without cloning original diff lines.
#[must_use]
pub fn merge_chunks_for_file(
    file: &Arc<DiffFile>,
    state: Option<&FileExpandedState>,
) -> Vec<MergedChunk> {
    let mut merged: Vec<MergedChunk> = Vec::with_capacity(file.chunks.len());

    for (idx, chunk) in file.chunks.iter().enumerate() {
        let ExpandedChunkLines {
            lines,
            old_start: start_old,
            new_start: start_new,
        } = expand_single_chunk(file, chunk, idx, state);
        let hidden_before = hidden_lines_before(file, chunk, idx, state);
        let hidden_after = hidden_lines_after(file, chunk, idx, state);

        if let Some(last) = merged.last_mut()
            && hidden_before == 0
        {
            last.lines.extend(lines);
            last.original_indices.push(idx);
            last.hidden_lines_after = hidden_after;
            update_merged_spans(last);
        } else {
            let mut item = MergedChunk {
                file: Arc::clone(file),
                old_start: start_old,
                old_lines: chunk.old_lines,
                new_start: start_new,
                new_lines: chunk.new_lines,
                header: chunk.header.clone(),
                lines,
                original_indices: alloc::vec![idx],
                hidden_lines_before: hidden_before,
                hidden_lines_after: hidden_after,
            };
            update_merged_spans(&mut item);
            merged.push(item);
        }
    }

    merged
}

fn expanded_count_in(
    state: &FileExpandedState,
    chunk_index: usize,
    direction: ExpandDirection,
) -> u32 {
    state
        .expanded_ranges
        .iter()
        .find(|r| r.chunk_index == chunk_index && r.direction == direction)
        .map_or(0, |r| r.count)
}

fn supports_expansion(file: &DiffFile) -> bool {
    file.path != "Commit Message"
        && file.status != FileStatus::Added
        && file.status != FileStatus::Deleted
}

fn use_new_anchor(file: &DiffFile) -> bool {
    file.status != FileStatus::Deleted
}

fn hidden_lines_before(
    file: &DiffFile,
    chunk: &DiffChunk,
    chunk_index: usize,
    state: Option<&FileExpandedState>,
) -> u32 {
    if !supports_expansion(file) {
        return 0;
    }

    let raw_hidden = if let Some(prev) = chunk_index.checked_sub(1).and_then(|p| file.chunks.get(p))
    {
        if use_new_anchor(file) {
            chunk
                .new_start
                .saturating_sub(prev.new_start.saturating_add(prev.new_lines))
        } else {
            chunk
                .old_start
                .saturating_sub(prev.old_start.saturating_add(prev.old_lines))
        }
    } else if use_new_anchor(file) {
        chunk.new_start.saturating_sub(1)
    } else {
        chunk.old_start.saturating_sub(1)
    };

    let Some(st) = state else {
        return raw_hidden;
    };
    let expanded_up = expanded_count_in(st, chunk_index, ExpandDirection::Up);
    let expanded_prev_down = if chunk_index > 0 {
        expanded_count_in(st, chunk_index - 1, ExpandDirection::Down)
    } else {
        0
    };

    raw_hidden.saturating_sub(expanded_up.saturating_add(expanded_prev_down))
}

fn hidden_lines_after(
    file: &DiffFile,
    chunk: &DiffChunk,
    chunk_index: usize,
    state: Option<&FileExpandedState>,
) -> Option<u32> {
    if !supports_expansion(file) {
        return Some(0);
    }
    if chunk_index + 1 < file.chunks.len() {
        return Some(0);
    }

    let st = state?;
    let total = if use_new_anchor(file) {
        st.new_total_lines.or(st.old_total_lines)?
    } else {
        st.old_total_lines.or(st.new_total_lines)?
    };
    let chunk_end = if use_new_anchor(file) {
        chunk
            .new_start
            .saturating_add(chunk.new_lines)
            .saturating_sub(1)
    } else {
        chunk
            .old_start
            .saturating_add(chunk.old_lines)
            .saturating_sub(1)
    };
    let expanded_down = expanded_count_in(st, chunk_index, ExpandDirection::Down);
    Some(
        total
            .saturating_sub(chunk_end)
            .saturating_sub(expanded_down),
    )
}

fn expand_single_chunk(
    file: &DiffFile,
    chunk: &DiffChunk,
    chunk_index: usize,
    state: Option<&FileExpandedState>,
) -> ExpandedChunkLines {
    let orig_refs = |count: usize| -> Vec<ChunkLineRef> {
        (0..count)
            .map(|li| ChunkLineRef::Original(chunk_index, li))
            .collect()
    };

    let Some(st) = state else {
        return ExpandedChunkLines {
            lines: orig_refs(chunk.lines.len()),
            old_start: chunk.old_start,
            new_start: chunk.new_start,
        };
    };

    let expanded_up = expanded_count_in(st, chunk_index, ExpandDirection::Up);
    let expanded_down = expanded_count_in(st, chunk_index, ExpandDirection::Down);
    if expanded_up == 0 && expanded_down == 0 {
        return ExpandedChunkLines {
            lines: orig_refs(chunk.lines.len()),
            old_start: chunk.old_start,
            new_start: chunk.new_start,
        };
    }

    let old_content = st.old_content.as_deref().unwrap_or(&[]);
    let new_content = st.new_content.as_deref().unwrap_or(&[]);

    let mut out_lines =
        Vec::with_capacity(chunk.lines.len() + expanded_up as usize + expanded_down as usize);

    if expanded_up > 0 {
        let prev = chunk_index.checked_sub(1).and_then(|p| file.chunks.get(p));
        // Skip gap lines already appended below the preceding hunk.
        let prev_down = if chunk_index > 0 {
            expanded_count_in(st, chunk_index - 1, ExpandDirection::Down)
        } else {
            0
        };
        let end_new = chunk.new_start.saturating_sub(1);
        let end_old = chunk.old_start.saturating_sub(1);
        let min_new = prev
            .map_or(0, |p| {
                p.new_start.saturating_add(p.new_lines).saturating_sub(1)
            })
            .saturating_add(prev_down)
            .min(end_new);
        let min_old = prev
            .map_or(0, |p| {
                p.old_start.saturating_add(p.old_lines).saturating_sub(1)
            })
            .saturating_add(prev_down)
            .min(end_old);

        let avail = if use_new_anchor(file) {
            end_new.saturating_sub(min_new)
        } else {
            end_old.saturating_sub(min_old)
        };
        let to_show = expanded_up.min(avail);
        let start_new_zero = end_new.saturating_sub(to_show);
        let start_old_zero = end_old.saturating_sub(to_show);

        for offset in 0..to_show {
            let old_num = start_old_zero + offset + 1;
            let new_num = start_new_zero + offset + 1;
            let content = pick_context_line(file, old_content, new_content, old_num, new_num);
            out_lines.push(ChunkLineRef::Expanded(Arc::new(DiffLine {
                kind: DiffLineKind::Context,
                content,
                old_line_number: Some(old_num),
                new_line_number: Some(new_num),
                syntax_spans: Vec::new(),
                word_diff: Vec::new(),
            })));
        }
    }

    for li in 0..chunk.lines.len() {
        out_lines.push(ChunkLineRef::Original(chunk_index, li));
    }

    if expanded_down > 0 {
        let chunk_end_old = chunk
            .old_start
            .saturating_add(chunk.old_lines)
            .saturating_sub(1);
        let chunk_end_new = chunk
            .new_start
            .saturating_add(chunk.new_lines)
            .saturating_sub(1);
        let next = file.chunks.get(chunk_index + 1);
        let max_new = next.map_or_else(
            || st.new_total_lines.unwrap_or(chunk_end_new),
            |n| n.new_start.saturating_sub(1),
        );
        let max_old = next.map_or_else(
            || st.old_total_lines.unwrap_or(chunk_end_old),
            |n| n.old_start.saturating_sub(1),
        );

        let avail = if use_new_anchor(file) {
            max_new.saturating_sub(chunk_end_new)
        } else {
            max_old.saturating_sub(chunk_end_old)
        };
        let to_show = expanded_down.min(avail);

        for offset in 0..to_show {
            let old_num = chunk_end_old + offset + 1;
            let new_num = chunk_end_new + offset + 1;
            let content = pick_context_line(file, old_content, new_content, old_num, new_num);
            out_lines.push(ChunkLineRef::Expanded(Arc::new(DiffLine {
                kind: DiffLineKind::Context,
                content,
                old_line_number: Some(old_num),
                new_line_number: Some(new_num),
                syntax_spans: Vec::new(),
                word_diff: Vec::new(),
            })));
        }
    }

    let first_line = out_lines.first().and_then(|r| match r {
        ChunkLineRef::Original(ci, li) => file.chunks.get(*ci)?.lines.get(*li),
        ChunkLineRef::Expanded(l) => Some(l.as_ref()),
    });
    let first_old = first_line
        .and_then(|l| l.old_line_number)
        .unwrap_or(chunk.old_start);
    let first_new = first_line
        .and_then(|l| l.new_line_number)
        .unwrap_or(chunk.new_start);

    ExpandedChunkLines {
        lines: out_lines,
        old_start: first_old,
        new_start: first_new,
    }
}

fn pick_context_line(
    file: &DiffFile,
    old_content: &[String],
    new_content: &[String],
    old_num: u32,
    new_num: u32,
) -> String {
    let from_new = usize::try_from(new_num)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|idx| new_content.get(idx))
        .cloned();
    let from_old = usize::try_from(old_num)
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|idx| old_content.get(idx))
        .cloned();

    if use_new_anchor(file) {
        from_new.or(from_old).unwrap_or_default()
    } else {
        from_old.or(from_new).unwrap_or_default()
    }
}

fn update_merged_spans(chunk: &mut MergedChunk) {
    let first_old = chunk.iter_lines().find_map(|l| l.old_line_number);
    let first_new = chunk.iter_lines().find_map(|l| l.new_line_number);
    let last_old = chunk
        .lines
        .iter()
        .rev()
        .filter_map(|r| chunk.resolve_line(r))
        .find_map(|l| l.old_line_number);
    let last_new = chunk
        .lines
        .iter()
        .rev()
        .filter_map(|r| chunk.resolve_line(r))
        .find_map(|l| l.new_line_number);

    if let Some(first_old) = first_old {
        chunk.old_start = first_old;
    }
    if let Some(first_new) = first_new {
        chunk.new_start = first_new;
    }
    if let Some(last_old) = last_old {
        chunk.old_lines = last_old.saturating_sub(chunk.old_start).saturating_add(1);
    }
    if let Some(last_new) = last_new {
        chunk.new_lines = last_new.saturating_sub(chunk.new_start).saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_line(
        kind: DiffLineKind,
        old_n: Option<u32>,
        new_n: Option<u32>,
        text: &str,
    ) -> DiffLine {
        DiffLine {
            kind,
            content: String::from(text),
            old_line_number: old_n,
            new_line_number: new_n,
            syntax_spans: Vec::new(),
            word_diff: Vec::new(),
        }
    }

    #[test]
    fn expands_and_merges_two_chunks_when_gap_closes() {
        let file = Arc::new(DiffFile {
            path: String::from("src/lib.rs"),
            old_path: None,
            status: FileStatus::Modified,
            additions: 1,
            deletions: 0,
            is_generated: false,
            chunks: alloc::vec![
                DiffChunk {
                    old_start: 2,
                    old_lines: 1,
                    new_start: 2,
                    new_lines: 1,
                    header: String::from("@@ -2,1 +2,1 @@"),
                    lines: alloc::vec![sample_line(
                        DiffLineKind::Context,
                        Some(2),
                        Some(2),
                        "line 2"
                    )],
                },
                DiffChunk {
                    old_start: 6,
                    old_lines: 1,
                    new_start: 6,
                    new_lines: 1,
                    header: String::from("@@ -6,1 +6,1 @@"),
                    lines: alloc::vec![sample_line(DiffLineKind::Add, None, Some(6), "line 6")],
                },
            ],
            chunks_loaded: true,
        });

        let mut map = ExpandedLinesMap::new();
        map.insert(
            String::from("src/lib.rs"),
            FileExpandedState {
                old_content: Some((1..=8).map(|n| alloc::format!("line {n}")).collect()),
                new_content: Some((1..=8).map(|n| alloc::format!("line {n}")).collect()),
                old_total_lines: Some(8),
                new_total_lines: Some(8),
                expanded_ranges: Vec::new(),
            },
        );

        let before = merge_chunks_for_file(&file, map.get("src/lib.rs"));
        assert_eq!(before.len(), 2);
        assert_eq!(before[0].hidden_lines_before, 1);
        assert_eq!(before[1].hidden_lines_before, 3);
        assert_eq!(before[1].hidden_lines_after, Some(2));

        record_expand_all_before(&mut map, "src/lib.rs", 1, before[1].hidden_lines_before);
        let after = merge_chunks_for_file(&file, map.get("src/lib.rs"));
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].lines.len(), 5);
        assert_eq!(after[0].line(1).unwrap().content, "line 3");
        assert_eq!(after[0].line(3).unwrap().content, "line 5");
        let expanded_flags: Vec<bool> = (0..after[0].lines.len())
            .map(|i| after[0].is_line_expanded(i))
            .collect();
        assert_eq!(expanded_flags, vec![false, true, true, true, false]);
    }

    #[test]
    fn dual_direction_expansion_merges_without_duplicate_lines() {
        let file = Arc::new(DiffFile {
            path: String::from("src/lib.rs"),
            old_path: None,
            status: FileStatus::Modified,
            additions: 1,
            deletions: 0,
            is_generated: false,
            chunks: alloc::vec![
                DiffChunk {
                    old_start: 2,
                    old_lines: 1,
                    new_start: 2,
                    new_lines: 1,
                    header: String::from("@@ -2,1 +2,1 @@"),
                    lines: alloc::vec![sample_line(
                        DiffLineKind::Context,
                        Some(2),
                        Some(2),
                        "line 2"
                    )],
                },
                DiffChunk {
                    old_start: 28,
                    old_lines: 1,
                    new_start: 28,
                    new_lines: 1,
                    header: String::from("@@ -28,1 +28,1 @@"),
                    lines: alloc::vec![sample_line(DiffLineKind::Add, None, Some(28), "line 28")],
                },
            ],
            chunks_loaded: true,
        });

        let mut map = ExpandedLinesMap::new();
        map.insert(
            String::from("src/lib.rs"),
            FileExpandedState {
                old_content: Some((1..=30).map(|n| alloc::format!("line {n}")).collect()),
                new_content: Some((1..=30).map(|n| alloc::format!("line {n}")).collect()),
                old_total_lines: Some(30),
                new_total_lines: Some(30),
                expanded_ranges: Vec::new(),
            },
        );

        record_line_expansion(&mut map, "src/lib.rs", 0, ExpandDirection::Down, 20);
        record_line_expansion(&mut map, "src/lib.rs", 1, ExpandDirection::Up, 20);

        let merged = merge_chunks_for_file(&file, map.get("src/lib.rs"));
        assert_eq!(merged.len(), 1);
        let contents: Vec<&str> = merged[0].iter_lines().map(|l| l.content.as_str()).collect();
        let expected: Vec<String> = (2..=28).map(|n| alloc::format!("line {n}")).collect();
        assert_eq!(contents, expected);
    }
}
