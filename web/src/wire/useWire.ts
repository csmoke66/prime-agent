import { useEffect, useState } from "react";
import { wire, type ConnectionStatus, type Frame } from "./client";

export function useConnectionStatus(): ConnectionStatus {
  const [status, setStatus] = useState<ConnectionStatus>(wire.status);
  useEffect(() => wire.onStatus(setStatus), []);
  return status;
}

/** Subscribe to every non-response frame while mounted. */
export function useFrames(handler: (frame: Frame) => void): void {
  useEffect(() => wire.onFrame(handler), [handler]);
}
