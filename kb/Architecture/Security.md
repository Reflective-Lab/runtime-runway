---
source: llm
---
# Security Model

How requests are authenticated and authorised across the Reflective platform boundary, within the api-server, and between internal services.

---

## Outer boundary — Firebase Bearer JWT

Every protected HTTP route is guarded by `AuthLayer` from `runway-auth`.

**Verification flow per request:**

```
Client                    api-server                     Firebase
  │                           │                              │
  │  GET /v1/accounts/me      │                              │
  │  Authorization: Bearer {token}                           │
  │──────────────────────────►│                              │
  │                           │  POST /accounts:lookup       │
  │                           │  { idToken: token }          │
  │                           │─────────────────────────────►│
  │                           │  { users: [{ localId, email, │
  │                           │    customAttributes }] }      │
  │                           │◄─────────────────────────────│
  │                           │                              │
  │  200 + body               │                              │
  │◄──────────────────────────│                              │
```

The verification is **offline** — `runway-auth` verifies the RS256 signature locally using Google's public JWKS, with no outbound call per request.

**Verification flow:**
1. Decode the JWT header to extract `kid`
2. Look up the matching JWK from the in-memory cache (TTL: 1 hour); fetch from Google if missing or expired
3. Verify RS256 signature, `iss` (`https://securetoken.google.com/{project_id}`), `aud` (project_id), and `exp`
4. Extract `sub` → `uid`, `email`, and custom claims (`org_id`, `apps`, `role`) from the payload

**Trade-off vs. online verification:** Revoked tokens remain valid until expiry (up to 1 hour). Acceptable for this use case — Firebase ID tokens are short-lived and logout is handled client-side.

**Token lifetime:** Firebase ID tokens expire after 1 hour. Clients must call `firebase.auth().currentUser.getIdToken(true)` to refresh before expiry.

---

## Claims structure

Custom claims are a JSON string stored in Firebase's `customAttributes` field and decoded into `FirebaseClaims` on every verified request:

```rust
pub struct FirebaseClaims {
    pub uid: String,            // Firebase UID — immutable user identifier
    pub email: Option<String>,  // from Firebase Auth profile
    pub org_id: Option<String>, // org the user belongs to (set by runway-accounts)
    pub apps: Vec<String>,      // app IDs with active subscriptions: ["folio", "wolfgang", ...]
    pub role: Option<String>,   // role within org: "admin" | "member" (not yet enforced)
}
```

Claims are minted by `ClaimsService` in `runway-accounts` after subscription events. They propagate on the user's next token refresh (up to 1 hour lag after a subscription change).

---

## Route categories

| Category | Auth | Gate |
|----------|------|------|
| `/status`, `/health` | None | Public |
| `/v1/billing/webhooks/stripe` | HMAC-SHA256 | Stripe signature |
| `/api/me`, `/api/events` | Firebase JWT | Valid token |
| `/v1/accounts/me`, `/v1/orgs/*`, `/v1/billing/*` | Firebase JWT | Valid token + optional app claim |

App-level entitlement gate (used per route):
```rust
AuthLayer::new(auth).requiring_app("folio")
// → 403 if claims.apps does not contain "folio"
```

Not yet applied to `runway-accounts` routes — all authenticated users can access billing endpoints regardless of subscription plan. Add `.requiring_app(...)` when paid-only features ship.

---

## Stripe webhook boundary

`POST /v1/billing/webhooks/stripe` sits in `public_routes` — no Firebase token required. Protection is HMAC-SHA256:

```
signed_payload = "{timestamp}.{raw_body}"
expected       = HMAC-SHA256(STRIPE_WEBHOOK_SECRET, signed_payload)
```

- Header parsed: `Stripe-Signature: t=timestamp,v1=hex_signature`
- Timestamp checked: must be within 5 minutes of server clock (replay protection)
- Comparison: constant-time byte-by-byte XOR fold (no early exit)

Commerce Rails owns the Stripe adapter code that performs signature mechanics,
provider receipt construction, and event mapping. Runway owns the public route
and passes the raw signed payload to that Commerce Rails-owned adapter.

