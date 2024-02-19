#[cfg(feature = "bevy-async")]
mod bevy;

#[cfg(feature = "bevy-async")]
pub use bevy::*;

use std::{fmt::Debug, pin::Pin, time::Duration};

use std::future::Future;

pub trait Runtime: Send + Sync + Debug + 'static {
    fn timer(&self, duration: Duration) -> Pin<Box<dyn AsyncTimer>>;

    fn spawn(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>);
}

pub trait AsyncTimer: Send + Debug + Future<Output = ()> + 'static {}
