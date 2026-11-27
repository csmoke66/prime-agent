import { useEffect, useState } from "react";

// Hash routes keep the embedded static server trivial: `#/sessions`, `#/session/<activeSessionId>`,
// `#/agents`.
export type Route =
  | { name: "sessions" }
  | { name: "agents" }
  | { name: "session"; activeSessionId: string };

export function parseRoute(hash: string): Route {
  const parts = hash.replace(/^#\/?/, "").split("/").filter(Boolean);
  if (parts[0] === "session" && parts[1]) return { name: "session", activeSessionId: decodeURIComponent(parts[1]) };
  if (parts[0] === "agents") return { name: "agents" };
  return { name: "sessions" };
}

export function hrefFor(route: Route): string {
  switch (route.name) {
    case "session":
      return `#/session/${encodeURIComponent(route.activeSessionId)}`;
    case "agents":
      return "#/agents";
    case "sessions":
      return "#/sessions";
  }
}

export function navigate(route: Route): void {
  location.hash = hrefFor(route);
}

export function useRoute(): Route {
  const [route, setRoute] = useState(() => parseRoute(location.hash));
  useEffect(() => {
    const onChange = () => setRoute(parseRoute(location.hash));
    window.addEventListener("hashchange", onChange);
    return () => window.removeEventListener("hashchange", onChange);
  }, []);
  return route;
}
