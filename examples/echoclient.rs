use anyhow::Result;
use std::{env, net::Ipv4Addr, io};
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
    loop{
        let mut input = String::new();
        io::stdin().read_line(&mut input);

        socket.send(input.as_bytes())?;
    }
    Ok(())
}
