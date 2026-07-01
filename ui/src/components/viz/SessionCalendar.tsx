/** SessionCalendar — a GitHub-contribution-style heatmap of sessions over time.
 *
 *  One square per day, columns = weeks, rows = weekdays, colored by how many
 *  sessions started that day. Click a day to filter the dashboard to it.
 *  Built on the pure `buildCalendar` model; D3 supplies the color scale.
 */

import { useMemo, useState } from "react";
import { scaleOrdinal } from "d3-scale";
import { buildCalendar, type CalendarCell } from "@/lib/sessions-overview";
import type { StorySession } from "@/lib/story-api";
import { cn } from "@/lib/cn";

interface Props {
  sessions: readonly StorySession[];
  selectedDay: string | null;
  onSelectDay: (day: string | null) => void;
  end?: Date;
  weeks?: number;
  className?: string;
}

const CELL = 12;
const GAP = 3;
const STEP = CELL + GAP;
const TOP = 16; // month-label band
const LEFT = 24; // weekday-label gutter

// Tokyonight green ramp: empty → busiest. Level 1 is deliberately well clear of
// the empty cell so a single-session day is unmistakably "there".
const LEVEL_FILL = scaleOrdinal<string, string>()
  .domain(["0", "1", "2", "3", "4"])
  .range(["#1b1d29", "#375e43", "#4f9560", "#6dbd78", "#b9e88a"]);

const WEEKDAY_LABELS: [number, string][] = [
  [1, "Mon"],
  [3, "Wed"],
  [5, "Fri"],
];

function cellTitle(c: CalendarCell): string {
  if (c.sessionCount === 0) return `${c.date} — no sessions`;
  const ev = c.eventCount.toLocaleString();
  return `${c.date} — ${c.sessionCount} session${c.sessionCount > 1 ? "s" : ""}, ${ev} events`;
}

export function SessionCalendar({ sessions, selectedDay, onSelectDay, end, weeks = 26, className }: Props) {
  const model = useMemo(() => buildCalendar(sessions, { end, weeks }), [sessions, end, weeks]);
  const [hover, setHover] = useState<{ c: CalendarCell; x: number; y: number } | null>(null);

  const width = LEFT + model.weeks * STEP + 4;
  const height = TOP + 7 * STEP + 4;

  return (
    <div className={cn("relative", className)}>
      <div className="flex items-center justify-between px-1 pb-1 text-[10px] text-[#565f89]">
        <span>{model.totalDays} active days · {model.totalSessions.toLocaleString()} sessions</span>
        <span className="flex items-center gap-1">
          Less
          {(["0", "1", "2", "3", "4"] as const).map((l) => (
            <span key={l} className="inline-block rounded-[2px]" style={{ width: 9, height: 9, background: LEVEL_FILL(l) }} />
          ))}
          More
        </span>
      </div>

      <svg width={width} height={height} className="block overflow-visible" role="img" aria-label="Sessions calendar heatmap">
        {/* month labels */}
        {model.monthLabels.map((m, i) => (
          <text key={i} x={LEFT + m.weekIndex * STEP} y={11} fontSize={9} fill="#565f89">
            {m.label}
          </text>
        ))}

        {/* weekday labels */}
        {WEEKDAY_LABELS.map(([dow, label]) => (
          <text key={dow} x={0} y={TOP + dow * STEP + CELL - 1} fontSize={9} fill="#565f89">
            {label}
          </text>
        ))}

        {/* day cells */}
        {model.cells.map((c) => {
          if (!c.inRange) return null;
          const x = LEFT + c.weekIndex * STEP;
          const y = TOP + c.dow * STEP;
          const active = c.sessionCount > 0;
          const selected = selectedDay === c.date;
          return (
            <rect
              key={c.date}
              data-cal-date={c.date}
              data-cal-active={active ? "true" : "false"}
              x={x}
              y={y}
              width={CELL}
              height={CELL}
              rx={2}
              fill={LEVEL_FILL(String(c.level))}
              stroke={selected ? "#7aa2f7" : active ? "#000000" : "transparent"}
              strokeOpacity={selected ? 1 : active ? 0.15 : 0}
              strokeWidth={selected ? 1.5 : 1}
              className={active ? "cursor-pointer" : undefined}
              onMouseEnter={() => setHover({ c, x, y })}
              onMouseLeave={() => setHover((h) => (h?.c.date === c.date ? null : h))}
              onClick={active ? () => onSelectDay(selected ? null : c.date) : undefined}
            >
              <title>{cellTitle(c)}</title>
            </rect>
          );
        })}
      </svg>

      {hover && hover.c.sessionCount > 0 && (
        <div
          className="pointer-events-none absolute z-10 whitespace-nowrap rounded border border-[#2f3348] bg-[#1a1b26] px-2 py-1 text-[10px] text-[#c0caf5] shadow-lg"
          style={{ left: Math.min(hover.x + 14, width - 40), top: hover.y + TOP + 14 }}
        >
          <span className="text-[#c0caf5]">{hover.c.date}</span>
          <span className="ml-1 text-[#9ece6a]">{hover.c.sessionCount} sess</span>
          <span className="ml-1 text-[#565f89]">{hover.c.eventCount.toLocaleString()} ev</span>
        </div>
      )}
    </div>
  );
}
