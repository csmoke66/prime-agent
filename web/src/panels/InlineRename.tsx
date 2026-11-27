import { useState } from "react";

/** Inline rename field: Enter saves, Escape cancels. */
export function InlineRename({ initial, onSave, onCancel }: { initial: string; onSave: (name: string) => void; onCancel: () => void }) {
  const [value, setValue] = useState(initial);
  return (
    <form
      className="flex w-full items-center gap-2"
      onSubmit={(event) => {
        event.preventDefault();
        if (value.trim()) onSave(value.trim());
      }}
      onClick={(event) => event.stopPropagation()}
    >
      <input
        className="bh-input py-1!"
        value={value}
        autoFocus
        aria-label="Session name"
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.stopPropagation();
            onCancel();
          }
        }}
      />
      <button className="bh-btn bh-btn-primary" type="submit">Save</button>
      <button className="bh-btn" type="button" onClick={onCancel}>Cancel</button>
    </form>
  );
}
