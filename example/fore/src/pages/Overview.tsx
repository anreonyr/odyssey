// Overview dashboard — the `/` route. One-glance view of
// the cspace + agent state, plus shortcut cards to the
// other pages.
//
// Data comes from the same hooks every other page uses; the
// sidebar counts and the cards here stay in sync without
// any extra fetching.

import { Activity, Bot, Boxes, Cpu, MessageSquare, Save } from "lucide-react";
import { Link } from "react-router-dom";

import { MemoryPanel } from "@/components/MemoryPanel";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { useAgentSession } from "@/hooks/useAgentSession";
import { useCaps } from "@/hooks/useCaps";
import { cn } from "@/lib/utils";

export function Overview() {
  const { caps, reachable, ready } = useCaps();
  const { sessions } = useAgentSession();

  const liveSessions = Array.from(sessions.values()).filter(
    (s) => s.status === "Running" || s.status === "AwaitingObservation",
  );

  return (
    <div className="space-y-6">
      <section aria-label="system status">
        <h2 className="text-muted-foreground mb-3 text-sm font-medium">System status</h2>
        {!ready ? (
          <div className="grid grid-cols-4 gap-4">
            {Array.from({ length: 4 }).map((_, i) => (
              <Skeleton key={i} className="h-24" />
            ))}
          </div>
        ) : (
          <div className="grid grid-cols-2 gap-4 md:grid-cols-4">
            <Stat icon={Cpu} label="Capabilities mounted" value={caps.length} />
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
            <Stat icon={Save} label="Saved checkpoints" value={"—"} hint="in session detail" />
          </div>
        )}
      </section>

      <section aria-label="quick links">
        <h2 className="text-muted-foreground mb-3 text-sm font-medium">Jump to</h2>
        <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
          <QuickCard
            to="/chat"
            icon={MessageSquare}
            title="Start chatting"
            description="Drive an agent session token-by-token."
          />
          <QuickCard
            to="/agent/sessions"
            icon={Bot}
            title="Agent sessions"
            description="Step-by-step control over running sessions."
            count={sessions.size}
          />
          <QuickCard
            to="/explore"
            icon={Boxes}
            title="Explore"
            description="Capabilities, raw invoke, and the LLM playground."
            count={caps.length}
          />
        </div>
      </section>

      <section aria-label="memory">
        <h2 className="text-muted-foreground mb-3 text-sm font-medium">Memory</h2>
        <Card>
          <CardContent className="p-6">
            <MemoryPanel />
          </CardContent>
        </Card>
      </section>
    </div>
  );
}

interface StatProps {
  icon: React.ComponentType<{ className?: string | undefined }>;
  label: string;
  value: number | string;
  tone?: "default" | "success" | "warning" | "muted";
  hint?: string;
}

function Stat({ icon: Icon, label, value, tone = "default", hint }: StatProps) {
  const toneClass = {
    default: "text-foreground",
    success: "text-success",
    warning: "text-warning",
    muted: "text-muted-foreground",
  }[tone];
  return (
    <Card>
      <CardContent className="flex items-center gap-3 p-6">
        <Icon className={cn("h-4 w-4 shrink-0", toneClass)} />
        <div className="min-w-0 flex-1">
          <div className="text-muted-foreground text-xs">{label}</div>
          <div className={cn("font-mono text-2xl tabular-nums leading-none", toneClass)}>
            {value}
          </div>
          {hint && <div className="text-muted-foreground text-xs">{hint}</div>}
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
    <Card>
      <CardHeader>
        <div className="flex items-start justify-between">
          <div className="flex items-start gap-3">
            <Icon className="text-muted-foreground mt-0.5 h-4 w-4" />
            <div className="min-w-0">
              <CardTitle className="text-base">{title}</CardTitle>
              <CardDescription className="mt-1">{description}</CardDescription>
            </div>
          </div>
          {count !== undefined && <Badge variant="muted">{count}</Badge>}
        </div>
      </CardHeader>
      <CardContent>
        <Button asChild variant="outline" size="sm">
          <Link to={to}>Open</Link>
        </Button>
      </CardContent>
    </Card>
  );
}
