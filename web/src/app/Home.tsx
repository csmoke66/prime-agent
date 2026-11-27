import { openPanel } from "../panels/store";

export function Home() {
  return (
    <div className="grid h-full place-items-center p-6 text-center">
      <div className="max-w-sm">
        <div className="mb-2 text-3xl text-primary-soft">&#x2B22;</div>
        <p className="mb-1 text-sm text-fg">Select an agent in the sidebar</p>
        <p className="mb-4 text-xs text-dim">Running, idle and saved sessions are listed on the left. Subagents nest under their parent.</p>
        <div className="flex justify-center gap-2">
          <button className="bh-btn bh-btn-primary" onClick={() => openPanel("new")}>+ New session</button>
          <button className="bh-btn" onClick={() => openPanel("import")}>Import</button>
        </div>
      </div>
    </div>
  );
}
