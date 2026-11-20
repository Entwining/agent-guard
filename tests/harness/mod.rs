#![forbid(unsafe_code)]

pub mod cli;
pub mod lifecycle;
pub mod model;
pub mod process;
pub mod registration;
pub mod runtime;
pub mod runtime_driver;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = std::result::Result<T, Error>;

pub fn sha256_hex(body: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(body)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
