// Capabilities tab — plugin-grouped list with a reachable-only
// toggle and an inspector panel on the right (Input JSON, Run,
// Result).
//
// The `capToPlugin` mapping is hand-maintained — the cspace
// doesn't carry a plugin field on each cap. The fallback
// `capToPlugin(name) = name` keeps an unmapped cap visible
// in its own group rather than dropping it silently.

import type { CapInfo } from "@/api/types";

import { Play } from "lucide-react";
import { useMemo, useState } from "react";

import { BridgeError, client } from "@/api/client";
import { JsonEditor } from "@/components/JsonEditor";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useCaps } from "@/hooks/useCaps";
import { cn } from "@/lib/utils";

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

export function CapabilitiesTab() {
  const { caps, reachable, describes, ready } = useCaps();
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<string | null>(null);

  const filtered = useMemo(() => {
    const f = filter.toLowerCase();
    return caps.filter((c) => {
      if (f && !c.name.toLowerCase().includes(f) && !capToPlugin(c.name).toLowerCase().includes(f))
        return false;
      return true;
    });
  }, [caps, filter]);

  const groups = useMemo(() => {
    const map = new Map<string, CapInfo[]>();
    for (const c of filtered) {
      const k = capToPlugin(c.name);
      if (!map.has(k)) map.set(k, []);
      map.get(k)!.push(c);
    }
    return Array.from(map.entries());
  }, [filtered]);

  const sel = selected ? caps.find((c) => c.name === selected) : null;

  return (
    // The two cards share the viewport-bottom space; capping
    // their height to `100vh - <header+padding>` lets the
    // ScrollArea inside the left card actually scroll when
    // the cap list grows past the viewport, instead of
    // overflowing into the parent `<main>` and pushing the
    // right card off-screen.
    <div className="grid h-[calc(100vh-8rem)] grid-cols-1 gap-4 lg:grid-cols-[420px,1fr]">
      <Card className="flex min-h-0 flex-col">
        <CardHeader className="space-y-3">
          <div className="flex items-center gap-2">
            <Input
              value={filter}
              onChange={(e) => setFilter(e.target.value)}
              placeholder="Filter capabilities…"
              data-input="caps-filter"
            />
            <Button
              variant="outline"
              size="sm"
              disabled
              data-action="toggle-binding-row"
            >
              Binding row only
            </Button>
          </div>
        </CardHeader>
        <CardContent className="min-h-0 flex-1 overflow-hidden p-0">
          {!ready ? (
            <p className="text-muted-foreground p-4 text-sm">Loading capabilities…</p>
          ) : groups.length === 0 ? (
            <p className="text-muted-foreground p-4 text-sm">No capabilities match the filter.</p>
          ) : (
            <ScrollArea className="h-full">
              {groups.map(([plugin, group]) => (
                <div key={plugin} className="border-t" data-plugin={plugin}>
                  <div className="bg-muted/30 flex items-center justify-between px-3 py-2">
                    <span className="text-muted-foreground text-xs">{plugin}</span>
                    <Badge variant="muted">{group.length}</Badge>
                  </div>
                  <ul className="divide-y">
                    {group.map((cap) => {
                      const r = reachable.get(cap.name);
                      const d = describes.get(cap.name);
                      const ops = d && d.live ? d.operations : null;
                      return (
                        <li key={cap.name}>
                          <button
                            type="button"
                            onClick={() => setSelected(cap.name)}
                            data-cap-row={cap.name}
                            className={cn(
                              "hover:bg-accent/50 flex w-full flex-col gap-1 px-3 py-2 text-left text-sm transition-colors",
                              !r && "opacity-60",
                              selected === cap.name && "bg-accent",
                            )}
                          >
                            <div className="flex items-center gap-2">
                              <Badge
                                variant={cap.streaming ? "warning" : "muted"}
                                className="shrink-0"
                              >
                                {cap.streaming ? "stream" : "sync"}
                              </Badge>
                              <code className="flex-1 truncate font-mono">{cap.name}</code>
                              <span className="text-muted-foreground font-mono text-xs tabular-nums">
                                {cap.timeout_ms}ms
                              </span>
                            </div>
                            <div className="flex flex-wrap gap-1 pl-1">
                              {ops ? (
                                ops.map((op) => (
                                  <Badge key={op} variant="muted" className="text-[10px]">
                                    {op}
                                  </Badge>
                                ))
                              ) : d && !d.live ? (
                                <Badge variant="destructive" className="text-[10px]">
                                  revoked
                                </Badge>
                              ) : !r ? (
                                <Badge variant="outline" className="text-[10px]">
                                  not in cspace
                                </Badge>
                              ) : (
                                <Badge variant="outline" className="text-[10px]">
                                  via cspace lookup
                                </Badge>
                              )}
                            </div>
                          </button>
                        </li>
                      );
                    })}
                  </ul>
                </div>
              ))}
            </ScrollArea>
          )}
        </CardContent>
      </Card>

      <Card className="flex min-h-0 flex-col overflow-hidden">
        <CardHeader>
          {sel ? (
            <div className="flex items-center gap-2">
              <code className="font-mono text-base">{sel.name}</code>
              <Badge variant={sel.streaming ? "warning" : "muted"}>
                {sel.streaming ? "stream" : "sync"}
              </Badge>
            </div>
          ) : (
            <span className="text-muted-foreground text-sm">Pick a capability on the left</span>
          )}
        </CardHeader>
        <CardContent className="min-h-0 flex-1 overflow-auto">
          {sel ? <CapInspector cap={sel} /> : null}
        </CardContent>
      </Card>
    </div>
  );
}

