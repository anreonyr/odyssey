// Capability detail (`/caps/:name`). Replaces the old
// `CapabilityDrawer`: now a full page instead of a side panel,
// so the JSON inspector has room to breathe and deep-links
// work.
//
// Shows cap meta (from `/api/caps`), reachability + rights
// (from the agent's `describe`), and a live invoke form.

import { ArrowLeft, Play, Copy } from "lucide-react";
import { useMemo, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";

import { client, BridgeError } from "../api/client";
import { JsonEditor } from "../components/JsonEditor";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";
import { ScrollArea } from "../components/ui/scroll-area";
import { Separator } from "../components/ui/separator";
import { useCaps } from "../hooks/useCaps";
import { cn } from "../lib/utils";

export function CapDetail() {
  const params = useParams();
  const navigate = useNavigate();
  const name = decodeURIComponent(params.name ?? "");
  const { caps, reachable, describes, ready, agentError } = useCaps();
  const cap = useMemo(() => caps.find((c) => c.name === name), [caps, name]);

  const [input, setInput] = useState<string>("{}");
  const [output, setOutput] = useState<unknown>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  if (!ready) return <p className="text-muted-foreground text-xs">loading…</p>;

  if (!cap) {
    return (
      <div className="space-y-3">
        <Button asChild variant="ghost" size="sm">
          <Link to="/caps">
            <ArrowLeft className="h-3.5 w-3.5" /> back
          </Link>
        </Button>
        <Card>
          <CardContent className="text-muted-foreground p-6 text-sm">
            capability <code>{name}</code> not found.
          </CardContent>
        </Card>
      </div>
    );
  }

  const cap_ = cap;
  const isReachable = reachable.has(cap_.name);
  const describe = describes.get(cap.name);
  const ops = describe && describe.live ? describe.operations : null;

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
      const value = await client.invoke(cap_.name, parsed);
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
      {/* Breadcrumb */}
      <div className="flex items-center gap-2 text-xs">
        <Button asChild variant="ghost" size="sm">
          <Link to="/caps">
            <ArrowLeft className="h-3.5 w-3.5" /> capabilities
          </Link>
        </Button>
        <span className="text-muted-foreground">/</span>
        <code className="text-foreground font-mono">{cap.name}</code>
      </div>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-[320px,1fr]">
        {/* Left: meta */}
        <div className="space-y-4">
          <Card>
            <CardHeader>
              <div className="flex items-start justify-between gap-2">
                <CardTitle className="min-w-0 flex-1 truncate font-mono" title={cap.name}>
                  {cap.name}
                </CardTitle>
                <Badge variant={cap.streaming ? "stream" : "muted"} className="shrink-0">
                  {cap.streaming ? "stream" : "sync"}
                </Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-3 text-xs">
              <Meta label="id" value={cap.id} mono />
              <Meta label="plugin" value={capToPlugin(cap.name)} mono />
              <Meta label="timeout" value={`${cap.timeout_ms} ms`} mono />
              <Separator />
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="text-xs">Reachability</CardTitle>
            </CardHeader>
            <CardContent className="space-y-2 text-xs">
              {agentError ? (
                <p className="text-destructive">agent unavailable: {agentError}</p>
              ) : describe && !describe.live ? (
                <Badge variant="destructive">revoked</Badge>
              ) : !isReachable ? (
                <Badge variant="outline">not in binding row</Badge>
              ) : (
                <>
                  <p className="text-muted-foreground">in agent's binding row</p>
                  {ops ? (
                    <div className="flex flex-wrap gap-1">
                      {ops.map((op) => (
                        <Badge key={op} variant="muted">
                          {op}
                        </Badge>
                      ))}
                    </div>
                  ) : null}
                </>
              )}
            </CardContent>
          </Card>
        </div>

        {/* Right: invoke */}
        <Card>
          <CardHeader className="flex flex-row items-center justify-between space-y-0">
            <CardTitle className="text-xs">Invoke</CardTitle>
            <div className="flex items-center gap-2">
              <Badge variant="muted" className="font-mono">
                /api/{cap.streaming ? "stream" : "invoke"}
              </Badge>
              <Button
                size="sm"
                variant="success"
                onClick={run}
                disabled={running}
                data-action="invoke"
              >
                <Play className="h-3.5 w-3.5" />
                {running ? "running…" : "run"}
              </Button>
            </div>
          </CardHeader>
          <CardContent className="space-y-3">
            <div>
              <label className="text-muted-foreground mb-2 block font-mono text-[10px] uppercase tracking-wider">
                input (JSON)
              </label>
              <JsonEditor value={input} onChange={setInput} rows={8} />
            </div>

            <Separator />

            <div>
              <label className="text-muted-foreground mb-2 block font-mono text-[10px] uppercase tracking-wider">
                result
              </label>
              {error ? (
                <pre className="border-destructive/40 bg-destructive/10 text-destructive rounded-md border p-3 font-mono text-xs">
                  {error}
                </pre>
              ) : output == null ? (
                <p className="text-muted-foreground font-mono text-xs">
                  {running ? "running…" : "(no result yet)"}
                </p>
              ) : (
                <ScrollArea className="border-border bg-muted/20 max-h-[400px] rounded-md border">
                  <pre className="p-3 font-mono text-xs">{JSON.stringify(output, null, 2)}</pre>
                </ScrollArea>
              )}
            </div>

            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                navigator.clipboard?.writeText(JSON.stringify({ name: cap.name, input }));
                navigate("/invoke", { state: { cap: cap.name, input } });
              }}
            >
              <Copy className="h-3.5 w-3.5" /> open in raw invoke
            </Button>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

function Meta({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="flex items-baseline justify-between gap-3">
      <span className="text-muted-foreground shrink-0 font-mono text-[10px] uppercase tracking-wider">
        {label}
      </span>
      <span
        className={cn(
          "min-w-0 truncate text-right",
          mono ? "text-foreground font-mono" : "text-foreground",
        )}
        title={value}
      >
        {value}
      </span>
    </div>
  );
}

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
