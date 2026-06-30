import { useEffect, useRef, useState } from 'react';
import { api, streamActivity } from './api';
import type { ActivityEvent } from './activityTypes';
import { reduceActivity, seedOffice, type OfficeState } from './officeState';

export interface ActivityStream {
  office: OfficeState;
  connected: boolean;
}

/**
 * Seed the office from the snapshot, then apply live SSE deltas. Reconnects with
 * backoff; on each (re)connect it re-seeds from the snapshot to resync after any
 * dropped events.
 */
export function useActivityStream(enabled: boolean): ActivityStream {
  const [office, setOffice] = useState<OfficeState>(new Map());
  const [connected, setConnected] = useState(false);

  // keep latest office in a ref so the SSE callback reduces onto current state
  const officeRef = useRef<OfficeState>(office);

  useEffect(() => {
    officeRef.current = office;
  });

  useEffect(() => {
    if (!enabled) {
      return;
    }
    let cancelled = false;
    const controller = new AbortController();
    let backoff = 1000;

    const apply = (event: ActivityEvent) => {
      const next = reduceActivity(officeRef.current, event);
      officeRef.current = next;
      setOffice(next);
    };

    async function run() {
      while (!cancelled) {
        try {
          const snap = await api.activitySnapshot();
          if (cancelled) return;
          const seeded = seedOffice(snap.personas);
          officeRef.current = seeded;
          setOffice(seeded);
          setConnected(true);
          backoff = 1000;
          await streamActivity({ onEvent: apply, signal: controller.signal });
        } catch {
          if (cancelled) return;
          setConnected(false);
        }
        if (cancelled) return;
        await new Promise((r) => setTimeout(r, backoff));
        backoff = Math.min(backoff * 2, 15000);
      }
    }

    void run();
    return () => {
      cancelled = true;
      controller.abort();
    };
  }, [enabled]);

  return { office, connected };
}
