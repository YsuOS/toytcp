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
pub struct Socket {
    pub sock_id: RwLock<SockId>,
    tcb: RwLock<Tcb>,
    retransmission_queue: Mutex<VecDeque<RetransmissionQueue>>,
}

impl Socket {
    pub fn new() -> Arc<Self> {
        let socket = Arc::new(Self {
            sock_id: RwLock::new(SockId::new().unwrap()),
            tcb: RwLock::new(Tcb {
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

    pub fn connect(&self, remote_addr: Ipv4Addr, remote_port: u16) -> Result<()> {
        let local_addr = LOCAL_ADDR;
        let local_port = set_unsed_port().unwrap();

        self.set_sockid(local_addr, local_port, remote_addr, remote_port);
        self.init_seq();

        self.send_tcp_packet_lock(TcpStatus::SynSent);

        self.wait_tcp_status(TcpStatus::Established);

        Ok(())
    }

    pub fn listen(&self, local_addr: Ipv4Addr, local_port: u16) -> Result<()> {
        let remote_addr = UNDETERMINED_ADDR;
        let remote_port = UNDETERMINED_PORT;

        self.set_sockid(local_addr, local_port, remote_addr, remote_port);

        self.set_status(TcpStatus::Listen);
        Ok(())
    }

    fn set_status(&self, status: TcpStatus) {
        let mut tcb = self.tcb.write().unwrap();
        tcb.status = status;
    }

    fn set_sockid(
        &self,
        local_addr: Ipv4Addr,
        local_port: u16,
        remote_addr: Ipv4Addr,
        remote_port: u16,
    ) {
        let mut sock_id = self.sock_id.write().unwrap();
        *sock_id = SockId {
            local_addr,
            local_port,
            remote_addr,
            remote_port,
        };
    }

    fn get_local(&self) -> Result<(Ipv4Addr, u16)> {
        let sock_id = self.sock_id.read().unwrap();
        Ok((sock_id.local_addr, sock_id.local_port))
    }

    fn init_seq(&self) {
        let mut tcb = self.tcb.write().unwrap();
        tcb.send_params.next = random();
        tcb.send_params.una = tcb.send_params.next;
        dbg!(self, &tcb.send_params, &tcb.recv_params);
    }

    fn wait_tcp_status(&self, status: TcpStatus) {
        loop {
            let tcb = self.tcb.read().unwrap();
            if tcb.status == status {
                break;
            }
        }
    }

    fn send_tcp_packet_lock(&self, status: TcpStatus) {
        let mut tcb = self.tcb.write().unwrap();
        self.send_tcp_packet(
            tcpflags::SYN,
            tcb.send_params.next,
            0,
            tcb.recv_params.window,
            &[],
        ).unwrap();
        tcb.status = status;
        tcb.send_params.next += 1;
        dbg!(&tcb.status);
    }

    pub fn accept(&self) -> Result<()> {
        self.wait_tcp_status(TcpStatus::Established);
        Ok(())
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
                tcb.recv_params.window,
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

    pub fn recv(&self, buf: &mut [u8]) -> Result<usize> {
        let mut received_size = {
            let tcb = self.tcb.read().unwrap();
            tcb.recv_buffer.len() - tcb.recv_params.window as usize
        };

        while received_size == 0 {
            {
                let tcb = self.tcb.read().unwrap();
                match tcb.status {
                    TcpStatus::CloseWait | TcpStatus::LastAck | TcpStatus::TimeWait => break,
                    _ => {}
                }
            }

            received_size = {
                let tcb = self.tcb.read().unwrap();
                tcb.recv_buffer.len() - tcb.recv_params.window as usize
            };
        }

        let copy_size = cmp::min(buf.len(), received_size);
        let mut tcb = self.tcb.write().unwrap();
        buf[..copy_size].copy_from_slice(&tcb.recv_buffer[..copy_size]);
        tcb.recv_buffer.copy_within(copy_size.., 0);
        tcb.recv_params.window += copy_size as u16;

        Ok(copy_size)
    }

    pub fn close(&self) -> Result<()> {
        let mut tcb = self.tcb.write().unwrap();
        self.send_tcp_packet(
            tcpflags::FIN | tcpflags::ACK,
            tcb.send_params.next,
            tcb.recv_params.next,
            tcb.recv_params.window,
            &[],
        )?;
        tcb.send_params.next += 1;

        match tcb.status {
            TcpStatus::Established => {
                tcb.status = TcpStatus::FinWait1;
            }
            TcpStatus::CloseWait => {
                tcb.status = TcpStatus::LastAck;
            }
            _ => return Ok(()),
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
            let tcp_packet =
                TcpPacket::from(pnet::packet::tcp::TcpPacket::new(packet.payload()).unwrap());
            if !tcp_packet.is_correct_checksum(local_addr, remote_addr) {
                dbg!("invalid checksum");
            }

            let tcb = self.tcb.write().unwrap();
            match tcb.status {
                TcpStatus::Listen => self.listen_handler(tcp_packet, tcb, remote_addr)?,
                TcpStatus::SynSent => {
                    dbg!("Received SYN|ACK");
                    self.synsent_handler(tcp_packet, tcb)?;
                }
                TcpStatus::SynRcvd => self.synrcvd_handler(tcp_packet, tcb)?,
                TcpStatus::Established => self.established_handler(tcp_packet, tcb)?,
                TcpStatus::CloseWait | TcpStatus::LastAck => self.close_handler(tcp_packet, tcb)?,
                TcpStatus::FinWait1 | TcpStatus::FinWait2 => self.finwait_handler(tcp_packet, tcb)?,
                _ => {
                    dbg!("Unsupported Status");
                    break;
                }
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
        let (local_addr, local_port) = self.get_local().unwrap();
        let remote_port = packet.get_src();
        self.set_sockid(local_addr, local_port, remote_addr, remote_port);

        tcb.recv_params.next = packet.get_seq() + 1;

        tcb.send_params.next = random();
        tcb.send_params.una = tcb.send_params.next;

        self.send_tcp_packet(
            tcpflags::SYN | tcpflags::ACK,
            tcb.send_params.next,
            tcb.recv_params.next,
            tcb.recv_params.window,
            &[],
        )?;
        tcb.status = TcpStatus::SynRcvd;
        tcb.send_params.next += 1;

        dbg!(&tcb.status);
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
            tcb.recv_params.window,
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
            self.delete_segment_from_queue(&mut tcb)?;
        } else if packet.get_ack() > tcb.send_params.next {
            dbg!("received ACK is too big than expected one");
        }

        if !packet.payload().is_empty() {
            self.process_payload(&packet, &mut tcb)?;
        }

        if packet.get_flag() & tcpflags::FIN > 0 {
            tcb.recv_params.next = packet.get_seq() + 1;
            self.send_tcp_packet(
                tcpflags::ACK,
                tcb.send_params.next,
                tcb.recv_params.next,
                tcb.recv_params.window,
                &[],
            )?;
            tcb.status = TcpStatus::CloseWait;
        }

        Ok(())
    }

    fn close_handler(&self, packet: TcpPacket, mut tcb: RwLockWriteGuard<Tcb>) -> Result<()> {
        tcb.send_params.una = packet.get_ack();
        Ok(())
    }

    fn finwait_handler(&self, packet: TcpPacket, mut tcb: RwLockWriteGuard<Tcb>) -> Result<()> {
        if packet.get_ack() <= tcb.send_params.next && packet.get_ack() >= tcb.send_params.una {
            tcb.send_params.una = packet.get_ack();
            self.delete_segment_from_queue(&mut tcb)?;
        } else if packet.get_ack() > tcb.send_params.next {
            dbg!("received ACK is too big than expected one");
        }

        if !packet.payload().is_empty() {
            self.process_payload(&packet, &mut tcb)?;
        }

        if tcb.status == TcpStatus::FinWait1 && tcb.send_params.next == tcb.send_params.una {
            tcb.status = TcpStatus::FinWait2;
        }

        if packet.get_flag() & tcpflags::FIN > 0 {
            tcb.recv_params.next += 1;
            self.send_tcp_packet(
                tcpflags::ACK,
                tcb.send_params.next,
                tcb.recv_params.next,
                tcb.recv_params.window,
                &[],
            )?;
            // Change status to TIMEWAIT. but its implementation is omitted.
        }

        Ok(())
    }

    fn delete_segment_from_queue(&self, tcb: &mut RwLockWriteGuard<Tcb>) -> Result<()> {
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

    fn process_payload(&self, packet: &TcpPacket, tcb: &mut RwLockWriteGuard<Tcb>) -> Result<()> {
        let offset = tcb.recv_buffer.len() - tcb.recv_params.window as usize
            + (packet.get_seq() - tcb.recv_params.next) as usize;
        let copy_size = cmp::min(packet.payload().len(), tcb.recv_buffer.len() - offset);

        tcb.recv_buffer[offset..offset + copy_size].copy_from_slice(&packet.payload()[..copy_size]);

        tcb.recv_params.tail = cmp::max(tcb.recv_params.tail, packet.get_seq() + copy_size as u32);

        if packet.get_seq() == tcb.recv_params.next {
            tcb.recv_params.next = tcb.recv_params.tail;
            tcb.recv_params.window -= (tcb.recv_params.tail - packet.get_seq()) as u16;
        }

        if copy_size > 0 {
            self.send_tcp_packet(
                tcpflags::ACK,
                tcb.send_params.next,
                tcb.recv_params.next,
                tcb.recv_params.window,
                &[],
            )?;
        } else {
            dbg!("recv buffer overflow");
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
                        if entry.packet.get_flag() & tcpflags::FIN > 0
                            && tcb.status == TcpStatus::LastAck
                        {
                            dbg!("connection closed");
                        }
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
                        if entry.packet.get_flag() & tcpflags::FIN > 0
                            && (tcb.status == TcpStatus::LastAck
                                || tcb.status == TcpStatus::FinWait1
                                || tcb.status == TcpStatus::FinWait2)
                        {
                            dbg!("connection closed");
                        }
                    }
                }
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

#[derive(Debug, Clone)]
pub struct SockId {
    pub local_addr: Ipv4Addr,
    pub remote_addr: Ipv4Addr,
    pub local_port: u16,
    pub remote_port: u16,
}

impl SockId {
    pub fn new() -> Result<Self> {
        Ok(SockId {
            local_addr: UNDETERMINED_ADDR,
            local_port: UNDETERMINED_PORT,
            remote_addr: UNDETERMINED_ADDR,
            remote_port: UNDETERMINED_PORT,
        })
    }
}

fn set_unsed_port() -> Result<u16> {
    let mut rng = rand::thread_rng();
    Ok(rng.gen_range(PORT_RANGE))
}

#[derive(Debug)]
struct Tcb {
    status: TcpStatus,
    send_params: SendParams,
    recv_params: ReceiveParams,
    recv_buffer: Vec<u8>,
}

#[derive(Debug)]
struct SendParams {
    next: u32,
    una: u32,
    window: u16,
}

#[derive(Debug)]
struct ReceiveParams {
    next: u32,
    tail: u32,
    window: u16,
}

#[derive(Debug)]
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
