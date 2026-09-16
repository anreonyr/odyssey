// JsonEditor — textarea + live JSON validation. The original
// version lived in src/components/JsonEditor.tsx; this is a
// drop-in replacement that swaps the inline-styled <textarea>
// for a shadcn Textarea with a small status bar below.
//
// The validation runs on every keystroke but doesn't block
// typing — invalid JSON shows a warning, valid JSON shows a
// green tick, and the parent reads the raw string (the parent
// decides whether to throw on parse error before invoking).

import { Check, AlertTriangle } from "lucide-react";
import { useMemo } from "react";

import { cn } from "../lib/utils";
import { Textarea } from "./ui/textarea";

interface JsonEditorProps {
  value: string;
  onChange: (value: string) => void;
  rows?: number;
  placeholder?: string;
  className?: string;
}

export function JsonEditor({ value, onChange, rows = 6, placeholder, className }: JsonEditorProps) {
  const state = useMemo(() => {
    if (!value.trim()) return { valid: true, empty: true } as const;
    try {
      JSON.parse(value);
      return { valid: true, empty: false } as const;
    } catch (e) {
      return { valid: false, empty: false, error: (e as Error).message } as const;
    }
  }, [value]);

  return (
    <div className={cn("space-y-1", className)}>
      <Textarea
        rows={rows}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder={placeholder ?? '{"hello": "world"}'}
        className="leading-relaxed"
        data-input="json-editor"
      />
      <div className="flex items-center gap-2 font-mono text-[10px]">
        {state.empty ? (
          <span className="text-muted-foreground">empty (will send {"{}"})</span>
        ) : state.valid ? (
          <>
            <Check className="text-success h-3 w-3" />
            <span className="text-success">valid JSON</span>
          </>
        ) : (
          <>
            <AlertTriangle className="text-warning h-3 w-3" />
            <span className="text-warning">invalid JSON</span>
            <span className="text-muted-foreground truncate">— {state.error}</span>
          </>
        )}
      </div>
    </div>
  );
}
