use anyhow::Result;
use std::{env, net::Ipv4Addr};
use toytcp::socket::Socket;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let addr: Ipv4Addr = args[1].parse()?;
    let port: u16 = args[2].parse()?;
    echo_server(addr, port)?;
    Ok(())
}

fn echo_server(local_addr: Ipv4Addr, local_port: u16) -> Result<()> {
    let socket = Socket::listen(local_addr, local_port)?;
    {
        let sock_id = socket.sock_id.read().unwrap();
        println!(
            "listening at {:?}:{:?}",
            sock_id.local_addr, sock_id.local_port
        );
    }
    socket.accept()?;
    println!("Accepted");

    Ok(())
}
