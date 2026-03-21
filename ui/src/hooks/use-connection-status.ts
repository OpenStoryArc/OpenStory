import { connectionStatus$, type ConnectionStatus } from "@/streams/connection";
import { useObservable } from "./use-observable";

export function useConnectionStatus(): ConnectionStatus {
  return useObservable(connectionStatus$(), "disconnected");
}
