---
source: llm
---
# URL Model

All Reflective apps share a single Firebase Hosting entry point at `apps.reflective.se`. Every app gets a path-prefix URL that stays valid even after the app acquires its own domain.

## Structure

```
apps.reflective.se/{app-name}/**         →  rolling latest
apps.reflective.se/{app-name}/v{N}/**    →  frozen major version (separate Cloud Run service)
```

The path prefix is the Cloud Run `ROUTE_PREFIX` env var. The Cloud Run service mounts its routes under that prefix. `/health` always stays at the root path for Cloud Run's own health checks.

## Live services

| App | Firebase path | Cloud Run service | Project |
|-----|--------------|-------------------|---------|
| api-server | `/api-server/**` | `api-server` | `wolfgang-kb-prod` |

New apps: add a `run` rewrite block in `ops/infra/firebase/apps/firebase.json` and run `just apps-deploy`.

## Versioning

### What a deploy produces

```
apps.reflective.se/api-server/**
  → Cloud Run traffic config → latest revision (100%)

https://v3-4-1---api-server-{hash}-ew.a.run.app/api-server/**
  → Cloud Run revision api-server-00003-xxx pinned forever

https://sha-bdccdc7---api-server-{hash}-ew.a.run.app/api-server/**
  → Same revision, pinned by git SHA
```

Version and SHA tags are set automatically by `ops/scripts/deploy-api-server.sh` after every deploy. Tags survive future deploys — the tagged revision URL is stable for the lifetime of the Cloud Run service.

### Freezing a major version

When a breaking change ships (v4) and frontends need time to migrate, freeze v3 as a separate Cloud Run service:

```bash
just api-freeze 3
```

This:
1. Deploys `api-server-v3` Cloud Run service with `ROUTE_PREFIX=/api-server/v3`
2. Tags its revision `v3-...` and `sha-...`
3. Prints the `firebase.json` rewrite block to add:
   ```json
   {
     "source": "/api-server/v3/**",
     "run": { "serviceId": "api-server-v3", "region": "europe-west1", "projectId": "wolfgang-kb-prod" }
   }
   ```
4. You add that block **before** the `/api-server/**` catch-all and run `just apps-deploy`

Result: `apps.reflective.se/api-server/v3/**` routes to the frozen service indefinitely. `apps.reflective.se/api-server/**` continues rolling forward.

### Why not Firebase Hosting for pinned revisions?

Firebase Hosting's `run` rewrite always routes through a service's traffic config — there is no way to pin to a specific revision from Hosting. Pinned revision URLs (the `v3-4-1---...` form) are direct Cloud Run URLs, not Firebase Hosting paths. They are stable and usable by frontends that need an exact version, but they bypass Hosting headers and rewrites. For the `apps.reflective.se/v3/**` pattern, a separate Cloud Run **service** is the correct unit.

## Custom domain

`apps.reflective.se` currently resolves to `apps-reflective-se.web.app`. To activate the custom domain:

1. Firebase Console → Hosting → `apps-reflective-se` → Add custom domain → `apps.reflective.se`
2. Add `CNAME apps.reflective.se → apps-reflective-se.web.app` in DNS
3. Firebase issues a managed TLS cert automatically (~minutes)

## Adding a new app

1. Deploy the Cloud Run service with `ROUTE_PREFIX=/{app-name}`:
   ```bash
   SERVICE_NAME={app-name} ROUTE_PREFIX=/{app-name} bash ops/scripts/deploy-{app-name}.sh
   ```
2. Add a rewrite to `ops/infra/firebase/apps/firebase.json`:
   ```json
   {
     "source": "/{app-name}/**",
     "run": { "serviceId": "{app-name}", "region": "europe-west1", "projectId": "wolfgang-kb-prod" }
   }
   ```
3. Run `just apps-deploy`

The app is immediately available at `apps-reflective-se.web.app/{app-name}/**` and at `apps.reflective.se/{app-name}/**` once the custom domain is live.

See also: [[Building/Deployment]]
