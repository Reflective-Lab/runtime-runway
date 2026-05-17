---
tags: [architecture, authority, runway, movement, commerce]
source: llm
---
# Movement Boundary

Runway owns platform identity and runtime authority. Movement (Commerce Rails, at `reflective/movement/commerce-rails/`) owns Reflective commercial authority.

The boundary is decided by who has authority over the consequence — not by which system first receives a request or event.

## Ownership

| Area | Owner | Rule |
|---|---|---|
| Users | Runway | Canonical identity, authentication, sessions, invites, roles, and membership |
| Organizations | Runway | Canonical tenant and organization container |
| Customer commercial org | Movement | Commercial buyer/account projection of a Runway organization |
| DevOps | Runway | Deployments, secrets, environments, runtime config, telemetry, and operational substrate |
| Subscriptions | Movement | Plans, prices, subscription state, billing state, and entitlement grants |
| Billing | Movement | Invoices, charges, refunds, revenue share, payout obligations, ledger, and reconciliation |
| Stripe transport | Runway | Secret access, webhook ingress plumbing, deployment config, and runtime observability |
| Stripe commerce adapter | Movement | Provider mapping, idempotency, webhook receipts, commercial state transitions, and reconciliation semantics |

## Organization Model

Runway owns the login and tenancy container:

```text
RunwayOrg (in runway-accounts)
  org_id
  name
  billing_owner_uid
  members            → OrgMember { uid, role, invited_by }
  plan               → what apps are granted (subscription mirror, not source of truth)
  stripe_customer_id → Stripe transport reference, not commercial authority
```

Movement owns the commercial projection:

```text
CustomerOrg (in commerce-rails-contracts)
  id
  runway_org_id      → reference to Runway org (not identity)
  legal/commercial name
  billing status
  provider refs
```

Runway answers who can act for an organization. Movement answers what that organization can buy, owes, receives, or is entitled to use.

## Stripe Split

Stripe crosses the boundary, but the responsibilities are not shared ambiguously.

```text
Stripe webhook HTTP request
  → Runway routes it, verifies HMAC, provides secret access, observes runtime health
  → Movement Stripe adapter verifies provider semantics and records WebhookReceipt
  → Movement escapement applies idempotency, replay, policy, and HITL gates
  → Movement updates Subscription, EntitlementGrant, LedgerEntry, or payout state
```

Today, `runway-accounts` mirrors basic plan/app state onto `RunwayOrg` so the auth layer can mint `apps` claims without calling Movement. That mirror is eventually consistent — Movement is the source of truth for entitlement.

## Make.com / Automation

Webhook HTTP plumbing (secret storage, routing, Cloud Function deployment) belongs to Runway. The business automation downstream — what happens when a partner applies, when a subscription is created, what emails go out — belongs to Movement's operational layer.

## Rule

If the question is **who can log in, where code runs, where secrets live, or how the runtime is operated** → Runway.

If the question is **who pays, what is owed, what is granted, what is refundable, what must be reconciled, or what commercial state is accepted** → Movement.

See also: [[Architecture/Security]], [[Architecture/Crate Map]], [[Operations/Secrets]]
