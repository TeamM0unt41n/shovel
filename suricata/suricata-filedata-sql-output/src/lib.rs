// Copyright (C) 2025  A. Iooss
// SPDX-License-Identifier: GPL-2.0-or-later

mod database;
mod ffi;

use std::collections::HashMap;
use std::fmt::Debug;
use std::fmt::Write as _;
use std::os::raw::{c_int, c_void};
use std::sync::mpsc;
use suricata_sys::sys::{SC_API_VERSION, SC_PACKAGE_VERSION, SCPlugin};

// Default configuration values.
const DEFAULT_DATABASE_URL: &str = "sqlite://./output_suricata/filedata.db?mode=rwc";
const DEFAULT_BUFFER_SIZE: usize = 1000;

#[derive(Debug, Clone)]
struct Config {
    database_url: String,
    buffer: usize,
}

impl Config {
    fn new() -> Self {
        Self {
            database_url: std::env::var("FILEDATA_DATABASE_URL")
                .unwrap_or_else(|_| DEFAULT_DATABASE_URL.into()),
            buffer: std::env::var("FILEDATA_BUFFER")
                .unwrap_or_else(|_| DEFAULT_BUFFER_SIZE.to_string())
                .parse()
                .unwrap_or(DEFAULT_BUFFER_SIZE),
        }
    }
}

struct Filedata {
    name: String,
    original_size: i64,
    data: Vec<u8>, // might be compressed
}

struct Context {
    tx: mpsc::SyncSender<Filedata>,
    filedata_blob: HashMap<u32, Vec<u8>>,
}

extern "C" fn filedata_log(
    _thread_vars: *mut *mut c_void, // ThreadVars *
    thread_data: *mut *mut c_void,
    _p: *const *mut c_void, // Packet *
    ff: *mut ffi::File,
    _tx: *mut *mut c_void,
    _tx_id: u64,
    data: *const u8,
    data_len: u32,
    flags: u8,
    _dir: u8,
) -> c_int {
    // Handle FFI arguments, Suricata owns the data
    let ff = unsafe { ff.as_mut() }.expect("null ff pointer");
    let data_slice = unsafe { std::slice::from_raw_parts(data, data_len as usize) };

    // Write data blob to temporary buffer
    let context =
        unsafe { thread_data.cast::<Context>().as_mut() }.expect("null thread_data pointer");
    match context.filedata_blob.get_mut(&ff.file_store_id) {
        Some(pending_blob) => {
            pending_blob.extend_from_slice(data_slice);
        }
        None => {
            context
                .filedata_blob
                .insert(ff.file_store_id, data_slice.to_owned());
        }
    }

    if flags & ffi::OUTPUT_FILEDATA_FLAG_CLOSE != 0 {
        // Got last part of data, compress then send filedata to database thread
        if let Some(blob) = context.filedata_blob.remove(&ff.file_store_id) {
            let name = ff.sha256.iter().fold(String::new(), |mut output, b| {
                let _ = write!(output, "{b:02x}");
                output
            });
            let original_size = blob.len().try_into().unwrap_or(0i64);
            let data = if original_size < 256 {
                // Do not compress smaller blobs
                blob
            } else {
                // Compress using deflate
                let mut buf = vec![0u8; zlib_rs::compress_bound(blob.len())];
                match zlib_rs::compress_slice(&mut buf, &blob, zlib_rs::DeflateConfig::best_speed())
                {
                    (_, zlib_rs::ReturnCode::Ok) => buf,
                    _ => blob,
                }
            };

            let filedata = Filedata {
                name,
                original_size,
                data,
            };
            // Block until there is space in database buffer
            if let Err(err) = context.tx.send(filedata) {
                panic!("Database thread is no longer alive: {err:?}");
            }
        }
    }
    0
}

extern "C" fn filedata_thread_init(
    _thread_vars: *mut *mut c_void, // ThreadVars *
    _initdata: *const *mut c_void,
    thread_data: *mut *mut c_void,
) -> c_int {
    // Load configuration
    let config = Config::new();

    // Create thread context
    let (tx, rx) = mpsc::sync_channel(config.buffer);
    let mut database_client = match database::Database::new(&config.database_url, rx) {
        Ok(db) => db,
        Err(err) => panic!("Failed to open database: {err:?}"),
    };
    std::thread::spawn(move || database_client.run());
    let context_ptr = Box::into_raw(Box::new(Context {
        tx,
        filedata_blob: HashMap::new(),
    }));

    unsafe {
        *thread_data = context_ptr.cast();
    }
    0
}

extern "C" fn filedata_thread_deinit(
    _thread_vars: *mut *mut c_void,
    thread_data: *mut *mut c_void,
) {
    let context = unsafe { Box::from_raw(thread_data.cast::<Context>()) };
    log::debug!("SQL filedata output finished");
    std::mem::drop(context);
}

extern "C" fn plugin_init() {
    // Init Rust logger
    // don't log using `suricata` crate to reduce build time.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Force filestore in engine
    unsafe {
        ffi::FileForceFilestoreEnable();
        ffi::FileForceSha256Enable();
        ffi::ProvidesFeature(c"output::file-store".as_ptr());
    }

    // Register new filedata logger
    if !unsafe {
        ffi::SCOutputRegisterFiledataLogger(
            ffi::LOGGER_USER,
            c"filedata-sql".as_ptr(),
            filedata_log,
            std::ptr::null_mut(),
            filedata_thread_init,
            filedata_thread_deinit,
        )
    } == 0
    {
        log::error!("Failed to register SQL filedata plugin");
    }
}

/// Plugin entrypoint, registers [`plugin_init`] function in Suricata
#[unsafe(no_mangle)]
extern "C" fn SCPluginRegister() -> *const SCPlugin {
    let plugin = SCPlugin {
        version: SC_API_VERSION,
        suricata_version: SC_PACKAGE_VERSION.as_ptr().cast::<::std::os::raw::c_char>(),
        name: c"Filedata SQL Output".as_ptr(),
        plugin_version: c"0.1.0".as_ptr(),
        license: c"GPL-2.0".as_ptr(),
        author: c"ECSC TeamFrance".as_ptr(),
        Init: Some(plugin_init),
    };
    Box::into_raw(Box::new(plugin))
}
