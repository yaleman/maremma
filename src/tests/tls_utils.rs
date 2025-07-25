//! Code for generating test certs, and doing crypto-crimes.

use chrono::TimeDelta;
use openssl::ec::{EcGroup, EcKey};
use openssl::error::ErrorStack;
use openssl::nid::Nid;
use openssl::pkey::{PKeyRef, Private};
use openssl::rsa::Rsa;
use openssl::x509::{
    extension::{
        AuthorityKeyIdentifier, BasicConstraints, ExtendedKeyUsage, KeyUsage,
        SubjectAlternativeName, SubjectKeyIdentifier,
    },
    X509NameBuilder, X509ReqBuilder, X509,
};
use openssl::{asn1, bn, hash, pkey};
use tempfile::NamedTempFile;
use tracing::*;

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

const CA_VALID_DAYS: u32 = 30;

// Basing minimums off https://www.keylength.com setting "year" to 2030 - tested as at 2023-09-25
//
// |Method           |Date     |Symmetric| FM      |DL Key| DL Group|Elliptic Curve|Hash|
// |   ---           |   ---   |   ---   |   ---   | ---  |   ---   |  ---         | ---|
// |Lenstra / Verheul|2030     |  93     |2493^2016|165   | 2493    |  176         | 186|
// |Lenstra Updated  |2030     |  88     |1698^2063|176   | 1698    |  176         | 176|
// |ECRYPT           |2029-2068|  256    |15360    |512   | 15360   |  512         | 512|
// |NIST             |2019-2030|  112    |2048     |224   | 2048    |  224         | 224|
// |ANSSI            |> 2030   |  128    |3072     |200   | 3072    |  256         | 256|
// |NSA              |-        |  256    |3072     |-     | -       |  384         | 384|
// |RFC3766          |-        |  -      |   -     | -    |   -     |   -          |  - |
// |BSI              |-        |  -      |   -     | -    |   -     |   -          |  - |
// DL - Discrete Logarithm
// FM - Factoring Modulus

const RSA_MIN_KEY_SIZE_BITS: u64 = 2048;
const EC_MIN_KEY_SIZE_BITS: u64 = 224;

/// returns a signing function that meets a sensible minimum
fn get_signing_func() -> hash::MessageDigest {
    hash::MessageDigest::sha256()
}

/// Ensure we're enforcing safe minimums for TLS keys
pub fn check_privkey_minimums(privkey: &PKeyRef<Private>) -> Result<(), String> {
    if let Ok(key) = privkey.rsa() {
        if key.size() < (RSA_MIN_KEY_SIZE_BITS / 8) as u32 {
            return Err(format!(
                "TLS RSA key is less than {RSA_MIN_KEY_SIZE_BITS} bits!"
            ));
        } else {
            debug!(
                "The RSA private key size is: {} bits, that's OK!",
                key.size() * 8
            );
            return Ok(());
        }
    }

    match privkey.ec_key() {
        Ok(key) => {
            // allowing this to panic because ... it's an i32 and hopefully we don't have negative bit lengths?
            #[allow(clippy::panic)]
            let key_bits: u64 = key.private_key().num_bits().try_into().unwrap_or_else(|_| {
                panic!(
                    "Failed to convert EC bitlength {} to u64",
                    key.private_key().num_bits()
                )
            });

            if key_bits < EC_MIN_KEY_SIZE_BITS {
                Err(format!(
                    "TLS EC key is less than {EC_MIN_KEY_SIZE_BITS} bits! Got: {key_bits}"
                ))
            } else {
                debug!("The EC private key size is: {} bits, that's OK!", key_bits);
                Ok(())
            }
        }
        Err(_) => {
            error!("TLS key is not RSA or EC, cannot check minimums!");
            Ok(())
        }
    }
}

fn get_ec_group() -> Result<EcGroup, ErrorStack> {
    EcGroup::from_curve_name(Nid::X9_62_PRIME256V1)
}

#[derive(Debug)]
pub(crate) struct CaHandle {
    key: pkey::PKey<pkey::Private>,
    cert: X509,
}

