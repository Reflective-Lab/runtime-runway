# App Execution Container

Runtime Runway owns the standard server execution container for Reflective apps.

This is a hard architectural direction: marquee apps should not each invent
their own HTTP, gRPC, GraphQL, auth, telemetry, secrets, realtime, or deployment
host. Apps instantiate the Runtime Runway container with an app packet. Helm mounts
operator-control and governed-job modules into that container. Axiom, Organism,
Converge, and Mosaic keep their own lower-layer authority.

## Decision

The deployable unit for an app backend should be:

```text
Runtime Runway execution container
  -> Runtime Runway auth, middleware, telemetry, secrets, storage, and deployment
  -> Helm operator-control and governed-job routes
  -> app packet: app id, truths, projections, subject refs, fixtures, copy
  -> optional domain routes when the app has real product-specific HTTP
```

The app should not own the generic server. It owns domain meaning.

## Why This Exists

The current `application-server` shape in Helm is useful, but it mixes two
responsibilities:

- a generic execution host that any app needs;
- Helm-specific operator-control, jobs, approvals, and projections.

That mix is a bad smell. It encourages Catalyst, Tally, Quorum, Fathom, Atlas,
and later apps to either call a Helm server as if it were the platform, or to
copy the same server concerns into each app. The right split is to move the
host responsibility to Runtime Runway and keep Helm as a mounted operator module.

## Ownership

| Layer | Owns | Must not own |
|---|---|---|
| Runtime Runway | process lifecycle, ports, Cloud Run packaging, health, auth, middleware, CORS, secrets, telemetry, storage, append-only event log, public transport defaults | domain truth semantics, operator-control authority, convergence rules, specialist cores |
| Helm | operator-control read models, governed job surface, HITL approvals, readiness packets, receipt views, workbench/client contracts | deployment substrate, app-specific business authority, lower-layer specialist implementations |
| App | product UX, domain truths, fixtures, app subject refs, projections, copy, product-specific routes | reusable server host, generic realtime parsing, auth/secrets/telemetry stacks, generic HTTP/gRPC/GraphQL frameworks |
| Axiom | truth validation, intent artifacts, compiled invariants, calibration doctrine | hosting, operator control, deployment |
| Organism/Converge/Mosaic | formations, fixed-point execution, promotion, receipts, specialists | app deployment topology |

## Packet Shape

The first implementation is intentionally boring and typed. The
`runway-app-host` crate exposes the packet and host bootstrap shape:

```rust
RunwayAppHost::from_env(catalyst_packet())
    .await?
    .serve(public_routes, protected_routes)
    .await?
```

The app packet should carry:

- `app_id`;
- app display metadata;
- truth/job registrations;
- operator-control packet registrations;
- app subject-ref codecs;
- projection/writeback adapters;
- fixture/demo seeds for local development;
- auth app-scope requirements;
- optional domain routes.

It should not carry auth implementation, middleware implementation, telemetry
bootstrap, event-log implementation, or container/deployment policy.

## Protocol Defaults

HTTP plus SSE is the default public app surface because browsers and desktop
webviews consume it directly.

gRPC is for internal typed service-to-service paths or lower-level runtime
streams where the client is controlled.

GraphQL is not a default app backend style. It can become a read/query facade
later if portfolio-wide projection browsing needs it, but it should not be
introduced independently by each app.

## Migration Path

1. Keep Helm `application-server` working as the reference host while the
   contract is extracted.
2. Define a typed app packet around the Catalyst proof first.
3. Extract Runtime Runway host construction from `crates/api-server` into
   `crates/runway-app-host`.
4. Mount Helm operator-control/job routes into that Runtime Runway host.
5. Move Catalyst from "calls Helm application-server" to "runs in Runtime Runway
   container with Helm module mounted".
6. Repeat with Tally, Quorum, Fathom, Warden, Plumb, Atlas, and the rest of the
   marquee apps.
7. Retire or shrink any app-local server crates to product-specific route
   adapters.

## Guardrails

- No new app-owned server framework unless it is explicitly temporary and has a
  deletion path.
- No app-local auth, secrets, telemetry, CORS, health, or deployment bootstrap.
- No app-local realtime parser when the Helm/Runtime Runway envelope exists.
- No GraphQL-per-app experiment until there is a portfolio-level query contract.
- New generic routes belong in Helm if they are operator-control semantics, and
  in Runtime Runway if they are host/runtime/deployment semantics.

## Immediate Priority

For the rest of the current workday, this is higher priority than adding more
app probes:

1. make the contract explicit in Runtime Runway, Helm, and marquee-apps docs;
2. identify the minimal `AppExecutionPacket` for Catalyst;
3. decide which pieces of Helm `application-server` are host concerns versus
   Helm module concerns;
4. implement the first Runtime Runway-hosted Catalyst path before adding another
   app-owned backend pattern.

Current slice:

- `runway-app-host` defines `AppExecutionPacket` and route ownership metadata.
- Catalyst uses the Runtime Runway host for telemetry, storage, auth, route prefixing,
  status, health, and middleware.
- Catalyst still marks Helm job/operator routes as planned module mounts until
  Helm exposes them as a mountable router.
