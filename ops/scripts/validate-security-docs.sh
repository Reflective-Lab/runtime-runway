#!/usr/bin/env bash
set -euo pipefail

required_files=(
  "SECURITY.md"
  "kb/Architecture/Audits/2026-04-11 Security Review.md"
  "kb/Architecture/Security Review Plan.md"
)

for file in "${required_files[@]}"; do
  [[ -f "$file" ]] || {
    echo "Missing required security/compliance document: $file" >&2
    exit 1
  }
done

for needle in \
  "## Current Security Baseline" \
  "## Security Regression Gate" \
  "## Shared Responsibility" \
  "## Compliance Declarations"
do
  rg -q "$needle" SECURITY.md || {
    echo "Missing required declaration text: $needle" >&2
    exit 1
  }
done

claim_pattern='SOC 2 certified|ISO 27001 certified|HIPAA compliant|PCI compliant|GDPR compliant'
if rg -n "$claim_pattern" README.md SECURITY.md CONTRIBUTING.md AGENTS.md CODEX.md CLAUDE.md GEMINI.md scripts .github kb/Building kb/Workflow \
  --glob '!scripts/validate-security-docs.sh'
then
  echo "Found unsupported compliance claim outside the approved disclaimer docs." >&2
  exit 1
fi

echo "Security/compliance docs validation passed."
