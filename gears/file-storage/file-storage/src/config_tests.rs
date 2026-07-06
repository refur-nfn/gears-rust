use super::*;

#[test]
fn default_max_url_ttl_is_seven_days() {
    let cfg = FileStorageConfig::default();
    assert_eq!(cfg.max_url_ttl_secs, 7 * 24 * 60 * 60);
}

#[test]
fn default_url_ttl_is_short_and_within_ceiling() {
    let cfg = FileStorageConfig::default();
    assert_eq!(cfg.default_url_ttl_secs, 15 * 60);
    assert!(
        cfg.default_url_ttl_secs <= cfg.max_url_ttl_secs,
        "default issuance TTL must not exceed the hard ceiling"
    );
}

#[test]
fn default_url_ttl_can_be_overridden() {
    let cfg: FileStorageConfig = serde_json::from_str(r#"{"default_url_ttl_secs": 300}"#).unwrap();
    assert_eq!(cfg.default_url_ttl_secs, 300);
}

#[test]
fn serde_default_applies_when_field_absent() {
    let cfg: FileStorageConfig = serde_json::from_str("{}").unwrap();
    assert_eq!(
        cfg.max_url_ttl_secs,
        FileStorageConfig::default().max_url_ttl_secs,
        "serde(default) must fall back to the Default impl"
    );
}

#[test]
fn max_url_ttl_can_be_overridden() {
    let cfg: FileStorageConfig = serde_json::from_str(r#"{"max_url_ttl_secs": 3600}"#).unwrap();
    assert_eq!(cfg.max_url_ttl_secs, 3600);
}

#[test]
fn rejects_unknown_fields() {
    // deny_unknown_fields guards against silently-ignored config typos.
    let json = r#"{"max_url_ttl_secs": 60, "unexpected": true}"#;
    assert!(
        serde_json::from_str::<FileStorageConfig>(json).is_err(),
        "unknown keys must be rejected"
    );
}

#[test]
fn validate_rejects_zero_sweep_interval_when_sweep_enabled() {
    // A zero interval with the sweep on would spin the background loop tight.
    let cfg = FileStorageConfig {
        // Isolate this test to the sweep-interval check, not the (unrelated)
        // signing-key-seed guard added later.
        require_signing_key_seed: false,
        sweep_interval_secs: 0,
        enable_background_sweep: true,
        ..FileStorageConfig::default()
    };
    assert!(
        cfg.validate().is_err(),
        "sweep_interval_secs == 0 must be rejected when the sweep is enabled"
    );
}

#[test]
fn validate_accepts_positive_sweep_interval_when_sweep_enabled() {
    let cfg = FileStorageConfig {
        require_signing_key_seed: false,
        sweep_interval_secs: 60,
        enable_background_sweep: true,
        ..FileStorageConfig::default()
    };
    assert!(
        cfg.validate().is_ok(),
        "a positive sweep interval must pass validation"
    );
}

#[test]
fn validate_ignores_zero_sweep_interval_when_sweep_disabled() {
    // With the sweep off the interval is unused, so it need not be constrained.
    let cfg = FileStorageConfig {
        require_signing_key_seed: false,
        sweep_interval_secs: 0,
        enable_background_sweep: false,
        ..FileStorageConfig::default()
    };
    assert!(cfg.validate().is_ok());
}

#[test]
fn validate_rejects_missing_signing_key_seed_when_required_flag_set() {
    let cfg = FileStorageConfig {
        signing_key_seed: None,
        require_signing_key_seed: true,
        ..FileStorageConfig::default()
    };
    assert!(
        cfg.validate().is_err(),
        "a missing signing_key_seed must be rejected when require_signing_key_seed is true"
    );
}

#[test]
fn validate_allows_missing_signing_key_seed_when_required_flag_unset() {
    let cfg = FileStorageConfig {
        signing_key_seed: None,
        require_signing_key_seed: false,
        ..FileStorageConfig::default()
    };
    assert!(
        cfg.validate().is_ok(),
        "a missing signing_key_seed must be allowed when require_signing_key_seed is false"
    );
}

#[test]
fn validate_allows_present_signing_key_seed_when_required_flag_set() {
    let cfg = FileStorageConfig {
        signing_key_seed: Some("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned()),
        require_signing_key_seed: true,
        ..FileStorageConfig::default()
    };
    assert!(
        cfg.validate().is_ok(),
        "a present signing_key_seed must pass validation even when required"
    );
}

#[test]
fn default_require_signing_key_seed_is_true() {
    assert!(
        FileStorageConfig::default().require_signing_key_seed,
        "require_signing_key_seed must default to true (secure-by-default)"
    );
}

#[test]
fn default_enable_background_sweep_is_true() {
    assert!(
        FileStorageConfig::default().enable_background_sweep,
        "enable_background_sweep must default to true; deployments that need \
         deterministic behavior (tests, some e2e profiles) must opt out explicitly"
    );
}

#[test]
fn serde_round_trip_preserves_value() {
    let original = FileStorageConfig {
        max_url_ttl_secs: 12_345,
        ..FileStorageConfig::default()
    };
    let json = serde_json::to_string(&original).unwrap();
    let back: FileStorageConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(back.max_url_ttl_secs, original.max_url_ttl_secs);
}
