use crate::packet::TCPPacket;
use crate::tcpflags;
use anyhow::Result;
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::transport::{self, TransportChannelType, TransportProtocol, TransportSender};
use std::net::{IpAddr, Ipv4Addr};

//const UNDETERMINED_IP_ADDR: std::net::Ipv4Addr = Ipv4Addr::new(0, 0, 0, 0);
//const UNDETERMINED_PORT: u16 = 0;
//const MAX_TRANSMITTION: u8 = 5;
//const RETRANSMITTION_TIMEOUT: u64 = 3;
//const MSS: usize = 1460;
//const PORT_RANGE: Range<u16> = 40000..60000;

// how much data can be buffered on the socket
//const SOCKET_BUFFER_SIZE: usize = 4380;

const MAX_PACKET_SIZE: usize = 65535;

const LOCAL_ADDR: Ipv4Addr = Ipv4Addr::new(10, 0, 0, 1);
const LOCAL_PORT: u16 = 33445;

pub struct Socket {
    sock_id: SockId,
    sender: TransportSender,
}

impl Socket {
    pub fn new(remote_addr: Ipv4Addr, remote_port: u16) -> Result<Self> {
        let sock_id = SockId::new(remote_addr, remote_port);
        let (sender, _) = transport::transport_channel(
            MAX_PACKET_SIZE,
            TransportChannelType::Layer4(TransportProtocol::Ipv4(IpNextHeaderProtocols::Tcp)),
        )?;
        Ok(Self { sock_id, sender })
    }

    pub fn connect(&mut self) -> Result<()> {
        self.send_tcp_packet(tcpflags::SYN, &[])?;
        Ok(())
    }

    fn send_tcp_packet(&mut self, flag: u8, payload: &[u8]) -> Result<()> {
        let mut packet = TCPPacket::new(payload.len());
        packet.set_src(self.sock_id.local_port);
        packet.set_dst(self.sock_id.remote_port);
        packet.set_flag(flag);
        self.sender.send_to(packet, IpAddr::V4(self.sock_id.remote_addr));
        Ok(())
    }
}

struct SockId {
    local_addr: Ipv4Addr,
    remote_addr: Ipv4Addr,
    local_port: u16,
    remote_port: u16,
}

impl SockId {
    fn new(remote_addr: Ipv4Addr, remote_port: u16) -> Self {
        Self {
            local_addr: LOCAL_ADDR,
            remote_addr,
            local_port: LOCAL_PORT,
            remote_port,
        }
    }
}
