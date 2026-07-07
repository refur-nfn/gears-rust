use toolkit_gts::gts_id;
use uuid::Uuid;

use crate::id::{USAGE_RECORD_ID_NAMESPACE, derive_usage_record_id};
use crate::models::{IdempotencyKey, UsageTypeGtsId};

fn tenant() -> Uuid {
    Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap()
}
fn gts() -> UsageTypeGtsId {
    // Must be a valid derived GTS instance id: the segment after `~` is itself
    // a full vendor.package.namespace.type.vMAJOR[.MINOR] chain.
    UsageTypeGtsId::new(gts_id!(
        "cf.core.uc.usage_record.v1~cf.mini_chat._.tokens_consumed.v1"
    ))
    .unwrap()
}
fn key(s: &str) -> IdempotencyKey {
    IdempotencyKey::new(s).unwrap()
}

#[test]
fn derive_is_deterministic() {
    assert_eq!(
        derive_usage_record_id(tenant(), &gts(), &key("idem-1")),
        derive_usage_record_id(tenant(), &gts(), &key("idem-1")),
    );
}

#[test]
fn derive_matches_golden_vector() {
    // UUIDv5(NS, "11111111-1111-1111-1111-111111111111" 0x1F
    //            "gts.cf.core.uc.usage_record.v1~cf.mini_chat._.tokens_consumed.v1" 0x1F
    //            "idem-1")
    assert_eq!(
        derive_usage_record_id(tenant(), &gts(), &key("idem-1")),
        Uuid::parse_str("94afa5f8-5699-5453-ba68-0699985ebc2e").unwrap(),
    );
}

#[test]
fn derive_produces_a_v5_uuid() {
    assert_eq!(
        derive_usage_record_id(tenant(), &gts(), &key("idem-1")).get_version_num(),
        5,
    );
}

#[test]
fn distinct_keys_yield_distinct_ids() {
    assert_ne!(
        derive_usage_record_id(tenant(), &gts(), &key("idem-1")),
        derive_usage_record_id(tenant(), &gts(), &key("idem-2")),
    );
}

#[test]
fn distinct_tenants_yield_distinct_ids() {
    let other = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
    assert_ne!(
        derive_usage_record_id(tenant(), &gts(), &key("idem-1")),
        derive_usage_record_id(other, &gts(), &key("idem-1")),
    );
}

#[test]
fn distinct_gts_ids_yield_distinct_ids() {
    // Same tenant + idempotency_key, different gts_id: the third dedup-key
    // field must participate in the derivation just like the other two.
    let other = UsageTypeGtsId::new(gts_id!(
        "cf.core.uc.usage_record.v1~cf.mini_chat._.messages_sent.v1"
    ))
    .unwrap();
    assert_ne!(
        derive_usage_record_id(tenant(), &gts(), &key("idem-1")),
        derive_usage_record_id(tenant(), &other, &key("idem-1")),
    );
}

#[test]
fn separator_in_key_does_not_alias() {
    let with_us = derive_usage_record_id(tenant(), &gts(), &key("idem\u{1f}1"));
    assert_eq!(
        with_us,
        Uuid::parse_str("1f91f541-9df0-51c8-9458-f2ded04ec396").unwrap(),
    );
    assert_ne!(
        with_us,
        derive_usage_record_id(tenant(), &gts(), &key("idem-1"))
    );
}

#[test]
fn namespace_is_pinned() {
    assert_eq!(
        USAGE_RECORD_ID_NAMESPACE,
        Uuid::parse_str("56313026-863b-4de8-b32b-1f96b67306ed").unwrap(),
    );
}
