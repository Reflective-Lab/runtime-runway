#!/usr/bin/env bash
# Register or update an app entry in the Firebase Hosting apps.json registry.
#
# Usage:
#   register-app.sh \
#     --key catalyst \
#     --name "Catalyst" \
#     --description "Business ops workflows with human approval gates" \
#     --path /catalyst \
#     --status-path /catalyst/status \
#     --version 0.1.0 \
#     --sha abc1234
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REGISTRY="$ROOT_DIR/ops/infra/firebase/apps/public/apps.json"

KEY="" NAME="" DESCRIPTION="" PATH_VAL="" STATUS_PATH="" VERSION="" SHA=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --key)           KEY="$2";         shift 2 ;;
    --name)          NAME="$2";        shift 2 ;;
    --description)   DESCRIPTION="$2"; shift 2 ;;
    --path)          PATH_VAL="$2";    shift 2 ;;
    --status-path)   STATUS_PATH="$2"; shift 2 ;;
    --version)       VERSION="$2";     shift 2 ;;
    --sha)           SHA="$2";         shift 2 ;;
    *) echo "Unknown arg: $1" >&2; exit 1 ;;
  esac
done

[[ -n "$KEY" && -n "$NAME" && -n "$PATH_VAL" && -n "$VERSION" && -n "$SHA" ]] || {
  echo "Usage: register-app.sh --key KEY --name NAME --path PATH --version VER --sha SHA [--description DESC] [--status-path PATH]" >&2
  exit 1
}

python3 - <<PYEOF
import json, datetime

registry = "$REGISTRY"
with open(registry) as f:
    reg = json.load(f)

entry = {
    "key":         "$KEY",
    "name":        "$NAME",
    "description": "$DESCRIPTION",
    "path":        "$PATH_VAL",
    "status_path": "$STATUS_PATH" or None,
    "version":     "$VERSION",
    "sha":         "$SHA",
    "deployed_at": datetime.datetime.utcnow().strftime("%Y-%m-%dT%H:%M:%SZ"),
}

apps = [a for a in reg.get("apps", []) if a["key"] != "$KEY"]
apps.append(entry)
apps.sort(key=lambda x: x["name"])
reg["apps"] = apps

with open(registry, "w") as f:
    json.dump(reg, f, indent=2)
    f.write("\n")

print(f"Registered {entry['name']} v{entry['version']} ({entry['sha']}) → {entry['path']}")
PYEOF
