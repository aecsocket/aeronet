use std::time::Duration;

#[derive(Debug)]
pub struct ByteBucket {
    max: usize,
    now: usize,
}

impl ByteBucket {
    pub fn new(max: usize) -> Self {
        Self { max, now: max }
    }

    pub fn get(&self) -> usize {
        self.now
    }

    pub fn consume(&mut self, amount: usize) -> bool {
        if self.now >= amount {
            self.now -= amount;
            true
        } else {
            false
        }
    }

    pub fn update(&mut self, delta_time: Duration) {
        let added = ((self.max as f64) * delta_time.as_secs_f64()) as usize;
        self.now = (self.now + added).min(self.max);
    }
}
