use crate::common::{DisconnectReason, Error, Op, WireWeaverUsbLink};
use crate::{CRC_KIND, PacketSink, PacketSource};
use shrink_wrap::SerializeShrinkWrap;
use wire_weaver::MessageSink;

/// Can be used to monitor how many messages, packets and bytes were sent since link setup.
#[derive(Default, Debug, Copy, Clone)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct SenderStats {
    pub messages_sent: u32,
    pub packets_sent: u32,
    /// Only message bytes are counted
    pub bytes_sent: u64,
}

impl<'i, T: PacketSink, R: PacketSource> WireWeaverUsbLink<'i, T, R> {
    /// Sends NOP and immediately forces a transmission, without waiting for other packets to accumulate.
    /// If USB data toggle bits are messed up, this will ensure that no useful data packets are lost.
    /// Windows seem to ignore this, while Linux and Mac do not.
    pub async fn send_nop(&mut self) -> Result<(), Error<T::Error, R::Error>> {
        if self.tx_writer.bytes_left() < 2 {
            self.force_send().await?;
        }
        self.write_op_len(Op::Nop, 0)?;
        self.force_send().await?;
        Ok(())
    }

    #[cfg(feature = "host")]
    pub async fn send_get_device_info(&mut self) -> Result<(), Error<T::Error, R::Error>> {
        // -> seem to be fixed with not calling set_alt_setting from host side.
        // -> encountered missing packet anyway, but much, much rarer event
        self.send_nop().await?;

        // no need because send_nop() sends a packet
        // if self.tx_writer.bytes_left() < 2 {
        //     self.force_send().await?;
        // }
        self.write_op_len(Op::GetDeviceInfo, 0)?;
        self.force_send().await?;
        Ok(())
    }

    #[cfg(feature = "device")]
    pub async fn send_device_info(
        &mut self,
        max_message_size: u32,
    ) -> Result<(), Error<T::Error, R::Error>> {
        // See send_get_device_info comment for explanation on why this is sent.
        self.send_nop().await?;

        self.write_op_len(Op::DeviceInfo, 0)?; // packet is sent right away and whole buffer is used by ShrinkWrap to deserialize DeviceInfo

        use wire_weaver::shrink_wrap::UNib32;
        let dev_info = crate::common::DeviceInfo {
            dev_link_version: wire_weaver::ww_version::CompactVersion {
                global_type_id: ww_global::WIRE_WEAVER_USB_LINK,
                major: UNib32(
                    env!("CARGO_PKG_VERSION_MAJOR")
                        .parse::<u32>()
                        .expect("Cargo version"),
                ),
                minor: UNib32(
                    env!("CARGO_PKG_VERSION_MINOR")
                        .parse::<u32>()
                        .expect("Cargo version"),
                ),
                patch: UNib32(
                    env!("CARGO_PKG_VERSION_PATCH")
                        .parse::<u32>()
                        .expect("Cargo version"),
                ),
            },
            dev_user_version: self.user_protocol.clone(),
            dev_max_message_len: max_message_size,
        };
        dev_info
            .ser_shrink_wrap(&mut self.tx_writer)
            .map_err(|_| Error::InternalBufOverflow)?;

        self.force_send().await?;
        Ok(())
    }

    /// Sends link setup from the host to device. Called automatically on device side in [wait_for_link()](Self::wait_for_link)
    #[cfg(feature = "host")]
    pub async fn send_link_setup(
        &mut self,
        max_message_size: u32,
    ) -> Result<(), Error<T::Error, R::Error>> {
        self.write_op_len(Op::LinkSetup, 0)?; // packet is sent right away and whole buffer is used by ShrinkWrap to deserialize LinkSetup

        let link_setup = crate::common::LinkSetup {
            host_user_version: self.user_protocol.clone(),
            host_max_message_len: max_message_size,
        };
        link_setup
            .ser_shrink_wrap(&mut self.tx_writer)
            .map_err(|_| Error::InternalBufOverflow)?;

        self.force_send().await?;
        Ok(())
    }

