//! Rust port of `kyth_shared` (`src/kyth_shared`) — the Python library
//! ~200 kyth-welcome pages, ujust recipes, and systemd units share for
//! host tuning. Porting all of it in one pass isn't realistic or safe:
//! much of it touches privileged/hardware operations (GPU switching, VPN,
//! installer disk ops) — exactly the kind of thing CLAUDE.md already
//! flags as high-risk. See `MIGRATION.md` (repo root of this crate) for
//! the actual scope and how more of `kyth_shared` moves over.
//!
//! This slice ports the Hub-facing reads and selected, explicit user actions
//! that used to cross the Tauri/Python boundary — now called directly from
//! the crate, without a subprocess/JSON bridge. Most modules are read-only;
//! a small number of bounded action paths (notably Guardian repairs) are
//! intentionally user-triggered and policy-gated. Nothing here replaces the
//! live Python probe sweep or the installer/high-risk writer paths.

pub mod atomic_io;
pub mod build_checks;
pub mod build_metadata;
pub mod build_metrics;
pub mod cloud_idempotent;
pub mod commands;
pub mod config_loader;
pub mod containers;
pub mod desktop_polish;
pub mod diagnostic_report;
pub mod diagnostics_scrub;
pub mod doctor;
pub mod guardian;
pub mod health;
pub mod network_share;
pub mod privileged;
pub mod release_identity;
pub mod release_publish;
pub mod repos;
pub mod sarif;
pub mod secret_scan;
pub mod setup_transfer;
pub mod system;
pub mod transfer;
pub mod url_encode;
pub mod work_migration;
