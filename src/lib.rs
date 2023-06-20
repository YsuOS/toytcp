use anyhow::Result;
use std::{env, net::Ipv4Addr};

mod packet;
pub mod socket;
mod sock;
mod tcpflags;

pub fn parse_args() -> Result<(Ipv4Addr, u16)> {
    let args: Vec<String> = env::args().collect();
    let addr: Ipv4Addr = args[1].parse()?;
    let port: u16 = args[2].parse()?;

    Ok((addr, port))
}
