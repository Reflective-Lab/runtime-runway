mod config;
pub use config::HostConfig;

use std::sync::Arc;

use anyhow::Result;
use axum::routing::get;
use axum::{Json, Router};
use runway_auth::{AuthLayer, FirebaseAuth};
use runway_middleware::{MiddlewareConfig, serve, stack};
use runway_storage::{StorageKit, remote::RemoteConfig};
use runway_telemetry::{TelemetryConfig, TelemetryGuard, init as init_telemetry};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppExecutionPacket {
    pub app_id: String,
    pub display_name: String,
    pub description: String,
    pub version: String,
    pub route_prefix: String,
    #[serde(default)]
    pub auth_app: Option<String>,
    #[serde(default)]
    pub jobs: Vec<JobRegistration>,
    #[serde(default)]
    pub operator_packets: Vec<OperatorPacketRegistration>,
    #[serde(default)]
    pub subject_refs: Vec<SubjectRefRegistration>,
    #[serde(default)]
    pub fixtures: Vec<FixtureRegistration>,
    #[serde(default)]
    pub domain_routes: Vec<RouteRegistration>,
    #[serde(default)]
    pub mounted_modules: Vec<MountedModule>,
    #[serde(default)]
    pub boundaries: Vec<BoundaryRegistration>,
}

impl AppExecutionPacket {
    pub fn new(
        app_id: impl Into<String>,
        display_name: impl Into<String>,
        description: impl Into<String>,
        route_prefix: impl Into<String>,
    ) -> Self {
        Self {
            app_id: app_id.into(),
            display_name: display_name.into(),
            description: description.into(),
            version: String::new(),
            route_prefix: route_prefix.into(),
            auth_app: None,
            jobs: Vec::new(),
            operator_packets: Vec::new(),
            subject_refs: Vec::new(),
            fixtures: Vec::new(),
            domain_routes: Vec::new(),
            mounted_modules: Vec::new(),
            boundaries: Vec::new(),
        }
    }

