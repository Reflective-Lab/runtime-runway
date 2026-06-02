---
source: llm
---
# Secrets and Environment Variables

Operational reference for secrets across the Reflective infrastructure. The long-term intent is that runway owns all API surface and all secrets migrate here. Current state documents where each secret lives today.

---

## GCP Projects in play

| Project | Purpose |
|---------|---------|
| `wolfgang-kb-prod` | runway api-server (Cloud Run), Firebase Auth, Firestore, GCS |
| `converge-369ad` | apps.reflective.se Firebase Hosting + Cloud Functions (interim) |

As runway matures, Firebase Functions in `converge-369ad` will be replaced by runway-accounts endpoints, and their secrets will migrate to `wolfgang-kb-prod` as Cloud Run secret mounts.

---

## runway api-server (wolfgang-kb-prod)

Environment variables are set at Cloud Run deploy time via `cloudbuild.api-server.yaml` and the `just api-deploy` recipe. See [[Building/Deployment]] for the full variable table.

Sensitive values (`FIREBASE_API_KEY`, `STRIPE_SECRET_KEY`, `STRIPE_WEBHOOK_SECRET`) are stored as Cloud Run environment variable secrets. Runtime Runway supplies them as runtime config; Commerce Rails owns the Stripe adapter semantics. Set via Console or:

```bash
gcloud run services update api-server \
  --region=europe-west1 \
  --project=wolfgang-kb-prod \
  --update-secrets=STRIPE_SECRET_KEY=STRIPE_SECRET_KEY:latest
```

### Stripe webhook secret

`STRIPE_WEBHOOK_SECRET` — the signing secret for the Commerce Rails Stripe adapter behind the Runtime Runway webhook route. Obtained from the Stripe Dashboard → Webhooks → endpoint → "Signing secret".

If you roll the Stripe webhook secret:
1. Copy the new signing secret from Stripe Dashboard
2. Update the Cloud Run secret version (command above, substituting `STRIPE_WEBHOOK_SECRET`)
3. Cloud Run picks it up on the next revision deploy (`just api-deploy`) — no redeploy needed if you update the secret version in place

---

## apps.reflective.se Cloud Functions (converge-369ad) — interim

These exist until the partner-application flow moves into runway-accounts.

### APPLICATION_NOTIFY_WEBHOOK_URL

Make.com webhook URL for partner application notifications. Fires when a submission hits ≥ 74% readiness. Stored in GCP Secret Manager, project `converge-369ad`. The Cloud Function resolves it at runtime via `defineSecret`; no function redeploy needed when rotating.

**Rotate:**
```bash
echo -n "https://hook.eu1.make.com/NEW_WEBHOOK_ID" | \
  gcloud secrets versions add APPLICATION_NOTIFY_WEBHOOK_URL \
    --project=converge-369ad \
    --data-file=-
```

**Inspect versions:**
```bash
gcloud secrets versions list APPLICATION_NOTIFY_WEBHOOK_URL --project=converge-369ad
```

**Local dev:** create `apps.reflective.se/functions/.env` (gitignored):
```
APPLICATION_NOTIFY_WEBHOOK_URL=https://hook.eu1.make.com/your-dev-webhook
```

---

## Migration intent

When runway-accounts gains the partner application endpoint:
1. Move `submitPartnerApplication` logic into a new `runway-accounts` route
2. Store `APPLICATION_NOTIFY_WEBHOOK_URL` as a Cloud Run secret in `wolfgang-kb-prod`
3. Remove the Firebase Function from `converge-369ad` — `firebase.json` rewrite for `/apply-api/**` points to the Cloud Run service instead
4. The `converge-369ad` project becomes hosting-only (static files), no Function secrets remaining
