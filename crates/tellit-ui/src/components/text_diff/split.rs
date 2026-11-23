//! Side-by-side line pairing for split diff rendering.

extern crate alloc;

use alloc::vec::Vec;

use tellit_types::DiffLineKind;

use super::expanded::MergedChunk;

/// Single paired row for split (side-by-side) diff presentation referencing lines in [`MergedChunk`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SideBySideRow {
    /// Index into [`MergedChunk::lines`] for the old (base) side, present for context and delete rows.
    pub old_line_idx: Option<usize>,
    /// Index into [`MergedChunk::lines`] for the new (target) side, present for context and add rows.
    pub new_line_idx: Option<usize>,
}

/// Pairs contiguous delete and add runs row-for-row while keeping context lines aligned across both panes.
#[must_use]
pub fn pair_side_by_side_rows(chunk: &MergedChunk) -> Vec<SideBySideRow> {
    let total = chunk.lines.len();
    let mut rows = Vec::with_capacity(total);
    let mut i = 0usize;

    while i < total {
        let Some(line) = chunk.line(i) else {
            i += 1;
            continue;
        };
        match line.kind {
            DiffLineKind::Context => {
                rows.push(SideBySideRow {
                    old_line_idx: Some(i),
                    new_line_idx: Some(i),
                });
                i += 1;
            }
            DiffLineKind::Delete => {
                let del_start = i;
                while i < total
                    && chunk
                        .line(i)
                        .is_some_and(|l| l.kind == DiffLineKind::Delete)
                {
                    i += 1;
                }
                let del_end = i;

                let add_start = i;
                while i < total && chunk.line(i).is_some_and(|l| l.kind == DiffLineKind::Add) {
                    i += 1;
                }
                let add_end = i;

                let del_len = del_end - del_start;
                let add_len = add_end - add_start;
                let max_len = del_len.max(add_len);

                for offset in 0..max_len {
                    let old_line_idx = (offset < del_len).then_some(del_start + offset);
                    let new_line_idx = (offset < add_len).then_some(add_start + offset);
                    rows.push(SideBySideRow {
                        old_line_idx,
                        new_line_idx,
                    });
                }
            }
            DiffLineKind::Add => {
                rows.push(SideBySideRow {
                    old_line_idx: None,
                    new_line_idx: Some(i),
                });
                i += 1;
            }
        }
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use alloc::sync::Arc;
    use tellit_types::{DiffChunk, DiffFile, DiffLine, FileStatus};

    use super::super::expanded::ChunkLineRef;

    fn mk(kind: DiffLineKind, old_n: Option<u32>, new_n: Option<u32>) -> DiffLine {
        DiffLine {
            kind,
            content: String::from("code"),
            old_line_number: old_n,
            new_line_number: new_n,
            syntax_spans: Vec::new(),
            word_diff: Vec::new(),
        }
    }

    #[test]
    fn pairs_deletes_and_adds_leaving_trailing_add_on_right_only() {
        let file = Arc::new(DiffFile {
            path: String::from("src/lib.rs"),
            old_path: None,
            status: FileStatus::Modified,
            additions: 2,
            deletions: 1,
            is_generated: false,
            chunks: alloc::vec![DiffChunk {
                old_start: 2,
                old_lines: 2,
                new_start: 2,
                new_lines: 3,
                header: String::from("@@ -2,2 +2,3 @@"),
                lines: alloc::vec![
                    mk(DiffLineKind::Delete, Some(2), None),
                    mk(DiffLineKind::Add, None, Some(2)),
                    mk(DiffLineKind::Add, None, Some(3)),
                    mk(DiffLineKind::Context, Some(3), Some(4)),
                ],
            }],
        });
        let chunk = MergedChunk {
            file,
            old_start: 1,
            old_lines: 3,
            new_start: 1,
            new_lines: 4,
            header: String::from("@@ -1,3 +1,4 @@"),
            lines: alloc::vec![
                ChunkLineRef::Expanded(Arc::new(mk(DiffLineKind::Context, Some(1), Some(1)))),
                ChunkLineRef::Original(0, 0),
                ChunkLineRef::Original(0, 1),
                ChunkLineRef::Original(0, 2),
                ChunkLineRef::Original(0, 3),
            ],
            original_indices: alloc::vec![0],
            hidden_lines_before: 0,
            hidden_lines_after: Some(0),
        };
        let paired = pair_side_by_side_rows(&chunk);

        assert_eq!(paired.len(), 4);
        assert!(chunk.is_line_expanded(paired[0].old_line_idx.unwrap()));
        assert!(chunk.is_line_expanded(paired[0].new_line_idx.unwrap()));
        assert!(paired[1].old_line_idx.is_some() && paired[1].new_line_idx.is_some());
        assert!(paired[2].old_line_idx.is_none() && paired[2].new_line_idx.is_some());
        assert_eq!(
            chunk
                .line(paired[3].new_line_idx.unwrap())
                .unwrap()
                .new_line_number,
            Some(4)
        );
    }
}
