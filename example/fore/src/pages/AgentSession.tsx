// Session timeline (`/agent/sessions/:sessionId`). Replaces the
// middle column of the old Agent panel — now a full page so the
// timeline and resume form don't compete for vertical space.
//
// Drives the session by calling agent_resume after each Tick /
// ToolResult / UserReply observation. The kernel decides what
// next step to emit; this view just renders the history and
// the resume form.

import type { Observation } from "@/api/types";

import { ArrowLeft, Brain, FolderOpen, Loader2, Save, Send, X } from "lucide-react";
import { useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";

import { MemoryPanel } from "@/components/MemoryPanel";
import { StepRow } from "@/components/StepRow";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { useAgentSession } from "@/hooks/useAgentSession";
import { cn } from "@/lib/utils";

type ObservationKind = "Tick" | "ToolResult" | "UserReply";

export function AgentSession() {
  const { sessionId } = useParams();
  const id = sessionId ?? "";
  const sessionStore = useAgentSession();
  const session = sessionStore.sessions.get(id) ?? null;
  const navigate = useNavigate();

  const [obsKind, setObsKind] = useState<ObservationKind>("Tick");
  const [obsText, setObsText] = useState("");
  const [obsTool, setObsTool] = useState("");
  const [ckptPath, setCkptPath] = useState("");
  const [loadPath, setLoadPath] = useState("");

  if (!session) {
    return (
      <div className="space-y-3">
        <Button asChild variant="ghost" size="sm">
          <Link to="/agent/sessions">
            <ArrowLeft className="h-4 w-4" /> Back to sessions
          </Link>
        </Button>
        <Card>
          <CardContent className="text-muted-foreground p-6 text-sm">
            Session <code>{id}</code> not in memory.
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
      <div className="flex items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <Button asChild variant="ghost" size="sm">
            <Link to="/agent/sessions">
              <ArrowLeft className="h-4 w-4" /> Sessions
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
            <X className="h-4 w-4" /> Cancel
          </Button>
        </div>
      </div>

      <Tabs defaultValue="timeline">
        <TabsList>
          <TabsTrigger value="timeline">Timeline</TabsTrigger>
          <TabsTrigger value="memory">Memory</TabsTrigger>
          <TabsTrigger value="checkpoint">Checkpoint</TabsTrigger>
        </TabsList>

        <TabsContent value="timeline" className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle className="text-sm font-medium">Goal</CardTitle>
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
              <CardTitle className="text-sm font-medium">History</CardTitle>
            </CardHeader>
            <CardContent className="p-0">
              <ScrollArea className="max-h-[480px]">
                {history.length === 0 ? (
                  <p className="text-muted-foreground p-4 text-sm">
                    No history yet — send the first observation below.
                  </p>
                ) : (
                  <ol className="divide-y">
                    {history.map((step, idx) => (
                      <li key={idx} className="px-4 py-3">
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
              <CardTitle className="text-sm font-medium">Resume</CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              <div className="flex gap-1">
                {(["Tick", "ToolResult", "UserReply"] as ObservationKind[]).map((k) => (
                  <button
                    key={k}
                    type="button"
                    onClick={() => setObsKind(k)}
                    className={cn(
                      "rounded-md px-3 py-1 text-sm transition-colors",
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
                <Input
                  placeholder='Tool name (e.g. "echo")'
                  value={obsTool}
                  onChange={(e) => setObsTool(e.target.value)}
                />
              )}
              {obsKind !== "Tick" && (
                <Textarea
                  rows={3}
                  placeholder={
                    obsKind === "UserReply" ? "User reply…" : "Tool value (JSON or text)"
                  }
                  value={obsText}
                  onChange={(e) => setObsText(e.target.value)}
                />
              )}
              <Button onClick={resume} disabled={!canResume} data-action="resume">
                {sessionStore.busy ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : (
                  <Send className="h-4 w-4" />
                )}
                Send observation
              </Button>
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="memory">
          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2 text-sm font-medium">
                <Brain className="h-4 w-4" /> Memory
              </CardTitle>
            </CardHeader>
            <CardContent>
              <MemoryPanel />
            </CardContent>
          </Card>
        </TabsContent>

        <TabsContent value="checkpoint" className="space-y-4">
          <Card className="border-warning/40">
            <CardHeader>
              <CardTitle className="text-warning flex items-center gap-2 text-sm font-medium">
                <Save className="h-4 w-4" /> Save & cancel
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              <div>
                <Label htmlFor="ckpt-path">Checkpoint path</Label>
                <Input
                  id="ckpt-path"
                  placeholder="/path/to/checkpoint.json"
                  value={ckptPath}
                  onChange={(e) => setCkptPath(e.target.value)}
                  data-input="checkpoint-path"
                />
              </div>
              <Button
                variant="default"
                disabled={!ckptPath || sessionStore.busy}
                onClick={() => sessionStore.cancel(id, ckptPath)}
                data-action="save-checkpoint"
              >
                <Save className="h-4 w-4" /> Save & cancel
              </Button>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle className="flex items-center gap-2 text-sm font-medium">
                <FolderOpen className="h-4 w-4" /> Load checkpoint
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-3">
              <div>
                <Label htmlFor="load-path">Path</Label>
                <Input
                  id="load-path"
                  placeholder="/path/to/checkpoint.json"
                  value={loadPath}
                  onChange={(e) => setLoadPath(e.target.value)}
                  data-input="load-path"
                />
              </div>
              <Button
                onClick={async () => {
                  const newId = await sessionStore.load(loadPath);
                  setLoadPath("");
                  navigate(`/agent/sessions/${newId}`);
                }}
                data-action="load-checkpoint"
              >
                Load
              </Button>
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
