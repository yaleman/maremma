use std::io::Write;
use tempfile::NamedTempFile;
use testcontainers::core::{ContainerPort, Mount};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

use crate::tests::tls_utils::{TestCertificateBuilder, TestCertificates};

const TEST_CONTAINER_NGINX_CERT_PATH: &str = "/data/cert.pem";
const TEST_CONTAINER_NGINX_KEY_PATH: &str = "/data/key.pem";

fn generate_nginx_config() -> String {
    let config_string = r#"
server {
    listen 443 ssl;
    server_name test_maremma_host;

    ssl_certificate #SSL_CERT_PATH#;
    ssl_certificate_key  #SSL_KEY_PATH#;
    ssl_protocols       TLSv1 TLSv1.1 TLSv1.2 TLSv1.3;

    location / {
        proxy_pass http://localhost;
    }
}"#;

    config_string
        .replace("#SSL_CERT_PATH#", TEST_CONTAINER_NGINX_CERT_PATH)
        .replace("#SSL_KEY_PATH#", TEST_CONTAINER_NGINX_KEY_PATH)
}

fn get_nginx_config_file() -> NamedTempFile {
    let nginx_config = generate_nginx_config();
    let mut config_file = tempfile::NamedTempFile::new().expect("Failed to create temp file");
    config_file
        .write_all(nginx_config.as_bytes())
        .expect("Failed to write to temp file");
    config_file
}

async fn handle_err_or_shutdown_container<T>(
    container: &testcontainers::ContainerAsync<testcontainers::GenericImage>,
    input: Result<T, testcontainers::TestcontainersError>,
) -> T {
    match input {
        Ok(val) => val,
        Err(e) => {
            container.stop().await.expect("Failed to stop container");
            panic!("Failed to do something! {:?}", e);
        }
    }
}

pub struct TestContainer {
    pub container: ContainerAsync<GenericImage>,
    pub tls_port: u16,
}

impl TestContainer {
    /// Start up an NGINX container with a TLS config
    pub async fn new(test_certs: TestCertificates, name: &str) -> Self {
        let nginx_config = get_nginx_config_file();

        let container = GenericImage::new("nginx", "latest")
            .with_exposed_port(ContainerPort::Tcp(443))
            .with_wait_for(testcontainers::core::WaitFor::message_on_stderr(
                "start worker process",
            ))
            .with_container_name(name)
            .with_mount(Mount::bind_mount(
                test_certs.cert_file.path().display().to_string(),
                TEST_CONTAINER_NGINX_CERT_PATH,
            ))
            .with_mount(Mount::bind_mount(
                test_certs.key_file.path().display().to_string(),
                TEST_CONTAINER_NGINX_KEY_PATH,
            ))
            .with_mount(Mount::bind_mount(
                nginx_config.path().display().to_string(),
                "/etc/nginx/conf.d/tls.conf",
            ))
            .start()
            .await
            .expect("Failed to start container!");
        let ports = handle_err_or_shutdown_container(&container, container.ports().await).await;
        let tls_port = match ports.map_to_host_port_ipv4(443) {
            Some(port) => port,
            None => {
                container.stop().await.expect("Failed to stop container");
                panic!("Failed to get port from container");
            }
        };
        Self {
            container,
            tls_port,
        }
    }
}

#[tokio::test]
async fn test_basic_testcontainer() {
    use crate::prelude::*;

    let (_db, _config) = test_setup().await.expect("Failed to set up test");

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
        .expect("failed to build reqwest client");

    let container = TestContainer::new(
        TestCertificateBuilder::new().build(),
        "test_basic_testcontainer",
    )
    .await;

    debug!("TLS PORT: {}", container.tls_port);

    let res = match client
        .get(&format!("https://localhost:{}", container.tls_port))
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => {
            container
                .container
                .stop()
                .await
                .expect("Failed to stop container");
            panic!("Failed to get response from container: {:?}", e);
        }
    };

    debug!("Response: {:?}", res);
}