pub(crate) fn write_ca(
    key_ar: impl AsRef<Path>,
    cert_ar: impl AsRef<Path>,
    handle: &CaHandle,
) -> Result<(), ()> {
    let key_path: &Path = key_ar.as_ref();
    let cert_path: &Path = cert_ar.as_ref();

    let key_pem = handle.key.private_key_to_pem_pkcs8().map_err(|e| {
        error!(err = ?e, "Failed to convert key to PEM");
    })?;

    let cert_pem = handle.cert.to_pem().map_err(|e| {
        error!(err = ?e, "Failed to convert cert to PEM");
    })?;

    File::create(key_path)
        .and_then(|mut file| file.write_all(&key_pem))
        .map_err(|e| {
            error!(err = ?e, "Failed to create {:?}", key_path);
        })?;

    File::create(cert_path)
        .and_then(|mut file| file.write_all(&cert_pem))
        .map_err(|e| {
            error!(err = ?e, "Failed to create {:?}", cert_path);
        })
}

#[derive(Debug)]
pub enum KeyType {
    #[allow(dead_code)]
    Rsa,
    Ec,
}
impl Default for KeyType {
    fn default() -> Self {
        Self::Ec
    }
}

#[derive(Debug)]
pub struct CAConfig {
    pub key_type: KeyType,
    pub key_bits: u64,
    pub skip_enforce_minimums: bool,
}

impl Default for CAConfig {
    fn default() -> Self {
        #[allow(clippy::expect_used)]
        Self::new(KeyType::Ec, 256, false)
            .expect("Somehow the defaults failed to pass validation while building a CA Config?")
    }
}

impl CAConfig {
    fn new(key_type: KeyType, key_bits: u64, skip_enforce_minimums: bool) -> Result<Self, String> {
        let res = Self {
            key_type,
            key_bits,
            skip_enforce_minimums,
        };
        if !skip_enforce_minimums {
            res.enforce_minimums()?;
        };
        Ok(res)
    }

    /// Make sure we're meeting the minimum spec for key length etc
    fn enforce_minimums(&self) -> Result<(), String> {
        match self.key_type {
            KeyType::Rsa => {
                trace!(
                    "Generating CA Config for RSA Key with {} bits",
                    self.key_bits
                );
                if self.key_bits < RSA_MIN_KEY_SIZE_BITS {
                    return Err(format!(
                        "RSA key size must be at least {RSA_MIN_KEY_SIZE_BITS} bits"
                    ));
                }
            }
            KeyType::Ec => {
                trace!("Generating CA Config for EcKey with {} bits", self.key_bits);
                if self.key_bits < EC_MIN_KEY_SIZE_BITS {
                    return Err(format!(
                        "EC key size must be at least {EC_MIN_KEY_SIZE_BITS} bits"
                    ));
                }
            }
        };
        Ok(())
    }
}

pub(crate) fn gen_private_key(
    key_type: &KeyType,
    key_bits: Option<u64>,
) -> Result<pkey::PKey<pkey::Private>, ErrorStack> {
    match key_type {
        KeyType::Rsa => {
            let key_bits = key_bits.unwrap_or(RSA_MIN_KEY_SIZE_BITS);
            let rsa = Rsa::generate(key_bits as u32)?;
            pkey::PKey::from_rsa(rsa)
        }
        KeyType::Ec => {
            let ecgroup = get_ec_group()?;
            let eckey = EcKey::generate(&ecgroup)?;
            pkey::PKey::from_ec_key(eckey)
        }
    }
}

/// build up a CA certificate and key.
pub(crate) fn build_ca(
    ca_config: Option<CAConfig>,
    signing_function: Option<hash::MessageDigest>,
) -> Result<CaHandle, ErrorStack> {
    let ca_config = ca_config.unwrap_or_default();

    let ca_key = gen_private_key(&ca_config.key_type, Some(ca_config.key_bits))?;

    if !ca_config.skip_enforce_minimums {
        check_privkey_minimums(&ca_key).map_err(|err| {
            error!("failed to build_ca due to privkey minimums {}", err);
            ErrorStack::get() // this probably should be a real errorstack but... how?
        })?;
    }
    let mut x509_name = X509NameBuilder::new()?;

    x509_name.append_entry_by_text("C", "AU")?;
    x509_name.append_entry_by_text("ST", "QLD")?;
    x509_name.append_entry_by_text("O", "Test Org")?;
    x509_name.append_entry_by_text("CN", "Test CA")?;
    x509_name.append_entry_by_text("OU", "Development and Evaluation - NOT FOR PRODUCTION")?;
    let x509_name = x509_name.build();

    let mut cert_builder = X509::builder()?;
    // Yes, 2 actually means 3 here ...
    cert_builder.set_version(2)?;

    let serial_number = bn::BigNum::from_u32(1).and_then(|serial| serial.to_asn1_integer())?;

    cert_builder.set_serial_number(&serial_number)?;
    cert_builder.set_subject_name(&x509_name)?;
    cert_builder.set_issuer_name(&x509_name)?;

    let not_before = asn1::Asn1Time::days_from_now(0)?;
    cert_builder.set_not_before(&not_before)?;
    let not_after = asn1::Asn1Time::days_from_now(CA_VALID_DAYS)?;
    cert_builder.set_not_after(&not_after)?;

    cert_builder.append_extension(BasicConstraints::new().critical().ca().pathlen(0).build()?)?;
    cert_builder.append_extension(
        KeyUsage::new()
            .critical()
            .key_cert_sign()
            .crl_sign()
            .build()?,
    )?;

    let subject_key_identifier =
        SubjectKeyIdentifier::new().build(&cert_builder.x509v3_context(None, None))?;
    cert_builder.append_extension(subject_key_identifier)?;

    cert_builder.set_pubkey(&ca_key)?;

    if let Some(signing_function) = signing_function {
        cert_builder.sign(&ca_key, signing_function)?;
    } else {
        cert_builder.sign(&ca_key, get_signing_func())?;
    }

    cert_builder.sign(&ca_key, get_signing_func())?;
    let ca_cert = cert_builder.build();

    Ok(CaHandle {
        key: ca_key,
        cert: ca_cert,
    })
}

