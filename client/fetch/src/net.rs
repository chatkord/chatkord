//! Where a preview's connections come from: the names it resolves and the sockets it opens. A trait, so a test
//! supplies both; production has only [`Internet`], and the guard sits between the two calls in the caller.

use crate::get::{GetError, Stream};
use crate::guard::Vetted;
use link::Trust;
use std::net::SocketAddr;
use terms::LinkError;

/// Names to addresses, and addresses to connections.
pub trait Reach: Send + Sync {
    /// The addresses `host` resolves to at `port`, asked once per address a preview visits.
    fn resolve(&self, host: &str, port: u16) -> impl Future<Output = Result<Vec<SocketAddr>, GetError>> + Send;

    /// A connection secured as `host`'s own, to one of `to`'s addresses, which is the only place it may connect.
    fn connect(&self, to: &Vetted, host: &str) -> impl Future<Output = Result<Stream, GetError>> + Send;
}

/// The internet: the system's resolver, TCP, and TLS 1.2 or 1.3 checked by the operating system's verifier alone.
#[derive(Clone, Debug)]
pub struct Internet {
    trust: Trust,
}

impl Internet {
    /// A connector to the internet sharing `trust`'s verifier.
    pub fn new(trust: &Trust) -> Internet {
        Internet { trust: trust.clone() }
    }
}

impl Reach for Internet {
    async fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, GetError> {
        let found = tokio::net::lookup_host((host, port)).await.map_err(|_| GetError::Reach(LinkError::NoSuchHost))?;
        Ok(found.collect())
    }

    async fn connect(&self, to: &Vetted, host: &str) -> Result<Stream, GetError> {
        let secured = self.trust.secure_at(to.addresses().to_vec(), host).await.map_err(GetError::Reach)?;
        Ok(Box::new(secured))
    }
}
