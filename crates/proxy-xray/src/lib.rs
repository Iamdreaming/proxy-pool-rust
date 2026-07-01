//! proxy-xray: xray-core integration for encrypted proxy protocols.
//!
//! Manages xray-core as a subprocess, communicates via gRPC HandlerService
//! for hot-reloading outbounds, allocates local SOCKS5 ports, and syncs
//! pending encrypted nodes from Redis into active xray outbounds.

pub mod config_gen;
pub mod models;
pub mod outbound_sync;
pub mod port_manager;
pub mod process;
pub mod proto;
pub mod xray_client;