pub(crate) fn load_ca(
    ca_key_ar: impl AsRef<Path>,
    ca_cert_ar: impl AsRef<Path>,
) -> Result<CaHandle, ()> {
    let ca_key_path: &Path = ca_key_ar.as_ref();
    let ca_cert_path: &Path = ca_cert_ar.as_ref();

    let mut ca_key_pem = vec![];
    File::open(ca_key_path)
        .and_then(|mut file| file.read_to_end(&mut ca_key_pem))
        .map_err(|e| {
            error!(err = ?e, "Failed to read {:?}", ca_key_path);
        })?;

    let mut ca_cert_pem = vec![];
    File::open(ca_cert_path)
        .and_then(|mut file| file.read_to_end(&mut ca_cert_pem))
        .map_err(|e| {
            error!(err = ?e, "Failed to read {:?}", ca_cert_path);
        })?;

    let ca_key = pkey::PKey::private_key_from_pem(&ca_key_pem).map_err(|e| {
        error!(err = ?e, "Failed to convert PEM to key");
    })?;

    check_privkey_minimums(&ca_key).map_err(|err| {
        error!("{}", err);
    })?;

    let ca_cert = X509::from_pem(&ca_cert_pem).map_err(|e| {
        error!(err = ?e, "Failed to convert PEM to cert");
    })?;

    Ok(CaHandle {
        key: ca_key,
        cert: ca_cert,
    })
}

#[allow(dead_code)]
pub(crate) struct CertHandle {
    pub key: pkey::PKey<pkey::Private>,
    pub cert: X509,
    pub chain: Vec<X509>,
}

pub(crate) fn build_cert(
    domain_name: Option<&str>,
    ca_handle: &CaHandle,
    key_type: Option<KeyType>,
    key_bits: Option<u64>,
    issue_time: i64,
    expiry_time: i64,
) -> Result<CertHandle, ErrorStack> {
    let key_type = key_type.unwrap_or_default();
    let int_key = gen_private_key(&key_type, key_bits)?;

    let mut req_builder = X509ReqBuilder::new()?;
    req_builder.set_pubkey(&int_key)?;

    if let Some(domain_name) = domain_name {
        let mut x509_name = X509NameBuilder::new()?;
        x509_name.append_entry_by_text("C", "AU")?;
        x509_name.append_entry_by_text("ST", "QLD")?;
        x509_name.append_entry_by_text("O", "Test Organisation")?;
        x509_name.append_entry_by_text("CN", domain_name)?;
        // Requirement of packed attestation.
        x509_name.append_entry_by_text("OU", "Development and Evaluation - NOT FOR PRODUCTION")?;
        let x509_name = x509_name.build();

        req_builder.set_subject_name(&x509_name)?;
    }

    req_builder.sign(&int_key, get_signing_func())?;
    let req = req_builder.build();
    // ==

    let mut cert_builder = X509::builder()?;
    // Yes, 2 actually means 3 here ...
    cert_builder.set_version(2)?;
    let serial_number = bn::BigNum::from_u32(2).and_then(|serial| serial.to_asn1_integer())?;

    cert_builder.set_pubkey(&int_key)?;

    cert_builder.set_serial_number(&serial_number)?;
    cert_builder.set_subject_name(req.subject_name())?;
    cert_builder.set_issuer_name(ca_handle.cert.subject_name())?;

    let not_before = asn1::Asn1Time::from_unix(issue_time)?;
    cert_builder.set_not_before(&not_before)?;
    let not_after = asn1::Asn1Time::from_unix(expiry_time)?;
    cert_builder.set_not_after(&not_after)?;

    cert_builder.append_extension(BasicConstraints::new().build()?)?;

    cert_builder.append_extension(
        KeyUsage::new()
            .critical()
            .digital_signature()
            .key_encipherment()
            .build()?,
    )?;

    cert_builder.append_extension(
        ExtendedKeyUsage::new()
            // .critical()
            .server_auth()
            .build()?,
    )?;

    let subject_key_identifier = SubjectKeyIdentifier::new()
        .build(&cert_builder.x509v3_context(Some(&ca_handle.cert), None))?;
    cert_builder.append_extension(subject_key_identifier)?;

    let auth_key_identifier = AuthorityKeyIdentifier::new()
        .keyid(false)
        .issuer(false)
        .build(&cert_builder.x509v3_context(Some(&ca_handle.cert), None))?;
    cert_builder.append_extension(auth_key_identifier)?;

    if let Some(domain_name) = domain_name {
        let subject_alt_name = SubjectAlternativeName::new()
            .dns(domain_name)
            .build(&cert_builder.x509v3_context(Some(&ca_handle.cert), None))?;
        cert_builder.append_extension(subject_alt_name)?;
    }

    cert_builder.sign(&ca_handle.key, get_signing_func())?;
    let int_cert = cert_builder.build();

    Ok(CertHandle {
        key: int_key,
        cert: int_cert,
        chain: vec![ca_handle.cert.clone()],
    })
}

