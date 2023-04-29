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
    println!("{:?}:{:?}", local_addr, local_port);
    let mut s = Socket::new()?;
    let socket = s.listen(local_addr, local_port)?;
    println!(
        "listening at {:?}:{:?}",
        socket.local_addr, socket.local_port
    );
    loop {}
    Ok(())
}
