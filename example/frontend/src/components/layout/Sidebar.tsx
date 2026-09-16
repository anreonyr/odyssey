// Sidebar — fixed-width navigation rail. Active link gets a
// left border + accent text; inactive links are muted.
//
// Counts (caps, sessions, checkpoints) come from the same
// hooks the pages use, so the sidebar's badges always reflect
// the same source of truth as the page content.

import { NavLink } from "react-router-dom";
import {
  LayoutDashboard,
  Boxes,
  Bot,
  Terminal,
  Sparkles,
  Save,
  CircleDot,
} from "lucide-react";
import { useCaps } from "../../hooks/useCaps";
import { useAgentSession } from "../../hooks/useAgentSession";
import { useCheckpoints } from "../../hooks/useCheckpoints";
import { cn } from "../../lib/utils";

interface NavItem {
  to: string;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  /** Returns the count badge for this item. `null` means no badge. */
  badge?: () => number | null;
}

export function Sidebar() {
  const { caps, reachable, ready, agentError } = useCaps();
  const { sessions } = useAgentSession();
  const { checkpoints } = useCheckpoints();

  const items: NavItem[] = [
    { to: "/", label: "Overview", icon: LayoutDashboard },
    { to: "/caps", label: "Capabilities", icon: Boxes, badge: () => caps.length },
    {
      to: "/agent",
      label: "Agent",
      icon: Bot,
      badge: () => sessions.size || null,
    },
    { to: "/invoke", label: "Invoke", icon: Terminal },
    { to: "/playground", label: "Playground", icon: Sparkles },
    {
      to: "/checkpoints",
      label: "Checkpoints",
      icon: Save,
      badge: () => checkpoints.length || null,
    },
  ];

  return (
    <aside className="flex h-screen w-56 shrink-0 flex-col border-r border-border bg-card">
      <div className="flex h-12 items-center gap-2 border-b border-border px-4">
        <CircleDot className="h-3.5 w-3.5 text-success" />
        <span className="font-mono text-sm font-semibold tracking-tight">
          odyssey
        </span>
        <span className="ml-auto font-mono text-[10px] uppercase tracking-wider text-muted-foreground">
          v0.1
        </span>
      </div>

      <nav className="flex-1 space-y-0.5 overflow-y-auto p-2">
        {items.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            end={item.to === "/"}
            className={({ isActive }) =>
              cn(
                "group flex h-8 items-center gap-2.5 rounded-md px-3 text-xs transition-colors",
                isActive
                  ? "bg-accent text-foreground"
                  : "text-muted-foreground hover:bg-accent/50 hover:text-foreground",
              )
            }
          >
            {({ isActive }) => (
              <>
                {isActive && (
                  <span className="absolute ml-[-12px] h-4 w-0.5 rounded-r bg-primary" />
                )}
                <item.icon className="h-3.5 w-3.5 shrink-0" />
                <span className="min-w-0 flex-1 truncate">{item.label}</span>
                {item.badge && item.badge() != null && (
                  <span className="shrink-0 font-mono text-[10px] tabular-nums text-muted-foreground">
                    {item.badge()}
                  </span>
                )}
              </>
            )}
          </NavLink>
        ))}
      </nav>

      <div className="border-t border-border p-3 font-mono text-[10px] leading-relaxed text-muted-foreground">
        <div className="flex items-center justify-between">
          <span>mounted</span>
          <span className="text-foreground tabular-nums">{caps.length}</span>
        </div>
        <div className="flex items-center justify-between">
          <span>reachable</span>
          <span className="text-foreground tabular-nums">{reachable.size}</span>
        </div>
        <div className="mt-1 flex items-center justify-between">
          <span>agent</span>
          <AgentStatusPill ready={ready} error={agentError} />
        </div>
      </div>
    </aside>
  );
}

function AgentStatusPill({ ready, error }: { ready: boolean; error: string | null }) {
  if (error) return <span className="text-destructive">down</span>;
  if (!ready) return <span className="text-muted-foreground">…</span>;
  return <span className="text-success">up</span>;
}
