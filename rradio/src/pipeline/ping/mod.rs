use std::{net::SocketAddr, time::Duration};

use anyhow::Context;

mod ipv4;

struct Pinger {
    ipv4: ipv4::Pinger,
}

fn parse_address(address: &str) -> anyhow::Result<Option<std::net::Ipv4Addr>> {
    let address_url = url::Url::parse(address)?;

    let scheme = address_url.scheme();
    if let "file" | "cdda" = scheme {
        log::info!("Ignoring {}", scheme);
        return Ok(None);
    }

    let host = address_url.host_str().context("No Host")?;

    let mut dns_addresses =
        std::net::ToSocketAddrs::to_socket_addrs(&(host, 0)).context("Failed to get addresses")?;

    loop {
        match dns_addresses.next() {
            Some(SocketAddr::V4(ipv4_address)) => break Ok(Some(*ipv4_address.ip())),
            Some(SocketAddr::V6(ipv6_address)) => {
                log::info!("Ignoring IPV6 Address {} ({:?})", ipv6_address, address);
                continue;
            }
            None => anyhow::bail!("No valid addresses"),
        }
    }
}

pub type Handles = (
    std::thread::JoinHandle<()>,
    std::sync::mpsc::Sender<Option<String>>,
    tokio::sync::mpsc::UnboundedReceiver<Option<Duration>>,
);

pub fn run(ping_count: usize) -> Result<Handles, ipv4::PermissionsError> {
    let mut pinger = Pinger {
        ipv4: ipv4::Pinger::new()?,
    };

    let (ping_address_tx, ping_address_rx) = std::sync::mpsc::channel::<Option<String>>();

    let (ping_time_tx, ping_time_rx) = tokio::sync::mpsc::unbounded_channel();

    let handle = std::thread::spawn(move || loop {
        let mut current_address = match ping_address_rx.recv() {
            Ok(Some(address)) => address,
            Ok(None) => continue,
            Err(std::sync::mpsc::RecvError) => return,
        };

        'parse_loop: loop {
            let ping_ip = match parse_address(&current_address) {
                Ok(Some(ip_address)) => ip_address,
                Ok(None) => break 'parse_loop,
                Err(err) => {
                    log::error!("{:#}: {:?}", err, current_address);
                    break 'parse_loop;
                }
            };

            let mut ping_iter = 0..ping_count;

            current_address = loop {
                if ping_iter.next().is_none() {
                    break 'parse_loop;
                }

                match pinger.ipv4.ping(ping_ip) {
                    Ok(ping_time) => {
                        if ping_time_tx.send(Some(ping_time)).is_err() {
                            return;
                        }
                    }
                    Err(err) => log::error!("Failed to ping: {}", err),
                }
                std::thread::sleep(std::time::Duration::from_secs(1));

                match ping_address_rx.try_recv() {
                    Ok(Some(address)) => break address,
                    Ok(None) => break 'parse_loop,
                    Err(std::sync::mpsc::TryRecvError::Empty) => (),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                }
            };
        }

        if ping_time_tx.send(None).is_err() {
            return;
        }
    });

    Ok((handle, ping_address_tx, ping_time_rx))
}
