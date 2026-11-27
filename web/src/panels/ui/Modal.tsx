import { useEffect, useId, useRef, type ReactNode } from "react";

type Props = {
  title: ReactNode;
  subtitle?: ReactNode;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  /** `modal` is a centred dialog; `drawer` slides in from the right (full screen on phones). */
  variant?: "modal" | "drawer";
  size?: "sm" | "md" | "lg" | "xl";
  actions?: ReactNode;
};

const SIZES = { sm: "max-w-md", md: "max-w-xl", lg: "max-w-3xl", xl: "max-w-5xl" } as const;
const FOCUSABLE = 'a[href],button:not([disabled]),textarea,input,select,[tabindex]:not([tabindex="-1"])';

/** Shared dialog primitive: focus trap, Escape / backdrop close, restores focus on close. */
export function Modal({ title, subtitle, onClose, children, footer, variant = "modal", size = "md", actions }: Props) {
  const ref = useRef<HTMLDivElement>(null);
  const titleId = useId();

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const root = ref.current;
    const first = root?.querySelector<HTMLElement>("[data-autofocus]") ?? root?.querySelector<HTMLElement>(FOCUSABLE);
    (first ?? root)?.focus();
    return () => previous?.focus?.();
  }, []);

  const onKeyDown = (event: React.KeyboardEvent) => {
    if (event.key === "Escape") {
      event.stopPropagation();
      onClose();
      return;
    }
    if (event.key !== "Tab" || !ref.current) return;
    const items = [...ref.current.querySelectorAll<HTMLElement>(FOCUSABLE)].filter((el) => el.offsetParent !== null);
    if (items.length === 0) return;
    const first = items[0];
    const last = items[items.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  };

  const drawer = variant === "drawer";
  return (
    <div
      className={`fixed inset-0 z-50 flex bg-black/60 backdrop-blur-[2px] bh-fade-in ${drawer ? "justify-end" : "items-end justify-center p-0 sm:items-center sm:p-6"}`}
      onMouseDown={(event) => event.target === event.currentTarget && onClose()}
    >
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onKeyDown={onKeyDown}
        className={
          drawer
            ? `bh-fade-in flex h-full w-full ${SIZES[size]} flex-col border-l border-grid/50 bg-surface shadow-2xl`
            : `flex max-h-[92vh] w-full ${SIZES[size]} flex-col rounded-t-2xl border border-grid/50 bg-surface shadow-2xl sm:rounded-2xl`
        }
      >
        <header className="flex items-start gap-3 border-b border-grid/40 px-4 py-3">
          <div className="min-w-0 flex-1">
            <h2 id={titleId} className="truncate text-base font-semibold tracking-tight">{title}</h2>
            {subtitle ? <p className="mt-0.5 text-xs text-muted">{subtitle}</p> : null}
          </div>
          {actions}
          <button className="bh-btn px-2! py-1!" onClick={onClose} aria-label="Close dialog">&#x2715;</button>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">{children}</div>
        {footer ? <footer className="flex flex-wrap items-center justify-end gap-2 border-t border-grid/40 px-4 py-3">{footer}</footer> : null}
      </div>
    </div>
  );
}
