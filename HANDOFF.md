# HANDOFF

**Written**: 2026-07-29 · **Last updated**: 2026-08-05.
**Last verified green**: 2026-08-05 — `v0.1.0-alpha.2` (JS-source
compat pass). Local verification set clean: `cargo fmt --check`,
`cargo clippy --all-targets -- -D warnings`, `cargo test --no-fail-fast`
(24 unit + 16 e2e + 17 regression + 1 compat corpus + 1 cross-impl
repro = 59/0/0), `cargo audit --deny warnings`, `cargo deny check all`,
`mdbook build book` (mdbook 0.4.40 + linkcheck 0.7.7).
**Branch**: `dev` — pushed to `origin` at
<https://github.com/turnerrainer/datamapper>.
**Releases**:
- `v0.1.0-alpha.2` — tagged, pushed, published under `:alpha` floating tag on Docker Hub + GHCR.
- `v0.1.0-alpha.1` — tagged, pushed, published (2026-07-31).

Next contributor (human or Claude) must:

1. Read [`../DEV-REQUIREMENTS.md`](../DEV-REQUIREMENTS.md)
   front-to-back before touching anything. That's the
   authoritative ruleset for all Buerostack Rust projects.
2. Read [`../REFACTO-REQUIREMENTS.md`](../REFACTO-REQUIREMENTS.md) —
   this repo is a reimplementation, so the refacto ruleset applies
   on top of the base ruleset.
3. Read this file for DataMapper-specific state.
4. Read [`docs/DESIGN.md`](./docs/DESIGN.md) for domain shape.
5. Read the JS→Rust porting summary at
   [`book/src/porting-from-js.md`](./book/src/porting-from-js.md) —
   what a JS DataMapper operator needs to know.
6. Consult the in-house refacto paperwork (kept LOCAL, gitignored):
   `DIVERGENCES.md`, `MIGRATION.md`, `REFACTO-DEVIATIONS.md`,
   `docs/REFACTO-MATRIX.md`, `docs/REFACTO-PORT-PLAN.md`,
   `docs/REFACTO-AUDIT-S2.md`,
   `docs/REFACTO-AUDIT-NEGATIVE-SPACE.md`. These live under
   `Buerostack/DataMapper-on-Rust/` on the maintainer's disk but
   are NOT committed to the public repo.
7. Run the verification set (below) — every command exits 0.

## REFACTO-REQUIREMENTS compliance

Compliance landed as of `v0.1.0-alpha.2` on `2026-08-05`.
Enforcement is in-tree:

| §-ref | How enforced (public) |
|---|---|
| §1.1 coverage matrix | In-house `docs/REFACTO-MATRIX.md` (gitignored). |
| §1.3 test-corpus port plan | In-house `docs/REFACTO-PORT-PLAN.md` (gitignored). |
| §2 audit (contract preservation) | In-house `docs/REFACTO-AUDIT-S2.md` (gitignored). |
| §8.3 negative-space audit | In-house `docs/REFACTO-AUDIT-NEGATIVE-SPACE.md` (gitignored). |
| §5 divergences | In-house `DIVERGENCES.md` (gitignored). Public summary in `book/src/porting-from-js.md`. |
| §7.2 migration guide | In-house `MIGRATION.md` (gitignored). Public summary in `book/src/porting-from-js.md`. |
| §10.2 known deviations from REFACTO-REQUIREMENTS | In-house `REFACTO-DEVIATIONS.md` (gitignored). |
| §7.3 syntactic corpus | Public: `compat/js-DSL/`. |
| §7.3 CI gate on that corpus | Public: `tests/it_compat_js_dsl_corpus.rs`. |
| §4.3 cross-impl repro | Public: `tests/it_repro_cross_impl.rs`, `compat/js-server/` + `scripts/setup-repro.sh`. |
| §4.4 regression tests | Public: `tests/it_regression_refacto.rs`. |

## What this repo IS today

Working Rust re-implementation of DataMapper. Point at a folder of
`.hbs` templates → each becomes a `POST /<project>/<view>` REST
endpoint that renders the template against the JSON request body.

- `POST /:project/*view` — template folder-drop routing
- `GET /healthz` + `GET /health` — liveness probe
- Built-in `{{now}}`, `{{{json obj}}}`, `{{len items}}` helpers
- YAML config with search-path resolution
- Structured errors (413 request-too-large, 404 template-not-found,
  400 invalid JSON, 400 invalid path, 405 method-not-allowed)
- 11 sample DSLs covering the common shaping patterns
- 40 tests (24 unit + 16 integration), all green
- Multi-stage Dockerfile with non-root user + read-only rootfs
- Production-hardened `docker-compose.yml`
- Four GitHub Actions workflows ready to run on first push

