// Re-export berth-proto for backward compatibility.
// Crates that previously imported from berth_core:: continue to work.

// Shared protocol types — re-exported from berth-proto
pub use berth_proto::nats_relay;
pub use berth_proto::env;
pub mod executor;
pub mod agent_transport {
    pub use berth_proto::transport::*;
}

// Runtime types from berth-proto + detection logic
pub mod runtime;

// Agent client (gRPC client + types) — wraps proto and transport types
pub mod agent_client;
pub mod agent_service;
pub mod archive;
pub mod container;
pub mod containerfile;
#[cfg(target_os = "macos")]
pub mod credentials;
pub mod discovery;
pub mod local_agent;
pub mod path_safety;
pub mod project;
pub mod scheduler;
pub mod setup;
pub mod store;
pub mod target;
pub mod template_store;
pub mod tls;
pub mod tunnel;
pub mod uds;
#[cfg(feature = "nats")]
pub mod nats_subscriber;
#[cfg(feature = "nats")]
pub mod nats_cmd_client;

pub use project::{Project, ProjectStatus, RunMode};
pub use runtime::{Runtime, RuntimeInfo};
pub use scheduler::Schedule;
pub use target::{Target, TargetKind, TargetStatus};
