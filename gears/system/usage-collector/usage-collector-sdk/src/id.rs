//! Deterministic derivation of the usage-record identity.
//!
//! The record `id` is not an independent field: it is a deterministic
//! projection of the dedup key `(tenant_id, gts_id, idempotency_key)`. The
//! gateway derives it on every create; a client MAY reproduce the same value
//! locally with this function (e.g. to reference a not-yet-acked record via
//! `corrects_id` without a round-trip).

use uuid::Uuid;

use crate::models::{IdempotencyKey, UsageTypeGtsId};

/// Fixed namespace for deterministic usage-record `id` derivation (`UUIDv5`).
///
/// NEVER change this value: changing it re-maps every dedup key to a new
/// `id`, breaking idempotency and every stored `corrects_id` reference.
pub const USAGE_RECORD_ID_NAMESPACE: Uuid =
    Uuid::from_u128(0x5631_3026_863b_4de8_b32b_1f96_b673_06ed);

/// ASCII unit separator between the three dedup-key fields. `tenant_id`
/// (hex + hyphen) and `gts_id` (GTS grammar) cannot contain it, and
/// `idempotency_key` is the final field (consumes the remainder), so the
/// encoding is injective even when a key itself contains `0x1F`.
const FIELD_SEPARATOR: u8 = 0x1F;

/// Derive the deterministic record id from the dedup key:
/// `id = UUIDv5(NS, tenant_id ⟨0x1F⟩ gts_id ⟨0x1F⟩ idempotency_key)`,
/// where `tenant_id` is its canonical lowercase-hyphenated string form and
/// `gts_id` / `idempotency_key` are their UTF-8 bytes.
#[must_use]
pub fn derive_usage_record_id(
    tenant_id: Uuid,
    gts_id: &UsageTypeGtsId,
    idempotency_key: &IdempotencyKey,
) -> Uuid {
    let mut input = Vec::new();
    input.extend_from_slice(tenant_id.to_string().as_bytes());
    input.push(FIELD_SEPARATOR);
    input.extend_from_slice(gts_id.as_ref().as_bytes());
    input.push(FIELD_SEPARATOR);
    input.extend_from_slice(idempotency_key.as_str().as_bytes());
    Uuid::new_v5(&USAGE_RECORD_ID_NAMESPACE, &input)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
#[path = "id_tests.rs"]
mod id_tests;
