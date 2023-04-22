use pnet::packet::Packet;

const TCP_HEADER_SIZE: usize = 20;

pub struct TcpPacket {
    buffer: Vec<u8>,
}

impl TcpPacket {
    pub fn new(payload_len: usize) -> Self {
        Self {
            buffer: vec![0; TCP_HEADER_SIZE + payload_len],
        }
    }

    pub fn set_flag(&mut self, flag: u8) {
        self.buffer[13] = flag;
    }

    pub fn set_src(&mut self, port: u16) {
        self.buffer[0..2].copy_from_slice(&port.to_be_bytes());
    }

    pub fn set_dst(&mut self, port: u16) {
        self.buffer[2..4].copy_from_slice(&port.to_be_bytes());
    }

    pub fn set_data_offset(&mut self) {
        // Data offset is 4 bits field. 4 octets per bit
        let offset: u8 = TCP_HEADER_SIZE as u8 / 4;
        self.buffer[12] |= offset << 4;
    }

    pub fn set_seq(&mut self, seq: u32) {
        self.buffer[4..8].copy_from_slice(&seq.to_be_bytes());
    }

    pub fn set_ack(&mut self, ack: u32) {
        self.buffer[8..12].copy_from_slice(&ack.to_be_bytes());
    }

    pub fn set_window_size(&mut self, window: u16) {
        self.buffer[14..16].copy_from_slice(&window.to_be_bytes());
    }

    pub fn set_checksum(&mut self, checksum: u16) {
        self.buffer[16..18].copy_from_slice(&checksum.to_be_bytes());
    }

    pub fn get_flag(&self) -> u8 {
        self.buffer[13]
    }

    pub fn get_seq(&self) -> u32 {
        u32::from_be_bytes([
            self.buffer[4],
            self.buffer[5],
            self.buffer[6],
            self.buffer[7],
        ])
    }

    pub fn get_ack(&self) -> u32 {
        u32::from_be_bytes([
            self.buffer[8],
            self.buffer[9],
            self.buffer[10],
            self.buffer[11],
        ])
    }
    
    pub fn get_window_size(&self) -> u16 {
        u16::from_be_bytes([
            self.buffer[14],
            self.buffer[15],
        ])
    }
}

impl Packet for TcpPacket {
    fn packet(&self) -> &[u8] {
        &self.buffer
    }

    fn payload(&self) -> &[u8] {
        &self.buffer[TCP_HEADER_SIZE..]
    }
}

impl From<pnet::packet::tcp::TcpPacket<'_>> for TcpPacket {
    fn from(packet: pnet::packet::tcp::TcpPacket) -> Self {
        Self {
            buffer: packet.packet().to_vec(),
        }
    }
}
