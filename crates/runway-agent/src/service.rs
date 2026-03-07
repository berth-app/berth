// Legacy re-export kept for compatibility.
// The remote agent now uses PersistentAgentService from persistent_service.rs.
// The local embedded agent still uses AgentServiceImpl from runway-core.
#[allow(unused_imports)]
pub use runway_core::agent_service::AgentServiceImpl;
