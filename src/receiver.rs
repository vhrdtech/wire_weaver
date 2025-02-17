use crate::common::{Error, Op, WireWeaverUsbLink};
use crate::{
    PacketSink, PacketSource, ProtocolInfo, CRC_KIND, LINK_PROTOCOL_VERSION, MIN_MESSAGE_SIZE,
};
use shrink_wrap::BufReader;

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

impl<'a, T: PacketSink, R: PacketSource> WireWeaverUsbLink<'a, T, R> {
    /// Tries to unpack a next message sent from the [MessageSender](crate::MessageSender).
    /// If one or more packets are needed to reassemble a message, waits for all of them
    /// to arrive. If packet contained multiple messages, this function returns immediately with the
    /// next one.
    pub async fn receive_message(
        &mut self,
        message: &mut [u8],
    ) -> Result<MessageKind, Error<T::Error, R::Error>> {
        let mut staging_idx = 0;
        'next_message: loop {
            let (packet, is_new_frame) = if self.rx_left_bytes > 0 {
                (
                    &self.rx_packet_buf[self.rx_start_pos..self.rx_start_pos + self.rx_left_bytes],
                    false,
                )
            } else {
                let len = self
                    .rx
                    .read_packet(&mut self.rx_packet_buf)
                    .await
                    .map_err(|e| Error::SourceError(e))?;
                self.rx_stats.packets_received = self.rx_stats.packets_received.wrapping_add(1);
                if len == 0 {
                    break Err(Error::ReceivedEmptyPacket);
                }
                (&self.rx_packet_buf[..len], true)
            };
            // println!("rx frame: {:?}", frame);
            let mut rd = BufReader::new(packet);
            while rd.bytes_left() >= 2 {
                let kind = rd.read_u4().map_err(|_| Error::InternalBufOverflow)?;
                let Some(kind) = Op::from_repr(kind) else {
                    self.rx_left_bytes = 0; // skip whole packet on malformed data
                    continue 'next_message;
                };
                if self.remote_protocol.is_none() && kind != Op::LinkSetup && kind != Op::NoOp {
                    self.rx_left_bytes = 0;
                    return Err(Error::ProtocolsVersionMismatch);
                }
                let len11_8 = rd.read_u4().map_err(|_| Error::InternalBufOverflow)?;
                let len7_0 = rd.read_u8().map_err(|_| Error::InternalBufOverflow)?;
                let len = (len11_8 as usize) << 8 | len7_0 as usize;
                match kind {
                    Op::NoOp => {}
                    Op::MessageStart | Op::MessageContinue | Op::MessageEnd => {
                        let Ok(message_piece) = rd.read_raw_slice(len) else {
                            self.rx_stats.receive_errors =
                                self.rx_stats.receive_errors.wrapping_add(1);
                            staging_idx = 0;
                            self.rx_in_fragmented_message = false;
                            continue 'next_message;
                        };
                        if kind == Op::MessageStart {
                            self.rx_in_fragmented_message = true;
                            staging_idx = 0;
                        } else if !self.rx_in_fragmented_message {
                            self.rx_stats.receive_errors =
                                self.rx_stats.receive_errors.wrapping_add(1);
                            if kind == Op::MessageEnd {
                                if let Ok(_crc) = rd.read_u16() {
                                    continue;
                                } else {
                                    continue 'next_message;
                                }
                            } else {
                                continue;
                            }
                        }
                        let staging_bytes_left = message.len() - staging_idx;
                        if message_piece.len() <= staging_bytes_left {
                            message[staging_idx..(staging_idx + message_piece.len())]
                                .copy_from_slice(message_piece);
                            staging_idx += message_piece.len();
                            if kind == Op::MessageEnd {
                                let Ok(crc_received) = rd.read_u16() else {
                                    self.rx_stats.receive_errors =
                                        self.rx_stats.receive_errors.wrapping_add(1);
                                    staging_idx = 0;
                                    continue 'next_message;
                                };
                                let crc_calculated = CRC_KIND.checksum(&message[..staging_idx]);
                                if crc_received == crc_calculated {
                                    self.rx_in_fragmented_message = false;

                                    self.adjust_read_pos(
                                        is_new_frame,
                                        rd.bytes_left(),
                                        packet.len(),
                                    );
                                    self.rx_stats.bytes_received = self
                                        .rx_stats
                                        .bytes_received
                                        .wrapping_add(staging_idx as u64);
                                    self.rx_stats.messages_received =
                                        self.rx_stats.messages_received.wrapping_add(1);
                                    return Ok(MessageKind::Data(staging_idx));
                                } else {
                                    self.rx_stats.receive_errors =
                                        self.rx_stats.receive_errors.wrapping_add(1);
                                    staging_idx = 0;
                                    continue; // try to receive other packets if any, previous frames might be lost leading to crc error
                                }
                            }
                        } else {
                            staging_idx = 0;
                            self.rx_stats.receive_errors =
                                self.rx_stats.receive_errors.wrapping_add(1);
                            self.rx_in_fragmented_message = false;
                            continue 'next_message;
                        }
                    }
                    Op::MessageStartEnd => {
                        if let Ok(message_read) = rd.read_raw_slice(len) {
                            message[..message_read.len()].copy_from_slice(message_read);

                            let message_read_len = message_read.len();
                            self.rx_stats.bytes_received = self
                                .rx_stats
                                .bytes_received
                                .wrapping_add(message_read.len() as u64);
                            self.rx_stats.messages_received =
                                self.rx_stats.messages_received.wrapping_add(1);

                            self.adjust_read_pos(is_new_frame, rd.bytes_left(), packet.len());
                            return Ok(MessageKind::Data(message_read_len));
                        } else {
                            self.rx_stats.receive_errors =
                                self.rx_stats.receive_errors.wrapping_add(1);
                            staging_idx = 0;
                            self.rx_in_fragmented_message = false;
                            continue 'next_message;
                        }
                    }
                    Op::GetVersions => {
                        #[cfg(feature = "device")]
                        self.send_link_setup(message.len() as u32).await?;
                        continue 'next_message;
                    }
                    Op::LinkSetup => {
                        if rd.bytes_left() >= 4 + 1 + ProtocolInfo::size_bytes() {
                            let remote_max_message_size =
                                rd.read_u32().map_err(|_| Error::InternalBufOverflow)?;
                            let link_protocol_version =
                                rd.read_u8().map_err(|_| Error::InternalBufOverflow)?;
                            let remote_protocol = ProtocolInfo::read(&mut rd)
                                .map_err(|_| Error::InternalBufOverflow)?;
                            if link_protocol_version == LINK_PROTOCOL_VERSION
                                && remote_protocol.is_compatible(&self.protocol)
                            {
                                self.remote_protocol = Some(remote_protocol);
                                self.remote_max_message_size = remote_max_message_size;
                            } else {
                                self.remote_protocol = None;
                            }

                            #[cfg(feature = "device")]
                            self.send_link_setup(message.len() as u32).await?;

                            return Ok(MessageKind::LinkInfo {
                                remote_max_message_size: remote_max_message_size as usize,
                                remote_protocol,
                            });
                        }
                    }
                    Op::Disconnect => {
                        self.remote_protocol = None;
                        self.remote_max_message_size = MIN_MESSAGE_SIZE as u32;
                        self.rx_left_bytes = 0;
                        return Ok(MessageKind::Disconnect);
                    }
                    Op::Ping => {
                        return Ok(MessageKind::Ping);
                    }
                }
            }
            self.rx_left_bytes = 0;
        }
    }

    /// If packet contains more than one message, adjust indices accordingly
    fn adjust_read_pos(&mut self, is_new_frame: bool, rd_bytes_left: usize, packet_len: usize) {
        let min_bytes_left = rd_bytes_left >= 2;
        let read_bytes = packet_len - rd_bytes_left;
        match (is_new_frame, min_bytes_left) {
            (true, true) => {
                self.rx_start_pos = read_bytes;
                self.rx_left_bytes = rd_bytes_left;
            }
            (false, true) => {
                self.rx_start_pos += read_bytes;
                self.rx_left_bytes -= read_bytes;
            }
            _ => {
                // new packet will be awaited in next receive_message() call
                self.rx_start_pos = 0;
                self.rx_left_bytes = 0;
            }
        }
    }

    #[cfg(feature = "device")]
    /// Waits for host to send link setup with compatible link and protocol version.
    /// Due to internal limitations same message buffer need to be provided here, and it's size
    /// will be communicated to the host as maximum message size.
    pub async fn wait_link_connection(
        &mut self,
        message: &mut [u8],
    ) -> Result<(), Error<T::Error, R::Error>> {
        while self.remote_protocol.is_none() {
            match self.receive_message(message).await {
                Ok(MessageKind::LinkInfo {
                    remote_max_message_size,
                    remote_protocol,
                }) => {
                    #[cfg(feature = "defmt")]
                    defmt::trace!(
                        "Link established, remote max message size: {}, remote protocol: {}",
                        remote_max_message_size,
                        remote_protocol
                    );
                    break;
                }
                Ok(_) => continue, // shouldn't happen, but exit if setup is actually done
                Err(Error::ProtocolsVersionMismatch) => {
                    #[cfg(feature = "defmt")]
                    defmt::warn!("Ignoring data before link setup");
                    continue;
                }
                Err(e) => return Err(e),
            }
        }
        self.tx_stats = Default::default();
        self.rx_stats = Default::default();
        Ok(())
    }

    /// Returns remote protocol information.
    pub fn remote_protocol(&self) -> Option<ProtocolInfo> {
        self.remote_protocol
    }

    /// Returns statistics struct.
    pub fn receiver_stats(&self) -> &ReceiverStats {
        &self.rx_stats
    }
}
