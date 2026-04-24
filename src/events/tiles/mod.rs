//! Tiles projection — derives a minimal set of "agent instance" tiles
//! from the last N hours of events on demand. See spec
//! 2026-04-23-tiles-projection-design.md.

pub mod tile;
pub mod surfaces;
// Submodules added in later tasks: derive, cluster.