    #[cfg(feature = "device")]
    pub async fn send_link_setup_result(&mut self) -> Result<(), Error<T::Error, R::Error>> {
        if self.is_protocol_compatible() {
            self.write_op_len(Op::LinkReady, 0)?;
        } else {
            self.write_op_len(Op::Disconnect, 0)?;
            DisconnectReason::IncompatibleVersion
                .ser_shrink_wrap(&mut self.tx_writer)
                .map_err(|_| Error::InternalBufOverflow)?;
        }

        // packet is sent right away and whole buffer is used by ShrinkWrap to deserialize DisconnectReason
        self.force_send().await?;
        Ok(())
    }

    /// Tries to write provided message bytes into the current packet.
    /// If message fits, nothing will be actually sent to the sink just yet.
    /// If it doesn't fit, one or more packets will be sent immediately to send the whole message,
    /// except possibly the last piece of it.
    ///
    /// Maximum message length is limited to the remote buffers length, negotiated during link setup phase.
    ///
    /// [force_send()](Self::force_send) can be called to send all the accumulated messages immediately.
    /// Intended use is to call force_send periodically, so that receiver sees messages no older,
    /// than chosen period.
    pub async fn send_message(&mut self, message: &[u8]) -> Result<(), Error<T::Error, R::Error>> {
        if !self.is_link_up() {
            return Err(Error::Disconnected);
        }
        if message.len() > self.remote_max_message_size as usize {
            return Err(Error::MessageTooBig);
        }
        if message.len() + 2 <= self.tx_writer.bytes_left() {
            // message fits fully into current packet
            self.write_packet_start_end(message)?;
            self.tx_stats.messages_sent = self.tx_stats.messages_sent.wrapping_add(1);
            self.tx_stats.bytes_sent = self.tx_stats.bytes_sent.wrapping_add(message.len() as u64);
            // need at least 3 bytes for next message
            if self.tx_writer.bytes_left() < 3 {
                self.force_send().await?;
            }
        } else {
            let mut remaining_bytes = message;
            let mut crc_in_next_packet = None;
            let mut is_first_chunk = true;
            while !remaining_bytes.is_empty() {
                if self.tx_writer.bytes_left() < 3 {
                    self.force_send().await?;
                }
                let len_chunk = remaining_bytes.len().min(self.tx_writer.bytes_left() - 2);
                // .min(max_remote_packet_size);
                let kind = if is_first_chunk {
                    is_first_chunk = false;
                    Op::MessageStart
                } else if remaining_bytes.len() - len_chunk > 0 {
                    Op::MessageContinue
                } else if self.tx_writer.bytes_left() - len_chunk - 2 >= 2 {
                    // CRC will fit
                    Op::MessageEnd
                } else {
                    // CRC in the next packet with 0 remaining bytes of the message
                    let crc = CRC_KIND.checksum(message);
                    crc_in_next_packet = Some(crc);
                    Op::MessageContinue
                };
                self.write_op_len(kind, len_chunk as u16)?;
                self.tx_writer
                    .write_raw_slice(&remaining_bytes[..len_chunk])
                    .map_err(|_| Error::InternalBufOverflow)?;
                remaining_bytes = &remaining_bytes[len_chunk..];
                if kind == Op::MessageEnd {
                    let crc = CRC_KIND.checksum(message);
                    self.tx_writer
                        .write_u16(crc)
                        .map_err(|_| Error::InternalBufOverflow)?;
                    self.tx_stats.messages_sent = self.tx_stats.messages_sent.wrapping_add(1);
                    self.tx_stats.bytes_sent =
                        self.tx_stats.bytes_sent.wrapping_add(message.len() as u64);
                }
            }
            if let Some(crc) = crc_in_next_packet {
                if self.tx_writer.bytes_left() < 2 {
                    self.force_send().await?;
                }
                self.write_op_len(Op::MessageEnd, 0)?;
                self.tx_writer
                    .write_u16(crc)
                    .map_err(|_| Error::InternalBufOverflow)?;
            }
            if self.tx_writer.bytes_left() < 3 {
                // TODO: Send multi-packet message immediately or wait for more messages?
                self.force_send().await?;
            }
        }
        Ok(())
    }

