// shadcn/ui Skeleton — placeholder for the loading state on
// the overview dashboard while caps + agent are still being
// fetched.

import { cn } from "../../lib/utils";

function Skeleton({ className, ...props }: React.HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("bg-muted animate-pulse rounded-md", className)} {...props} />;
}

export { Skeleton };
