//! Freejazz, the app: everything but `main` (so integration tests reach the device
//! manager).

pub mod app;
pub mod autostart;
pub mod backups;
pub mod designs;
pub mod device;
pub mod keyboard;
pub mod live;
pub mod mem;
pub mod mock;
pub mod paths;
pub mod profiles;
pub mod sections;
pub mod settings;
pub mod single_instance;
pub mod state;
pub mod tray;
pub mod ui;
pub mod updater;
