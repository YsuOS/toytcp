use crate::{packet::TcpPacket, socket::SockId, tcpflags};
use anyhow::{Ok, Result};
use pnet::{
    packet::ip::IpNextHeaderProtocols,
    transport::{self, TransportChannelType, TransportProtocol},
};
use rand::random;
use std::{collections::VecDeque, net::IpAddr, sync::Mutex, time::SystemTime};

// How much data can be buffered on the socket
const SOCKET_BUFFER_SIZE: usize = 4380;

// How much data can be "in flight" on the network
const WINDOW_SIZE: u16 = SOCKET_BUFFER_SIZE as u16;

const MAX_PACKET_SIZE: usize = 65535;

#[derive(Debug, PartialEq)]
pub enum TcpStatus {
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

#[derive(Debug)]
pub struct Sock {
    pub sock_id: SockId,
    pub status: TcpStatus,
    pub send_params: SendParams,
    pub recv_params: ReceiveParams,
    pub recv_buffer: Vec<u8>,
    pub retransmission_queue: Mutex<VecDeque<RetransmissionQueue>>,
}

impl Sock {
    pub fn new(sock_id: SockId) -> Result<Self> {
        Ok(Self {
            sock_id,
            status: TcpStatus::Closed,
            send_params: SendParams {
                next: 0,
                una: 0,
                window: WINDOW_SIZE,
            },
            recv_params: ReceiveParams {
                next: 0,
                tail: 0,
                window: WINDOW_SIZE,
            },
            recv_buffer: vec![0; SOCKET_BUFFER_SIZE],
            retransmission_queue: Mutex::new(VecDeque::new()),
        })
    }

    pub fn init_seq(&mut self) -> Result<()> {
        self.send_params.next = random();
        self.send_params.una = self.send_params.next;
        Ok(())
    }

    fn send_tcp_packet(
        &self,
        flag: u8,
        seq: u32,
        ack: u32,
        win: u16,
        payload: &[u8],
    ) -> Result<()> {
        let mut packet = TcpPacket::new(payload.len());
        packet.set_src(self.sock_id.local_port);
        packet.set_dst(self.sock_id.remote_port);
        packet.set_flag(flag);
        packet.set_data_offset();
        packet.set_seq(seq);
        packet.set_ack(ack);
        packet.set_window_size(win);
        packet.set_payload(payload);
        packet.set_checksum(self.sock_id.local_addr, self.sock_id.remote_addr);
        let (mut sender, _) = transport::transport_channel(
            MAX_PACKET_SIZE,
            TransportChannelType::Layer4(TransportProtocol::Ipv4(IpNextHeaderProtocols::Tcp)),
        )?;
        sender.send_to(packet.clone(), IpAddr::V4(self.sock_id.remote_addr))?;

        if payload.is_empty() && packet.get_flag() == tcpflags::ACK {
            // Don't need to enqueue to retransmission queue
            return Ok(());
        }
        let mut queue = self.retransmission_queue.lock().unwrap();
        queue.push_back(RetransmissionQueue::new(packet));

        Ok(())
    }

    //TODO: Rename
    pub fn send_tcp_packet_syn(&mut self, flag: u8, status: TcpStatus) {
        self.send_tcp_packet_common(flag, status);
        self.send_params.next += 1;
    }

    //TODO: Rename
    pub fn send_tcp_packet_fin(&mut self, flag: u8) -> Result<()> {
        self.send_tcp_packet(
            flag,
            self.send_params.next,
            self.recv_params.next,
            self.recv_params.window,
            &[],
        )
        .unwrap();
        self.send_params.next += 1;

        match self.status {
            TcpStatus::Established => {
                self.status = TcpStatus::FinWait1;
            }
            TcpStatus::CloseWait => {
                self.status = TcpStatus::LastAck;
            }
            _ => return Ok(()),
        }
        Ok(())
    }

    //TODO: Rename
    pub fn send_tcp_packet_ack(&mut self, flag: u8, status: TcpStatus) {
        self.send_tcp_packet_common(flag, status);
    }

    //TODO: Rename
    pub fn send_tcp_packet_send(&mut self, flag: u8, payload: &[u8]) {
        self.send_tcp_packet(
            flag,
            self.send_params.next,
            self.recv_params.next,
            self.recv_params.window,
            payload,
        )
        .unwrap();
        self.send_params.next += payload.len() as u32;
        self.send_params.window -= payload.len() as u16;
    }

    //TODO: Rename
    fn send_tcp_packet_common(&mut self, flag: u8, status: TcpStatus) {
        self.send_tcp_packet(
            flag,
            self.send_params.next,
            self.recv_params.next,
            self.recv_params.window,
            &[],
        )
        .unwrap();
        self.status = status;
        dbg!(&self.status);
    }
}

#[derive(Debug)]
pub struct SendParams {
    pub next: u32,
    pub una: u32,
    pub window: u16,
}

#[derive(Debug)]
pub struct ReceiveParams {
    pub next: u32,
    pub tail: u32,
    pub window: u16,
}

#[derive(Debug)]
pub struct RetransmissionQueue {
    pub packet: TcpPacket,
    pub latest_transmission_time: SystemTime,
    pub transmission_count: u8,
}

impl RetransmissionQueue {
    fn new(packet: TcpPacket) -> Self {
        Self {
            packet,
            latest_transmission_time: SystemTime::now(),
            transmission_count: 1,
        }
    }
}
