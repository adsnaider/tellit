import { ChevronDown, ChevronRight, GitBranch, Search, X } from 'lucide-react';
import { type FormEvent, useEffect, useMemo, useRef, useState } from 'react';

import { type JjChange, type JjGraphRow } from '../../types/diff';

export const REVISIONS_TREE_STORAGE_KEY = 'tellit.revisionsTreeOpen';

const COL_WIDTH = 11;
const COMMIT_ROW_HEIGHT = 28;
const CONNECTOR_ROW_HEIGHT = 16;
const STEM_CLEARANCE = 5;

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

const UPWARD_CONNECTING_GLYPHS = '│|:╯┘\'╰└`├┤┴┼+';

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
        case '\'':
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

  // Connect node and elision cells vertically to neighboring rows.
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

export function CommentDot({
  commentCount,
  unresolvedCommentCount,
  approved,
  cleanRebaseApproved,
  cleanRebaseSourceNumber,
}: {
  commentCount?: number;
  unresolvedCommentCount?: number;
  approved?: boolean;
  cleanRebaseApproved?: boolean;
  cleanRebaseSourceNumber?: number;
}) {
  const count = commentCount ?? 0;
  const unresolved = unresolvedCommentCount ?? 0;

  // Comments take precedence. Only if all comments are resolved should it show green.
  if (unresolved > 0) {
    return (
      <span
        className="inline-block w-2 h-2 rounded-full shrink-0 bg-yellow-400 shadow-sm"
        title={`${unresolved} unresolved comment${unresolved > 1 ? 's' : ''}`}
      />
    );
  }

  if (approved) {
    return (
      <span
        className="inline-block w-2 h-2 rounded-full shrink-0 bg-green-500 shadow-sm"
        title={count > 0 ? 'Approved (all comments resolved)' : 'Approved'}
      />
    );
  }

  if (cleanRebaseApproved) {
    return (
      <span
        className="inline-block w-2.5 h-2.5 rounded-full shrink-0 bg-emerald-400 border border-emerald-200/60 shadow-sm"
        title={
          cleanRebaseSourceNumber
            ? `Approved (Clean Rebase from #${cleanRebaseSourceNumber})`
            : 'Approved (Clean Rebase)'
        }
      />
    );
  }

  if (count > 0) {
    return (
      <span
        className="inline-block w-2 h-2 rounded-full shrink-0 bg-gray-400"
        title="All comments resolved"
      />
    );
  }

  return null;
}