#[test]
// might as well test my logic
fn test_enforced_minimums() {
    let good_ca_configs = vec![
        // test rsa 4096 (ok)
        (KeyType::Rsa, 4096, false),
        // test rsa 2048 (ok)
        (KeyType::Rsa, 2048, false),
        // test ec 256 (ok)
        (KeyType::Ec, 256, false),
    ];
    good_ca_configs.into_iter().for_each(|config| {
        dbg!(&config);
        assert!(CAConfig::new(config.0, config.1, config.2).is_ok());
    });
    let bad_ca_configs = vec![
        // test rsa 1024 (no)
        (KeyType::Rsa, 1024, false),
        // test ec 128 (no)
        (KeyType::Ec, 128, false),
    ];
    bad_ca_configs.into_iter().for_each(|config| {
        dbg!(&config);
        assert!(CAConfig::new(config.0, config.1, config.2).is_err());
    });
}

#[test]
fn test_ca_loader() {
    let ca_key_tempfile = tempfile::NamedTempFile::new().expect("Failed to generate tempfile");
    let ca_cert_tempfile = tempfile::NamedTempFile::new().expect("Failed to generate tempfile");
    // let's test the defaults first

    let ca_config = CAConfig::default();
    if let Ok(ca) = build_ca(Some(ca_config), None) {
        write_ca(ca_key_tempfile.path(), ca_cert_tempfile.path(), &ca)
            .expect("Failed to write the CA");
        assert!(load_ca(ca_key_tempfile.path(), ca_cert_tempfile.path()).is_ok());
    };

    let good_ca_configs = vec![
        // test rsa 4096 (ok)
        (KeyType::Rsa, 4096, false),
        // test rsa 2048 (ok)
        (KeyType::Rsa, 2048, false),
        // test ec 256 (ok)
        (KeyType::Ec, 256, false),
    ];
    good_ca_configs.into_iter().for_each(|config| {
        println!("testing good config {config:?}");
        let ca_config = CAConfig::new(config.0, config.1, config.2).expect("Failed bo build CA");
        let ca = build_ca(Some(ca_config), None).expect("Failed bo build CA");
        write_ca(ca_key_tempfile.path(), ca_cert_tempfile.path(), &ca).expect("Failed to write CA");
        let ca_result = load_ca(ca_key_tempfile.path(), ca_cert_tempfile.path());
        println!("result: {ca_result:?}");
        assert!(ca_result.is_ok());
    });
    let bad_ca_configs = vec![
        // test rsa 1024 (bad)
        (KeyType::Rsa, 1024, true),
    ];
    bad_ca_configs.into_iter().for_each(|config| {
        println!(
            "\ntesting bad config keytype: {:?} key size: {}, skip_enforce_minimums: {}",
            config.0, config.1, config.2
        );
        let ca_config =
            CAConfig::new(config.0, config.1, config.2).expect("Failed to build CA config");
        let ca = build_ca(Some(ca_config), None).expect("Failed to build CA");
        write_ca(ca_key_tempfile.path(), ca_cert_tempfile.path(), &ca).expect("Failed to write CA");
        let ca_result = load_ca(ca_key_tempfile.path(), ca_cert_tempfile.path());
        println!("result: {ca_result:?}");
        assert!(ca_result.is_err());
    });
}

