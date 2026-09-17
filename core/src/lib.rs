//! EasyClean core — the headless engine: config, storage model, database,
//! scanning, thumbnails, detection (blur + perceptual-hash duplicates), file
//! operations (trash / clean / purge), and resumable uploads.

pub mod auth;
pub mod categorize;
pub mod config;
pub mod db;
pub mod decode;
pub mod detect;
pub mod embed;
pub mod error;
pub mod fileops;
pub mod models;
pub mod scan;
pub mod storage;
pub mod thumbs;
pub mod upload;

pub use config::Config;
pub use error::{Error, Result};
