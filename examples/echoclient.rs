use anyhow::Result;
use std::{env, io, net::Ipv4Addr, str};
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
    let cloned_socket = socket.clone();
    ctrlc::set_handler(move || {
        cloned_socket.close().unwrap();
        std::thread::sleep(std::time::Duration::from_secs(1));
        std::process::exit(0);
    })?;
    loop {
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        socket.send(input.as_bytes())?;
        // test for sliding window
        //loop {
        //    socket.send(input.repeat(2000).as_bytes())?;
        //}

        let mut buffer = vec![0; 1500];
        let n = socket.recv(&mut buffer)?;
        print!("> {}", str::from_utf8(&buffer[..n])?);
    }
}
