use anyhow::Result;
use std::{env, net::Ipv4Addr, str};
use toytcp::socket::{Socket, SockId};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    let addr: Ipv4Addr = args[1].parse()?;
    let port: u16 = args[2].parse()?;
    echo_server(addr, port)?;
    Ok(())
}

fn echo_server(local_addr: Ipv4Addr, local_port: u16) -> Result<()> {
    let sock_id = SockId::listen(local_addr, local_port)?;
    loop {
        let cloned_sock_id = sock_id.clone();
        let connected_socket = Socket::accept(cloned_sock_id)?;
        std::thread::spawn(move || {
            let mut buffer = [0; 1024];
            loop {
                let nbytes = connected_socket.recv(&mut buffer).unwrap();
                if nbytes == 0 {
                    todo!();
                }
                print!("> {}", str::from_utf8(&buffer[..nbytes]).unwrap());
                connected_socket.send(&buffer[..nbytes]).unwrap();
            }
        });
    }
    Ok(())
}
