//! Waiting for the keyboard after it stopped answering: a report-rate change makes it
//! re-enumerate on USB (protocol.md 9.7), so the link is gone and must be reopened.

use std::time::{Duration, Instant};

use crate::error::Result;

/// How long to wait for the keyboard to come back, and how often to try.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wait {
    pub timeout: Duration,
    pub poll: Duration,
}

impl Wait {
    /// A USB re-enumeration: up to 10 s, polled every 500 ms.
    pub const USB: Wait = Wait {
        timeout: Duration::from_secs(10),
        poll: Duration::from_millis(500),
    };
}

/// Calls `connect` every `wait.poll` (first after one poll interval) until it succeeds,
/// for at most `wait.timeout`; the last error otherwise.
pub fn reconnect_with<K>(wait: Wait, mut connect: impl FnMut() -> Result<K>) -> Result<K> {
    let deadline = Instant::now() + wait.timeout;
    loop {
        std::thread::sleep(wait.poll);
        match connect() {
            Ok(k) => return Ok(k),
            Err(e) if Instant::now() >= deadline => return Err(e),
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    const FAST: Wait = Wait {
        timeout: Duration::from_millis(30),
        poll: Duration::from_millis(1),
    };

    #[test]
    fn retries_until_the_keyboard_answers() {
        let mut tries = 0;
        let got = reconnect_with(FAST, || {
            tries += 1;
            if tries < 3 {
                Err(Error::NotFound)
            } else {
                Ok(tries)
            }
        });
        assert_eq!(got.unwrap(), 3);
    }

    #[test]
    fn gives_up_after_the_timeout() {
        let got: Result<()> = reconnect_with(FAST, || Err(Error::NotFound));
        assert!(matches!(got, Err(Error::NotFound)));
    }
}
