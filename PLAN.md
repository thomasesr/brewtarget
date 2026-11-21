# PLAN.md — Porting Brewtarget to a Qt-free, Boost-free web application

Status: **draft, not yet started.** No code under `rust/`, `web/` or `tools/` exists yet.

## 1. Objective

Run Brewtarget as a server-rendered web application:

- **No Qt6, no Boost** anywhere in the shipped artefact.
- **Rust** backend, **Bun/TypeScript** frontend.
- **Same database schema** as the C++ application — an existing Brewtarget SQLite or
  PostgreSQL database opens unchanged, and MariaDB is added as a third backend.
- Behaviour ported **as-is**: same brewing maths, same units, same import/export
  formats, same screens.
- Ships as a **Docker container**: SQLite embedded by default, or PostgreSQL /
  MariaDB as sibling containers via `docker compose`.

### Non-goals (v1)

- **Authentication and per-user data isolation.** Both are designed (Appendix A and
  Appendix B) but deliberately deferred to Stage 2. Stage 1 is single-user and binds
  to loopback by default — see §3.3 for the constraints that keep Stage 2 cheap.
- Compiling the existing Qt app to WebAssembly. Qt-for-WASM would keep the Qt
  dependency and renders to a canvas, not a web page. Explicitly rejected.
- Rewriting the audio timers, printing, or the "check for new release" feature in v1.

## 2. Current state (measured, 2026-11-21)

| Area | Path | LOC | Fate |
| --- | --- | ---: | --- |
| Domain model | `src/model/` | 28,253 | **Port** to Rust |
| Serialization (BeerXML / BeerJSON / .beer) | `src/serialization/` | 14,765 | **Port** to Rust |
| Persistence | `src/database/` | 11,211 | **Port** to Rust |
| Units & physical quantities | `src/measurement/` | 6,457 | **Port** to Rust |
| Brewing maths | `src/calculations/` | 3,392 | **Port** to Rust |
| Tests | `src/unitTests/` | 9,106 | **Mirror** in Rust |
| Qt widgets / editors / trees / models / catalogs / tools | `src/widgets/`, `src/editors/`, `src/trees/`, `src/qtModels/`, `src/catalogs/`, `src/tools/`, `src/undoRedo/` | ~30,000 | **Replace** with web UI |

Roughly **64k LOC to port**, **30k LOC to replace**.

Dependencies to eliminate and their Rust replacements:

| Current | Used for | Replacement |
| --- | --- | --- |
| Qt6 Core | containers, strings, `QVariant`, i18n | std, `serde`, `fluent`/`rust-i18n` |
| Qt6 Sql | DB access, SQLite + Postgres drivers | `sqlx` (sqlite, postgres, mysql) |
| Qt6 Widgets / Gui / Svg / PrintSupport | desktop UI | Bun + TypeScript web UI |
| Qt6 Network | release check | `reqwest` (or drop in v1) |
| Qt6 Multimedia | timer sounds | browser `Audio` |
| Boost.JSON, Blaze/sourcemeta | BeerJSON parse + schema validation | `serde_json` + `jsonschema` |
| Boost.Asio / Boost.Beast | HTTP client for release check | `reqwest` / dropped |
| Boost.Stacktrace | crash logging | `backtrace` / `color-eyre` |
| Boost.Algorithm, Boost.Container | string split, flat_set | std, `itertools`, `indexmap` |
| libxml2 + Xerces | BeerXML parse + XSD validation | `quick-xml` + `xsd`-driven checks |
| OpenSSL | TLS for release check | `rustls` / dropped |

## 3. Target architecture

```
browser  ──HTTP/JSON──>  bt-server (Rust, axum)  ──sqlx──>  SQLite | PostgreSQL | MariaDB
   ▲                            │
   └── static assets ───────────┘   (bundled by Bun at build time, served by Rust)
```

**One process, one container.** The Rust binary serves both the JSON API and the
Bun-built static frontend. Bun is the frontend toolchain (bundler, type checker, test
runner, dev server with proxy) — it is not a runtime dependency of the shipped image.
This keeps the production image a single static Rust binary plus an assets directory.

*Alternative considered:* a Bun SSR sidecar process. Rejected for v1 — it doubles the
runtime surface for no benefit, since the screens are data-dense SPA views, not
content pages needing SEO.

### Rust workspace

