use crate::{
    packet::TcpPacket,
    sock::{Sock, TcpStatus},
    tcpflags,
};
use anyhow::{Ok, Result};
use pnet::{
    packet::{ip::IpNextHeaderProtocols, Packet},
    transport::{self, TransportChannelType, TransportProtocol},
};
use rand::Rng;
use std::{
    cmp,
    collections::{HashMap, VecDeque},
    net::{IpAddr, Ipv4Addr},
    ops::Range,
    sync::{Arc, Condvar, Mutex, RwLock, RwLockWriteGuard},
    thread,
    time::{Duration, SystemTime},
};

const UNDETERMINED_ADDR: Ipv4Addr = Ipv4Addr::new(0, 0, 0, 0);
const UNDETERMINED_PORT: u16 = 0;
const MAX_TRANSMITTION: u8 = 5;
const RETRANSMITTION_TIMEOUT: u64 = 3;
const MSS: usize = 1460;

const MAX_PACKET_SIZE: usize = 65535;

const LOCAL_ADDR: Ipv4Addr = Ipv4Addr::new(10, 0, 0, 1);
const PORT_RANGE: Range<u16> = 40000..60000;

#[derive(Debug)]
pub struct Socket {
    socks: RwLock<HashMap<SockId, Sock>>,
    socket_state: (Mutex<SocketState>, Condvar),
    backlog: Mutex<VecDeque<SockId>>,
}

