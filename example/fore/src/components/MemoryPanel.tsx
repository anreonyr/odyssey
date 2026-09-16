// Memory panel — recall + record side by side.
//
// The original AgentMemory.tsx used Tabs to switch between
// recall and record; both buttons need to be present in the
// DOM at the same time so the data-binding test can find the
// record button even when recall is the active view. Side-by-
// side is also faster: switching tabs hides a control the
// user might want to grab next, and on a developer-facing page
// both surfaces earn their place.

import type { AgentMemoryHit } from "../api/types";

import { Search, Plus, Loader2 } from "lucide-react";
import { useState } from "react";

import { agent } from "../api/client";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "./ui/card";
import { Input } from "./ui/input";
import { Textarea } from "./ui/textarea";

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
        <CardTitle className="text-xs">Recall</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="flex gap-2">
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="search memory…"
            onKeyDown={(e) => e.key === "Enter" && recall()}
            data-input="memory-query"
          />
          <Button
            size="sm"
            variant="default"
            onClick={recall}
            disabled={running || !query.trim()}
            data-action="memory-recall"
          >
            {running ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
            ) : (
              <Search className="h-3.5 w-3.5" />
            )}
            recall
          </Button>
        </div>
        {hits.length === 0 ? (
          <p className="text-muted-foreground text-xs">no hits yet.</p>
        ) : (
          <ul className="space-y-2">
            {hits.map((h, i) => (
              <li key={i} className="border-border bg-muted/20 rounded-md border p-2.5">
                <p className="font-mono text-xs">{h.content}</p>
                <div className="text-muted-foreground mt-1 flex items-center gap-2 text-[10px]">
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
        <CardTitle className="text-xs">Record</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <Textarea
          rows={4}
          placeholder="memory content…"
          value={content}
          onChange={(e) => setContent(e.target.value)}
          data-input="memory-content"
        />
        <Input
          value={tags}
          onChange={(e) => setTags(e.target.value)}
          placeholder="tags (comma-separated)"
          data-input="memory-tags"
        />
        <Button
          size="sm"
          variant="success"
          onClick={record}
          disabled={running || !content.trim()}
          data-action="memory-record"
        >
          {running ? (
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
          ) : (
            <Plus className="h-3.5 w-3.5" />
          )}
          record
        </Button>
        {lastId && (
          <p className="text-muted-foreground font-mono text-[10px]">
            stored · id {lastId.slice(0, 12)}…
          </p>
        )}
      </CardContent>
    </Card>
  );
}
