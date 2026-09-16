// AppShell — the layout chrome that wraps every route:
//   ┌─ Sidebar ─┬─ Header (caps + agent status) ──────────┐
//   │           ├──────────────────────────────────────────┤
//   │           │  <Outlet />                              │
//   │           │                                          │
//   └───────────┴──────────────────────────────────────────┘
//
// Uses an `<Outlet />` from react-router so each route page
// renders into the right pane without re-mounting the sidebar
// (sidebar stays mounted → its state survives navigation).

import { Outlet } from "react-router-dom";

import { SessionProvider } from "../../hooks/useAgentSession";
import { TooltipProvider } from "../ui/tooltip";
import { Header } from "./Header";
import { Sidebar } from "./Sidebar";

export function AppShell() {
  return (
    <TooltipProvider delayDuration={150}>
      <SessionProvider>
        <div className="bg-background text-foreground flex h-screen w-full overflow-hidden">
          <Sidebar />
          <div className="flex flex-1 flex-col overflow-hidden">
            <Header />
            <main className="flex-1 overflow-auto">
              <div className="mx-auto h-full w-full max-w-6xl px-8 py-6">
                <Outlet />
              </div>
            </main>
          </div>
        </div>
      </SessionProvider>
    </TooltipProvider>
  );
}
