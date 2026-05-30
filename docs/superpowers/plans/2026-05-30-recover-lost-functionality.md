# Recover Lost Functionality from Helm Boundary Refactor

> **For agentic workers:** This is an audit-then-recover plan, not pure greenfield implementation. Each item starts with a discovery step (read pre-deletion git state, decide intent), then implements only what's worth keeping.

**Date:** 2026-05-30
**Triggered by:** Honest functionality audit after the Helm boundary refactor (Phases 1–9 + 11 + 12 of the `2026-05-28-runway-helm-app-host-boundary` plan). Deletions in Phases 6a and 9 dropped code without a route-by-route audit of consumer impact.

**Goal:** Decide what to bring back and where. Some deletions were intentional (5 subscription truths → Movement). Others may have been collateral. Surface the gaps, make per-item recover/drop decisions, then implement.

**Working assumption:** Default to **drop** unless an item has a real consumer or product reason to exist. The refactor's whole point was to shed monolithic surface area; reviving everything reverses the wins.

---

## State at start of plan

| Repo | Head |
|---|---|
| runway | `d054908` (Phase 1 + 1.5 + 1.6 + Commerce Rails + runway-storage hardening) |
| helms | `9244426` (Phase 3a + 3b + 4b + 5 + 6a + 9 — `application-server` deleted) |
| atelier-showcase | `fd35148` (Phase 2 + 6) |
| catalyst-biz | `e18aa39` (Phase 6a + 12) |
| quorum-sense | `162ddf5` (Phase 8) |
| atlas-integration | `ea7d029` (Phase 11) |

**All 7 marquee apps that have backends are migrated to `RunwayAppHost`:** Quorum, Atlas, Catalyst, Tally-escrow, Fathom-narrative, Plumb-execution, Scout-sourcing. The other 6 directories in `marquee-apps/` (kb, keystone-architecture, shoal-meta, triage-keeper, vouch-lending, warden-compliance) have no backend code yet — they'll be RunwayAppHost-native when built. No migration backlog.

`just spike-1-smoke` passes all 4 stages (happy path only).

---

## Items to triage

### Item A — `application-server/src/http_api.rs` route audit (~2,200 lines deleted)

**What we know:** Phase 3a extracted only 2 operator-control preview routes from `http_api.rs` (which was 2,302 lines). The other ~2,200 lines were deleted with the crate in Phase 9. No one audited route-by-route what was in there.

**What's at risk:** any HTTP route that wasn't gRPC (those moved in Phase 6b) and wasn't operator-control. Likely candidates based on the original `application-server` shape: workbench dashboard, account workspace summaries, approvals listing, system profile, capability-module listing, truth catalog browsing, organization listing, workflow case listing — all of these had HTTP read-models per the `workbench_backend` types we noted during Phase 1.5.

**Discovery steps:**

- [ ] **A1: Recover the pre-deletion file from git**
  ```bash
  cd /Users/kpernyer/dev/reflective/stack/bedrock-platform/helms
  git show af4cd23^:crates/application-server/src/http_api.rs > /tmp/old-http-api.rs
  wc -l /tmp/old-http-api.rs    # expect ~2,302
  ```

- [ ] **A2: Extract route table**
  ```bash
  grep -nE '\.route\(|axum::routing::|async fn .+ \(.+State<' /tmp/old-http-api.rs > /tmp/old-http-api-routes.txt
  ```
  Hand-walk the output. For each route, classify:
  - **Operator-control** (`/v1/workbench/operator-control/*`) — already in `helm-operator-control`, ignore
  - **Workbench** (`/v1/workbench/*` non-operator-control) — workbench dashboard etc.; candidate for `helm-workbench` crate or fold into `helm-operator-control` if small
  - **System** (`/v1/system/*`) — system profile; candidate for runway-app-host
  - **Approvals** (`/v1/approvals/*`) — already in runway-app-host transport
  - **Capability/Module** registry routes — candidate for a new `helm-registry` crate
  - **Catalog browse** (truths, modules) — candidate for `helm-truth-execution` (browse alongside execute)
  - **Domain** (parties, opportunities, etc.) — already moved in Phase 6b atelier scenario
  - **One-off** — drop if no plausible consumer

