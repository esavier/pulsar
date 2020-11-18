#![allow(unused_imports)]
pub use log::{debug, info, trace, error, warn};
use env_logger::{Builder, Env};
use fern;

use std::error::Error;

pub fn setup_logging() -> Result<(), Box<dyn Error>> {
    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "{}[{}][{}] {}",
                chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                record.target(),
                record.level(),
                message
                ))
            })
    // .level(log::LevelFilter::Debug)
    .level(log::LevelFilter::Error)
    // .level_for("hyper", log::LevelFilter::Info)
    .chain(std::io::stdout())
    .chain(fern::log_file("pulsar.log")?)
    .apply()?;
    Ok(())
}