function GraphCellSvg({
  cell,
  colIdx,
  rowHeight,
  isSelected,
}: {
  cell: GraphCellLayout;
  colIdx: number;
  rowHeight: number;
  isSelected: boolean;
}) {
  const xLeft = colIdx * COL_WIDTH;
  const xRight = xLeft + COL_WIDTH;
  const cx = xLeft + COL_WIDTH / 2;
  const cy = rowHeight / 2;
  const strokeClass = 'stroke-github-text-muted/70';

  const isPureCorner =
    !cell.nodeKind &&
    !cell.isElided &&
    !cell.diagonal &&
    Number(cell.up) + Number(cell.down) + Number(cell.left) + Number(cell.right) === 2 &&
    (cell.up || cell.down) &&
    (cell.left || cell.right);

  const hasNodeOrElision = Boolean(cell.nodeKind || cell.isElided);
  const upEnd = hasNodeOrElision ? Math.max(0, cy - STEM_CLEARANCE) : cy;
  const downStart = hasNodeOrElision ? Math.min(rowHeight, cy + STEM_CLEARANCE) : cy;

  return (
    <g>
      {isPureCorner ? (
        <path
          d={`M ${cx} ${cell.up ? 0 : rowHeight} Q ${cx} ${cy} ${cell.left ? xLeft : xRight} ${cy}`}
          fill="none"
          className={strokeClass}
          strokeWidth={1.5}
          strokeLinecap="round"
        />
      ) : (
        <>
          {cell.up && (
            <line
              x1={cx}
              y1={0}
              x2={cx}
              y2={upEnd}
              className={strokeClass}
              strokeWidth={1.5}
              strokeDasharray={cell.dashedDown && !hasNodeOrElision ? '2 2' : undefined}
            />
          )}
          {cell.down && (
            <line
              x1={cx}
              y1={downStart}
              x2={cx}
              y2={rowHeight}
              className={strokeClass}
              strokeWidth={1.5}
              strokeDasharray={cell.dashedDown ? '2 2' : undefined}
            />
          )}
          {cell.left && (
            <line x1={xLeft} y1={cy} x2={cx} y2={cy} className={strokeClass} strokeWidth={1.5} />
          )}
          {cell.right && (
            <line x1={cx} y1={cy} x2={xRight} y2={cy} className={strokeClass} strokeWidth={1.5} />
          )}
        </>
      )}

      {cell.diagonal === '/' && (
        <line x1={xLeft} y1={rowHeight} x2={xRight} y2={0} className={strokeClass} strokeWidth={1.5} />
      )}
      {cell.diagonal === '\\' && (
        <line x1={xLeft} y1={0} x2={xRight} y2={rowHeight} className={strokeClass} strokeWidth={1.5} />
      )}

      {cell.isElided && (
        <text
          x={cx}
          y={cy}
          textAnchor="middle"
          dominantBaseline="central"
          className="fill-github-text-muted font-mono text-[11px]"
          data-elided="true"
        >
          ~
        </text>
      )}

      {cell.nodeKind === 'working' && (
        <text
          x={cx}
          y={cy}
          textAnchor="middle"
          dominantBaseline="central"
          className="fill-github-accent font-mono text-[11px] font-bold"
          data-node-kind="working"
        >
          @
        </text>
      )}

      {cell.nodeKind === 'mutable' && (
        <circle
          cx={cx}
          cy={cy}
          r={3.5}
          className={
            isSelected
              ? 'fill-github-bg-tertiary stroke-github-accent'
              : 'fill-github-bg-secondary stroke-github-text-secondary'
          }
          strokeWidth={1.6}
          data-node-kind="mutable"
        />
      )}

      {cell.nodeKind === 'immutable' && (
        <polygon
          points={`${cx},${cy - 4} ${cx + 4},${cy} ${cx},${cy + 4} ${cx - 4},${cy}`}
          className="fill-github-text-muted stroke-github-text-muted"
          strokeWidth={1}
          data-node-kind="immutable"
        />
      )}

      {cell.nodeKind === 'conflict' && (
        <path
          d={`M ${cx - 3} ${cy - 3} L ${cx + 3} ${cy + 3} M ${cx + 3} ${cy - 3} L ${cx - 3} ${cy + 3}`}
          className="stroke-red-400"
          strokeWidth={1.75}
          strokeLinecap="round"
          data-node-kind="conflict"
        />
      )}

      {cell.nodeKind === 'hidden' && (
        <circle
          cx={cx}
          cy={cy}
          r={3.5}
          className="fill-none stroke-github-text-muted"
          strokeWidth={1.5}
          strokeDasharray="2 1.5"
          data-node-kind="hidden"
        />
      )}
    </g>
  );
}

export interface JjRevisionTreeProps {
  graphRows: JjGraphRow[];
  selectedChangeId: string;
  onSelectChange: (changeId: string) => void;
  loading?: boolean;
  className?: string;
}

