project_id      = "reflective-labs-platform-prod"
region          = "europe-west1"
env             = "prod"
apps            = ["folio", "inkling", "wolfgang", "scout", "quorum", "vouch"]
spanner_config  = "regional-europe-west1"
redis_tier      = "BASIC"
redis_memory_gb = 1

releases_domain   = "releases.reflective.se"
releases_location = "EU"
