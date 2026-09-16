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
import { Sidebar } from "./Sidebar";
import { Header } from "./Header";
import { TooltipProvider } from "../ui/tooltip";

export function AppShell() {
  return (
    <TooltipProvider delayDuration={150}>
      <div className="flex h-screen w-full overflow-hidden bg-background text-foreground">
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
    </TooltipProvider>
  );
}
