import { type JjChange, type JjGraphRow } from '../../types/diff';

export function makeStandaloneCommitRow(change: JjChange): JjGraphRow {
  return {
    type: 'commit',
    key: change.commitId,
    graphCols: [change.isWorkingCopy ? '@' : change.isImmutable ? '◆' : '○'],
    downCols: [],
    change,
  };
}

export type GraphNodeKind = 'working' | 'mutable' | 'immutable' | 'conflict' | 'hidden';

export interface GraphCellLayout {
  up: boolean;
  down: boolean;
  left: boolean;
  right: boolean;
  diagonal?: '/' | '\\';
  nodeKind?: GraphNodeKind;
  isElided: boolean;
  dashedDown: boolean;
}

export interface GraphRowLayout {
  cols: GraphCellLayout[];
}

export interface ReviewStatusDot {
  kind: 'unresolved' | 'approved' | 'clean-rebase' | 'resolved';
  classes: string;
  title: string;
}

const NODE_GLYPHS: Record<string, GraphNodeKind> = {
  '@': 'working',
  '○': 'mutable',
  o: 'mutable',
  '◆': 'immutable',
  '*': 'immutable',
  '×': 'conflict',
  x: 'conflict',
  '◌': 'hidden',
};

const UPWARD_CONNECTING_GLYPHS = "│|:╯┘'╰└`├┤┴┼+";

function createEmptyCell(): GraphCellLayout {
  return {
    up: false,
    down: false,
    left: false,
    right: false,
    isElided: false,
    dashedDown: false,
  };
}

function connectsUpwardFromChar(ch: string): boolean {
  return ch in NODE_GLYPHS || ch === '~' || UPWARD_CONNECTING_GLYPHS.includes(ch);
}

/**
 * Computes per-row, per-column SVG rendering instructions from parsed `jj log`
 * graph rows, supporting curved, square, and ASCII box-drawing glyphs.
 */
export function computeGraphLayout(rows: JjGraphRow[]): GraphRowLayout[] {
  const layout: GraphRowLayout[] = rows.map((row) => {
    const cols: GraphCellLayout[] = row.graphCols.map((ch, colIdx) => {
      const cell = createEmptyCell();
      const nodeKind = NODE_GLYPHS[ch];
      if (nodeKind) {
        cell.nodeKind = nodeKind;
        cell.down = row.type === 'commit' ? row.downCols.includes(colIdx) : false;
        return cell;
      }

      if (ch === '~') {
        cell.isElided = true;
        cell.dashedDown = row.type === 'connector' ? Boolean(row.bridgesElided) : false;
        return cell;
      }

      switch (ch) {
        case '│':
        case '|':
          cell.up = true;
          cell.down = row.type === 'commit' ? row.downCols.includes(colIdx) : true;
          break;
        case ':':
          cell.up = true;
          cell.down = row.type === 'commit' ? row.downCols.includes(colIdx) : true;
          cell.dashedDown = true;
          break;
        case '─':
        case '-':
          cell.left = true;
          cell.right = true;
          break;
        case '╭':
        case '┌':
          cell.down = true;
          cell.right = true;
          break;
        case '╮':
        case '┐':
          cell.down = true;
          cell.left = true;
          break;
        case '╯':
        case '┘':
        case "'":
          cell.up = true;
          cell.left = true;
          break;
        case '╰':
        case '└':
        case '`':
          cell.up = true;
          cell.right = true;
          break;
        case '├':
          cell.up = true;
          cell.down = true;
          cell.right = true;
          break;
        case '┤':
          cell.up = true;
          cell.down = true;
          cell.left = true;
          break;
        case '┬':
          cell.down = true;
          cell.left = true;
          cell.right = true;
          break;
        case '┴':
          cell.up = true;
          cell.left = true;
          cell.right = true;
          break;
        case '┼':
        case '+':
          cell.up = true;
          cell.down = true;
          cell.left = true;
          cell.right = true;
          break;
        case '/':
          cell.diagonal = '/';
          break;
        case '\\':
          cell.diagonal = '\\';
          break;
        default:
          break;
      }

      return cell;
    });

    return { cols };
  });

  for (let r = 0; r < rows.length; r++) {
    const row = rows[r];
    const rowLayout = layout[r];
    if (!row || !rowLayout) continue;

    const prevRowLayout = r > 0 ? layout[r - 1] : undefined;
    const nextRow = r + 1 < rows.length ? rows[r + 1] : undefined;

    for (let c = 0; c < rowLayout.cols.length; c++) {
      const cell = rowLayout.cols[c];
      if (!cell) continue;

      if (cell.nodeKind || cell.isElided) {
        const prevCell = prevRowLayout?.cols[c];
        if (prevCell?.down) {
          cell.up = true;
        }
      }

      if (cell.isElided && nextRow) {
        const nextChar = nextRow.graphCols[c] ?? ' ';
        if (connectsUpwardFromChar(nextChar)) {
          cell.down = true;
          if (row.type === 'connector' && row.bridgesElided) {
            cell.dashedDown = true;
          }
        }
      }
    }
  }

  return layout;
}

export function getReviewStatusDot(options: {
  commentCount?: number;
  unresolvedCommentCount?: number;
  approved?: boolean;
  cleanRebaseApproved?: boolean;
  cleanRebaseSourceNumber?: number;
  cleanRebaseSourceCommitNumber?: number;
}): ReviewStatusDot | null {
  const count = options.commentCount ?? 0;
  const unresolved = options.unresolvedCommentCount ?? 0;
  const sourceNumber = options.cleanRebaseSourceNumber ?? options.cleanRebaseSourceCommitNumber;

  // Unresolved comments take precedence over approval state.
  if (unresolved > 0) {
    return {
      kind: 'unresolved',
      classes: 'w-2 h-2 bg-yellow-400 shadow-sm',
      title: `${unresolved} unresolved comment${unresolved === 1 ? '' : 's'}`,
    };
  }

  if (options.approved) {
    return {
      kind: 'approved',
      classes: 'w-2 h-2 bg-green-500 shadow-sm',
      title: count > 0 ? 'Approved (all comments resolved)' : 'Approved',
    };
  }

  if (options.cleanRebaseApproved) {
    return {
      kind: 'clean-rebase',
      classes: 'w-2.5 h-2.5 bg-emerald-400 border border-emerald-200/60 shadow-sm',
      title: sourceNumber
        ? `Approved (Clean Rebase from #${sourceNumber})`
        : 'Approved (Clean Rebase)',
    };
  }

  if (count > 0) {
    return {
      kind: 'resolved',
      classes: 'w-2 h-2 bg-gray-400',
      title: 'All comments resolved',
    };
  }

  return null;
}
