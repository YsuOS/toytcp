use crate::packet::TcpPacket;
use crate::tcpflags;
use anyhow::Result;
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::{util, Packet};
use pnet::transport::{
    self, TransportChannelType, TransportProtocol, TransportReceiver, TransportSender,
};
use rand::random;
use std::net::{IpAddr, Ipv4Addr};

//const UNDETERMINED_IP_ADDR: std::net::Ipv4Addr = Ipv4Addr::new(0, 0, 0, 0);
//const UNDETERMINED_PORT: u16 = 0;
//const MAX_TRANSMITTION: u8 = 5;
//const RETRANSMITTION_TIMEOUT: u64 = 3;
//const MSS: usize = 1460;
//const PORT_RANGE: Range<u16> = 40000..60000;

// How much data can be buffered on the socket
const SOCKET_BUFFER_SIZE: usize = 4380;

// How much data can be "in flight" on the network
// TODO: Use the same value with a socket buffer size as a temporary value
const WINDOW_SIZE: usize = SOCKET_BUFFER_SIZE;

const MAX_PACKET_SIZE: usize = 65535;

const LOCAL_ADDR: Ipv4Addr = Ipv4Addr::new(10, 0, 0, 1);
const LOCAL_PORT: u16 = 33445;

enum TcpStatus {
    Closed,
    Listen,
    SynSent,
    SynRcvd,
    Established,
    FinWait1,
    FinWait2,
    TimeWait,
    CloseWait,
    LastAck,
}

pub struct Socket {
    sock_id: SockId,
    sender: TransportSender,
    receiver: TransportReceiver,
    status: TcpStatus,
    send_params: SendParams,
    recv_params: ReceiveParams,
}

impl Socket {
    pub fn new(remote_addr: Ipv4Addr, remote_port: u16) -> Result<Self> {
        let sock_id = SockId::new(remote_addr, remote_port);
        let (sender, receiver) = transport::transport_channel(
            MAX_PACKET_SIZE,
            TransportChannelType::Layer4(TransportProtocol::Ipv4(IpNextHeaderProtocols::Tcp)),
        )?;
        Ok(Self {
            sock_id,
            sender,
            receiver,
            status: TcpStatus::Closed,
            send_params: SendParams { next: 0 },
            recv_params: ReceiveParams { next: 0 },
        })
    }

    pub fn connect(&mut self) -> Result<SockId> {
        self.send_params.next = random();

        self.send_tcp_packet(tcpflags::SYN, self.send_params.next, 0, &[])?;
        self.status = TcpStatus::SynSent;
        self.wait_tcp_packet(tcpflags::SYN | tcpflags::ACK)?;
        self.send_tcp_packet(tcpflags::ACK, 0, self.recv_params.next, &[])?;
        self.status = TcpStatus::Established;
        Ok(self.sock_id)
    }

    fn send_tcp_packet(&mut self, flag: u8, seq: u32, ack: u32, payload: &[u8]) -> Result<()> {
        let mut packet = TcpPacket::new(payload.len());
        packet.set_src(self.sock_id.local_port);
        packet.set_dst(self.sock_id.remote_port);
        packet.set_flag(flag);
        packet.set_data_offset();
        packet.set_seq(seq);
        packet.set_ack(ack);
        packet.set_window_size(WINDOW_SIZE as u16);
        packet.set_checksum(util::ipv4_checksum(
            &packet.packet(),
            8,
            &[],
            &self.sock_id.local_addr,
            &self.sock_id.remote_addr,
            IpNextHeaderProtocols::Tcp,
        ));
        self.sender
            .send_to(packet, IpAddr::V4(self.sock_id.remote_addr))?;
        println!("DEBUG: send {:?} !", flag);
        Ok(())
    }

    fn wait_tcp_packet(&mut self, flag: u8) -> Result<()> {
        let mut packet_iter = transport::tcp_packet_iter(&mut self.receiver);
        loop {
            let (packet, _) = packet_iter.next().unwrap();
            let packet = TcpPacket::from(packet);
            if packet.get_flag() == flag {
                self.recv_params.next = packet.get_seq() + 1;
                self.send_params.next = packet.get_ack();
                break;
            }
        }
        println!("DEBUG: receive SYN/ACK !");
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub struct SockId {
    pub local_addr: Ipv4Addr,
    pub remote_addr: Ipv4Addr,
    pub local_port: u16,
    pub remote_port: u16,
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

struct SendParams {
    next: u32,
}

struct ReceiveParams {
    next: u32,
}
