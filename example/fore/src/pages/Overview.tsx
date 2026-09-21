// Overview dashboard — the `/` route. One-glance view of
// the cspace + agent state, plus shortcut cards to the
// other pages.
//
// Data comes from the same hooks every other page uses; the
// sidebar counts and the cards here stay in sync without
// any extra fetching.

import { Boxes, Bot, Save, Sparkles, Terminal, Activity, Cpu } from "lucide-react";
import { Link } from "react-router-dom";

import { MemoryPanel } from "../components/MemoryPanel";
import { Badge } from "../components/ui/badge";
import { Button } from "../components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../components/ui/card";
import { Skeleton } from "../components/ui/skeleton";
import { useAgentSession } from "../hooks/useAgentSession";
import { useCaps } from "../hooks/useCaps";
import { useCheckpoints } from "../hooks/useCheckpoints";
import { cn } from "../lib/utils";

export function Overview() {
  const { caps, reachable, describes, ready, agentError } = useCaps();
  const { sessions } = useAgentSession();
  const { checkpoints, loading: ckptsLoading } = useCheckpoints();

  const liveSessions = Array.from(sessions.values()).filter(
    (s) => s.status === "Running" || s.status === "AwaitingObservation",
  );

  return (
    <div className="space-y-6">
      {/* Top stats row */}
      <section aria-label="system status">
        <h2 className="text-muted-foreground mb-3 font-mono text-xs uppercase tracking-wider">
          System status
        </h2>
        {!ready ? (
          <div className="grid grid-cols-4 gap-4">
            {Array.from({ length: 4 }).map((_, i) => (
              <Skeleton key={i} className="h-24" />
            ))}
          </div>
        ) : (
          <div className="grid grid-cols-2 gap-4 md:grid-cols-4">
            <Stat icon={Cpu} label="Capabilities mounted" value={caps.length} tone="default" />
            <Stat
              icon={Activity}
              label="Reachable via agent"
              value={reachable.size}
              tone={reachable.size === caps.length ? "success" : "warning"}
            />
            <Stat
              icon={Bot}
              label="Active sessions"
              value={liveSessions.length}
              tone={liveSessions.length > 0 ? "success" : "muted"}
            />
            <Stat
              icon={Save}
              label="Saved checkpoints"
              value={ckptsLoading ? "…" : checkpoints.length}
              tone="muted"
            />
          </div>
        )}
      </section>

      {/* Quick links */}
      <section aria-label="quick links">
        <h2 className="text-muted-foreground mb-3 font-mono text-xs uppercase tracking-wider">
          Jump to
        </h2>
        <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
          <QuickCard
            to="/caps"
            icon={Boxes}
            title="Capabilities"
            description="Browse every cap in the cspace; click a row to invoke or inspect."
            count={caps.length}
          />
          <QuickCard
            to="/agent"
            icon={Bot}
            title="Agent sessions"
            description="Start a session, drive it step by step, inspect the timeline."
            count={sessions.size}
          />
          <QuickCard
            to="/checkpoints"
            icon={Save}
            title="Checkpoints"
            description="Pick up where a cancelled session left off."
            count={checkpoints.length}
          />
          <QuickCard
            to="/invoke"
            icon={Terminal}
            title="Invoke"
            description="Raw capability dispatch — pick a cap, send JSON, see the result."
          />
          <QuickCard
            to="/playground"
            icon={Sparkles}
            title="Playground"
            description="LLM playground: prompt → completion, streamed."
          />
          <div className="hidden md:block" />
        </div>
      </section>

      {/* Memory */}
      <section aria-label="memory">
        <h2 className="text-muted-foreground mb-3 font-mono text-xs uppercase tracking-wider">
          Memory
        </h2>
        <Card>
          <CardContent className="p-4">
            <MemoryPanel />
          </CardContent>
        </Card>
      </section>

      {/* Agent binding summary */}
      {ready && (
        <section aria-label="agent binding row">
          <h2 className="text-muted-foreground mb-3 font-mono text-xs uppercase tracking-wider">
            Agent binding row
          </h2>
          <Card>
            <CardHeader>
              <CardTitle>What the agent can reach</CardTitle>
              <CardDescription>
                The four reachable handles — echo, reverse, database, streaming_echo — carry all
                four rights. Everything else in the cspace is registered but not in the binding row.
              </CardDescription>
            </CardHeader>
            <CardContent>
              {agentError ? (
                <p className="text-destructive text-xs">Agent unavailable: {agentError}</p>
              ) : (
                <ul className="grid grid-cols-1 gap-2 md:grid-cols-2">
                  {Array.from(reachable.values()).map((h) => {
                    const d = describes.get(h.handle);
                    const ops = d && d.live ? d.operations : null;
                    return (
                      <li
                        key={h.handle}
                        className="border-border bg-card flex items-center justify-between rounded-md border px-3 py-2"
                      >
                        <span className="font-mono text-xs">{h.handle}</span>
                        <div className="flex items-center gap-1">
                          {ops ? (
                            ops.map((op) => (
                              <Badge key={op} variant="muted">
                                {op}
                              </Badge>
                            ))
                          ) : (
                            <Badge variant="destructive">revoked</Badge>
                          )}
                        </div>
                      </li>
                    );
                  })}
                </ul>
              )}
            </CardContent>
          </Card>
        </section>
      )}
    </div>
  );
}

interface StatProps {
  icon: React.ComponentType<{ className?: string | undefined }>;
  label: string;
  value: number | string;
  tone: "default" | "success" | "warning" | "muted";
}

function Stat({ icon: Icon, label, value, tone }: StatProps) {
  const toneClass = {
    default: "text-foreground",
    success: "text-success",
    warning: "text-warning",
    muted: "text-muted-foreground",
  }[tone];
  return (
    <Card>
      <CardContent className="flex items-center gap-3 p-4">
        <Icon className={cn("h-4 w-4 shrink-0", toneClass)} />
        <div className="min-w-0 flex-1">
          <div className="text-muted-foreground font-mono text-[10px] uppercase tracking-wider">
            {label}
          </div>
          <div className={cn("font-mono text-2xl tabular-nums leading-none", toneClass)}>
            {value}
          </div>
        </div>
      </CardContent>
    </Card>
  );
}

interface QuickCardProps {
  to: string;
  icon: React.ComponentType<{ className?: string | undefined }>;
  title: string;
  description: string;
  count?: number;
}

function QuickCard({ to, icon: Icon, title, description, count }: QuickCardProps) {
  return (
    <Card className="hover:border-primary/50 group transition-colors">
      <CardHeader className="flex flex-row items-start justify-between space-y-0">
        <div className="flex items-start gap-3">
          <Icon className="text-primary mt-1 h-4 w-4" />
          <div className="min-w-0">
            <CardTitle>{title}</CardTitle>
            <CardDescription className="mt-1">{description}</CardDescription>
          </div>
        </div>
        {count !== undefined && (
          <Badge variant="muted" className="shrink-0">
            {count}
          </Badge>
        )}
      </CardHeader>
      <CardContent>
        <Button asChild variant="outline" size="sm">
          <Link to={to}>Open →</Link>
        </Button>
      </CardContent>
    </Card>
  );
}
