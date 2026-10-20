//! Launcher-owned pieces, separate from platform presentation.
pub mod agent_setup;
pub mod app_config;
pub mod fx;
pub mod identity;
mod model_hardware;
pub mod model_selection;
pub mod panel_ipc;
pub mod settings;
pub mod share_rules;
pub mod status;

pub mod reset;

pub mod runtime_config;
pub mod startup_log;

pub mod payments;

pub mod private_reset;

pub mod lifecycle;
pub mod payment_reset;
pub mod wallet_setup;
