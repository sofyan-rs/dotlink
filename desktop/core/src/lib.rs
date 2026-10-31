//! Dotlink core: everything except the window. Runs inside the desktop app, or headless as
//! `dotlink daemon`.

pub mod cast;
pub mod codes;
pub mod config;
pub mod events;
pub mod files;
pub mod input;
pub mod pair_code;
pub mod ipc;
pub mod media;
pub mod peer;
pub mod platform;
pub mod server;
pub mod service;
pub mod term;
pub mod tls;
pub mod update;
pub mod usb;
pub mod weblink;
