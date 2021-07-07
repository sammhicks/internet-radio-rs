use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use tokio::sync::mpsc;

use rradio_messages::{ArcStr, PingError, PingTarget, PingTimes};

mod ipv4;

const PING_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug)]
enum Never {}

enum PingInterruption {
    Finished,
    SuspendUntilNewTrack,
    NewTrack(ArcStr),
}

impl From<mpsc::error::SendError<PingTimes>> for PingInterruption {
    fn from(_: mpsc::error::SendError<PingTimes>) -> Self {
        log::error!("Could not send ping times");
        Self::Finished
    }
}

#[derive(Clone, Copy, Debug)]
struct FailedToPing(PingError);

struct Pinger {
    gateway_address: Ipv4Addr,
    ping_count: usize,
    ipv4_pinger: ipv4::Pinger,
    track_urls: mpsc::UnboundedReceiver<Option<ArcStr>>,
    ping_times: mpsc::UnboundedSender<PingTimes>,
}

impl Pinger {
    fn parse_url(&mut self, url_str: ArcStr) -> Result<(ArcStr, url::Url), PingInterruption> {
        match url::Url::parse(&url_str) {
            Ok(parsed_url) => Ok((url_str, parsed_url)),
            Err(err) => {
                log::error!("Bad url ({:?}): {}", url_str, err);
                self.ping_times.send(PingTimes::BadUrl)?;
                Err(PingInterruption::SuspendUntilNewTrack)
            }
        }
    }

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
        f: impl FnOnce(Result<Duration, PingError>) -> PingTimes,
    ) -> Result<Result<Duration, FailedToPing>, PingInterruption> {
        self.check_for_new_track().await?;

        let ping_time_result = tokio::task::block_in_place(|| self.ipv4_pinger.ping(address));

        Ok(match ping_time_result {
            Ok(ping_time) => {
                self.ping_times.send(f(Ok(ping_time)))?;
                Ok(ping_time)
            }
            Err(err) => {
                log::error!("Failed to ping {} ({:?}): {}", name, address, err);
                self.ping_times.send(f(Err(err)))?;
                Err(FailedToPing(err))
            }
        })
    }

    async fn ping_gateway(
        &mut self,
        f: impl FnOnce(Result<Duration, PingError>) -> PingTimes,
    ) -> Result<Result<Duration, FailedToPing>, PingInterruption> {
        self.ping_address("gateway", self.gateway_address, f).await
    }

    async fn ping_remote(
        &mut self,
        address: Ipv4Addr,
        f: impl FnOnce(Result<Duration, PingError>) -> PingTimes,
    ) -> Result<Result<Duration, FailedToPing>, PingInterruption> {
        self.ping_address("remote", address, f).await
    }

    async fn get_remote_address(&mut self, host: &str) -> Result<Ipv4Addr, PingInterruption> {
        let dns_addresses = loop {
            let gateway_ping = match self.ping_gateway(PingTimes::Gateway).await? {
                Ok(ping) => ping,
                Err(FailedToPing(_err)) => continue,
            };

            match std::net::ToSocketAddrs::to_socket_addrs(&(host, 0)) {
                Ok(addrs) => break addrs,
                Err(err) => {
                    log::error!("Could not resolve DNS ({:?}): {}", host, err);
                    self.ping_times
                        .send(rradio_messages::PingTimes::GatewayAndRemote {
                            gateway_ping,
                            remote_ping: Err(PingError::Dns),
                            latest: PingTarget::Remote,
                        })?;
                }
            }
        };

        for address in dns_addresses {
            match address {
                SocketAddr::V4(ipv4_address) => return Ok(*ipv4_address.ip()),
                SocketAddr::V6(ipv6_address) => {
                    log::debug!("Ignoring ipv6 address ({:?}): {}", host, ipv6_address)
                }
            }
        }

        log::error!("No addresses ({:?})", host);
        Err(PingInterruption::SuspendUntilNewTrack)
    }

    async fn run_sequence(
        &mut self,
        track_url_str: ArcStr,
        ping_remote_forever: bool,
    ) -> Result<Never, PingInterruption> {
        let (track_url_str, track_url) = self.parse_url(track_url_str)?;

        let scheme = track_url.scheme();
        if let "file" | "cdda" = scheme {
            log::info!("Ignoring {}", scheme);
            return Err(PingInterruption::SuspendUntilNewTrack);
        }

        let host = track_url.host_str().ok_or_else(|| {
            log::error!("No host ({:?})", track_url_str);
            PingInterruption::SuspendUntilNewTrack
        })?;

        let remote_address = self.get_remote_address(host).await?;

        let mut maybe_remote_ping = None;

        'retry: loop {
            let mut gateway_ping = match self
                .ping_gateway(|gateway_ping| match (gateway_ping, maybe_remote_ping) {
                    (Err(err), _) => PingTimes::Gateway(Err(err)),
                    (Ok(gateway_ping), Some(remote_ping)) => PingTimes::GatewayAndRemote {
                        gateway_ping,
                        remote_ping,
                        latest: PingTarget::Gateway,
                    },
                    (Ok(gateway_ping), None) => PingTimes::Gateway(Ok(gateway_ping)),
                })
                .await?
            {
                Ok(ping) => ping,
                Err(FailedToPing(_err)) => continue 'retry,
            };

            let mut remote_pings_remaining = self.ping_count;

            while remote_pings_remaining > 0 {
                if !ping_remote_forever {
                    remote_pings_remaining -= 1;
                }

                let remote_ping_result = self
                    .ping_remote(remote_address, |remote_ping| PingTimes::GatewayAndRemote {
                        gateway_ping,
                        remote_ping,
                        latest: PingTarget::Remote,
                    })
                    .await?;

                maybe_remote_ping = Some(remote_ping_result.map_err(|FailedToPing(err)| err));

                let remote_ping = match remote_ping_result {
                    Ok(ping) => ping,
                    Err(FailedToPing(_err)) => continue 'retry,
                };

                match self
                    .ping_gateway(|result| match result {
                        Ok(gateway_ping) => PingTimes::GatewayAndRemote {
                            gateway_ping,
                            remote_ping: Ok(remote_ping),
                            latest: PingTarget::Gateway,
                        },
                        Err(err) => PingTimes::Gateway(Err(err)),
                    })
                    .await?
                {
                    Ok(ping) => gateway_ping = ping,
                    Err(FailedToPing(_err)) => continue 'retry,
                }
            }

            maybe_remote_ping.take();

            loop {
                if self
                    .ping_gateway(|result| match result {
                        Ok(gateway_ping) => PingTimes::FinishedPingingRemote { gateway_ping },
                        Err(err) => PingTimes::Gateway(Err(err)),
                    })
                    .await?
                    .is_err()
                {
                    continue 'retry;
                };
            }
        }
    }

    async fn run(mut self, initial_ping_address: ArcStr) {
        let initial_url_str = rradio_messages::arcstr::format!("http://{}", initial_ping_address);
        let mut track_url_str = {
            match self.run_sequence(initial_url_str, true).await.unwrap_err() {
                PingInterruption::Finished => return,
                PingInterruption::SuspendUntilNewTrack => loop {
                    match self.track_urls.recv().await {
                        Some(Some(track_url)) => break track_url,
                        Some(None) => continue,
                        None => return,
                    }
                },
                PingInterruption::NewTrack(new_track_url_str) => new_track_url_str,
            }
        };

        loop {
            track_url_str = match self.run_sequence(track_url_str, false).await.unwrap_err() {
                PingInterruption::Finished => return,
                PingInterruption::SuspendUntilNewTrack => loop {
                    match self.track_urls.recv().await {
                        Some(Some(track_url)) => break track_url,
                        Some(None) => continue,
                        None => return,
                    }
                },
                PingInterruption::NewTrack(new_track_url_str) => new_track_url_str,
            };
        }
    }
}

pub fn run(
    config: &crate::config::Config,
) -> Result<
    (
        impl std::future::Future<Output = ()>,
        mpsc::UnboundedSender<Option<ArcStr>>,
        mpsc::UnboundedReceiver<PingTimes>,
    ),
    ipv4::PermissionsError,
> {
    let ipv4_pinger = ipv4::Pinger::new()?;

    let (track_url_tx, track_url_rx) = mpsc::unbounded_channel::<Option<ArcStr>>();

    let (ping_time_tx, ping_time_rx) = mpsc::unbounded_channel();

    log::info!("Gateway address: {:?}", config.gateway_address);

    let task = Pinger {
        gateway_address: config.gateway_address,
        ping_count: config.ping_count,
        ipv4_pinger,
        track_urls: track_url_rx,
        ping_times: ping_time_tx,
    }
    .run(config.initial_ping_address.clone());

    Ok((task, track_url_tx, ping_time_rx))
}
