import type { ReactNode } from "react";

export function Badge({ children, tone, title }: { children: ReactNode; tone?: string; title?: string }) {
  return <span className={`badge ${tone ?? ""}`} title={title}>{children}</span>;
}
