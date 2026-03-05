//! Runway MCP Server — Phase 2 implementation.
//!
//! Will expose tools: runway_list_projects, runway_deploy, runway_logs, etc.
//! Transports: stdio (primary for Claude Code) + HTTP via axum.

pub mod tools {
    pub const TOOL_LIST: &[&str] = &[
        "runway_list_projects",
        "runway_project_status",
        "runway_deploy",
        "runway_stop",
        "runway_restart",
        "runway_logs",
        "runway_list_targets",
        "runway_add_target",
        "runway_list_agents",
        "runway_import_code",
        "runway_detect_runtime",
        "runway_health",
    ];
}