pub struct TestCertificateBuilder {
    pub issue_time: i64,
    pub expiry_time: i64,
    pub hostname: String,
    pub use_sha1_intermediate: bool,
    pub skip_cert_name: bool,
}

impl TestCertificateBuilder {
    pub fn new() -> Self {
        Self {
            issue_time: chrono::Utc::now().timestamp() - TimeDelta::days(30).num_seconds(),
            expiry_time: chrono::Utc::now().timestamp() + TimeDelta::days(30).num_seconds(),
            hostname: "maremma_test".to_string(),
            use_sha1_intermediate: false,
            skip_cert_name: false,
        }
    }

    pub fn with_sha1_intermediate(self) -> Self {
        Self {
            use_sha1_intermediate: true,
            ..self
        }
    }

    pub fn with_name(self, name: &str) -> Self {
        Self {
            hostname: name.to_string(),
            ..self
        }
    }

    pub fn with_expiry(self, expiry_time: i64) -> Self {
        Self {
            expiry_time,
            ..self
        }
    }

    pub fn with_issue_time(self, issue_time: i64) -> Self {
        Self { issue_time, ..self }
    }

    pub fn build(self) -> TestCertificates {
        TestCertificates::new(
            &self.hostname,
            self.issue_time,
            self.expiry_time,
            self.use_sha1_intermediate,
            self.skip_cert_name,
        )
    }

    pub fn without_cert_name(self) -> Self {
        Self {
            skip_cert_name: true,
            ..self
        }
    }
}

pub(crate) struct TestCertificates {
    pub cert_file: NamedTempFile,
    pub key_file: NamedTempFile,
    pub ca_file: NamedTempFile,
}

impl TestCertificates {
    pub fn new(
        hostname: &str,
        issue_time: i64,
        expiry_time: i64,
        use_sha1_intermediate: bool,
        skip_cert_name: bool,
    ) -> Self {
        let mut cert_file = NamedTempFile::new().expect("Failed to create cert temp file");
        let mut key_file = NamedTempFile::new().expect("Failed to create key temp file");
        let mut ca_file = NamedTempFile::new().expect("Failed to create CA temp file");

        let ca_config = crate::tests::tls_utils::CAConfig::default();

        let signing_function = if use_sha1_intermediate {
            hash::MessageDigest::sha1()
        } else {
            hash::MessageDigest::sha256()
        };
        let ca_handle = crate::tests::tls_utils::build_ca(Some(ca_config), Some(signing_function))
            .expect("Failed to build CA");

        let hostname = match skip_cert_name {
            true => None,
            false => Some(hostname),
        };

        let cert = crate::tests::tls_utils::build_cert(
            hostname,
            &ca_handle,
            None,
            None,
            issue_time,
            expiry_time,
        )
        .expect("Failed to generate TLS Certificate");

        ca_file
            .write_all(&ca_handle.cert.to_pem().expect("Failed to get CA as pem"))
            .expect("Failed to write CA cert to file");

        cert_file
            .write_all(&cert.cert.to_pem().expect("Failed to get cert pem"))
            .expect("Failed to write cert to file");

        key_file
            .write_all(
                &cert
                    .key
                    .private_key_to_pem_pkcs8()
                    .expect("Failed to get key as PEM"),
            )
            .expect("Failed to write key to file");
        Self {
            cert_file,
            key_file,
            ca_file,
        }
    }
}

#[test]
fn test_build_cert() {
    let ca_config = CAConfig::default();

    let ca_handle = build_ca(Some(ca_config), None).expect("Failed to build CA");

    let cert = build_cert(
        Some("test.example.com"),
        &ca_handle,
        None,
        None,
        chrono::Utc::now().timestamp() - 86400,
        chrono::Utc::now().timestamp() - 3600,
    );

    assert!(cert.is_ok());
}

#[test]
fn test_build_nameless_cert() {
    let ca_config = CAConfig::default();

    let ca_handle = build_ca(Some(ca_config), None).expect("Failed to build CA");

    let cert = build_cert(
        None,
        &ca_handle,
        None,
        None,
        chrono::Utc::now().timestamp() - 86400,
        chrono::Utc::now().timestamp() - 3600,
    );

    assert!(cert.is_ok());
}
