/** Renders a harness-wrapper user message (slash command, task notification,
 *  system reminder, local stdout) as a clean, structured block instead of raw
 *  XML — while keeping the full content visible (never truncated). */

import { classifyHarnessMessage } from "@/lib/harness-message";

export function HarnessMessageBlock({ text }: { text: string }) {
  const m = classifyHarnessMessage(text);

  if (m.kind === "slash_command") {
    return (
      <div className="text-sm" data-harness="slash_command">
        <span className="inline-flex items-center gap-1 rounded bg-[#7aa2f7]/15 px-1.5 py-0.5 font-mono text-[12px] text-[color:var(--accent)]">
          <span className="opacity-70">⌘</span>/{m.command}
        </span>
        {m.args && (
          <pre className="mt-1 whitespace-pre-wrap break-words text-[13px] text-[color:var(--text)]">{m.args}</pre>
        )}
        {m.stdout && (
          <pre className="mt-1 whitespace-pre-wrap break-words rounded bg-[color:var(--bg)] p-1.5 font-mono text-[11px] text-[color:var(--green)]">{m.stdout}</pre>
        )}
      </div>
    );
  }

  if (m.kind === "task_notification") {
    return (
      <div className="text-sm" data-harness="task_notification">
        <span className="inline-flex items-center gap-1 rounded bg-[#bb9af7]/15 px-1.5 py-0.5 text-[11px] text-[color:var(--purple)]">
          ⚙ background task{m.status ? ` — ${m.status}` : ""}
        </span>
        {m.summary && <div className="mt-1 text-[13px] text-[color:var(--text)]">{m.summary}</div>}
        {m.taskId && <div className="mt-0.5 font-mono text-[10px] text-[color:var(--text-muted)]">{m.taskId}</div>}
      </div>
    );
  }

  if (m.kind === "system_reminder") {
    return (
      <div className="text-xs" data-harness="system_reminder">
        <span className="text-[10px] uppercase tracking-wide text-[color:var(--text-muted)]">system reminder</span>
        <pre className="mt-0.5 whitespace-pre-wrap break-words text-[12px] text-[color:var(--text-muted)]">{m.body}</pre>
      </div>
    );
  }

  if (m.kind === "local_stdout") {
    return (
      <pre className="whitespace-pre-wrap break-words rounded bg-[color:var(--bg)] p-1.5 font-mono text-[11px] text-[color:var(--green)]" data-harness="local_stdout">
        {m.body}
      </pre>
    );
  }

  // plain — caller should normally not reach here, but render text safely.
  return <pre className="whitespace-pre-wrap break-words text-sm text-[color:var(--text)]">{m.text}</pre>;
}
