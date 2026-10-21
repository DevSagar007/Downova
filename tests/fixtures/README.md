# Test fixtures

Controlled, deterministic fixtures for protocol and failure testing. No fixtures exist yet. The first
ones arrive in milestone B0.4 (see [../../docs/roadmap.md](../../docs/roadmap.md)).

Planned layout:

| Path | Purpose | Requirements |
|---|---|---|
| `http/` | Local HTTP server with scripted behaviours: range honoured/ignored, wrong Content-Range, truncation, 416, strong/weak/missing ETag, same-length change, 429/503 + Retry-After, auth failures, redirects, `filename*` | COR-01..04, CORE-02, E04 |
| `files/` | Generated payloads with known SHA-256 (created at test time, not committed when large) | COR-05 |
| `secrets/` | Synthetic secret values for redaction tests (never real credentials) | SEC-01, E05 |

Rules:
- Fixtures run offline on loopback and are deterministic (seeded).
- Each fixture documents the expected safe outcome. The oracle is the completed file's hash, or an explicit failure kind.
- Temporary output goes under `tests/fixtures/.tmp/` (gitignored).
