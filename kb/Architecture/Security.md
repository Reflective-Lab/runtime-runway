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

The verification is **online** — every request makes one outbound call to `identitytoolkit.googleapis.com/v1/accounts:lookup`. This means:

- Token revocation is effective immediately (Firebase checks its own user DB)
- There is no local signature verification; a revoked user is blocked without waiting for token expiry
- One extra network round-trip per request (~20–50 ms depending on region)

**Token lifetime:** Firebase ID tokens expire after 1 hour. Clients must call `firebase.auth().currentUser.getIdToken(true)` to refresh before expiry.

> **Known improvement path:** Replace `accounts:lookup` with offline JWKS signature verification (`jsonwebtoken` crate + Firebase public key cache). Removes the per-request hop; token revocation window becomes up to 1 hour unless explicitly checked. See `runway-auth/src/firebase.rs` — comment already marks this.

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

**Critical:** if `STRIPE_WEBHOOK_SECRET` is empty or unset, signature verification is skipped entirely. A startup assertion must guard this in production:

```rust
// TODO: add to main() for non-dev environments
assert!(
    local_dev || std::env::var("STRIPE_WEBHOOK_SECRET").map(|v| !v.is_empty()).unwrap_or(false),
    "STRIPE_WEBHOOK_SECRET must be set in production"
);
```

---

## Within the boundary — authorisation checks

Once a token is verified, `AuthContext` is injected as an Axum `Extension`. Handlers apply these guards:

| Handler | Check |
|---------|-------|
| `GET /v1/orgs/:org_id` | `org.billing_owner_uid == ctx.uid()` OR `ctx.org_id() == org_id` |
| `POST /v1/billing/checkout` | Authenticated user only (no plan check — any user can start checkout) |
| `POST /v1/billing/portal` | Org must have a `stripe_customer_id` (i.e. prior checkout completed) |
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

**Production requirement:** set `ALLOWED_ORIGINS=https://apps.reflective.se` (and any app subdomains) before public launch.

---

## Env vars summary

| Var | Purpose | Required in prod |
|-----|---------|-----------------|
| `FIREBASE_API_KEY` | Identity Toolkit auth | Yes |
| `STRIPE_SECRET_KEY` | Stripe API access | Yes |
| `STRIPE_WEBHOOK_SECRET` | Webhook HMAC verification | Yes — empty disables verification |
| `STRIPE_PRICE_STARTER_MONTHLY` | Plan mapping in webhook handler | Yes |
| `STRIPE_PRICE_TEAM_MONTHLY` | Plan mapping in webhook handler | Yes |
| `ALLOWED_ORIGINS` | CORS allowed origins | Yes — unset = open |
| `LOCAL_DEV` | Dev bypass (never in prod) | Must be absent or false |

---

## Open issues before public launch

1. **Offline JWT verification** — replace `accounts:lookup` with JWKS + signature check; eliminates per-request Firebase call and gives sub-millisecond verification.
2. **Lock `ALLOWED_ORIGINS`** — currently open if unset; must be set to exact prod domain.
3. **Guard `STRIPE_WEBHOOK_SECRET`** — add startup assertion; currently a missing secret silently disables HMAC.
4. **Role enforcement** — `role` claim is minted and stored but no handler checks it; needed before team/admin features ship.

See also: [[Architecture/Crate Map]], [[Architecture/Application]], [[Building/Deployment]]
