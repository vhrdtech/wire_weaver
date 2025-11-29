use crate::common::{DisconnectReason, Error, Op, WireWeaverUsbLink};
use crate::{CRC_KIND, MIN_MESSAGE_SIZE, PacketSink, PacketSource};
use shrink_wrap::{BufReader, DeserializeShrinkWrap, SerializeShrinkWrap};
use wire_weaver::prelude::FullVersion;

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
    /// Loopback data
    #[cfg(feature = "host")]
    Loopback {
        seq: u32,
        len: usize,
    },
    /// Ping from the other end
    Ping,
    /// Link is up, versions are compatible, ready to transfer application data
    LinkUp,
    #[cfg(feature = "host")]
    DeviceInfo {
        max_message_len: u32,
        link_version: wire_weaver::ww_version::CompactVersion,
    },
    Disconnect(DisconnectReason),
}

impl<T: PacketSink, R: PacketSource> WireWeaverUsbLink<'_, T, R> {
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
                    .read_packet(self.rx_packet_buf)
                    .await
                    .map_err(Error::SourceError)?;
                self.rx_start_pos = 0;
                self.rx_stats.packets_received = self.rx_stats.packets_received.wrapping_add(1);
                if len == 0 {
                    self.rx_left_bytes = 0;
                    break Err(Error::ReceivedEmptyPacket);
                }
                (&self.rx_packet_buf[..len], true)
            };
            // println!("rx frame: {:?}", frame);
            let mut rd = BufReader::new(packet);
            while rd.bytes_left() >= 2 {
                let kind = rd.read_u4().map_err(|_| Error::InternalBufOverflow)?;
                let Some(kind) = Op::from_repr(kind) else {
                    self.continue_with_new_packet(); // skip whole packet on malformed data
                    continue 'next_message;
                };
                if !self.is_link_up()
                    && kind != Op::GetDeviceInfo
                    && kind != Op::DeviceInfo
                    && kind != Op::LinkSetup
                    && kind != Op::LinkReady
                    && kind != Op::Nop
                    && kind != Op::Ping // could be received during link setup if host did not send Disconnected and reconnected faster than timeout on device
                    && kind != Op::Loopback
                {
                    self.continue_with_new_packet();
                    return Err(Error::UnexpectedOp(kind));
                }
                let len11_8 = rd.read_u4().map_err(|_| Error::InternalBufOverflow)?;
                let len7_0 = rd.read_u8().map_err(|_| Error::InternalBufOverflow)?;
                let len = ((len11_8 as usize) << 8) | len7_0 as usize;
                match kind {
                    Op::Nop => {}
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
                    #[cfg(feature = "device")]
                    Op::GetDeviceInfo => {
                        self.send_device_info(message.len() as u32).await?;
                        continue 'next_message;
                    }
                    #[cfg(feature = "host")]
                    Op::DeviceInfo => {
                        let device_info = crate::common::DeviceInfo::des_shrink_wrap(&mut rd)
                            .map_err(|_| Error::InternalBufOverflow)?;
                        self.remote_max_message_size = device_info.dev_max_message_len;
                        self.remote_protocol
                            .set(|wr| device_info.dev_user_version.ser_shrink_wrap(wr))
                            .map_err(|_| Error::InternalBufOverflow)?;

                        let max_message_len = device_info.dev_max_message_len;
                        let link_version = device_info.dev_link_version;
                        self.continue_with_new_packet();
                        return Ok(MessageKind::DeviceInfo {
                            max_message_len,
                            link_version,
                        });
                    }
                    #[cfg(feature = "device")]
                    Op::LinkSetup => {
                        let link_setup = crate::common::LinkSetup::des_shrink_wrap(&mut rd)
                            .map_err(|_| Error::InternalBufOverflow)?;
                        let protocol_compatible = self
                            .user_protocol
                            .is_protocol_compatible(&link_setup.host_user_version);
                        // when host app is generic, and it will work with API dynamically by requesting serialized AST from device first
                        let dynamic_host = link_setup.host_user_version.crate_id.is_empty();
                        if protocol_compatible || dynamic_host {
                            self.remote_max_message_size = link_setup.host_max_message_len;
                            self.remote_protocol
                                .set(|wr| link_setup.host_user_version.ser_shrink_wrap(wr))
                                .map_err(|_| Error::InternalBufOverflow)?;
                        } else {
                            self.remote_protocol.clear();
                        }
                        self.send_link_setup_result().await?;
                        self.continue_with_new_packet();
                        return Ok(MessageKind::LinkUp);
                    }
                    #[cfg(feature = "host")]
                    Op::LinkReady => {
                        self.continue_with_new_packet();
                        return Ok(MessageKind::LinkUp);
                    }
                    Op::Disconnect => {
                        self.remote_protocol.clear();
                        self.remote_max_message_size = MIN_MESSAGE_SIZE as u32;
                        let reason = DisconnectReason::des_shrink_wrap(&mut rd)
                            .map_err(|_| Error::InternalBufOverflow)?;
                        self.continue_with_new_packet();
                        return Ok(MessageKind::Disconnect(reason));
                    }
                    Op::Ping => {
                        self.adjust_read_pos(is_new_frame, rd.bytes_left(), packet.len());
                        return Ok(MessageKind::Ping);
                    }
                    Op::Loopback => {
                        let _repeat = rd.read_u32().map_err(|_| Error::InternalBufOverflow)?;
                        let seq = rd.read_u32().map_err(|_| Error::InternalBufOverflow)?;
                        let data = rd
                            .read_raw_slice(rd.bytes_left())
                            .map_err(|_| Error::InternalBufOverflow)?;
                        let len = data.len();
                        message[..len].copy_from_slice(data); // cannot mutably use self otherwise
                        #[cfg(feature = "device")]
                        {
                            let data = &message[..len];
                            if _repeat == 0 {
                                self.continue_with_new_packet();
                            } else {
                                let mut seq = if _repeat == 1 { seq } else { 0 };
                                for _ in 0.._repeat {
                                    self.send_loopback(0, seq, data).await?;
                                    seq += 1;
                                }
                            }
                            continue 'next_message;
                        }
                        #[cfg(feature = "host")]
                        {
                            return Ok(MessageKind::Loopback { seq, len });
                        }
                    }
                    _ => {
                        continue 'next_message;
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
                self.continue_with_new_packet();
            }
        }
    }

    /// New packet will be awaited in next receive_message() call
    fn continue_with_new_packet(&mut self) {
        self.rx_start_pos = 0;
        self.rx_left_bytes = 0;
    }

    #[cfg(feature = "device")]
    /// Waits for host to send link setup with compatible link and protocol version.
    /// Due to internal limitations same message buffer need to be provided here, and it's size
    /// will be communicated to the host as maximum message size.
    pub async fn wait_link_connection(
        &mut self,
        message: &mut [u8],
    ) -> Result<(), Error<T::Error, R::Error>> {
        while !self.is_link_up() {
            match self.receive_message(message).await {
                Ok(MessageKind::LinkUp) => {
                    #[cfg(feature = "defmt")]
                    defmt::trace!("LinkUp");
                    // if versions_matches {
                    //     break;
                    // }
                    break;
                    // wait for another LinkSetup
                    // continue;
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
    pub fn remote_protocol(&self) -> Result<FullVersion<'_>, shrink_wrap::Error> {
        self.remote_protocol.get()
    }

    /// Returns statistics struct.
    pub fn receiver_stats(&self) -> &ReceiverStats {
        &self.rx_stats
    }
}
