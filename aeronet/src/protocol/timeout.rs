use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct TimeoutConfig {
    pub timeout: Option<Duration>,
    pub keep_alive_interval: Option<Duration>,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            timeout: Some(Duration::from_secs(30)),
            keep_alive_interval: Some(Duration::from_secs(10)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Timeout {
    timeout: Option<Duration>,
    keep_alive_interval: Option<Duration>,
    last_msg_at: Instant,
}

impl Timeout {
    pub fn new(config: TimeoutConfig) -> Self {
        Self {
            timeout: config.timeout,
            keep_alive_interval: config.keep_alive_interval,
            last_msg_at: Instant::now(),
        }
    }

    pub fn update(&mut self) {
        self.last_msg_at = Instant::now();
    }

    pub fn timed_out(&self) -> bool {
        match self.timeout {
            None => false,
            Some(timeout) => Instant::now().duration_since(self.last_msg_at) > timeout,
        }
    }

    pub fn send_keep_alive(&self) -> bool {
        match self.keep_alive_interval {
            None => false,
            Some(interval) => Instant::now().duration_since(self.last_msg_at) > interval,
        }
    }
}
