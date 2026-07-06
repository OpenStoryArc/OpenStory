import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { SessionCalendar } from "@/components/viz/SessionCalendar";
import type { StorySession } from "@/lib/story-api";
import { dayKey } from "@/lib/sessions-overview";

function sess(id: string, start: string, events = 1): StorySession {
  return { session_id: id, start_time: start, event_count: events, status: "completed" };
}

const A = "2026-06-10T09:00:00.000Z";
const SESSIONS = [sess("s1", A, 10), sess("s2", A, 3), sess("s3", "2026-06-11T14:00:00.000Z", 7)];
const END = new Date(2026, 5, 13);

describe("SessionCalendar", () => {
  it("renders a clickable cell for every active day", () => {
    render(<SessionCalendar sessions={SESSIONS} end={END} weeks={8} selectedDay={null} onSelectDay={() => {}} />);
    const active = document.querySelectorAll('[data-cal-active="true"]');
    // two active days: 2026-06-10 and 2026-06-11
    expect(active.length).toBe(2);
  });

  it("fires onSelectDay with the day key when a cell is clicked", () => {
    const onSelect = vi.fn();
    render(<SessionCalendar sessions={SESSIONS} end={END} weeks={8} selectedDay={null} onSelectDay={onSelect} />);
    const cell = document.querySelector(`[data-cal-date="${dayKey(new Date(A))}"]`)!;
    fireEvent.click(cell);
    expect(onSelect).toHaveBeenCalledWith(dayKey(new Date(A)));
  });

  it("shows a summary of how many active days are in range", () => {
    render(<SessionCalendar sessions={SESSIONS} end={END} weeks={8} selectedDay={null} onSelectDay={() => {}} />);
    expect(screen.getByText(/2 active days/i)).toBeInTheDocument();
  });
});
