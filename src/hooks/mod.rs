//! Universal agent hook processing for `zestful hook`.
//!
//! Individual submodules:
//! - [`detect`]: figures out which agent invoked us
//! - [`policy`]: event-name → notification policy tables

pub mod detect;
pub mod policy;

pub use detect::{detect_agent, AgentKind};
pub use policy::{resolve as resolve_policy, Policy, Severity};
