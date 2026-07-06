//! proxy-core: core library for the proxy pool system.
//!
//! Contains data models, configuration, fetchers, validator, store,
//! scheduler, GeoIP lookup, domain router, and WARP integration.

pub mod circuit;
pub mod config;
pub mod dedup;
pub mod ewma;
pub mod fetcher;
pub mod geoip;
pub mod models;
pub mod pacing;
pub mod route_debug;
pub mod router;
pub mod scheduler;
pub mod status;
pub mod store;
pub mod validator;
pub mod warp;
pub mod xray_status;
