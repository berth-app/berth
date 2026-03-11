//! berth-proto: Shared protocol types for Berth agent-app communication.
//!
//! This crate contains the protobuf-generated types, transport trait,
//! NATS relay types, and shared data models used by both the Berth
//! desktop app and the remote agent.

pub mod proto {
    tonic::include_proto!("berth");
}

pub mod nats_relay;
pub mod transport;
pub mod runtime;
pub mod executor;
pub mod env;
pub mod message_auth;
pub mod schedule;
