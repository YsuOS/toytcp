use anyhow::Result;
use std::{io, net::Ipv4Addr, str};
use toytcp::{parse_args, socket::Socket};

fn main() -> Result<()> {
    let (addr, port) = parse_args()?;

    echo_client(addr, port)?;
    Ok(())
}

fn echo_client(remote_addr: Ipv4Addr, remote_port: u16) -> Result<()> {
    let socket = Socket::new();
    let sock_id = socket.connect(remote_addr, remote_port)?;
    dbg!(sock_id);

    //    let cloned_socket = socket.clone();
    //    ctrlc::set_handler(move || {
    //        cloned_socket.close().unwrap();
    //        std::thread::sleep(std::time::Duration::from_secs(1));
    //        std::process::exit(0);
    //    })?;
    loop {
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        socket.send(sock_id, input.as_bytes())?;
        // test for sliding window
        //loop {
        //    socket.send(input.repeat(2000).as_bytes())?;
        //}

        let mut buffer = vec![0; 1500];
        let n = socket.recv(sock_id, &mut buffer)?;
        print!("> {}", str::from_utf8(&buffer[..n])?);
    }
    //Ok(())
}
