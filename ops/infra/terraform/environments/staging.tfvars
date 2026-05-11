project_id      = "reflective-staging"
region          = "europe-west1"
env             = "staging"
apps            = ["folio", "inkling", "wolfgang", "scout", "quorum", "vouch"]
spanner_config  = "regional-europe-west1"
redis_tier      = "BASIC"
redis_memory_gb = 1

releases_domain   = "releases-staging.reflective.app"
releases_location = "EU"
