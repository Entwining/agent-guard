#![forbid(unsafe_code)]

pub mod cli;
pub mod lifecycle;
pub mod model;
pub mod process;
pub mod runtime;
pub mod runtime_driver;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;
