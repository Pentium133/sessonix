import { create } from "zustand";
import {
  getTelegramStatus,
  type TelegramStatus,
  type TelegramStatusKind,
} from "../lib/api";

interface TelegramState {
  status: TelegramStatusKind;
  message: string | null;
  ownerChatId: number | null;
  hasToken: boolean;
  loaded: boolean;
  refresh: () => Promise<void>;
  setFromStatus: (s: TelegramStatus) => void;
}

export const useTelegramStore = create<TelegramState>((set) => ({
  status: "disabled",
  message: null,
  ownerChatId: null,
  hasToken: false,
  loaded: false,

  setFromStatus: (s) =>
    set({
      status: s.status,
      message: s.message,
      ownerChatId: s.owner_chat_id,
      hasToken: s.has_token,
      loaded: true,
    }),

  refresh: async () => {
    try {
      const s = await getTelegramStatus();
      set({
        status: s.status,
        message: s.message,
        ownerChatId: s.owner_chat_id,
        hasToken: s.has_token,
        loaded: true,
      });
    } catch (e) {
      console.warn("telegram status refresh failed", e);
    }
  },
}));
