//! Dotlink core: everything except the window. Runs inside the desktop app, or headless as
//! `dotlink daemon`.

pub mod cast;
pub mod config;
pub mod events;
pub mod input;
pub mod ipc;
pub mod platform;
pub mod server;
pub mod service;
pub mod term;
pub mod tls;
pub mod update;
pub mod usb;
