import { type WatchEvent } from '../../types/watch';

import { resolveEventSourceUrl } from './eventSourceUrl';

export interface LiveWatchCallbacks {
  onReloadSignal: (changeType: 'file' | 'commit' | 'staging') => void;
  onCommentsChanged: () => void;
  onConnectionChange?: (status: 'connected' | 'disconnected' | 'reconnecting') => void;
}

export interface EventSourceLike {
  onopen: ((event: Event) => void) | null;
  onmessage: ((event: MessageEvent) => void) | null;
  onerror: ((event: Event) => void) | null;
  close: () => void;
}

export type EventSourceFactory = (url: string) => EventSourceLike;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isWatchEvent(value: unknown): value is WatchEvent {
  if (!isRecord(value) || typeof value.type !== 'string') return false;
  return (
    value.type === 'connected' ||
    value.type === 'reload' ||
    value.type === 'error' ||
    value.type === 'commentsChanged'
  );
}

const DEFAULT_RECONNECT_DELAY_MS = 3000;
const MAX_RECONNECT_ATTEMPTS = 5;

export function connectLiveWatch(
  callbacks: LiveWatchCallbacks,
  createEventSource: EventSourceFactory | undefined = typeof EventSource !== 'undefined'
    ? (url) => new EventSource(url)
    : undefined,
): () => void {
  if (!createEventSource) {
    return () => {};
  }

  let disposed = false;
  let watchSource: EventSourceLike | null = null;
  let heartbeatSource: EventSourceLike | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let reconnectAttempts = 0;

  const openWatchStream = () => {
    if (disposed || watchSource) return;

    try {
      const source = createEventSource(resolveEventSourceUrl('/api/watch'));
      watchSource = source;

      source.onopen = () => {
        if (disposed) return;
        reconnectAttempts = 0;
        callbacks.onConnectionChange?.('connected');
      };

      source.onmessage = (event: MessageEvent) => {
        if (disposed || typeof event.data !== 'string') return;
        try {
          const parsed: unknown = JSON.parse(event.data);
          if (!isWatchEvent(parsed)) return;

          if (parsed.type === 'connected') {
            callbacks.onConnectionChange?.('connected');
          } else if (parsed.type === 'reload') {
            callbacks.onReloadSignal(parsed.changeType);
          } else if (parsed.type === 'commentsChanged') {
            callbacks.onCommentsChanged();
          }
        } catch {
          // Ignore malformed SSE frames.
        }
      };

      source.onerror = () => {
        source.close();
        if (watchSource === source) {
          watchSource = null;
        }
        if (disposed) return;

        if (reconnectAttempts < MAX_RECONNECT_ATTEMPTS) {
          reconnectAttempts += 1;
          callbacks.onConnectionChange?.('reconnecting');
          reconnectTimer = setTimeout(() => {
            reconnectTimer = null;
            openWatchStream();
          }, DEFAULT_RECONNECT_DELAY_MS);
        } else {
          callbacks.onConnectionChange?.('disconnected');
        }
      };
    } catch {
      callbacks.onConnectionChange?.('disconnected');
    }
  };

  try {
    const hb = createEventSource(resolveEventSourceUrl('/api/heartbeat'));
    heartbeatSource = hb;
    hb.onerror = () => {
      hb.close();
      if (heartbeatSource === hb) {
        heartbeatSource = null;
      }
    };
  } catch {
    // Heartbeat is optional in environments without SSE support.
  }

  openWatchStream();

  return () => {
    disposed = true;
    if (reconnectTimer) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
    if (watchSource) {
      watchSource.close();
      watchSource = null;
    }
    if (heartbeatSource) {
      heartbeatSource.close();
      heartbeatSource = null;
    }
  };
}
