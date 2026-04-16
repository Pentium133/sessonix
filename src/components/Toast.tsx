import { useState, useEffect, useCallback } from "react";

export interface ToastMessage {
  id: number;
  text: string;
  type: "error" | "info" | "success";
  action?: { label: string; onClick: () => void };
}

let nextId = 0;
let addToastFn: ((
  text: string,
  type: ToastMessage["type"],
  action?: ToastMessage["action"]
) => void) | null = null;

export function showToast(
  text: string,
  type: ToastMessage["type"] = "error",
  action?: ToastMessage["action"]
) {
  addToastFn?.(text, type, action);
}

const MAX_TOASTS = 3;
const AUTO_DISMISS_MS = 5000;

export default function ToastContainer() {
  const [toasts, setToasts] = useState<ToastMessage[]>([]);

  const addToast = useCallback(
    (text: string, type: ToastMessage["type"], action?: ToastMessage["action"]) => {
      const id = nextId++;
      setToasts((prev) => [...prev.slice(-(MAX_TOASTS - 1)), { id, text, type, action }]);
      setTimeout(() => {
        setToasts((prev) => prev.filter((t) => t.id !== id));
      }, AUTO_DISMISS_MS);
    },
    []
  );

  useEffect(() => {
    addToastFn = addToast;
    return () => {
      addToastFn = null;
    };
  }, [addToast]);

  const dismiss = (id: number) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  };

  if (toasts.length === 0) return null;

  return (
    <div className="toast-container">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className={`toast toast-${toast.type}`}
        >
          <span onClick={() => dismiss(toast.id)}>{toast.text}</span>
          {toast.action && (
            <button
              className="toast-action"
              onClick={(e) => {
                e.stopPropagation();
                toast.action!.onClick();
                dismiss(toast.id);
              }}
            >
              {toast.action.label}
            </button>
          )}
        </div>
      ))}
    </div>
  );
}
