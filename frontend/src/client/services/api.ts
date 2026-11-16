import { type BaseMode, type DiffResponse, type JjChange, type JjGraphRow } from '../../types/diff';

export interface JjStatusResponse {
  isJj: boolean;
  repoRoot?: string;
}

export interface JjChangesResponse {
  changes: JjChange[];
  graphRows: JjGraphRow[];
}

export interface FetchDiffOptions {
  base?: string;
  target?: string;
  baseMode?: BaseMode;
  ignoreWhitespace?: boolean;
  signal?: AbortSignal;
}

export interface FetchJjDiffOptions {
  target: string;
  base?: string;
  ignoreRebase?: boolean;
  signal?: AbortSignal;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isAbortError(error: unknown): boolean {
  return isRecord(error) && error.name === 'AbortError';
}

function isDiffResponse(value: unknown): value is DiffResponse {
  return isRecord(value) && typeof value.commit === 'string' && Array.isArray(value.files);
}

export function normalizeJjBaseCommitId(base?: string): string | undefined {
  if (!base || base === 'parent' || base.endsWith('^') || base.endsWith('-')) {
    return undefined;
  }
  return base;
}

function makeStandaloneCommitRow(change: JjChange): JjGraphRow {
  return {
    type: 'commit',
    key: change.commitId,
    graphCols: [change.isWorkingCopy ? '@' : change.isImmutable ? '◆' : '○'],
    downCols: [],
    change,
  };
}

export async function readApiErrorMessage(res: Response, fallbackMessage: string): Promise<string> {
  try {
    const payload: unknown = await res.json();
    if (isRecord(payload) && typeof payload.error === 'string' && payload.error.trim()) {
      return payload.error.trim();
    }
  } catch {
    // Fall back to HTTP status details when the response body is not JSON.
  }
  const statusSuffix = res.statusText ? `${res.status} ${res.statusText}` : `${res.status}`;
  return `${fallbackMessage} (${statusSuffix})`;
}

export async function fetchJjStatus(signal?: AbortSignal): Promise<JjStatusResponse> {
  try {
    const res = await fetch('/api/jj/status', signal ? { signal } : undefined);
    if (!res.ok) {
      return { isJj: false };
    }
    const payload: unknown = await res.json();
    if (!isRecord(payload)) {
      return { isJj: false };
    }
    return {
      isJj: Boolean(payload.isJj),
      repoRoot: typeof payload.repoRoot === 'string' ? payload.repoRoot : undefined,
    };
  } catch (error) {
    if (isAbortError(error)) {
      throw error;
    }
    return { isJj: false };
  }
}

export async function fetchJjChanges(
  query?: string,
  signal?: AbortSignal,
): Promise<JjChangesResponse> {
  const trimmedQuery = query?.trim();
  const url = trimmedQuery
    ? `/api/jj/changes?query=${encodeURIComponent(trimmedQuery)}`
    : '/api/jj/changes';
  const res = await fetch(url, signal ? { signal } : undefined);
  if (!res.ok) {
    throw new Error(await readApiErrorMessage(res, 'Failed to fetch Jujutsu changes'));
  }

  const payload: unknown = await res.json();
  if (!isRecord(payload)) {
    return { changes: [], graphRows: [] };
  }

  const changes: JjChange[] = Array.isArray(payload.changes) ? (payload.changes as JjChange[]) : [];
  const rawGraphRows: JjGraphRow[] | undefined = Array.isArray(payload.graphRows)
    ? (payload.graphRows as JjGraphRow[])
    : undefined;
  const graphRows =
    rawGraphRows && (rawGraphRows.length > 0 || changes.length === 0)
      ? rawGraphRows
      : changes.map(makeStandaloneCommitRow);

  return { changes, graphRows };
}

export async function fetchDiff(options: FetchDiffOptions = {}): Promise<DiffResponse> {
  const params = new URLSearchParams({
    ignoreWhitespace: String(options.ignoreWhitespace ?? true),
  });
  if (options.base) {
    params.set('base', options.base);
  }
  if (options.target) {
    params.set('target', options.target);
  }
  if (options.baseMode === 'merge-base') {
    params.set('baseMode', options.baseMode);
  }

  const res = await fetch(
    `/api/diff?${params.toString()}`,
    options.signal ? { signal: options.signal } : undefined,
  );
  if (!res.ok) {
    throw new Error(await readApiErrorMessage(res, 'Failed to fetch diff data'));
  }

  const payload: unknown = await res.json();
  if (!isDiffResponse(payload)) {
    throw new Error('Invalid diff response');
  }
  return payload;
}

export async function fetchJjDiff(options: FetchJjDiffOptions): Promise<DiffResponse> {
  const params = new URLSearchParams({
    target: options.target,
  });
  const normalizedBase = normalizeJjBaseCommitId(options.base);
  if (normalizedBase) {
    params.set('base', normalizedBase);
  }
  if (options.ignoreRebase !== undefined) {
    params.set('ignoreRebase', String(options.ignoreRebase));
  }

  const res = await fetch(
    `/api/jj/diff?${params.toString()}`,
    options.signal ? { signal: options.signal } : undefined,
  );
  if (!res.ok) {
    throw new Error(await readApiErrorMessage(res, 'Failed to fetch Jujutsu diff'));
  }

  const payload: unknown = await res.json();
  if (!isDiffResponse(payload)) {
    throw new Error('Invalid diff response');
  }
  return payload;
}
