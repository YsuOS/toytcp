use crate::packet::TcpPacket;
use crate::tcpflags;
use anyhow::Result;
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::Packet;
use pnet::transport::{self, TransportChannelType, TransportProtocol};
use rand::{random, Rng};
use std::net::{IpAddr, Ipv4Addr};
use std::ops::Range;
use std::sync::{Arc, Mutex};
use std::thread;

//const UNDETERMINED_ADDR: Ipv4Addr = Ipv4Addr::new(0, 0, 0, 0);
//const UNDETERMINED_PORT: u16 = 0;
//const MAX_TRANSMITTION: u8 = 5;
//const RETRANSMITTION_TIMEOUT: u64 = 3;
//const MSS: usize = 1460;
//const PORT_RANGE: Range<u16> = 40000..60000;

// How much data can be buffered on the socket
const SOCKET_BUFFER_SIZE: usize = 4380;

// How much data can be "in flight" on the network
const WINDOW_SIZE: u16 = SOCKET_BUFFER_SIZE as u16;

const MAX_PACKET_SIZE: usize = 65535;

const LOCAL_ADDR: Ipv4Addr = Ipv4Addr::new(10, 0, 0, 1);
const PORT_RANGE: Range<u16> = 40000..60000;

#[derive(PartialEq)]
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
    pub sock_id: SockId,
    tcb: Mutex<Tcb>,
}

impl Socket {
    pub fn new(
        local_addr: Ipv4Addr,
        local_port: u16,
        remote_addr: Ipv4Addr,
        remote_port: u16,
    ) -> Arc<Self> {
        let sock_id = SockId {
            local_addr,
            remote_addr,
            local_port,
            remote_port,
        };
        let socket = Arc::new(Self {
            sock_id,
            tcb: Mutex::new(Tcb {
                status: TcpStatus::Closed,
                send_params: SendParams {
                    next: 0,
                    una: 0,
                    window: WINDOW_SIZE,
                },
                recv_params: ReceiveParams { next: 0 },
            }),
        });
        let cloned_socket = socket.clone();
        thread::spawn(move || {
            loop {
                let tcb = cloned_socket.tcb.lock().unwrap();
                if tcb.status == TcpStatus::SynSent {
                    break;
                }
            }
            cloned_socket.wait_tcp_packet(tcpflags::SYN | tcpflags::ACK)
        });
        socket
    }

    pub fn connect(remote_addr: Ipv4Addr, remote_port: u16) -> Result<Arc<Socket>> {
        let socket = Socket::new(
            LOCAL_ADDR,
            set_unsed_port().unwrap(),
            remote_addr,
            remote_port,
        );
        {
            let mut tcb = socket.tcb.lock().unwrap();
            tcb.send_params.next = random();
            tcb.send_params.una = tcb.send_params.next;
            socket.send_tcp_packet(
                tcpflags::SYN,
                tcb.send_params.next,
                0,
                tcb.send_params.window,
                &[],
            )?;
            tcb.status = TcpStatus::SynSent;
            tcb.send_params.next += 1;
        }
        loop {
            let tcb = socket.tcb.lock().unwrap();
            if tcb.status == TcpStatus::Established {
                break;
            }
        }
        Ok(socket)
    }

    //    pub fn listen(&mut self, local_addr: Ipv4Addr, local_port: u16) -> Result<&SockId> {
    //        self.sock_id.local_addr = local_addr;
    //        self.sock_id.local_port = local_port;
    //        //self.status = TcpStatus::Listen;
    //
    //        Ok(&self.sock_id)
    //    }

