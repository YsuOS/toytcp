use anyhow::Result;
use std::{net::Ipv4Addr, str};
use toytcp::{
    parse_args,
    socket::{SockId, Socket},
};

fn main() -> Result<()> {
    let (addr, port) = parse_args()?;

    echo_server(addr, port)?;
    Ok(())
}

fn echo_server(local_addr: Ipv4Addr, local_port: u16) -> Result<()> {
    let socket = Socket::new();
    socket.listen(local_addr, local_port)?;
    //let sock_id = SockId::listen(local_addr, local_port)?;
    loop {
        let cloned_socket = socket.clone();
        cloned_socket.accept()?;
        dbg!("Accepted");
        std::thread::sleep(std::time::Duration::from_secs(100));
//        let cloned_sock_id = sock_id.clone();
//        let connected_socket = Socket::accept(cloned_sock_id)?;
//        std::thread::spawn(move || {
//            let mut buffer = [0; 1024];
//            loop {
//                let nbytes = connected_socket.recv(&mut buffer).unwrap();
//                if nbytes == 0 {
//                    dbg!("closing connection");
//                    connected_socket.close().unwrap();
//                    return;
//                }
//                print!("> {}", str::from_utf8(&buffer[..nbytes]).unwrap());
//                connected_socket.send(&buffer[..nbytes]).unwrap();
//            }
//        });
    }
}
