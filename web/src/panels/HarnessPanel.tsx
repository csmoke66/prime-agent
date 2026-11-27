import { useMemo, useState } from "react";
import { Modal } from "./ui/Modal";
import { Badge, ConfirmButton, Empty, ErrorLine, SearchInput, Spinner } from "./ui/bits";
import { fuzzyFilter } from "./ui/format";
import { rest, type HarnessEntry } from "./api";
import { useAsync } from "./useAsync";
import { toast, errorText } from "./toasts";

/** `/harness`: browse continual-harness entries (scope → kind → path) and delete them. */
export function HarnessPanel({ sessionFile, onClose }: { sessionFile?: string; onClose: () => void }) {
  const data = useAsync(() => rest.harness(sessionFile), [sessionFile]);
  const [search, setSearch] = useState("");
  const [selected, setSelected] = useState<HarnessEntry | null>(null);
  const entries = useMemo(
    () => fuzzyFilter(data.data?.entries ?? [], search, (e) => `${e.scope} ${e.kind} ${e.path ?? ""} ${e.title ?? ""} ${e.id}`),
    [data.data, search],
  );
  const groups = useMemo(() => {
    const map = new Map<string, HarnessEntry[]>();
    for (const e of entries) map.set(`${e.scope} · ${e.kind}`, [...(map.get(`${e.scope} · ${e.kind}`) ?? []), e]);
    return [...map.entries()].sort(([a], [b]) => a.localeCompare(b));
  }, [entries]);

  const remove = async (entry: HarnessEntry) => {
    try {
      await rest.deleteHarness(entry, sessionFile);
      setSelected(null);
      data.reload();
      toast.success("Harness entry deleted");
    } catch (reason) {
      toast.error(errorText(reason));
    }
  };

  return (
    <Modal title="Harness" subtitle="Local and global prompt notes, memories, skills and subagent specs." size="xl" variant="drawer" onClose={onClose}>
      <div className="flex flex-col gap-3">
        <SearchInput value={search} onChange={setSearch} placeholder="Search entries…" autoFocus />
        <ErrorLine>{data.error}</ErrorLine>
        {data.loading && !data.data && <Spinner label="Loading entries" />}
        {!data.loading && entries.length === 0 && !data.error && <Empty title="No harness entries" />}
        <div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_minmax(0,1.3fr)]">
          <div className="flex flex-col gap-3">
            {groups.map(([group, list]) => (
              <section key={group}>
                <h3 className="mb-1 text-xs font-medium uppercase tracking-wider text-dim">{group} · {list.length}</h3>
                <ul className="flex flex-col gap-1">
                  {list.map((e) => (
                    <li key={`${e.scope}/${e.kind}/${e.id}`}>
                      <button onClick={() => setSelected(e)} className={`w-full truncate rounded-lg px-2 py-1.5 text-left text-sm ${selected === e ? "bg-selected" : "hover:bg-selected/60"}`}>
                        {e.title || e.id}
                        {e.path && e.path !== "general" && <span className="ml-2 text-xs text-dim">{e.path}</span>}
                      </button>
                    </li>
                  ))}
                </ul>
              </section>
            ))}
          </div>
          {selected && (
            <article className="bh-card flex min-w-0 flex-col gap-2 p-3">
              <div className="flex flex-wrap items-center gap-2">
                <h3 className="min-w-0 flex-1 truncate font-medium">{selected.title || selected.id}</h3>
                <Badge tone={selected.scope === "global" ? "primary" : "info"}>{selected.scope}</Badge>
                <Badge>{selected.kind}</Badge>
              </div>
              <pre className="max-h-[50vh] overflow-auto whitespace-pre-wrap break-words rounded-lg bg-panel p-3 text-xs">{selected.content ?? JSON.stringify(selected, null, 2)}</pre>
              <ConfirmButton className="self-end" label="Delete entry" confirmLabel="Really delete?" onConfirm={() => void remove(selected)} />
            </article>
          )}
        </div>
      </div>
    </Modal>
  );
}
