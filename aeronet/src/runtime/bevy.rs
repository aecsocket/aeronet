use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, OnceLock},
    task::Poll,
    time::Duration,
};

use bevy_tasks::{futures_lite::FutureExt, AsyncComputeTaskPool};

use crate::AsyncTimer;

use super::Runtime;

#[derive(Debug, Clone)]
pub struct BevyRuntime;

static RT: OnceLock<Arc<BevyRuntime>> = OnceLock::new();

impl BevyRuntime {
    pub fn arc() -> Arc<BevyRuntime> {
        RT.get_or_init(|| Arc::new(BevyRuntime)).clone()
    }
}

impl Runtime for BevyRuntime {
    fn timer(&self, duration: Duration) -> Pin<Box<dyn AsyncTimer>> {
        Box::pin(BevyTimer(async_io::Timer::after(duration)))
    }

    fn spawn(&self, future: Pin<Box<dyn Future<Output = ()> + Send>>) {
        AsyncComputeTaskPool::get().spawn(future).detach();
    }
}

#[derive(Debug)]
pub struct BevyTimer(async_io::Timer);

impl Future for BevyTimer {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        self.0.poll(cx).map(|_| ())
    }
}

impl AsyncTimer for BevyTimer {}
