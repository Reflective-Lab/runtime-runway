---
tags: [architecture, authority, runway, commerce]
source: llm
---
# Commerce Rails Boundary

Runway owns platform identity and runtime authority. Commerce Rails owns Reflective commercial authority.

The boundary is decided by who has authority over the consequence — not by which system first receives a request or event.

## Ownership

| Area | Owner | Rule |
|---|---|---|
| Users | Runway | Canonical identity, authentication, sessions, invites, roles, and membership |
| Organizations | Runway | Canonical tenant and organization container |
| Customer commercial org | Commerce Rails | Commercial buyer/account projection of a Runway organization |
| DevOps | Runway | Deployments, secrets, environments, runtime config, telemetry, and operational substrate |
| Subscriptions | Commerce Rails | Plans, prices, subscription state, billing state, and entitlement grants |
| Billing | Commerce Rails | Invoices, charges, refunds, revenue share, payout obligations, ledger, and reconciliation |
| Stripe transport | Runway | Secret access, webhook ingress plumbing, deployment config, and runtime observability |
| Stripe commerce adapter | Commerce Rails | Provider mapping, idempotency, webhook receipts, commercial state transitions, and reconciliation semantics |

## Organization Model

Runway owns the login and tenancy container:

```text
RunwayOrg (in runway-accounts)
  org_id
  name
  billing_owner_uid
  members            → OrgMember { uid, role, invited_by }
  plan                 → what apps are granted (subscription mirror, not source of truth)
  billing_customer_ref → provider customer reference mirror, not commercial authority
```

Commerce Rails owns the commercial projection:

```text
CustomerOrg (in commerce-rails-contracts)
  id
  runway_org_id      → reference to Runway org (not identity)
  legal/commercial name
  billing status
  provider refs
```

Runway answers who can act for an organization. Commerce Rails answers what that organization can buy, owes, receives, or is entitled to use.

## Stripe Split

Stripe crosses the boundary, but the responsibilities are not shared ambiguously.

```text
Stripe webhook HTTP request
  → Runway routes it, verifies HMAC, provides secret access, observes runtime health
  → Commerce Rails Stripe adapter verifies provider semantics and records WebhookReceipt
  → Commerce Rails gates apply idempotency, replay, policy, and HITL checks
  → Commerce Rails updates Subscription, EntitlementGrant, LedgerEntry, or payout state
```

Today, `runway-accounts` mirrors basic plan/app state onto `RunwayOrg` so the auth layer can mint `apps` claims without calling Commerce Rails. That mirror is eventually consistent — Commerce Rails is the source of truth for entitlement.

## Make.com / Automation

Webhook HTTP plumbing (secret storage, routing, Cloud Function deployment) belongs to Runway. The business automation downstream — what happens when a partner applies, when a subscription is created, what emails go out — belongs to Commerce Rails.

## Rule

If the question is **who can log in, where code runs, where secrets live, or how the runtime is operated** → Runway.

If the question is **who pays, what is owed, what is granted, what is refundable, what must be reconciled, or what commercial state is accepted** → Commerce Rails.

See also: [[Architecture/Security]], [[Architecture/Crate Map]], [[Operations/Secrets]]