    //    pub fn accept(&mut self) -> Result<&SockId> {
    //        //        self.send_params.next = random();
    //        //        self.send_params.una = self.send_params.next;
    //        //
    //        //        self.wait_tcp_packet2(tcpflags::SYN)?;
    //        //        //self.status = TcpStatus::SynRcvd;
    //        //        self.send_tcp_packet(
    //        //            tcpflags::SYN | tcpflags::ACK,
    //        //            self.send_params.next,
    //        //            self.recv_params.next,
    //        //            &[],
    //        //        )?;
    //        //        // FIXME: accept() will never return if ACK receives before calling wait_tcp_packet()
    //        //        // Need to receive thread.
    //        //        self.wait_tcp_packet2(tcpflags::ACK)?;
    //        //        //self.status = TcpStatus::Established;
    //        //
    //        Ok(&self.sock_id)
    //    }

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
        packet.set_checksum(self.sock_id.local_addr, self.sock_id.remote_addr);
        let (mut sender, _) = transport::transport_channel(
            MAX_PACKET_SIZE,
            TransportChannelType::Layer4(TransportProtocol::Ipv4(IpNextHeaderProtocols::Tcp)),
        )?;
        sender.send_to(packet, IpAddr::V4(self.sock_id.remote_addr))?;
        println!("DEBUG: send {:?} !", flag);
        Ok(())
    }

    fn wait_tcp_packet(&self, flag: u8) -> Result<()> {
        let (_, mut receiver) = transport::transport_channel(
            MAX_PACKET_SIZE,
            TransportChannelType::Layer3(IpNextHeaderProtocols::Tcp),
        )?;
        let mut packet_iter = transport::ipv4_packet_iter(&mut receiver);
        loop {
            let (packet, _) = packet_iter.next().unwrap();
            let local_addr = packet.get_destination();
            let remote_addr = packet.get_source();
            let packet =
                TcpPacket::from(pnet::packet::tcp::TcpPacket::new(packet.payload()).unwrap());
            if !packet.is_correct_checksum(local_addr, remote_addr) {
                println!("invalid checksum");
            }
            if packet.get_flag() == flag {
                let mut tcb = self.tcb.lock().unwrap();
                tcb.recv_params.next = packet.get_seq() + 1;
                tcb.send_params.una = packet.get_ack();
                tcb.send_params.window = packet.get_window_size();
                if tcb.send_params.una != tcb.send_params.next {
                    println!("SND.NXT don't match SND.UNA!");
                }
                break;
            }
        }
        println!("DEBUG: receive SYN/ACK !");
        let mut tcb = self.tcb.lock().unwrap();
        self.send_tcp_packet(
            tcpflags::ACK,
            tcb.send_params.next,
            tcb.recv_params.next,
            tcb.send_params.window,
            &[],
        )?;
        tcb.status = TcpStatus::Established;
        println!("DEBUG: Send ACK !");
        Ok(())
    }

    //    fn wait_tcp_packet2(&mut self, flag: u8) -> Result<()> {
    //        let (_, mut receiver) = transport::transport_channel(
    //            MAX_PACKET_SIZE,
    //            TransportChannelType::Layer3(IpNextHeaderProtocols::Tcp),
    //        )?;
    //        let mut packet_iter = transport::ipv4_packet_iter(&mut receiver);
    //        loop {
    //            let (packet, _) = packet_iter.next().unwrap();
    //            let local_addr = packet.get_destination();
    //            let remote_addr = packet.get_source();
    //            self.sock_id.remote_addr = remote_addr;
    //
    //            let packet =
    //                TcpPacket::from(pnet::packet::tcp::TcpPacket::new(packet.payload()).unwrap());
    //            self.sock_id.remote_port = packet.get_src();
    //
    //            if !packet.is_correct_checksum(local_addr, remote_addr) {
    //                println!("invalid checksum");
    //            }
    //            if packet.get_flag() == flag {
    //                self.recv_params.next = packet.get_seq() + 1;
    //                //self.send_params.una = packet.get_ack();
    //                //self.send_params.window = packet.get_window_size();
    //                //if self.send_params.una != self.send_params.next {
    //                //    println!("SND.NXT don't match SND.UNA!");
    //                //}
    //                break;
    //            }
    //        }
    //        println!("DEBUG: receive SYN or ACK!");
    //        Ok(())
    //    }
}

pub struct SockId {
    pub local_addr: Ipv4Addr,
    pub remote_addr: Ipv4Addr,
    pub local_port: u16,
    pub remote_port: u16,
}

fn set_unsed_port() -> Result<u16> {
    let mut rng = rand::thread_rng();
    Ok(rng.gen_range(PORT_RANGE))
}

struct Tcb {
    status: TcpStatus,
    send_params: SendParams,
    recv_params: ReceiveParams,
}

struct SendParams {
    next: u32,
    una: u32,
    window: u16,
}

struct ReceiveParams {
    next: u32,
}
