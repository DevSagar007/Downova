// Service-client boundary: the ONLY module family in the UI allowed to reach the desktop shell
// (Tauri) and, through it, the background service. Features depend on the `ServiceClient`
// interface, never on transport details. Contract types come from @dm/contracts.
import type { ErrorData, HelloResult } from "@dm/contracts";
import { invoke, isTauri } from "@tauri-apps/api/core";

export type ConnectionState =
  | { status: "connected"; hello: HelloResult }
  | {
      status: "unavailable";
      /** Why the UI cannot talk to the service. Presented to the user honestly. */
      reason: "not_implemented" | "not_running" | "incompatible_version";
      error: ErrorData | null;
    };

export interface ServiceClient {
  /** Calls service.v1.hello. Never throws for expected unavailability. */
  connect(): Promise<ConnectionState>;
}

/**
 * Foundation-phase client. The service IPC (named pipe / Unix socket, JSON-RPC 2.0) does not
 * exist yet (milestone B0), so this reports `not_implemented` instead of pretending to connect.
 * Replace with the Tauri-command-backed client when `dm-service` exposes service.v1.hello.
 */
export class FoundationServiceClient implements ServiceClient {
  async connect(): Promise<ConnectionState> {
    return {
      status: "unavailable",
      reason: "not_implemented",
      error: {
        kind: "SERVICE_UNAVAILABLE",
        retryable: false,
        next_action: "none",
        detail: null,
      },
    };
  }
}

/**
 * Desktop client (B0.5): the shell's `service_connect` command finds the per-user service or
 * starts the packaged one, and returns the service's own `hello` (or why it is unavailable).
 */
export class TauriServiceClient implements ServiceClient {
  async connect(): Promise<ConnectionState> {
    try {
      return await invoke<ConnectionState>("service_connect");
    } catch {
      return {
        status: "unavailable",
        reason: "not_running",
        error: {
          kind: "SERVICE_UNAVAILABLE",
          retryable: true,
          next_action: "retry",
          detail: null,
        },
      };
    }
  }
}

export function createServiceClient(): ServiceClient {
  // In a plain browser (`pnpm dev`) there is no shell and no service bridge.
  return isTauri() ? new TauriServiceClient() : new FoundationServiceClient();
}
