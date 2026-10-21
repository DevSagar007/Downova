import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import "./MenuBar.css";

export type MenuEntry =
  | { kind: "separator"; id: string }
  | {
      kind: "item";
      id: string;
      label: string;
      onSelect?: () => void;
      /** Unavailable items stay visible and explain why instead of acting. */
      unavailableReason?: string | undefined;
      checked?: boolean;
      /** How `checked` is exposed: one-of-many (radio, default) or independent toggle (checkbox). */
      checkType?: "radio" | "checkbox";
    };

export interface Menu {
  id: string;
  label: string;
  entries: MenuEntry[];
}

/**
 * Classic application menu bar (Tasks / File / Downloads / View / Help).
 * Keyboard: ←/→ between menus, ↓/Enter/Space opens, ↑/↓ moves inside, Esc closes.
 */
export function MenuBar({ menus, label }: { menus: Menu[]; label: string }) {
  const [open, setOpen] = useState<number | null>(null);
  const barRef = useRef<HTMLDivElement>(null);
  const topRefs = useRef<(HTMLButtonElement | null)[]>([]);

  useEffect(() => {
    if (open === null) return;
    const close = (e: MouseEvent) => {
      if (!barRef.current?.contains(e.target as Node)) setOpen(null);
    };
    document.addEventListener("mousedown", close);
    return () => document.removeEventListener("mousedown", close);
  }, [open]);

  useEffect(() => {
    if (open === null) return;
    barRef.current?.querySelector<HTMLElement>(`[data-menu="${open}"] [role^="menuitem"]`)?.focus();
  }, [open]);

  const focusTop = (index: number) => {
    const count = menus.length;
    const next = ((index % count) + count) % count;
    topRefs.current[next]?.focus();
    return next;
  };

  function onTopKeyDown(event: KeyboardEvent, index: number) {
    if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
      event.preventDefault();
      const next = focusTop(index + (event.key === "ArrowRight" ? 1 : -1));
      if (open !== null) setOpen(next);
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      setOpen(index);
    }
  }

  function onMenuKeyDown(event: KeyboardEvent, index: number) {
    const items = Array.from(
      event.currentTarget.querySelectorAll<HTMLElement>('[role^="menuitem"]'),
    );
    const current = items.indexOf(document.activeElement as HTMLElement);
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const delta = event.key === "ArrowDown" ? 1 : -1;
      items[(current + delta + items.length) % items.length]?.focus();
    } else if (event.key === "Escape") {
      event.preventDefault();
      setOpen(null);
      topRefs.current[index]?.focus();
    } else if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
      event.preventDefault();
      setOpen(focusTop(index + (event.key === "ArrowRight" ? 1 : -1)));
    } else if (event.key === "Tab") {
      setOpen(null);
    }
  }

  return (
    <div ref={barRef} className="menubar" role="menubar" aria-label={label}>
      {menus.map((menu, index) => (
        <div key={menu.id} className="menubar__menu" data-menu={index}>
          <button
            ref={(el) => {
              topRefs.current[index] = el;
            }}
            type="button"
            role="menuitem"
            className="menubar__top"
            aria-haspopup="menu"
            aria-expanded={open === index}
            tabIndex={index === 0 ? 0 : -1}
            onClick={() => setOpen(open === index ? null : index)}
            onKeyDown={(e) => onTopKeyDown(e, index)}
          >
            {menu.label}
          </button>
          {open === index && (
            <div
              className="menubar__popup"
              role="menu"
              tabIndex={-1}
              aria-label={menu.label}
              onKeyDown={(e) => onMenuKeyDown(e, index)}
            >
              {menu.entries.map((entry) =>
                entry.kind === "separator" ? (
                  <div key={entry.id} role="separator" className="menubar__separator" />
                ) : (
                  <button
                    key={entry.id}
                    type="button"
                    role={
                      entry.checked === undefined
                        ? "menuitem"
                        : entry.checkType === "checkbox"
                          ? "menuitemcheckbox"
                          : "menuitemradio"
                    }
                    aria-checked={entry.checked}
                    className="menubar__item"
                    tabIndex={-1}
                    aria-disabled={entry.unavailableReason !== undefined || undefined}
                    title={entry.unavailableReason}
                    onClick={() => {
                      if (entry.unavailableReason !== undefined) return;
                      setOpen(null);
                      topRefs.current[index]?.focus();
                      entry.onSelect?.();
                    }}
                  >
                    <span className="menubar__check" aria-hidden="true">
                      {entry.checked ? (entry.checkType === "checkbox" ? "✓" : "●") : ""}
                    </span>
                    {entry.label}
                  </button>
                ),
              )}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
