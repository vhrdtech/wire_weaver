use crate::common::Kind;
use crate::{LinkMgmtCmd, PacketSource, ProtocolInfo, CRC_KIND, LINK_PROTOCOL_VERSION};
use shrink_wrap::BufReader;

/// Unpacks messages from one or more USB packets.
/// Message size is only limited by remote end buffer size (and u32::MAX, which is unlikely to be the case).
///
/// To ensure backward and forward format compatibility, there is a link setup phase, during which user protocol
/// and this link versions are checked.
/// Also buffer sizes are exchanged.
pub struct MessageReceiver<'a, S> {
    source: S,
    packet_buf: &'a mut [u8],
    receive_start_pos: usize,
    receive_left_bytes: usize,
    stats: ReceiverStats,
    in_fragmented_packet: bool,
    local_protocol: ProtocolInfo,
    remote_protocol: Option<ProtocolInfo>,
    protocols_versions_matches: bool,
}

/// Can be used to monitor how many messages, packets and bytes were received since link setup.
#[derive(Default, Debug, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ReceiverStats {
    pub packets_received: u32,
    pub messages_received: u32,
    pub bytes_received: u64,
    pub receive_errors: u32,
}

/// Kind of message that can be received.
#[derive(Debug)]
pub enum MessageKind {
    /// Message data
    Data(usize),
    /// Ping from the other end
    Ping,
    /// Remote end protocol and maximum message size
    LinkInfo {
        remote_max_message_size: usize,
        remote_protocol: ProtocolInfo,
    },
    Disconnect,
}

#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum ReceiveError<T> {
    SourceError(T),
    EmptyFrame,
    InternalBufOverflow,
    ProtocolsVersionsMismatch,
}

impl<T> From<T> for ReceiveError<T> {
    fn from(value: T) -> Self {
        ReceiveError::SourceError(value)
    }
}

impl<'a, S: PacketSource> MessageReceiver<'a, S> {
    /// Creates new MessageReceiver.
    pub fn new(source: S, packet_buf: &'a mut [u8], local_protocol: ProtocolInfo) -> Self {
        Self {
            source,
            packet_buf,
            receive_start_pos: 0,
            receive_left_bytes: 0,
            stats: ReceiverStats::default(),
            in_fragmented_packet: false,
            local_protocol,
            remote_protocol: None,
            #[cfg(not(test))]
            protocols_versions_matches: false,
            #[cfg(test)]
            protocols_versions_matches: true,
        }
    }

