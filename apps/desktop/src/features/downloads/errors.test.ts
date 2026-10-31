import { describe, expect, it } from "vitest";
import type { DownloadSnapshot, ErrorData, ErrorKind, NextAction } from "@dm/contracts";
import schema from "@dm/contracts/schema/service-api.v1.schema.json";
import { bn, catalogs, en, type MessageKey } from "../../i18n/messages";
import { errorText } from "./present";

// The contract's enums, read from the canonical schema so that a new kind or action without
// copy fails here even if a generated type were stale.
const KINDS = schema.$defs.ErrorKind.enum as ErrorKind[];
const ACTIONS = schema.$defs.NextAction.enum as NextAction[];
const BENGALI = /[ঀ-৿]/;
// Features that do not exist yet; no error copy may offer them.
const NOT_OFFERED = [/diagnostic/i, /export/i, /sign in to Downova/i, /log ?in to Downova/i];

const tEn = (key: MessageKey) => en[key];

function failing(error: ErrorData, overrides: Partial<DownloadSnapshot> = {}): DownloadSnapshot {
  return {
    job_id: "job_1",
    attempt_id: null,
    display_name: "file.zip",
    source_display: "https://example.org/file.zip",
    desired_state: "running",
    phase: "failed",
    blockers: [],
    downloaded_bytes: "0",
    total_bytes: null,
    error,
    ...overrides,
  };
}

const err = (
  kind: ErrorKind,
  next_action: NextAction,
  detail: string | null = null,
): ErrorData => ({
  kind,
  retryable: false,
  next_action,
  detail,
});

describe("error catalogue (CORE-11)", () => {
  it("has English and Bangla text for every error kind and every next action", () => {
    expect(KINDS).toHaveLength(20);
    const keys = [
      ...KINDS.map((k) => `error.kind.${k}`),
      ...ACTIONS.filter((a) => a !== "none").map((a) => `error.action.${a}`),
      "error.followUp.failed",
      "error.followUp.continues",
      "error.followUp.autoRetry",
    ] as MessageKey[];
    for (const key of keys) {
      expect(en[key], key).toMatch(/\S{3}/);
      expect(bn[key], key).toMatch(BENGALI);
      expect(bn[key], key).not.toBe(en[key]);
    }
    // No stray keys for kinds or actions the contract does not have.
    const catalogued = Object.keys(en).filter((k) => k.startsWith("error.kind."));
    expect(catalogued.sort()).toEqual(KINDS.map((k) => `error.kind.${k}`).sort());
  });

  it("offers only actions that exist", () => {
    for (const messages of Object.values(catalogs)) {
      for (const [key, text] of Object.entries(messages)) {
        if (!key.startsWith("error.")) continue;
        for (const pattern of NOT_OFFERED) expect(text, key).not.toMatch(pattern);
      }
    }
    // Authenticated downloads are not implemented, so "sign in" never pretends otherwise.
    expect(en["error.action.sign_in"]).toMatch(/cannot sign in/);
    expect(en["error.action.contact_support"]).not.toMatch(/diagnostic/i);
  });

  it("explains a failed download and how to start it again", () => {
    const text = errorText(failing(err("REMOTE_CHANGED", "choose_restart_or_keep")), tEn);
    expect(text).toEqual({
      summary: en["error.kind.REMOTE_CHANGED"],
      action: en["error.action.choose_restart_or_keep"],
      followUp: en["error.followUp.failed"],
    });
    const link = errorText(failing(err("LINK_REFRESH_REQUIRED", "refresh_link")), tEn);
    expect(link?.followUp).toBeNull();
    const tls = errorText(failing(err("TLS_HANDSHAKE_FAILED", "none")), tEn);
    expect(tls?.action).toBeNull();
  });

  it("says a waiting download continues by itself and a retrying one needs nothing", () => {
    const disk = errorText(
      failing(
        { ...err("DISK_FULL", "free_disk_space"), retryable: true },
        { phase: "queued", blockers: ["disk_unavailable"] },
      ),
      tEn,
    );
    expect(disk?.action).toBe(en["error.action.free_disk_space"]);
    expect(disk?.followUp).toBe(en["error.followUp.continues"]);
    const retrying = errorText(
      failing({ ...err("NETWORK_OFFLINE", "retry"), retryable: true }, { phase: "queued" }),
      tEn,
    );
    expect(retrying).toEqual({
      summary: en["error.kind.NETWORK_OFFLINE"],
      action: null,
      followUp: en["error.followUp.autoRetry"],
    });
    const paused = errorText(
      failing(err("SERVER_BUSY", "wait"), { phase: "queued", desired_state: "paused" }),
      tEn,
    );
    expect(paused?.followUp).toBeNull();
  });

  it("never derives text from the detail, so a secret in it cannot be shown", () => {
    const detail = "refused https://u:p@h/a.zip?token=SYNTHETIC-SECRET-ui17";
    for (const kind of KINDS) {
      for (const action of ACTIONS) {
        for (const messages of Object.values(catalogs)) {
          const text = errorText(failing(err(kind, action, detail)), (k) => messages[k]);
          const shown = JSON.stringify(text);
          expect(shown).not.toContain("SYNTHETIC-SECRET");
          expect(shown).not.toContain("u:p@");
        }
      }
    }
  });
});
