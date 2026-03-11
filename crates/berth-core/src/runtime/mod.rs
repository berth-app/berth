// Re-export all types from berth-proto::runtime
pub use berth_proto::runtime::*;

// Runtime detection logic (app-side)
mod detect;
pub use detect::detect_runtime;
