import { describe, it, expect } from "vitest";
import { AGENT_PRESETS, AGENT_OPTIONS, buildAgentArgs } from "../lib/agentLauncher";
import { AGENT_COLORS } from "../lib/constants";

describe("Cursor launcher wiring", () => {
  it("exposes a preset with the `agent` binary", () => {
    const preset = AGENT_PRESETS.find((p) => p.type === "cursor");
    expect(preset).toBeDefined();
    expect(preset?.command).toBe("agent");
    expect(preset?.label).toBe("cursor");
    expect(preset?.color).toBe("var(--cursor)");
  });

  it("registers cursor options with new/continue/resume modes", () => {
    const spec = AGENT_OPTIONS.cursor;
    expect(spec).toBeDefined();
    expect(spec?.modes).toEqual(["new", "continue", "resume"]);
    expect(spec?.resumePlaceholder).toContain("uuid");
  });

  it("includes cursor in AGENT_COLORS", () => {
    expect(AGENT_COLORS.cursor).toBe("var(--cursor)");
  });
});

describe("buildAgentArgs(cursor)", () => {
  // Cursor's builder ignores `skipPerms`; keep it on the base fixture only
  // because `BuildArgsOptions` marks it required. It's never asserted on.
  const base = {
    agentType: "cursor" as const,
    skipPerms: false,
    resumeSessionId: "",
    prompt: "",
  };

  it("passes positional prompt for a new session", () => {
    const args = buildAgentArgs({ ...base, mode: "new", prompt: "fix the bug" });
    expect(args).toEqual(["fix the bug"]);
  });

  it("returns empty args for a new session with no prompt", () => {
    const args = buildAgentArgs({ ...base, mode: "new" });
    expect(args).toEqual([]);
  });

  it("emits --continue for continue mode", () => {
    const args = buildAgentArgs({ ...base, mode: "continue" });
    expect(args).toEqual(["--continue"]);
  });

  it("emits --resume <uuid> for resume mode with an id", () => {
    const args = buildAgentArgs({
      ...base,
      mode: "resume",
      resumeSessionId: "6ffd78e9-b552-49a7-9abf-2b00327c2764",
    });
    expect(args).toEqual(["--resume", "6ffd78e9-b552-49a7-9abf-2b00327c2764"]);
  });

  it("drops resume arg when id is empty", () => {
    const args = buildAgentArgs({ ...base, mode: "resume", resumeSessionId: "" });
    expect(args).toEqual([]);
  });

  it("ignores prompt when not in new mode", () => {
    const args = buildAgentArgs({
      ...base,
      mode: "continue",
      prompt: "should be ignored",
    });
    expect(args).toEqual(["--continue"]);
  });
});
