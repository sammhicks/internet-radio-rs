use std::{
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use tokio::sync::{mpsc, oneshot};

use rradio_messages::{ArcStr, PingError, PingTarget, PingTimes};

mod ipv4;

const PING_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug)]
enum Never {}

#[derive(Debug)]
enum PingInterruption {
    Finished,
    SuspendUntilNewTrack,
    NewTrack(ArcStr),
}

impl From<mpsc::error::SendError<PingTimes>> for PingInterruption {
    fn from(_: mpsc::error::SendError<PingTimes>) -> Self {
        tracing::error!("Could not send ping times");
        Self::Finished
    }
}

#[derive(Clone, Copy, Debug)]
struct FailedToPing(PingError);

struct Ivp4PingRequest {
    address: Ipv4Addr,
    response_tx: oneshot::Sender<Result<Duration, PingError>>,
}

struct Pinger {
    gateway_address: Ipv4Addr,
    ping_count: usize,
    ipv4_pinger: mpsc::Sender<Ivp4PingRequest>,
    track_urls: mpsc::UnboundedReceiver<Option<ArcStr>>,
    ping_times: mpsc::UnboundedSender<PingTimes>,
}

impl Pinger {
    fn parse_url(&mut self, url_str: ArcStr) -> Result<(ArcStr, url::Url), PingInterruption> {
        match url::Url::parse(&url_str) {
            Ok(parsed_url) => Ok((url_str, parsed_url)),
            Err(err) => {
                tracing::error!("Bad url ({:?}): {}", url_str, err);
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

        tracing::trace!(%address, "Pinging {name}");

        let (response_tx, response_rx) = oneshot::channel();

        self.ipv4_pinger
            .send(Ivp4PingRequest {
                address,
                response_tx,
            })
            .await
            .map_err(|_| {
                tracing::error!("Could not send ping request");
                PingInterruption::Finished
            })?;

        let ping_time_response = response_rx.await.map_err(|_| {
            tracing::error!("pinger worker has died");
            PingInterruption::Finished
        })?;

        Ok(match ping_time_response {
            Ok(ping_time) => {
                self.ping_times.send(f(Ok(ping_time)))?;
                Ok(ping_time)
            }
            Err(err) => {
                tracing::error!("Failed to ping {} ({:?}): {}", name, address, err);
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
                    tracing::error!("Could not resolve DNS ({:?}): {}", host, err);
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
                    tracing::debug!("Ignoring ipv6 address ({:?}): {}", host, ipv6_address);
                }
            }
        }

        tracing::error!("No addresses ({:?})", host);
        Err(PingInterruption::SuspendUntilNewTrack)
    }

    async fn run_sequence(
        &mut self,
        track_url_str: ArcStr,
        ping_remote_forever: bool,
    ) -> Result<Never, PingInterruption> {
        tracing::debug!(?track_url_str, ping_remote_forever, "Running sequence",);

        let (track_url_str, track_url) = self.parse_url(track_url_str)?;

        let scheme = track_url.scheme();
        if let "file" | "cdda" = scheme {
            tracing::trace!("Ignoring {}", scheme);
            return Err(PingInterruption::SuspendUntilNewTrack);
        }

        let host = track_url.host_str().ok_or_else(|| {
            tracing::error!(?track_url_str, "No host");
            PingInterruption::SuspendUntilNewTrack
        })?;

        let remote_address = self.get_remote_address(host).await?;

        let mut maybe_remote_ping = None;

        'retry: loop {
            tracing::trace!(gateway_address=%self.gateway_address, "Checking gateway ping");
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

            tracing::trace!(gateway_address=%self.gateway_address, %remote_address, "Pinging gateway and remote");

            let mut remote_pings_remaining = self.ping_count;

            while remote_pings_remaining > 0 {
                if !ping_remote_forever {
                    remote_pings_remaining -= 1;
                }

                let remote_ping = self
                    .ping_remote(remote_address, |remote_ping| PingTimes::GatewayAndRemote {
                        gateway_ping,
                        remote_ping,
                        latest: PingTarget::Remote,
                    })
                    .await?
                    .map_err(|FailedToPing(err)| err);

                maybe_remote_ping = Some(remote_ping);

                if !matches!(&remote_ping, Ok(_) | Err(PingError::Timeout)) {
                    continue 'retry;
                }

                match self
                    .ping_gateway(|result| match result {
                        Ok(gateway_ping) => PingTimes::GatewayAndRemote {
                            gateway_ping,
                            remote_ping,
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

            tracing::trace!(%remote_address, "Finished pinging remote.");

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

    async fn run(mut self, initial_ping_address: &str) {
        let initial_url_str = rradio_messages::arcstr::format!("http://{}", initial_ping_address);
        let mut track_url_str = {
            let interruption = self.run_sequence(initial_url_str, true).await.unwrap_err();
            tracing::trace!("Ping Interruption: {:?}", interruption);
            match interruption {
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
            let interruption = self.run_sequence(track_url_str, false).await.unwrap_err();
            tracing::trace!("Ping Interruption: {:?}", interruption);
            track_url_str = match interruption {
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
    config: std::sync::Arc<crate::config::ping::Config>,
) -> Result<
    (
        impl std::future::Future<Output = ()>,
        mpsc::UnboundedSender<Option<ArcStr>>,
        mpsc::UnboundedReceiver<PingTimes>,
    ),
    ipv4::PermissionsError,
> {
    let ipv4_pinger = {
        let mut ipv4_pinger = ipv4::Pinger::new()?;

        let (ping_request_tx, mut ping_request_rx) = mpsc::channel(1);

        std::thread::spawn(move || {
            while let Some(Ivp4PingRequest {
                address,
                response_tx,
            }) = ping_request_rx.blocking_recv()
            {
                let _ = response_tx.send(ipv4_pinger.ping(address));
            }
        });

        ping_request_tx
    };

    let (track_url_tx, track_url_rx) = mpsc::unbounded_channel::<Option<ArcStr>>();

    let (ping_time_tx, ping_time_rx) = mpsc::unbounded_channel();

    let task = async move {
        Pinger {
            gateway_address: config.gateway_address,
            ping_count: config.remote_ping_count,
            ipv4_pinger,
            track_urls: track_url_rx,
            ping_times: ping_time_tx,
        }
        .run(&config.initial_ping_address)
        .await;

        tracing::debug!("Shut down");
    };

    Ok((task, track_url_tx, ping_time_rx))
}
