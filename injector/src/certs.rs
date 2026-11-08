use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rcgen::{CertificateParams, DnType, ExtendedKeyUsagePurpose, Issuer, KeyPair};
use rustls::ServerConfig;
use rustls::pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer};
use time::{Duration, OffsetDateTime};

const LIFETIME: Duration = Duration::hours(48);
const RENEW_BEFORE: Duration = Duration::hours(24);
const BACKDATE: Duration = Duration::hours(1);

pub struct Leaves {
    issuer: Issuer<'static, KeyPair>,
    issued: Mutex<HashMap<String, Leaf>>,
}

struct Leaf {
    config: Arc<ServerConfig>,
    not_after: OffsetDateTime,
}

impl Leaves {
    pub fn load(cert_pem: &str, key_pem: &str) -> Result<Leaves, String> {
        let key = KeyPair::from_pem(key_pem).map_err(|e| format!("CA key: {e}"))?;
        let issuer =
            Issuer::from_ca_cert_pem(cert_pem, key).map_err(|e| format!("CA cert: {e}"))?;
        Ok(Leaves {
            issuer,
            issued: Mutex::new(HashMap::new()),
        })
    }

    pub fn config(&self, host: &str, now: OffsetDateTime) -> Result<Arc<ServerConfig>, String> {
        let mut issued = self.issued.lock().map_err(|_| "leaf cache poisoned")?;
        if let Some(leaf) = issued.get(host)
            && leaf.not_after - now > RENEW_BEFORE
        {
            return Ok(Arc::clone(&leaf.config));
        }
        let leaf = self.issue(host, now)?;
        let config = Arc::clone(&leaf.config);
        issued.insert(host.to_string(), leaf);
        Ok(config)
    }

    fn issue(&self, host: &str, now: OffsetDateTime) -> Result<Leaf, String> {
        let key = KeyPair::generate().map_err(|e| e.to_string())?;
        let mut params =
            CertificateParams::new(vec![host.to_string()]).map_err(|e| e.to_string())?;
        params.distinguished_name.push(DnType::CommonName, host);
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.use_authority_key_identifier_extension = true;
        params.not_before = now - BACKDATE;
        params.not_after = now + LIFETIME;
        let cert = params
            .signed_by(&key, &self.issuer)
            .map_err(|e| format!("leaf of {host}: {e}"))?;
        let private = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
        let mut config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert.der().clone()], private)
            .map_err(|e| format!("leaf of {host}: {e}"))?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Leaf {
            config: Arc::new(config),
            not_after: params.not_after,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{BasicConstraints, GeneralSubtree, IsCa, KeyUsagePurpose, NameConstraints};
    use rustls::pki_types::{CertificateDer, ServerName};
    use rustls::{ClientConfig, RootCertStore};
    use tokio_rustls::{TlsAcceptor, TlsConnector};

    fn ca(permitted: &str) -> (String, String) {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params
            .distinguished_name
            .push(DnType::CommonName, "test injector CA");
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        params.name_constraints = Some(NameConstraints {
            permitted_subtrees: vec![GeneralSubtree::DnsName(permitted.to_string())],
            excluded_subtrees: vec![],
        });
        let cert = params.self_signed(&key).unwrap();
        (cert.pem(), key.serialize_pem())
    }

    async fn handshake(server: Arc<ServerConfig>, ca_pem: &str, name: &str) -> Result<(), String> {
        let mut roots = RootCertStore::empty();
        let der = rustls::pki_types::pem::PemObject::from_pem_slice(ca_pem.as_bytes())
            .map(|d: CertificateDer| d)
            .unwrap();
        roots.add(der).unwrap();
        let client = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let (a, b) = tokio::io::duplex(16 * 1024);
        let accept = tokio::spawn(async move { TlsAcceptor::from(server).accept(b).await });
        let name = ServerName::try_from(name.to_string()).unwrap();
        let connected = TlsConnector::from(Arc::new(client)).connect(name, a).await;
        let _ = accept.await;
        connected.map(|_| ()).map_err(|e| e.to_string())
    }

    #[tokio::test]
    async fn a_client_that_trusts_the_ca_accepts_the_leaf_of_its_host_only() {
        let (cert, key) = ca("github.com");
        let leaves = Leaves::load(&cert, &key).unwrap();
        let config = leaves
            .config("api.github.com", OffsetDateTime::now_utc())
            .unwrap();
        assert_eq!(
            handshake(config.clone(), &cert, "api.github.com").await,
            Ok(())
        );
        assert!(handshake(config, &cert, "github.com").await.is_err());
    }

    #[test]
    fn a_leaf_is_kept_while_fresh_and_issued_again_near_its_end() {
        let (cert, key) = ca("github.com");
        let leaves = Leaves::load(&cert, &key).unwrap();
        let now = OffsetDateTime::now_utc();
        let first = leaves.config("github.com", now).unwrap();
        let fresh = leaves
            .config("github.com", now + Duration::hours(23))
            .unwrap();
        let late = leaves
            .config("github.com", now + Duration::hours(25))
            .unwrap();
        assert!(Arc::ptr_eq(&first, &fresh));
        assert!(!Arc::ptr_eq(&first, &late));
    }

    #[test]
    fn a_broken_ca_is_an_error() {
        let (cert, _) = ca("github.com");
        assert!(Leaves::load(&cert, "no key").is_err());
        let (_, key) = ca("github.com");
        assert!(Leaves::load("no cert", &key).is_err());
    }
}
