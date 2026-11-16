import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { connectLiveWatch, type EventSourceLike } from './liveWatch';

interface MockSource extends EventSourceLike {
  url: string;
  closed: boolean;
}

describe('connectLiveWatch', () => {
  let createdSources: MockSource[];

  beforeEach(() => {
    vi.useFakeTimers();
    createdSources = [];
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  const factory = (url: string): EventSourceLike => {
    const source: MockSource = {
      url,
      closed: false,
      onopen: null,
      onmessage: null,
      onerror: null,
      close: vi.fn(() => {
        source.closed = true;
      }),
    };
    createdSources.push(source);
    return source;
  };

  it('connects to /api/heartbeat and /api/watch, dispatches events, and closes both on teardown', () => {
    const onReloadSignal = vi.fn();
    const onCommentsChanged = vi.fn();
    const onConnectionChange = vi.fn();

    const disconnect = connectLiveWatch(
      { onReloadSignal, onCommentsChanged, onConnectionChange },
      factory,
    );

    expect(createdSources.map((s) => s.url)).toEqual(['/api/heartbeat', '/api/watch']);
    const watch = createdSources[1]!;

    watch.onopen?.(new Event('open'));
    expect(onConnectionChange).toHaveBeenCalledWith('connected');

    watch.onmessage?.(
      new MessageEvent('message', {
        data: JSON.stringify({
          type: 'reload',
          diffMode: 'default',
          changeType: 'commit',
          timestamp: '2026-11-16T21:00:00Z',
        }),
      }),
    );
    expect(onReloadSignal).toHaveBeenCalledWith('commit');

    watch.onmessage?.(
      new MessageEvent('message', {
        data: JSON.stringify({
          type: 'commentsChanged',
          version: 2,
          timestamp: '2026-11-16T21:01:00Z',
        }),
      }),
    );
    expect(onCommentsChanged).toHaveBeenCalledTimes(1);

    disconnect();
    expect(createdSources[0]?.close).toHaveBeenCalledTimes(1);
    expect(createdSources[1]?.close).toHaveBeenCalledTimes(1);
  });

  it('reconnects /api/watch after errors and stops reconnecting once disposed', () => {
    const onConnectionChange = vi.fn();
    const disconnect = connectLiveWatch(
      { onReloadSignal: vi.fn(), onCommentsChanged: vi.fn(), onConnectionChange },
      factory,
    );

    const firstWatch = createdSources[1]!;
    firstWatch.onerror?.(new Event('error'));
    expect(firstWatch.close).toHaveBeenCalledTimes(1);
    expect(onConnectionChange).toHaveBeenCalledWith('reconnecting');

    vi.advanceTimersByTime(3000);
    expect(createdSources).toHaveLength(3);
    expect(createdSources[2]?.url).toBe('/api/watch');

    disconnect();
    vi.advanceTimersByTime(10000);
    expect(createdSources).toHaveLength(3);
  });
});
