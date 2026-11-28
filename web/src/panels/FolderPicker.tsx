import { useEffect, useState } from "react";
import { rest } from "./api";
import { errorText } from "./toasts";

/** A directory browser: click a folder to open it, ".." to go up, "Use this folder" to choose. */
export function FolderPicker({ start, onPick, onCancel }: { start: string; onPick: (dir: string) => void; onCancel: () => void }) {
  const [dir, setDir] = useState(start || "/");
  const [dirs, setDirs] = useState<string[]>([]);
  const [error, setError] = useState("");

  useEffect(() => {
    let live = true;
    rest.files(dir, "").then(
      (result) => {
        if (!live) return;
        setError("");
        setDirs(result.entries.filter((e) => e.isDir).map((e) => String(e.path)));
      },
      (reason) => live && setError(errorText(reason)),
    );
    return () => { live = false; };
  }, [dir]);

  const parent = dir === "/" ? null : dir.replace(/\/[^/]*\/?$/, "") || "/";
  const name = (path: string) => path.split("/").filter(Boolean).pop() ?? "/";

  return (
    <div className="rounded border border-line bg-surface">
      <div className="flex items-center gap-2 border-b border-line px-2 py-1.5">
        <span className="min-w-0 flex-1 truncate font-mono text-[12px]" title={dir}>{dir}</span>
      </div>
      <ul className="max-h-56 overflow-y-auto py-1 text-[12px]">
        {parent ? <li><button type="button" className="w-full px-2 py-0.5 text-left font-mono text-muted hover:bg-selected" onClick={() => setDir(parent)}>..</button></li> : null}
        {dirs.map((path) => (
          <li key={path}><button type="button" className="w-full truncate px-2 py-0.5 text-left font-mono hover:bg-selected" onClick={() => setDir(path)}>{name(path)}/</button></li>
        ))}
        {!dirs.length && !error ? <li className="px-2 py-1 text-dim">No folders here</li> : null}
        {error ? <li className="px-2 py-1 text-error">{error}</li> : null}
      </ul>
      <div className="flex justify-end gap-2 border-t border-line px-2 py-1.5">
        <button type="button" className="bh-btn" onClick={onCancel}>Cancel</button>
        <button type="button" className="bh-btn bh-btn-primary" onClick={() => onPick(dir)}>Use this folder</button>
      </div>
    </div>
  );
}