    /// Tries to unpack a next message sent from the [MessageSender](crate::MessageSender).
    /// If one or more packets are needed to reassemble a message, waits for all of them
    /// to arrive. If packet contained multiple messages, this function returns immediately with the
    /// next one.
    pub async fn receive_message(
        &mut self,
        message: &mut [u8],
    ) -> Result<MessageKind, ReceiveError<S::Error>> {
        let mut staging_idx = 0;
        'next_frame: loop {
            let (frame, is_new_frame) = if self.receive_left_bytes > 0 {
                (
                    &self.packet_buf
                        [self.receive_start_pos..self.receive_start_pos + self.receive_left_bytes],
                    false,
                )
            } else {
                let len = self.source.read_packet(&mut self.packet_buf).await?;
                self.stats.packets_received = self.stats.packets_received.wrapping_add(1);
                if len == 0 {
                    break Err(ReceiveError::EmptyFrame);
                }
                (&self.packet_buf[..len], true)
            };
            // println!("rx frame: {:?}", frame);
            let mut rd = BufReader::new(frame);
            while rd.bytes_left() >= 2 {
                let kind = rd
                    .read_u4()
                    .map_err(|_| ReceiveError::InternalBufOverflow)?;
                let Some(kind) = Kind::from_repr(kind) else {
                    continue 'next_frame;
                };
                if !self.protocols_versions_matches && kind != Kind::LinkInfo {
                    self.receive_left_bytes = 0;
                    return Err(ReceiveError::ProtocolsVersionsMismatch);
                }
                let len11_8 = rd
                    .read_u4()
                    .map_err(|_| ReceiveError::InternalBufOverflow)?;
                let len7_0 = rd
                    .read_u8()
                    .map_err(|_| ReceiveError::InternalBufOverflow)?;
                let len = (len11_8 as usize) << 8 | len7_0 as usize;
                match kind {
                    Kind::NoOp => {}
                    Kind::PacketStart | Kind::PacketContinue | Kind::PacketEnd => {
                        let Ok(packet_piece) = rd.read_raw_slice(len) else {
                            self.stats.receive_errors = self.stats.receive_errors.wrapping_add(1);
                            staging_idx = 0;
                            self.in_fragmented_packet = false;
                            continue 'next_frame;
                        };
                        if kind == Kind::PacketStart {
                            self.in_fragmented_packet = true;
                            staging_idx = 0;
                        } else if !self.in_fragmented_packet {
                            self.stats.receive_errors = self.stats.receive_errors.wrapping_add(1);
                            if kind == Kind::PacketEnd {
                                if let Ok(_crc) = rd.read_u16() {
                                    continue;
                                } else {
                                    continue 'next_frame;
                                }
                            } else {
                                continue;
                            }
                        }
                        let staging_bytes_left = message.len() - staging_idx;
                        if packet_piece.len() <= staging_bytes_left {
                            message[staging_idx..(staging_idx + packet_piece.len())]
                                .copy_from_slice(packet_piece);
                            staging_idx += packet_piece.len();
                            if kind == Kind::PacketEnd {
                                let Ok(crc_received) = rd.read_u16() else {
                                    self.stats.receive_errors =
                                        self.stats.receive_errors.wrapping_add(1);
                                    staging_idx = 0;
                                    continue 'next_frame;
                                };
                                let crc_calculated = CRC_KIND.checksum(&message[..staging_idx]);
                                if crc_received == crc_calculated {
                                    self.in_fragmented_packet = false;

                                    let min_bytes_left = rd.bytes_left() >= 2;
                                    let read_bytes = frame.len() - rd.bytes_left();
                                    match (is_new_frame, min_bytes_left) {
                                        (true, true) => {
                                            self.receive_start_pos = read_bytes;
                                            self.receive_left_bytes = rd.bytes_left();
                                        }
                                        (false, true) => {
                                            self.receive_start_pos += read_bytes;
                                            self.receive_left_bytes -= read_bytes;
                                        }
                                        _ => {
                                            self.receive_start_pos = 0;
                                            self.receive_left_bytes = 0;
                                        }
                                    }
                                    self.stats.bytes_received =
                                        self.stats.bytes_received.wrapping_add(staging_idx as u64);
                                    self.stats.messages_received =
                                        self.stats.messages_received.wrapping_add(1);
                                    return Ok(MessageKind::Data(staging_idx));
                                } else {
                                    self.stats.receive_errors =
                                        self.stats.receive_errors.wrapping_add(1);
                                    staging_idx = 0;
                                    continue; // try to receive other packets if any, previous frames might be lost leading to crc error
                                }
                            }
                        } else {
                            staging_idx = 0;
                            self.stats.receive_errors = self.stats.receive_errors.wrapping_add(1);
                            self.in_fragmented_packet = false;
                            continue 'next_frame;
                        }
                    }
                    Kind::PacketStartEnd => {
                        if let Ok(packet_read) = rd.read_raw_slice(len) {
                            message[..packet_read.len()].copy_from_slice(packet_read);

                            let min_bytes_left = rd.bytes_left() >= 2;
                            let read_bytes = frame.len() - rd.bytes_left();
                            match (is_new_frame, min_bytes_left) {
                                (true, true) => {
                                    self.receive_start_pos = read_bytes;
                                    self.receive_left_bytes = rd.bytes_left();
                                }
                                (false, true) => {
                                    self.receive_start_pos += read_bytes;
                                    self.receive_left_bytes -= read_bytes;
                                }
                                _ => {
                                    self.receive_start_pos = 0;
                                    self.receive_left_bytes = 0;
                                }
                            }
                            self.stats.bytes_received = self
                                .stats
                                .bytes_received
                                .wrapping_add(packet_read.len() as u64);
                            self.stats.messages_received =
                                self.stats.messages_received.wrapping_add(1);
                            return Ok(MessageKind::Data(packet_read.len()));
                        } else {
                            self.stats.receive_errors = self.stats.receive_errors.wrapping_add(1);
                            staging_idx = 0;
                            self.in_fragmented_packet = false;
                            continue 'next_frame;
                        }
                    }
                    Kind::LinkInfo => {
                        if rd.bytes_left() >= 4 + 1 + ProtocolInfo::size_bytes() {
                            let remote_max_packet_size = rd
                                .read_u32()
                                .map_err(|_| ReceiveError::InternalBufOverflow)?;
                            let link_protocol_version = rd
                                .read_u8()
                                .map_err(|_| ReceiveError::InternalBufOverflow)?;
                            let remote_protocol = ProtocolInfo::read(&mut rd)
                                .map_err(|_| ReceiveError::InternalBufOverflow)?;
                            if link_protocol_version == LINK_PROTOCOL_VERSION
                                && remote_protocol.is_compatible(&self.local_protocol)
                            {
                                self.protocols_versions_matches = true;
                                self.remote_protocol = Some(remote_protocol);
                            } else {
                                self.protocols_versions_matches = false;
                            }
                            self.source.send_to_sink(LinkMgmtCmd::LinkInfo {
                                link_version_matches: self.protocols_versions_matches,
                                local_max_packet_size: message.len() as u32,
                                remote_max_message_size: remote_max_packet_size,
                                remote_protocol,
                            });
                            return Ok(MessageKind::LinkInfo {
                                remote_max_message_size: remote_max_packet_size as usize,
                                remote_protocol,
                            });
                        }
                    }
                    Kind::Disconnect => {
                        self.protocols_versions_matches = false;
                        self.receive_left_bytes = 0;
                        self.source.send_to_sink(LinkMgmtCmd::Disconnect);
                        return Ok(MessageKind::Disconnect);
                    }
                    Kind::Ping => {
                        return Ok(MessageKind::Ping);
                    }
                }
            }
            self.receive_left_bytes = 0;
        }
    }

    #[cfg(feature = "device")]
    /// Device only function. Waits for frame source connection, i.e. waits for physical USB cable connection.
    pub async fn wait_source_connection(&mut self) {
        self.source.wait_connection().await;
    }

    #[cfg(feature = "device")]
    /// Waits for host to send link setup with compatible link and protocol version.
    pub async fn wait_link_connection(
        &mut self,
        packet: &mut [u8],
    ) -> Result<(), ReceiveError<S::Error>> {
        while !self.protocols_versions_matches {
            match self.receive_message(packet).await {
                Ok(MessageKind::LinkInfo {
                    remote_max_message_size: remote_max_packet_size,
                    remote_protocol,
                }) => {
                    #[cfg(feature = "defmt")]
                    defmt::trace!(
                        "Link established, remote max packet size: {}, remote protocol: {}",
                        remote_max_packet_size,
                        remote_protocol
                    );
                    break;
                }
                Ok(_) => continue, // shouldn't happen, but exit if setup is actually done
                Err(ReceiveError::ProtocolsVersionsMismatch) => {
                    #[cfg(feature = "defmt")]
                    defmt::warn!("Ignoring data before link setup");
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        self.stats = Default::default();
        Ok(())
    }

    /// Device only function. Marks link as not connected, but does not send anything to the host.
    #[cfg(feature = "device")]
    pub fn disconnect(&mut self) {
        self.protocols_versions_matches = false;
        self.remote_protocol = None;
    }

    /// Returns remote protocol information.
    pub fn remote_protocol(&self) -> Option<ProtocolInfo> {
        self.remote_protocol
    }

    /// Returns statistics struct.
    pub fn stats(&self) -> &ReceiverStats {
        &self.stats
    }
}
