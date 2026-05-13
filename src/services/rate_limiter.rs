use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

struct TokenBucket {
    capacity: u64,
    tokens: f64,
    refill_rate: f64,
    last_refill: Instant,
}

impl TokenBucket {
    fn new(capacity: u64, refill_per_sec: f64) -> Self {
        Self { capacity, tokens: capacity as f64, refill_rate: refill_per_sec, last_refill: Instant::now() }
    }
    fn try_consume(&mut self, count: u64) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.tokens = (self.tokens + elapsed * self.refill_rate).min(self.capacity as f64);
        self.last_refill = now;
        let cost = count as f64;
        if self.tokens >= cost { self.tokens -= cost; true } else { false }
    }
}

pub struct RateLimiter {
    buckets: Mutex<HashMap<String, TokenBucket>>,
    default_capacity: u64,
    default_refill_rate: f64,
}

impl RateLimiter {
    pub fn new(capacity: u64, refill_per_sec: f64) -> Self {
        Self { buckets: Mutex::new(HashMap::new()), default_capacity: capacity, default_refill_rate: refill_per_sec }
    }
    pub fn check(&self, key: &str, cost: u64) -> bool {
        let mut b = self.buckets.lock().unwrap();
        let bucket = b.entry(key.to_string()).or_insert_with(|| TokenBucket::new(self.default_capacity, self.default_refill_rate));
        bucket.try_consume(cost)
    }
    pub fn remaining(&self, key: &str) -> u64 {
        let mut b = self.buckets.lock().unwrap();
        let bucket = b.entry(key.to_string()).or_insert_with(|| TokenBucket::new(self.default_capacity, self.default_refill_rate));
        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        (bucket.tokens + elapsed * bucket.refill_rate).min(bucket.capacity as f64) as u64
    }
    pub fn reset(&self, key: &str) { self.buckets.lock().unwrap().remove(key); }
}

