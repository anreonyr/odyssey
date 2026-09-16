// Capability browser (`/caps`). Replaces the first half of
// the old App.tsx — every cap, grouped by plugin, click a
// row to navigate to the detail page (`/caps/:name`).
//
// Same `capToPlugin` heuristic as the old code (hand-maintained
// because cap→plugin isn't a strict suffix relationship); the
// describe result from the agent still overrides the lookup so
// reachability wins over the heuristic.

import type { CapInfo } from "../api/types";

import { Search, Filter } from "lucide-react";
import { useMemo, useState } from "react";
import { Link } from "react-router-dom";

import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardHeader } from "../components/ui/card";
import { Input } from "../components/ui/input";
import { useCaps } from "../hooks/useCaps";
import { cn } from "../lib/utils";

const CAP_TO_PLUGIN: Record<string, string> = {
  echo: "echo",
  reverse: "reverse",
  database: "database",
  streaming_echo: "streaming_echo",
  agent_list: "agent_list",
  agent_describe: "agent_describe",
  tool_describe: "tool_descriptor",
  profile_inspect: "profile_inspector",
  llm_complete: "llm",
  llm_embed: "llm",
  memory_query: "memory",
  memory_insert: "memory",
  agent_start: "agent_runtime",
  agent_resume: "agent_runtime",
  agent_cancel: "agent_runtime",
  agent_plan: "agent_runtime",
  agent_stream: "agent_runtime",
  agent_load: "agent_runtime",
  agent_memory_recall: "agent_runtime",
  agent_memory_record: "agent_runtime",
};

function capToPlugin(name: string): string {
  return CAP_TO_PLUGIN[name] ?? name;
}

function groupByPlugin(caps: CapInfo[]): Map<string, CapInfo[]> {
  const groups = new Map<string, CapInfo[]>();
  for (const c of caps) {
    const key = capToPlugin(c.name);
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key)!.push(c);
  }
  return groups;
}

export function Caps() {
  const { caps, reachable, describes, ready } = useCaps();
  const [filter, setFilter] = useState("");
  const [onlyReachable, setOnlyReachable] = useState(false);

  const groups = useMemo(() => groupByPlugin(caps), [caps]);
  const filteredGroups = useMemo(() => {
    if (!filter && !onlyReachable) return Array.from(groups.entries());
    const f = filter.toLowerCase();
    return Array.from(groups.entries())
      .map(([plugin, items]) => {
        const filtered = items.filter((c) => {
          if (onlyReachable && !reachable.has(c.name)) return false;
          if (f && !c.name.toLowerCase().includes(f) && !plugin.toLowerCase().includes(f))
            return false;
          return true;
        });
        return filtered.length > 0 ? ([plugin, filtered] as const) : null;
      })
      .filter((x): x is readonly [string, CapInfo[]] => x !== null);
  }, [groups, filter, onlyReachable, reachable]);

  return (
    <div className="space-y-4">
      {/* Filter bar */}
      <div className="flex items-center gap-2">
        <div className="relative max-w-md flex-1">
          <Search className="text-muted-foreground absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2" />
          <Input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Filter capabilities or plugins…"
            className="pl-8"
            data-input="caps-filter"
          />
        </div>
        <Button
          variant={onlyReachable ? "default" : "outline"}
          size="sm"
          onClick={() => setOnlyReachable((v) => !v)}
          data-action="toggle-reachable"
        >
          <Filter className="h-3.5 w-3.5" />
          Reachable only
        </Button>
      </div>

      {!ready ? (
        <p className="text-muted-foreground text-xs">loading capabilities…</p>
      ) : filteredGroups.length === 0 ? (
        <p className="text-muted-foreground text-xs">no capabilities match the filter.</p>
      ) : (
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          {filteredGroups.map(([plugin, group]) => (
            <Card key={plugin} data-plugin={plugin}>
              <CardHeader className="border-border bg-muted/30 flex flex-row items-center justify-between space-y-0 border-b py-2">
                <span className="text-muted-foreground font-mono text-[11px] uppercase tracking-wider">
                  {plugin}
                </span>
                <Badge variant="muted">{group.length}</Badge>
              </CardHeader>
              <CardContent className="p-0">
                {group.map((cap) => {
                  const reachable_ = reachable.has(cap.name);
                  const d = describes.get(cap.name);
                  const ops = d && d.live ? d.operations : null;
                  return (
                    <Link
                      key={cap.name}
                      to={`/caps/${encodeURIComponent(cap.name)}`}
                      data-cap-row={cap.name}
                      className={cn(
                        "border-border/50 hover:bg-accent/50 flex flex-col gap-1.5 border-b px-3 py-2 transition-colors last:border-b-0",
                        !reachable_ && "opacity-60",
                      )}
                    >
                      {/* line 1: kind badge · cap name · timeout */}
                      <div className="flex min-w-0 items-center gap-2">
                        <Badge variant={cap.streaming ? "stream" : "muted"} className="shrink-0">
                          {cap.streaming ? "stream" : "sync"}
                        </Badge>
                        <code
                          className="text-foreground min-w-0 flex-1 truncate font-mono text-xs"
                          title={cap.name}
                        >
                          {cap.name}
                        </code>
                        <span className="text-muted-foreground shrink-0 font-mono text-[10px] tabular-nums">
                          {cap.timeout_ms}ms
                        </span>
                      </div>
                      {/* line 2: ops badges (or status) — wraps when long */}
                      <div className="flex flex-wrap items-center gap-1 pl-1">
                        {ops ? (
                          ops.map((op) => (
                            <Badge key={op} variant="muted" className="text-[9px]">
                              {op}
                            </Badge>
                          ))
                        ) : d && !d.live ? (
                          <Badge variant="destructive" className="text-[9px]">
                            revoked
                          </Badge>
                        ) : !reachable_ ? (
                          <Badge variant="outline" className="text-[9px]">
                            not in binding row
                          </Badge>
                        ) : null}
                      </div>
                    </Link>
                  );
                })}
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
