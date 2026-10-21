// Public entry point for TypeScript consumers (desktop UI, browser extension).
// Types are generated from schema/service-api.v1.schema.json; never edit them by hand.
export type * from "./generated/service-api.v1.js";

/** Service API version these contracts describe. Must match HelloResult.api_version. */
export const SERVICE_API_VERSION = 1 as const;
