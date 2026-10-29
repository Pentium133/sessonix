// @vitest-environment node
// @ts-nocheck -- exercises a Node plugin (node:fs, process); the project ships no @types/node.
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { mkdtempSync, readFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const PLUGIN = "../../src-tauri/resources/opencode-status-plugin.js";

type Hooks = { event: (input: { event: unknown }) => Promise<void> };

let home: string;
const ENV_KEYS = ["HOME", "SESSONIX_PTY_ID", "SESSONIX_AGENT"] as const;
const savedEnv = Object.fromEntries(ENV_KEYS.map((k) => [k, process.env[k]]));

async function loadHooks(): Promise<Hooks> {
  vi.resetModules();
  const mod = await import(/* @vite-ignore */ PLUGIN);
  return mod.default.server();
}

function statusFile(): Record<string, unknown> | null {
  const path = join(home, ".sessonix", "hooks", "42.json");
  return existsSync(path) ? JSON.parse(readFileSync(path, "utf8")) : null;
}

const status = (sessionID: string, type: string) => ({
  event: { type: "session.status", properties: { sessionID, status: { type } } },
});

describe("opencode status plugin", () => {
  beforeEach(() => {
    home = mkdtempSync(join(tmpdir(), "sessonix-oc-"));
    process.env.HOME = home;
    process.env.SESSONIX_PTY_ID = "42";
    process.env.SESSONIX_AGENT = "opencode";
  });

  afterEach(() => {
    // Assign per key: replacing process.env detaches it from the real environment.
    for (const k of ENV_KEYS) {
      if (savedEnv[k] === undefined) delete process.env[k];
      else process.env[k] = savedEnv[k];
    }
  });

  it("busy session marks running, idle marks idle", async () => {
    const hooks = await loadHooks();
    await hooks.event(status("ses_a", "busy"));
    expect(statusFile()).toMatchObject({ status: "running", pty_id: 42, session_id: "ses_a" });
    await hooks.event(status("ses_a", "idle"));
    expect(statusFile()).toMatchObject({ status: "idle" });
  });

  it("subagent idle keeps running while the parent is busy", async () => {
    const hooks = await loadHooks();
    await hooks.event(status("ses_parent", "busy"));
    await hooks.event(status("ses_child", "busy"));
    await hooks.event(status("ses_child", "idle"));
    expect(statusFile()).toMatchObject({ status: "running" });
    await hooks.event(status("ses_parent", "idle"));
    expect(statusFile()).toMatchObject({ status: "idle" });
  });

  it("retry counts as running", async () => {
    const hooks = await loadHooks();
    await hooks.event(status("ses_a", "retry"));
    expect(statusFile()).toMatchObject({ status: "running" });
  });

  it("permission asked and replied", async () => {
    const hooks = await loadHooks();
    await hooks.event(status("ses_a", "busy"));
    await hooks.event({ event: { type: "permission.asked", properties: { sessionID: "ses_a" } } });
    expect(statusFile()).toMatchObject({ status: "waiting_permission" });
    await hooks.event({ event: { type: "permission.replied", properties: { sessionID: "ses_a" } } });
    expect(statusFile()).toMatchObject({ status: "running" });
  });

  it("session error is reported with its name", async () => {
    const hooks = await loadHooks();
    await hooks.event({
      event: { type: "session.error", properties: { sessionID: "ses_a", error: { name: "ProviderAuthError" } } },
    });
    expect(statusFile()).toMatchObject({ status: "error", error: "ProviderAuthError" });
  });

  it("error survives the idle that follows it", async () => {
    const hooks = await loadHooks();
    await hooks.event(status("ses_a", "busy"));
    await hooks.event({
      event: { type: "session.error", properties: { sessionID: "ses_a", error: { name: "APIError" } } },
    });
    await hooks.event(status("ses_a", "idle"));
    expect(statusFile()).toMatchObject({ status: "error", error: "APIError" });
    await hooks.event(status("ses_a", "busy"));
    expect(statusFile()).toMatchObject({ status: "running" });
  });

  it("subagent error while the parent is busy keeps running", async () => {
    const hooks = await loadHooks();
    await hooks.event(status("ses_parent", "busy"));
    await hooks.event(status("ses_child", "busy"));
    await hooks.event({
      event: { type: "session.error", properties: { sessionID: "ses_child", error: { name: "APIError" } } },
    });
    expect(statusFile()).toMatchObject({ status: "running" });
  });

  it("status without sessionID does not pin running", async () => {
    const hooks = await loadHooks();
    await hooks.event({ event: { type: "session.status", properties: { status: { type: "busy" } } } });
    await hooks.event(status("ses_a", "busy"));
    await hooks.event(status("ses_a", "idle"));
    expect(statusFile()).toMatchObject({ status: "idle" });
  });

  it("deprecated session.idle also ends the turn", async () => {
    const hooks = await loadHooks();
    await hooks.event(status("ses_a", "busy"));
    await hooks.event({ event: { type: "session.idle", properties: { sessionID: "ses_a" } } });
    expect(statusFile()).toMatchObject({ status: "idle" });
  });

  it("does nothing outside a Sessonix opencode PTY", async () => {
    process.env.SESSONIX_AGENT = "claude";
    const hooks = await loadHooks();
    await hooks.event(status("ses_a", "busy"));
    expect(statusFile()).toBeNull();
  });

  it("ignores malformed events", async () => {
    const hooks = await loadHooks();
    await expect(hooks.event({ event: null })).resolves.toBeUndefined();
    await expect(hooks.event({ event: { type: "session.status" } })).resolves.toBeUndefined();
  });

  it("exposes a V2 definition", async () => {
    vi.resetModules();
    const mod = await import(/* @vite-ignore */ PLUGIN);
    expect(mod.default.id).toBe("sessonix-status");
    expect(typeof mod.default.setup).toBe("function");
  });
});
