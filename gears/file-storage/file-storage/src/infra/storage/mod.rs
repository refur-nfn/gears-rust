//! Metadata persistence for the control plane: `SeaORM` entities, tenant-scoped
//! repositories (`SecureORM`), and the migration registry.

pub mod db;
pub mod entity;
pub mod mapper;
pub mod migrations;
pub mod repo;
