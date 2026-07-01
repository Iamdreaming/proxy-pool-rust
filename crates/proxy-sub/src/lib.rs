//! proxy-sub: subscription source discovery and format parsing.
//!
//! Parses Clash YAML, V2Ray base64, V2Ray JSON, and Surge subscription
//! formats into `SubscriptionProxy` nodes. Discoveres subscription URLs
//! from static config, GitHub search, and aggregator projects.

pub mod convert;
pub mod discover;
pub mod models;
pub mod parser;
pub mod pending;
pub mod refresh;
pub mod source;
