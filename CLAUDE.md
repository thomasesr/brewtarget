# CLAUDE.md

Guidance for Claude Code working in this repository.

## What this repo is

A fork of [Brewtarget](https://github.com/Brewtarget/brewtarget), a Qt6/C++ desktop
brewing-recipe application. **The fork's objective is to port it to a
server-rendered web application with no Qt and no Boost dependency**, keeping the
existing database schema byte-compatible so existing user databases open unchanged.

Target stack: **Rust** backend (HTTP + JSON API + persistence) and **Bun/TypeScript**
frontend, shipped as a Docker container.

**Current stage: Stage 1 — port the application.** Single-user, no authentication, no
per-user isolation. Those are designed but deferred to Stage 2 (`PLAN.md` §5 and
Appendices A/B). Do not start building them; *do* respect the four forward-compat
constraints in `PLAN.md` §3.3, repeated as porting rules 7–9 below. See `PLAN.md` for the full porting plan,
phase order, and acceptance criteria. **Read `PLAN.md` before starting any port work.**

## Two trees live side by side

| Path | Status | Rule |
| --- | --- | --- |
| `src/`, `ui/`, `CMakeLists.txt`, `meson.build` | **Legacy / reference** | Read freely. Do **not** add features. Edit only to fix a porting-blocker or to extract canonical data. |
| `rust/` | New backend | All new server code. |
| `web/` | New frontend | All new UI code. |
| `tools/` | Port tooling | Extractors/codegen that read `src/` and emit Rust/SQL/TS. |
| `data/`, `schemas/`, `testData/` | **Shared, authoritative** | Used by both trees. Do not rewrite; both trees must agree with these. |

The legacy tree is the **specification**. When Rust behaviour and C++ behaviour
disagree, the C++ is right unless `PLAN.md` says otherwise.

## Authoritative sources (do not re-derive by hand)

- **Database schema** — `src/database/ObjectStoreTyped.cpp`. Tables are declared as
  `PRIMARY_TABLE<T>` / `JUNCTION_TABLES<T>` specialisations with explicit column
  names, types, foreign keys and enum mappings. Parse it; never hand-transcribe.
- **SQL dialect differences** — `src/database/Database.cpp:74` (`DbNativeVariants`).
  Currently SQLite + PostgreSQL. MariaDB is a new third variant to be added.
- **Schema version + migrations** — `src/database/DatabaseSchemaHelper.cpp`.
  `latestVersion = 23`; `migrate_to_N()` functions carry the upgrade SQL.
- **Units and physical quantities** — `src/measurement/`.
- **Brewing maths** — `src/calculations/` plus `src/model/Recipe.cpp`.
- **File formats** — `schemas/` (BeerXML XSD, BeerJSON 1.0, `.beer`) and
  `src/serialization/`.
- **Seed content** — `data/DefaultContent*.{xml,json,beer}` and
  `data/default_db.sqlite`.
- **Behavioural oracle** — `src/unitTests/`. Every ported module must have a Rust
  test mirroring the corresponding C++ test, with the same expected values.

## Toolchain available in this environment

`bun`, `node`, `cargo`, `rustc`, `g++`, `psql`, `python3`.
**Missing: `cmake`, `meson`, Qt6, `sqlite3` CLI.** The legacy C++ app therefore
**cannot be built or run here** — do not attempt it or propose it as a verification
step. To inspect `.sqlite` files use `python3 -c "import sqlite3; ..."`.

## Commands

Backend (once `rust/` exists):

```
cargo build --workspace
cargo test  --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

Frontend (once `web/` exists):

```
bun install
bun run dev
bun test
bun run build
```

Full stack:

```
docker compose up --build              # sqlite, single container
docker compose --profile postgres up   # external postgres
docker compose --profile mariadb  up   # external mariadb
```

## Porting rules

1. **Schema is frozen.** Column names, types, nullability and foreign keys must match
   what the C++ produces for schema v23. New columns require a new migration and a
   bump of the schema version in *both* trees.
2. **Generate, don't transcribe.** Schema DDL, enum string mappings and model structs
   come from `tools/` extractors run against `src/`. Check the generated output into
   git so builds are hermetic, and note the generator in a header comment.
3. **Port maths literally first.** Keep the C++ variable names, formula shape and
   magic constants in the first Rust pass. Refactor only after the mirrored test
   passes. Brewing formulas are full of empirical constants — a "cleaner" rearrangement
   silently changes results.
4. **Floating point.** Compare with the same tolerance the C++ test uses. Do not
   substitute exact equality.
5. **No Qt types in the public Rust API.** `QVariant`-shaped dynamic property access
   is a Qt artefact; model it with typed structs and `serde`, not a dynamic bag.
6. **Units stay in the model.** Values are stored canonically (SI-ish) in the DB and
   converted only at the presentation edge, exactly as `src/measurement/` does.
7. **No ownership columns, ever.** Do not add `user_id` / `owner_id` to a ported
   table and do not add a tenant predicate to a query. Stage 2 isolates users by
   giving each a separate store (SQLite file / PostgreSQL schema / MariaDB database),
   which also keeps every store a complete, desktop-openable Brewtarget database. If
   a query looks like it needs a tenant predicate, the design is wrong.
8. **Every repository is built from an explicit store handle.** No global pool, no
   `static CURRENT_DB`, no ambient "current database". A function that reaches brewing
   data without being handed a store is a bug. Stage 2 then only changes where the
   handle comes from.
9. **Generated DDL takes the target namespace as a parameter** and never hardcodes
   `public` or a fixed database name — even though Stage 1 always passes the default.
   Likewise, keep `bt-api` routes grouped so a session layer can wrap them later
   without touching handlers.
10. **Licence.** Brewtarget is GPLv3. New files in `rust/` and `web/` carry a GPLv3
   header and preserve the upstream copyright line when the file is a port of a
   specific C++ file; name that file in the header.

## Security posture

**Stage 1 ships with no authentication.** Anyone who can reach the port has full
read/write control of the database and can export or wipe it. Therefore:

- The server binds to `127.0.0.1` by default. `BT_BIND` must be set deliberately to
  change that, and the server logs a prominent warning when it is not loopback.
- Do not publish the container port beyond the host, and do not deploy Stage 1 to a
  shared or public network. Wait for Stage 2.
- Do not paper over this with a hardcoded password, a shared secret, or an IP
  allowlist invented ad hoc. If auth is needed, that is Stage 2 — say so rather than
  improvising one.

### Stage 2 rules (not yet in force — do not implement early)

Recorded here so the eventual implementation does not drift from `PLAN.md`
Appendix A / B. Breaking one of these is a vulnerability, not a style nit.

- Passwords are hashed with **Argon2id** only. Never MD5, SHA-*, or an unsalted hash.
  Never store or log a plaintext password.
- Session cookies carry an opaque random token; the database stores only its SHA-256.
  Never put user identity, role or expiry in the cookie itself.
- Cookies are always `HttpOnly` + `SameSite=Lax`, and `Secure` unless
  `BT_DEV_INSECURE=1` is explicitly set for local HTTP development.
- Every state-changing request is checked for a same-origin `Origin` /
  `Sec-Fetch-Site`. Do not add a route that mutates state on `GET`.
- Authorisation is enforced **server-side on every route**. Hiding a button in the
  frontend is a usability affordance, never an access control.
- Login failures return one generic message. Do not leak whether an account exists,
  is locked, or has the wrong password.
- Passwords, tokens, and the `Cookie` / `Authorization` headers are redacted by the
  logging layer. Do not add a `Debug`/`Display` impl that prints them; use a redacting
  wrapper type for secrets.
- Secrets come from files or a secret store (`BT_ADMIN_PASSWORD_FILE`), never from an
  inline environment variable and never committed.
- There is no default password anywhere. A fresh install blocks on a first-run setup
  page until an admin is created.
- **Namespace identifiers are the one place a tenant value reaches SQL as text**,
  because schema and database names cannot be bound as parameters. Keys are
  server-generated 128-bit hex, never derived from a username or email; they are
  validated against `^bt_u_[0-9a-f]{32}$` in a single choke-point function
  immediately before interpolation, and quoted per dialect. Do not add a second code
  path that builds DDL.
- **Connection pools are keyed strictly by namespace key.** Handing a pool to a
  request bound to another tenant is a cross-user data leak — the most severe failure
  this system can have.
- On PostgreSQL the `search_path` is pinned in the connection string at connect time.
  **Never issue `SET search_path` at runtime**; a recycled pooled connection would
  then carry another tenant's path.
- Two database roles: `BT_DB_URL` has DML rights only, `BT_PROVISION_DB_URL` has the
  DDL rights used solely by provisioning and migration. Do not run normal requests
  with the provisioning role.
- The tenant for a request is always re-derived from the authenticated account. Never
  read it from a header, query parameter, request body, or cookie field.

See `PLAN.md` Appendix A and Appendix B for the full designs and the test lists that
prove them.

## Conventions

- SQL identifiers are `snake_case`; SQL keywords upper case.
- Rust crates are named `bt-*`; modules mirror the C++ directory they came from
  (`src/calculations/IbuMethods.cpp` → `bt-calc/src/ibu.rs`).
- Every ported file gets a header comment: `Ported from src/<path> @ <git-sha>`.
- Commits: Conventional Commits. Scope by crate (`feat(bt-calc): ...`).
- Branch: work lands on `develop`. `main` tracks upstream Brewtarget.

## Watch out for

- `data/default_db.sqlite` is an **old-schema** seed (it still has `hop_in_recipe`,
  `yeast_in_recipe`, …). It is migrated forward on first load. Do not treat its table
  list as the v23 schema.
- `migrate_to_20` and `migrate_to_21` are large data transformations, not plain DDL.
  They need real porting, not SQL copy-paste.
- Boost appears in two unrelated roles: JSON (`boost/json`) and networking
  (`boost/asio`, `boost/beast`, used by the release checker). Both disappear in the
  Rust port; do not look for a single replacement.
- **MariaDB has no schema-vs-database distinction** — `CREATE SCHEMA` is a synonym
  for `CREATE DATABASE`. Relevant in Stage 2: PostgreSQL's "schema per user" maps onto
  "database per user" on the same MariaDB server. Same isolation, different DDL verb,
  different way of selecting the namespace on connect.
- `data/DefaultContent*` is ~1.6 MB of source files (820 KB as a built SQLite). In
  Stage 2 that is seeded into every per-user store, so provisioning will need to be a
  background job — another reason seeding must stay a callable function, not something
  welded into server startup.
- `src/model/` uses deep CRTP/template inheritance (`IngredientBase`, `StepBase`,
  `StockPurchaseBase`, …). The shared columns live in `*_COMMON_FIELDS` vectors in
  `ObjectStoreTyped.cpp` — resolve those when extracting a table's real column list.
