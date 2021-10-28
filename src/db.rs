use std::path::Path;

use anyhow::Context;
use async_trait::async_trait;
use sodiumoxide::version;
use sqlx::{FromRow, SqlitePool, query_as, sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions}};

use crate::{crypto, server::{Post, ReadQuery}};

const DB_VERSION: u32 = 1;

pub(crate) fn options(file: impl AsRef<Path>) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
    .filename(&file)
    // sqlx defaults to WAL, but we don't need that type of performance, or the extra files:
    .journal_mode(SqliteJournalMode::Delete)
}

 pub(crate) fn pool(opts: SqliteConnectOptions) -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_lazy_with(opts)
}


#[async_trait]
pub(crate) trait VaultExt {
    async fn get_version(&self) -> anyhow::Result<u32>;
    async fn needs_upgrade(&self) -> anyhow::Result<bool>;
    async fn public_key(&self) -> anyhow::Result<crypto::SealedBoxPublicKey>;
    async fn get_posts(&self, query: &ReadQuery) -> anyhow::Result<Vec<Entry>>;
    async fn write_entry(&self, entry: Entry) -> anyhow::Result<()>;
}

#[async_trait]
impl VaultExt for sqlx::Pool<sqlx::Sqlite> {
    
    async fn get_posts(&self, query: &ReadQuery) -> anyhow::Result<Vec<Entry>> {
        let entries = sqlx::query_as("
                SELECT timestamp_ms_utc, contents, offset_utc_mins
                FROM entry
                ORDER BY timestamp_ms_utc DESC
                LIMIT ?, ?
            ")
            .bind(query.offset.map(|u| u as i64).unwrap_or(0))
            .bind(query.limit.map(|u| u as i64).unwrap_or(50))
            .fetch_all(self)
            .await?;
        Ok(entries)
    }

    async fn write_entry(&self, entry: Entry) -> anyhow::Result<()> {
        let Entry{timestamp_ms_utc, offset_utc_mins, contents} = entry;
        sqlx::query("
                INSERT INTO entry(timestamp_ms_utc, offset_utc_mins, contents)
                VALUES(?,?,?)
            ")
            .bind(timestamp_ms_utc)
            .bind(offset_utc_mins)
            .bind(contents)
            .execute(self).await?;

        Ok(())
    }

    async fn get_version(&self) -> anyhow::Result<u32> {
        let (version_str,): (String,) = query_as("SELECT value FROM settings WHERE key = 'version'")
            .fetch_one(self)
            .await?;
        
        let version = version_str.parse().context("Error parsing DB version")?;
        Ok(version)
    }

    // TODO: Separate out the println bits into a different method.
    async fn needs_upgrade(&self) -> anyhow::Result<bool> {
        let version = self.get_version().await?;
        if version == DB_VERSION {
            return Ok(false);
        } else if DB_VERSION > version {
            println!("Database version {} needs upgrade to version {}", version, DB_VERSION);
            return Ok(true);
        } else {
            println!("Database version {} is greater than supported version {}", version, DB_VERSION);
            return Ok(true);
        }
    }

    async fn public_key(&self) -> anyhow::Result<crypto::SealedBoxPublicKey> {
        let (key_str,): (String,) = query_as("SELECT value FROM settings WHERE key = 'publicKey'")
            .fetch_one(self)
            .await?;

        let key = crypto::SealedBoxPublicKey::from_base58(&key_str).context("Decoding public key")?;
        Ok(key)
    }

}


#[derive(FromRow)]
pub(crate) struct Entry {
    /// ms since UTC epoch.
    pub(crate) timestamp_ms_utc: i64,
    pub(crate) offset_utc_mins: i32,

    /// Encrypted data. Probably markdown text.
    pub(crate) contents: Vec<u8>,
    

}