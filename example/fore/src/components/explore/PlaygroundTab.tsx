// Playground tab — wraps llm_complete with a prompt/completion
// UI. The system prompt is optional; the request envelope is
// shown verbatim in the "Request" tab so users can copy it.

import { Loader2, Play } from "lucide-react";
import { useState } from "react";

import { client } from "@/api/client";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";

export function PlaygroundTab() {
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
      const input = { prompt, ...(system ? { system } : {}) };
      setRawJson(JSON.stringify(input, null, 2));
      const value = await client.invoke<{ completion: string }>("llm_complete", input);
      setCompletion(value.completion);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setRunning(false);
    }
  }

  return (
    <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
      <Card>
        <CardHeader className="flex flex-row items-center justify-between space-y-0">
          <CardTitle className="text-sm font-medium">Prompt</CardTitle>
          <Button size="sm" onClick={run} disabled={running || !prompt.trim()}>
            {running ? <Loader2 className="h-4 w-4 animate-spin" /> : <Play className="h-4 w-4" />}
            Run
          </Button>
        </CardHeader>
        <CardContent className="space-y-3">
          <div>
            <div className="text-muted-foreground mb-2 text-xs">System (optional)</div>
            <Input
              value={system}
              onChange={(e) => setSystem(e.target.value)}
              placeholder="You are a helpful assistant…"
            />
          </div>
          <div>
            <div className="text-muted-foreground mb-2 text-xs">User prompt</div>
            <Textarea
              rows={10}
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="Ask the model…"
            />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm font-medium">Result</CardTitle>
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
                <ScrollArea className="bg-muted/30 max-h-[480px] rounded-md border">
                  <pre className="p-3 font-mono text-xs leading-relaxed">
                    {completion || <span className="text-muted-foreground">(empty)</span>}
                  </pre>
                </ScrollArea>
              )}
            </TabsContent>
            <TabsContent value="raw">
              <ScrollArea className="bg-muted/30 max-h-[480px] rounded-md border">
                <pre className="p-3 font-mono text-xs">
                  {rawJson || (
                    <span className="text-muted-foreground">(run to see the request envelope)</span>
                  )}
                </pre>
              </ScrollArea>
            </TabsContent>
          </Tabs>
          {rawJson && (
            <p className="text-muted-foreground mt-2 text-xs">
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
