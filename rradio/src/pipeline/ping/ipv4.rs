use std::{
    net::Ipv4Addr,
    time::{Duration, Instant},
};

use pnet::{
    packet::{
        icmp::{
            echo_reply::{EchoReplyPacket, IcmpCodes},
            echo_request, IcmpTypes,
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
        let mut packet_iter = icmp_packet_iter(&mut self.receiver);

        while packet_iter
            .next_with_timeout(std::time::Duration::from_micros(1))
            .map_err(|io_err| {
                let err = PingError::FailedToRecieveICMP;
                log::error!("{}: {}", err, io_err);
                err
            })?
            .is_some()
        {
            // purge buffer of previous packets
        }

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
                log::error!("{} to {}: {}", err, address, io_err);
                err
            })?;

        let send_time = Instant::now();

        loop {
            let (packet, remote_address) = packet_iter
                .next_with_timeout(std::time::Duration::from_secs(4))
                .map_err(|io_err| {
                    let err = PingError::FailedToRecieveICMP;
                    log::error!("{}: {}", err, io_err);
                    err
                })?
                .ok_or(PingError::Timeout)?;
            let ping_time = Instant::now().saturating_duration_since(send_time);

            match packet.get_icmp_type() {
                IcmpTypes::EchoReply => (),
                IcmpTypes::DestinationUnreachable => {
                    let err = PingError::DestinationUnreachable;
                    log::error!("{}: {}", err, address);
                    return Err(err);
                }
                icmp_type => {
                    println!("Ignoring {:?}", icmp_type);
                    continue;
                }
            }

            if packet.get_icmp_code() != IcmpCodes::NoCode {
                println!("Invalid ICMP Packet");
                continue;
            }

            if remote_address != address {
                println!("Unexpected ping response from {:<16}:", remote_address);
                continue;
            }

            let echo_packet = EchoReplyPacket::new(packet.packet()).unwrap();

            let echo_sequence_number = echo_packet.get_sequence_number();
            if sequence_number != echo_sequence_number {
                println!(
                    "IPV4 packet with invalid sequence number: Request: {}; Response: {}",
                    sequence_number, echo_sequence_number
                );
                continue; // Ignore unexpected packet
            }

            let echo_identifier = echo_packet.get_identifier();
            if identifier != echo_identifier {
                println!(
                    "IPV4 packet with invalid identifier: Request: {}; Response: {}",
                    identifier, echo_identifier
                );
                continue; // Ignore unexpected packet
            }

            log::info!("Ping time: {}ms", ping_time.as_secs_f32() * 1000.0);

            return Ok(ping_time);
        }
    }
}
