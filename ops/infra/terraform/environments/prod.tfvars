project_id      = "reflective-prod"
region          = "europe-west1"
env             = "prod"
apps            = ["folio", "inkling", "wolfgang", "scout", "quorum", "vouch"]
spanner_config  = "nam-eur-asia1"
redis_tier      = "STANDARD_HA"
redis_memory_gb = 4

releases_domain   = "releases.reflective.app"
releases_location = "EU"
