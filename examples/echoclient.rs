use anyhow::Result;
use std::{env, net::Ipv4Addr};
use toytcp::socket::Socket;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let addr: Ipv4Addr = args[1].parse()?;
    let port: u16 = args[2].parse()?;
    echo_client(addr, port)?;
    Ok(())
}

fn echo_client(remote_addr: Ipv4Addr, remote_port: u16) -> Result<()> {
    let socket = Socket::connect(remote_addr, remote_port)?;
    println!(
        "{:?}:{:?} -> {:?}:{:?}",
        socket.sock_id.local_addr,
        socket.sock_id.local_port,
        socket.sock_id.remote_addr,
        socket.sock_id.remote_port
    );
    Ok(())
}
