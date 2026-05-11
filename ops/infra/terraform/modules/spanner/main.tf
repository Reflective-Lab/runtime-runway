resource "google_spanner_instance" "main" {
  project      = var.project_id
  name         = "reflective-${var.env}"
  config       = var.spanner_config
  display_name = "Reflective ${title(var.env)}"
  num_nodes    = var.env == "prod" ? 3 : 1

  labels = { env = var.env }
}

resource "google_spanner_database" "governance" {
  project  = var.project_id
  instance = google_spanner_instance.main.name
  name     = "governance"

  deletion_protection = var.env == "prod"

  # ACID tables: orgs, identities, fact promotions, loan decisions.
  # All tables are interleaved under orgs for co-location (same node = faster joins).
  ddl = [
    <<-SQL
    CREATE TABLE orgs (
      org_id    STRING(36)  NOT NULL,
      plan      STRING(32),
      stripe_id STRING(256),
      created_at TIMESTAMP NOT NULL OPTIONS (allow_commit_timestamp=true),
    ) PRIMARY KEY (org_id)
    SQL
    ,
    <<-SQL
    CREATE TABLE identities (
      org_id       STRING(36)  NOT NULL,
      user_id      STRING(128) NOT NULL,
      email        STRING(256),
      cedar_claims JSON,
      created_at   TIMESTAMP NOT NULL OPTIONS (allow_commit_timestamp=true),
    ) PRIMARY KEY (org_id, user_id),
    INTERLEAVE IN PARENT orgs ON DELETE CASCADE
    SQL
    ,
    <<-SQL
    CREATE TABLE fact_promotions (
      org_id        STRING(36) NOT NULL,
      fact_id       STRING(36) NOT NULL,
      context_id    STRING(36) NOT NULL,
      gate_decision STRING(32) NOT NULL,
      authority     JSON,
      committed_at  TIMESTAMP NOT NULL OPTIONS (allow_commit_timestamp=true),
    ) PRIMARY KEY (org_id, fact_id),
    INTERLEAVE IN PARENT orgs ON DELETE CASCADE
    SQL
    ,
    <<-SQL
    CREATE TABLE loan_decisions (
      org_id           STRING(36) NOT NULL,
      application_id   STRING(36) NOT NULL,
      decision         STRING(32) NOT NULL,
      rationale        JSON,
      compliance_voice STRING(MAX),
      decided_at       TIMESTAMP NOT NULL OPTIONS (allow_commit_timestamp=true),
    ) PRIMARY KEY (org_id, application_id),
    INTERLEAVE IN PARENT orgs ON DELETE CASCADE
    SQL
    ,
    <<-SQL
    CREATE TABLE billing_subscriptions (
      org_id           STRING(36)  NOT NULL,
      subscription_id  STRING(256) NOT NULL,
      stripe_sub_id    STRING(256),
      status           STRING(32)  NOT NULL,
      current_period_end TIMESTAMP,
      updated_at       TIMESTAMP NOT NULL OPTIONS (allow_commit_timestamp=true),
    ) PRIMARY KEY (org_id, subscription_id),
    INTERLEAVE IN PARENT orgs ON DELETE CASCADE
    SQL
  ]
}
