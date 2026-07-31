# HANDOFF

**Written**: 2026-07-29
**Last verified green**: 2026-07-29 — `cargo test` 24 unit + 16
integration = 40/0/0; `cargo fmt --check` clean;
`cargo clippy --all-targets -- -D warnings` clean; `cargo audit
--deny warnings` clean (200 crate deps); `cargo deny check all`
clean; `mdbook build book` clean (mdbook 0.4.40 + linkcheck 0.7.7);
container image built + smoke-tested locally (healthz, samples/ping,
samples/echo, samples/arrays/map_products, 404, path-traversal 400,
405 on POST /healthz — all as expected).
**Branch**: `dev` — one commit (`7a9d3fd`), not yet pushed to a
GitHub remote (the repo does not yet exist).
**Release**: `v0.1.0-alpha.1` — NOT yet tagged locally. Cut the tag as
step 8 in "Hand-off — publishing setup" below, once the GitHub repo
exists and the first CI push has gone green.

Next contributor (human or Claude) must:

1. Read [`../DEV-REQUIREMENTS.md`](../DEV-REQUIREMENTS.md)
   front-to-back before touching anything. That's the
   authoritative ruleset for all Buerostack Rust projects.
2. Read this file for DataMapper-specific state.
3. Read [`docs/DESIGN.md`](./docs/DESIGN.md) for domain shape.
4. Run the verification set (below) — every command exits 0.

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

## Hand-off — publishing setup (I stopped here)

The four GitHub Actions workflows are in place. Everything below
needs the operator's credentials / one-time console clicks and is
NOT automated — DEV-REQUIREMENTS §9.1 gates these on human review.

Run these in order the first time; subsequent releases are just
`git push origin vX.Y.Z`.

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

9. **After first publish, link the auto-created GHCR package**
   back to the repo at
   <https://github.com/users/turnerrainer/packages/container/datamapper/settings>
   → **Change visibility** to **Public** if desired → **Manage
   Actions access** → link `turnerrainer/datamapper` with
   **Write** role. (Without this, subsequent `GITHUB_TOKEN`
   pushes to GHCR fail with a permissions error.)

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
| Public docs | https://turnerrainer.github.io/datamapper/ (after first push) |
| Full change history | [`./CHANGELOG.md`](./CHANGELOG.md) |
| Private security disclosure | [`./SECURITY.md`](./SECURITY.md) |
| CI workflows | [`.github/workflows/`](./.github/workflows/) |
| Task tracking | [`./tasks/`](./tasks/) |