- [ ] **A3: For each "candidate" route, grep across all 7 migrated apps for `fetch("/v1/...")` or gRPC client calls. Any frontend or backend that targets the route is a real consumer; deletion broke it.**
  ```bash
  for path in $(grep -oE '/v1/[^"]+' /tmp/old-http-api-routes.txt | sort -u); do
    echo "=== $path ==="
    rg -l "$path" /Users/kpernyer/dev/reflective/marquee-apps/ 2>/dev/null
  done
  ```

**Decision template (one row per route):**

| Route | Consumers found | Action |
|---|---|---|
| `/v1/system/profile` | (list) | move to runway-app-host / move to helm-X / drop |

**Recovery steps (per-route, once classified):**

- [ ] **A4: For "move to runway-app-host" rows** — implement each route as a host-mounted endpoint alongside `/status` and `/healthz`. Likely small read-models, no state required.
- [ ] **A5: For "move to helm-workbench" rows** — create new crate `helm-workbench` under `helms/crates/` if there's >1 route; otherwise fold into `helm-operator-control`.
- [ ] **A6: For "move to helm-registry" rows** — same shape; new crate if warranted.
- [ ] **A7: Run `just spike-1-smoke`** and any per-app smoke that exists. Verify recovery didn't break the cross-app contract.

**Effort estimate:** 2–4 hours (most is reading + decision; implementation likely <500 lines total because most routes were thin read-models).

---

### Item B — `IdentityGrpc`, `TruthCatalogGrpc`, `ModuleRegistryGrpc` gRPC services audit

**What we know:** Three gRPC services (out of 10) in `application-server/src/service.rs` were NOT moved to atelier-showcase in Phase 6b because they were tagged "platform" — Identity, TruthCatalog, ModuleRegistry. They stayed in `application-server`. Phase 9 then deleted the whole crate.

**What's at risk:**

- **`IdentityGrpc`** — auth-adjacent. Most likely real consumers (workbench frontend, possibly desktop). The 7 migrated marquee apps all do their own auth via `runway-auth` / Firebase, so they're not consumers. But Helm's own desktop UI almost certainly was.
- **`TruthCatalogGrpc`** — browse the truth catalog (vs execute). Workbench-side feature.
- **`ModuleRegistryGrpc`** — module discovery. Workbench-side feature.

**Discovery steps:**

- [ ] **B1: Recover the pre-deletion service.rs**
  ```bash
  cd /Users/kpernyer/dev/reflective/stack/bedrock-platform/helms
  git show af4cd23^:crates/application-server/src/service.rs > /tmp/old-service.rs
  ```

- [ ] **B2: Pull out the impl blocks for the three services + the proto definitions they implement.** Note method names per service.

- [ ] **B3: Grep frontend code for any of the gRPC service client paths.** The `apps/desktop/src-tauri/` directory in helms likely has Rust clients. The Tauri/Svelte frontend may have gRPC-web clients.
  ```bash
  rg -l "IdentityService|TruthCatalogService|ModuleRegistryService" \
    /Users/kpernyer/dev/reflective/stack/bedrock-platform/helms/apps/ \
    /Users/kpernyer/dev/reflective/marquee-apps/ \
    2>/dev/null
  ```

**Decision criteria:**

- If any consumer found → recover the service. Likely home: a new `helm-platform-services` crate, or fold each into the responsibility-aligned crate (Identity → `runway-auth`? TruthCatalog → `helm-truth-execution`? ModuleRegistry → `helm-operator-control`?).
- If no consumer found → confirm safe to leave deleted, document the decision in `kb/Architecture/Application Server Deletion.md` so it's not surprising later.

**Recovery steps (if any consumer found):**

- [ ] **B4: For each service with consumers, identify the simplest home crate** based on what the service does (auth → runway, truth-browse → helm-truth-execution, module discovery → helm-operator-control or helm-registry).
- [ ] **B5: Copy the service impl into that crate**, replacing `crate::truth_runtime::*` paths with the post-refactor equivalents (Phase 5 / Phase 3b pattern).
- [ ] **B6: Re-mount the gRPC service in the consumer app's `main.rs`** via `HelmModule::grpc_services()`.
- [ ] **B7: Build + smoke + Helm desktop sanity** (if any desktop UI touched).

**Effort estimate:** 1–3 hours. If consumers are zero, this is a 30-minute decision-and-document. If real, ~2 hours per service to relocate.

---

### Item C — 5 subscription truth bodies → commerce-rails

