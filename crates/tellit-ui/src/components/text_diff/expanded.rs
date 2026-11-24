//! Hidden context line expansion, auto-expanded comment context, and hunk merging state.

extern crate alloc;

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use std::collections::{HashMap, HashSet};

use tellit_types::{
    CommentSide, DiffChunk, DiffFile, DiffLine, DiffLineKind, FileStatus, HighlightedLine,
    HighlightedLinesResponse, JjCommentThread,
};

/// Default number of context lines revealed per manual expansion click.
pub const DEFAULT_EXPAND_COUNT: u32 = 20;

/// Lines of context revealed above and below an unresolved comment outside visible diff hunks.
pub const COMMENT_AUTO_EXPAND_CONTEXT: u32 = 5;

/// Half-open 0-based line offset range `[start_offset, end_offset)` inside one unchanged gap.
///
/// `gap_index` addresses the unchanged region immediately before `file.chunks[gap_index]`, or the
/// trailing region after the final hunk when `gap_index == file.chunks.len()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GapSlice {
    /// Gap index in `0..=file.chunks.len()`.
    pub gap_index: usize,
    /// Inclusive 0-based start offset inside the gap.
    pub start_offset: u32,
    /// Exclusive 0-based end offset inside the gap (`end_offset > start_offset`).
    pub end_offset: u32,
}

/// Unexpanded slice of an unchanged gap immediately before or after a [`MergedChunk`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HiddenGap {
    /// Gap index in `0..=file.chunks.len()`.
    pub gap_index: usize,
    /// Inclusive 0-based start offset of this hidden sub-range inside `gap_index`.
    pub start_offset: u32,
    /// Exclusive 0-based end offset of this hidden sub-range inside `gap_index`.
    pub end_offset: u32,
    /// 1-based base-file line number corresponding to `start_offset`.
    pub old_start_line: u32,
    /// 1-based target-file line number corresponding to `start_offset`.
    pub new_start_line: u32,
}

/// Concrete expansion request derived from a [`HiddenGap`] click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GapSliceRequest {
    /// Gap slice to mark revealed in [`FileExpandedState`].
    pub slice: GapSlice,
    /// Inclusive 1-based target-file line range needed to render `slice`.
    pub new_line_range: (u32, u32),
    /// Inclusive 1-based base-file line range needed as fallback.
    pub old_line_range: (u32, u32),
}

impl HiddenGap {
    /// Returns the number of hidden lines covered by this gap.
    #[must_use]
    pub const fn line_count(&self) -> u32 {
        self.end_offset.saturating_sub(self.start_offset)
    }

    /// Builds a request revealing up to `count` lines from the top of this gap (growing downward).
    #[must_use]
    pub const fn take_from_start(&self, count: u32) -> GapSliceRequest {
        let len = self.line_count();
        let take = if count < len { count } else { len };
        let span = take.saturating_sub(1);
        GapSliceRequest {
            slice: GapSlice {
                gap_index: self.gap_index,
                start_offset: self.start_offset,
                end_offset: self.start_offset.saturating_add(take),
            },
            new_line_range: (
                self.new_start_line,
                self.new_start_line.saturating_add(span),
            ),
            old_line_range: (
                self.old_start_line,
                self.old_start_line.saturating_add(span),
            ),
        }
    }

    /// Builds a request revealing up to `count` lines from the bottom of this gap (growing upward).
    #[must_use]
    pub const fn take_from_end(&self, count: u32) -> GapSliceRequest {
        let len = self.line_count();
        let take = if count < len { count } else { len };
        let rel_start = len.saturating_sub(take);
        let span = take.saturating_sub(1);
        let new_start = self.new_start_line.saturating_add(rel_start);
        let old_start = self.old_start_line.saturating_add(rel_start);
        GapSliceRequest {
            slice: GapSlice {
                gap_index: self.gap_index,
                start_offset: self.start_offset.saturating_add(rel_start),
                end_offset: self.end_offset,
            },
            new_line_range: (new_start, new_start.saturating_add(span)),
            old_line_range: (old_start, old_start.saturating_add(span)),
        }
    }

    /// Builds a request revealing every line in this hidden gap.
    #[must_use]
    pub const fn as_full_slice(&self) -> GapSliceRequest {
        self.take_from_start(self.line_count())
    }
}

/// Cached syntax-highlighted source lines for one revision of a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CachedLines {
    /// Highlighted lines keyed by 1-based line number.
    pub by_line: HashMap<u32, HighlightedLine>,
    /// `true` once every line `1..=total_lines` has been fetched and stored.
    pub complete: bool,
}

/// Reference-counted wrapper over [`CachedLines`] with O(1) pointer fast-path equality.
#[derive(Debug, Clone, Default)]
pub struct SharedCachedLines(Arc<CachedLines>);

impl PartialEq for SharedCachedLines {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}

impl Eq for SharedCachedLines {}

impl SharedCachedLines {
    /// Looks up the cached highlighted line at 1-based line number `line_num`.
    #[must_use]
    pub fn get(&self, line_num: u32) -> Option<&HighlightedLine> {
        self.0.by_line.get(&line_num)
    }

