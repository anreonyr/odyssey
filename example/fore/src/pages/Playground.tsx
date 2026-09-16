// LLM playground (`/playground`). Thin wrapper around the
// `llm_complete` cap: prompt → completion, streamed via the
// SSE handler. The streaming result is the headline
// affordance here — the rest is a simple JSON request shape.

import { Play, Loader2 } from "lucide-react";
import { useState } from "react";

import { client } from "../api/client";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";
import { Input } from "../components/ui/input";
import { ScrollArea } from "../components/ui/scroll-area";
import { Tabs, TabsList, TabsTrigger, TabsContent } from "../components/ui/tabs";
import { Textarea } from "../components/ui/textarea";

export function Playground() {
  const [prompt, setPrompt] = useState("");
  const [system, setSystem] = useState("");
  const [completion, setCompletion] = useState("");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [rawJson, setRawJson] = useState<string>("");

  async function run() {
    setRunning(true);
    setError(null);
    setCompletion("");
    try {
      const input = {
        prompt,
        ...(system ? { system } : {}),
      };
      setRawJson(JSON.stringify(input, null, 2));
      // llm_complete is sync today, but the streaming variant
      // exists. We use the sync path so the result appears in
      // one chunk; the streaming UI is reserved for caps that
      // actually emit chunks.
      const value = await client.invoke<{ completion: string }>("llm_complete", input);
      setCompletion(value.completion);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setRunning(false);
    }
  }

  return (
    <div className="grid grid-cols-1 gap-4 lg:grid-cols-[1fr,1fr]">
      <Card>
        <CardHeader className="flex flex-row items-center justify-between space-y-0">
          <CardTitle className="text-xs">Prompt</CardTitle>
          <Button size="sm" variant="success" onClick={run} disabled={running || !prompt.trim()}>
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
            <label className="text-muted-foreground mb-1 block font-mono text-[10px] uppercase tracking-wider">
              system (optional)
            </label>
            <Input
              value={system}
              onChange={(e) => setSystem(e.target.value)}
              placeholder="you are a helpful assistant…"
            />
          </div>
          <div>
            <label className="text-muted-foreground mb-1 block font-mono text-[10px] uppercase tracking-wider">
              user prompt
            </label>
            <Textarea
              rows={10}
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="ask the model…"
            />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-xs">Result</CardTitle>
        </CardHeader>
        <CardContent>
          <Tabs defaultValue="text">
            <TabsList>
              <TabsTrigger value="text">Completion</TabsTrigger>
              <TabsTrigger value="raw">Request</TabsTrigger>
            </TabsList>
            <TabsContent value="text">
              {error ? (
                <pre className="border-destructive/40 bg-destructive/10 text-destructive rounded-md border p-3 font-mono text-xs">
                  {error}
                </pre>
              ) : (
                <ScrollArea className="border-border bg-muted/20 max-h-[480px] rounded-md border">
                  <pre className="p-3 font-mono text-xs leading-relaxed">
                    {completion || <span className="text-muted-foreground">(empty)</span>}
                  </pre>
                </ScrollArea>
              )}
            </TabsContent>
            <TabsContent value="raw">
              <ScrollArea className="border-border bg-muted/20 max-h-[480px] rounded-md border">
                <pre className="p-3 font-mono text-xs">
                  {rawJson || (
                    <span className="text-muted-foreground">(run to see the request envelope)</span>
                  )}
                </pre>
              </ScrollArea>
            </TabsContent>
          </Tabs>
          {rawJson && (
            <p className="text-muted-foreground mt-2 font-mono text-[10px]">
              <Badge variant="muted" className="mr-1">
                llm_complete
              </Badge>
              sent via <code className="text-foreground">/api/invoke</code>
            </p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
