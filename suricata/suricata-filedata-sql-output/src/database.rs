// Copyright (C) 2025-2026  A. Iooss
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::Filedata;
use sqlx::Connection;
use std::str::FromStr;

const SQL_SCHEMA: &str =
    "CREATE TABLE IF NOT EXISTS filedata (name TEXT PRIMARY KEY, sz INT, data BYTEA);";

enum DatabaseConnection {
    Sqlite(sqlx::sqlite::SqliteConnection),
    Postgres(sqlx::postgres::PgConnection),
}

pub struct Database {
    runtime: Option<tokio::runtime::Runtime>,
    conn: Option<DatabaseConnection>,
    rx: std::sync::mpsc::Receiver<Filedata>,
    count_batch: usize,
    count_incoming: usize,
    count_inserted: u64,
}

impl Database {
    /// Init database
    pub fn new(url: &str, rx: std::sync::mpsc::Receiver<Filedata>) -> Result<Self, sqlx::Error> {
        // sqlx requires async runtime
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let conn = runtime.block_on(async {
            if url.starts_with("sqlite:") {
                let options = sqlx::sqlite::SqliteConnectOptions::from_str(url)?
                    .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
                    .synchronous(sqlx::sqlite::SqliteSynchronous::Off);
                let mut conn = sqlx::sqlite::SqliteConnection::connect_with(&options).await?;
                sqlx::raw_sql(SQL_SCHEMA).execute(&mut conn).await?;
                Ok(DatabaseConnection::Sqlite(conn))
            } else if url.starts_with("postgres:") {
                // Wait for database to be ready
                let mut conn = {
                    let mut maybe_conn: Option<sqlx::postgres::PgConnection> = None;
                    while maybe_conn.is_none() {
                        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                        maybe_conn = sqlx::postgres::PgConnection::connect(url).await.ok();
                    }
                    maybe_conn.unwrap() // won't panic
                };
                sqlx::raw_sql(SQL_SCHEMA).execute(&mut conn).await?;
                Ok(DatabaseConnection::Postgres(conn))
            } else {
                Err(sqlx::Error::Configuration(
                    "Only sqlite and postgres database schemes are supported".into(),
                ))
            }
        })?;
        Ok(Self {
            runtime: Some(runtime),
            conn: Some(conn),
            rx,
            count_batch: 0,
            count_incoming: 0,
            count_inserted: 0,
        })
    }

    /// Main worker loop
    async fn batch_write_filedata(&mut self) -> Result<(), sqlx::Error> {
        while let Ok(filedata) = self.rx.recv() {
            let mut batch = vec![filedata];
            batch.extend(self.rx.try_iter()); // Drain channel
            self.count_batch = self.count_batch.saturating_add(1);
            self.count_incoming = self.count_incoming.saturating_add(batch.len());

            // Insert batch in database
            let inserted = match self.conn.as_mut().unwrap() {
                DatabaseConnection::Sqlite(conn) => {
                    let mut transaction = conn.begin().await?;
                    let mut inserted = 0u64;
                    for fd in batch {
                        let count = sqlx::query(
                            "INSERT INTO filedata (name, sz, data) VALUES ($1, $2, $3) ON CONFLICT DO NOTHING"
                        )
                        .bind(fd.name)
                        .bind(fd.original_size)
                        .bind(fd.data)
                        .execute(&mut *transaction)
                        .await
                        .map(|r| r.rows_affected())?;
                        inserted = inserted.saturating_add(count);
                    }
                    transaction.commit().await?;
                    inserted
                }
                DatabaseConnection::Postgres(conn) => {
                    let batch_name: Vec<&str> = batch.iter().map(|t| t.name.as_str()).collect();
                    let batch_original_size: Vec<i64> =
                        batch.iter().map(|t| t.original_size).collect();
                    let batch_data: Vec<&[u8]> = batch.iter().map(|t| t.data.as_slice()).collect();
                    let mut transaction = conn.begin().await?;
                    let inserted = sqlx::query(
                            "INSERT INTO filedata (name, sz, data) SELECT * FROM UNNEST($1::text[], $2::int8[], $3::bytea[]) ON CONFLICT DO NOTHING",
                        )
                        .bind(&batch_name)
                        .bind(&batch_original_size)
                        .bind(&batch_data)
                        .execute(&mut *transaction)
                        .await
                        .map(|r| r.rows_affected())?;
                    transaction.commit().await?;
                    inserted
                }
            };
            log::debug!("Inserted {inserted} rows");
            self.count_inserted = self.count_inserted.saturating_add(inserted);
        }
        match self.conn.take() {
            Some(DatabaseConnection::Sqlite(c)) => {
                c.close().await?;
            }
            Some(DatabaseConnection::Postgres(c)) => {
                c.close().await?;
            }
            None => {}
        }
        Ok(())
    }

    /// Database thread entry
    pub fn run(&mut self) {
        log::debug!("Database thread started");
        let rt = self.runtime.take().unwrap();
        rt.block_on(async {
            if let Err(err) = self.batch_write_filedata().await {
                log::error!("Database thread ended prematurely: {err:?}");
            }
        });
        log::info!(
            "Database thread finished: batch={} incoming={} inserted={}",
            self.count_batch,
            self.count_incoming,
            self.count_inserted
        );
    }
}
