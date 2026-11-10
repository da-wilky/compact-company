use std::{
    collections::HashMap,
    net::{IpAddr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::Instant,
};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{HeaderMap, request::Parts},
};

use crate::{
    App,
    api::error::{ApiError, ErrorCode},
};

/// Requests one address may make in a burst before it has to wait.
const BURST: f64 = 20.0;
/// Requests one address regains per second: ten a minute.
const REFILL: f64 = 10.0 / 60.0;
/// Addresses remembered before idle ones are forgotten.
const REMEMBERED: usize = 10_000;

/// A token bucket per client address for the requests anyone may make without signing in, so
/// nobody can fill the ceremony table or guess invitation links at speed.
pub struct Limiter {
    buckets: Mutex<HashMap<IpAddr, (f64, Instant)>>,
    trusted_proxies: Vec<IpAddr>,
}

impl Limiter {
    pub fn new(trusted_proxies: Vec<IpAddr>) -> Self {
        Self {
            buckets: Mutex::default(),
            trusted_proxies,
        }
    }

    fn allow(&self, client: IpAddr) -> bool {
        let now = Instant::now();
        let mut buckets = self
            .buckets
            .lock()
            .expect("no panic while holding the lock");
        if buckets.len() >= REMEMBERED {
            buckets.retain(|_, (tokens, at)| {
                *tokens + now.duration_since(*at).as_secs_f64() * REFILL < BURST
            });
        }
        let (tokens, at) = buckets.entry(client).or_insert((BURST, now));
        *tokens = (*tokens + now.duration_since(*at).as_secs_f64() * REFILL).min(BURST);
        *at = now;
        if *tokens < 1.0 {
            return false;
        }
        *tokens -= 1.0;
        true
    }

    /// The client's address: the peer's, or behind a trusted proxy the last address in
    /// `X-Forwarded-For` that no trusted proxy added. IPv6 clients count per /64, since one
    /// host usually holds a whole /64.
    pub fn client(&self, peer: Option<IpAddr>, headers: &HeaderMap) -> IpAddr {
        let peer = peer.unwrap_or(IpAddr::from([0, 0, 0, 0]));
        let mut client = peer;
        if self.trusted(peer) {
            let forwarded = headers
                .get_all("x-forwarded-for")
                .iter()
                .filter_map(|v| v.to_str().ok())
                .flat_map(|v| v.split(','))
                .map(str::trim)
                .collect::<Vec<_>>();
            for hop in forwarded.iter().rev() {
                let Ok(address) = hop.parse::<IpAddr>() else {
                    break;
                };
                client = address;
                if !self.trusted(address) {
                    break;
                }
            }
        }
        match client {
            IpAddr::V6(v6) if v6.to_ipv4_mapped().is_none() => {
                let s = v6.segments();
                IpAddr::V6(Ipv6Addr::new(s[0], s[1], s[2], s[3], 0, 0, 0, 0))
            }
            IpAddr::V6(v6) => IpAddr::V4(v6.to_ipv4_mapped().expect("checked above")),
            v4 => v4,
        }
    }

    fn trusted(&self, address: IpAddr) -> bool {
        let address = match address {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(address, IpAddr::V4),
            v4 => v4,
        };
        self.trusted_proxies.contains(&address)
    }
}

/// Spends one of the client's requests, or refuses with `rate_limited`.
pub struct Limited;

impl FromRequestParts<Arc<App>> for Limited {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<App>) -> Result<Self, ApiError> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(addr)| addr.ip());
        let client = app.limiter.client(peer, &parts.headers);
        if app.limiter.allow(client) {
            Ok(Limited)
        } else {
            Err(ApiError::new(ErrorCode::RateLimited))
        }
    }
}
