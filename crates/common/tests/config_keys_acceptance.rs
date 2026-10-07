//! Acceptance tests for `features/merge_m6_job_safety.feature` (dead config
//! keys, F-06) -- the scenarios about `ServerConfig` parsing and the pre-start
//! validation of `tls_enabled`.
//!
//! The criterion is "the key is really read from the file", not "the field
//! exists on the struct": the tests parse YAML the way the server does.

use common::config::ServerConfig;

#[test]
fn the_configuration_keys_that_used_to_be_dead_are_read_from_the_file() {
    let yaml = r#"
tls_enabled: true
tls_cert_path: /tmp/m6-cert.pem
tls_key_path: /tmp/m6-key.pem
prometheus_enabled: true
prometheus_addr: "127.0.0.1:9191"
max_concurrent_ws_clients: 7
"#;
    let config: ServerConfig = serde_yaml::from_str(yaml).expect("config must parse");

    assert!(config.tls_enabled, "tls_enabled is read from the file");
    assert_eq!(
        config.tls_cert_path.as_deref(),
        Some(std::path::Path::new("/tmp/m6-cert.pem"))
    );
    assert_eq!(
        config.tls_key_path.as_deref(),
        Some(std::path::Path::new("/tmp/m6-key.pem"))
    );
    assert!(
        config.prometheus_enabled,
        "prometheus_enabled is read from the file"
    );
    assert_eq!(
        config.prometheus_addr, "127.0.0.1:9191",
        "prometheus_addr is read from the file"
    );
    assert_eq!(
        config.max_concurrent_ws_clients, 7,
        "max_concurrent_ws_clients is read from the file"
    );

    // Keys that are not written keep their defaults.
    let defaults = ServerConfig::default();
    assert_eq!(config.ws_max_backlog, defaults.ws_max_backlog);
    assert_eq!(config.http_addr, defaults.http_addr);
    assert_eq!(
        config.jwt_access_expiry_secs,
        defaults.jwt_access_expiry_secs
    );
    assert_eq!(config.trust_proxy_headers, defaults.trust_proxy_headers);
}

#[test]
fn enabling_tls_without_certificate_or_key_paths_is_refused_with_a_clear_error() {
    let yaml = r#"
tls_enabled: true
"#;
    let config: ServerConfig = serde_yaml::from_str(yaml).expect("config must parse");
    assert!(config.tls_cert_path.is_none() && config.tls_key_path.is_none());

    let error = config
        .validate()
        .expect_err("tls_enabled without a certificate pair must be refused");
    assert!(
        error.contains("tls_cert_path") && error.contains("tls_key_path"),
        "the error must name both missing paths, got: {error}"
    );

    // Turning TLS off makes the very same config valid.
    let mut without_tls = config;
    without_tls.tls_enabled = false;
    assert!(
        without_tls.validate().is_ok(),
        "tls_enabled=false passes the same validation"
    );
}
