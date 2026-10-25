//! Pairing two PCs with a 6-digit code instead of a link (the joining PC finds the other one
//! over mDNS, the user types the code that PC shows).
//!
//! A short code can't be sent as is: anyone on the network can advertise a fake PC, and a
//! 6-digit code is guessed offline in a moment. So the code is only a SPAKE2 password: both PCs
//! derive a key from it and from the certificate fingerprint the joining PC sees. A man in the
//! middle (who has a different certificate) gets one online guess per attempt, and the PC
//! showing the code drops it after a few wrong ones. Each side then proves it holds the key
//! before the token is handed over.

use crate::config::sha256_hex;
use spake2::{Ed25519Group, Identity, Password, Spake2};

/// The joining side's SPAKE2 identity.
const JOINER: &[u8] = b"dotlink pc joining";
/// Wrong codes before the code shown is dropped (a new one is needed then).
pub const MAX_FAILURES: u32 = 3;

/// A new random 6-digit code.
pub fn new_code() -> String {
    let mut buf = [0u8; 4];
    getrandom::fill(&mut buf).expect("system random number generator");
    format!("{:06}", u32::from_le_bytes(buf) % 1_000_000)
}

/// "123 456", for reading out.
pub fn spaced(code: &str) -> String {
    if code.len() == 6 { format!("{} {}", &code[..3], &code[3..]) } else { code.to_string() }
}

/// What the user typed: digits only.
pub fn clean(code: &str) -> String {
    code.chars().filter(|c| c.is_ascii_digit()).collect()
}

pub struct Exchange(Spake2<Ed25519Group>);

/// The joining PC's first message. `fingerprint`: the other PC's certificate as seen on this
/// connection.
pub fn start_joiner(code: &str, fingerprint: &str) -> (Exchange, Vec<u8>) {
    let (state, msg) = Spake2::<Ed25519Group>::start_a(
        &Password::new(clean(code)),
        &Identity::new(JOINER),
        &Identity::new(fingerprint.as_bytes()),
    );
    (Exchange(state), msg)
}

/// The PC showing the code answers with its own message; `fingerprint` is its own certificate's.
pub fn start_shower(code: &str, fingerprint: &str) -> (Exchange, Vec<u8>) {
    let (state, msg) = Spake2::<Ed25519Group>::start_b(
        &Password::new(clean(code)),
        &Identity::new(JOINER),
        &Identity::new(fingerprint.as_bytes()),
    );
    (Exchange(state), msg)
}

impl Exchange {
    /// The shared key, or None for a malformed message. A wrong code still gives a key, just a
    /// different one: the confirmations tell.
    pub fn finish(self, other: &[u8]) -> Option<Vec<u8>> {
        self.0.finish(other).ok()
    }
}

/// Proof of holding `key`, one per direction so neither can be echoed back.
pub fn confirm(key: &[u8], joiner: bool) -> String {
    let label: &[u8] = if joiner { b"dotlink pair: joiner" } else { b"dotlink pair: shower" };
    sha256_hex(&[key, label].concat())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(joiner_code: &str, shower_code: &str, seen: &str, own: &str) -> bool {
        let (j, j_msg) = start_joiner(joiner_code, seen);
        let (s, s_msg) = start_shower(shower_code, own);
        let (jk, sk) = (j.finish(&s_msg).unwrap(), s.finish(&j_msg).unwrap());
        confirm(&jk, true) == confirm(&sk, true) && confirm(&jk, false) == confirm(&sk, false)
    }

    #[test]
    fn same_code_and_certificate_agree() {
        assert!(run("123 456", "123456", "fp", "fp"));
    }

    #[test]
    fn wrong_code_or_other_certificate_disagree() {
        assert!(!run("123457", "123456", "fp", "fp"));
        // A man in the middle shows the joiner its own certificate.
        assert!(!run("123456", "123456", "attacker", "fp"));
    }

    #[test]
    fn codes_are_six_digits() {
        for _ in 0..50 {
            let code = new_code();
            assert_eq!(code.len(), 6);
            assert!(code.chars().all(|c| c.is_ascii_digit()));
        }
        assert_eq!(spaced("012345"), "012 345");
    }
}