    /// Returns `true` when the full file has been cached.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.0.complete
    }

    /// Returns `true` when every line in inclusive `start..=end` (clamped to `total_lines` if
    /// known) is already cached.
    #[must_use]
    pub fn has_range(&self, start: u32, end: u32, total_lines: Option<u32>) -> bool {
        if self.0.complete {
            return true;
        }
        let clamped_start = start.max(1);
        let clamped_end = match total_lines {
            Some(total) => end.min(total),
            None => end,
        };
        if clamped_start > clamped_end {
            return total_lines.is_some();
        }
        (clamped_start..=clamped_end).all(|ln| self.0.by_line.contains_key(&ln))
    }

    /// Inserts highlighted lines returned by `GET /api/lines/{*path}` into the cache.
    pub fn insert_response(&mut self, resp: HighlightedLinesResponse, full_file_request: bool) {
        let inner = Arc::make_mut(&mut self.0);
        let returned_len = u32::try_from(resp.lines.len()).unwrap_or(u32::MAX);
        for (idx, line) in resp.lines.into_iter().enumerate() {
            let line_num = resp
                .start_line
                .saturating_add(u32::try_from(idx).unwrap_or(u32::MAX));
            inner.by_line.insert(line_num, line);
        }
        if full_file_request || (resp.start_line <= 1 && returned_len >= resp.total_lines) {
            inner.complete = true;
        }
    }

    #[cfg(test)]
    pub(crate) fn from_full_lines(lines: Vec<HighlightedLine>) -> Self {
        let mut by_line = HashMap::with_capacity(lines.len());
        for (idx, line) in lines.into_iter().enumerate() {
            let line_num = u32::try_from(idx + 1).unwrap_or(u32::MAX);
            by_line.insert(line_num, line);
        }
        Self(Arc::new(CachedLines {
            by_line,
            complete: true,
        }))
    }
}

/// Cached highlighted file lines, line counts, and active manual/auto expansions for one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FileExpandedState {
    /// Cached highlighted lines of the base revision (`old`) file.
    pub old_lines: SharedCachedLines,
    /// Cached highlighted lines of the target revision (`new`) file.
    pub new_lines: SharedCachedLines,
    /// Total lines in the base revision file.
    pub old_total_lines: Option<u32>,
    /// Total lines in the target revision file.
    pub new_total_lines: Option<u32>,
    /// Manually revealed gap slices from `ExpandBar` clicks.
    pub manual_slices: Vec<GapSlice>,
    /// Automatically revealed gap slices around unresolved inline comments.
    pub auto_slices: Vec<GapSlice>,
    /// Unresolved thread keys `(side, start, end)` already attempted for auto-expansion in this diff load.
    pub attempted_auto_expands: HashSet<(CommentSide, u32, u32)>,
    /// `true` when the user clicked "Expand file" to reveal every hidden context gap.
    pub expand_entire_file: bool,
}

impl FileExpandedState {
    /// Returns `true` when any manual expansion ("Expand file" or `ExpandBar` click) is active.
    #[must_use]
    pub fn has_manual_expansion(&self) -> bool {
        self.expand_entire_file || !self.manual_slices.is_empty()
    }

    /// Returns `true` when full highlighted lines are cached for the primary expansion side.
    #[must_use]
    pub fn has_complete_lines(&self, status: FileStatus) -> bool {
        if status == FileStatus::Deleted {
            self.old_lines.is_complete()
        } else {
            self.new_lines.is_complete() || self.old_lines.is_complete()
        }
    }

    /// Returns `true` when every line required for the given range is already cached.
    #[must_use]
    pub fn has_line_range(
        &self,
        status: FileStatus,
        new_range: (u32, u32),
        old_range: (u32, u32),
    ) -> bool {
        if status == FileStatus::Deleted {
            self.old_lines
                .has_range(old_range.0, old_range.1, self.old_total_lines)
        } else {
            self.new_lines
                .has_range(new_range.0, new_range.1, self.new_total_lines)
                || self
                    .old_lines
                    .has_range(old_range.0, old_range.1, self.old_total_lines)
        }
    }
}

/// Reference to one line inside a [`MergedChunk`], avoiding cloning original [`DiffLine`] entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkLineRef {
    /// Original line indexed by `(chunk_index, line_index)` inside `DiffFile::chunks`.
    Original(usize, usize),
    /// Context line materialized on demand from `GET /api/lines`.
    Expanded(Arc<DiffLine>),
}

impl ChunkLineRef {
    /// Returns `true` when this line came from context expansion.
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
    /// Original or synthetic hunk header string.
    pub header: String,
    /// Ordered references to diff and expanded context lines.
    pub lines: Vec<ChunkLineRef>,
    /// Sorted original indices in [`DiffFile::chunks`] merged into this block.
    pub original_indices: Vec<usize>,
    /// Hidden lines still collapsed immediately above this block.
    pub hidden_lines_before: u32,
    /// Concrete hidden gap immediately above this block, if `hidden_lines_before > 0`.
    pub before_gap: Option<HiddenGap>,
    /// Hidden lines still collapsed immediately below the last block (`None` when total file length is unknown).
    pub hidden_lines_after: Option<u32>,
    /// Concrete hidden gap immediately below the last block, if known and non-empty.
    pub after_gap: Option<HiddenGap>,
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

/// Batched auto-expansion plan for unresolved inline threads outside visible diff hunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoExpandPlan {
    /// Coalesced gap slices to record in [`FileExpandedState::auto_slices`].
    pub slices: Vec<GapSlice>,
    /// Bounding inclusive 1-based target-file line range covering all `slices`.
    pub new_line_range: (u32, u32),
    /// Bounding inclusive 1-based base-file line range covering all `slices`.
    pub old_line_range: (u32, u32),
    /// Thread attempt keys `(side, start, end)` represented by this plan.
    pub attempt_keys: Vec<(CommentSide, u32, u32)>,
}

