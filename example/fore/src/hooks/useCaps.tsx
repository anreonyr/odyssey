// Loads /api/caps + the agent's reachable set. Caps come from the
// cspace; reachability comes from agent_list. They are independent
// reads, so a slow agent doesn't block the cap list, and vice versa.

import type { AgentHandleDescribe, AgentListEntry, CapInfo } from "../api/types";

import { useEffect, useMemo, useState } from "react";

import { agent, client } from "../api/client";

/** Caps that aren't tools — they're the agent's own meta caps
 *  plus the read-only observers. These should not show up in the
 *  Chat / AgentSessions tool pickers (where users choose what
 *  the agent is allowed to call). */
const META_CAP_PREFIXES = [
  "agent_",
  "profile_",
  "schema_",
  "http_bridge",
];

export function isMetaCap(name: string): boolean {
  return META_CAP_PREFIXES.some((p) => name === p || name.startsWith(p));
}

export interface CapsSnapshot {
  caps: CapInfo[];
  /** Every cspace cap, indexed by capability name. Populated from
   *  agent_list (which walks the global cspace) so a cap not on
   *  any plugin's binding row is still visible to the UI. */
  reachable: Map<string, AgentListEntry>;
  /** Subset of `reachable` that are actual tools (not meta caps).
   *  Drives the Chat / AgentSessions tool pickers. */
  tools: string[];
  /** Result of one agent_describe per cspace cap. */
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
        // Key by capability name — the cspace-wide cap id, not the
        // binding-row handle (which is only set for typed slots).
        // Other surfaces (CapabilitiesTab filter, Chat tool picker)
        // join on cap.name, so the map key has to match.
        const map = new Map(list.handles.map((h) => [h.capability, h]));
        if (cancelled) return;
        setReachable(map);
        // Resolve describes in parallel — one round trip per
        // cspace cap, all independent.
        const describes = new Map<string, AgentHandleDescribe>();
        await Promise.all(
          list.handles.map(async (h): Promise<void> => {
            try {
              const d = await agent.describe(h.capability);
              describes.set(h.capability, d);
            } catch {
              // Per-cap failure: the row is still listed but
              // the describe panel shows "unknown". Don't take
              // down the whole table for one row.
            }
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

  const tools = useMemo(
    () => Array.from(reachable.keys()).filter((n) => !isMetaCap(n)).sort(),
    [reachable],
  );

  return { caps, reachable, tools, describes, ready, agentError };
}
