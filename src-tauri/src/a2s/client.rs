//! Async UDP client: one short-lived socket per query, challenge handling,
//! split-packet reassembly, bounded fan-out and send pacing (docs/03 §7, docs/05 §3).
//!
//! Pacing exists because loss rises sharply with burst size through consumer NAT:
//! 18 678 addresses answered 12 561 at 256-wide / 2 s, 9 326 at 256-wide / 1 s and
//! only 3 803 at 512-wide / 1 s (D-037). A minimum gap between datagrams keeps the
//! send rate predictable regardless of how many tasks are waiting.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::net::UdpSocket;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;

use super::packet::{challenge_of, classify, Datagram, Kind, Reassembler};
use super::{info, players, rules, A2sError, A2sResult, Info, Players, Rules};

/// Default send rate; ~400 datagrams/s stayed loss-free on a home connection.
pub const DEFAULT_PACKETS_PER_SECOND: u32 = 400;
pub const DEFAULT_CONCURRENCY: usize = 128;

#[derive(Debug, Clone)]
pub struct Reply<T> {
    pub value: T,
    /// Round trip of the final (post-challenge) exchange.
    pub rtt: Duration,
    pub packets: u8,
}

#[derive(Debug)]
struct Pacer {
    gap: Duration,
    next: Mutex<Instant>,
}

#[derive(Debug, Clone)]
pub struct Client {
    pub timeout: Duration,
    /// Extra attempts after a timeout.
    pub retries: u8,
    permits: Arc<Semaphore>,
    pacer: Option<Arc<Pacer>>,
}

impl Default for Client {
    fn default() -> Self {
        Self::new(DEFAULT_CONCURRENCY)
    }
}

impl Client {
    pub fn new(concurrency: usize) -> Self {
        Self {
            timeout: Duration::from_secs(1),
            retries: 1,
            permits: Arc::new(Semaphore::new(concurrency.max(1))),
            pacer: None,
        }
        .with_rate(DEFAULT_PACKETS_PER_SECOND)
    }

