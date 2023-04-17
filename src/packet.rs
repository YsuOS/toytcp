use pnet::packet::Packet;

const TCP_HEADER_SIZE: usize = 20;

pub struct TCPPacket {
    buffer: Vec<u8>,
}

impl TCPPacket {
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

    pub fn set_seq(&mut self) {
        let seq: u32 = 1000; // TODO: 1000 is no meanings. temporary value
        self.buffer[4..8].copy_from_slice(&seq.to_be_bytes());
    }

    pub fn set_window_size(&mut self, window: u16) {
        self.buffer[14..16].copy_from_slice(&window.to_be_bytes());
    }

    pub fn set_checksum(&mut self, checksum: u16) {
        self.buffer[16..18].copy_from_slice(&checksum.to_be_bytes());
    }
}

impl Packet for TCPPacket {
    fn packet(&self) -> &[u8] {
        &self.buffer
    }

    fn payload(&self) -> &[u8] {
        &self.buffer[TCP_HEADER_SIZE..]
    }
}
