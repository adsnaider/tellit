//! Side-by-side line pairing for split diff rendering.

extern crate alloc;

use alloc::vec::Vec;

use tellit_types::{DiffLine, DiffLineKind};

/// Single paired row for split (side-by-side) diff presentation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SideBySideRow {
    /// Old (base) side line, present for context and delete rows.
    pub old_line: Option<DiffLine>,
    /// New (target) side line, present for context and add rows.
    pub new_line: Option<DiffLine>,
    /// Whether `old_line` came from context expansion rather than the original diff hunk.
    pub old_is_expanded: bool,
    /// Whether `new_line` came from context expansion rather than the original diff hunk.
    pub new_is_expanded: bool,
}

/// Pairs contiguous delete and add runs row-for-row while keeping context lines aligned across both panes.
#[must_use]
pub fn pair_side_by_side_rows(lines: &[DiffLine], expanded_flags: &[bool]) -> Vec<SideBySideRow> {
    let mut rows = Vec::with_capacity(lines.len());
    let mut i = 0usize;

    while i < lines.len() {
        let is_exp = |idx: usize| expanded_flags.get(idx).copied().unwrap_or(false);
        match lines[i].kind {
            DiffLineKind::Context => {
                rows.push(SideBySideRow {
                    old_line: Some(lines[i].clone()),
                    new_line: Some(lines[i].clone()),
                    old_is_expanded: is_exp(i),
                    new_is_expanded: is_exp(i),
                });
                i += 1;
            }
            DiffLineKind::Delete => {
                let del_start = i;
                while i < lines.len() && lines[i].kind == DiffLineKind::Delete {
                    i += 1;
                }
                let del_end = i;

                let add_start = i;
                while i < lines.len() && lines[i].kind == DiffLineKind::Add {
                    i += 1;
                }
                let add_end = i;

                let del_len = del_end - del_start;
                let add_len = add_end - add_start;
                let max_len = del_len.max(add_len);

                for offset in 0..max_len {
                    let old_idx = (offset < del_len).then_some(del_start + offset);
                    let new_idx = (offset < add_len).then_some(add_start + offset);
                    rows.push(SideBySideRow {
                        old_line: old_idx.map(|idx| lines[idx].clone()),
                        new_line: new_idx.map(|idx| lines[idx].clone()),
                        old_is_expanded: old_idx.is_some_and(is_exp),
                        new_is_expanded: new_idx.is_some_and(is_exp),
                    });
                }
            }
            DiffLineKind::Add => {
                rows.push(SideBySideRow {
                    old_line: None,
                    new_line: Some(lines[i].clone()),
                    old_is_expanded: false,
                    new_is_expanded: is_exp(i),
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
        let lines = alloc::vec![
            mk(DiffLineKind::Context, Some(1), Some(1)),
            mk(DiffLineKind::Delete, Some(2), None),
            mk(DiffLineKind::Add, None, Some(2)),
            mk(DiffLineKind::Add, None, Some(3)),
            mk(DiffLineKind::Context, Some(3), Some(4)),
        ];
        let flags = alloc::vec![true, false, false, false, false];
        let paired = pair_side_by_side_rows(&lines, &flags);

        assert_eq!(paired.len(), 4);
        assert!(paired[0].old_is_expanded && paired[0].new_is_expanded);
        assert!(paired[1].old_line.is_some() && paired[1].new_line.is_some());
        assert!(paired[2].old_line.is_none() && paired[2].new_line.is_some());
        assert_eq!(
            paired[3].new_line.as_ref().unwrap().new_line_number,
            Some(4)
        );
    }
}
