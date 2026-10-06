#![forbid(unsafe_code)]

use std::{
    fs,
    io::{self, Write},
    time::Duration,
};

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let mode = args
        .next()
        .ok_or_else(|| io::Error::other("missing fixture mode"))?;
    let receipt = args
        .next()
        .ok_or_else(|| io::Error::other("missing fixture receipt path"))?;
    fs::write(receipt, format!("ready:{}\n", std::process::id()))?;
    io::stdout().write_all(b"fixture worker ready\n")?;
    if mode == "fault" {
        std::process::exit(71);
    }
    loop {
        std::thread::sleep(Duration::from_millis(10));
    }
}
