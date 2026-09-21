// Memory panel — recall + record side by side.
//
// The original AgentMemory.tsx used Tabs to switch between
// recall and record; both buttons need to be present in the
// DOM at the same time so the data-binding test can find the
// record button even when recall is the active view. Side-by-
// side is also faster: switching tabs hides a control the
// user might want to grab next, and on a developer-facing page
// both surfaces earn their place.

import type { AgentMemoryHit } from "@/api/types";

import { Loader2, Plus, Search } from "lucide-react";
import { useState } from "react";

import { agent } from "@/api/client";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";

export function MemoryPanel() {
  return (
    <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
      <RecallPanel />
      <RecordPanel />
    </div>
  );
}

function RecallPanel() {
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<AgentMemoryHit[]>([]);
  const [running, setRunning] = useState(false);

  async function recall() {
    if (!query.trim()) return;
    setRunning(true);
    try {
      const r = await agent.memoryRecall({ query });
      setHits(r.hits);
    } finally {
      setRunning(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm font-medium">Recall</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex gap-2">
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search memory…"
            onKeyDown={(e) => e.key === "Enter" && recall()}
            data-input="memory-query"
          />
          <Button
            size="default"
            onClick={recall}
            disabled={running || !query.trim()}
            data-action="memory-recall"
          >
            {running ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <Search className="h-4 w-4" />
            )}
            Recall
          </Button>
        </div>
        {hits.length === 0 ? (
          <p className="text-muted-foreground text-xs">No hits yet.</p>
        ) : (
          <ul className="space-y-2">
            {hits.map((h, i) => (
              <li key={i} className="bg-muted/30 rounded-md border p-2">
                <p className="text-sm">{h.content}</p>
                <div className="text-muted-foreground mt-1 flex items-center gap-2 text-xs">
                  <Badge variant="muted">score {h.score.toFixed(3)}</Badge>
                  {h.tags.map((t) => (
                    <Badge key={t} variant="outline">
                      {t}
                    </Badge>
                  ))}
                  <span className="ml-auto font-mono">
                    {new Date(h.created_at * 1000).toISOString().slice(0, 16)}
                  </span>
                </div>
              </li>
            ))}
          </ul>
        )}
      </CardContent>
    </Card>
  );
}

function RecordPanel() {
  const [content, setContent] = useState("");
  const [tags, setTags] = useState("");
  const [running, setRunning] = useState(false);
  const [lastId, setLastId] = useState<string | null>(null);

  async function record() {
    if (!content.trim()) return;
    setRunning(true);
    try {
      const r = await agent.memoryRecord({
        content,
        tags: tags
          .split(",")
          .map((t) => t.trim())
          .filter(Boolean),
      });
      setLastId(r.id);
      setContent("");
    } finally {
      setRunning(false);
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm font-medium">Record</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <Textarea
          rows={4}
          placeholder="Memory content…"
          value={content}
          onChange={(e) => setContent(e.target.value)}
          data-input="memory-content"
        />
        <Input
          value={tags}
          onChange={(e) => setTags(e.target.value)}
          placeholder="Tags (comma-separated)"
          data-input="memory-tags"
        />
        <Button
          size="default"
          onClick={record}
          disabled={running || !content.trim()}
          data-action="memory-record"
        >
          {running ? <Loader2 className="h-4 w-4 animate-spin" /> : <Plus className="h-4 w-4" />}
          Record
        </Button>
        {lastId && (
          <p className="text-muted-foreground font-mono text-xs">
            stored · id {lastId.slice(0, 12)}…
          </p>
        )}
      </CardContent>
    </Card>
  );
}