#[derive(Debug, PartialEq)]
enum SocketState {
    Free,
    Unconnected,
    Connecting,
    Connected,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct SockId {
    pub local_addr: Ipv4Addr,
    pub remote_addr: Ipv4Addr,
    pub local_port: u16,
    pub remote_port: u16,
}

impl SockId {
    pub fn new(
        local_addr: Ipv4Addr,
        local_port: u16,
        remote_addr: Ipv4Addr,
        remote_port: u16,
    ) -> Result<Self> {
        Ok(Self {
            local_addr,
            local_port,
            remote_addr,
            remote_port,
        })
    }
}

impl Socket {
    pub fn new() -> Arc<Self> {
        let socket = Arc::new(Self {
            socks: RwLock::new(HashMap::new()),
            socket_state: (Mutex::new(SocketState::Free), Condvar::new()),
            backlog: Mutex::new(VecDeque::new()),
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

    pub fn connect(&self, remote_addr: Ipv4Addr, remote_port: u16) -> Result<SockId> {
        let local_addr = LOCAL_ADDR;
        let local_port = set_unsed_port().unwrap();

        let sock_id = SockId::new(local_addr, local_port, remote_addr, remote_port)?;
        let mut sock = Sock::new(sock_id).unwrap();

        sock.init_seq()?;

        sock.send_tcp_packet_syn(tcpflags::SYN, TcpStatus::SynSent);

        self.insert_sock(sock_id, sock);

        self.set_state(SocketState::Connecting);

        self.wait_state(SocketState::Connected);

        Ok(sock_id)
    }

    pub fn listen(&self, local_addr: Ipv4Addr, local_port: u16) -> Result<()> {
        let remote_addr = UNDETERMINED_ADDR;
        let remote_port = UNDETERMINED_PORT;

        let sock_id = SockId::new(local_addr, local_port, remote_addr, remote_port)?;
        let mut sock = Sock::new(sock_id).unwrap();

        sock.status = TcpStatus::Listen;

        self.insert_sock(sock_id, sock);
        dbg!("listen socket", &sock_id);

        self.set_state(SocketState::Unconnected);

        Ok(())
    }

    pub fn accept(&self) -> Result<SockId> {
        loop {
            match self.pop_front_backlog() {
                Some(sock_id) => {
                    dbg!("Accepted");
                    return Ok(sock_id);
                }
                None => continue,
            };
        }
    }

    pub fn send(&self, sock_id: SockId, buf: &[u8]) -> Result<()> {
        let mut cursor = 0;
        while cursor < buf.len() {
            let mut send_size = {
                let table = self.socks.write().unwrap();
                let sock = table.get(&sock_id).unwrap();
                cmp::min(
                    MSS,
                    cmp::min(sock.send_params.window as usize, buf.len() - cursor),
                )
            };

            while send_size == 0 {
                send_size = {
                    let table = self.socks.read().unwrap();
                    let sock = table.get(&sock_id).unwrap();
                    cmp::min(
                        MSS,
                        cmp::min(sock.send_params.window as usize, buf.len() - cursor),
                    )
                };
            }

            let mut table = self.socks.write().unwrap();
            let sock = table.get_mut(&sock_id).unwrap();
            sock.send_tcp_packet_send(&buf[cursor..cursor + send_size]);
            cursor += send_size;
            thread::sleep(Duration::from_millis(1));
        }
        Ok(())
    }

    pub fn recv(&self, sock_id: SockId, buf: &mut [u8]) -> Result<usize> {
        let mut received_size = {
            let table = self.socks.read().unwrap();
            let sock = table.get(&sock_id).unwrap();
            sock.recv_buffer.len() - sock.recv_params.window as usize
        };

        while received_size == 0 {
            {
                let table = self.socks.read().unwrap();
                let sock = table.get(&sock_id).unwrap();
                match sock.status {
                    TcpStatus::CloseWait | TcpStatus::LastAck | TcpStatus::TimeWait => break,
                    _ => {}
                }
            }

            received_size = {
                let table = self.socks.read().unwrap();
                let sock = table.get(&sock_id).unwrap();
                sock.recv_buffer.len() - sock.recv_params.window as usize
            };
        }

        let copy_size = cmp::min(buf.len(), received_size);
        let mut table = self.socks.write().unwrap();
        let sock = table.get_mut(&sock_id).unwrap();
        buf[..copy_size].copy_from_slice(&sock.recv_buffer[..copy_size]);
        sock.recv_buffer.copy_within(copy_size.., 0);
        sock.recv_params.window += copy_size as u16;

        Ok(copy_size)
    }

    pub fn close(&self, sock_id: SockId) -> Result<()> {
        let mut table = self.socks.write().unwrap();
        let sock = table.get_mut(&sock_id).unwrap();
        sock.send_tcp_packet_fin()?;

        match sock.status {
            TcpStatus::Established => {
                sock.status = TcpStatus::FinWait1;
                drop(table);
                // FIXME: is it appropriate?
                self.wait_state(SocketState::Free);
                let mut table = self.socks.write().unwrap();
                table.remove(&sock_id);
                dbg!("connection closed & removed", sock_id);
            }
            TcpStatus::CloseWait => {
                sock.status = TcpStatus::LastAck;
                drop(table);
                // FIXME: is it appropriate?
                self.wait_state(SocketState::Free);
                let mut table = self.socks.write().unwrap();
                table.remove(&sock_id);
                dbg!("connection closed & removed", sock_id);
            }
            _ => return Ok(()),
        }

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
            let local_port = tcp_packet.get_dst();
            let remote_port = tcp_packet.get_src();
            let sock_id = SockId::new(local_addr, local_port, remote_addr, remote_port).unwrap();
            dbg!(&sock_id);

            let mut table = self.socks.write().unwrap();
            let sock = match table.get_mut(&sock_id) {
                Some(sock) => sock,
                None => match table.get_mut(&SockId {
                    local_addr,
                    remote_addr: UNDETERMINED_ADDR,
                    local_port,
                    remote_port: UNDETERMINED_PORT,
                }) {
                    Some(sock) => sock,
                    None => continue,
                },
            };

            let sock_id = sock.sock_id;
            match sock.status {
                TcpStatus::Listen => {
                    self.set_state(SocketState::Connecting);
                    self.listen_handler(tcp_packet, sock_id, remote_addr, table)?
                }
                TcpStatus::SynSent => self.synsent_handler(tcp_packet, sock)?,
                TcpStatus::SynRcvd => self.synrcvd_handler(tcp_packet, sock_id, table)?,
                TcpStatus::Established => self.established_handler(tcp_packet, sock)?,
                TcpStatus::CloseWait | TcpStatus::LastAck => {
                    self.close_handler(tcp_packet, sock)?
                }
                TcpStatus::FinWait1 | TcpStatus::FinWait2 => {
                    self.finwait_handler(tcp_packet, sock)?
                }
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
        listen_sock_id: SockId,
        remote_addr: Ipv4Addr,
        mut table: RwLockWriteGuard<HashMap<SockId, Sock>>,
    ) -> Result<()> {
        let local_addr = listen_sock_id.local_addr;
        let local_port = listen_sock_id.local_port;
        let remote_port = packet.get_src();

        let conn_sock_id = SockId::new(local_addr, local_port, remote_addr, remote_port)?;
        let mut conn_sock = Sock::new(conn_sock_id).unwrap();
        conn_sock.status = TcpStatus::Listen;
        conn_sock.init_seq()?;
        conn_sock.recv_params.next = packet.get_seq() + 1;
        conn_sock.send_params.window = packet.get_window_size();

        conn_sock.send_tcp_packet_syn(tcpflags::SYN | tcpflags::ACK, TcpStatus::SynRcvd);

        table.insert(conn_sock_id, conn_sock);

        dbg!("Sent SYN|ACK");

        Ok(())
    }

    fn synsent_handler(&self, packet: TcpPacket, sock: &mut Sock) -> Result<()> {
        dbg!("Received SYN|ACK");

        sock.recv_params.next = packet.get_seq() + 1;
        sock.send_params.una = packet.get_ack();
        sock.send_params.window = packet.get_window_size();

        if sock.send_params.una != sock.send_params.next {
            dbg!("SND.NXT don't match SND.UNA!");
        }

        sock.send_tcp_packet_ack(TcpStatus::Established);
        self.set_state(SocketState::Connected);
        dbg!("Sent ACK");
        Ok(())
    }

    fn synrcvd_handler(
        &self,
        packet: TcpPacket,
        conn_sock_id: SockId,
        mut table: RwLockWriteGuard<HashMap<SockId, Sock>>,
    ) -> Result<()> {
        let mut sock = table.get_mut(&conn_sock_id).unwrap();
        sock.send_params.una = packet.get_ack();
        sock.status = TcpStatus::Established;

        self.push_backlog(conn_sock_id);
        self.set_state(SocketState::Connected);
        Ok(())
    }

    fn established_handler(&self, packet: TcpPacket, sock: &mut Sock) -> Result<()> {
        if packet.get_ack() <= sock.send_params.next && packet.get_ack() >= sock.send_params.una {
            sock.send_params.una = packet.get_ack();
            self.delete_segment_from_queue(sock)?;
        } else if packet.get_ack() > sock.send_params.next {
            dbg!("received ACK is too big than expected one");
        }

        if !packet.payload().is_empty() {
            self.process_payload(&packet, sock)?;
        }

        if packet.get_flag() & tcpflags::FIN > 0 {
            sock.recv_params.next = packet.get_seq() + 1;
            sock.send_tcp_packet_ack(TcpStatus::CloseWait);
        }

        Ok(())
    }

    fn close_handler(&self, packet: TcpPacket, sock: &mut Sock) -> Result<()> {
        sock.send_params.una = packet.get_ack();
        Ok(())
    }

    fn finwait_handler(&self, packet: TcpPacket, sock: &mut Sock) -> Result<()> {
        if packet.get_ack() <= sock.send_params.next && packet.get_ack() >= sock.send_params.una {
            sock.send_params.una = packet.get_ack();
            self.delete_segment_from_queue(sock)?;
        } else if packet.get_ack() > sock.send_params.next {
            dbg!("received ACK is too big than expected one");
        }

        if !packet.payload().is_empty() {
            self.process_payload(&packet, sock)?;
        }

        if sock.status == TcpStatus::FinWait1 && sock.send_params.next == sock.send_params.una {
            sock.status = TcpStatus::FinWait2;
        }

        if packet.get_flag() & tcpflags::FIN > 0 {
            sock.recv_params.next += 1;
            sock.send_tcp_packet_ack(TcpStatus::TimeWait);
            // TODO: not implemented TimeWait state. The socket closes immediately after sending
            // ack
            self.set_state(SocketState::Free);
        }

        Ok(())
    }

    fn delete_segment_from_queue(&self, sock: &mut Sock) -> Result<()> {
        let mut queue = sock.retransmission_queue.lock().unwrap();
        while let Some(entry) = queue.pop_front() {
            if entry.packet.get_ack() < sock.send_params.una {
                sock.send_params.window += entry.packet.payload().len() as u16;
            } else {
                // the entry's packet has not ACKed. return to the queue
                queue.push_front(entry);
                break;
            }
        }
        Ok(())
    }

    fn process_payload(&self, packet: &TcpPacket, sock: &mut Sock) -> Result<()> {
        let offset = sock.recv_buffer.len() - sock.recv_params.window as usize
            + (packet.get_seq() - sock.recv_params.next) as usize;
        let copy_size = cmp::min(packet.payload().len(), sock.recv_buffer.len() - offset);

        sock.recv_buffer[offset..offset + copy_size]
            .copy_from_slice(&packet.payload()[..copy_size]);

        sock.recv_params.tail =
            cmp::max(sock.recv_params.tail, packet.get_seq() + copy_size as u32);

        if packet.get_seq() == sock.recv_params.next {
            sock.recv_params.next = sock.recv_params.tail;
            sock.recv_params.window -= (sock.recv_params.tail - packet.get_seq()) as u16;
        }

        if copy_size > 0 {
            sock.send_tcp_packet_ack(TcpStatus::Established);
        } else {
            dbg!("recv buffer overflow");
        }

        Ok(())
    }

    fn timer(&self) {
        loop {
            let mut table = self.socks.write().unwrap();
            for (sock_id, sock) in table.iter_mut() {
                let mut queue = sock.retransmission_queue.lock().unwrap();
                while let Some(mut entry) = queue.pop_front() {
                    // Remove entry that has already gotten ACK except Established state
                    if sock.send_params.una > entry.packet.get_seq() {
                        sock.send_params.window += entry.packet.payload().len() as u16;
                        if entry.packet.get_flag() & tcpflags::FIN > 0
                            && sock.status == TcpStatus::LastAck
                        {
                            self.set_state(SocketState::Free);
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
                            && (sock.status == TcpStatus::LastAck
                                || sock.status == TcpStatus::FinWait1
                                || sock.status == TcpStatus::FinWait2)
                        {
                            self.set_state(SocketState::Free);
                            dbg!("connection closed");
                        }
                    }
                }
            }
            drop(table);
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn wait_state(&self, wait_state: SocketState) {
        let (state, cvar) = &self.socket_state;
        let mut state = state.lock().unwrap();
        while *state != wait_state {
            state = cvar.wait(state).unwrap();
        }
    }

    fn set_state(&self, set_state: SocketState) {
        let (state, cvar) = &self.socket_state;
        let mut state = state.lock().unwrap();
        *state = set_state;
        cvar.notify_all();
    }

    fn insert_sock(&self, sock_id: SockId, sock: Sock) {
        let mut table = self.socks.write().unwrap();
        table.insert(sock_id, sock);
    }

    fn push_backlog(&self, sock_id: SockId) {
        let mut backlog = self.backlog.lock().unwrap();
        backlog.push_back(sock_id);
    }

    fn pop_front_backlog(&self) -> Option<SockId> {
        let mut backlog = self.backlog.lock().unwrap();
        backlog.pop_front()
    }
}

fn set_unsed_port() -> Result<u16> {
    let mut rng = rand::thread_rng();
    Ok(rng.gen_range(PORT_RANGE))
}
