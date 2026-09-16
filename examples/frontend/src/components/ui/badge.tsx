// shadcn/ui Badge — replaces the hand-rolled Chip / KindBadge /
// OpsBadge / StatusPill components. Variant covers everything the
// old components encoded via inline colour: default (sync cap),
// streaming (cyan-tinted via primary), success / warning /
// destructive for status, outline for neutral tags, secondary
// for muted meta.

import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "../../lib/utils";

const badgeVariants = cva(
  "inline-flex items-center gap-1 rounded-md border px-2 py-0.5 text-[10px] font-mono font-medium uppercase tracking-wide transition-colors focus:outline-none focus:ring-2 focus:ring-ring focus:ring-offset-2",
  {
    variants: {
      variant: {
        default:
          "border-transparent bg-primary text-primary-foreground hover:bg-primary/80",
        secondary:
          "border-transparent bg-secondary text-secondary-foreground hover:bg-secondary/80",
        destructive:
          "border-transparent bg-destructive text-destructive-foreground hover:bg-destructive/80",
        success:
          "border-transparent bg-success/15 text-success border-success/30",
        warning:
          "border-transparent bg-warning/15 text-warning border-warning/30",
        outline: "text-foreground",
        muted:
          "border-transparent bg-muted text-muted-foreground",
        stream:
          "border-transparent bg-accent text-accent-foreground",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  },
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLSpanElement>,
    VariantProps<typeof badgeVariants> {}

function Badge({ className, variant, ...props }: BadgeProps) {
  return <span className={cn(badgeVariants({ variant }), className)} {...props} />;
}

export { Badge, badgeVariants };
