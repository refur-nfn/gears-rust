//! `FileService` — control-plane business logic.
//!
//! Owns the P1 flows: create + presign upload, finalize + bind (optimistic CAS),
//! download-URL issuance, metadata CRUD, listing, versioning, and delete. It
//! depends on the repositories (tenant-scoped persistence), the backend registry
//! (byte storage), the signed-URL issuer, the content pipeline, and an
//! [`Authorizer`]. Content never flows through this service except via the
//! data-plane helpers used by the sidecar/tests.

// Domain terms (ETag, If-Match, FileStorage, GET/PUT) recur throughout the docs.
#![allow(clippy::doc_markdown)]

use std::sync::Arc;

use bytes::Bytes;
use time::OffsetDateTime;
use toolkit_db::{DBProvider, DbError};
use toolkit_security::{AccessScope, SecurityContext};
use uuid::Uuid;

use file_storage_sdk::{
    ByteRange, CustomMetadataEntry, CustomMetadataPatch, File, FileVersion, NewFile, OwnerFilter,
    VersionStatus,
};

use crate::domain::authz::{Authorizer, actions};
use crate::domain::error::DomainError;
use crate::infra::backend::{BackendCapabilities, BackendRegistry};
use crate::infra::content::{hash, mime};
use crate::infra::signed_url::{Claims, Issuer, Op, UploadConstraints};
use crate::infra::storage::repo::{FileRepo, MetadataRepo, VersionRepo};

/// Service-level configuration distilled from [`crate::config::FileStorageConfig`].
#[allow(unknown_lints, de0309_must_have_domain_model)]
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// Short default TTL (seconds) stamped on every signed URL; the issuer caps
    /// it at `max_url_ttl` (DESIGN §4.5).
    pub default_url_ttl_secs: i64,
    pub sidecar_base_url: String,
    pub default_page_size: u64,
    pub max_page_size: u64,
}

/// Result of creating a file or presigning a new version: identity plus the
/// signed URL the client `PUT`s the bytes to.
#[allow(unknown_lints, de0309_must_have_domain_model)]
#[derive(Debug, Clone)]
pub struct UploadTicket {
    pub file_id: Uuid,
    pub version_id: Uuid,
    pub upload_url: String,
}

/// Result of `download-url`: the signed URL plus the content ETag.
#[allow(unknown_lints, de0309_must_have_domain_model)]
#[derive(Debug, Clone)]
pub struct DownloadTicket {
    pub download_url: String,
    pub etag: String,
    pub version_id: Uuid,
}

/// The control-plane file service.
#[allow(unknown_lints, de0309_must_have_domain_model)]
pub struct FileService {
    db: Arc<DBProvider<DbError>>,
    files: FileRepo,
    versions: VersionRepo,
    metadata: MetadataRepo,
    backends: BackendRegistry,
    issuer: Arc<Issuer>,
    authorizer: Arc<dyn Authorizer>,
    cfg: ServiceConfig,
}

