//! One file fetched from a feed: the shared bounded GET (`fetch`), its connection `https` under the system's verifier
//! as every server's, or for a check by hand plain HTTP to this machine's loopback; redirects taken to `https` only.

use crate::address::{Address, Reached};
use crate::error::UpdateError;
use fetch::{GetError, Route, Stream};
use link::Trust;
use terms::LinkError;
use tokio::net::TcpStream;

pub(crate) use fetch::{Answer, Patience};

/// The most redirects one fetch follows: a release host's download answers with one.
pub(crate) const MAX_REDIRECTS: usize = 5;

impl From<GetError> for UpdateError {
    fn from(failed: GetError) -> UpdateError {
        match failed {
            GetError::Reach(failed) => UpdateError::Reach(failed),
            // A feed's route vets no address.
            GetError::Refused => UpdateError::Reach(LinkError::Connect),
            GetError::Timeout => UpdateError::Timeout,
            GetError::Lost => UpdateError::Lost,
            GetError::Status(status) => UpdateError::Status(status),
            GetError::BadRedirect => UpdateError::BadRedirect,
            GetError::TooManyRedirects => UpdateError::TooManyRedirects,
            GetError::TooLarge => UpdateError::TooLarge,
            GetError::Malformed => UpdateError::Malformed,
        }
    }
}

/// A feed's address, reached under `trust`.
struct Hop<'a> {
    trust: &'a Trust,
    at: Address,
}

impl Route for Hop<'_> {
    fn authority(&self) -> &str {
        self.at.authority()
    }

    fn origin_form(&self) -> &str {
        self.at.origin_form()
    }

    fn user_agent(&self) -> String {
        format!("{}/{}", channel::THIS.file, terms::BUILD)
    }

    async fn connect(&self) -> Result<Stream, GetError> {
        match self.at.reached() {
            Reached::Https => {
                let secured = self.trust.secure(self.at.uri()).await.map_err(GetError::Reach)?;
                Ok(Box::new(secured))
            }
            Reached::Loopback(socket) => {
                let stream = TcpStream::connect(socket).await.map_err(|_| GetError::Reach(LinkError::Connect))?;
                Ok(Box::new(stream))
            }
        }
    }

    fn redirected(&self, location: &str) -> Result<Self, GetError> {
        let at = self.at.redirected(location).map_err(|_| GetError::BadRedirect)?;
        Ok(Hop { trust: self.trust, at })
    }
}

/// The file at `address`, following at most [`MAX_REDIRECTS`] redirects, each to `https`.
pub(crate) async fn get(trust: &Trust, address: &Address, patience: Patience) -> Result<Answer, UpdateError> {
    let (answer, _) = fetch::get(Hop { trust, at: address.clone() }, patience, MAX_REDIRECTS).await?;
    Ok(answer)
}
