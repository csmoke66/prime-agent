import { create } from "zustand";

const KEY = "bh.layout";
type Layout = { sidebarOpen: boolean; sidebarWidth: number; inspectorOpen: boolean; inspectorWidth: number };
const defaults: Layout = { sidebarOpen: true, sidebarWidth: 280, inspectorOpen: false, inspectorWidth: 320 };

function read(): Layout {
  try {
    return { ...defaults, ...JSON.parse(localStorage.getItem(KEY) ?? "{}") };
  } catch {
    return defaults;
  }
}

export const useLayout = create<Layout>(() => read());

export const clampWidth = (value: number) => Math.min(520, Math.max(200, Math.round(value)));

export function setLayout(patch: Partial<Layout>): void {
  useLayout.setState(patch);
  const { sidebarOpen, sidebarWidth, inspectorOpen, inspectorWidth } = useLayout.getState();
  localStorage.setItem(KEY, JSON.stringify({ sidebarOpen, sidebarWidth, inspectorOpen, inspectorWidth }));
}
