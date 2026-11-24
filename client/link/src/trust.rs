use crate::session::{about_the_network_io, connect, reach};
use hyper::Uri;
use proto::pin::KeyPin;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, verify_tls12_signature, verify_tls13_signature};
use rustls::{AlertDescription, CertificateError, ClientConfig, DigitallySignedStruct, PeerIncompatible};
use rustls::{SignatureScheme, version};
use rustls_pki_types::{CertificateDer, ServerName, UnixTime};
use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use terms::LinkError;
use tokio::net::TcpStream;
use tokio::sync::OnceCell;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

/// Whom the client believes about a server's certificate: the operating system's verifier (on Linux its roots, or
/// for a script the certificates `SSL_CERT_FILE` / `SSL_CERT_DIR` name), and for a server it does not vouch for, the
/// key this device was told to trust there. The system's verifier is made on the first connect, on the blocking
/// pool, and each server's configuration on its first connect; every clone, taken before or after, keeps both, so
/// the roots are read once a process and a later connect to a server resumes its session.
#[derive(Clone, Default)]
pub struct Trust {
    shared: Arc<Shared>,
    pin: Option<KeyPin>,
}

/// What every clone of a [`Trust`] keeps: the system's verifier, each server's configuration by its authority, and
/// the one configuration every public site is reached with.
#[derive(Default)]
struct Shared {
    system: OnceCell<Result<Arc<System>, Unbuilt>>,
    servers: Mutex<HashMap<String, Arc<Tls>>>,
    web: OnceLock<Arc<ClientConfig>>,
}

/// What vouched for the server a connection reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vouched {
    /// The operating system's verifier: an authority, or a root this device was given. A key kept for that server
    /// is needed no more.
    System,
    /// The key this device was told to trust for that server.
    Pin,
    /// Nothing: an unencrypted address, which only this machine's may be.
    Plain,
}

/// Why the operating system's verifier could not be made.
#[derive(Clone, Debug, report::Public)]
enum Unbuilt {
    /// It refused to start: on Linux, no root certificate was found.
    System(rustls::Error),
    /// The blocking pool ended before it was made.
    Pool,
}

impl fmt::Display for Unbuilt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System(failed) => write!(f, "the system's certificate verifier: {failed}"),
            Self::Pool => f.write_str("the blocking pool ended"),
        }
    }
}

/// The operating system's verifier and the crypto every handshake is checked with.
#[derive(Debug)]
struct System {
    verifier: Arc<dyn ServerCertVerifier>,
    provider: Arc<CryptoProvider>,
}

impl System {
    fn build() -> Result<Arc<System>, Unbuilt> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = rustls_platform_verifier::Verifier::new(Arc::clone(&provider)).map_err(|failed| {
            let unbuilt = Unbuilt::System(failed);
            report::note!(warn, "no certificate can be checked", why = unbuilt);
            unbuilt
        })?;
        Ok(Arc::new(System { verifier: Arc::new(verifier), provider }))
    }
}

impl fmt::Debug for Trust {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Trust").field("pin", &self.pin).finish_non_exhaustive()
    }
}

impl Trust {
    /// The operating system's verifier alone: a server whose certificate an authority, or this device, vouches for.
    pub fn system() -> Self {
        Self::default()
    }

    /// The same verifier and configurations, and for a server it does not vouch for, `pin`: the key this device was
    /// told to trust there, or none.
    pub fn for_server(&self, pin: Option<KeyPin>) -> Trust {
        Trust { shared: Arc::clone(&self.shared), pin }
    }

    /// The configuration a handshake with `server` (an address's authority) uses: the one kept for it while its pin
    /// is this trust's, else a new one (TLS 1.3 only, checked by [`Pinned`]) kept in its place.
    pub(crate) async fn handshake(&self, server: &str) -> Result<Handshake, LinkError> {
        let system = self.verifier().await?;
        let mut servers = self.shared.servers.lock().unwrap_or_else(PoisonError::into_inner);
        let tls = match servers.get(server) {
            Some(kept) if kept.pin == self.pin => Arc::clone(kept),
            Some(_) | None => {
                let made = Arc::new(Tls::new(&system, self.pin)?);
                servers.insert(server.to_string(), Arc::clone(&made));
                made
            }
        };
        Ok(Handshake { tls, shared: Arc::clone(&self.shared), server: server.to_string() })
    }

