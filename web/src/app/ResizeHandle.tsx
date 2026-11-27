import { useState } from "react";

/** Vertical drag handle; `side` is the edge of the panel it resizes, `onResize` receives the new pixel width. */
export function ResizeHandle({ side, width, onResize, label }: { side: "right" | "left"; width: number; onResize: (width: number) => void; label: string }) {
  const [drag, setDrag] = useState(false);
  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      tabIndex={0}
      className="bh-handle hidden md:block"
      data-drag={drag}
      onPointerDown={(e) => {
        e.currentTarget.setPointerCapture(e.pointerId);
        setDrag(true);
      }}
      onPointerMove={(e) => {
        if (!drag) return;
        const box = e.currentTarget.parentElement!.getBoundingClientRect();
        onResize(side === "right" ? e.clientX - box.left : box.right - e.clientX);
      }}
      onPointerUp={() => setDrag(false)}
      onKeyDown={(e) => {
        if (e.key === "ArrowLeft") onResize(width + (side === "right" ? -16 : 16));
        if (e.key === "ArrowRight") onResize(width + (side === "right" ? 16 : -16));
      }}
    />
  );
}