    pub fn with_auth_app(mut self, app: impl Into<String>) -> Self {
        self.auth_app = Some(app.into());
        self
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    pub fn with_job(mut self, job: JobRegistration) -> Self {
        self.jobs.push(job);
        self
    }

    pub fn with_operator_packet(mut self, packet: OperatorPacketRegistration) -> Self {
        self.operator_packets.push(packet);
        self
    }

    pub fn with_subject_ref(mut self, subject_ref: SubjectRefRegistration) -> Self {
        self.subject_refs.push(subject_ref);
        self
    }

    pub fn with_fixture(mut self, fixture: FixtureRegistration) -> Self {
        self.fixtures.push(fixture);
        self
    }

    pub fn with_domain_route(mut self, route: RouteRegistration) -> Self {
        self.domain_routes.push(route);
        self
    }

    pub fn with_mounted_module(mut self, module: MountedModule) -> Self {
        self.mounted_modules.push(module);
        self
    }

    pub fn with_boundary(mut self, boundary: BoundaryRegistration) -> Self {
        self.boundaries.push(boundary);
        self
    }

    pub fn from_json_str(input: &str) -> serde_json::Result<Self> {
        serde_json::from_str(input)
    }

    fn required_auth_app(&self) -> &str {
        self.auth_app.as_deref().unwrap_or(&self.app_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobRegistration {
    pub key: String,
    pub display_name: String,
    pub source: RegistrationSource,
}

impl JobRegistration {
    pub fn new(
        key: impl Into<String>,
        display_name: impl Into<String>,
        source: RegistrationSource,
    ) -> Self {
        Self {
            key: key.into(),
            display_name: display_name.into(),
            source,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RegistrationSource {
    AxiomTruth,
    HelmTruthCatalog,
    AppDomain,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperatorPacketRegistration {
    pub packet_key: String,
    pub receipt_family: String,
    pub authority_effect: AuthorityEffect,
}

impl OperatorPacketRegistration {
    pub fn no_authority(packet_key: impl Into<String>, receipt_family: impl Into<String>) -> Self {
        Self {
            packet_key: packet_key.into(),
            receipt_family: receipt_family.into(),
            authority_effect: AuthorityEffect::None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AuthorityEffect {
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SubjectRefRegistration {
    pub kind: String,
    pub codec: String,
}

impl SubjectRefRegistration {
    pub fn new(kind: impl Into<String>, codec: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            codec: codec.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FixtureRegistration {
    pub key: String,
    pub description: String,
}

impl FixtureRegistration {
    pub fn new(key: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            description: description.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteRegistration {
    pub method: String,
    pub path: String,
    pub owner: RouteOwner,
}

impl RouteRegistration {
    pub fn new(method: impl Into<String>, path: impl Into<String>, owner: RouteOwner) -> Self {
        Self {
            method: method.into(),
            path: path.into(),
            owner,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RouteOwner {
    RunwayHost,
    HelmModule,
    AppDomain,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MountedModule {
    pub module_id: String,
    pub mount_kind: MountKind,
    pub routes: Vec<RouteRegistration>,
}

impl MountedModule {
    pub fn new(module_id: impl Into<String>, mount_kind: MountKind) -> Self {
        Self {
            module_id: module_id.into(),
            mount_kind,
            routes: Vec::new(),
        }
    }

    pub fn with_route(mut self, route: RouteRegistration) -> Self {
        self.routes.push(route);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MountKind {
    Mounted,
    Planned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoundaryRegistration {
    pub layer: ContractLayer,
    pub owns: Vec<String>,
    #[serde(default)]
    pub consumes: Vec<String>,
    pub status: BoundaryStatus,
}

impl BoundaryRegistration {
    pub fn new(layer: ContractLayer, owns: Vec<String>, status: BoundaryStatus) -> Self {
        Self {
            layer,
            owns,
            consumes: Vec::new(),
            status,
        }
    }

    pub fn consuming(mut self, consumes: Vec<String>) -> Self {
        self.consumes = consumes;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ContractLayer {
    Axiom,
    Helm,
    Runway,
    App,
    Movement,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BoundaryStatus {
    Mounted,
    Planned,
    AppOwned,
    OutOfScope,
}

#[derive(Clone)]
pub struct AppHostState {
    pub packet: Arc<AppExecutionPacket>,
    pub storage: Arc<StorageKit>,
}

pub struct RunwayAppHost {
    packet: Arc<AppExecutionPacket>,
    storage: Arc<StorageKit>,
    config: HostConfig,
    _telemetry: TelemetryGuard,
}

impl RunwayAppHost {
    pub async fn from_env(packet: AppExecutionPacket) -> Result<Self> {
        let config = HostConfig::from_env(&packet);
        Self::with_config(packet, config).await
    }

    pub async fn with_config(packet: AppExecutionPacket, config: HostConfig) -> Result<Self> {
        let _telemetry = init_telemetry(TelemetryConfig::from_env(&packet.app_id))?;

        let storage = if config.local_dev {
            StorageKit::local(&config.storage_path).await?
        } else {
            StorageKit::remote(RemoteConfig::from_env()?).await?
        };

        tracing::info!(
            app_id = %packet.app_id,
            route_prefix = ?config.route_prefix,
            local_dev = config.local_dev,
            "runway app host initialized"
        );

        Ok(Self {
            packet: Arc::new(packet),
            storage: Arc::new(storage),
            config,
            _telemetry,
        })
    }

    pub fn packet(&self) -> Arc<AppExecutionPacket> {
        self.packet.clone()
    }

    pub fn storage(&self) -> Arc<StorageKit> {
        self.storage.clone()
    }

    pub fn state(&self) -> AppHostState {
        AppHostState {
            packet: self.packet(),
            storage: self.storage(),
        }
    }

    pub fn router(&self, public_routes: Router, protected_routes: Router) -> Router {
        let auth_layer = AuthLayer::new(
            FirebaseAuth::new(self.config.firebase_project_id.clone()),
            self.config.local_dev,
        )
        .requiring_app(self.packet.required_auth_app());
        let protected = protected_routes.layer(auth_layer);
        let public = self.status_routes().merge(public_routes);
        let routed = match self.config.route_prefix.as_deref() {
            Some(prefix) => Router::new().nest(prefix, public.merge(protected)),
            None => public.merge(protected),
        };

        let mw_config = MiddlewareConfig {
            allowed_origins: self.config.allowed_origins.clone(),
        };
        stack(routed, &mw_config)
    }

    pub async fn serve(self, public_routes: Router, protected_routes: Router) -> Result<()> {
        let app = self.router(public_routes, protected_routes);
        serve(app).await;
        Ok(())
    }

    fn status_routes(&self) -> Router {
        let packet = self.packet.clone();
        Router::new().route(
            "/status",
            get(move || {
                let packet = packet.clone();
                async move { Json(status_payload(&packet)) }
            }),
        )
    }
}

fn status_payload(packet: &AppExecutionPacket) -> Value {
    json!({
        "status": "ok",
        "service": packet.app_id,
        "name": packet.display_name,
        "description": packet.description,
        "version": if packet.version.is_empty() {
            env!("CARGO_PKG_VERSION")
        } else {
            &packet.version
        },
        "packet": packet,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_packet_preserves_registered_jobs_and_modules() {
        let packet = AppExecutionPacket::new("catalyst", "Catalyst", "Ops workflows", "/catalyst")
            .with_job(JobRegistration::new(
                "schedule-strategic-meetings",
                "Schedule strategic meetings",
                RegistrationSource::HelmTruthCatalog,
            ))
            .with_operator_packet(OperatorPacketRegistration::no_authority(
                "job-readiness",
                "long-running-job",
            ))
            .with_mounted_module(
                MountedModule::new("helm.jobs", MountKind::Planned).with_route(
                    RouteRegistration::new("POST", "/v1/jobs/{key}/stream", RouteOwner::HelmModule),
                ),
            )
            .with_boundary(BoundaryRegistration::new(
                ContractLayer::Runway,
                vec!["host".to_string()],
                BoundaryStatus::Mounted,
            ));

        assert_eq!(packet.app_id, "catalyst");
        assert_eq!(packet.jobs.len(), 1);
        assert_eq!(
            packet.operator_packets[0].authority_effect,
            AuthorityEffect::None
        );
        assert_eq!(
            packet.mounted_modules[0].routes[0].owner,
            RouteOwner::HelmModule
        );
        assert_eq!(packet.boundaries[0].layer, ContractLayer::Runway);
    }

    #[test]
    fn route_registration_distinguishes_host_module_and_domain_owners() {
        let routes = [
            RouteRegistration::new("GET", "/status", RouteOwner::RunwayHost),
            RouteRegistration::new("POST", "/v1/jobs/{key}/stream", RouteOwner::HelmModule),
            RouteRegistration::new("GET", "/api/jobs", RouteOwner::AppDomain),
        ];

        assert!(matches!(routes[0].owner, RouteOwner::RunwayHost));
        assert!(matches!(routes[1].owner, RouteOwner::HelmModule));
        assert!(matches!(routes[2].owner, RouteOwner::AppDomain));
    }

    #[test]
    fn app_packet_parses_manifest_shape() {
        let packet = AppExecutionPacket::from_json_str(
            r#"{
                "app_id": "tally",
                "display_name": "Tally Escrow",
                "description": "Escrow release readiness",
                "version": "0.1.0",
                "route_prefix": "/tally",
                "auth_app": "tally",
                "jobs": [
                    {
                        "key": "escrow-release-readiness",
                        "display_name": "Escrow release readiness",
                        "source": "axiom-truth"
                    }
                ],
                "operator_packets": [
                    {
                        "packet_key": "escrow-release-readiness",
                        "receipt_family": "long-running-job",
                        "authority_effect": "none"
                    }
                ],
                "subject_refs": [
                    { "kind": "agreement", "codec": "tally://agreements/{agreement_id}" }
                ],
                "fixtures": [],
                "domain_routes": [],
                "mounted_modules": [
                    {
                        "module_id": "helm.operator-control",
                        "mount_kind": "planned",
                        "routes": [
                            {
                                "method": "GET",
                                "path": "/v1/workbench/operator-control/previews",
                                "owner": "helm-module"
                            }
                        ]
                    }
                ],
                "boundaries": [
                    {
                        "layer": "runway",
                        "owns": ["host"],
                        "consumes": [],
                        "status": "planned"
                    }
                ]
            }"#,
        )
        .expect("packet parses");

        assert_eq!(packet.app_id, "tally");
        assert_eq!(packet.jobs[0].source, RegistrationSource::AxiomTruth);
        assert_eq!(packet.mounted_modules[0].mount_kind, MountKind::Planned);
        assert_eq!(packet.boundaries[0].status, BoundaryStatus::Planned);
    }
}
