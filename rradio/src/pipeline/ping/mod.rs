use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use rradio_messages::PingTimes;
use tokio::sync::mpsc;

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

impl From<mpsc::error::SendError<PingTimes>> for PingInterruption {
    fn from(_: mpsc::error::SendError<PingTimes>) -> Self {
        log::error!("Could not send ping times");
        Self::Finished
    }
}

struct Pinger {
    gateway_address: Ipv4Addr,
    ping_count: usize,
    ipv4_pinger: ipv4::Pinger,
    track_urls: mpsc::UnboundedReceiver<Option<String>>,
    ping_times: mpsc::UnboundedSender<PingTimes>,
}

impl Pinger {
    async fn check_for_new_track(&mut self) -> Result<(), PingInterruption> {
        match tokio::time::timeout(PING_INTERVAL, self.track_urls.recv()).await {
            Ok(Some(Some(track))) => Err(PingInterruption::NewTrack(track)),
            Ok(Some(None)) => Err(PingInterruption::SuspendUntilNewTrack),
            Ok(None) => Err(PingInterruption::Finished),
            Err(_) => Ok(()),
        }
    }

    async fn ping_address(
        &mut self,
        name: &str,
        address: Ipv4Addr,
    ) -> Result<Result<Duration, ()>, PingInterruption> {
        self.check_for_new_track().await?;

        tokio::task::block_in_place(|| {
            Ok(self.ipv4_pinger.ping(address).map_err(|err| {
                log::error!("Failed to ping {} ({:?}): {}", name, address, err);
            }))
        })
    }

    async fn ping_gateway(&mut self) -> Result<Result<Duration, ()>, PingInterruption> {
        self.ping_address("gateway", self.gateway_address).await
    }

    async fn ping_remote(
        &mut self,
        address: Ipv4Addr,
    ) -> Result<Result<Duration, ()>, PingInterruption> {
        self.ping_address("remote", address).await
    }

    #[allow(clippy::needless_pass_by_value)]
    async fn run_sequence(&mut self, track_url_str: String) -> Result<Never, PingInterruption> {
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
            match self.ping_gateway().await? {
                Ok(gateway_ping) => self.ping_times.send(PingTimes::OnlyGateway(gateway_ping))?,
                Err(()) => {
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
            let gateway_ping = match self.ping_gateway().await? {
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

                    remote_ping = match self.ping_remote(remote_address).await? {
                        Ok(ping) => ping,
                        Err(()) => continue 'retry,
                    };

                    self.ping_times.send(PingTimes::RemoteAndGateway {
                        remote_ping,
                        gateway_ping,
                    })?;

                    gateway_ping = match self.ping_gateway().await? {
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
                match self.ping_gateway().await? {
                    Ok(gateway_ping) => {
                        self.ping_times.send(PingTimes::OnlyGateway(gateway_ping))?
                    }
                    Err(()) => continue 'retry,
                };
            }
        }
    }

    async fn run(mut self) {
        loop {
            let mut track_url_str = match self.track_urls.recv().await {
                Some(Some(track_url)) => track_url,
                Some(None) => continue,
                None => return,
            };

            loop {
                match self.run_sequence(track_url_str).await.unwrap_err() {
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

pub fn run(
    config: &crate::config::Config,
) -> Result<
    (
        impl std::future::Future<Output = ()>,
        mpsc::UnboundedSender<Option<String>>,
        mpsc::UnboundedReceiver<PingTimes>,
    ),
    ipv4::PermissionsError,
> {
    let ipv4_pinger = ipv4::Pinger::new()?;

    let (track_url_tx, track_url_rx) = mpsc::unbounded_channel::<Option<String>>();

    let (ping_time_tx, ping_time_rx) = mpsc::unbounded_channel();

    let task = Pinger {
        gateway_address: config.gateway_address,
        ping_count: config.ping_count,
        ipv4_pinger,
        track_urls: track_url_rx,
        ping_times: ping_time_tx,
    }
    .run();

    Ok((task, track_url_tx, ping_time_rx))
}
