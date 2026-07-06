/** Renders a harness-wrapper user message (slash command, task notification,
 *  system reminder, local stdout) as a clean, structured block instead of raw
 *  XML — while keeping the full content visible (never truncated). */

import { classifyHarnessMessage } from "@/lib/harness-message";

export function HarnessMessageBlock({ text }: { text: string }) {
  const m = classifyHarnessMessage(text);

  if (m.kind === "slash_command") {
    return (
      <div className="text-sm" data-harness="slash_command">
        <span className="inline-flex items-center gap-1 rounded bg-[#7aa2f7]/15 px-1.5 py-0.5 font-mono text-[12px] text-[#7aa2f7]">
          <span className="opacity-70">⌘</span>/{m.command}
        </span>
        {m.args && (
          <pre className="mt-1 whitespace-pre-wrap break-words text-[13px] text-[#c0caf5]">{m.args}</pre>
        )}
        {m.stdout && (
          <pre className="mt-1 whitespace-pre-wrap break-words rounded bg-[#1a1b26] p-1.5 font-mono text-[11px] text-[#9ece6a]">{m.stdout}</pre>
        )}
      </div>
    );
  }

  if (m.kind === "task_notification") {
    return (
      <div className="text-sm" data-harness="task_notification">
        <span className="inline-flex items-center gap-1 rounded bg-[#bb9af7]/15 px-1.5 py-0.5 text-[11px] text-[#bb9af7]">
          ⚙ background task{m.status ? ` — ${m.status}` : ""}
        </span>
        {m.summary && <div className="mt-1 text-[13px] text-[#c0caf5]">{m.summary}</div>}
        {m.taskId && <div className="mt-0.5 font-mono text-[10px] text-[#565f89]">{m.taskId}</div>}
      </div>
    );
  }

  if (m.kind === "system_reminder") {
    return (
      <div className="text-xs" data-harness="system_reminder">
        <span className="text-[10px] uppercase tracking-wide text-[#565f89]">system reminder</span>
        <pre className="mt-0.5 whitespace-pre-wrap break-words text-[12px] text-[#565f89]">{m.body}</pre>
      </div>
    );
  }

  if (m.kind === "local_stdout") {
    return (
      <pre className="whitespace-pre-wrap break-words rounded bg-[#1a1b26] p-1.5 font-mono text-[11px] text-[#9ece6a]" data-harness="local_stdout">
        {m.body}
      </pre>
    );
  }

  // plain — caller should normally not reach here, but render text safely.
  return <pre className="whitespace-pre-wrap break-words text-sm text-[#c0caf5]">{m.text}</pre>;
}