/// Returns `true` when `file` can have hidden unchanged context outside its diff hunks.
#[must_use]
pub fn supports_expansion(file: &DiffFile) -> bool {
    file.path != "Commit Message"
        && file.status != FileStatus::Added
        && file.status != FileStatus::Deleted
}

/// Returns `true` when `file` still has unexpanded hidden context lines.
#[must_use]
pub fn has_hidden_context(
    file: &DiffFile,
    merged_chunks: &[MergedChunk],
    state: Option<&FileExpandedState>,
) -> bool {
    if !supports_expansion(file) {
        return false;
    }
    if merged_chunks
        .iter()
        .any(|mc| mc.hidden_lines_before > 0 || mc.hidden_lines_after.is_some_and(|n| n > 0))
    {
        return true;
    }
    if file.chunks.is_empty() && merged_chunks.is_empty() {
        return state
            .and_then(|st| st.new_total_lines.or(st.old_total_lines))
            .is_some_and(|total| total > 0);
    }
    false
}

/// Records a manual gap slice in `map` for `file_path`, coalescing overlapping slices.
pub fn record_manual_slice(map: &mut ExpandedLinesMap, file_path: &str, slice: GapSlice) {
    if slice.end_offset <= slice.start_offset {
        return;
    }
    let entry = map.entry(String::from(file_path)).or_default();
    entry.manual_slices.push(slice);
    coalesce_gap_slices(&mut entry.manual_slices);
}

/// Records attempted auto-expand thread keys in `map` for `file_path` to prevent duplicate fetches.
pub fn record_auto_expand_attempts(
    map: &mut ExpandedLinesMap,
    file_path: &str,
    keys: &[(CommentSide, u32, u32)],
) {
    if keys.is_empty() {
        return;
    }
    let entry = map.entry(String::from(file_path)).or_default();
    for &key in keys {
        entry.attempted_auto_expands.insert(key);
    }
}

/// Records auto-expanded comment context slices in `map` for `file_path`.
pub fn record_auto_slices(map: &mut ExpandedLinesMap, file_path: &str, slices: &[GapSlice]) {
    let entry = map.entry(String::from(file_path)).or_default();
    for &slice in slices {
        if slice.end_offset > slice.start_offset {
            entry.auto_slices.push(slice);
        }
    }
    coalesce_gap_slices(&mut entry.auto_slices);
}

/// Marks every hidden context gap in `file_path` as expanded.
pub fn record_expand_entire_file(map: &mut ExpandedLinesMap, file_path: &str) {
    let entry = map.entry(String::from(file_path)).or_default();
    entry.expand_entire_file = true;
}

/// Collapses all manual context expansions for `file_path`, preserving cached lines and
/// auto-expanded unresolved comment context.
pub fn collapse_manual_expansions(map: &mut ExpandedLinesMap, file_path: &str) {
    if let Some(entry) = map.get_mut(file_path) {
        entry.expand_entire_file = false;
        entry.manual_slices.clear();
    }
}

/// Computes an [`AutoExpandPlan`] for any unresolved inline comment on `file` whose line falls
/// outside `merged_chunks` and has not yet been attempted in `state`.
#[must_use]
pub fn compute_auto_expand_plan(
    file: &DiffFile,
    merged_chunks: &[MergedChunk],
    state: Option<&FileExpandedState>,
    threads: &[JjCommentThread],
) -> Option<AutoExpandPlan> {
    if !file.chunks_loaded || !supports_expansion(file) {
        return None;
    }

    let covered = collect_merged_line_keys(merged_chunks);
    let gaps = compute_file_gaps(file, state);
    if gaps.is_empty() {
        return None;
    }

    let mut slices: Vec<GapSlice> = Vec::new();
    let mut attempt_keys: Vec<(CommentSide, u32, u32)> = Vec::new();
    let mut min_new = u32::MAX;
    let mut max_new = 0u32;
    let mut min_old = u32::MAX;
    let mut max_old = 0u32;

    for thread in threads {
        if thread.resolved || thread.file_path != file.path {
            continue;
        }
        let Some(lr) = thread.line_range else {
            continue;
        };
        let key = (thread.side, lr.start, lr.end);
        let already_attempted = state.is_some_and(|st| st.attempted_auto_expands.contains(&key));
        if already_attempted || covered.contains(&(thread.side, lr.end)) {
            continue;
        }

        let want_start = lr.start.saturating_sub(COMMENT_AUTO_EXPAND_CONTEXT).max(1);
        let want_end = lr.end.saturating_add(COMMENT_AUTO_EXPAND_CONTEXT);
        let mut matched_any = false;

        for gap in &gaps {
            let gap_side_start = match thread.side {
                CommentSide::Old => gap.old_start,
                CommentSide::New => gap.new_start,
            };
            let gap_side_end = match gap.len {
                Some(0) => continue,
                Some(len) => gap_side_start.saturating_add(len).saturating_sub(1),
                None => {
                    if want_end < gap_side_start {
                        continue;
                    }
                    want_end.max(gap_side_start)
                }
            };

            // Only expand gaps that actually overlap the commented range itself.
            if lr.end < gap_side_start || lr.start > gap_side_end {
                continue;
            }

            let hit_start = want_start.max(gap_side_start);
            let hit_end = want_end.min(gap_side_end);
            if hit_start > hit_end {
                continue;
            }

            let start_offset = hit_start.saturating_sub(gap_side_start);
            let end_offset = hit_end.saturating_sub(gap_side_start).saturating_add(1);
            let span = end_offset.saturating_sub(start_offset).saturating_sub(1);

            let new_start = gap.new_start.saturating_add(start_offset);
            let new_end = new_start.saturating_add(span);
            let old_start = gap.old_start.saturating_add(start_offset);
            let old_end = old_start.saturating_add(span);

            min_new = min_new.min(new_start);
            max_new = max_new.max(new_end);
            min_old = min_old.min(old_start);
            max_old = max_old.max(old_end);

            slices.push(GapSlice {
                gap_index: gap.gap_index,
                start_offset,
                end_offset,
            });
            matched_any = true;
        }

        if matched_any {
            attempt_keys.push(key);
        }
    }

    if slices.is_empty() {
        return None;
    }

    coalesce_gap_slices(&mut slices);
    Some(AutoExpandPlan {
        slices,
        new_line_range: (min_new, max_new),
        old_line_range: (min_old, max_old),
        attempt_keys,
    })
}

