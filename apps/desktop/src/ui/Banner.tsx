import type { ReactNode } from "react";
import { AlertTriangle, CheckCircle2, Info, XCircle } from "lucide-react";
import "./Banner.css";

export type Tone = "info" | "success" | "warning" | "danger";

const ICONS = { info: Info, success: CheckCircle2, warning: AlertTriangle, danger: XCircle };

export interface BannerProps {
  tone: Tone;
  title: string;
  children?: ReactNode;
  /** Live region politeness. Use "polite" for status that changes while the user works. */
  live?: "polite" | "off";
}

/** Inline, non-modal message about application or service state. Colour is never the only cue. */
export function Banner({ tone, title, children, live = "polite" }: BannerProps) {
  const Icon = ICONS[tone];
  return (
    <section
      className={`banner banner--${tone}`}
      role={live === "polite" ? "status" : undefined}
      aria-label={live === "polite" ? undefined : title}
    >
      <Icon className="icon banner__icon" aria-hidden="true" />
      <div className="banner__body">
        <h2 className="banner__title">{title}</h2>
        {children && <div className="banner__text">{children}</div>}
      </div>
    </section>
  );
}
