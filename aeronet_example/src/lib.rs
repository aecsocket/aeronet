//! Support code for [`aeronet`] implementation examples.

mod client;
mod complex;
mod echo;
mod ui;

pub use {client::*, complex::*, echo::*, ui::*};

/// Default filter used for the Bevy logging plugin.
pub const LOG_FILTER: &str = "wgpu=error,naga=warn,log=warn,aeronet_wt_native=debug";
