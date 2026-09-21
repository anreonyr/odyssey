// Explore (`/explore`) — consolidated dev-tools surface. Three
// tabs:
//
//   - Capabilities: plugin-grouped cap list with filter and
//     reachable-only toggle; clicking a row opens an inspector
//     on the right with the JSON input editor + Run + Result.
//   - Invoke: simpler pick-a-cap-and-run layout (replaces the
//     old `/invoke` page).
//   - Playground: prompt → completion, with completion/raw
//     tabs on the result.

import { useState } from "react";

import { CapabilitiesTab } from "@/components/explore/CapabilitiesTab";
import { InvokeTab } from "@/components/explore/InvokeTab";
import { PlaygroundTab } from "@/components/explore/PlaygroundTab";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";

type Tab = "capabilities" | "invoke" | "playground";

export function Explore() {
  const [tab, setTab] = useState<Tab>("capabilities");

  return (
    <Tabs value={tab} onValueChange={(v) => setTab(v as Tab)} className="space-y-4">
      <TabsList>
        <TabsTrigger value="capabilities">Capabilities</TabsTrigger>
        <TabsTrigger value="invoke">Invoke</TabsTrigger>
        <TabsTrigger value="playground">Playground</TabsTrigger>
      </TabsList>

      <TabsContent value="capabilities">
        <CapabilitiesTab />
      </TabsContent>
      <TabsContent value="invoke">
        <InvokeTab />
      </TabsContent>
      <TabsContent value="playground">
        <PlaygroundTab />
      </TabsContent>
    </Tabs>
  );
}
