import { Channel } from "@tauri-apps/api/core";
import { subscribePtyOutput } from "./api";
import { writeToTerminal } from "./terminalPool";

const HEADER_BYTES = 4;

/** Frame layout: [u32 little-endian session id][raw output bytes]. */
export function decodeFrame(
  frame: ArrayBuffer | ArrayBufferView
): { id: number; data: Uint8Array } | null {
  const view = ArrayBuffer.isView(frame)
    ? frame
    : new Uint8Array(frame);
  if (view.byteLength < HEADER_BYTES) return null;
  const id = new DataView(view.buffer, view.byteOffset, view.byteLength).getUint32(0, true);
  const data = new Uint8Array(
    view.buffer,
    view.byteOffset + HEADER_BYTES,
    view.byteLength - HEADER_BYTES
  );
  return { id, data };
}

let subscription: Promise<void> | null = null;

/** Subscribes to the backend output channel once per page load; retries after failure. */
export function ensurePtyOutputSubscribed(): Promise<void> {
  if (!subscription) {
    const channel = new Channel<ArrayBuffer>();
    channel.onmessage = (frame) => {
      const decoded = decodeFrame(frame);
      if (!decoded || decoded.data.length === 0) return;
      writeToTerminal(decoded.id, decoded.data);
    };
    subscription = subscribePtyOutput(channel).catch((err) => {
      subscription = null;
      throw err;
    });
  }
  return subscription;
}