**What we know:** Phase 6a deleted these:
- `activate_subscription.rs`
- `upgrade_subscription_plan.rs`
- `refill_prepaid_ai_credits.rs`
- `suspend_service_on_payment_failure.rs`
- `reconcile_model_usage_against_customer_ledger.rs`

Intentional deletion — Movement territory. But the truth keys themselves no longer dispatch. Any caller asking `helm-truth-execution` to execute `activate-subscription` gets `"no truth body registered"`.

The user has separately landed `commerce-rails/crates/commerce-rails-stripe` (referenced from `runway-accounts`). That's the new home, but the truth-style entry points may not have been reimplemented yet.

**Discovery steps:**

- [ ] **C1: Recover the 5 truth bodies from git**
  ```bash
  cd /Users/kpernyer/dev/reflective/stack/bedrock-platform/helms
  git show a63811c^:crates/application-server/src/truth_runtime/activate_subscription.rs > /tmp/activate_subscription.rs
  # repeat for the other 4 files
  ```

- [ ] **C2: Survey `/Users/kpernyer/dev/reflective/commerce-rails/`** for whether equivalent business logic already exists. If yes, the truth wrappers are just dispatch shells.
  ```bash
  ls /Users/kpernyer/dev/reflective/commerce-rails/crates/
  rg -l "activate_subscription|upgrade_subscription_plan|reconcile_model_usage" \
    /Users/kpernyer/dev/reflective/commerce-rails/ 2>/dev/null
  ```

- [ ] **C3: Decide: are these "truths" in the platform-truth sense, or are they commerce operations that don't need the truth contract at all?** The truth pattern carries `JobReadinessPacket` + receipt families — meaningful for HITL governance. Subscription operations may or may not warrant that.

**Recovery options:**

1. **As real truths** — implement as `TruthBody` impls in a new `commerce-rails/crates/commerce-rails-truths` crate. Apps register them with `helm-truth-execution` like Catalyst does its 3 truths.
2. **As plain commerce-rails operations** — drop the truth dispatch wrapper, expose the operation as a regular method on `commerce-rails-stripe` or similar. Callers invoke directly instead of through the truth dispatcher.
3. **Both** — implement the operation in commerce-rails, then wrap as a `TruthBody` impl that delegates. Best of both worlds at modest cost.

**Recommendation:** Option 3 if HITL gates around subscription operations matter (likely they do — operators may want to review a refund or a plan downgrade before it fires).

**Steps once option chosen:**

- [ ] **C4: Implement the 5 operations in commerce-rails** following the rest of commerce-rails-stripe's pattern.
- [ ] **C5: If option 3: create `commerce-rails-truths` crate with 5 `TruthBody` impls.**
- [ ] **C6: Document in `kb/Architecture/Truth Dispatch.md`** how to register commerce-rails truths from a marquee app.

**Effort estimate:** 4–8 hours if implementing as truths (5 × ~1.5h). 2–3 hours if implementing as plain operations.

---

### Item D — `generate_data_transformer` evaluation

**What we know:** 482-line truth body, `#[cfg(test)]`-gated, noted as "TODO Phase 9 evaluation" by the Phase 6a implementer. Deleted with `application-server` in Phase 9.

