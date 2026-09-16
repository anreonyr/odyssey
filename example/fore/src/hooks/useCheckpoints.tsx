// Checkpoints hook. The Rust bridge exposes
// `GET /api/checkpoints` (added alongside the new IA); this
// hook fetches the list once on mount and refetches when the
// caller asks for it (after a session is cancelled with a
// checkpoint path).
//
// The shape mirrors the Rust bridge's CheckpointInfo record.

import type { CheckpointInfo } from "../api/types";

import { useCallback, useEffect, useState } from "react";

import { client } from "../api/client";

export interface CheckpointsSnapshot {
  checkpoints: CheckpointInfo[];
  loading: boolean;
  error: string | null;
  refetch: () => void;
}

export function useCheckpoints(): CheckpointsSnapshot {
  const [checkpoints, setCheckpoints] = useState<CheckpointInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [tick, setTick] = useState(0);

  const refetch = useCallback(() => setTick((n) => n + 1), []);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    client
      .listCheckpoints()
      .then((rows) => {
        if (cancelled) return;
        setCheckpoints(rows);
        setError(null);
      })
      .catch((e: unknown) => {
        if (cancelled) return;
        setError((e as Error).message ?? String(e));
        setCheckpoints([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [tick]);

  return { checkpoints, loading, error, refetch };
}