## Verification set (all should exit 0)

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo build --release --bin datamapper
cargo test --no-fail-fast
cargo audit --deny warnings
( cd book && mdbook build )
```

Live smoke:

```bash
docker compose up -d --build
curl -fsS http://localhost:3000/healthz
curl -sS -X POST http://localhost:3000/samples/ping \
  -H 'content-type: application/json' -H 'type: json' -d '{}'
docker compose down
```

## Roadmap

Landed (see [CHANGELOG.md](./CHANGELOG.md) for detail):

- ✅ Task 001 — domain deep-dive (`docs/DESIGN.md`)
- ✅ Task 002 — MVP Rust rewrite

Open (`tasks/backlog/`):

| Task | Notes |
|---|---|
| 003 | OpenTelemetry `traceparent` propagation (PATTERNS.md §4) |
| 004 | Optional per-DSL JSON-schema validation |
| 005 | Helper library expansion (uppercase, fmt_date, default, …) |

## Hand-off — publishing setup

**Status: completed 2026-07-31.** The steps below are preserved as
runbook for future releases. Subsequent releases are typically just
`git push origin vX.Y.Z` — CI does the rest.

1. **Create the GitHub repo:**
   ```bash
   gh repo create turnerrainer/datamapper --public \
     --source=. --remote=origin --push
   ```
   (`--push` sends the current `dev` branch straight to origin.)

2. **Enable Pages via workflow:**
   ```bash
   gh api repos/turnerrainer/datamapper/pages -X POST \
     -f 'build_type=workflow'
   ```

3. **Bump Actions workflow permissions to Read+Write:**
   ```bash
   gh api repos/turnerrainer/datamapper/actions/permissions/workflow \
     -X PUT \
     -F 'default_workflow_permissions=write' \
     -F 'can_approve_pull_request_reviews=false'
   ```

4. **Create the Docker Hub repo** at
   <https://hub.docker.com/repositories/turnerrainer> → New
   repository → name `datamapper` → Public.

5. **Generate a scoped Docker Hub PAT** at
   <https://app.docker.com/settings/personal-access-tokens> →
   New Access Token → **Restricted access** to
   `turnerrainer/datamapper` only → **Read + Write + Delete**.

6. **Set repo secrets** (paste the token as stdin so it doesn't
   land in shell history):
   ```bash
   gh secret set DOCKERHUB_USERNAME \
     --repo turnerrainer/datamapper --body 'turnerrainer'
   echo -n '<paste-token-here>' | gh secret set DOCKERHUB_TOKEN \
     --repo turnerrainer/datamapper
   ```

7. **Watch the first-push CI go green.** `tests`, `security`,
   `docs` should all succeed on the `dev` push.

8. **Cut the first release:**
   ```bash
   git tag -a v0.1.0-alpha.1 \
     -m "DataMapper v0.1.0-alpha.1 — Rust MVP"
   git push origin v0.1.0-alpha.1
   ```
   Tag push triggers `publish.yml`: multi-arch build → Trivy →
   arch smoke test → cosign sign both registries. Watch the
   Actions tab to green.

9. **After first publish, review the auto-created GHCR package**
   at
   <https://github.com/users/turnerrainer/packages/container/datamapper/settings>.
   For a package pushed under a public user-owned repo, GHCR
   inherits **public** visibility automatically and repo access
   is already linked — no manual step needed for the initial
   release. Only revisit this if the repo is switched to private,
   or if the org later moves off `turnerrainer`.

10. **Verify from a fresh machine:**
    ```bash
    docker logout && docker pull \
      docker.io/turnerrainer/datamapper:0.1.0-alpha.1
    docker run -d --rm -p 3000:3000 \
      docker.io/turnerrainer/datamapper:0.1.0-alpha.1
    curl -fsS http://localhost:3000/healthz
    ```

11. **Update this HANDOFF** with the new "Last verified green"
    date + confirmation the container came up on a fresh host.

## Where to look for more detail

| Topic | File |
|---|---|
| Cross-project ruleset (authoritative) | [`../DEV-REQUIREMENTS.md`](../DEV-REQUIREMENTS.md) |
| Domain design (DataMapper-specific) | [`./docs/DESIGN.md`](./docs/DESIGN.md) |
| Project-specific standards addendum | [`./STANDARDS.md`](./STANDARDS.md) |
| Public docs | https://turnerrainer.github.io/datamapper/ |
| Full change history | [`./CHANGELOG.md`](./CHANGELOG.md) |
| Private security disclosure | [`./SECURITY.md`](./SECURITY.md) |
| CI workflows | [`.github/workflows/`](./.github/workflows/) |
| Task tracking | [`./tasks/`](./tasks/) |