**What it does (per the implementer's report):** "Generic convergence experiment (EXP-002) proving code-gen as a convergence step — no CRM domain logic, no app-specific imports."

**Decision criteria:**

- If the experiment is still active or referenced in any KB doc / experiments tracker → recover it
- If the experiment is closed → confirm dropped

**Discovery steps:**

- [ ] **D1: Recover the file**
  ```bash
  git show a63811c^:crates/application-server/src/truth_runtime/generate_data_transformer.rs > /tmp/generate_data_transformer.rs
  ```

- [ ] **D2: Search KB and experiments dirs for "EXP-002" or "data_transformer"**
  ```bash
  rg -l "EXP-002|data_transformer|data transformer" \
    /Users/kpernyer/dev/reflective/stack/bedrock-platform/helms/kb/ \
    /Users/kpernyer/dev/reflective/stack/bedrock-platform/helms/experiments/ \
    2>/dev/null
  ```

- [ ] **D3: Decide.** If active: relocate to `helms/experiments/` as a standalone crate (since it's an experiment, not platform). If closed: document the decision.

**Effort estimate:** 30 min if closed, 1–2 hours if active.

---

### Item E — `spike-1-smoke` gate edge cases

**What we know:** Smoke validates only the happy path (job starts, gate pauses, approval-approved, job completes). Phase 4b's HITL flow rewrite was claimed to preserve gate-rejected and timeout behavior, but neither path is tested.

**Recovery isn't the right word** — this is adding test coverage that didn't exist before. But it's listed here because Phase 4b's contract gap (the original `RealtimeHub` had 600s gate timeout + rejection path that the redo preserved but never exercised) means we're flying blind on those flows.

**Steps:**

- [ ] **E1: Read `atlas-integration/Justfile:33` (the spike-1-smoke target)** and the underlying script to understand the current shape.

- [ ] **E2: Add a "gate-rejected" stage** — fire the job, wait for `gate.paused`, hit `/v1/approvals/{ref}/reject` instead of approve, assert the resulting event sequence shows `gate.rejected → job.completed` with rejected outcome (or `job.failed`, depending on the original semantics — match what Phase 4b's redo actually does).

- [ ] **E3: Add a "gate timeout" stage** — fire the job, wait for `gate.paused`, then deliberately do nothing for >600s. Assert the timeout fires and produces a `job.failed` (or equivalent) event.

   The 600s wall-time is too slow for routine smoke. Two options:
   - Configurable timeout: thread the timeout through `JobStreamState` so tests can pass a 10s timeout.
   - Separate test fixture: a `helm-governed-jobs` integration test, not part of spike-1-smoke.

- [ ] **E4: Add edge stages to `atlas-integration` smoke OR to a new `helms/crates/helm-governed-jobs/tests/timeout_test.rs`.**

- [ ] **E5: Run the full extended smoke. All happy + reject + timeout stages must pass.**

**Effort estimate:** 2–3 hours. Most is wiring; the actual assertions are short.

---

### Item F — *(removed)*

Originally a Warden-compliance migration item. Audit revealed warden-compliance (and 5 other directories in `marquee-apps/`) has no backend code yet, so there's nothing to migrate. They'll be RunwayAppHost-native when eventually built.

---

## Execution order

Each item is independent; order is just a recommendation:

1. **Item A (http_api.rs audit)** — biggest unknown surface area, settle it first so subsequent items don't re-tread the same ground.
2. **Item B (gRPC services audit)** — closely related to A; likely some overlap on consumer surveys.
3. **Item E (smoke edge cases)** — independent, can land anytime. Doing it before commerce/subscription work means we're defensively covered if Item C uncovers HITL gate behavior we want to validate.
4. **Item C (subscription truths → commerce-rails)** — needs decision on truth-wrapping pattern; biggest design surface.
5. **Item D (data_transformer evaluation)** — smallest, leave for last unless it surfaces during Item A as needed.

## Effort summary

| Item | Hours (low) | Hours (high) |
|---|---|---|
| A: http_api.rs audit | 2 | 4 |
| B: gRPC services audit | 1 | 3 |
| C: subscription truths | 2 | 8 |
| D: data_transformer | 0.5 | 2 |
| E: smoke edge cases | 2 | 3 |
| **Total** | **7.5** | **20** |

A single focused day on the low end. Two days max if Item C goes the full "implement as truths" route.

---

## Acceptance criteria for the whole plan

- [ ] Per-route decision table for `http_api.rs` exists and is committed to `kb/Architecture/` (decisions are documented even when the answer is "dropped")
- [ ] No silent functionality loss — every deleted route either recovered or explicitly documented as intentionally dropped
- [ ] `just spike-1-smoke` extended with reject + timeout stages, all passing
- [ ] Subscription truth keys either dispatch through commerce-rails-truths or callers updated to invoke the operations directly
- [ ] `generate_data_transformer` resolved (recovered to experiments dir or documented as dropped)
- [ ] `kb/Architecture/Application Server Deletion.md` exists summarizing what was kept, moved, and dropped (single source of truth for future archaeology)

## Disposition notes

This plan is **not on a timer**. Each item asks "did we lose something that matters?" — a question that benefits from careful per-item judgment, not session-pressure decisions. Pick items up individually when the answer becomes clearer or when something downstream (a frontend feature, a billing flow) surfaces a missing dependency. The plan exists so nothing slips through the cracks, not as a forcing function.
