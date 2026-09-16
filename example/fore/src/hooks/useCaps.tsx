// Loads /api/caps + the agent's reachable set. Caps come from the
// cspace; reachability comes from agent_list. They are independent
// reads, so a slow agent doesn't block the cap list, and vice versa.

import type { AgentHandleDescribe, AgentListEntry, CapInfo } from "../api/types";

import { useEffect, useState } from "react";

import { agent, client } from "../api/client";

export interface CapsSnapshot {
  caps: CapInfo[];
  /** The agent's binding row, indexed by capability name. */
  reachable: Map<string, AgentListEntry>;
  /** Result of one agent_describe per reachable handle. */
  describes: Map<string, AgentHandleDescribe>;
  /** True once both reads have settled (success OR failure). */
  ready: boolean;
  /** Human-readable error if the agent is unavailable. The cap
   *  list keeps working in this state — only reachability and
   *  rights are missing. */
  agentError: string | null;
}

export function useCaps(): CapsSnapshot {
  const [caps, setCaps] = useState<CapInfo[]>([]);
  const [reachable, setReachable] = useState<Map<string, AgentListEntry>>(new Map());
  const [describes, setDescribes] = useState<Map<string, AgentHandleDescribe>>(new Map());
  const [ready, setReady] = useState(false);
  const [agentError, setAgentError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const capRes = await client.listCaps().catch((e) => {
        if (!cancelled) throw e;
        return [] as CapInfo[];
      });
      if (cancelled) return;
      setCaps(capRes);

      // Try the agent. The whole UI degrades to "no reachability
      // info" if this fails — the cap list and the invoke forms
      // still work.
      try {
        const list = await agent.list();
        const map = new Map(list.handles.map((h) => [h.handle, h]));
        if (cancelled) return;
        setReachable(map);
        // Resolve describes in parallel — each handle is one
        // round trip, all independent.
        const describes = new Map<string, AgentHandleDescribe>();
        await Promise.all(
          list.handles.map(async (h) => {
            try {
              const d = await agent.describe(h.handle);
              describes.set(h.handle, d);
            } catch {
              // Per-handle failure: the row is still listed but
              // the describe panel shows "unknown". Don't take
              // down the whole table for one row.
            }
            return;
          }),
        );
        if (cancelled) return;
        setDescribes(describes);
        setAgentError(null);
      } catch (e) {
        if (cancelled) return;
        setReachable(new Map());
        setDescribes(new Map());
        setAgentError((e as Error).message);
      }
      if (!cancelled) setReady(true);
    })().catch((e) => {
      if (!cancelled) {
        setReady(true);
        setAgentError((e as Error).message);
      }
    });
    return () => {
      cancelled = true;
    };
  }, []);

  return { caps, reachable, describes, ready, agentError };
}
