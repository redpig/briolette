// Copyright 2023 The Briolette Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! IP-based token bucket rate limiter for gRPC endpoints.
//!
//! Use for unauthenticated endpoints (GetEpoch, ValidateTokens, RegisterCall
//! with Algorithm::NONE). Authenticated endpoints should use NAC-based
//! tracking via the existing GroupPolicy / bloom filter mechanisms.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Configuration for the rate limiter.
#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    /// Maximum requests allowed in the burst window.
    pub burst: u32,
    /// Refill rate: tokens added per second.
    pub per_second: f64,
    /// Maximum number of tracked IPs before evicting the oldest.
    pub max_entries: usize,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            burst: 20,
            per_second: 5.0,
            max_entries: 10_000,
        }
    }
}

struct Bucket {
    tokens: f64,
    last_refill: Instant,
}

impl Bucket {
    fn new(burst: u32) -> Self {
        Self {
            tokens: burst as f64,
            last_refill: Instant::now(),
        }
    }

    fn try_consume(&mut self, per_second: f64, burst: u32) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;

        // Refill tokens
        self.tokens = (self.tokens + elapsed * per_second).min(burst as f64);

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Thread-safe IP-based rate limiter using token bucket algorithm.
#[derive(Clone)]
pub struct RateLimiter {
    config: RateLimitConfig,
    buckets: Arc<Mutex<HashMap<IpAddr, Bucket>>>,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            buckets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if a request from the given IP is allowed.
    /// Returns true if allowed, false if rate limited.
    pub fn check(&self, ip: IpAddr) -> bool {
        let mut buckets = self.buckets.lock().unwrap();

        // Evict oldest entries if at capacity
        if buckets.len() >= self.config.max_entries && !buckets.contains_key(&ip) {
            // Simple eviction: remove the entry with the oldest last_refill
            if let Some(oldest_ip) = buckets
                .iter()
                .min_by_key(|(_, b)| b.last_refill)
                .map(|(ip, _)| *ip)
            {
                buckets.remove(&oldest_ip);
            }
        }

        let bucket = buckets
            .entry(ip)
            .or_insert_with(|| Bucket::new(self.config.burst));
        bucket.try_consume(self.config.per_second, self.config.burst)
    }
}

/// Extract the client IP from a tonic request's remote address.
pub fn extract_ip<T>(request: &tonic::Request<T>) -> Option<IpAddr> {
    request.remote_addr().map(|addr| addr.ip())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_allows_within_burst() {
        let limiter = RateLimiter::new(RateLimitConfig {
            burst: 5,
            per_second: 1.0,
            max_entries: 100,
        });
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        for _ in 0..5 {
            assert!(limiter.check(ip));
        }
        // 6th should be rejected
        assert!(!limiter.check(ip));
    }

    #[test]
    fn test_different_ips_independent() {
        let limiter = RateLimiter::new(RateLimitConfig {
            burst: 2,
            per_second: 0.0, // no refill
            max_entries: 100,
        });
        let ip1 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));

        assert!(limiter.check(ip1));
        assert!(limiter.check(ip1));
        assert!(!limiter.check(ip1)); // exhausted

        // ip2 should still have tokens
        assert!(limiter.check(ip2));
        assert!(limiter.check(ip2));
        assert!(!limiter.check(ip2));
    }

    #[test]
    fn test_refill() {
        let limiter = RateLimiter::new(RateLimitConfig {
            burst: 1,
            per_second: 1000.0, // fast refill for testing
            max_entries: 100,
        });
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));

        assert!(limiter.check(ip));
        // Sleep briefly to allow refill
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(limiter.check(ip));
    }

    #[test]
    fn test_eviction() {
        let limiter = RateLimiter::new(RateLimitConfig {
            burst: 10,
            per_second: 1.0,
            max_entries: 2,
        });

        let ip1 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        let ip3 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3));

        limiter.check(ip1);
        limiter.check(ip2);
        // Adding ip3 should evict the oldest (ip1)
        limiter.check(ip3);

        let buckets = limiter.buckets.lock().unwrap();
        assert_eq!(buckets.len(), 2);
        assert!(!buckets.contains_key(&ip1));
    }
}