| Crate | Ports from | Responsibility |
| --- | --- | --- |
| `bt-measure` | `src/measurement/` | units, physical quantities, unit systems, conversions |
| `bt-model` | `src/model/` | entity structs, enums, enum↔string mappings, invariants |
| `bt-calc` | `src/calculations/`, `src/model/Recipe.cpp` | OG/FG/ABV/IBU/colour/mash maths |
| `bt-schema` | `src/database/ObjectStoreTyped.cpp`, `DatabaseSchemaHelper.cpp` | generated DDL, dialect layer, migrations 1→23 |
| `bt-store` | `src/database/ObjectStore*.cpp` | repository layer over `sqlx`, transactions, inventory |
| `bt-format` | `src/serialization/` | BeerXML, BeerJSON, `.beer` import/export |
| `bt-api` | — | axum routes, DTOs, OpenAPI |
| `bt-server` | — | binary: config, static assets, migrations on boot |

Dependency order is strictly left-to-right; no cycles.

### Frontend

Bun + TypeScript + **React** + TanStack Query/Table. The UI is dense tabular editing
(ingredient tables, recipe trees, tab stacks) which maps directly onto TanStack
Table, and React has the deepest ecosystem for that shape. Styling: Tailwind.

Screens mirror the Qt `mainWindow.ui` structure: left tree (Recipes, Styles,
Equipment, Mash/Boil/Fermentation Profiles, Fermentables, Hops, Miscs, Yeasts, Water)
and a recipe view with tabs (Fermentables, Hops, Miscs, Yeast, Extras, Water,
Brewday). Editors in `ui/*.ui` become modal forms, one per entity type.

### 3.3 Forward-compatibility constraints

Authentication and per-user isolation are **deferred to Stage 2** (§5, Appendix A and
Appendix B). Stage 1 is a single-user server: whoever can reach the port has full
control of the data, exactly like the desktop app on a local machine.

That deferral is only cheap if Stage 1 does not design itself into a corner. These
four constraints cost nothing now and are what make Stage 2 mechanical rather than a
rewrite:

1. **No ownership columns.** Never add `user_id` / `owner_id` to a ported table and
   never add a tenant predicate to a query. Isolation in Stage 2 is by namespace — a
   separate SQLite file, PostgreSQL schema, or MariaDB database — which also keeps
   each store a complete, desktop-openable Brewtarget database.
2. **No ambient database handle.** Every `bt-store` repository is constructed from an
   explicit store handle passed in by the caller. No global pool, no
   `static CURRENT_DB`, no function that reaches brewing data without being given a
   handle. Stage 2 then only has to change *where the handle comes from*.
3. **Namespace-agnostic DDL.** The generated schema must not hardcode `public` or a
   fixed database name. Phase 1's DDL renderer takes the target namespace as a
   parameter, even though Stage 1 always passes the default.
4. **A middleware seam in the API.** `bt-api` routes are grouped so a session/role
   layer can be wrapped around them later without touching handlers, and handlers
   receive their store handle from an extractor rather than reading a global.

Stage 1 ships bound to `127.0.0.1` by default. Exposing it on a network before
Stage 2 means publishing an unauthenticated database to that network; `BT_BIND` must
be set deliberately, and the server logs a prominent warning when it is not loopback.

## 4. Stage 1 — Port the application

The whole of Stage 1 is the port: get Brewtarget running as a web application with no
Qt and no Boost, single-user, on all three backends. Authentication and per-user
isolation come afterwards, in Stage 2 (§5).

Each phase is independently mergeable and ends with a green `cargo test --workspace`.
Nothing in a later phase starts before the previous phase's acceptance criteria hold.

### Phase 0 — Scaffolding

