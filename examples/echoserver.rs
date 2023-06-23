use anyhow::Result;
use std::{net::Ipv4Addr, str};
use toytcp::{parse_args, socket::Socket};

fn main() -> Result<()> {
    let (addr, port) = parse_args()?;

    echo_server(addr, port)?;
    Ok(())
}

fn echo_server(local_addr: Ipv4Addr, local_port: u16) -> Result<()> {
    let socket = Socket::new();
    socket.listen(local_addr, local_port)?;
    loop {
        let connected_sock_id = socket.accept()?;
        dbg!(&connected_sock_id);
        let cloned_socket = socket.clone();

        std::thread::spawn(move || {
            let mut buffer = [0; 1024];
            loop {
                let nbytes = cloned_socket.recv(connected_sock_id, &mut buffer).unwrap();
                if nbytes == 0 {
                    dbg!("closing connection");
                    cloned_socket.close(connected_sock_id).unwrap();
                    return;
                }
                print!("> {}", str::from_utf8(&buffer[..nbytes]).unwrap());
                cloned_socket
                    .send(connected_sock_id, &buffer[..nbytes])
                    .unwrap();
            }
        });
    }
}
