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

---

## h2ck.me security-audit pipeline

**Added**: 2026-09-06. Describes the ongoing pre-publication security audit + fix + review flow with the `h2ckme` private GitHub org. If you land in this repo cold and see an open `fix/h2ck-v1-audit` PR, start here.

### What it is

h2ck.me runs a versioned audit → fix → validate cycle against every Bürostack-fleet service before it goes public. Each round is a `vN/` folder in the corresponding private repo under [`github.com/h2ckme`](https://github.com/h2ckme):

- `vN/AUDIT.md` — findings by severity, file:line pointers, attack scenarios.
- `vN/FIX-KIT.md` — runnable attack sandbox, diff-shaped fix code, per-finding acceptance criteria.
- `vN/PR-REVIEWS/<pr-number>-<head-sha7>.md` — one per PR reviewed.

**Fleet-wide index** — [`h2ckme/security-fleet` → `REVIEW-INDEX.md`](https://github.com/h2ckme/security-fleet/blob/main/REVIEW-INDEX.md).

### Where feedback lives (hybrid pipeline as of 2026-09-06)

1. **The open v1 audit PR carries a comment** starting with `## h2ck.me v1 review`.
2. **Full per-PR write-up** at [`h2ckme/DataMapper-on-Rust/v1/PR-REVIEWS/`](https://github.com/h2ckme/DataMapper-on-Rust/tree/main/v1/PR-REVIEWS).
3. **Audit + fix-kit context**: [`h2ckme/DataMapper-on-Rust/v1/AUDIT.md`](https://github.com/h2ckme/DataMapper-on-Rust/blob/main/v1/AUDIT.md) + [`v1/FIX-KIT.md`](https://github.com/h2ckme/DataMapper-on-Rust/blob/main/v1/FIX-KIT.md).

**h2ckme access**: `git clone git@github.com:h2ckme/DataMapper-on-Rust.git` (private, read via org membership).

### Open v1 PR on this repo

| PR | Branch | Findings | h2ck.me verdict |
|---|---|---|---|
| [#3](https://github.com/turnerrainer/DataMapper/pull/3) | `fix/h2ck-v1-audit` | M1 HTML-safety docs, M2 `text/plain` fallback default, M3 cap-aware writer, I1 refuse-to-start on `cors_origin`, L1 writable-DSL WARN | ✅ pass (5 findings, 66/66 tests, `cargo audit` + `cargo deny` + `clippy -D warnings` clean) |

### Standout in the fix

The **`CappedWriter`** in `src/renderer.rs` handles cap enforcement mid-render (via `render_template_to_write`), replacing the buffer-then-check anti-pattern that allowed transient GiB-scale allocations. Edge-case handling is thoughtful: `saturating_add` for `cap = usize::MAX`, initial buffer bounded by `cap.min(4096)`, `into_string` uses `from_utf8_lossy` defensively so a hypothetical broken renderer can't panic the request path. h2ck.me flagged this as extraction candidate for a future `buerostack-security::render` crate.

The **`accepts_html` is explicit-only** — `*/*` correctly does NOT count as HTML opt-in. This was the M2 bug; it's now regression-pinned with 3 positive + 3 negative tests.

### Next action for a maintainer landing here

1. **Open [PR #3](https://github.com/turnerrainer/DataMapper/pull/3)** and read the `## h2ck.me v1 review` comment.
2. Follow the link for the acceptance-marker table + break-the-fix probes.
3. **Merge** on your release cadence (verdict is ✅ pass; DataMapper stays 🟢 SHIP).
4. Bump version + tag + push image.
5. **Wait ~2 weeks**, then h2ck.me opens `v2/` as an adversarial re-audit.

### h2ck.me does NOT touch this repo

Explicit boundary: h2ck.me writes only to `h2ckme/*` (private org) + PR comment threads. It never pushes code, opens PRs, or edits files in `turnerrainer/*`.
