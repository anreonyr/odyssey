// Generic invoke hook. Wraps the typed `client.invoke` and exposes
// the loading + error states the UI cares about.

import { useCallback, useState } from "react";
import { client } from "../api/client";

export interface InvokeState<T> {
  value: T | null;
  error: string | null;
  busy: boolean;
}

export function useInvoke<T = unknown>(capability: string, buildInput: () => unknown) {
  const [state, setState] = useState<InvokeState<T>>({ value: null, error: null, busy: false });

  const run = useCallback(async () => {
    setState({ value: null, error: null, busy: true });
    try {
      const value = await client.invoke<T>(capability, buildInput());
      setState({ value, error: null, busy: false });
      return value;
    } catch (e) {
      setState({ value: null, error: (e as Error).message, busy: false });
      throw e;
    }
  }, [capability, buildInput]);

  const reset = useCallback(() => setState({ value: null, error: null, busy: false }), []);

  return { ...state, run, reset };
}