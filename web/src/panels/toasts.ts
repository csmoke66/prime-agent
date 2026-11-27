import { create } from "zustand";

export type ToastTone = "info" | "success" | "error";
export type Toast = { id: number; tone: ToastTone; text: string };

type ToastState = { toasts: Toast[]; push: (tone: ToastTone, text: string) => void; dismiss: (id: number) => void };
let counter = 0;

export const useToasts = create<ToastState>((set) => ({
  toasts: [],
  push: (tone, text) => {
    const id = ++counter;
    set((state) => ({ toasts: [...state.toasts.slice(-3), { id, tone, text }] }));
    window.setTimeout(() => set((state) => ({ toasts: state.toasts.filter((t) => t.id !== id) })), tone === "error" ? 8000 : 4000);
  },
  dismiss: (id) => set((state) => ({ toasts: state.toasts.filter((t) => t.id !== id) })),
}));

export const toast = {
  info: (text: string) => useToasts.getState().push("info", text),
  success: (text: string) => useToasts.getState().push("success", text),
  error: (text: string) => useToasts.getState().push("error", text),
};

export function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