export function JjRevisionTree({
  graphRows,
  selectedChangeId,
  onSelectChange,
  loading = false,
  className = 'max-h-[38vh] overflow-y-auto overflow-x-auto border-b border-github-border',
}: JjRevisionTreeProps) {
  const selectedRowRef = useRef<HTMLButtonElement>(null);
  const layout = useMemo(() => computeGraphLayout(graphRows), [graphRows]);
  const maxCols = useMemo(
    () => Math.max(1, ...graphRows.map((row) => row.graphCols.length)),
    [graphRows],
  );
  const gutterWidth = maxCols * COL_WIDTH;

  useEffect(() => {
    selectedRowRef.current?.scrollIntoView?.({ block: 'nearest' });
  }, [selectedChangeId]);

  if (loading && graphRows.length === 0) {
    return (
      <div className={`${className} px-3 py-2 text-xs text-github-text-secondary`}>
        Loading revisions...
      </div>
    );
  }

  if (graphRows.length === 0) {
    return (
      <div className={`${className} px-3 py-2 text-xs text-github-text-muted`}>
        No revisions found
      </div>
    );
  }

  return (
    <ul className={className} aria-label="Jujutsu revisions">
      {graphRows.map((row, rowIdx) => {
        const rowLayout = layout[rowIdx];
        if (!rowLayout) return null;

        if (row.type === 'connector') {
          return (
            <li
              key={row.key}
              aria-hidden="true"
              className="h-4 flex items-center px-2 select-none pointer-events-none"
            >
              <svg width={gutterWidth} height={CONNECTOR_ROW_HEIGHT} className="shrink-0">
                {rowLayout.cols.map((cell, colIdx) => (
                  <GraphCellSvg
                    key={colIdx}
                    cell={cell}
                    colIdx={colIdx}
                    rowHeight={CONNECTOR_ROW_HEIGHT}
                    isSelected={false}
                  />
                ))}
              </svg>
            </li>
          );
        }

        const { change } = row;
        const isSelected = change.changeId === selectedChangeId;
        const isMuted = Boolean(change.isEmpty && change.hasDescription === false && !isSelected);
        const displayId = change.shortChangeId || change.changeId.slice(0, 8);
        const shortestLen = change.shortestChangeId
          ? Math.min(change.shortestChangeId.length, displayId.length)
          : 0;
        const idPrefix = shortestLen > 0 ? displayId.slice(0, shortestLen) : displayId;
        const idSuffix = shortestLen > 0 ? displayId.slice(shortestLen) : '';

        return (
          <li key={row.key}>
            <button
              ref={isSelected ? selectedRowRef : undefined}
              type="button"
              aria-current={isSelected ? 'true' : undefined}
              aria-label={`${displayId}${change.bookmarks?.length ? ` ${change.bookmarks.join(' ')}` : ''}${change.isEmpty ? ' (empty)' : ''} ${change.description}`}
              onClick={() => onSelectChange(change.changeId)}
              className={`w-full h-7 px-2 text-left flex items-center gap-1.5 transition-colors cursor-pointer ${
                isSelected
                  ? 'bg-github-bg-tertiary text-github-text-primary font-medium'
                  : isMuted
                    ? 'opacity-55 hover:opacity-90 hover:bg-github-bg-tertiary/60 text-github-text-secondary'
                    : 'text-github-text-secondary hover:bg-github-bg-tertiary/60 hover:text-github-text-primary'
              }`}
            >
              <svg
                width={gutterWidth}
                height={COMMIT_ROW_HEIGHT}
                className="shrink-0"
                aria-hidden="true"
              >
                {rowLayout.cols.map((cell, colIdx) => (
                  <GraphCellSvg
                    key={colIdx}
                    cell={cell}
                    colIdx={colIdx}
                    rowHeight={COMMIT_ROW_HEIGHT}
                    isSelected={isSelected}
                  />
                ))}
              </svg>
              <span className="font-mono text-[11px] shrink-0">
                <span className="font-bold text-github-accent">{idPrefix}</span>
                {idSuffix && <span className="text-github-text-muted">{idSuffix}</span>}
              </span>
              {change.bookmarks?.map((bookmark) => (
                <span
                  key={bookmark}
                  className="px-1 py-0 text-[10px] font-mono rounded bg-github-accent/15 text-github-accent border border-github-accent/30 shrink-0 max-w-[90px] truncate"
                  title={bookmark}
                >
                  {bookmark}
                </span>
              ))}
              {change.isEmpty && (
                <span className="text-[10px] text-github-text-muted shrink-0">(empty)</span>
              )}
              <span
                className={`text-xs truncate min-w-0 flex-1 ${
                  change.hasDescription === false
                    ? 'text-github-text-muted italic'
                    : isSelected
                      ? 'text-github-text-primary'
                      : 'text-github-text-secondary'
                }`}
              >
                {change.description}
              </span>
              <CommentDot
                commentCount={change.commentCount}
                unresolvedCommentCount={change.unresolvedCommentCount}
                approved={change.approved}
                cleanRebaseApproved={change.cleanRebaseApproved}
              />
            </button>
          </li>
        );
      })}
    </ul>
  );
}

function getInitialRevisionsTreeOpen(): boolean {
  if (typeof window === 'undefined') return true;
  try {
    const saved = window.localStorage.getItem(REVISIONS_TREE_STORAGE_KEY);
    if (saved === 'false') return false;
    if (saved === 'true') return true;
  } catch {
    // Ignore storage failures
  }
  return true;
}