function CapInspector({ cap }: { cap: CapInfo }) {
  const [input, setInput] = useState<string>("{}");
  const [output, setOutput] = useState<unknown>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  async function run() {
    let parsed: unknown;
    try {
      parsed = input.trim() ? JSON.parse(input) : {};
    } catch (e) {
      setError(`input is not valid JSON: ${(e as Error).message}`);
      return;
    }
    setRunning(true);
    setError(null);
    setOutput(null);
    try {
      const value = await client.invoke(cap.name, parsed);
      setOutput(value);
    } catch (e) {
      if (e instanceof BridgeError) setError(e.payload.error);
      else setError((e as Error).message ?? String(e));
    } finally {
      setRunning(false);
    }
  }

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-3 text-sm">
        <div>
          <div className="text-muted-foreground text-xs">Plugin</div>
          <div className="font-mono">{capToPlugin(cap.name)}</div>
        </div>
        <div>
          <div className="text-muted-foreground text-xs">Timeout</div>
          <div className="font-mono">{cap.timeout_ms} ms</div>
        </div>
        <div className="col-span-2">
          <div className="text-muted-foreground text-xs">ID</div>
          <div className="font-mono text-xs">{cap.id}</div>
        </div>
      </div>

      <div>
        <div className="mb-2 flex items-center justify-between">
          <span className="text-muted-foreground text-xs">Input (JSON)</span>
          <Button size="sm" onClick={run} disabled={running} data-action="invoke">
            <Play className="h-4 w-4" />
            {running ? "Running…" : "Run"}
          </Button>
        </div>
        <JsonEditor value={input} onChange={setInput} rows={6} />
      </div>

      <div>
        <div className="text-muted-foreground mb-2 text-xs">Result</div>
        {error ? (
          <pre className="border-destructive/40 bg-destructive/10 text-destructive rounded-md border p-3 font-mono text-xs">
            {error}
          </pre>
        ) : output == null ? (
          <p className="text-muted-foreground text-xs">
            {running ? "Running…" : "(no result yet)"}
          </p>
        ) : (
          <ScrollArea className="bg-muted/30 max-h-[400px] rounded-md border">
            <pre className="p-3 font-mono text-xs">{JSON.stringify(output, null, 2)}</pre>
          </ScrollArea>
        )}
      </div>
    </div>
  );
}