impl FileService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        db: Arc<DBProvider<DbError>>,
        backends: BackendRegistry,
        issuer: Arc<Issuer>,
        authorizer: Arc<dyn Authorizer>,
        cfg: ServiceConfig,
    ) -> Self {
        Self {
            db,
            files: FileRepo::new(),
            versions: VersionRepo::new(),
            metadata: MetadataRepo::new(),
            backends,
            issuer,
            authorizer,
            cfg,
        }
    }

    // ── helpers ─────────────────────────────────────────────────────────────

    fn tenant_scope(ctx: &SecurityContext) -> AccessScope {
        AccessScope::for_tenant(ctx.subject_tenant_id())
    }

    /// Content ETag derived from `(file_id, content_id)` — opaque, never equal to
    /// the content hash (DESIGN §3.1, §4.2).
    fn content_etag(file_id: Uuid, content_id: Uuid) -> String {
        let digest =
            hash::sha256_parts(&[b"fs-etag-v1", file_id.as_bytes(), content_id.as_bytes()]);
        format!("\"{}\"", hex::encode(&digest[..16]))
    }

    fn backend_path(file_id: Uuid, version_id: Uuid) -> String {
        format!("/{file_id}/{version_id}")
    }

    fn validate_gts_type(t: &str) -> Result<(), DomainError> {
        if t.starts_with("gts.") && t.contains('~') {
            Ok(())
        } else {
            Err(DomainError::invalid_gts_type(t))
        }
    }

    fn sign_url(
        &self,
        op: Op,
        v: &VersionRef,
        constraints: UploadConstraints,
    ) -> Result<String, DomainError> {
        let now = OffsetDateTime::now_utc();
        let claims = Claims {
            op,
            file_id: v.file_id,
            version_id: v.version_id,
            backend_id: v.backend_id.clone(),
            backend_path: v.backend_path.clone(),
            exp: now.unix_timestamp() + self.cfg.default_url_ttl_secs,
            upload: constraints,
        };
        let token = self.issuer.issue(claims, now)?;
        let verb = match op {
            Op::Get => "download",
            Op::Put => "upload",
        };
        Ok(format!(
            "{}/api/file-storage-data/v1/{}/{}/{}?fs-token={}",
            self.cfg.sidecar_base_url.trim_end_matches('/'),
            verb,
            v.file_id,
            v.version_id,
            token
        ))
    }

    /// Fetch a file within the caller's tenant or fail with `FileNotFound`.
    async fn require_file<C: toolkit_db::secure::DBRunner>(
        &self,
        conn: &C,
        scope: &AccessScope,
        file_id: Uuid,
    ) -> Result<File, DomainError> {
        self.files
            .get(conn, scope, file_id)
            .await?
            .ok_or_else(|| DomainError::file_not_found(file_id))
    }

    // ── create + presign ─────────────────────────────────────────────────────

    /// `POST /files`: create a file and presign the first content upload.
    pub async fn create_file(
        &self,
        ctx: &SecurityContext,
        new: NewFile,
    ) -> Result<UploadTicket, DomainError> {
        Self::validate_gts_type(&new.gts_file_type)?;
        let _scope = self
            .authorizer
            .authorize(ctx, actions::WRITE, &new.gts_file_type, None)
            .await?;

        let conn = self.db.conn().map_err(DomainError::from)?;
        let now = OffsetDateTime::now_utc();
        let file_id = Uuid::now_v7();
        let version_id = Uuid::now_v7();
        let backend = self.backends.default_backend();
        let backend_id = backend.id().to_owned();
        let backend_path = Self::backend_path(file_id, version_id);

        let file = File {
            file_id,
            tenant_id: ctx.subject_tenant_id(),
            owner_kind: new.owner_kind,
            owner_id: new.owner_id,
            name: new.name,
            gts_file_type: new.gts_file_type.clone(),
            content_id: None,
            meta_version: 0,
            created_at: now,
            last_modified_at: now,
        };
        self.files
            .create(&conn, &AccessScope::allow_all(), &file)
            .await?;

        let pending = pending_version(
            file_id,
            version_id,
            &new.mime_type,
            &backend_id,
            &backend_path,
            now,
        );
        self.versions
            .insert(&conn, &AccessScope::allow_all(), &pending)
            .await?;

        for entry in &new.custom_metadata {
            self.metadata
                .upsert(
                    &conn,
                    &AccessScope::allow_all(),
                    file_id,
                    &entry.key,
                    &entry.value,
                    now,
                )
                .await?;
        }

        let upload_url = self.sign_url(
            Op::Put,
            &VersionRef {
                file_id,
                version_id,
                backend_id,
                backend_path,
            },
            UploadConstraints::default(),
        )?;
        Ok(UploadTicket {
            file_id,
            version_id,
            upload_url,
        })
    }

    /// `POST /files/{id}/versions`: presign a new content version on an existing
    /// file (the upload's bytes will be bound via `bind`).
    pub async fn presign_version(
        &self,
        ctx: &SecurityContext,
        file_id: Uuid,
    ) -> Result<UploadTicket, DomainError> {
        let prefetch = Self::tenant_scope(ctx);
        let conn = self.db.conn().map_err(DomainError::from)?;
        let file = self.require_file(&conn, &prefetch, file_id).await?;
        let _scope = self
            .authorizer
            .authorize(ctx, actions::WRITE, &file.gts_file_type, Some(file_id))
            .await?;

        let now = OffsetDateTime::now_utc();
        let version_id = Uuid::now_v7();
        let backend = self.backends.default_backend();
        let backend_id = backend.id().to_owned();
        let backend_path = Self::backend_path(file_id, version_id);

        // Reuse the current version's mime as the declared type placeholder.
        let mime_type = self
            .current_version_mime(&conn, &AccessScope::allow_all(), &file)
            .await
            .unwrap_or_else(|| "application/octet-stream".to_owned());
        let pending = pending_version(
            file_id,
            version_id,
            &mime_type,
            &backend_id,
            &backend_path,
            now,
        );
        self.versions
            .insert(&conn, &AccessScope::allow_all(), &pending)
            .await?;

        let upload_url = self.sign_url(
            Op::Put,
            &VersionRef {
                file_id,
                version_id,
                backend_id,
                backend_path,
            },
            UploadConstraints::default(),
        )?;
        Ok(UploadTicket {
            file_id,
            version_id,
            upload_url,
        })
    }

    async fn current_version_mime<C: toolkit_db::secure::DBRunner>(
        &self,
        conn: &C,
        scope: &AccessScope,
        file: &File,
    ) -> Option<String> {
        let content_id = file.content_id?;
        self.versions
            .get(conn, scope, file.file_id, content_id)
            .await
            .ok()
            .flatten()
            .map(|v| v.mime_type)
    }

    // ── finalize + bind (the optimistic CAS) ──────────────────────────────────

    /// Record an uploaded version's size+hash and mark it available. Called by
    /// the sidecar after streaming bytes to the backend (write action).
    pub async fn finalize_upload(
        &self,
        ctx: &SecurityContext,
        file_id: Uuid,
        version_id: Uuid,
        size: i64,
        hash_value: Vec<u8>,
    ) -> Result<(), DomainError> {
        let prefetch = Self::tenant_scope(ctx);
        let conn = self.db.conn().map_err(DomainError::from)?;
        let file = self.require_file(&conn, &prefetch, file_id).await?;
        let _scope = self
            .authorizer
            .authorize(ctx, actions::WRITE, &file.gts_file_type, Some(file_id))
            .await?;
        let ok = self
            .versions
            .finalize(
                &conn,
                &AccessScope::allow_all(),
                file_id,
                version_id,
                size,
                hash_value,
            )
            .await?;
        if !ok {
            return Err(DomainError::version_not_found(file_id, version_id));
        }
        Ok(())
    }

    /// `POST /files/{id}/bind`: swap the content pointer to `version_id` under
    /// optimistic CAS guarded by the `If-Match` content ETag. Returns the
    /// updated file; `412` on conflict (re-read the ETag and rebind).
    ///
    /// `if_match` is the opaque content ETag (or `*`, or `None` for the first
    /// bind). The server recomputes the current ETag and compares — it never
    /// reverses the ETag back to a `content_id`.
    pub async fn bind(
        &self,
        ctx: &SecurityContext,
        file_id: Uuid,
        version_id: Uuid,
        if_match: Option<&str>,
    ) -> Result<File, DomainError> {
        let prefetch = Self::tenant_scope(ctx);
        let conn = self.db.conn().map_err(DomainError::from)?;
        let file = self.require_file(&conn, &prefetch, file_id).await?;
        let scope = self
            .authorizer
            .authorize(ctx, actions::WRITE, &file.gts_file_type, Some(file_id))
            .await?;

        // The version must exist and be available.
        let version = self
            .versions
            .get(&conn, &AccessScope::allow_all(), file_id, version_id)
            .await?
            .ok_or_else(|| DomainError::version_not_found(file_id, version_id))?;
        if version.status != VersionStatus::Available {
            return Err(DomainError::conflict(
                "cannot bind a version whose upload has not been finalized",
            ));
        }

        // Validate the If-Match precondition against the current content ETag.
        let expected_content_id = file.content_id;
        let current_etag = expected_content_id.map(|c| Self::content_etag(file_id, c));
        match if_match {
            // The first bind (no content yet) may omit If-Match; rebinding
            // already-bound content MUST carry it, otherwise the advertised
            // conditional update degrades into an unconditional overwrite.
            None => {
                if expected_content_id.is_some() {
                    return Err(DomainError::precondition_failed(
                        "If-Match is required to rebind already-bound content",
                    ));
                }
            }
            Some(m) => {
                let m = m.trim();
                if m != "*" && Some(m) != current_etag.as_deref() {
                    return Err(DomainError::precondition_failed(
                        "If-Match does not match the current content ETag",
                    ));
                }
            }
        }

        // Swap the content pointer (CAS) and flip `is_current` in a SINGLE
        // transaction so `files.content_id` and `file_versions.is_current` can
        // never diverge if a later write fails (DESIGN §3.7 bind invariant).
        let now = OffsetDateTime::now_utc();
        let files = self.files.clone();
        let versions = self.versions.clone();
        let bind_scope = scope.clone();
        let swapped = self
            .db
            .db()
            .transaction_ref_mapped(move |tx| {
                Box::pin(async move {
                    let swapped = files
                        .bind_content_cas(
                            tx,
                            &bind_scope,
                            file_id,
                            expected_content_id,
                            version_id,
                            now,
                        )
                        .await?;
                    if !swapped {
                        return Ok(false);
                    }
                    // Promote the new version as current (unique-current index honoured).
                    versions
                        .clear_current(tx, &AccessScope::allow_all(), file_id)
                        .await?;
                    versions
                        .set_current(tx, &AccessScope::allow_all(), file_id, version_id)
                        .await?;
                    Ok::<bool, DomainError>(true)
                })
            })
            .await?;
        if !swapped {
            return Err(DomainError::precondition_failed(
                "content pointer changed concurrently; re-read the ETag and rebind",
            ));
        }

        self.require_file(&conn, &scope, file_id).await
    }

    // ── reads ─────────────────────────────────────────────────────────────────

    /// Get a file's metadata.
    pub async fn get_file(
        &self,
        ctx: &SecurityContext,
        file_id: Uuid,
    ) -> Result<File, DomainError> {
        let prefetch = Self::tenant_scope(ctx);
        let conn = self.db.conn().map_err(DomainError::from)?;
        let file = self.require_file(&conn, &prefetch, file_id).await?;
        let scope = self
            .authorizer
            .authorize(ctx, actions::READ, &file.gts_file_type, Some(file_id))
            .await?;
        self.require_file(&conn, &scope, file_id).await
    }

    /// Get a file plus its custom metadata.
    pub async fn get_file_with_metadata(
        &self,
        ctx: &SecurityContext,
        file_id: Uuid,
    ) -> Result<(File, Vec<CustomMetadataEntry>), DomainError> {
        let file = self.get_file(ctx, file_id).await?;
        let conn = self.db.conn().map_err(DomainError::from)?;
        let meta = self
            .metadata
            .list(&conn, &AccessScope::allow_all(), file_id)
            .await?;
        Ok((file, meta))
    }

    /// List files for a mandatory owner filter, offset-paginated.
    pub async fn list_files(
        &self,
        ctx: &SecurityContext,
        owner: OwnerFilter,
        limit: Option<u64>,
        offset: u64,
    ) -> Result<Vec<File>, DomainError> {
        // Authorize (access gate), then always tenant-scope the query so the
        // tenant boundary holds regardless of the PDP's returned constraints.
        self.authorizer
            .authorize(ctx, actions::READ, "", None)
            .await?;
        let limit = limit
            .unwrap_or(self.cfg.default_page_size)
            .min(self.cfg.max_page_size);
        let conn = self.db.conn().map_err(DomainError::from)?;
        self.files
            .list(&conn, &Self::tenant_scope(ctx), owner, limit, offset)
            .await
    }

    /// The content ETag for a file (or `None` if no content is bound yet).
    #[must_use]
    pub fn etag_for(file: &File) -> Option<String> {
        file.content_id
            .map(|cid| Self::content_etag(file.file_id, cid))
    }

    // ── metadata update ────────────────────────────────────────────────────────

    /// `PATCH /files/{id}`: JSON-merge-patch the custom metadata and bump
    /// `meta_version`, optionally guarded by `If-Match-Metadata`.
    pub async fn update_metadata(
        &self,
        ctx: &SecurityContext,
        file_id: Uuid,
        patch: CustomMetadataPatch,
        expected_meta_version: Option<i64>,
    ) -> Result<File, DomainError> {
        let prefetch = Self::tenant_scope(ctx);
        let conn = self.db.conn().map_err(DomainError::from)?;
        let file = self.require_file(&conn, &prefetch, file_id).await?;
        let scope = self
            .authorizer
            .authorize(ctx, actions::WRITE, &file.gts_file_type, Some(file_id))
            .await?;

        // Apply the meta-version CAS and the patch in ONE transaction. The CAS
        // runs first, so a stale `expected_meta_version` aborts before any row
        // is touched and the rollback guarantees no partial metadata change is
        // committed (the optimistic-concurrency guard cannot be bypassed). The
        // per-key delete-then-insert upsert is also covered by the rollback, so
        // a failed insert can never leave a key permanently removed.
        let now = OffsetDateTime::now_utc();
        let files = self.files.clone();
        let metadata = self.metadata.clone();
        let patch_scope = scope.clone();
        self.db
            .db()
            .transaction_ref_mapped(move |tx| {
                Box::pin(async move {
                    let bumped = files
                        .touch_meta(tx, &patch_scope, file_id, expected_meta_version, now)
                        .await?;
                    if !bumped {
                        return Err(DomainError::precondition_failed(
                            "metadata revision changed concurrently (If-Match-Metadata)",
                        ));
                    }
                    for (key, value) in &patch.entries {
                        match value {
                            Some(v) => {
                                metadata
                                    .upsert(tx, &AccessScope::allow_all(), file_id, key, v, now)
                                    .await?;
                            }
                            None => {
                                metadata
                                    .delete_key(tx, &AccessScope::allow_all(), file_id, key)
                                    .await?;
                            }
                        }
                    }
                    Ok::<(), DomainError>(())
                })
            })
            .await?;
        self.require_file(&conn, &scope, file_id).await
    }

    // ── delete ──────────────────────────────────────────────────────────────────

    /// `DELETE /files/{id}`: remove the file and all versions (FK cascade), then
    /// best-effort delete the backend blobs.
    pub async fn delete_file(
        &self,
        ctx: &SecurityContext,
        file_id: Uuid,
    ) -> Result<(), DomainError> {
        let prefetch = Self::tenant_scope(ctx);
        let conn = self.db.conn().map_err(DomainError::from)?;
        let file = self.require_file(&conn, &prefetch, file_id).await?;
        let scope = self
            .authorizer
            .authorize(ctx, actions::DELETE, &file.gts_file_type, Some(file_id))
            .await?;

        // Collect backend blobs before the metadata row (and FK children) vanish.
        let versions = self
            .versions
            .list_by_file(&conn, &AccessScope::allow_all(), file_id)
            .await?;
        let removed = self.files.delete(&conn, &scope, file_id).await?;
        if !removed {
            return Err(DomainError::file_not_found(file_id));
        }
        // Best-effort backend cleanup; a failure degrades to an orphan (P2 GC).
        for v in versions {
            self.best_effort_blob_delete(&v.backend_id, &v.backend_path)
                .await;
        }
        Ok(())
    }

    // ── download + versioning ─────────────────────────────────────────────────

    /// `GET /files/{id}/download-url`: issue a signed download URL pinned to the
    /// current content (or a specific `version_id`).
    pub async fn download_url(
        &self,
        ctx: &SecurityContext,
        file_id: Uuid,
        version_id: Option<Uuid>,
    ) -> Result<DownloadTicket, DomainError> {
        let prefetch = Self::tenant_scope(ctx);
        let conn = self.db.conn().map_err(DomainError::from)?;
        let file = self.require_file(&conn, &prefetch, file_id).await?;
        let _scope = self
            .authorizer
            .authorize(ctx, actions::READ, &file.gts_file_type, Some(file_id))
            .await?;

        let target = match version_id {
            Some(v) => v,
            None => file
                .content_id
                .ok_or_else(|| DomainError::conflict("file has no bound content yet"))?,
        };
        let version = self
            .versions
            .get(&conn, &AccessScope::allow_all(), file_id, target)
            .await?
            .ok_or_else(|| DomainError::version_not_found(file_id, target))?;

        let download_url = self.sign_url(
            Op::Get,
            &VersionRef {
                file_id,
                version_id: target,
                backend_id: version.backend_id,
                backend_path: version.backend_path,
            },
            UploadConstraints::default(),
        )?;
        Ok(DownloadTicket {
            download_url,
            etag: Self::content_etag(file_id, target),
            version_id: target,
        })
    }

    /// List all versions of a file.
    pub async fn list_versions(
        &self,
        ctx: &SecurityContext,
        file_id: Uuid,
    ) -> Result<Vec<FileVersion>, DomainError> {
        let prefetch = Self::tenant_scope(ctx);
        let conn = self.db.conn().map_err(DomainError::from)?;
        let file = self.require_file(&conn, &prefetch, file_id).await?;
        let _scope = self
            .authorizer
            .authorize(ctx, actions::READ, &file.gts_file_type, Some(file_id))
            .await?;
        self.versions
            .list_by_file(&conn, &AccessScope::allow_all(), file_id)
            .await
    }

    /// Restore a prior version as current (a rebind: pointer swap, no re-upload).
    pub async fn restore_version(
        &self,
        ctx: &SecurityContext,
        file_id: Uuid,
        version_id: Uuid,
    ) -> Result<File, DomainError> {
        let file = self.get_file(ctx, file_id).await?;
        let if_match = Self::etag_for(&file);
        self.bind(ctx, file_id, version_id, if_match.as_deref())
            .await
    }

    /// Delete a single version (and its backend blob). Deleting the only version
    /// is equivalent to deleting the file.
    pub async fn delete_version(
        &self,
        ctx: &SecurityContext,
        file_id: Uuid,
        version_id: Uuid,
    ) -> Result<(), DomainError> {
        let prefetch = Self::tenant_scope(ctx);
        let conn = self.db.conn().map_err(DomainError::from)?;
        let file = self.require_file(&conn, &prefetch, file_id).await?;
        let _scope = self
            .authorizer
            .authorize(ctx, actions::DELETE, &file.gts_file_type, Some(file_id))
            .await?;

        let all = self
            .versions
            .list_by_file(&conn, &AccessScope::allow_all(), file_id)
            .await?;
        if all.len() <= 1 {
            // Last version → delete the whole file.
            return self.delete_file(ctx, file_id).await;
        }
        let Some(version) = all.into_iter().find(|v| v.version_id == version_id) else {
            return Err(DomainError::version_not_found(file_id, version_id));
        };
        if file.content_id == Some(version_id) {
            return Err(DomainError::conflict(
                "cannot delete the current version; bind another version first",
            ));
        }
        self.versions
            .delete(&conn, &AccessScope::allow_all(), file_id, version_id)
            .await?;
        self.best_effort_blob_delete(&version.backend_id, &version.backend_path)
            .await;
        Ok(())
    }

    // ── backends discovery ────────────────────────────────────────────────────

    /// `GET /storages`: configured backends and their capabilities.
    #[must_use]
    pub fn list_backends(&self) -> Vec<(String, BackendCapabilities)> {
        self.backends.list()
    }

    /// `GET /storages/{id}`.
    pub fn get_backend(&self, id: &str) -> Result<(String, BackendCapabilities), DomainError> {
        let b = self.backends.get(id)?;
        Ok((b.id().to_owned(), b.capabilities()))
    }

    /// Delete a backend blob, logging (not failing) on error. A failed delete
    /// degrades to an orphan reconciled by the P2 cleanup engine.
    async fn best_effort_blob_delete(&self, backend_id: &str, path: &str) {
        let Ok(backend) = self.backends.get(backend_id) else {
            return;
        };
        if let Err(err) = backend.delete(path).await {
            tracing::warn!(?err, path, "best-effort backend delete failed");
        }
    }

    // ── data-plane helpers (used by the sidecar and by tests) ──────────────────

    /// Validate, store, hash, and finalize an uploaded blob in one step. This is
    /// the in-process equivalent of the sidecar's stream-and-bind; the sidecar
    /// performs the same steps while streaming.
    pub async fn put_content(
        &self,
        ctx: &SecurityContext,
        file_id: Uuid,
        version_id: Uuid,
        declared_mime: &str,
        bytes: Bytes,
    ) -> Result<(), DomainError> {
        // Content-type validation against the actual bytes.
        mime::validate(declared_mime, &bytes)?;

        let _ = ctx;
        let conn = self.db.conn().map_err(DomainError::from)?;
        let version = self
            .versions
            .get(&conn, &AccessScope::allow_all(), file_id, version_id)
            .await?
            .ok_or_else(|| DomainError::version_not_found(file_id, version_id))?;

        let backend = self.backends.get(&version.backend_id)?;
        backend.put(&version.backend_path, bytes.clone()).await?;

        let size = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
        let digest = hash::sha256(&bytes);
        self.finalize_upload(ctx, file_id, version_id, size, digest)
            .await
    }

    /// Read a (range of a) version's content from its backend.
    pub async fn read_content(
        &self,
        ctx: &SecurityContext,
        file_id: Uuid,
        version_id: Uuid,
        range: Option<ByteRange>,
    ) -> Result<Bytes, DomainError> {
        let _ = ctx;
        let conn = self.db.conn().map_err(DomainError::from)?;
        let version = self
            .versions
            .get(&conn, &AccessScope::allow_all(), file_id, version_id)
            .await?
            .ok_or_else(|| DomainError::version_not_found(file_id, version_id))?;

        let backend = self.backends.get(&version.backend_id)?;
        match range {
            Some(r) => backend.get_range(&version.backend_path, r).await,
            None => backend.get(&version.backend_path).await,
        }
    }
}

/// A minimal reference to a version's backend location, for URL signing.
#[allow(unknown_lints, de0309_must_have_domain_model)]
struct VersionRef {
    file_id: Uuid,
    version_id: Uuid,
    backend_id: String,
    backend_path: String,
}

/// Build a `pending` version row with placeholder size/hash (filled at finalize).
fn pending_version(
    file_id: Uuid,
    version_id: Uuid,
    mime_type: &str,
    backend_id: &str,
    backend_path: &str,
    now: OffsetDateTime,
) -> FileVersion {
    FileVersion {
        file_id,
        version_id,
        mime_type: mime_type.to_owned(),
        size: 0,
        hash_algorithm: hash::ALGORITHM.to_owned(),
        // 32 zero bytes — satisfies the NOT NULL + length-32 CHECK until finalize.
        hash_value: vec![0u8; 32],
        status: VersionStatus::Pending,
        is_current: false,
        backend_id: backend_id.to_owned(),
        backend_path: backend_path.to_owned(),
        created_at: now,
    }
}
