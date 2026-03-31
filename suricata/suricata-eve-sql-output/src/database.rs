// Copyright (C) 2024  ANSSI
// Copyright (C) 2025-2026  A. Iooss
// SPDX-License-Identifier: GPL-2.0-or-later

use crate::EveEvent;
use sqlx::{Connection, Transaction};
use std::str::FromStr;

const SQL_SCHEMA: &str = include_str!("schema.sql");

fn sc_ip_format(buf: &str) -> (String, String) {
    let src_ip_part = buf.split(r#","src_ip":""#).nth(1).unwrap_or_default();
    let src_ip = src_ip_part.split('"').next().unwrap_or("0.0.0.0");
    let src_ip_fmt = match src_ip.parse() {
        Ok(std::net::IpAddr::V4(ip)) => ip.to_string(),
        Ok(std::net::IpAddr::V6(ip)) => format!("[{ip}]"),
        Err(_) => src_ip.to_string(),
    };
    let dest_ip_part = buf.split(r#","dest_ip":""#).nth(1).unwrap_or_default();
    let dest_ip = dest_ip_part.split('"').next().unwrap_or("0.0.0.0");
    let dest_ip_fmt = match dest_ip.parse() {
        Ok(std::net::IpAddr::V4(ip)) => ip.to_string(),
        Ok(std::net::IpAddr::V6(ip)) => format!("[{ip}]"),
        Err(_) => src_ip.to_string(),
    };
    (src_ip_fmt, dest_ip_fmt)
}

/// Add events to the SQLite database
async fn write_batch_sqlite(
    transaction: &mut Transaction<'_, sqlx::Sqlite>,
    events: &[EveEvent],
) -> Result<u64, sqlx::Error> {
    let mut inserted = 0u64;
    for event in events {
        let count = match event.type_.as_str() {
            "flow" => {
                let (src_ip, dest_ip) = sc_ip_format(&event.data);
                // SQLite UNIXEPOCH currently has only millisecond precision using "subsec", which is not enough
                sqlx::query(
                    "INSERT INTO flow (id, ts_start, ts_end, src_ip, src_port, dest_ip, dest_port, proto, app_proto, metadata, extra_data) \
                    VALUES ($1->>'flow_id', \
                    (UNIXEPOCH(SUBSTR($1->>'$.flow.start', 1, 19))*1000000 + SUBSTR($1->>'$.flow.start', 21, 6)), \
                    (UNIXEPOCH(SUBSTR($1->>'$.flow.end', 1, 19))*1000000 + SUBSTR($1->>'$.flow.end', 21, 6)), \
                    $2, $1->>'src_port', $3, $1->>'dest_port', $1->>'proto', $1->>'app_proto', jsonb_extract($1, '$.metadata'), jsonb_extract($1, '$.flow')) \
                    ON CONFLICT DO NOTHING")
                .bind(&event.data)
                .bind(src_ip)
                .bind(dest_ip)
                .execute(&mut **transaction)
                .await
                .map(|r| r.rows_affected())
            },
            "alert" => sqlx::query(
                "WITH vars AS (SELECT jsonb_extract($1, '$.' || $2) AS extra_data) \
                INSERT OR IGNORE INTO alert (flow_id, tag, color, timestamp, extra_data) \
                SELECT $1->>'flow_id', (vars.extra_data->>'$.metadata.tag[0]'), (vars.extra_data->>'$.metadata.color[0]'), (UNIXEPOCH(SUBSTR($1->>'timestamp', 1, 19))*1000000 + SUBSTR($1->>'timestamp', 21, 6)), vars.extra_data \
                FROM vars")
                .bind(&event.data)
                .bind(&event.type_).execute(&mut **transaction)
                .await
                .map(|r| r.rows_affected()),
            "stats" => sqlx::query("INSERT INTO stats (timestamp, extra_data) VALUES ((UNIXEPOCH(SUBSTR($1->>'timestamp', 1, 19))*1000000 + SUBSTR($1->>'timestamp', 21, 6)), jsonb_extract($1, '$.stats')) ON CONFLICT DO NOTHING")
                .bind(&event.data)
                .execute(&mut **transaction)
                .await
                .map(|r| r.rows_affected()),
            _ => sqlx::query(
                "INSERT INTO 'other-event' (flow_id, timestamp, event_type, extra_data) \
                VALUES ($1->>'flow_id', (UNIXEPOCH(SUBSTR($1->>'timestamp', 1, 19))*1000000 + SUBSTR($1->>'timestamp', 21, 6)), $2, jsonb_extract($1, '$.' || $2)) \
                ON CONFLICT DO NOTHING")
                .bind(&event.data)
                .bind(&event.type_)
                .execute(&mut **transaction)
                .await
                .map(|r| r.rows_affected()),
        }?;
        inserted = inserted.saturating_add(count);
    }
    Ok(inserted)
}

/// Add events to the PostgreSQL database
async fn write_batch_postgres(
    transaction: &mut Transaction<'_, sqlx::Postgres>,
    events: &[EveEvent],
) -> Result<u64, sqlx::Error> {
    let mut inserted = 0u64;
    // TODO: rewrite this to use 3 queries with UNNEST
    for event in events {
        let count = match event.type_.as_str() {
            "flow" => {
                let (src_ip, dest_ip) = sc_ip_format(&event.data);
                sqlx::query(
                    "INSERT INTO flow (id, ts_start, ts_end, src_ip, src_port, dest_ip, dest_port, proto, app_proto, metadata, extra_data) \
                    VALUES (($1::json->>'flow_id')::bigint, EXTRACT(EPOCH FROM ($1::json#>>'{flow,start}')::timestamp) * 1000000, \
                    EXTRACT(EPOCH FROM ($1::json#>>'{flow,end}')::timestamp) * 1000000, $2, ($1::json->>'src_port')::int, $3, ($1::json->>'dest_port')::int, \
                    $1::json->>'proto', $1::json->>'app_proto', $1::json->'metadata', $1::json->'flow') ON CONFLICT DO NOTHING")
                .bind(&event.data)
                .bind(src_ip)
                .bind(dest_ip)
                .execute(&mut **transaction)
                .await
                .map(|r| r.rows_affected())
            },
            "alert" => sqlx::query(
                "WITH vars AS (SELECT $1::json->$2 AS extra_data) INSERT INTO alert (flow_id, tag, color, timestamp, extra_data) \
                SELECT ($1::json->>'flow_id')::bigint, COALESCE(vars.extra_data#>>'{metadata,tag,0}', ''), (vars.extra_data#>>'{metadata,color,0}'), \
                EXTRACT(EPOCH FROM ($1::json->>'timestamp')::timestamp) * 1000000, vars.extra_data FROM vars ON CONFLICT DO NOTHING")
                .bind(&event.data)
                .bind(&event.type_)
                .execute(&mut **transaction)
                .await
                .map(|r| r.rows_affected()),
            "stats" => sqlx::query(
                "INSERT INTO stats (timestamp, extra_data) \
                VALUES (EXTRACT(EPOCH FROM ($1::json->>'timestamp')::timestamp) * 1000000, $1::json->'stats') ON CONFLICT DO NOTHING")
                .bind(&event.data)
                .execute(&mut **transaction)
                .await
                .map(|r| r.rows_affected()),
            _ => sqlx::query(
                "INSERT INTO \"other-event\" (flow_id, timestamp, event_type, extra_data) \
                VALUES (($1::json->>'flow_id')::bigint, EXTRACT(EPOCH FROM ($1::json->>'timestamp')::timestamp) * 1000000, $2, $1::json->$2) ON CONFLICT DO NOTHING")
                .bind(&event.data).bind(&event.type_).execute(&mut **transaction)
                .await
                .map(|r| r.rows_affected()),
        }?;
        inserted = inserted.saturating_add(count);
    }
    Ok(inserted)
}

enum DatabaseConnection {
    Sqlite(sqlx::sqlite::SqliteConnection),
    Postgres(sqlx::postgres::PgConnection),
}

pub struct Database {
    runtime: Option<tokio::runtime::Runtime>,
    conn: Option<DatabaseConnection>,
    rx: std::sync::mpsc::Receiver<EveEvent>,
    count_batch: usize,
    count_incoming: usize,
    count_inserted: u64,
}

impl Database {
    /// Init database
    pub fn new(url: &str, rx: std::sync::mpsc::Receiver<EveEvent>) -> Result<Self, sqlx::Error> {
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
    async fn batch_write_events(&mut self) -> Result<(), sqlx::Error> {
        while let Ok(buffer) = self.rx.recv() {
            let mut batch = vec![buffer];
            batch.extend(self.rx.try_iter()); // Drain channel
            self.count_batch = self.count_batch.saturating_add(1);
            self.count_incoming = self.count_incoming.saturating_add(batch.len());

            // Insert batch in database
            let inserted = match self.conn.as_mut().unwrap() {
                DatabaseConnection::Sqlite(conn) => {
                    let mut transaction = conn.begin().await?;
                    let inserted = write_batch_sqlite(&mut transaction, &batch).await?;
                    transaction.commit().await?;
                    inserted
                }
                DatabaseConnection::Postgres(conn) => {
                    let mut transaction = conn.begin().await?;
                    let inserted = write_batch_postgres(&mut transaction, &batch).await?;
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
            if let Err(err) = self.batch_write_events().await {
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
