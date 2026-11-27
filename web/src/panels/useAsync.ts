import { useCallback, useEffect, useRef, useState } from "react";
import { errorText } from "./toasts";

export type AsyncState<T> = { data: T | undefined; error: string; loading: boolean; reload: () => void };

/** Run an async loader on mount (and whenever `deps` change); ignores stale results. */
export function useAsync<T>(loader: () => Promise<T>, deps: readonly unknown[]): AsyncState<T> {
  const [data, setData] = useState<T>();
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [nonce, setNonce] = useState(0);
  const loaderRef = useRef(loader);
  loaderRef.current = loader;
  useEffect(() => {
    let live = true;
    setLoading(true);
    loaderRef.current().then(
      (value) => {
        if (!live) return;
        setData(value);
        setError("");
        setLoading(false);
      },
      (reason) => {
        if (!live) return;
        setError(errorText(reason));
        setLoading(false);
      },
    );
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, nonce]);
  const reload = useCallback(() => setNonce((n) => n + 1), []);
  return { data, error, loading, reload };
}