    /// A connection to `uri`'s host, at its port or else 443, secured by TLS 1.3 under this trust: the first of the
    /// host's addresses to answer, its certificate checked as a server's is. A failure is said as a socket's would be.
    pub async fn secure(&self, uri: &Uri) -> Result<TlsStream<TcpStream>, LinkError> {
        let authority = uri.authority().ok_or(LinkError::NoSuchHost)?;
        let name = uri
            .host()
            .map(|host| host.trim_start_matches('[').trim_end_matches(']'))
            .and_then(|host| ServerName::try_from(host.to_string()).ok())
            .ok_or(LinkError::NoSuchHost)?;
        let stream = reach(uri).await?;
        let handshake = self.handshake(authority.as_str()).await?;
        TlsConnector::from(handshake.config()).connect(name, stream).await.map_err(|failed| {
            if about_the_network_io(&failed) { LinkError::Connect } else { handshake.refused(Some(&failed)) }
        })
    }

    /// A connection to the first of `addresses` to answer, secured as the public site `host` (a name, or an address
    /// without brackets): TLS 1.2 or 1.3, as sites this client does not run speak, vouched for by the operating
    /// system's verifier alone, never a pin. No name is looked up, so the caller's choice of addresses holds.
    pub async fn secure_at(&self, addresses: Vec<SocketAddr>, host: &str) -> Result<TlsStream<TcpStream>, LinkError> {
        let name = ServerName::try_from(host.to_string()).map_err(|_| LinkError::NoSuchHost)?;
        let system = self.verifier().await?;
        let config = match self.shared.web.get() {
            Some(kept) => Arc::clone(kept),
            None => {
                let made = ClientConfig::builder_with_provider(Arc::clone(&system.provider))
                    .with_safe_default_protocol_versions()
                    .map_err(|_| LinkError::Untrusted)?
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::clone(&system.verifier))
                    .with_no_client_auth();
                Arc::clone(self.shared.web.get_or_init(|| Arc::new(made)))
            }
        };
        let stream = connect(addresses).await?;
        TlsConnector::from(config)
            .connect(name, stream)
            .await
            .map_err(|failed| if about_the_network_io(&failed) { LinkError::Connect } else { LinkError::Untrusted })
    }

    async fn verifier(&self) -> Result<Arc<System>, LinkError> {
        let built = || async { tokio::task::spawn_blocking(System::build).await.unwrap_or(Err(Unbuilt::Pool)) };
        self.shared.system.get_or_init(built).await.clone().map_err(|_| LinkError::Untrusted)
    }
}

/// A server's TLS configuration for one pin, with its sessions, and what its verifier last decided about the
/// server's certificate: a resumed handshake runs no verifier and re-uses a session that decision vouched for.
struct Tls {
    pin: Option<KeyPin>,
    config: Arc<ClientConfig>,
    verdict: Arc<Mutex<Option<Result<Vouched, LinkError>>>>,
}

impl Tls {
    fn new(system: &System, pin: Option<KeyPin>) -> Result<Tls, LinkError> {
        let verdict = Arc::new(Mutex::new(None));
        let verifier = Pinned {
            system: Arc::clone(&system.verifier),
            provider: Arc::clone(&system.provider),
            pin,
            verdict: Arc::clone(&verdict),
        };
        let config = ClientConfig::builder_with_provider(Arc::clone(&system.provider))
            .with_protocol_versions(&[&version::TLS13])
            .map_err(|_| LinkError::Untrusted)?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        Ok(Tls { pin, config: Arc::new(config), verdict })
    }