export interface JjRevisionTreePanelProps {
  changes: JjChange[];
  graphRows: JjGraphRow[];
  selectedChangeId: string;
  onSelectChange: (changeId: string) => void;
  isCustomInput: boolean;
  setIsCustomInput: (val: boolean) => void;
  customInput: string;
  setCustomInput: (val: string) => void;
  customError: string | null;
  handleCustomSubmit: (e: FormEvent) => void;
  loading?: boolean;
}

export function JjRevisionTreePanel({
  changes,
  graphRows,
  selectedChangeId,
  onSelectChange,
  isCustomInput,
  setIsCustomInput,
  customInput,
  setCustomInput,
  customError,
  handleCustomSubmit,
  loading = false,
}: JjRevisionTreePanelProps) {
  const [isOpen, setIsOpen] = useState(getInitialRevisionsTreeOpen);

  const selectedChange = changes.find((c) => c.changeId === selectedChangeId);

  const toggleOpen = () => {
    setIsOpen((prev) => {
      const next = !prev;
      try {
        window.localStorage.setItem(REVISIONS_TREE_STORAGE_KEY, String(next));
      } catch {
        // Ignore storage failures
      }
      return next;
    });
  };

  return (
    <section className="shrink-0 flex flex-col" aria-label="Revisions">
      <div className="flex items-center justify-between px-2.5 py-2 border-b border-github-border bg-github-bg-secondary">
        <button
          type="button"
          onClick={toggleOpen}
          aria-expanded={isOpen}
          className="flex items-center gap-1.5 min-w-0 flex-1 text-left text-xs font-semibold text-github-text-primary hover:text-github-accent cursor-pointer"
        >
          {isOpen ? (
            <ChevronDown size={14} className="text-github-text-muted shrink-0" />
          ) : (
            <ChevronRight size={14} className="text-github-text-muted shrink-0" />
          )}
          <GitBranch size={13} className="text-github-accent shrink-0" />
          <span>Revisions</span>
          {!isOpen && selectedChange && (
            <span className="ml-1 flex items-center gap-1 font-mono text-[11px] font-normal text-github-text-secondary truncate">
              <span>{selectedChange.shortChangeId}</span>
              <CommentDot
                commentCount={selectedChange.commentCount}
                unresolvedCommentCount={selectedChange.unresolvedCommentCount}
                approved={selectedChange.approved}
                cleanRebaseApproved={selectedChange.cleanRebaseApproved}
              />
            </span>
          )}
        </button>

        <button
          type="button"
          onClick={() => {
            if (!isOpen) {
              setIsOpen(true);
              try {
                window.localStorage.setItem(REVISIONS_TREE_STORAGE_KEY, 'true');
              } catch {
                // Ignore storage failures
              }
            }
            setIsCustomInput(!isCustomInput);
          }}
          className="text-github-text-muted hover:text-github-text-primary p-0.5 rounded cursor-pointer shrink-0"
          title="Enter Change ID / Rev"
          aria-label="Search revision"
        >
          <Search size={13} />
        </button>
      </div>

      {isOpen && isCustomInput && (
        <div className="border-b border-github-border bg-github-bg-tertiary/50 px-2 py-1.5">
          <form onSubmit={handleCustomSubmit} className="flex items-center gap-1">
            <input
              type="text"
              value={customInput}
              onChange={(e) => setCustomInput(e.target.value)}
              placeholder="e.g. kkmpptxz or @-"
              className="flex-1 min-w-0 bg-github-bg-secondary text-github-text-primary text-xs font-mono px-1.5 py-0.5 rounded border border-github-border outline-none focus:border-github-accent"
              autoFocus
            />
            <button
              type="submit"
              className="bg-github-accent text-white px-1.5 py-0.5 rounded text-[11px] font-semibold hover:opacity-90 cursor-pointer shrink-0"
            >
              Go
            </button>
            <button
              type="button"
              onClick={() => setIsCustomInput(false)}
              className="text-github-text-muted hover:text-github-text-primary p-0.5 cursor-pointer shrink-0"
              title="Cancel"
              aria-label="Cancel search"
            >
              <X size={12} />
            </button>
          </form>
          {customError && <div className="mt-1 text-[10px] text-red-400">{customError}</div>}
        </div>
      )}

      {isOpen && (
        <JjRevisionTree
          graphRows={graphRows}
          selectedChangeId={selectedChangeId}
          onSelectChange={onSelectChange}
          loading={loading}
        />
      )}
    </section>
  );
}
