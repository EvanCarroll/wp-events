//! TLS trust-root selection.
//!
//! Two ready-made modes are offered, each behind a cargo feature (both on by default):
//!
//! * [`TlsRoots::System`] (`system-roots`): the operating system's trust store via
//!   `rustls-platform-verifier`. Respects corporate CAs and OS revocation; needs the
//!   host to have a CA bundle (e.g. `ca-certificates` in containers).
//! * [`TlsRoots::Bundled`] (`bundled-roots`): Mozilla's root store compiled into the binary
//!   (`webpki-roots`). Works in `FROM scratch` containers; updated only when you rebuild.
//!
//! [`TlsRoots::Custom`] (always available) trusts only the PEM/DER roots you supply, and
//! [`crate::ClientBuilder::http_client`] lets you hand over a fully custom `reqwest::Client`.

use std::sync::Arc;

use rustls::ClientConfig;
use rustls::pki_types::CertificateDer;

use crate::error::{Error, Result};

/// Which certificate authorities to trust.
#[derive(Debug, Clone)]
pub enum TlsRoots {
    /// The operating system trust store (requires the `system-roots` feature).
    System,
    /// Mozilla roots baked into the binary (requires the `bundled-roots` feature).
    Bundled,
    /// Only these DER-encoded root certificates.
    Custom(Vec<CertificateDer<'static>>),
}

/// `System` if the `system-roots` feature is on, else `Bundled` if `bundled-roots` is on, else
/// an empty `Custom` set (building a client then fails until you supply roots).
impl Default for TlsRoots {
    fn default() -> Self {
        #[cfg(feature = "system-roots")]
        return TlsRoots::System;
        #[cfg(all(feature = "bundled-roots", not(feature = "system-roots")))]
        return TlsRoots::Bundled;
        #[cfg(not(any(feature = "system-roots", feature = "bundled-roots")))]
        return TlsRoots::Custom(Vec::new());
    }
}

impl TlsRoots {
    /// Parse one or more PEM certificates into a [`TlsRoots::Custom`].
    pub fn custom_pem(pem: &[u8]) -> Result<Self> {
        use rustls::pki_types::pem::PemObject;
        let certs = CertificateDer::pem_slice_iter(pem)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::Config(format!("invalid PEM certificate: {e}")))?;
        if certs.is_empty() {
            return Err(Error::Config("no certificates found in PEM input".into()));
        }
        Ok(TlsRoots::Custom(certs))
    }

    /// Build the rustls configuration for this choice.
    pub(crate) fn client_config(&self) -> Result<ClientConfig> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let builder =
            ClientConfig::builder_with_provider(provider).with_safe_default_protocol_versions()?;
        let mut config = match self {
            TlsRoots::System => {
                #[cfg(feature = "system-roots")]
                {
                    use rustls_platform_verifier::BuilderVerifierExt;
                    builder.with_platform_verifier()?.with_no_client_auth()
                }
                #[cfg(not(feature = "system-roots"))]
                {
                    let _ = builder;
                    return Err(Error::TlsRootsUnavailable("System"));
                }
            }
            TlsRoots::Bundled => {
                #[cfg(feature = "bundled-roots")]
                {
                    let roots = rustls::RootCertStore {
                        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
                    };
                    builder.with_root_certificates(roots).with_no_client_auth()
                }
                #[cfg(not(feature = "bundled-roots"))]
                {
                    let _ = builder;
                    return Err(Error::TlsRootsUnavailable("Bundled"));
                }
            }
            TlsRoots::Custom(certs) => {
                if certs.is_empty() {
                    return Err(Error::Config(
                        "no TLS roots: enable the `system-roots` or `bundled-roots` feature, or supply TlsRoots::Custom certificates".into(),
                    ));
                }
                let mut roots = rustls::RootCertStore::empty();
                for cert in certs {
                    roots.add(cert.clone())?;
                }
                builder.with_root_certificates(roots).with_no_client_auth()
            }
        };
        // reqwest does not set ALPN on preconfigured rustls configs.
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        Ok(config)
    }
}