    pub fn with_timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }

    pub fn with_retries(mut self, n: u8) -> Self {
        self.retries = n;
        self
    }

    /// A copy with its own permit pool.
    ///
    /// `with_rate` rebuilds the pacer but keeps the shared `Arc<Semaphore>`, so a bulk
    /// pass started from `state.a2s` queues in the same FIFO as the interactive
    /// queries. That costs twice. The pass inherits the interactive pool's size, and
    /// concurrency ÷ rate is how long a permit-holder waits for its datagram: at
    /// 128 ÷ 100 pps that is 1.28 s, already past the 1 s deadline before a challenge
    /// reply can be answered, so nearly every query was being rescued by its retry —
    /// 6 582 datagrams where 4 200 were needed. And in the other direction, a pass
    /// that streams instead of running in chunks holds those permits continuously and
    /// starves a row click of its 3 queries. Own pool, both problems gone (D-193).
    pub fn with_concurrency(mut self, n: usize) -> Self {
        self.permits = Arc::new(Semaphore::new(n.max(1)));
        self
    }

    /// Maximum datagrams per second across all queries; `0` disables pacing.
    pub fn with_rate(mut self, packets_per_second: u32) -> Self {
        self.pacer = (packets_per_second > 0).then(|| {
            Arc::new(Pacer {
                gap: Duration::from_secs_f64(1.0 / packets_per_second as f64),
                next: Mutex::new(Instant::now()),
            })
        });
        self
    }

    pub async fn info(&self, addr: SocketAddr) -> A2sResult<Reply<Info>> {
        let (payload, rtt, packets) = self.query(addr, Kind::Info).await?;
        Ok(Reply {
            value: info::parse(&payload)?,
            rtt,
            packets,
        })
    }

    pub async fn rules(&self, addr: SocketAddr) -> A2sResult<Reply<Rules>> {
        let (payload, rtt, packets) = self.query(addr, Kind::Rules).await?;
        Ok(Reply {
            value: rules::parse(&payload)?,
            rtt,
            packets,
        })
    }

    pub async fn players(&self, addr: SocketAddr) -> A2sResult<Reply<Players>> {
        let (payload, rtt, packets) = self.query(addr, Kind::Players).await?;
        Ok(Reply {
            value: players::parse(&payload)?,
            rtt,
            packets,
        })
    }

    /// INFO for many servers under the client's concurrency and rate bounds.
    /// Results come back in completion order.
    pub async fn info_many(
        &self,
        addrs: impl IntoIterator<Item = SocketAddr>,
    ) -> Vec<(SocketAddr, A2sResult<Reply<Info>>)> {
        let mut set = JoinSet::new();
        for addr in addrs {
            let c = self.clone();
            set.spawn(async move { (addr, c.info(addr).await) });
        }
        let mut out = Vec::with_capacity(set.len());
        while let Some(r) = set.join_next().await {
            if let Ok(pair) = r {
                out.push(pair);
            }
        }
        out
    }

    /// Waits for the next send slot when pacing is on.
    async fn pace(&self) {
        let Some(p) = &self.pacer else { return };
        let at = {
            let mut next = p.next.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            let at = if *next > now { *next } else { now };
            *next = at + p.gap;
            at
        };
        tokio::time::sleep_until(tokio::time::Instant::from_std(at)).await;
    }

    async fn query(&self, addr: SocketAddr, kind: Kind) -> A2sResult<(Vec<u8>, Duration, u8)> {
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| A2sError::Timeout)?;
        // One socket for the attempt and its retry: binding inside `query_once`
        // opened a second NAT flow for the same target on every retry, and burst
        // size is exactly what D-037 measured as the cause of answer loss (D-160).
        let sock = UdpSocket::bind(if addr.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        })
        .await?;
        sock.connect(addr).await?;
        let mut last = A2sError::Timeout;
        for _ in 0..=self.retries {
            match self.query_once(&sock, kind).await {
                Ok(v) => return Ok(v),
                Err(A2sError::Timeout) => last = A2sError::Timeout,
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    async fn query_once(&self, sock: &UdpSocket, kind: Kind) -> A2sResult<(Vec<u8>, Duration, u8)> {
        let mut challenge: Option<[u8; 4]> = None;
        let mut buf = vec![0u8; kind.max_datagram()];
        let mut challenges_seen = 0u8;
        // The deadline starts at the first send so pacing delay is not charged to the server.
        let mut deadline: Option<Instant> = None;

        'attempt: loop {
            self.pace().await;
            let sent_at = Instant::now();
            let deadline = *deadline.get_or_insert(sent_at + self.timeout);
            // After a challenge the pacer can hold the request past the deadline, and it
            // went out anyway into a wait that was already over: one more datagram and
            // NAT flow for nothing (D-037, D-256).
            if sent_at >= deadline {
                return Err(A2sError::Timeout);
            }
            sock.send(&kind.request(challenge)).await?;
            let mut reasm = Reassembler::new();
            let mut packets = 0u8;
            loop {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Err(A2sError::Timeout);
                }
                let n = match timeout(remaining, sock.recv(&mut buf)).await {
                    Ok(Ok(n)) => n,
                    Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                        return Err(A2sError::Unreachable)
                    }
                    Ok(Err(e)) => return Err(e.into()),
                    Err(_) => return Err(A2sError::Timeout),
                };
                let rtt = sent_at.elapsed();
                packets = packets.saturating_add(1);
                match classify(&buf[..n])? {
                    Datagram::Single(payload) => {
                        if let Some(c) = challenge_of(payload) {
                            challenges_seen += 1;
                            if challenges_seen > 3 {
                                return Err(A2sError::ChallengeLoop);
                            }
                            challenge = Some(c);
                            continue 'attempt;
                        }
                        if payload.first() != Some(&kind.response_type()) {
                            continue; // stray datagram; keep waiting
                        }
                        return Ok((payload.to_vec(), rtt, packets));
                    }
                    Datagram::Split {
                        id,
                        total,
                        number,
                        body,
                        ..
                    } => {
                        if let Some(full) = reasm.push(id, total, number, body)? {
                            return Ok((full, rtt, packets));
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_rate_shares_the_permit_pool_and_with_concurrency_does_not() {
        // The distinction D-193 turns on: a bulk pass built with `with_rate` alone is
        // still queueing behind whatever else holds `state.a2s`'s permits.
        let base = Client::new(128);
        let paced = base.clone().with_rate(100);
        assert!(
            Arc::ptr_eq(&base.permits, &paced.permits),
            "with_rate must not be mistaken for isolation"
        );
        assert_eq!(paced.permits.available_permits(), 128);

        let own = base.clone().with_concurrency(64).with_rate(100);
        assert!(!Arc::ptr_eq(&base.permits, &own.permits));
        assert_eq!(own.permits.available_permits(), 64);
        assert_eq!(base.permits.available_permits(), 128);
    }

    #[tokio::test]
    async fn pacing_spreads_sends() {
        let c = Client::new(64).with_rate(200); // 5 ms gap
        let t0 = Instant::now();
        for _ in 0..20 {
            c.pace().await;
        }
        let dt = t0.elapsed();
        assert!(
            dt >= Duration::from_millis(90),
            "20 slots at 200/s should take ≥ 95 ms, took {dt:?}"
        );
        let unpaced = Client::new(64).with_rate(0);
        let t1 = Instant::now();
        for _ in 0..20 {
            unpaced.pace().await;
        }
        assert!(t1.elapsed() < Duration::from_millis(20));
    }
}
