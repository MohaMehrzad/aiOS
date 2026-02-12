//! net.port_scan — Check if a TCP port is open on a host

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Deserialize)]
struct Input {
    host: String,
    port: u16,
}

#[derive(Serialize)]
struct Output {
    open: bool,
}

pub fn execute(input: &[u8]) -> Result<Vec<u8>> {
    let input: Input = serde_json::from_slice(input).context("Invalid JSON input")?;

    let addr_str = format!("{}:{}", input.host, input.port);
    let timeout = Duration::from_secs(5);

    let open = match addr_str.to_socket_addrs() {
        Ok(addrs) => {
            let mut is_open = false;
            for addr in addrs {
                match TcpStream::connect_timeout(&addr, timeout) {
                    Ok(_stream) => {
                        is_open = true;
                        break;
                    }
                    Err(_) => continue,
                }
            }
            is_open
        }
        Err(_) => {
            // DNS resolution failed
            false
        }
    };

    let result = Output { open };
    serde_json::to_vec(&result).context("Failed to serialize output")
}
