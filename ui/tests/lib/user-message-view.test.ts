import { describe, it, expect } from "vitest";
import { userMessageView } from "@/lib/harness-message";

/** UserMessage should read like the Live tab: a slash-command shows a clean chip
 *  (not raw <command-message> tags), and a real message renders as markdown. */
describe("userMessageView — clean chip vs markdown body", () => {
  it("turns a slash-command wrapper into a clean command chip, no raw tags", () => {
    const text =
      "<command-message>openstory:scan</command-message><command-name>/openstory:scan</command-name><command-args>this branch</command-args>";
    const v = userMessageView(text);
    expect(v.command).toBe("/openstory:scan this branch");
    expect(v.body).toBe("");
    expect(v.command).not.toContain("<command");
  });

  it("a command with no args is just the command", () => {
    const v = userMessageView("<command-name>/loop</command-name>");
    expect(v.command).toBe("/loop");
  });

  it("passes an ordinary message through as a markdown body", () => {
    const v = userMessageView("# scan\n\nA pre-share check.");
    expect(v.command).toBeNull();
    expect(v.body).toBe("# scan\n\nA pre-share check.");
  });

  it("surfaces a system-reminder body without the wrapper tag", () => {
    const v = userMessageView("<system-reminder>Do the thing</system-reminder>");
    expect(v.command).toBeNull();
    expect(v.body).toBe("Do the thing");
  });
});
