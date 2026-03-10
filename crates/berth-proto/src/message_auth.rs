//! HMAC-SHA256 message authentication for NATS commands.
//!
//! Provides signing and verification of NATS command payloads to prevent
//! unauthorized command injection. Uses a shared secret established during
//! agent pairing.

use sha2::Sha256;
use hmac::{Hmac, Mac};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

/// Maximum age of a command before it's rejected (seconds).
const MAX_COMMAND_AGE_SECS: i64 = 60;

/// Maximum number of nonces to track for replay prevention.
const MAX_NONCE_HISTORY: usize = 10_000;

/// Sign a command payload with HMAC-SHA256.
///
/// The signature covers: `nonce|timestamp|payload`
pub fn sign_command(payload: &[u8], nonce: &str, timestamp: i64, secret: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret)
        .expect("HMAC accepts any key length");
    mac.update(nonce.as_bytes());
    mac.update(&timestamp.to_le_bytes());
    mac.update(payload);
    hex::encode(mac.finalize().into_bytes())
}

/// Verify a command's HMAC-SHA256 signature.
pub fn verify_signature(payload: &[u8], nonce: &str, timestamp: i64, signature: &str, secret: &[u8]) -> bool {
    let expected = sign_command(payload, nonce, timestamp, secret);
    // Constant-time comparison
    constant_time_eq(expected.as_bytes(), signature.as_bytes())
}

/// Check if a timestamp is within the acceptable window.
pub fn is_timestamp_valid(timestamp: i64) -> bool {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let age = (now - timestamp).abs();
    age <= MAX_COMMAND_AGE_SECS
}

/// Get current Unix timestamp.
pub fn current_timestamp() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Constant-time byte comparison to prevent timing attacks (public for pairing challenge).
pub fn constant_time_eq_public(a: &[u8], b: &[u8]) -> bool {
    constant_time_eq(a, b)
}

/// Constant-time byte comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

/// Tracks recently seen nonces to prevent replay attacks.
pub struct NonceTracker {
    seen: Mutex<VecDeque<String>>,
}

impl NonceTracker {
    pub fn new() -> Self {
        Self {
            seen: Mutex::new(VecDeque::with_capacity(MAX_NONCE_HISTORY)),
        }
    }

    /// Returns `true` if the nonce is new (not seen before).
    /// Returns `false` if it's a replay.
    pub fn check_and_record(&self, nonce: &str) -> bool {
        let mut seen = self.seen.lock().unwrap();
        // Linear scan is fine for bounded set
        if seen.iter().any(|n| n == nonce) {
            return false;
        }
        if seen.len() >= MAX_NONCE_HISTORY {
            seen.pop_front();
        }
        seen.push_back(nonce.to_string());
        true
    }
}

impl Default for NonceTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_verify_roundtrip() {
        let secret = b"test-shared-secret-256bit-value!";
        let payload = b"hello world";
        let nonce = "unique-nonce-123";
        let ts = current_timestamp();

        let sig = sign_command(payload, nonce, ts, secret);
        assert!(verify_signature(payload, nonce, ts, &sig, secret));
    }

    #[test]
    fn test_tampered_payload_rejected() {
        let secret = b"test-shared-secret-256bit-value!";
        let payload = b"hello world";
        let nonce = "unique-nonce-123";
        let ts = current_timestamp();

        let sig = sign_command(payload, nonce, ts, secret);
        assert!(!verify_signature(b"tampered", nonce, ts, &sig, secret));
    }

    #[test]
    fn test_wrong_secret_rejected() {
        let secret = b"correct-secret";
        let wrong = b"wrong-secret!!";
        let payload = b"hello world";
        let nonce = "nonce";
        let ts = current_timestamp();

        let sig = sign_command(payload, nonce, ts, secret);
        assert!(!verify_signature(payload, nonce, ts, &sig, wrong));
    }

    #[test]
    fn test_expired_timestamp() {
        let old_ts = current_timestamp() - 120; // 2 minutes old
        assert!(!is_timestamp_valid(old_ts));

        let valid_ts = current_timestamp() - 30; // 30 seconds old
        assert!(is_timestamp_valid(valid_ts));

        let now_ts = current_timestamp();
        assert!(is_timestamp_valid(now_ts));
    }

    #[test]
    fn test_nonce_replay_prevention() {
        let tracker = NonceTracker::new();
        assert!(tracker.check_and_record("nonce-1"));
        assert!(tracker.check_and_record("nonce-2"));
        // Replay should fail
        assert!(!tracker.check_and_record("nonce-1"));
        assert!(!tracker.check_and_record("nonce-2"));
        // New nonce should succeed
        assert!(tracker.check_and_record("nonce-3"));
    }

    #[test]
    fn test_nonce_tracker_eviction() {
        let tracker = NonceTracker::new();
        for i in 0..MAX_NONCE_HISTORY {
            assert!(tracker.check_and_record(&format!("nonce-{i}")));
        }
        // Queue is full (10K items: nonce-0 through nonce-9999)
        // nonce-0 is still tracked
        assert!(!tracker.check_and_record("nonce-0"));
        // Insert a new item to evict nonce-0
        assert!(tracker.check_and_record("overflow"));
        // Now nonce-0 should be evicted
        assert!(tracker.check_and_record("nonce-0"));
        // Latest should still be tracked
        assert!(!tracker.check_and_record(&format!("nonce-{}", MAX_NONCE_HISTORY - 1)));
    }
}
