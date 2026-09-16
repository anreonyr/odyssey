// Raw invoke playground (`/invoke`). No agent in the loop —
// pick a capability, paste JSON, see the result. The same
// surface that backs the capability detail page, exposed as a
// standalone utility.
//
// Pre-fills from the route state if the user clicks "open in
// raw invoke" from the detail page.

import type { CapInfo } from "../api/types";

import { Play, Loader2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useLocation } from "react-router-dom";

import { client, BridgeError } from "../api/client";
import { JsonEditor } from "../components/JsonEditor";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";
import { Input } from "../components/ui/input";
import { ScrollArea } from "../components/ui/scroll-area";
import { useCaps } from "../hooks/useCaps";

interface RouteState {
  cap?: string;
  input?: string;
}

export function Invoke() {
  const { caps, ready } = useCaps();
  const location = useLocation();
  const state = (location.state as RouteState | null) ?? null;

  const [selectedCap, setSelectedCap] = useState<string>("");
  const [filter, setFilter] = useState("");
  const [input, setInput] = useState<string>("{}");
  const [output, setOutput] = useState<unknown>(null);
  const [error, setError] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  // Pre-fill from route state when navigating from CapDetail.
  useEffect(() => {
    if (state?.cap) setSelectedCap(state.cap);
    if (state?.input) setInput(state.input);
  }, [state?.cap, state?.input]);

  const filtered = useMemo(
    () => caps.filter((c) => !filter || c.name.toLowerCase().includes(filter.toLowerCase())),
    [caps, filter],
  );

  const cap: CapInfo | undefined = caps.find((c) => c.name === selectedCap);

  async function run() {
    if (!cap) return;
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
    <div className="grid grid-cols-1 gap-4 lg:grid-cols-[260px,1fr]">
      <Card>
        <CardHeader>
          <CardTitle className="text-xs">Capabilities</CardTitle>
          <Input
            placeholder="filter…"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            className="mt-2"
          />
        </CardHeader>
        <CardContent className="p-0">
          {!ready ? (
            <p className="text-muted-foreground p-3 text-xs">loading…</p>
          ) : (
            <ul className="divide-border/50 divide-y">
              {filtered.map((c) => (
                <li key={c.name}>
                  <button
                    type="button"
                    onClick={() => setSelectedCap(c.name)}
                    data-cap-name={c.name}
                    className={
                      "hover:bg-accent/50 flex w-full items-center gap-2 px-3 py-2 text-left text-xs " +
                      (selectedCap === c.name ? "bg-accent" : "")
                    }
                  >
                    <Badge variant={c.streaming ? "stream" : "muted"} className="text-[9px]">
                      {c.streaming ? "s" : "·"}
                    </Badge>
                    <code className="font-mono">{c.name}</code>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex flex-row items-center justify-between space-y-0">
          <div className="flex items-center gap-2">
            <CardTitle className="text-xs">
              {cap ? <code className="font-mono">{cap.name}</code> : "Pick a capability"}
            </CardTitle>
            {cap && (
              <Badge variant="muted" className="font-mono">
                {cap.streaming ? "/api/stream" : "/api/invoke"}
              </Badge>
            )}
          </div>
          <Button
            size="sm"
            variant="success"
            onClick={run}
            disabled={!cap || running}
            data-action="invoke"
          >
            {running ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Play className="h-3.5 w-3.5" />
            )}
            run
          </Button>
        </CardHeader>
        <CardContent className="space-y-3">
          <div>
            <label className="text-muted-foreground mb-2 block font-mono text-[10px] uppercase tracking-wider">
              input (JSON)
            </label>
            <JsonEditor value={input} onChange={setInput} rows={10} />
          </div>
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
                {running ? "running…" : cap ? "(no result yet)" : "(pick a capability on the left)"}
              </p>
            ) : (
              <ScrollArea className="border-border bg-muted/20 max-h-[480px] rounded-md border">
                <pre className="p-3 font-mono text-xs">{JSON.stringify(output, null, 2)}</pre>
              </ScrollArea>
            )}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
