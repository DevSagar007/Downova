//! Test PKI for the HTTPS fixtures: a throwaway CA and leaf certificates generated per test run.
//! Nothing here touches the OS certificate store; a test CA reaches a backend only through that
//! backend's explicit extra-roots option (aria2 `--ca-certificate`, dm-backend-http `extra_roots`).

use std::sync::Arc;

use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, DnType, IsCa, KeyPair};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio_rustls::TlsAcceptor;

/// A server identity (certificate chain + key).
#[derive(Clone)]
pub struct Identity {
    chain: Vec<CertificateDer<'static>>,
    key_der: Vec<u8>,
}

impl Identity {
    pub fn acceptor(&self) -> TlsAcceptor {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                self.chain.clone(),
                PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone())),
            )
            .unwrap();
        TlsAcceptor::from(Arc::new(config))
    }
}

/// A test certificate authority.
pub struct TestCa {
    issuer: CertifiedIssuer<'static, KeyPair>,
}

impl TestCa {
    pub fn new(name: &str) -> Self {
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.distinguished_name.push(DnType::CommonName, name);
        let issuer = CertifiedIssuer::self_signed(params, KeyPair::generate().unwrap()).unwrap();
        Self { issuer }
    }

    /// PEM of the CA certificate (for `--ca-certificate`).
    pub fn pem(&self) -> String {
        self.issuer.pem()
    }

    /// DER of the CA certificate (for a backend's explicit extra-roots option).
    pub fn der(&self) -> CertificateDer<'static> {
        self.issuer.der().clone()
    }

    /// Leaf certificate for the given DNS names / IPs, signed by this CA.
    pub fn leaf(&self, names: &[&str]) -> Identity {
        let key = KeyPair::generate().unwrap();
        let params =
            CertificateParams::new(names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>())
                .unwrap();
        let cert = params.signed_by(&key, &self.issuer).unwrap();
        Identity {
            chain: vec![cert.der().clone(), self.issuer.der().clone()],
            key_der: key.serialize_der(),
        }
    }
}

/// A self-signed leaf that no CA vouches for.
pub fn self_signed(names: &[&str]) -> Identity {
    let key = KeyPair::generate().unwrap();
    let params =
        CertificateParams::new(names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>()).unwrap();
    let cert = params.self_signed(&key).unwrap();
    Identity {
        chain: vec![cert.der().clone()],
        key_der: key.serialize_der(),
    }
}