    fn verdict(&self) -> Option<Result<Vouched, LinkError>> {
        self.verdict.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// One connect's use of a server's configuration.
pub(crate) struct Handshake {
    tls: Arc<Tls>,
    shared: Arc<Shared>,
    server: String,
}

impl Handshake {
    /// The configuration, sessions and all.
    pub(crate) fn config(&self) -> Arc<ClientConfig> {
        Arc::clone(&self.tls.config)
    }

    /// What vouched for the server, once the handshake is done, resumed or not.
    pub(crate) fn vouched(&self) -> Result<Vouched, LinkError> {
        self.tls.verdict().unwrap_or(Err(LinkError::Untrusted))
    }

    /// Why a handshake that failed with `failed`, when the TLS library said, did: what the verifier decided of the
    /// key, a server too old for TLS 1.3, else a certificate this device does not take. The configuration is not
    /// kept, so the next connect starts with no session and no verdict of this one's.
    pub(crate) fn refused(&self, failed: Option<&std::io::Error>) -> LinkError {
        let mut servers = self.shared.servers.lock().unwrap_or_else(PoisonError::into_inner);
        if servers.get(&self.server).is_some_and(|kept| Arc::ptr_eq(kept, &self.tls)) {
            servers.remove(&self.server);
        }
        match self.tls.verdict() {
            Some(Err(refused)) => refused,
            Some(Ok(_)) | None if failed.and_then(tls_error).is_some_and(too_old) => LinkError::OldEncryption,
            Some(Ok(_)) | None => LinkError::Untrusted,
        }
    }
}

/// The TLS library's own error inside a connection's I/O error, where it puts one.
fn tls_error(io: &std::io::Error) -> Option<&rustls::Error> {
    io.get_ref()?.downcast_ref()
}

/// Whether a handshake failed because the server speaks no TLS 1.3: it said so, or answered in an older version.
fn too_old(failed: &rustls::Error) -> bool {
    matches!(
        failed,
        rustls::Error::AlertReceived(AlertDescription::ProtocolVersion)
            | rustls::Error::PeerIncompatible(
                PeerIncompatible::ServerTlsVersionIsDisabledByOurConfig
                    | PeerIncompatible::ServerDoesNotSupportTls12Or13
            )
    )
}

/// A server's verifier for one pin: the operating system's, and for a certificate it vouches for only for want of an
/// authority, that key. What it last decided is kept for each connect to read.
#[derive(Debug)]
struct Pinned {
    system: Arc<dyn ServerCertVerifier>,
    provider: Arc<CryptoProvider>,
    pin: Option<KeyPin>,
    verdict: Arc<Mutex<Option<Result<Vouched, LinkError>>>>,
}

impl ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let system = self.system.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now);
        let dates = dates_at(end_entity, now, &self.provider);
        let judged = judge(system.as_ref().map(|_| ()), end_entity, self.pin, dates);
        let accepted = judged.is_ok();
        *self.verdict.lock().unwrap_or_else(PoisonError::into_inner) = Some(judged);
        match system {
            Err(_) if accepted => Ok(ServerCertVerified::assertion()),
            said => said,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// What the system's answer about a certificate and the key kept for its server come to. A trusted key is judged by
/// the key alone, as an SSH host key is, never by its certificate's dates or names; a revocation still stands.
fn judge(
    system: Result<(), &rustls::Error>,
    end_entity: &CertificateDer<'_>,
    pin: Option<KeyPin>,
    dates: InDates,
) -> Result<Vouched, LinkError> {
    let read = webpki::EndEntityCert::try_from(end_entity).ok().and_then(|leaf| Some((key_pin(&leaf)?, leaf)));
    let Some((presented, leaf)) = read else {
        return system.map(|()| Vouched::System).map_err(|_| LinkError::Untrusted);
    };
    match (pin, system) {
        (Some(trusted), Err(failed)) if trusted == presented && revoked(failed) => Err(LinkError::Untrusted),
        (Some(trusted), _) if trusted == presented => Ok(Vouched::Pin),
        (_, Ok(())) => Ok(Vouched::System),
        (Some(trusted), Err(_)) => Err(LinkError::KeyChanged { trusted, presented }),
        (None, Err(failed))
            if (unvouched(failed) || out_of_date(failed))
                && (dates == InDates::Yes || leaf.issuer() == leaf.subject()) =>
        {
            Err(LinkError::Unvouched(presented))
        }
        (None, Err(_)) => Err(LinkError::Untrusted),
    }
}

/// Whether the system refused a certificate only for want of an authority: its issuer is not one it knows, or it
/// signs itself as an authority would (what `openssl req -x509` makes by default).
fn unvouched(failed: &rustls::Error) -> bool {
    match failed {
        rustls::Error::InvalidCertificate(CertificateError::UnknownIssuer) => true,
        rustls::Error::InvalidCertificate(CertificateError::Other(other)) => {
            matches!(other.0.downcast_ref::<webpki::Error>(), Some(webpki::Error::CaUsedAsEndEntity))
        }
        _ => false,
    }
}

/// Whether the system refused a certificate for its dates, which it judges before its issuer.
fn out_of_date(failed: &rustls::Error) -> bool {
    matches!(
        failed,
        rustls::Error::InvalidCertificate(
            CertificateError::Expired
                | CertificateError::ExpiredContext { .. }
                | CertificateError::NotValidYet
                | CertificateError::NotValidYetContext { .. }
        )
    )
}

/// Whether a certificate is within its dates at a moment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InDates {
    Yes,
    No,
}

/// Whether `end_entity` is within its dates `now`, judged the same on every system: Windows' verifier names an unknown
/// issuer before an expiry, so its refusal alone cannot tell. webpki checks the dates before it looks for an issuer,
/// so a chain to no authority at all answers just that.
fn dates_at(end_entity: &CertificateDer<'_>, now: UnixTime, provider: &CryptoProvider) -> InDates {
    let Ok(leaf) = webpki::EndEntityCert::try_from(end_entity) else { return InDates::Yes };
    let algorithms = provider.signature_verification_algorithms.all;
    match leaf.verify_for_usage(algorithms, &[], &[], now, webpki::KeyUsage::server_auth(), None, None) {
        Err(webpki::Error::CertExpired { .. } | webpki::Error::CertNotValidYet { .. }) => InDates::No,
        Ok(_) | Err(_) => InDates::Yes,
    }
}

fn revoked(failed: &rustls::Error) -> bool {
    matches!(failed, rustls::Error::InvalidCertificate(CertificateError::Revoked))
}

/// A certificate's key as a pin: the SHA-256 of its SubjectPublicKeyInfo.
fn key_pin(certificate: &webpki::EndEntityCert<'_>) -> Option<KeyPin> {
    let digest = ring::digest::digest(&ring::digest::SHA256, certificate.subject_public_key_info().as_ref());
    digest.as_ref().try_into().ok().map(KeyPin::from_digest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair, PublicKeyData, date_time_ymd};

    fn pin_of(key: &KeyPair) -> KeyPin {
        let digest = ring::digest::digest(&ring::digest::SHA256, &key.subject_public_key_info());
        KeyPin::from_digest(digest.as_ref().try_into().expect("32 bytes"))
    }

    /// A self-signed certificate, as an authority's when `authority`, and the pin its key is.
    fn certificate(authority: bool) -> (CertificateDer<'static>, KeyPin) {
        let key = KeyPair::generate().expect("a key");
        let mut params = CertificateParams::new(vec!["localhost".to_string()]).expect("a name");
        if authority {
            params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        }
        let made = params.self_signed(&key).expect("signed");
        (made.der().clone(), pin_of(&key))
    }

    /// When a certificate is valid.
    #[derive(Clone, Copy, Debug)]
    enum Dates {
        Expired,
        NotYet,
    }

    /// A certificate for `localhost` valid only at `dates`, signed by its own key or, `by_authority`, by an authority
    /// no store holds; and the pin its key is.
    fn dated(dates: Dates, by_authority: bool) -> (CertificateDer<'static>, KeyPin) {
        let key = KeyPair::generate().expect("a key");
        let mut params = CertificateParams::new(vec!["localhost".to_string()]).expect("a name");
        (params.not_before, params.not_after) = match dates {
            Dates::Expired => (date_time_ymd(2020, 1, 1), date_time_ymd(2021, 1, 1)),
            Dates::NotYet => (date_time_ymd(2090, 1, 1), date_time_ymd(2091, 1, 1)),
        };
        let made = if by_authority {
            let mut authority = CertificateParams::new(Vec::<String>::new()).expect("no name");
            authority.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
            // rcgen names every certificate alike unless told; an authority is another name.
            authority.distinguished_name.push(DnType::CommonName, "An authority no store holds");
            let issuer = Issuer::new(authority, KeyPair::generate().expect("its key"));
            params.signed_by(&key, &issuer).expect("signed")
        } else {
            params.self_signed(&key).expect("signed")
        };
        (made.der().clone(), pin_of(&key))
    }

    /// What one server's verifier answers for `certificate` and keeps, the operating system's own verifier judging it.
    fn checked(certificate: &CertificateDer<'_>, pin: Option<KeyPin>) -> (bool, Result<Vouched, LinkError>) {
        let system = System::build().expect("the system's roots make a verifier");
        pinned_by(Arc::clone(&system.verifier), certificate, pin)
    }

    #[test]
    fn an_expired_or_not_yet_valid_self_signed_certificate_is_asked_about_by_its_key() {
        for dates in [Dates::Expired, Dates::NotYet] {
            let (certificate, pin) = dated(dates, false);
            assert_eq!(checked(&certificate, None), (false, Err(LinkError::Unvouched(pin))), "{dates:?}");
        }
    }

    #[test]
    fn a_trusted_key_is_taken_whatever_its_certificates_dates() {
        for dates in [Dates::Expired, Dates::NotYet] {
            let (certificate, pin) = dated(dates, false);
            assert_eq!(checked(&certificate, Some(pin)), (true, Ok(Vouched::Pin)), "{dates:?}");
        }
    }

    #[test]
    fn an_out_of_date_certificate_an_unknown_authority_signed_stays_untrusted() {
        for dates in [Dates::Expired, Dates::NotYet] {
            let (certificate, _) = dated(dates, true);
            assert_eq!(checked(&certificate, None), (false, Err(LinkError::Untrusted)), "{dates:?}");
        }
    }

    /// A system verifier that says `said` of every certificate.
    #[derive(Debug)]
    struct Says(Result<(), rustls::Error>);

    impl ServerCertVerifier for Says {
        fn verify_server_cert(
            &self,
            _: &CertificateDer<'_>,
            _: &[CertificateDer<'_>],
            _: &ServerName<'_>,
            _: &[u8],
            _: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            self.0.clone().map(|()| ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _: &[u8],
            _: &CertificateDer<'_>,
            _: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _: &[u8],
            _: &CertificateDer<'_>,
            _: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            rustls::crypto::ring::default_provider().signature_verification_algorithms.supported_schemes()
        }
    }

    /// What one connect's verifier answers for `certificate` and what it kept, the system saying `said`.
    fn verified(
        said: Result<(), rustls::Error>,
        certificate: &CertificateDer<'_>,
        pin: Option<KeyPin>,
    ) -> (bool, Result<Vouched, LinkError>) {
        pinned_by(Arc::new(Says(said)), certificate, pin)
    }

    /// The same, `system` judging it first.
    fn pinned_by(
        system: Arc<dyn ServerCertVerifier>,
        certificate: &CertificateDer<'_>,
        pin: Option<KeyPin>,
    ) -> (bool, Result<Vouched, LinkError>) {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verdict = Arc::new(Mutex::new(None));
        let pinned = Pinned { system, provider, pin, verdict: Arc::clone(&verdict) };
        let name = ServerName::try_from("localhost").expect("a name");
        let answered = pinned.verify_server_cert(certificate, &[], &name, &[], UnixTime::now()).is_ok();
        let kept = verdict.lock().expect("not poisoned").clone();
        (answered, kept.expect("a verdict"))
    }

    fn unknown_issuer() -> Result<(), rustls::Error> {
        Err(rustls::Error::InvalidCertificate(CertificateError::UnknownIssuer))
    }

    #[test]
    fn a_key_no_authority_vouches_for_is_refused_with_its_pin_on_first_use() {
        let (certificate, pin) = certificate(false);
        assert_eq!(verified(unknown_issuer(), &certificate, None), (false, Err(LinkError::Unvouched(pin))));
    }

    #[test]
    fn the_key_this_device_trusts_is_taken_where_no_authority_vouches_for_it() {
        let (certificate, pin) = certificate(false);
        assert_eq!(verified(unknown_issuer(), &certificate, Some(pin)), (true, Ok(Vouched::Pin)));
    }

    #[test]
    fn another_key_than_the_one_trusted_is_refused_naming_both_whatever_else_the_system_said() {
        let (certificate, presented) = certificate(false);
        let (_, trusted) = self::certificate(false);
        for refused in [CertificateError::UnknownIssuer, CertificateError::Expired, CertificateError::NotValidForName] {
            let said = Err(rustls::Error::InvalidCertificate(refused));
            assert_eq!(
                verified(said, &certificate, Some(trusted)),
                (false, Err(LinkError::KeyChanged { trusted, presented })),
            );
        }
    }

    #[test]
    fn a_certificate_the_system_vouches_for_is_taken_as_the_systems_so_a_kept_pin_can_go() {
        let (certificate, _) = certificate(false);
        let (_, kept) = self::certificate(false);
        assert_eq!(verified(Ok(()), &certificate, Some(kept)), (true, Ok(Vouched::System)));
        assert_eq!(verified(Ok(()), &certificate, None), (true, Ok(Vouched::System)));
    }

    #[test]
    fn the_trusted_key_is_judged_by_the_key_alone_but_a_revocation_stays_untrusted() {
        let (certificate, pin) = certificate(false);
        for refused in [CertificateError::Expired, CertificateError::NotValidYet, CertificateError::NotValidForName] {
            let said = Err(rustls::Error::InvalidCertificate(refused));
            assert_eq!(verified(said, &certificate, Some(pin)), (true, Ok(Vouched::Pin)));
        }
        let revoked = Err(rustls::Error::InvalidCertificate(CertificateError::Revoked));
        assert_eq!(verified(revoked, &certificate, Some(pin)), (false, Err(LinkError::Untrusted)));
    }

    #[test]
    fn with_no_key_trusted_any_other_refusal_of_the_system_stays_untrusted() {
        let (certificate, _) = certificate(false);
        for refused in [CertificateError::Revoked, CertificateError::NotValidForName, CertificateError::BadSignature] {
            let said = Err(rustls::Error::InvalidCertificate(refused));
            assert_eq!(verified(said, &certificate, None), (false, Err(LinkError::Untrusted)));
        }
    }

    #[test]
    fn an_unknown_issuer_said_of_an_out_of_date_certificate_is_untrusted_as_its_dates_say() {
        // Windows' verifier names the issuer first; the certificate's own dates decide on every system alike.
        for dates in [Dates::Expired, Dates::NotYet] {
            let (certificate, _) = dated(dates, true);
            assert_eq!(verified(unknown_issuer(), &certificate, None), (false, Err(LinkError::Untrusted)), "{dates:?}");
            let (own, pin) = dated(dates, false);
            assert_eq!(verified(unknown_issuer(), &own, None), (false, Err(LinkError::Unvouched(pin))), "{dates:?}");
        }
    }

    #[test]
    fn a_self_signed_authority_is_unvouched_by_the_systems_own_verifier() {
        let system = System::build().expect("the system's roots make a verifier");
        let name = ServerName::try_from("localhost").expect("a name");
        for authority in [false, true] {
            let (certificate, pin) = certificate(authority);
            let said = system.verifier.verify_server_cert(&certificate, &[], &name, &[], UnixTime::now());
            let said = said.as_ref().map(|_| ());
            assert_eq!(
                judge(said, &certificate, None, super::InDates::Yes),
                Err(LinkError::Unvouched(pin)),
                "an authority: {authority}"
            );
        }
    }

    #[tokio::test]
    async fn the_systems_verifier_is_made_once_and_shared_by_clones_taken_before_and_after() {
        let trust = Trust::system();
        let early = trust.clone();
        trust.handshake(LAN).await.expect("the system's roots make a verifier");
        let first = trust.shared.system.get().cloned().expect("made").expect("a verifier");
        for other in [&trust, &early, &trust.for_server(None)] {
            other.handshake(LAN).await.expect("again");
            let shared = other.shared.system.get().cloned().expect("made").expect("a verifier");
            assert!(Arc::ptr_eq(&first, &shared), "nothing is made again");
        }
    }

    const LAN: &str = "lan.example.invalid:8443";

    #[tokio::test]
    async fn a_servers_configuration_is_kept_for_its_pin_made_anew_for_another_and_dropped_when_refused() {
        let trust = Trust::system();
        let (_, pin) = certificate(false);
        let first = trust.handshake(LAN).await.expect("a configuration").config();
        let again = trust.clone().handshake(LAN).await.expect("the same").config();
        assert!(Arc::ptr_eq(&first, &again), "a later connect resumes on it");
        let other = trust.handshake("books.example.invalid").await.expect("another server's").config();
        assert!(!Arc::ptr_eq(&first, &other), "each server its own");
        let pinned = trust.for_server(Some(pin)).handshake(LAN).await.expect("pinned");
        assert!(!Arc::ptr_eq(&first, &pinned.config()), "another pin, another configuration");
        assert_eq!(pinned.refused(None), LinkError::Untrusted);
        let after = trust.for_server(Some(pin)).handshake(LAN).await.expect("made again").config();
        assert!(!Arc::ptr_eq(&pinned.config(), &after), "a refused handshake's configuration is not kept");
    }
    /// A trust whose system verifier says `said` of every certificate.
    fn trust_saying(said: Result<(), rustls::Error>) -> Trust {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let system = Arc::new(System { verifier: Arc::new(Says(said)), provider });
        let shared = Shared { system: OnceCell::new_with(Some(Ok(system))), ..Shared::default() };
        Trust { shared: Arc::new(shared), pin: None }
    }

    /// A site on a loopback port that speaks only TLS 1.2 and completes each handshake it is offered.
    async fn a_site_of_tls_1_2() -> SocketAddr {
        let key = KeyPair::generate().expect("a key");
        let made =
            CertificateParams::new(vec!["localhost".to_string()]).expect("a name").self_signed(&key).expect("signed");
        let key = rustls_pki_types::PrivateKeyDer::Pkcs8(key.serialize_der().into());
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&version::TLS12])
            .expect("a version")
            .with_no_client_auth()
            .with_single_cert(vec![made.der().clone()], key)
            .expect("a certificate");
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bound");
        let address = listener.local_addr().expect("an address");
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                tokio::spawn(async move { drop(acceptor.accept(stream).await) });
            }
        });
        address
    }

    #[tokio::test]
    async fn a_public_site_is_reached_over_tls_1_2_at_the_address_given_and_only_as_the_system_vouches() {
        let address = a_site_of_tls_1_2().await;
        let vouched = trust_saying(Ok(()));
        vouched.secure_at(vec![address], "localhost").await.expect("a site of TLS 1.2 is a site");
        let uri: Uri = format!("https://{address}/").parse().expect("an address");
        assert_eq!(vouched.secure(&uri).await.err(), Some(LinkError::OldEncryption), "a server's stays TLS 1.3");
        let unknown = trust_saying(Err(rustls::Error::InvalidCertificate(CertificateError::UnknownIssuer)));
        assert_eq!(unknown.secure_at(vec![address], "localhost").await.err(), Some(LinkError::Untrusted));
        let pinned = unknown.for_server(Some(KeyPin::from_digest([7; 32])));
        assert_eq!(pinned.secure_at(vec![address], "localhost").await.err(), Some(LinkError::Untrusted), "no pin");
        assert_eq!(vouched.secure_at(vec![address], "not a name").await.err(), Some(LinkError::NoSuchHost));
    }
}
