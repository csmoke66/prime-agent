import type { ReactNode } from "react";

const base = { width: 16, height: 16, viewBox: "0 0 16 16", fill: "none", stroke: "currentColor", strokeWidth: 1.5, strokeLinecap: "round", strokeLinejoin: "round", "aria-hidden": true } as const;
const icon = (children: ReactNode) => () => <svg {...base}>{children}</svg>;

export const IconSidebar = icon(<><rect x="2" y="3" width="12" height="10" rx="2" /><path d="M6 3v10" /></>);
export const IconInspector = icon(<><rect x="2" y="3" width="12" height="10" rx="2" /><path d="M10 3v10" /></>);
export const IconSearch = icon(<><circle cx="7" cy="7" r="4" /><path d="m10 10 3 3" /></>);
export const IconPlus = icon(<path d="M8 3v10M3 8h10" />);
export const IconSettings = icon(<><circle cx="8" cy="8" r="2" /><path d="M8 2v1.5M8 12.5V14M2 8h1.5M12.5 8H14M3.8 3.8l1 1M11.2 11.2l1 1M12.2 3.8l-1 1M4.8 11.2l-1 1" /></>);
export const IconHeartbeat = icon(<path d="M1.5 8h3l1.5-4 3 8 1.5-4h4" />);
export const IconChevronsLeft = icon(<path d="m9 4-4 4 4 4M13 4 9 8l4 4" />);

/** Honey hexagon used as the product mark. */
export function Logo({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" aria-hidden="true">
      <path d="M8 1.2 14 4.6v6.8L8 14.8 2 11.4V4.6z" fill="var(--bh-primary)" />
      <path d="M8 5.2 10.6 6.7v2.6L8 10.8 5.4 9.3V6.7z" fill="var(--bh-bg)" />
    </svg>
  );
}

export const IconTextSearch = icon(<><path d="M2 4h12M2 8h5M2 12h5" /><circle cx="11" cy="10.5" r="2" /><path d="m12.5 12 1.5 1.5" /></>);
export const IconClock = icon(<><circle cx="8" cy="8" r="5.5" /><path d="M8 5v3l2 1.5" /></>);
export const IconBranch = icon(<><circle cx="4" cy="3.5" r="1.5" /><circle cx="4" cy="12.5" r="1.5" /><circle cx="12" cy="8" r="1.5" /><path d="M4 5v6M4 8h6.5" /></>);
export const IconTarget = icon(<><circle cx="8" cy="8" r="5.5" /><circle cx="8" cy="8" r="2" /></>);
export const IconUp = icon(<path d="m4 10 4-4 4 4" />);
export const IconDown = icon(<path d="m4 6 4 4 4-4" />);
export const IconEdit = icon(<path d="M3 13h2.5L13 5.5 10.5 3 3 10.5zM9 4.5 11.5 7" />);
export const IconRestore = icon(<path d="M3 8h8a2.5 2.5 0 0 1 0 5H8M5.5 5.5 3 8l2.5 2.5" />);
export const IconX = icon(<path d="m4 4 8 8M12 4l-8 8" />);
export const IconMore = icon(<><circle cx="3.5" cy="8" r="0.8" fill="currentColor" /><circle cx="8" cy="8" r="0.8" fill="currentColor" /><circle cx="12.5" cy="8" r="0.8" fill="currentColor" /></>);
