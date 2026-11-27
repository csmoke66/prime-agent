import { openPanel } from "../panels/store";
import { usePalette } from "./shortcuts";
import { IconPlus, Logo } from "./icons";

const TIPS: [string, string][] = [
  ["Ctrl K", "Search sessions, run commands"],
  ["Ctrl B", "Toggle sidebar"],
  ["Ctrl I", "Toggle inspector"],
  ["?", "All keyboard shortcuts"],
];

export function Home() {
  return (
    <div className="grid h-full place-items-center p-6">
      <div className="w-full max-w-sm text-center">
        <div className="mx-auto mb-4 grid h-12 w-12 place-items-center rounded-xl border border-line bg-surface"><Logo size={24} /></div>
        <h1 className="mb-1 text-base font-semibold tracking-tight text-fg">Select an agent to get started</h1>
        <p className="mb-5 text-[13px] text-dim">Running, idle and saved sessions live in the sidebar. Subagents nest under their parent.</p>
        <div className="mb-6 flex justify-center gap-2">
          <button className="bh-btn bh-btn-primary" onClick={() => openPanel("new")}><IconPlus /> New session</button>
          <button className="bh-btn" onClick={() => usePalette.setState({ open: true })}>Search</button>
          <button className="bh-btn" onClick={() => openPanel("import")}>Import</button>
        </div>
        <ul className="mx-auto max-w-xs divide-y divide-line rounded-lg border border-line bg-surface text-left text-[12px]">
          {TIPS.map(([key, label]) => (
            <li key={key} className="flex items-center justify-between px-3 py-2 text-muted"><span>{label}</span><kbd className="bh-kbd">{key}</kbd></li>
          ))}
        </ul>
      </div>
    </div>
  );
}
