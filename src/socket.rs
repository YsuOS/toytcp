use crate::packet::TcpPacket;
use crate::tcpflags;
use anyhow::{Ok, Result};
use pnet::packet::ip::IpNextHeaderProtocols;
use pnet::packet::Packet;
use pnet::transport::{self, TransportChannelType, TransportProtocol};
use rand::{random, Rng};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use std::{
    cmp,
    net::{IpAddr, Ipv4Addr},
    ops::Range,
    sync::{Arc, RwLock, RwLockWriteGuard},
    thread,
};

const UNDETERMINED_ADDR: Ipv4Addr = Ipv4Addr::new(0, 0, 0, 0);
const UNDETERMINED_PORT: u16 = 0;
const MAX_TRANSMITTION: u8 = 5;
const RETRANSMITTION_TIMEOUT: u64 = 3;
const MSS: usize = 1460;
//const PORT_RANGE: Range<u16> = 40000..60000;

// How much data can be buffered on the socket
const SOCKET_BUFFER_SIZE: usize = 4380;

// How much data can be "in flight" on the network
const WINDOW_SIZE: u16 = SOCKET_BUFFER_SIZE as u16;

const MAX_PACKET_SIZE: usize = 65535;

const LOCAL_ADDR: Ipv4Addr = Ipv4Addr::new(10, 0, 0, 1);
const PORT_RANGE: Range<u16> = 40000..60000;

#[derive(PartialEq)]
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

pub struct Socket {
    pub sock_id: RwLock<SockId>,
    tcb: RwLock<Tcb>,
    retransmission_queue: Mutex<VecDeque<RetransmissionQueue>>,
}

impl Socket {
    pub fn new(
        local_addr: Ipv4Addr,
        local_port: u16,
        remote_addr: Ipv4Addr,
        remote_port: u16,
        status: TcpStatus,
    ) -> Arc<Self> {
        let sock_id = SockId {
            local_addr,
            remote_addr,
            local_port,
            remote_port,
        };
        let socket = Arc::new(Self {
            sock_id: RwLock::new(sock_id),
            tcb: RwLock::new(Tcb {
                status,
                send_params: SendParams {
                    next: 0,
                    una: 0,
                    window: WINDOW_SIZE,
                },
                recv_params: ReceiveParams { next: 0 },
            }),
            retransmission_queue: Mutex::new(VecDeque::new()),
        });
        let cloned_socket = socket.clone();
        thread::spawn(move || {
            cloned_socket.wait_tcp_packet().unwrap();
        });

        let cloned_socket = socket.clone();
        thread::spawn(move || {
            cloned_socket.timer();
        });

        socket
    }

    pub fn connect(remote_addr: Ipv4Addr, remote_port: u16) -> Result<Arc<Socket>> {
        let socket = Socket::new(
            LOCAL_ADDR,
            set_unsed_port().unwrap(),
            remote_addr,
            remote_port,
            TcpStatus::SynSent,
        );
        {
            let mut tcb = socket.tcb.write().unwrap();
            tcb.send_params.next = random();
            tcb.send_params.una = tcb.send_params.next;
            socket.send_tcp_packet(
                tcpflags::SYN,
                tcb.send_params.next,
                0,
                tcb.send_params.window,
                &[],
            )?;
            tcb.send_params.next += 1;
            dbg!("Sent SYN");
        }
        loop {
            let tcb = socket.tcb.read().unwrap();
            if tcb.status == TcpStatus::Established {
                break;
            }
        }
        Ok(socket)
    }

    pub fn listen(local_addr: Ipv4Addr, local_port: u16) -> Result<Arc<Socket>> {
        let socket = Socket::new(
            local_addr,
            local_port,
            UNDETERMINED_ADDR,
            UNDETERMINED_PORT,
            TcpStatus::Listen,
        );
        Ok(socket)
    }

    pub fn accept(&self) -> Result<&Socket> {
        loop {
            let tcb = self.tcb.read().unwrap();
            if tcb.status == TcpStatus::Established {
                break;
            }
        }
        Ok(self)
    }

    pub fn send(&self, buf: &[u8]) -> Result<()> {
        let mut cursor = 0;
        while cursor < buf.len() {
            let mut send_size = {
                let tcb = self.tcb.read().unwrap();
                cmp::min(
                    MSS,
                    cmp::min(tcb.send_params.window as usize, buf.len() - cursor),
                )
            };

            while send_size == 0 {
                send_size = {
                    let tcb = self.tcb.read().unwrap();
                    cmp::min(
                        MSS,
                        cmp::min(tcb.send_params.window as usize, buf.len() - cursor),
                    )
                };
            }

            let mut tcb = self.tcb.write().unwrap();
            self.send_tcp_packet(
                tcpflags::ACK,
                tcb.send_params.next,
                tcb.recv_params.next,
                tcb.send_params.window,
                &buf[cursor..cursor + send_size],
            )?;
            cursor += send_size;
            tcb.send_params.next += send_size as u32;
            tcb.send_params.window -= send_size as u16;
            //dbg!(tcb.send_params.window, send_size);
            thread::sleep(Duration::from_millis(1));
        }
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
        let sock_id = self.sock_id.read().unwrap();
        packet.set_src(sock_id.local_port);
        packet.set_dst(sock_id.remote_port);
        packet.set_flag(flag);
        packet.set_data_offset();
        packet.set_seq(seq);
        packet.set_ack(ack);
        packet.set_window_size(win);
        packet.set_payload(payload);
        packet.set_checksum(sock_id.local_addr, sock_id.remote_addr);
        let (mut sender, _) = transport::transport_channel(
            MAX_PACKET_SIZE,
            TransportChannelType::Layer4(TransportProtocol::Ipv4(IpNextHeaderProtocols::Tcp)),
        )?;
        sender.send_to(packet.clone(), IpAddr::V4(sock_id.remote_addr))?;

        if payload.is_empty() && packet.get_flag() == tcpflags::ACK {
            // Don't need to enqueue to retransmission queue
            return Ok(());
        }
        let mut queue = self.retransmission_queue.lock().unwrap();
        queue.push_back(RetransmissionQueue::new(packet));

        Ok(())
    }

