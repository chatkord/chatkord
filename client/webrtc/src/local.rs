//! Two peer connections of one factory joined in memory, with no server: the engine negotiates, finds a path and
//! carries media between them.

use libwebrtc::ice_candidate::IceCandidate;
use libwebrtc::peer_connection::{OfferOptions, PeerConnection, PeerConnectionState};
use libwebrtc::peer_connection_factory::PeerConnectionFactory;
use libwebrtc::session_description::SessionDescription;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::{sleep, timeout};

/// How often the candidates one end has found are handed to the other.
const HANDOVER_EVERY: Duration = Duration::from_millis(20);

/// The two ends did not negotiate or did not connect.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Unjoined;

type Gathered = Arc<Mutex<Vec<IceCandidate>>>;

fn gather(connection: &PeerConnection) -> Gathered {
    let found: Gathered = Arc::default();
    let sink = Arc::clone(&found);
    connection.on_ice_candidate(Some(Box::new(move |candidate| {
        if let Ok(mut held) = sink.lock() {
            held.push(candidate);
        }
    })));
    found
}

/// The offering end and the answering end.
pub(crate) struct Local {
    pub(crate) offerer: PeerConnection,
    pub(crate) answerer: PeerConnection,
    offerer_found: Gathered,
    answerer_found: Gathered,
}

impl Local {
    pub(crate) fn new(factory: &PeerConnectionFactory) -> Result<Self, Unjoined> {
        let offerer = factory.create_peer_connection(Default::default()).map_err(|_| Unjoined)?;
        let answerer = factory.create_peer_connection(Default::default()).map_err(|_| Unjoined)?;
        let (offerer_found, answerer_found) = (gather(&offerer), gather(&answerer));
        Ok(Self { offerer, answerer, offerer_found, answerer_found })
    }

    /// Negotiates what the offerer has added, then hands the ends each other's candidates until both are connected.
    pub(crate) async fn join(&self, within: Duration) -> Result<(), Unjoined> {
        self.negotiate().await?;
        timeout(within, self.connect()).await.map_err(|_| Unjoined)?
    }

    pub(crate) fn close(&self) {
        self.offerer.close();
        self.answerer.close();
    }

    async fn negotiate(&self) -> Result<(), Unjoined> {
        let offer = self.offerer.create_offer(OfferOptions::default()).await.map_err(|_| Unjoined)?;
        self.offerer.set_local_description(offer.clone()).await.map_err(|_| Unjoined)?;
        let offer = SessionDescription::parse(&offer.to_string(), offer.sdp_type()).map_err(|_| Unjoined)?;
        self.answerer.set_remote_description(offer).await.map_err(|_| Unjoined)?;
        let answer = self.answerer.create_answer(Default::default()).await.map_err(|_| Unjoined)?;
        self.answerer.set_local_description(answer.clone()).await.map_err(|_| Unjoined)?;
        let answer = SessionDescription::parse(&answer.to_string(), answer.sdp_type()).map_err(|_| Unjoined)?;
        self.offerer.set_remote_description(answer).await.map_err(|_| Unjoined)
    }

    async fn connect(&self) -> Result<(), Unjoined> {
        let states = || (self.offerer.connection_state(), self.answerer.connection_state());
        while states() != (PeerConnectionState::Connected, PeerConnectionState::Connected) {
            if matches!(states(), (PeerConnectionState::Failed, _) | (_, PeerConnectionState::Failed)) {
                return Err(Unjoined);
            }
            for (found, to) in [(&self.offerer_found, &self.answerer), (&self.answerer_found, &self.offerer)] {
                let batch: Vec<IceCandidate> =
                    found.lock().map(|mut held| held.drain(..).collect()).unwrap_or_default();
                for candidate in batch {
                    // A candidate the other end cannot take is one more path it will not use.
                    if let Ok(copy) =
                        IceCandidate::parse(&candidate.sdp_mid(), candidate.sdp_mline_index(), &candidate.candidate())
                    {
                        let _ = to.add_ice_candidate(copy).await;
                    }
                }
            }
            sleep(HANDOVER_EVERY).await;
        }
        Ok(())
    }
}
