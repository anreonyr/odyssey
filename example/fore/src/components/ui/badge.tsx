// shadcn/ui Badge — replaces the hand-rolled Chip / KindBadge /
// OpsBadge / StatusPill components. Variant covers everything the
// old components encoded via inline colour: default (sync cap),
// streaming (cyan-tinted via primary), success / warning /
// destructive for status, outline for neutral tags, secondary
// for muted meta.

import { cva, type VariantProps } from "class-variance-authority";
import * as React from "react";

import { cn } from "../../lib/utils";

const badgeVariants = cva(
  "focus:ring-ring inline-flex items-center gap-1 rounded-md border px-2 py-0.5 font-mono text-[10px] font-medium uppercase tracking-wide transition-colors focus:outline-none focus:ring-2 focus:ring-offset-2",
  {
    variants: {
      variant: {
        default: "bg-primary text-primary-foreground hover:bg-primary/80 border-transparent",
        secondary:
          "bg-secondary text-secondary-foreground hover:bg-secondary/80 border-transparent",
        destructive:
          "bg-destructive text-destructive-foreground hover:bg-destructive/80 border-transparent",
        success: "border-success/30 bg-success/15 text-success border-transparent",
        warning: "border-warning/30 bg-warning/15 text-warning border-transparent",
        outline: "text-foreground",
        muted: "bg-muted text-muted-foreground border-transparent",
        stream: "bg-accent text-accent-foreground border-transparent",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  },
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLSpanElement>, VariantProps<typeof badgeVariants> {}

function Badge({ className, variant, ...props }: BadgeProps) {
  return <span className={cn(badgeVariants({ variant }), className)} {...props} />;
}

export { Badge, badgeVariants };