    fn wait_tcp_packet(&self) -> Result<()> {
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
                dbg!("invalid checksum");
            }

            let tcb = self.tcb.write().unwrap();
            if tcb.status == TcpStatus::Listen {
                self.listen_handler(packet, tcb, remote_addr)?;
            } else if tcb.status == TcpStatus::SynSent {
                dbg!("Received SYN|ACK");
                self.synsent_handler(packet, tcb)?;
            } else if tcb.status == TcpStatus::SynRcvd {
                self.synrcvd_handler(packet, tcb)?;
            } else if tcb.status == TcpStatus::Established {
                self.established_handler(packet, tcb)?;
            } else {
                dbg!("Unsupported Status");
                break;
            }
        }
        Ok(())
    }

    fn listen_handler(
        &self,
        packet: TcpPacket,
        mut tcb: RwLockWriteGuard<Tcb>,
        remote_addr: Ipv4Addr,
    ) -> Result<()> {
        {
            let mut sock_id = self.sock_id.write().unwrap();
            sock_id.remote_addr = remote_addr;
            sock_id.remote_port = packet.get_src();
        }
        tcb.recv_params.next = packet.get_seq() + 1;
        tcb.send_params.next = random();
        tcb.send_params.una = tcb.send_params.next;
        tcb.status = TcpStatus::SynRcvd;
        self.send_tcp_packet(
            tcpflags::SYN | tcpflags::ACK,
            tcb.send_params.next,
            tcb.recv_params.next,
            tcb.send_params.window,
            &[],
        )?;
        Ok(())
    }

    fn synsent_handler(&self, packet: TcpPacket, mut tcb: RwLockWriteGuard<Tcb>) -> Result<()> {
        tcb.recv_params.next = packet.get_seq() + 1;
        tcb.send_params.una = packet.get_ack();
        tcb.send_params.window = packet.get_window_size();
        if tcb.send_params.una != tcb.send_params.next {
            dbg!("SND.NXT don't match SND.UNA!");
        }
        self.send_tcp_packet(
            tcpflags::ACK,
            tcb.send_params.next,
            tcb.recv_params.next,
            tcb.send_params.window,
            &[],
        )?;
        dbg!("Sent ACK");
        tcb.status = TcpStatus::Established;
        Ok(())
    }

    fn synrcvd_handler(&self, packet: TcpPacket, mut tcb: RwLockWriteGuard<Tcb>) -> Result<()> {
        tcb.send_params.una = packet.get_ack();
        tcb.status = TcpStatus::Established;
        Ok(())
    }

    fn established_handler(&self, packet: TcpPacket, mut tcb: RwLockWriteGuard<Tcb>) -> Result<()> {
        if packet.get_ack() <= tcb.send_params.next && packet.get_ack() >= tcb.send_params.una {
            tcb.send_params.una = packet.get_ack();
            self.delete_segment_from_queue(tcb)?;
        } else if packet.get_ack() > tcb.send_params.next {
            dbg!("received ACK is too big than expected one");
        }
        Ok(())
    }

    fn delete_segment_from_queue(&self, mut tcb: RwLockWriteGuard<Tcb>) -> Result<()> {
        let mut queue = self.retransmission_queue.lock().unwrap();
        while let Some(entry) = queue.pop_front() {
            if entry.packet.get_ack() < tcb.send_params.una {
                //dbg!("Successfully get acked");
                //dbg!(tcb.send_params.window);
                tcb.send_params.window += entry.packet.payload().len() as u16;
            } else {
                // the entry's packet has not ACKed. return to the queue
                queue.push_front(entry);
                break;
            }
        }
        Ok(())
    }

    fn timer(&self) {
        loop {
            {
                let mut tcb = self.tcb.write().unwrap();
                let mut queue = self.retransmission_queue.lock().unwrap();
                while let Some(mut entry) = queue.pop_front() {
                    // Remove entry that has already gotten ACK except Established state
                    if tcb.send_params.una > entry.packet.get_seq() {
                        //dbg!("Successfully get acked");
                        tcb.send_params.window += entry.packet.payload().len() as u16;
                        continue;
                    }

                    if entry.latest_transmission_time.elapsed().unwrap()
                        < Duration::from_secs(RETRANSMITTION_TIMEOUT)
                    {
                        queue.push_front(entry);
                        break;
                    }

                    if entry.transmission_count < MAX_TRANSMITTION {
                        let (mut sender, _) = transport::transport_channel(
                            MAX_PACKET_SIZE,
                            TransportChannelType::Layer4(TransportProtocol::Ipv4(
                                IpNextHeaderProtocols::Tcp,
                            )),
                        )
                        .unwrap();
                        let sock_id = self.sock_id.read().unwrap();
                        dbg!("Retransmission");
                        sender
                            .send_to(entry.packet.clone(), IpAddr::V4(sock_id.remote_addr))
                            .unwrap();
                        entry.transmission_count += 1;
                        entry.latest_transmission_time = SystemTime::now();
                        queue.push_back(entry);
                    } else {
                        dbg!("reached MAX_TRANSMISSION");
                    }
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
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

struct RetransmissionQueue {
    packet: TcpPacket,
    latest_transmission_time: SystemTime,
    transmission_count: u8,
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