/// Computes merged, expansion-aware diff chunks for `file` without cloning original diff lines.
#[must_use]
pub fn merge_chunks_for_file(
    file: &Arc<DiffFile>,
    state: Option<&FileExpandedState>,
) -> Vec<MergedChunk> {
    if !supports_expansion(file) {
        return file
            .chunks
            .iter()
            .enumerate()
            .map(|(idx, chunk)| {
                let mut item = MergedChunk {
                    file: Arc::clone(file),
                    old_start: chunk.old_start,
                    old_lines: chunk.old_lines,
                    new_start: chunk.new_start,
                    new_lines: chunk.new_lines,
                    header: chunk.header.clone(),
                    lines: (0..chunk.lines.len())
                        .map(|li| ChunkLineRef::Original(idx, li))
                        .collect(),
                    original_indices: alloc::vec![idx],
                    hidden_lines_before: 0,
                    before_gap: None,
                    hidden_lines_after: Some(0),
                    after_gap: None,
                };
                update_merged_spans(&mut item);
                item
            })
            .collect();
    }

    let gaps = compute_file_gaps(file, state);
    let mut segments: Vec<Segment> = Vec::with_capacity(file.chunks.len() * 3 + 2);

    for (gap_idx, &gap) in gaps.iter().enumerate() {
        push_gap_segments(&mut segments, gap, state);
        if gap_idx < file.chunks.len() {
            segments.push(Segment::Hunk(gap_idx));
        }
    }

    assemble_merged_chunks(file, state, &segments)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileGap {
    gap_index: usize,
    old_start: u32,
    new_start: u32,
    len: Option<u32>,
}

enum Segment {
    Hunk(usize),
    Context {
        gap: FileGap,
        start_offset: u32,
        end_offset: u32,
    },
    Hidden(HiddenGap),
    UnknownTrailing,
}

fn use_new_anchor(file: &DiffFile) -> bool {
    file.status != FileStatus::Deleted
}

const fn chunk_old_end(chunk: &DiffChunk) -> u32 {
    if chunk.old_lines == 0 {
        chunk.old_start
    } else {
        chunk
            .old_start
            .saturating_add(chunk.old_lines)
            .saturating_sub(1)
    }
}

const fn chunk_new_end(chunk: &DiffChunk) -> u32 {
    if chunk.new_lines == 0 {
        chunk.new_start
    } else {
        chunk
            .new_start
            .saturating_add(chunk.new_lines)
            .saturating_sub(1)
    }
}

const fn chunk_old_before(chunk: &DiffChunk) -> u32 {
    if chunk.old_lines == 0 {
        chunk.old_start
    } else {
        chunk.old_start.saturating_sub(1)
    }
}

const fn chunk_new_before(chunk: &DiffChunk) -> u32 {
    if chunk.new_lines == 0 {
        chunk.new_start
    } else {
        chunk.new_start.saturating_sub(1)
    }
}

fn compute_file_gaps(file: &DiffFile, state: Option<&FileExpandedState>) -> Vec<FileGap> {
    if !supports_expansion(file) {
        return Vec::new();
    }

    let k = file.chunks.len();
    if k == 0 {
        let total = state.and_then(|st| {
            if use_new_anchor(file) {
                st.new_total_lines.or(st.old_total_lines)
            } else {
                st.old_total_lines.or(st.new_total_lines)
            }
        });
        return alloc::vec![FileGap {
            gap_index: 0,
            old_start: 1,
            new_start: 1,
            len: total,
        }];
    }

    let mut gaps = Vec::with_capacity(k + 1);
    let first = &file.chunks[0];
    let first_len = if use_new_anchor(file) {
        chunk_new_before(first)
    } else {
        chunk_old_before(first)
    };
    gaps.push(FileGap {
        gap_index: 0,
        old_start: 1,
        new_start: 1,
        len: Some(first_len),
    });

    for idx in 1..k {
        let prev = &file.chunks[idx - 1];
        let curr = &file.chunks[idx];
        let prev_old_end = chunk_old_end(prev);
        let prev_new_end = chunk_new_end(prev);
        let len = if use_new_anchor(file) {
            chunk_new_before(curr).saturating_sub(prev_new_end)
        } else {
            chunk_old_before(curr).saturating_sub(prev_old_end)
        };
        gaps.push(FileGap {
            gap_index: idx,
            old_start: prev_old_end.saturating_add(1),
            new_start: prev_new_end.saturating_add(1),
            len: Some(len),
        });
    }

    let last = &file.chunks[k - 1];
    let last_old_end = chunk_old_end(last);
    let last_new_end = chunk_new_end(last);
    let trailing_len = state.and_then(|st| {
        if use_new_anchor(file) {
            st.new_total_lines
                .map(|t| t.saturating_sub(last_new_end))
                .or_else(|| st.old_total_lines.map(|t| t.saturating_sub(last_old_end)))
        } else {
            st.old_total_lines
                .map(|t| t.saturating_sub(last_old_end))
                .or_else(|| st.new_total_lines.map(|t| t.saturating_sub(last_new_end)))
        }
    });
    gaps.push(FileGap {
        gap_index: k,
        old_start: last_old_end.saturating_add(1),
        new_start: last_new_end.saturating_add(1),
        len: trailing_len,
    });

    gaps
}

fn push_gap_segments(segments: &mut Vec<Segment>, gap: FileGap, state: Option<&FileExpandedState>) {
    let Some(len) = gap.len else {
        segments.push(Segment::UnknownTrailing);
        return;
    };
    if len == 0 {
        return;
    }

    let intervals = revealed_intervals_for_gap(gap.gap_index, len, state);
    let mut cursor = 0u32;
    for (start, end) in intervals {
        if start > cursor {
            segments.push(Segment::Hidden(make_hidden_gap(gap, cursor, start)));
        }
        segments.push(Segment::Context {
            gap,
            start_offset: start,
            end_offset: end,
        });
        cursor = end;
    }
    if cursor < len {
        segments.push(Segment::Hidden(make_hidden_gap(gap, cursor, len)));
    }
}

fn revealed_intervals_for_gap(
    gap_index: usize,
    len: u32,
    state: Option<&FileExpandedState>,
) -> Vec<(u32, u32)> {
    let Some(st) = state else {
        return Vec::new();
    };
    if st.expand_entire_file {
        return alloc::vec![(0, len)];
    }

    let mut raw: Vec<(u32, u32)> = Vec::new();
    for slice in st.manual_slices.iter().chain(st.auto_slices.iter()) {
        if slice.gap_index != gap_index {
            continue;
        }
        let s = slice.start_offset.min(len);
        let e = slice.end_offset.min(len);
        if s < e {
            raw.push((s, e));
        }
    }
    if raw.is_empty() {
        return raw;
    }

    raw.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(raw.len());
    for (s, e) in raw {
        if let Some(last) = merged.last_mut()
            && s <= last.1
        {
            last.1 = last.1.max(e);
        } else {
            merged.push((s, e));
        }
    }
    merged
}

fn coalesce_gap_slices(slices: &mut Vec<GapSlice>) {
    slices.retain(|s| s.end_offset > s.start_offset);
    if slices.len() <= 1 {
        return;
    }
    slices.sort_unstable_by_key(|s| (s.gap_index, s.start_offset, s.end_offset));
    let mut out: Vec<GapSlice> = Vec::with_capacity(slices.len());
    for slice in slices.drain(..) {
        if let Some(last) = out.last_mut()
            && last.gap_index == slice.gap_index
            && slice.start_offset <= last.end_offset
        {
            last.end_offset = last.end_offset.max(slice.end_offset);
        } else {
            out.push(slice);
        }
    }
    *slices = out;
}

const fn make_hidden_gap(gap: FileGap, start_offset: u32, end_offset: u32) -> HiddenGap {
    HiddenGap {
        gap_index: gap.gap_index,
        start_offset,
        end_offset,
        old_start_line: gap.old_start.saturating_add(start_offset),
        new_start_line: gap.new_start.saturating_add(start_offset),
    }
}

fn assemble_merged_chunks(
    file: &Arc<DiffFile>,
    state: Option<&FileExpandedState>,
    segments: &[Segment],
) -> Vec<MergedChunk> {
    let mut merged: Vec<MergedChunk> = Vec::new();
    let mut pending_before: Option<HiddenGap> = None;
    let mut current: Option<MergedChunk> = None;
    let mut saw_unknown_trailing = false;

    for seg in segments {
        match *seg {
            Segment::Hidden(hg) => {
                if let Some(ch) = current.take() {
                    merged.push(ch);
                }
                pending_before = Some(hg);
            }
            Segment::UnknownTrailing => {
                saw_unknown_trailing = true;
                if let Some(ch) = current.take() {
                    merged.push(ch);
                }
            }
            Segment::Hunk(ci) => {
                let orig_chunk = &file.chunks[ci];
                let line_refs =
                    (0..orig_chunk.lines.len()).map(|li| ChunkLineRef::Original(ci, li));
                if let Some(ch) = current.as_mut() {
                    ch.lines.extend(line_refs);
                    ch.original_indices.push(ci);
                    if ch.original_indices.len() == 1 {
                        ch.header.clone_from(&orig_chunk.header);
                    }
                    update_merged_spans(ch);
                } else {
                    let before_gap = pending_before.take();
                    let mut ch = MergedChunk {
                        file: Arc::clone(file),
                        old_start: orig_chunk.old_start,
                        old_lines: orig_chunk.old_lines,
                        new_start: orig_chunk.new_start,
                        new_lines: orig_chunk.new_lines,
                        header: orig_chunk.header.clone(),
                        lines: line_refs.collect(),
                        original_indices: alloc::vec![ci],
                        hidden_lines_before: before_gap.as_ref().map_or(0, HiddenGap::line_count),
                        before_gap,
                        hidden_lines_after: Some(0),
                        after_gap: None,
                    };
                    update_merged_spans(&mut ch);
                    current = Some(ch);
                }
            }
            Segment::Context {
                gap,
                start_offset,
                end_offset,
            } => {
                let ctx_lines = build_context_lines(file, state, gap, start_offset, end_offset);
                if let Some(ch) = current.as_mut() {
                    ch.lines.extend(ctx_lines);
                    update_merged_spans(ch);
                } else {
                    let count = end_offset.saturating_sub(start_offset);
                    let old_start = gap.old_start.saturating_add(start_offset);
                    let new_start = gap.new_start.saturating_add(start_offset);
                    let before_gap = pending_before.take();
                    let mut ch = MergedChunk {
                        file: Arc::clone(file),
                        old_start,
                        old_lines: count,
                        new_start,
                        new_lines: count,
                        header: alloc::format!("@@ -{old_start},{count} +{new_start},{count} @@"),
                        lines: ctx_lines,
                        original_indices: Vec::new(),
                        hidden_lines_before: before_gap.as_ref().map_or(0, HiddenGap::line_count),
                        before_gap,
                        hidden_lines_after: Some(0),
                        after_gap: None,
                    };
                    update_merged_spans(&mut ch);
                    current = Some(ch);
                }
            }
        }
    }

    if let Some(ch) = current.take() {
        merged.push(ch);
    }

    if let Some(last) = merged.last_mut() {
        if saw_unknown_trailing {
            last.hidden_lines_after = None;
            last.after_gap = None;
        } else if let Some(trailing) = pending_before.take() {
            last.hidden_lines_after = Some(trailing.line_count());
            last.after_gap = Some(trailing);
        }
    }

    merged
}

fn build_context_lines(
    file: &DiffFile,
    state: Option<&FileExpandedState>,
    gap: FileGap,
    start_offset: u32,
    end_offset: u32,
) -> Vec<ChunkLineRef> {
    let count = usize::try_from(end_offset.saturating_sub(start_offset)).unwrap_or(0);
    let mut out = Vec::with_capacity(count);
    for offset in start_offset..end_offset {
        let old_num = gap.old_start.saturating_add(offset);
        let new_num = gap.new_start.saturating_add(offset);
        let (content, syntax_spans) = pick_context_line(file, state, old_num, new_num);
        out.push(ChunkLineRef::Expanded(Arc::new(DiffLine {
            kind: DiffLineKind::Context,
            content,
            old_line_number: Some(old_num),
            new_line_number: Some(new_num),
            syntax_spans,
            word_diff: Vec::new(),
        })));
    }
    out
}

fn pick_context_line(
    file: &DiffFile,
    state: Option<&FileExpandedState>,
    old_num: u32,
    new_num: u32,
) -> (String, Vec<tellit_types::SyntaxSpan>) {
    let Some(st) = state else {
        return (String::new(), Vec::new());
    };
    let picked = if use_new_anchor(file) {
        st.new_lines
            .get(new_num)
            .or_else(|| st.old_lines.get(old_num))
    } else {
        st.old_lines
            .get(old_num)
            .or_else(|| st.new_lines.get(new_num))
    };
    match picked {
        Some(hl) => (hl.content.clone(), hl.syntax_spans.clone()),
        None => (String::new(), Vec::new()),
    }
}

fn collect_merged_line_keys(merged_chunks: &[MergedChunk]) -> HashSet<(CommentSide, u32)> {
    let mut keys = HashSet::new();
    for mc in merged_chunks {
        for line in mc.iter_lines() {
            match line.kind {
                DiffLineKind::Delete => {
                    if let Some(old_n) = line.old_line_number {
                        keys.insert((CommentSide::Old, old_n));
                    }
                }
                DiffLineKind::Add => {
                    if let Some(new_n) = line.new_line_number {
                        keys.insert((CommentSide::New, new_n));
                    }
                }
                DiffLineKind::Context => {
                    if let Some(old_n) = line.old_line_number {
                        keys.insert((CommentSide::Old, old_n));
                    }
                    if let Some(new_n) = line.new_line_number {
                        keys.insert((CommentSide::New, new_n));
                    }
                }
            }
        }
    }
    keys
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
    use tellit_types::{LineRangeSpec, SyntaxSpan};

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

    fn full_state(count: u32) -> FileExpandedState {
        let lines: Vec<HighlightedLine> = (1..=count)
            .map(|n| HighlightedLine {
                content: alloc::format!("line {n}"),
                syntax_spans: alloc::vec![SyntaxSpan { len: 4, style: 14 }],
            })
            .collect();
        FileExpandedState {
            old_lines: SharedCachedLines::from_full_lines(lines.clone()),
            new_lines: SharedCachedLines::from_full_lines(lines),
            old_total_lines: Some(count),
            new_total_lines: Some(count),
            manual_slices: Vec::new(),
            auto_slices: Vec::new(),
            attempted_auto_expands: HashSet::new(),
            expand_entire_file: false,
        }
    }

    fn sample_thread(
        id: &str,
        file_path: &str,
        side: CommentSide,
        start: u32,
        end: u32,
        resolved: bool,
    ) -> JjCommentThread {
        JjCommentThread {
            id: String::from(id),
            commit_id: String::from("0123456789abcdef0123456789abcdef01234567"),
            change_id: String::from("klyvutpsqrvz"),
            file_path: String::from(file_path),
            side,
            line_range: Some(LineRangeSpec { start, end }),
            resolved,
            comments: Vec::new(),
        }
    }

    #[test]
    fn expands_and_merges_two_chunks_preserving_syntax_spans() {
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
        map.insert(String::from("src/lib.rs"), full_state(8));

        let before = merge_chunks_for_file(&file, map.get("src/lib.rs"));
        assert_eq!(before.len(), 2);
        assert_eq!(before[0].hidden_lines_before, 1);
        assert_eq!(before[1].hidden_lines_before, 3);
        assert_eq!(before[1].hidden_lines_after, Some(2));

        let req = before[1].before_gap.unwrap().as_full_slice();
        assert_eq!(req.new_line_range, (3, 5));
        record_manual_slice(&mut map, "src/lib.rs", req.slice);

        let after = merge_chunks_for_file(&file, map.get("src/lib.rs"));
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].lines.len(), 5);
        assert_eq!(after[0].line(1).unwrap().content, "line 3");
        assert_eq!(
            after[0].line(1).unwrap().syntax_spans,
            alloc::vec![SyntaxSpan { len: 4, style: 14 }]
        );
        assert_eq!(after[0].line(3).unwrap().content, "line 5");
        let expanded_flags: Vec<bool> = (0..after[0].lines.len())
            .map(|i| after[0].is_line_expanded(i))
            .collect();
        assert_eq!(expanded_flags, alloc::vec![false, true, true, true, false]);
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
        map.insert(String::from("src/lib.rs"), full_state(30));

        let initial = merge_chunks_for_file(&file, map.get("src/lib.rs"));
        let gap = initial[1].before_gap.unwrap();
        record_manual_slice(&mut map, "src/lib.rs", gap.take_from_start(20).slice);
        record_manual_slice(&mut map, "src/lib.rs", gap.take_from_end(20).slice);

        let merged = merge_chunks_for_file(&file, map.get("src/lib.rs"));
        assert_eq!(merged.len(), 1);
        let contents: Vec<&str> = merged[0].iter_lines().map(|l| l.content.as_str()).collect();
        let expected: Vec<String> = (2..=28).map(|n| alloc::format!("line {n}")).collect();
        assert_eq!(contents, expected);
    }

    #[test]
    fn auto_expand_places_unresolved_old_and_new_comments_with_line_drift() {
        // Hunk 0 adds 10 lines (old 10..=11 -> new 10..=21, so gap 1 has old_start=12, new_start=22).
        let mut hunk0_lines = alloc::vec![sample_line(
            DiffLineKind::Context,
            Some(10),
            Some(10),
            "line 10"
        )];
        for n in 11..=20 {
            hunk0_lines.push(sample_line(
                DiffLineKind::Add,
                None,
                Some(n),
                &alloc::format!("added {n}"),
            ));
        }
        hunk0_lines.push(sample_line(
            DiffLineKind::Context,
            Some(11),
            Some(21),
            "line 21",
        ));

        let file = Arc::new(DiffFile {
            path: String::from("src/drift.rs"),
            old_path: None,
            status: FileStatus::Modified,
            additions: 10,
            deletions: 0,
            is_generated: false,
            chunks: alloc::vec![
                DiffChunk {
                    old_start: 10,
                    old_lines: 2,
                    new_start: 10,
                    new_lines: 12,
                    header: String::from("@@ -10,2 +10,12 @@"),
                    lines: hunk0_lines,
                },
                DiffChunk {
                    old_start: 100,
                    old_lines: 2,
                    new_start: 110,
                    new_lines: 2,
                    header: String::from("@@ -100,2 +110,2 @@"),
                    lines: alloc::vec![
                        sample_line(DiffLineKind::Context, Some(100), Some(110), "line 110"),
                        sample_line(DiffLineKind::Context, Some(101), Some(111), "line 111"),
                    ],
                },
            ],
            chunks_loaded: true,
        });

        let mut map = ExpandedLinesMap::new();
        map.insert(String::from("src/drift.rs"), full_state(150));

        let threads = alloc::vec![
            // Unresolved Old-side comment at old line 50 (corresponds to new line 60 in gap 1).
            sample_thread("t-old", "src/drift.rs", CommentSide::Old, 50, 50, false),
            // Unresolved New-side comment at trailing new line 130 (old line 120 in gap 2).
            sample_thread("t-new", "src/drift.rs", CommentSide::New, 130, 132, false),
            // Resolved comment outside hunks must NOT trigger auto-expand.
            sample_thread("t-res", "src/drift.rs", CommentSide::New, 80, 80, true),
            // Unresolved comment already inside hunk 0 must NOT trigger auto-expand.
            sample_thread("t-in", "src/drift.rs", CommentSide::New, 15, 15, false),
        ];

        let initial = merge_chunks_for_file(&file, map.get("src/drift.rs"));
        let plan = compute_auto_expand_plan(&file, &initial, map.get("src/drift.rs"), &threads)
            .expect("should plan auto-expand for unresolved Old and New comments");

        assert_eq!(plan.attempt_keys.len(), 2);
        // Old 45..=55 maps to new 55..=65; New 125..=137 maps to old 115..=127.
        assert_eq!(plan.new_line_range, (55, 137));
        assert_eq!(plan.old_line_range, (45, 127));

        record_auto_expand_attempts(&mut map, "src/drift.rs", &plan.attempt_keys);
        // While fetch is in flight (before auto_slices are applied), planner deduplicates attempts.
        assert!(
            compute_auto_expand_plan(&file, &initial, map.get("src/drift.rs"), &threads).is_none()
        );

        record_auto_slices(&mut map, "src/drift.rs", &plan.slices);
        let after = merge_chunks_for_file(&file, map.get("src/drift.rs"));
        let keys = collect_merged_line_keys(&after);
        assert!(keys.contains(&(CommentSide::Old, 50)));
        assert!(keys.contains(&(CommentSide::New, 60)));
        assert!(keys.contains(&(CommentSide::New, 132)));
        assert!(keys.contains(&(CommentSide::Old, 122)));
        // Resolved comment at new 80 was not expanded.
        assert!(!keys.contains(&(CommentSide::New, 80)));

        // Re-running planner now returns None because both unresolved threads are placed.
        assert!(
            compute_auto_expand_plan(&file, &after, map.get("src/drift.rs"), &threads).is_none()
        );

        // Clearing expanded_lines (as Refresh / ignore_rebase toggle does) resets attempts so
        // unresolved threads re-plan and re-expand cleanly on the same commit pair.
        map.clear();
        let after_reset = merge_chunks_for_file(&file, map.get("src/drift.rs"));
        let replanned =
            compute_auto_expand_plan(&file, &after_reset, map.get("src/drift.rs"), &threads)
                .expect("clearing expanded_lines must allow auto-expand to re-plan");
        assert_eq!(replanned.attempt_keys.len(), 2);
    }

    #[test]
    fn expand_entire_file_and_collapse_preserves_auto_comment_context() {
        let file = Arc::new(DiffFile {
            path: String::from("src/app.rs"),
            old_path: None,
            status: FileStatus::Modified,
            additions: 1,
            deletions: 0,
            is_generated: false,
            chunks: alloc::vec![DiffChunk {
                old_start: 10,
                old_lines: 1,
                new_start: 10,
                new_lines: 1,
                header: String::from("@@ -10,1 +10,1 @@"),
                lines: alloc::vec![sample_line(
                    DiffLineKind::Context,
                    Some(10),
                    Some(10),
                    "line 10"
                )],
            }],
            chunks_loaded: true,
        });

        let mut map = ExpandedLinesMap::new();
        map.insert(String::from("src/app.rs"), full_state(40));
        record_auto_slices(
            &mut map,
            "src/app.rs",
            &[GapSlice {
                gap_index: 1,
                start_offset: 14,
                end_offset: 25,
            }],
        );

        let before = merge_chunks_for_file(&file, map.get("src/app.rs"));
        assert!(has_hidden_context(&file, &before, map.get("src/app.rs")));
        assert!(!map["src/app.rs"].has_manual_expansion());

        record_expand_entire_file(&mut map, "src/app.rs");
        let expanded = merge_chunks_for_file(&file, map.get("src/app.rs"));
        assert_eq!(expanded.len(), 1);
        assert_eq!(expanded[0].lines.len(), 40);
        assert_eq!(expanded[0].hidden_lines_before, 0);
        assert_eq!(expanded[0].hidden_lines_after, Some(0));
        assert!(!has_hidden_context(&file, &expanded, map.get("src/app.rs")));
        assert!(map["src/app.rs"].has_manual_expansion());

        collapse_manual_expansions(&mut map, "src/app.rs");
        let collapsed = merge_chunks_for_file(&file, map.get("src/app.rs"));
        assert!(has_hidden_context(&file, &collapsed, map.get("src/app.rs")));
        assert!(!map["src/app.rs"].has_manual_expansion());
        // Auto-expanded comment context (lines 25..=35) remains intact.
        let keys = collect_merged_line_keys(&collapsed);
        assert!(keys.contains(&(CommentSide::New, 30)));
        assert!(!keys.contains(&(CommentSide::New, 1)));
    }
}
