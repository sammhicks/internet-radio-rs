use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use rradio_messages::PingTimes;

mod ipv4;

const PING_INTERVAL: Duration = Duration::from_secs(1);

#[allow(clippy::clippy::empty_enum)]
#[derive(Debug)]
enum Never {}

enum PingInterruption {
    Finished,
    SuspendUntilNewTrack,
    NewTrack(String),
}

impl From<tokio::sync::mpsc::error::SendError<PingTimes>> for PingInterruption {
    fn from(_: tokio::sync::mpsc::error::SendError<PingTimes>) -> Self {
        log::error!("Could not send ping times");
        Self::Finished
    }
}

struct Pinger {
    gateway_address: Ipv4Addr,
    ping_count: usize,
    ipv4_pinger: ipv4::Pinger,
    track_urls: std::sync::mpsc::Receiver<Option<String>>,
    ping_times: tokio::sync::mpsc::UnboundedSender<PingTimes>,
}

impl Pinger {
    fn check_for_new_track(&mut self) -> Result<(), PingInterruption> {
        match self.track_urls.try_recv() {
            Ok(Some(track)) => Err(PingInterruption::NewTrack(track)),
            Ok(None) => Err(PingInterruption::SuspendUntilNewTrack),
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok(()),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(PingInterruption::Finished),
        }
    }

    fn ping_address(
        &mut self,
        name: &str,
        address: Ipv4Addr,
    ) -> Result<Result<Duration, ()>, PingInterruption> {
        self.check_for_new_track()?;

        Ok(self.ipv4_pinger.ping(address).map_err(|err| {
            log::error!("Failed to ping {} ({:?}): {}", name, address, err);
        }))
    }

    fn ping_gateway(&mut self) -> Result<Result<Duration, ()>, PingInterruption> {
        self.ping_address("gateway", self.gateway_address)
    }

    fn ping_remote(&mut self, address: Ipv4Addr) -> Result<Result<Duration, ()>, PingInterruption> {
        self.ping_address("remote", address)
    }

    #[allow(clippy::needless_pass_by_value)]
    fn run_sequence(&mut self, track_url_str: String) -> Result<Never, PingInterruption> {
        let track_url = url::Url::parse(&track_url_str).map_err(|err| {
            log::error!("Bad url ({:?}): {}", track_url_str, err);
            PingInterruption::SuspendUntilNewTrack
        })?;

        let scheme = track_url.scheme();
        if let "file" | "cdda" = scheme {
            log::info!("Ignoring {}", scheme);
            return Err(PingInterruption::SuspendUntilNewTrack);
        }

        let host = track_url.host_str().ok_or_else(|| {
            log::error!("No host ({:?})", track_url_str);
            PingInterruption::SuspendUntilNewTrack
        })?;

        let mut dns_addresses = loop {
            match self.ping_gateway()? {
                Ok(gateway_ping) => self.ping_times.send(PingTimes::OnlyGateway(gateway_ping))?,
                Err(()) => {
                    std::thread::sleep(PING_INTERVAL);
                    continue;
                }
            }

            match std::net::ToSocketAddrs::to_socket_addrs(&(host, 0)) {
                Ok(addrs) => break addrs,
                Err(err) => {
                    log::error!("Could not resolve DNS ({:?}): {}", host, err);
                }
            }
        };

        let remote_address = loop {
            match dns_addresses.next() {
                Some(SocketAddr::V4(ipv4_address)) => break *ipv4_address.ip(),
                Some(SocketAddr::V6(ipv6_address)) => {
                    log::debug!("Ignoring ipv6 address ({:?}): {}", host, ipv6_address)
                }
                None => {
                    log::error!("No addresses ({:?})", host);
                    return Err(PingInterruption::SuspendUntilNewTrack);
                }
            }
        };

        'retry: loop {
            std::thread::sleep(PING_INTERVAL);
            let gateway_ping = match self.ping_gateway()? {
                Ok(ping) => ping,
                Err(()) => continue,
            };

            self.ping_times.send(PingTimes::OnlyGateway(gateway_ping))?;

            {
                let mut gateway_ping = gateway_ping;
                let mut remote_ping;

                let mut remote_pings_remaining = self.ping_count;

                while remote_pings_remaining > 0 {
                    remote_pings_remaining -= 1;

                    std::thread::sleep(PING_INTERVAL);

                    remote_ping = match self.ping_remote(remote_address)? {
                        Ok(ping) => ping,
                        Err(()) => continue 'retry,
                    };

                    self.ping_times.send(PingTimes::RemoteAndGateway {
                        remote_ping,
                        gateway_ping,
                    })?;

                    std::thread::sleep(PING_INTERVAL);

                    gateway_ping = match self.ping_gateway()? {
                        Ok(ping) => ping,
                        Err(()) => continue 'retry,
                    };

                    self.ping_times.send(PingTimes::RemoteAndGateway {
                        remote_ping,
                        gateway_ping,
                    })?;
                }
            }

            loop {
                std::thread::sleep(PING_INTERVAL);

                match self.ping_gateway()? {
                    Ok(gateway_ping) => {
                        self.ping_times.send(PingTimes::OnlyGateway(gateway_ping))?
                    }
                    Err(()) => continue 'retry,
                };
            }
        }
    }

    fn run(mut self) {
        loop {
            let mut track_url_str = match self.track_urls.recv() {
                Ok(Some(track_url)) => track_url,
                Ok(None) => continue,
                Err(std::sync::mpsc::RecvError) => return,
            };

            loop {
                match self.run_sequence(track_url_str).unwrap_err() {
                    PingInterruption::Finished => return,
                    PingInterruption::SuspendUntilNewTrack => break,
                    PingInterruption::NewTrack(new_track_url_str) => {
                        track_url_str = new_track_url_str;
                        continue;
                    }
                }
            }
        }
    }
}

pub type Handles = (
    std::thread::JoinHandle<()>,
    std::sync::mpsc::Sender<Option<String>>,
    tokio::sync::mpsc::UnboundedReceiver<PingTimes>,
);

pub fn run(config: crate::config::Config) -> Result<Handles, ipv4::PermissionsError> {
    let ipv4_pinger = ipv4::Pinger::new()?;

    let (track_url_tx, track_url_rx) = std::sync::mpsc::channel::<Option<String>>();

    let (ping_time_tx, ping_time_rx) = tokio::sync::mpsc::unbounded_channel();

    let handle = std::thread::spawn(move || {
        Pinger {
            gateway_address: config.gateway_address,
            ping_count: config.ping_count,
            ipv4_pinger,
            track_urls: track_url_rx,
            ping_times: ping_time_tx,
        }
        .run();
    });

    Ok((handle, track_url_tx, ping_time_rx))
}
