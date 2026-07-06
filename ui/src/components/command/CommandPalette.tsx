/** CommandPalette — a ⌘K / Ctrl-K global navigator.
 *
 *  Fuzzy-jump to any session, switch tabs, or open a search — the keyboard-first
 *  navigation pattern shared by VS Code, GitHub, and Linear. Pure ranking lives
 *  in lib/command-palette.ts; this is the overlay + keyboard wiring.
 */

import { useEffect, useMemo, useRef, useState, useCallback } from "react";
import type { HashRoute } from "@/lib/hash-route";
import type { StorySession } from "@/lib/story-api";
import { rankItems } from "@/lib/command-palette";
import { sessionTitle } from "@/lib/session-title";
import { projectKey } from "@/lib/sessions-overview";
import { sessionColor } from "@/lib/session-colors";
import { cn } from "@/lib/cn";

export interface PaletteItem {
  readonly id: string;
  readonly title: string;
  readonly subtitle?: string;
  readonly icon: string;
  readonly group: "Navigate" | "Sessions" | "Recent";
  readonly color?: string;
  readonly searchText: string;
  readonly route: HashRoute;
}

const TAB_ITEMS: PaletteItem[] = [
  { id: "tab-live", title: "Live", icon: "◉", group: "Navigate", searchText: "live stream realtime", route: { view: "live" } },
  { id: "tab-canvas", title: "Canvas", icon: "◇", group: "Navigate", searchText: "canvas board sunburst treemap gantt scatter flow visualize", route: { view: "canvas" } },
  { id: "tab-ask", title: "Ask", icon: "?", group: "Navigate", searchText: "ask questions insights answers", route: { view: "ask" } },
  { id: "tab-explore", title: "Explore", icon: "⊞", group: "Navigate", searchText: "explore sessions browser dashboard calendar overview events conversation", route: { view: "explore" } },
  { id: "tab-story", title: "Story", icon: "❧", group: "Navigate", searchText: "story narrative sentences turns", route: { view: "story" } },
  { id: "tab-users", title: "Users", icon: "☺", group: "Navigate", searchText: "users people fleet", route: { view: "users" } },
  { id: "tab-admin", title: "Admin", icon: "⚙", group: "Navigate", searchText: "admin federation topology", route: { view: "admin" } },
];

/** Build the searchable palette items from the session universe. Pure + testable. */
export function buildPaletteItems(sessions: readonly StorySession[]): PaletteItem[] {
  const sessionItems: PaletteItem[] = sessions
    .filter((s) => !s.session_id.startsWith("agent-"))
    .map((s) => {
      const title = sessionTitle(s);
      const proj = projectKey(s);
      const subtitle = [proj, s.branch, s.user].filter(Boolean).join(" · ");
      return {
        id: `session-${s.session_id}`,
        title: title || s.session_id.slice(0, 8),
        subtitle,
        icon: "›",
        group: "Sessions" as const,
        color: sessionColor(s.session_id),
        searchText: `${title} ${subtitle} ${s.session_id}`,
        route: { view: "explore", sessionId: s.session_id } as HashRoute,
      };
    });
  return [...TAB_ITEMS, ...sessionItems];
}

interface Props {
  sessions: readonly StorySession[];
  onNavigate: (route: HashRoute) => void;
  /** Frecency-ranked recently-viewed session ids (best first). */
  recentIds?: readonly string[];
}

export function CommandPalette({ sessions, onNavigate, recentIds }: Props) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [sel, setSel] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const items = useMemo(() => buildPaletteItems(sessions), [sessions]);

  // Empty-query view: recently-viewed sessions first, then the tabs — so the
  // palette is useful before you type a character.
  const recentItems = useMemo<PaletteItem[]>(() => {
    if (!recentIds?.length) return [];
    return recentIds
      .map((id) => items.find((i) => i.id === `session-${id}`))
      .filter((i): i is PaletteItem => Boolean(i))
      .slice(0, 5)
      .map((i) => ({ ...i, group: "Recent" as const }));
  }, [recentIds, items]);

  const tabItems = useMemo(() => items.filter((i) => i.group === "Navigate"), [items]);

  const results = useMemo(
    () => (query.trim() ? rankItems(query, items, (i) => i.searchText, 40) : [...recentItems, ...tabItems]),
    [query, items, recentItems, tabItems],
  );

  // Global ⌘K / Ctrl-K toggle.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((o) => !o);
      } else if (e.key === "Escape") {
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Reset + focus on open.
  useEffect(() => {
    if (open) {
      setQuery("");
      setSel(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  useEffect(() => { setSel(0); }, [query]);

  const run = useCallback((item: PaletteItem | undefined) => {
    if (!item) return;
    onNavigate(item.route);
    setOpen(false);
  }, [onNavigate]);

  const onInputKey = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => Math.min(s + 1, results.length - 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => Math.max(s - 1, 0)); }
    else if (e.key === "Enter") { e.preventDefault(); run(results[sel]); }
  };

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-[100] flex items-start justify-center bg-black/50 pt-[12vh]"
      onClick={() => setOpen(false)}
      data-testid="command-palette"
    >
      <div
        className="w-[560px] max-w-[92vw] overflow-hidden rounded-xl border border-[#3b4261] bg-[#1f2335] shadow-2xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-2 border-b border-[#2f3348] px-3">
          <span className="text-[#565f89]">⌘</span>
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onInputKey}
            placeholder="Jump to a session or view…"
            className="w-full bg-transparent py-3 text-[14px] text-[#c0caf5] placeholder:text-[#565f89] focus:outline-none"
          />
          <kbd className="rounded bg-[#24283b] px-1.5 py-0.5 text-[10px] text-[#565f89]">esc</kbd>
        </div>

        <div className="max-h-[52vh] overflow-y-auto py-1">
          {results.length === 0 ? (
            <div className="px-4 py-6 text-center text-[12px] text-[#565f89]">No matches.</div>
          ) : (
            results.map((item, i) => {
              const active = i === sel;
              return (
                <button
                  key={item.id}
                  data-palette-item={item.id}
                  onMouseEnter={() => setSel(i)}
                  onClick={() => run(item)}
                  className={cn(
                    "flex w-full items-center gap-3 px-3 py-2 text-left",
                    active ? "bg-[#283549]" : "hover:bg-[#24283b]",
                  )}
                >
                  <span className="w-4 shrink-0 text-center text-[13px]" style={{ color: item.color ?? "#7aa2f7" }}>{item.icon}</span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[13px] text-[#c0caf5]">{item.title}</span>
                    {item.subtitle && <span className="block truncate text-[10px] text-[#565f89]">{item.subtitle}</span>}
                  </span>
                  <span className="shrink-0 text-[9px] uppercase tracking-wide text-[#565f89]">{item.group}</span>
                </button>
              );
            })
          )}
        </div>

        <div className="flex items-center gap-3 border-t border-[#2f3348] px-3 py-1.5 text-[10px] text-[#565f89]">
          <span><kbd className="text-[#7aa2f7]">↑↓</kbd> navigate</span>
          <span><kbd className="text-[#7aa2f7]">↵</kbd> open</span>
          <span className="ml-auto">{results.length} result{results.length === 1 ? "" : "s"}</span>
        </div>
      </div>
    </div>
  );
}
