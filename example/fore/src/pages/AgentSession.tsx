// Session timeline (`/agent/:sessionId`). Replaces the middle
// column of the old Agent panel — now a full page so the
// timeline and resume form don't compete for vertical space.
//
// Drives the session by calling agent_resume after each Tick /
// ToolResult / UserReply observation. The kernel decides what
// next step to emit; this view just renders the history and
// the resume form.

import type { Observation } from "../api/types";

import { ArrowLeft, Send, Loader2, X, Save, Brain } from "lucide-react";
import { useState } from "react";
import { Link, useParams } from "react-router-dom";

import { MemoryPanel } from "../components/MemoryPanel";
import { StepRow } from "../components/StepRow";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "../components/ui/card";
import { ScrollArea } from "../components/ui/scroll-area";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../components/ui/tabs";
import { Textarea } from "../components/ui/textarea";
import { useAgentSession } from "../hooks/useAgentSession";
import { cn } from "../lib/utils";

type ObservationKind = "Tick" | "ToolResult" | "UserReply";

export function AgentSession() {
  const { sessionId } = useParams();
  const id = sessionId ?? "";
  const sessionStore = useAgentSession();
  const session = sessionStore.sessions.get(id) ?? null;

  const [obsKind, setObsKind] = useState<ObservationKind>("Tick");
  const [obsText, setObsText] = useState("");
  const [obsTool, setObsTool] = useState("");
  const [ckptPath, setCkptPath] = useState("");

  if (!session) {
    return (
      <div className="space-y-3">
        <Button asChild variant="ghost" size="sm">
          <Link to="/agent">
            <ArrowLeft className="h-3.5 w-3.5" /> back to sessions
          </Link>
        </Button>
        <Card>
          <CardContent className="text-muted-foreground p-6 text-sm">
            session <code>{id}</code> not in memory.
          </CardContent>
        </Card>
      </div>
    );
  }

  async function resume() {
    let observation: Observation;
    if (obsKind === "Tick") observation = { kind: "Tick" };
    else if (obsKind === "UserReply") observation = { kind: "UserReply", text: obsText };
    else observation = { kind: "ToolResult", tool: obsTool || "tool", value: obsText, error: null };
    await sessionStore.resume(id, observation);
  }

  const history = session.history;
  const canResume = !sessionStore.busy && session.status === "AwaitingObservation";

  return (
    <div className="space-y-4">
      {/* Header */}
      <div className="flex items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <Button asChild variant="ghost" size="sm">
            <Link to="/agent">
              <ArrowLeft className="h-3.5 w-3.5" /> sessions
            </Link>
          </Button>
          <code className="text-muted-foreground font-mono text-xs">{session.session_id}</code>
          <Badge variant={statusVariant(session.status)}>{session.status.toLowerCase()}</Badge>
        </div>
        <div className="flex items-center gap-2">
          <Badge variant="muted">{session.history_len} steps</Badge>
          <Button
            size="sm"
            variant="destructive"
            disabled={sessionStore.busy || session.status === "Cancelled"}
            onClick={() => sessionStore.cancel(id)}
            data-action="cancel"
          >
            <X className="h-3.5 w-3.5" /> cancel
          </Button>
        </div>
      </div>

      <Tabs defaultValue="timeline">
        <TabsList>
          <TabsTrigger value="timeline">Timeline</TabsTrigger>
          <TabsTrigger value="memory">Memory</TabsTrigger>
        </TabsList>

        <TabsContent value="timeline" className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle className="text-xs">Goal</CardTitle>
            </CardHeader>
            <CardContent>
              <p className="text-sm">{session.goal}</p>
              <div className="mt-2 flex flex-wrap gap-1">
                {session.allowed_tools.map((t) => (
                  <Badge key={t} variant="muted">
                    {t}
                  </Badge>
                ))}
              </div>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="text-xs">History</CardTitle>
            </CardHeader>
            <CardContent className="p-0">
              <ScrollArea className="max-h-[480px]">
                {history.length === 0 ? (
                  <p className="text-muted-foreground p-4 text-xs">
                    no history yet — send the first observation below.
                  </p>
                ) : (
                  <ol>
                    {history.map((step, idx) => (
                      <li key={idx} className="border-border/50 border-b px-4 py-2 last:border-b-0">
                        <StepRow step={step} />
                      </li>
                    ))}
                  </ol>
                )}
              </ScrollArea>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="text-xs">Resume</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              <div className="flex gap-1">
                {(["Tick", "ToolResult", "UserReply"] as ObservationKind[]).map((k) => (
                  <button
                    key={k}
                    type="button"
                    onClick={() => setObsKind(k)}
                    className={cn(
                      "rounded-md px-2.5 py-1 font-mono text-[11px] uppercase tracking-wide transition-colors",
                      obsKind === k
                        ? "bg-primary/15 text-foreground"
                        : "bg-muted text-muted-foreground hover:bg-accent",
                    )}
                    data-obs-kind={k}
                  >
                    {k}
                  </button>
                ))}
              </div>
              {obsKind === "ToolResult" && (
                <Textarea
                  rows={2}
                  placeholder='tool name (e.g. "echo")'
                  value={obsTool}
                  onChange={(e) => setObsTool(e.target.value)}
                />
              )}
              {obsKind !== "Tick" && (
                <Textarea
                  rows={3}
                  placeholder={
                    obsKind === "UserReply" ? "user reply…" : "tool value (JSON or text)"
                  }
                  value={obsText}
                  onChange={(e) => setObsText(e.target.value)}
                />
              )}
              <Button
                size="sm"
                variant="success"
                onClick={resume}
                disabled={!canResume}
                data-action="resume"
              >
                {sessionStore.busy ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Send className="h-3.5 w-3.5" />
                )}
                Send observation
              </Button>
            </CardContent>
          </Card>

          <Card className="border-warning/40">
            <CardHeader>
              <CardTitle className="text-warning flex items-center gap-2 text-xs">
                <Save className="h-3.5 w-3.5" /> Checkpoint
              </CardTitle>
            </CardHeader>
            <CardContent className="flex items-center gap-2">
              <Textarea
                rows={1}
                placeholder="/path/to/checkpoint.json"
                value={ckptPath}
                onChange={(e) => setCkptPath(e.target.value)}
                className="min-h-[32px]"
                data-input="checkpoint-path"
              />
              <Button
                size="sm"
                variant="warning"
                disabled={!ckptPath || sessionStore.busy}
                onClick={() => sessionStore.cancel(id, ckptPath)}
                data-action="save-checkpoint"
              >
                <Save className="h-3.5 w-3.5" /> save &amp; cancel
              </Button>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="memory">
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2 text-xs">
                <Brain className="h-3.5 w-3.5" /> Memory
              </CardTitle>
            </CardHeader>
            <CardContent>
              <MemoryPanel />
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  );
}

function statusVariant(status: string): "success" | "warning" | "muted" | "destructive" {
  switch (status) {
    case "Running":
      return "success";
    case "AwaitingObservation":
      return "warning";
    case "Done":
      return "muted";
    case "Failed":
    case "Cancelled":
      return "destructive";
    default:
      return "muted";
  }
}