- `rust/` cargo workspace with the eight crates above, `web/` Bun project, `tools/`.
- CI: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`, `bun test`.
- GPLv3 headers, `rustfmt.toml`, `clippy.toml`.

**Done when:** empty workspace builds and CI is green.

### Phase 1 — Schema extraction and the dialect layer

The highest-leverage step: it removes all hand-transcription risk from the schema.

1. `tools/extract-schema.py` parses `src/database/ObjectStoreTyped.cpp`, resolving
   `PRIMARY_TABLE<T>`, `JUNCTION_TABLES<T>` and the `*_COMMON_FIELDS` vectors, and
   emits `schema/v23.json` — the canonical table/column/type/FK/enum-mapping model.
2. `tools/gen-ddl.rs` (or a `build.rs`) renders `schema/v23.json` into per-dialect
   DDL, mirroring `DbNativeVariants` in `src/database/Database.cpp:74` and adding a
   **MariaDB** column to every variant (`DOUBLE`, `AUTO_INCREMENT`, backtick quoting,
   `utf8mb4`).
3. Validate: create a fresh DB from the generated SQLite DDL, and compare its
   `sqlite_master` against a DB created by the C++ app. (The C++ app cannot be built
   in this environment — the reference dump must be produced on a machine with Qt6
   and committed to `testData/schema-v23.sqlite.sql`. **This is a prerequisite
   someone must produce; flag it early.**)

**Done when:** generated SQLite DDL is byte-equivalent (modulo whitespace and column
order normalisation) to the C++-produced schema for all 28 tables.

### Phase 2 — Migrations 1 → 23

- Port `src/database/DatabaseSchemaHelper.cpp` into `bt-schema/src/migrations/`.
- Migrations that are pure SQL strings are copied with dialect substitution.
- `migrate_to_20` and `migrate_to_21` are data transformations (they restructure
  recipe ingredient additions and inventory) — port the logic, not just the SQL.
- Schema version lives in the `settings` table, as today — read from whichever store
  is being opened, so Stage 2 gets per-tenant versioning for free.

**Done when:** `testData/Brewtarget_2.3.1_default_database.sqlite` (v3) and
`data/default_db.sqlite` (v13) both migrate to v23, the resulting schema matches
Phase 1's, and row counts are preserved per table. ✅

Note on verification: a freshly created v23 database and a migrated one are compared
against each other. The schema definitions and the migration chain are written
independently in the C++, and extracted independently here, so their agreeing is
meaningful evidence for both — and needs no Qt.

### Phase 3 — `bt-measure`

- Port `Unit`, `UnitSystem`, `PhysicalQuantity`, `Amount`, `NonPhysicalQuantity`,
  `Qualitative`, `SucroseConversion`, `PhysicalConstants`.
- Mirror `src/unitTests/MeasurementTests.cpp` as Rust tests, same tolerances.

**Done when:** every assertion in `MeasurementTests.cpp` has a passing Rust twin. ✅

`testUnitInverses` in the C++ walks a hand-maintained list of units carrying the
warning "If you add a new Unit, please add it here!"; the Rust twin walks every unit
in `schema/units.json` instead, so a new one cannot be missed. The two C++ cases that
exercise Qt widgets and locale-dependent number formatting
(`testPercentageDecimalPlaces`, `testNumberDisplayAndParsing`) belong to the
presentation edge and are deferred to Phase 8.

### Phase 4 — `bt-model` + `bt-store`

- Generate entity structs from `schema/v23.json` (Phase 1), hand-write the behaviour
  that the C++ keeps in the model classes (validation, derived properties, the
  `Recipe` ↔ `RecipeAddition*` ownership rules, folder paths, lock/share flags).
- `bt-store`: typed repositories over `sqlx`, replacing `ObjectStore` /
  `ObjectStoreTyped`. Every repository is constructed from an explicit **store
  handle**, never from a global pool (§3.3 constraint 2). Keep the C++ semantics for soft delete (`deleted`), display
  flag (`display`), and cascading deletes of owned objects.
- Load `data/DefaultContent*.{xml,json,beer}` on first run, tracking the default
  content version exactly as `DefaultContentLoader.cpp` does.
- Mirror `src/unitTests/DatabaseTests.cpp` and `CoreTests.cpp`.

**Done when:** a fresh DB is seeded identically to the C++ app (same row counts and
same primary keys per default-content file), and round-trip CRUD tests pass on all
three backends.

### Phase 5 — `bt-calc`

- Port `Algorithms.cpp`, `ColorMethods.cpp`, `IbuMethods.cpp`,
  `MashFermentability.cpp`, and the calculation half of `Recipe.cpp` (OG, FG, ABV,
  IBU by method, SRM/EBC colour, calories, efficiency, boil-off, water needs).
- Literal transcription first: same names, same constants, same order of operations.
- Mirror `RecipeCalcTests.cpp`, `MashFermentabilityTests.cpp`,
  `MashAutoAdjusterTests.cpp`, `WaterAutoAdjusterTests.cpp`.

**Done when:** all mirrored calculation tests pass at the C++ tolerances.

### Phase 6 — `bt-format`

- BeerXML v1 import/export via `quick-xml`, validated against
  `schemas/beerxml/v1/BeerXml.xsd`.
- BeerJSON 1.0 import/export via `serde_json`, validated against
  `schemas/beerjson/1.0/*.json` with the `jsonschema` crate.
- `.beer` format via `schemas/dotBeer/*.beer.schema`.
- Mirror `SerializationTests.cpp`; add round-trip property tests
  (import → export → import is a fixpoint) over `testData/`.

**Done when:** every file in `testData/` imports, and re-export round-trips.

### Phase 7 — `bt-api` + `bt-server`

- axum, one resource group per entity type, plus:
  - `GET /api/recipes/:id/calculated` — the derived values the Qt recipe tab shows.
  - `POST /api/import` / `GET /api/export` — the file formats from Phase 6.
  - `GET /api/inventory`, stock purchase/use endpoints.
  - `GET /api/brewday/:id` — the Brewday instruction sheet.
- DTOs are `serde` structs; OpenAPI emitted with `utoipa` and consumed by the frontend
  for typed clients.
- `bt-server`: config via env (`BT_DB_URL`, `BT_DATA_DIR`, `BT_BIND`, `BT_PORT`),
  runs migrations on boot, serves `web/dist` as static files with SPA fallback.
- No authentication in Stage 1. Routes are grouped so a session layer wraps them
  later without touching handlers, and each handler receives its store handle from an
  extractor rather than a global (§3.3).

**Done when:** every Phase 4–6 capability is reachable over HTTP, with an integration
test suite driving the API against all three backends.

### Phase 8 — Web UI

Ported screen by screen, in this order (each independently useful):

1. Left tree + ingredient catalogue tables (Fermentables, Hops, Miscs, Yeasts, Water,
   Styles, Equipment, Mash/Boil/Fermentation profiles) — read-only, then editable.
2. Recipe view: header (name, style, equipment, batch size, boil time) + the derived
   panel (OG, FG, ABV, IBU, colour) + the ingredient tabs.
3. Entity editor modals, one per `ui/*Editor.ui`.
4. Brewday sheet, print via browser print CSS (replaces Qt PrintSupport).
5. Tools: OG correction, refractometer, priming, pitch rate, strike water
   (`src/tools/`).
6. Timers (`src/tools/Timer*`, `TimerDialog`) — browser `Audio` plus
   `data/sounds/*.wav`.

**Done when:** every tab and dialog in `ui/` has a web equivalent, or is listed as a
deliberate v1 omission in this file.

### Phase 9 — Packaging

Multi-stage Dockerfile:

```
FROM oven/bun          AS web     # bun install && bun run build  -> /web/dist
FROM rust:slim         AS backend # cargo build --release --locked
FROM gcr.io/distroless/cc-debian12
COPY --from=backend /app/bt-server /bt-server
COPY --from=web     /web/dist      /assets
VOLUME /data
ENV BT_DB_URL=sqlite:///data/brewtarget.sqlite \
    BT_DATA_DIR=/data \
    BT_BIND=127.0.0.1
EXPOSE 8080
```

`docker-compose.yml` with profiles:

| Profile | Services | `BT_DB_URL` |
| --- | --- | --- |
| *(default)* | `brewtarget` only, named volume at `/data` | `sqlite:///data/brewtarget.sqlite` |
| `postgres` | `brewtarget` + `postgres:17` | `postgres://…@db/brewtarget` |
| `mariadb` | `brewtarget` + `mariadb:11` | `mysql://…@db/brewtarget` |

Stage 2 adds a second credential per profile (a DDL-capable provisioning role) — see
Appendix B. Stage 1 needs only the one.

Healthcheck on `GET /healthz` (liveness only — no version or schema detail). The app
waits for the DB and runs migrations before serving. Image target: < 50 MB. The
container runs as a non-root user; `/data` is the only writable path. The published
port must not be exposed beyond the host until Stage 2 lands.

**Done when:** all three profiles start clean, and a pre-existing Brewtarget SQLite
file mounted at `/data` is migrated and served, still containing only the 28
Brewtarget tables afterwards.

### Phase 9b — Optional, unscheduled

- **WASM**: compile `bt-calc` to `wasm32-unknown-unknown` so the recipe screen
  recalculates on keystroke without a round-trip, sharing one implementation with the
  server. This is the only place WASM earns its keep.
- Sharing a recipe between accounts, or a shared read-only ingredient catalogue
  (needs Stage 2 first).
- SSO (OIDC) and TOTP, on top of the Phase 10 password login.
- Tenant sharding across multiple database servers.
- i18n: `translations/*.ts` are Qt translation files; converting them to Fluent/JSON
  preserves ~20 existing locales.
- Undo/redo (`src/undoRedo/`) as a client-side command stack over the API.

## 5. Stage 2 — Authentication and tenancy (deferred)

Designed, not scheduled. Starts once Stage 1's Phase 9 is green. The full designs are
preserved in **Appendix A** (authentication) and **Appendix B** (per-user isolation);
they are unchanged by the deferral, and §3.3 lists what Stage 1 must respect for them
to drop in cleanly.

- **Phase 10 — Authentication.** New crate `bt-auth`: accounts, Argon2id, server-side
  sessions, login rate limiting, first-run admin setup, login page. See Appendix A.
- **Phase 11 — Per-user data isolation.** New crate `bt-tenant`: namespace per
  account (SQLite file / PostgreSQL schema / MariaDB database), bounded per-tenant
  pool cache, provisioning lifecycle, split DML and DDL database roles. See
  Appendix B.

Phase 10 depends on Phase 9; Phase 11 depends on Phase 10.

## 6. Risks

| Risk | Impact | Mitigation |
| --- | --- | --- |
| No Qt6 in this environment → cannot produce the reference schema dump or run the C++ tests | Blocks Phase 1 verification | Produce `testData/schema-v23.sqlite.sql` and a C++ test-output snapshot on a machine with Qt6, commit both. **Do this first.** |
| Migration 20/21 data transforms are subtle | Silent data corruption on upgrade | Port with per-migration fixture DBs; assert row-level equality against C++-migrated output |
| Floating-point drift between C++ `double` maths and Rust | Wrong IBU/OG shown | Literal transcription; mirrored tests at C++ tolerances; no algebraic "cleanups" before green |
| `ObjectStoreTyped.cpp` uses heavy templates; a naive parser will miss inherited columns | Missing columns | Extractor must resolve `*_COMMON_FIELDS`; cross-check column counts against the reference dump |
| Schema divergence if upstream Brewtarget bumps past v23 | Fork drifts | Pin the ported version to a git SHA in each file header; re-run extractors on upstream merge |
| MariaDB is a *new* backend, untested upstream | Unknown breakage | Run the full integration suite on all three backends in CI from Phase 4 onward |
| **Stage 1 has no authentication at all** | Anyone who can reach the port has full read/write control of the database, and can export or wipe it | Bind to `127.0.0.1` by default; require `BT_BIND` to be set deliberately and log a prominent warning when it is not loopback; do not publish the container port beyond the host; do not deploy Stage 1 to a shared or public network — wait for Stage 2 |
| Stage 1 design that blocks Stage 2 | Auth/tenancy becomes a rewrite instead of an addition | The four constraints in §3.3, each cheap now and checked in review: no ownership columns, no ambient handle, namespace-agnostic DDL, middleware seam |

Stage 2's own risks — cross-tenant pool leakage, DDL injection via namespace names,
over-privileged database roles, pool exhaustion, catalogue pressure at scale, and
shipping without TLS — are recorded with their mitigations in Appendix A and
Appendix B, and move into this table when Stage 2 starts.

## 7. Open decisions

### Needed for Stage 1

1. **Frontend framework** — React assumed above. Svelte or Solid would also fit.
   *Gates Phase 8.*
2. **Translations** — port the 20 existing locales from `translations/*.ts`, or ship
   English-only first? *Gates Phase 8.*
3. **Legacy tree** — keep `src/` in the repo as reference indefinitely, or delete it
   once the port is green? Keeping it costs nothing and preserves the oracle.

### Deferred with Stage 2

These are already answered with working assumptions in Appendix A and Appendix B;
they only need confirming when Stage 2 starts: auth store location, registration
model, whether an admin can read another user's library, whether an admin gets a
library of their own, and whether each tenant seeds its own copy of the default
content.

## 8. Immediate next steps

- [x] **Phase 0 scaffolding** — `rust/` workspace (8 crates, all three sqlx backends
  compiled in), `web/` Bun + React + TypeScript project, `.github/workflows/port.yml`.
  `cargo fmt`/`clippy -D warnings`/`test`/`build` and `bun typecheck`/`test`/`build`
  all green.
- [x] **`tools/extract-schema.py` → `schema/v23.json`** (Phase 1 step 1). Parses the
  `PRIMARY_TABLE<T>` declarations, resolves the `*_COMMON_FIELDS` vectors, expands the
  `TABLE_FOLDER_DEFN` macro, and emits 39 tables / 530 columns / 55 foreign keys at
  schema version 23. Output is byte-reproducible; CI diffs a fresh run against the
  committed file.
- [x] **Phase 1 step 2** — the Rust DDL renderer in `bt-schema`, driven by
  `schema/v23.json`. Reproduces the C++ output format exactly (including its `", \n"`
  field separator), renders all three dialects, and takes the target namespace as a
  parameter per §3.3. Verified by executing the generated DDL against in-memory
  SQLite: 39 tables with exactly the expected columns, and all 55 foreign keys
  resolving to their targets. A PostgreSQL equivalent runs inside a rolled-back
  transaction when `BT_TEST_POSTGRES_URL` is set.
- [x] **Phase 2** — migrations 1 → 23. 814 statements lifted into
  `schema/migrations.json`; `migrate_to_202` and `migrate_to_20` hand-ported in
  `bt-schema`. Both checked-in databases migrate to v23 with their data intact, and a
  database created from the schema definitions has exactly the same shape as one
  brought up by migration — a check the C++ never makes.
- [x] **Phase 3** — `bt-measure`. 79 units, 40 unit systems and the 801-point USDA
  sucrose table lifted into `schema/units.json`; the six by-formula units hand-ported.
  Mirrors `MeasurementTests.cpp`, including all 142 SG/Brix rows at the C++'s own
  per-row tolerances.
- [ ] **Get `testData/schema-v23.sqlite.sql` produced on a Qt6-capable machine.**
  Phase 1 can be *written* without it but cannot be *verified* without it, and Qt6,
  CMake and Meson are all absent from the current environment. **Still the only
  external dependency in Stage 1.**


## Appendix A — Authentication design (deferred to Stage 2)

*Written while auth was in v1 scope; retained verbatim as the Phase 10 specification.*

The desktop app had no login because it was a local single-user program. A server
exposed over a network needs one, so v1 ships a login page.

**Scope.** Local accounts, each owning one private brewing library. No self-service
registration and no SSO in v1.

Logging in does two things: it establishes the session, and it resolves **which
brewing data store the session is bound to** (Appendix B). A session is bound to exactly
one tenant for its whole lifetime; the binding is re-derived from the account on every
request, never taken from the client.

| Role | May |
| --- | --- |
| `user` | full control of their own library; no visibility of any other library |
| `admin` | manage accounts (create, disable, delete, reset password) and server settings. **Not** implicit read access to other users' libraries — an admin wanting one must trigger an audited export or a password reset |

**Storage.** Auth data lives in its own store with its own migration chain, so the
brewing database stays a pristine Brewtarget file that the C++ desktop app can still
open. Default location is `$BT_DATA_DIR/auth.sqlite`, independent of which backend
holds the brewing data; `BT_AUTH_DB_URL` can point it at Postgres or MariaDB instead.
Tables: `account`, `session`, `login_attempt`.

**Mechanism.**

- Passwords hashed with **Argon2id** (`argon2` crate), per-password random salt,
  parameters recorded in the stored hash string so they can be raised later. Existing
  hashes are re-hashed transparently on next successful login when parameters change.
- Password verification is constant-time; a failed lookup still performs a dummy hash
  so response timing does not reveal whether an account exists.
- Sessions are **server-side**: the cookie carries an opaque 256-bit random token
  (`rand::rngs::OsRng`), and the database stores only its SHA-256. Nothing about the
  user is encoded in the cookie, so revocation is immediate and total.
- Cookie flags: `HttpOnly`, `SameSite=Lax`, `Path=/`, and `Secure` unless
  `BT_DEV_INSECURE=1` is set for local HTTP development.
- Idle timeout 7 days, absolute lifetime 30 days, both configurable. Session token is
  rotated on login and on privilege change.
- CSRF: `SameSite=Lax` plus an `Origin`/`Sec-Fetch-Site` check on every state-changing
  request. Requests that fail the check are rejected with 403.
- Login rate limiting: per-account and per-IP exponential backoff recorded in
  `login_attempt`, with a lockout after repeated failures. Lockout is reported to the
  user as a generic failure, not as "this account is locked".

**Bootstrap.** There is no default password. On first boot with an empty `account`
table the server serves only a one-time setup page that creates the admin; every
other route returns 503 until that completes. Alternatively `BT_ADMIN_USER` and
`BT_ADMIN_PASSWORD_FILE` can seed the admin non-interactively for automated deploys,
and the server refuses to start if `BT_ADMIN_PASSWORD` is passed inline as an
environment variable.

**Deployment requirement.** The container must sit behind a TLS-terminating reverse
proxy. Running it on plain HTTP over a network exposes session cookies and passwords;
the server logs a loud warning at startup when `BT_DEV_INSECURE=1` is set.

**What is never logged:** passwords, session tokens, password-reset tokens, or the
`Cookie` and `Authorization` headers. The logging layer redacts these by name.

### A.1 Phase 10 work breakdown

New crate `bt-auth`, sitting between `bt-store` and `bt-api`. Implements §3.3.

- `bt-auth`: `account` / `session` / `login_attempt` schema and migrations, Argon2id
  hashing, session issue/verify/revoke, rate limiting and lockout.
- `bt-api` middleware: session extraction from the cookie, role guard per route,
  `Origin` / `Sec-Fetch-Site` CSRF check on mutating methods, redaction of
  credentials in the logging layer.
- Endpoints:
  - `GET  /api/auth/setup` / `POST /api/auth/setup` — first-run admin creation; 410
    once an account exists.
  - `POST /api/auth/login`, `POST /api/auth/logout`.
  - `GET  /api/auth/me` — current account and role.
  - `POST /api/auth/password` — change own password; revokes all other sessions.
  - `GET/POST/PATCH/DELETE /api/accounts` — admin only; account creation triggers
    tenant provisioning (Phase 11).
  - `GET  /api/library/status` — `provisioning` / `ready` / `failed`, for the
    "preparing your library" screen.
- Tests: wrong password rejected; locked account reports a generic failure; session
  revoked on password change; `viewer` blocked from every write route; cross-origin
  POST rejected; cookie carries `HttpOnly` + `SameSite=Lax` + `Secure`; no secret
  appears in captured log output; timing of a login against a non-existent account is
  indistinguishable from one against a real account with a wrong password.

**Done when:** an unauthenticated request to any data route returns 401, the setup
flow creates exactly one admin, and the test list above is green.

## Appendix B — Per-user isolation design (deferred to Stage 2)

*Written while tenancy was in v1 scope; retained verbatim as the Phase 11 specification.*

Every account owns a private, physically separate brewing data store. The Brewtarget
schema inside each store is the untouched v23 schema from Phase 1 — isolation is
achieved by giving each user their own namespace, never by adding a `user_id` column
to the ported tables. That keeps the "schema identical, opens in the desktop app"
guarantee intact, and makes per-user export a file copy rather than a query.

#### How each backend provides a namespace

| Backend | Isolation unit | Create | Connect | Drop |
| --- | --- | --- | --- | --- |
| **SQLite** | one file per user | create `$BT_DATA_DIR/users/<key>.sqlite` | `sqlite://…/users/<key>.sqlite` | delete file |
| **PostgreSQL** | one *schema* per user, inside one database | `CREATE SCHEMA bt_u_<key>` | same DSN, with `options=-c search_path=bt_u_<key>` fixed **at connect time** | `DROP SCHEMA bt_u_<key> CASCADE` |
| **MariaDB** | one *database* per user, on one server | `CREATE DATABASE bt_u_<key> CHARACTER SET utf8mb4` | same DSN with `/bt_u_<key>` as the default database | `DROP DATABASE bt_u_<key>` |

**The MariaDB answer.** MySQL and MariaDB have no schema-vs-database distinction —
`CREATE SCHEMA` is a documented synonym for `CREATE DATABASE`, and there is no
intermediate grouping level. So the PostgreSQL "schema per user" maps onto "database
per user" on the same server instance. Isolation, connection endpoint and credential
handling are identical; only the DDL verb and the way the namespace is selected
differ. Nothing about the design is weaker on MariaDB.

#### Namespace naming — the injection hot spot

Schema and database names **cannot be bound as query parameters**; they are
interpolated into DDL as text. This is the one place in the system where a tenant
identifier reaches SQL as a string, so:

- The namespace key is a server-generated opaque 128-bit value rendered as 32 lower
  case hex characters. It is **never** derived from the username, email, or any other
  user-supplied text.
- The full name is `bt_u_<key>`, validated against `^bt_u_[0-9a-f]{32}$` immediately
  before every interpolation, in a single choke-point function. A value failing the
  check is a panic-worthy internal invariant violation, not a user error.
- Identifiers are additionally quoted per dialect (`"…"` for PostgreSQL, backticks for
  MariaDB) and path components are rejected outright for SQLite (no separators, no
  `.`, no `..`).

#### Connection management

- A `TenantPool` cache holds at most `BT_TENANT_POOL_MAX` (default 64) live pools,
  LRU-evicted, each with a small connection cap (default 4) and a 10-minute idle
  timeout. Total server connections are therefore bounded independently of the number
  of accounts.
- Pools are keyed strictly by namespace key. A pool is never handed to a request bound
  to a different tenant — this is the single most important invariant in the system,
  and it gets a dedicated concurrency test.
- For PostgreSQL the `search_path` is pinned in the connection string, not issued as a
  runtime `SET`. A pooled connection therefore cannot be recycled carrying another
  tenant's search path. **Never call `SET search_path` at runtime.**

#### Two database roles, not one

Provisioning needs `CREATE SCHEMA` / `CREATE DATABASE`; serving requests does not.
Running everything as one all-powerful role means an application-level SQL flaw can
drop every tenant.

- `BT_DB_URL` — the runtime role: DML only, plus `USAGE` on existing namespaces.
- `BT_PROVISION_DB_URL` — the DDL role, used only by the provisioning/deprovisioning
  path and by migrations. Optional; falls back to `BT_DB_URL` with a startup warning.

#### Provisioning lifecycle

Tenant state lives in the auth store (`tenant` table: key, account id, backend,
status, schema version, created/last-opened timestamps). Status is one of
`provisioning`, `ready`, `failed`, `deleting`.

1. **Create** (on account creation): allocate key → create namespace → apply v23 DDL →
   seed `data/DefaultContent*` → mark `ready`. Runs as a background job; a user who
   logs in before it finishes sees a "preparing your library" screen.
2. **Open**: look up tenant → check its schema version → migrate if behind, under a
   per-tenant lock (`pg_advisory_lock` / `GET_LOCK` / SQLite file lock) so two
   concurrent requests cannot migrate the same tenant twice.
3. **Export**: BeerJSON export, plus a raw store copy (file copy for SQLite,
   `pg_dump --schema` for PostgreSQL, `mysqldump` for MariaDB).
4. **Delete**: admin-confirmed, two-step. The account is disabled and all its sessions
   revoked immediately; the namespace is dropped after a grace period
   (`BT_TENANT_GRACE_DAYS`, default 7), and an export is taken first.

#### Costs

Seeding costs roughly 1–3 MB per tenant (the default content is 1.6 MB of source
files, 820 KB as a built SQLite). A thousand accounts is single-digit GB, which is
fine. Catalogue pressure is the real ceiling: PostgreSQL carries each schema's tables
in `pg_class`, and MariaDB creates a filesystem directory per database and slows down
on `information_schema` scans. Treat low thousands of tenants per server as the
supported range, and document sharding as the answer beyond that.

### B.1 Phase 11 work breakdown

Implements Appendix B. New crate `bt-tenant`, between `bt-store` and `bt-api`.

- `Namespace` abstraction with the three backend implementations (SQLite file,
  PostgreSQL schema, MariaDB database), behind one trait: `create`, `open`, `migrate`,
  `export`, `drop`.
- The identifier choke-point: key generation, `^bt_u_[0-9a-f]{32}$` validation, and
  per-dialect quoting, all in one module with no other call path to DDL.
- `TenantPool` LRU cache, bounded pools, strict keying, PostgreSQL `search_path`
  pinned in the DSN.
- Separate provisioning role (`BT_PROVISION_DB_URL`) and runtime role (`BT_DB_URL`).
- `tenant` table in the auth store; provisioning as a background job with
  `provisioning` / `ready` / `failed` / `deleting` states.
- Per-tenant migration lock (`pg_advisory_lock` / `GET_LOCK` / SQLite file lock).
- Per-tenant export, and two-step deletion with a grace period and a pre-drop export.
- `bt-api` resolves the tenant from the authenticated account on **every** request.
  The tenant is never read from a header, query parameter, or cookie field.

**Tests, on all three backends:**

- Two accounts writing concurrently never observe each other's rows.
- A request bound to tenant A can never be served a pool belonging to tenant B, under
  concurrent load with an undersized pool cache that forces eviction and rebuild.
- On PostgreSQL, a recycled connection always reports the expected `search_path`.
- A namespace key failing the regex aborts before any SQL is issued; a crafted
  username containing `"; DROP SCHEMA` has no effect on the generated identifier.
- Dropping tenant A leaves tenant B intact, and the dropped namespace is gone from
  `information_schema` / the filesystem.
- A tenant at schema v19 is migrated to v23 on first open, exactly once, under
  concurrent first requests.
- The runtime role cannot create or drop a namespace.

**Done when:** the test list above is green on SQLite, PostgreSQL and MariaDB, and an
exported tenant store opens unchanged in the C++ desktop app.
