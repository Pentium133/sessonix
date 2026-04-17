import { describe, it, expect } from "vitest";
import {
  AGENT_OPTIONS,
  AGENT_PRESETS,
  buildAgentArgs,
  labelForMode,
  parseExtraArgs,
} from "../lib/agentLauncher";

describe("buildAgentArgs", () => {
  const baseline = {
    skipPerms: false,
    resumeSessionId: "",
    prompt: "",
  };

  describe("claude", () => {
    it("returns empty args for plain new session", () => {
      expect(buildAgentArgs({ ...baseline, agentType: "claude", mode: "new" })).toEqual([]);
    });

    it("prepends --dangerously-skip-permissions when skipPerms is set", () => {
      expect(
        buildAgentArgs({ ...baseline, agentType: "claude", mode: "new", skipPerms: true }),
      ).toEqual(["--dangerously-skip-permissions"]);
    });

    it("passes prompt as positional arg only in new mode", () => {
      expect(
        buildAgentArgs({ ...baseline, agentType: "claude", mode: "new", prompt: "do X" }),
      ).toEqual(["do X"]);
      expect(
        buildAgentArgs({ ...baseline, agentType: "claude", mode: "continue", prompt: "do X" }),
      ).toEqual(["--continue"]);
    });

    it("adds --continue in continue mode", () => {
      expect(buildAgentArgs({ ...baseline, agentType: "claude", mode: "continue" })).toEqual([
        "--continue",
      ]);
    });

    it("adds --resume <id> when resumeSessionId is non-empty", () => {
      expect(
        buildAgentArgs({ ...baseline, agentType: "claude", mode: "resume", resumeSessionId: "abc" }),
      ).toEqual(["--resume", "abc"]);
    });

    it("omits --resume when id is empty", () => {
      expect(buildAgentArgs({ ...baseline, agentType: "claude", mode: "resume" })).toEqual([]);
    });

    it("combines skipPerms + continue + skips prompt", () => {
      expect(
        buildAgentArgs({
          ...baseline,
          agentType: "claude",
          mode: "continue",
          skipPerms: true,
          prompt: "ignored",
        }),
      ).toEqual(["--dangerously-skip-permissions", "--continue"]);
    });
  });

  describe("codex", () => {
    it("returns empty args for new mode with no prompt", () => {
      expect(buildAgentArgs({ ...baseline, agentType: "codex", mode: "new" })).toEqual([]);
    });

    it("passes prompt as positional arg in new mode", () => {
      expect(
        buildAgentArgs({ ...baseline, agentType: "codex", mode: "new", prompt: "do X" }),
      ).toEqual(["do X"]);
    });

    it("resume <id> when mode=resume and id provided", () => {
      expect(
        buildAgentArgs({ ...baseline, agentType: "codex", mode: "resume", resumeSessionId: "thr-1" }),
      ).toEqual(["resume", "thr-1"]);
    });

    it("resume --last when mode=last", () => {
      expect(buildAgentArgs({ ...baseline, agentType: "codex", mode: "last" })).toEqual([
        "resume",
        "--last",
      ]);
    });

    it("drops prompt in non-new modes", () => {
      expect(
        buildAgentArgs({ ...baseline, agentType: "codex", mode: "last", prompt: "ignored" }),
      ).toEqual(["resume", "--last"]);
    });
  });

  describe("opencode", () => {
    it("returns empty args for plain new session (launches TUI)", () => {
      expect(buildAgentArgs({ ...baseline, agentType: "opencode", mode: "new" })).toEqual([]);
    });

    it("uses --prompt flag for new mode, not positional", () => {
      expect(
        buildAgentArgs({ ...baseline, agentType: "opencode", mode: "new", prompt: "do X" }),
      ).toEqual(["--prompt", "do X"]);
    });

    it("uses --session <id> for resume mode", () => {
      expect(
        buildAgentArgs({
          ...baseline,
          agentType: "opencode",
          mode: "resume",
          resumeSessionId: "ses_abc",
        }),
      ).toEqual(["--session", "ses_abc"]);
    });

    it("uses --continue for last mode", () => {
      expect(buildAgentArgs({ ...baseline, agentType: "opencode", mode: "last" })).toEqual([
        "--continue",
      ]);
    });
  });

  describe("gemini", () => {
    it("returns empty args without prompt", () => {
      expect(buildAgentArgs({ ...baseline, agentType: "gemini", mode: "new" })).toEqual([]);
    });

    it("passes prompt as positional arg", () => {
      expect(
        buildAgentArgs({ ...baseline, agentType: "gemini", mode: "new", prompt: "hi" }),
      ).toEqual(["hi"]);
    });
  });

  describe("shell and custom", () => {
    it("always returns empty args for shell", () => {
      expect(
        buildAgentArgs({ ...baseline, agentType: "shell", mode: "new", prompt: "ignored" }),
      ).toEqual([]);
    });

    it("always returns empty args for custom", () => {
      expect(
        buildAgentArgs({ ...baseline, agentType: "custom", mode: "new", prompt: "ignored" }),
      ).toEqual([]);
    });
  });
});

describe("parseExtraArgs", () => {
  it("returns [] for empty input", () => {
    expect(parseExtraArgs("")).toEqual([]);
    expect(parseExtraArgs("   ")).toEqual([]);
  });

  it("splits space-separated tokens", () => {
    expect(parseExtraArgs("--model sonnet --verbose")).toEqual([
      "--model",
      "sonnet",
      "--verbose",
    ]);
  });

  it("preserves double-quoted strings with spaces", () => {
    expect(parseExtraArgs('--system "you are helpful"')).toEqual([
      "--system",
      "you are helpful",
    ]);
  });

  it("strips surrounding quotes from tokens", () => {
    expect(parseExtraArgs('"hello world"')).toEqual(["hello world"]);
  });

  it("handles leading and trailing whitespace", () => {
    expect(parseExtraArgs("  --flag  ")).toEqual(["--flag"]);
  });
});

describe("labelForMode", () => {
  it("capitalises by default", () => {
    expect(labelForMode("new")).toBe("New");
    expect(labelForMode("continue")).toBe("Continue");
  });

  it("uses override when provided", () => {
    expect(labelForMode("last", { last: "Last" })).toBe("Last");
  });

  it("falls back to capitalisation when override is undefined", () => {
    expect(labelForMode("resume", { last: "Last" })).toBe("Resume");
  });
});

describe("AGENT_PRESETS / AGENT_OPTIONS", () => {
  it("exports exactly one preset per agent type", () => {
    const types = AGENT_PRESETS.map((p) => p.type);
    expect(new Set(types).size).toBe(types.length);
  });

  it("has option specs only for agents with session modes", () => {
    expect(AGENT_OPTIONS.claude).toBeDefined();
    expect(AGENT_OPTIONS.codex).toBeDefined();
    expect(AGENT_OPTIONS.opencode).toBeDefined();
    expect(AGENT_OPTIONS.gemini).toBeUndefined();
    expect(AGENT_OPTIONS.shell).toBeUndefined();
    expect(AGENT_OPTIONS.custom).toBeUndefined();
  });

  it("every specced agent supports a 'resume' mode with a placeholder", () => {
    for (const spec of Object.values(AGENT_OPTIONS)) {
      if (!spec) continue;
      expect(spec.modes).toContain("resume");
      expect(spec.resumePlaceholder).toBeTruthy();
    }
  });
});
