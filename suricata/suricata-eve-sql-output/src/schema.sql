-- Copyright (C) 2026  A. Iooss
-- SPDX-License-Identifier: GPL-2.0-or-later
CREATE TABLE IF NOT EXISTS "flow" (
    id BIGINT NOT NULL PRIMARY KEY,
    ts_start BIGINT,
    ts_end BIGINT,
    src_ip TEXT NOT NULL,
    src_port INTEGER,
    dest_ip TEXT NOT NULL,
    dest_port INTEGER,
    proto TEXT NOT NULL,
    app_proto TEXT,
    metadata JSONB,
    extra_data JSONB
);
CREATE TABLE IF NOT EXISTS "alert" (
    flow_id BIGINT NOT NULL,
    tag TEXT NOT NULL,
    color TEXT,
    timestamp BIGINT NOT NULL,
    extra_data JSONB,
    PRIMARY KEY(flow_id, tag, timestamp)
);
CREATE TABLE IF NOT EXISTS "other-event" (
    flow_id BIGINT NOT NULL,
    timestamp BIGINT NOT NULL,
    event_type TEXT NOT NULL,
    extra_data JSONB,
    PRIMARY KEY(flow_id, event_type, timestamp)
);
CREATE TABLE IF NOT EXISTS "stats" (
    timestamp BIGINT NOT NULL PRIMARY KEY,
    extra_data JSONB
);
CREATE INDEX IF NOT EXISTS "flow_ts_start_idx" ON flow(ts_start);
CREATE INDEX IF NOT EXISTS "flow_app_proto_idx" ON flow(app_proto);
CREATE INDEX IF NOT EXISTS "flow_src_port_idx" ON flow(src_port);
CREATE INDEX IF NOT EXISTS "flow_dest_port_idx" ON flow(dest_port);
CREATE INDEX IF NOT EXISTS "alert_tag_idx" ON alert(tag);
CREATE INDEX IF NOT EXISTS "alert_flow_id_idx" ON alert(flow_id);
CREATE INDEX IF NOT EXISTS "other-event_flow_id_idx" ON "other-event"(flow_id);