    /// Sends Ping message and immediately forces a packet transmission.
    pub async fn send_ping(&mut self) -> Result<(), Error<T::Error, R::Error>> {
        if self.tx_writer.bytes_left() < 2 {
            self.force_send().await?;
        }
        self.write_op_len(Op::Ping, 0)
            .map_err(|_| Error::InternalBufOverflow)?;
        self.force_send().await?;
        Ok(())
    }

    /// Sends Disconnect message, forces immediate packet transmission and marks link as not connected,
    /// to no accidentally receive data from incompatible host application.
    pub async fn send_disconnect(
        &mut self,
        reason: DisconnectReason,
    ) -> Result<(), Error<T::Error, R::Error>> {
        if self.tx_writer.bytes_left() < 4 {
            self.force_send().await?;
        }
        self.write_op_len(Op::Disconnect, 0)?; // DisconnectReason is variable length and can only be last in packet, ShrinkWrap is used to deserialize it
        reason
            .ser_shrink_wrap(&mut self.tx_writer)
            .map_err(|_| Error::InternalBufOverflow)?;
        self.force_send().await?;
        self.silent_disconnect();
        Ok(())
    }

    fn write_packet_start_end(&mut self, bytes: &[u8]) -> Result<(), Error<T::Error, R::Error>> {
        self.write_op_len(Op::MessageStartEnd, bytes.len() as u16)?;
        self.tx_writer
            .write_raw_slice(bytes)
            .map_err(|_| Error::InternalBufOverflow)?;
        Ok(())
    }

    fn write_op_len(&mut self, op: Op, len: u16) -> Result<(), Error<T::Error, R::Error>> {
        self.tx_writer
            .write_u4(op as u8)
            .map_err(|_| Error::InternalBufOverflow)?;
        let len11_8 = (len >> 8) as u8;
        let len7_0 = (len & 0xFF) as u8;
        self.tx_writer
            .write_u4(len11_8)
            .map_err(|_| Error::InternalBufOverflow)?;
        self.tx_writer
            .write_u8(len7_0)
            .map_err(|_| Error::InternalBufOverflow)?;
        Ok(())
    }

    /// Forces immediate transmission of a packet, if it's not empty.
    pub async fn force_send(&mut self) -> Result<(), Error<T::Error, R::Error>> {
        let data = self
            .tx_writer
            .finish()
            .map_err(|_| Error::InternalBufOverflow)?;
        if !data.is_empty() {
            self.tx.write_packet(data).await.map_err(Error::SinkError)?;
        }
        self.tx_stats.packets_sent = self.tx_stats.packets_sent.wrapping_add(1);
        Ok(())
    }

    /// Returns true if there are no queued messages
    pub fn is_tx_queue_empty(&self) -> bool {
        self.tx_writer.pos().0 == 0
    }

    /// Returns original buffer and sink.
    pub fn de_init(self) -> (&'i mut [u8], T, R) {
        (self.tx_writer.deinit(), self.tx, self.rx)
    }

    /// Returns maximum remote message size received during link setup. Or default one defined as
    /// [MIN_MESSAGE_SIZE]
    pub fn remote_max_message_size(&self) -> u32 {
        self.remote_max_message_size
    }

    /// Returns statistics struct.
    pub fn sender_stats(&self) -> &SenderStats {
        &self.tx_stats
    }
}

impl<'i, T: PacketSink, R: PacketSource> MessageSink for WireWeaverUsbLink<'i, T, R> {
    async fn send(&mut self, message: &[u8]) -> Result<(), ()> {
        self.send_message(message).await.map_err(|_| ())
    }
}
