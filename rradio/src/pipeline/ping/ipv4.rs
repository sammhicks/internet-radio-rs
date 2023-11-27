use std::{
    net::{IpAddr, Ipv4Addr},
    time::{Duration, Instant},
};

use pnet::{
    packet::{
        icmp::{
            echo_reply::{EchoReplyPacket, IcmpCodes},
            echo_request, IcmpPacket, IcmpTypes,
        },
        ip::IpNextHeaderProtocols,
        Packet,
    },
    transport::{
        icmp_packet_iter, TransportChannelType::Layer4, TransportProtocol::Ipv4, TransportReceiver,
        TransportSender,
    },
};

use rradio_messages::PingError;

pub struct Pinger {
    sender: TransportSender,
    receiver: TransportReceiver,
}

#[derive(Debug, thiserror::Error)]
#[error("Permission Error. Try running as root.")]
pub struct PermissionsError(#[from] std::io::Error);

struct IcmpTransportChannelIterator<'a>(pnet::transport::IcmpTransportChannelIterator<'a>);

impl<'a> IcmpTransportChannelIterator<'a> {
    fn clear(&mut self) -> Result<(), PingError> {
        while self
            .try_next(std::time::Duration::from_micros(1))?
            .is_some()
        {}

        Ok(())
    }

    fn next(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<(IcmpPacket<'_>, IpAddr), PingError> {
        self.try_next(timeout)?.ok_or(PingError::Timeout)
    }

    fn try_next(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<Option<(IcmpPacket<'_>, IpAddr)>, PingError> {
        self.0.next_with_timeout(timeout).map_err(|io_err| {
            let err = PingError::FailedToRecieveICMP;
            tracing::error!("{}: {}", err, io_err);
            err
        })
    }
}

impl Pinger {
    pub fn new() -> Result<Self, PermissionsError> {
        const BUFFER_SIZE: usize = 64;

        let (sender, receiver) = pnet::transport::transport_channel(
            BUFFER_SIZE,
            Layer4(Ipv4(IpNextHeaderProtocols::Icmp)),
        )?;
        Ok(Self { sender, receiver })
    }

    pub fn ping(&mut self, address: Ipv4Addr) -> Result<Duration, PingError> {
        tracing::debug!("Pinging {}", address);

        let mut packet_iter: IcmpTransportChannelIterator =
            IcmpTransportChannelIterator(icmp_packet_iter(&mut self.receiver));

        packet_iter.clear()?;

        let sequence_number = rand::random();
        let identifier = rand::random();
        let mut buffer = [0_u8; 16];

        let mut echo_packet = echo_request::MutableEchoRequestPacket::new(&mut buffer[..]).unwrap();

        echo_packet.set_sequence_number(sequence_number);
        echo_packet.set_identifier(identifier);
        echo_packet.set_icmp_type(IcmpTypes::EchoRequest);
        echo_packet.set_checksum(pnet::util::checksum(echo_packet.packet(), 1));

        self.sender
            .send_to(echo_packet, address.into())
            .map_err(|io_err| {
                let err = PingError::FailedToSendICMP;
                tracing::error!("{} to {}: {}", err, address, io_err);
                err
            })?;

        let send_time = Instant::now();

        loop {
            tracing::trace!("Waiting for next icmp message");

            let (packet, remote_address) = packet_iter.next(std::time::Duration::from_secs(4))?;
            let ping_time = Instant::now().saturating_duration_since(send_time);

            match packet.get_icmp_type() {
                IcmpTypes::EchoReply => tracing::trace!("Got ping reply"),
                IcmpTypes::DestinationUnreachable => {
                    let err = PingError::DestinationUnreachable;
                    tracing::error!("{}: {}", err, address);
                    return Err(err);
                }
                icmp_type => {
                    tracing::debug!("Ignoring {:?}", icmp_type);
                    continue;
                }
            }

            if packet.get_icmp_code() != IcmpCodes::NoCode {
                tracing::debug!("Ignoring Invalid ICMP Packet");
                continue;
            }

            if remote_address != address {
                tracing::debug!(
                    "Ignoring Unexpected ping response from {:<16}:",
                    remote_address
                );
                continue;
            }

            let echo_packet = EchoReplyPacket::new(packet.packet()).unwrap();

            let echo_sequence_number = echo_packet.get_sequence_number();
            if sequence_number != echo_sequence_number {
                tracing::debug!(
                    "IPV4 packet with invalid sequence number: Request: {}; Response: {}",
                    sequence_number,
                    echo_sequence_number
                );
                continue; // Ignore unexpected packet
            }

            let echo_identifier = echo_packet.get_identifier();
            if identifier != echo_identifier {
                tracing::debug!(
                    "IPV4 packet with invalid identifier: Request: {}; Response: {}",
                    identifier,
                    echo_identifier
                );
                continue; // Ignore unexpected packet
            }

            tracing::debug!(
                "Ping time to {:>16}: {:.3}ms",
                address,
                ping_time.as_secs_f32() * 1000.0
            );

            return Ok(ping_time);
        }
    }
}
