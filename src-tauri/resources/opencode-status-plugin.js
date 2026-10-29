// Sessonix status plugin for OpenCode. Installed and overwritten by Sessonix
// (~/.config/opencode/plugins/sessonix-status.js); edits will be lost.
//
// Writes the session status of the OpenCode process Sessonix spawned
// (SESSONIX_PTY_ID, SESSONIX_AGENT=opencode) to ~/.sessonix/hooks/<pty_id>.json,
// the same file Sessonix's hook handler writes for other agents. Does nothing
// in OpenCode processes Sessonix didn't start.
//
// Events: https://github.com/anomalyco/opencode (plugins.mdx,
// schema/session-status-event.ts). Ships both plugin APIs: V1 calls server(),
// V2 reads id + setup().
import { mkdirSync, renameSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

// Sessions currently working. Subagents run as further sessions, so the turn
// is over only when none is busy.
const busy = new Set();
// The turn ended on an error: keep showing it over the idle that follows.
let errored = false;

function write(event, status, sessionID, error) {
  const ptyId = process.env.SESSONIX_PTY_ID;
  if (!ptyId || !/^\d+$/.test(ptyId) || process.env.SESSONIX_AGENT !== "opencode") return;
  try {
    const dir = join(homedir(), ".sessonix", "hooks");
    mkdirSync(dir, { recursive: true });
    const nowMs = Date.now();
    const out = {
      event,
      status,
      session_id: sessionID ?? "",
      pty_id: Number(ptyId),
      ts: Math.floor(nowMs / 1000),
      ts_ms: nowMs,
    };
    if (error) out.error = error;
    const path = join(dir, `${ptyId}.json`);
    const tmp = `${path}.${process.pid}.tmp`;
    writeFileSync(tmp, JSON.stringify(out));
    renameSync(tmp, path);
  } catch {
    // Never break OpenCode over a status file.
  }
}

function onEvent(event) {
  if (!event || typeof event.type !== "string") return;
  const props = event.properties ?? event;
  const sessionID = props.sessionID;
  const idle = () => {
    busy.delete(sessionID);
    if (busy.size === 0 && !errored) write(event.type, "idle", sessionID);
  };
  switch (event.type) {
    case "session.status": {
      if (!sessionID) break;
      const type = props.status?.type;
      if (type === "busy" || type === "retry") {
        busy.add(sessionID);
        errored = false;
        write(event.type, "running", sessionID);
      } else if (type === "idle") {
        idle();
      }
      break;
    }
    // Deprecated in favour of session.status, still emitted by older versions.
    case "session.idle":
      if (sessionID) idle();
      break;
    case "permission.asked":
      write(event.type, "waiting_permission", sessionID);
      break;
    case "permission.replied":
      write(event.type, "running", sessionID);
      break;
    case "session.error":
      busy.delete(sessionID);
      // A subagent failing doesn't end the parent's turn.
      if (busy.size === 0) {
        errored = true;
        write(event.type, "error", sessionID, props.error?.name || "agent error");
      }
      break;
  }
}

function safeOnEvent(event) {
  try {
    onEvent(event);
  } catch {
    // Malformed event: ignore.
  }
}

export default {
  id: "sessonix-status",
  // V2
  setup(ctx) {
    const controller = new AbortController();
    void (async () => {
      try {
        for await (const event of ctx.event.subscribe({ signal: controller.signal })) {
          safeOnEvent(event);
        }
      } catch {
        // Subscription closed.
      }
    })();
    return () => controller.abort();
  },
  // V1
  async server() {
    return {
      event: async ({ event }) => safeOnEvent(event),
    };
  },
};
