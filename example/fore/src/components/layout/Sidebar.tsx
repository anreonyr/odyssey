// Sidebar — fixed-width navigation rail. Active link gets a
// left border + accent text; inactive links are muted.
//
// Counts (caps, sessions, checkpoints) come from the same
// hooks the pages use, so the sidebar's badges always reflect
// the same source of truth as the page content.
//
// Receives arbitrary HTMLAttributes so AppShell can pass
// `data-stagger-item` + style for the entrance choreography
// (DESIGN.md "动作"). Props.className is intentionally ignored:
// the sidebar's chrome is fixed, callers shouldn't override it.

import type { HTMLAttributes } from "react";

import {
  LayoutDashboard,
  Boxes,
  Bot,
  Terminal,
  Sparkles,
  Save,
  CircleDot,
  MessageSquare,
} from "lucide-react";
import { NavLink } from "react-router-dom";

import { useAgentSession } from "../../hooks/useAgentSession";
import { useCaps } from "../../hooks/useCaps";
import { useCheckpoints } from "../../hooks/useCheckpoints";
import { cn } from "../../lib/utils";

interface NavItem {
  to: string;
  label: string;
  icon: React.ComponentType<{ className?: string | undefined }>;
  /** Returns the count badge for this item. `null` means no badge. */
  badge?: () => number | null;
}

export function Sidebar(props: HTMLAttributes<HTMLElement>) {
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
    {
      to: "/chat",
      label: "Chat",
      icon: MessageSquare,
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
    <aside
      {...props}
      className="border-border bg-card flex h-screen w-56 shrink-0 flex-col border-r"
    >
      <div className="border-border flex h-12 items-center gap-2 border-b px-4">
        <CircleDot className="text-success h-3.5 w-3.5" />
        <span className="font-mono text-sm font-semibold tracking-tight">odyssey</span>
        <span className="text-muted-foreground ml-auto font-mono text-[10px] uppercase tracking-wider">
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
                  <span className="bg-primary absolute ml-[-12px] h-4 w-0.5 rounded-r" />
                )}
                <item.icon className="h-3.5 w-3.5 shrink-0" />
                <span className="min-w-0 flex-1 truncate">{item.label}</span>
                {item.badge && item.badge() != null && (
                  <span className="text-muted-foreground shrink-0 font-mono text-[10px] tabular-nums">
                    {item.badge()}
                  </span>
                )}
              </>
            )}
          </NavLink>
        ))}
      </nav>

      <div className="border-border text-muted-foreground border-t p-3 font-mono text-[10px] leading-relaxed">
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