**Critical:** if `STRIPE_WEBHOOK_SECRET` is empty or unset, signature
verification is skipped in local development only. Commerce Rails config rejects
empty required Stripe provider variables when `LOCAL_DEV` is not set.

---

## Within the boundary — authorisation checks

Once a token is verified, `AuthContext` is injected as an Axum `Extension`. Handlers apply these guards:

| Handler | Check |
|---------|-------|
| `GET /v1/orgs/:org_id` | `org.billing_owner_uid == ctx.uid()` OR `ctx.org_id() == org_id` |
| `POST /v1/billing/checkout` | Authenticated user only (no plan check — any user can start checkout) |
| `POST /v1/billing/portal` | Org must have a `billing_customer_ref` (i.e. prior checkout completed) |
| `GET /api/events` | `org_id` scoped to claim or query param |

**Not yet enforced:**

- `role` field — stored in claims, not checked in any handler
- Team member access beyond billing owner — a user with a matching `org_id` claim passes the org guard, but there is no invite or membership management flow yet

---

## Internal / service-to-service

**api-server → Firestore / GCS:** Authenticated via GCP service account identity. Cloud Run injects the service account token via the metadata server (`metadata.google.internal`). No credentials in code or env.

**api-server → Firebase Admin API (claims minting):** Same GCP token fetched from the metadata server, used as a Bearer token against `identitytoolkit.googleapis.com/v1/accounts:update`.

**Between runway services:** No service-to-service auth layer exists. If a second Cloud Run service calls api-server, it must present a valid Firebase Bearer token — identical to a client. There is no mTLS or shared secret between internal services.

---

## Local development bypass

`LOCAL_DEV=true` + `Authorization: Bearer dev` injects a hardcoded `AuthContext` without hitting Firebase:

```rust
FirebaseClaims {
    uid: "dev-uid",
    email: Some("dev@local"),
    org_id: Some("dev-org"),
    apps: vec!["api-server"],
    role: Some("admin"),
}
```

This must never reach a deployed environment. The `LOCAL_DEV` env var is the sole gate — ensure Cloud Run service configuration never sets it.

---

## CORS

Configured in `runway-middleware::stack` via `ALLOWED_ORIGINS` (comma-separated). If unset, `AllowOrigin::any()` applies — open to all origins.

`api-server` asserts `ALLOWED_ORIGINS` is non-empty at startup when `LOCAL_DEV` is not set.

---

## Env vars summary

| Var | Purpose | Required in prod |
|-----|---------|-----------------|
| `FIREBASE_PROJECT_ID` | JWKS iss/aud validation | Yes |
| `STRIPE_SECRET_KEY` | Commerce Rails Stripe API access | Yes |
| `STRIPE_WEBHOOK_SECRET` | Commerce Rails webhook HMAC verification | Yes — config fails if empty |
| `STRIPE_PRICE_STARTER_MONTHLY` | Commerce Rails provider price mapping | Yes |
| `STRIPE_PRICE_TEAM_MONTHLY` | Commerce Rails provider price mapping | Yes |
| `ALLOWED_ORIGINS` | CORS allowed origins | Yes — startup assertion fails if empty |
| `LOCAL_DEV` | Dev bypass (never in prod) | Must be absent or false |

---

## Open issues before public launch

1. ~~**Offline JWT verification**~~ ✅ Done — RS256 + JWKS cache in `runway-auth/src/firebase.rs`
2. ~~**Lock `ALLOWED_ORIGINS`**~~ ✅ Done — startup assertion in `api-server/src/main.rs`
3. ~~**Guard `STRIPE_WEBHOOK_SECRET`**~~ ✅ Done — Commerce Rails config fails in production when empty
4. ~~**Role enforcement**~~ ✅ Done — `role` minted into claims on provision and invite accept; `ctx.is_admin()` guards billing portal, invite management, and member management routes.

See also: [[Architecture/Crate Map]], [[Architecture/Application]], [[Building/Deployment]]
