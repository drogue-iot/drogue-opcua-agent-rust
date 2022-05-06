use crate::middleware::{self, Update};
use anyhow::Context;
use futures::{channel::mpsc::channel, select, FutureExt, Sink, SinkExt, Stream, StreamExt};
use percent_encoding::{percent_encode, NON_ALPHANUMERIC};
use rand::{distributions::Alphanumeric, Rng};
use rumqttc::{
    AsyncClient, ClientConfig, ConnAck, ConnectReturnCode, MqttOptions, Packet, Publish, QoS,
    Transport,
};
use rustls::client::NoClientSessionStorage;
use serde::Deserialize;
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::spawn;

#[cfg(feature = "megolm")]
use vodozemac::megolm::{GroupSession, GroupSessionPickle};

#[derive(Clone, Debug)]
pub struct Event {
    pub channel: String,
    pub payload: Value,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Identity {
    pub application: String,
    pub device: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Credentials {
    Username { username: String, password: String },
    Password { password: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Configuration {
    pub client_id: Option<String>,
    pub host: String,
    pub port: u16,

    #[serde(default = "defaults::tls")]
    pub tls: bool,

    #[serde(flatten)]
    pub identity: Identity,
    #[serde(flatten)]
    pub credentials: Credentials,

    #[serde(default)]
    #[serde(with = "humantime_serde")]
    pub keep_alive: Option<Duration>,

    #[cfg(feature = "megolm")]
    #[serde(default)]
    pub group_session_pickle: Option<String>,
}

mod defaults {
    pub const fn tls() -> bool {
        true
    }
}

pub struct MqttCloudConnector {
    #[cfg(feature = "megolm")]
    group_session: Option<GroupSession>,
    config: Configuration,
}

impl MqttCloudConnector {
    #[cfg(not(feature = "megolm"))]
    pub fn new(config: Configuration) -> anyhow::Result<Self> {
        Ok(Self { config })
    }

    #[cfg(feature = "megolm")]
    pub fn new(config: Configuration) -> anyhow::Result<Self> {
        let group_session = match &config.group_session_pickle {
            None => None,
            Some(pickle) => {
                log::info!("Enabling Megolm Group Session");
                Some(serde_json::from_str::<GroupSessionPickle>(&pickle)?.into())
            }
        };

        Ok(Self {
            config,
            group_session,
        })
    }

    pub async fn start(
        self,
    ) -> (
        impl Sink<Event, Error = impl std::error::Error>,
        impl Stream<Item = middleware::Event>,
    ) {
        let (tx, rx) = futures::channel::mpsc::unbounded();
        let (cmd_tx, cmd_rx) = channel::<middleware::Event>(1_000);

        spawn(async {
            self.run(rx, cmd_tx).await.ok();
        });

        (tx, cmd_rx)
    }

    async fn run(
        mut self,
        stream: impl Stream<Item = Event>,
        sink: impl Sink<middleware::Event, Error = impl std::error::Error>,
    ) -> anyhow::Result<()> {
        let mut opts = MqttOptions::new(
            self.config
                .client_id
                .as_ref()
                .cloned()
                .unwrap_or_else(random_id),
            &self.config.host,
            self.config.port,
        );

        opts.set_clean_session(true);

        if self.config.tls {
            let mut roots = rustls::RootCertStore::empty();
            for cert in
                rustls_native_certs::load_native_certs().context("could not load platform certs")?
            {
                roots.add(&rustls::Certificate(cert.0))?;
            }

            let mut client_config = ClientConfig::builder()
                .with_safe_defaults()
                .with_root_certificates(roots)
                .with_no_client_auth();
            client_config.session_storage = Arc::new(NoClientSessionStorage {});
            opts.set_transport(Transport::Tls(client_config.into()));
        }

        let (username, password) = match &self.config.credentials {
            Credentials::Password { password } => (
                format!(
                    "{}@{}",
                    percent_encode(self.config.identity.device.as_bytes(), NON_ALPHANUMERIC),
                    &self.config.identity.application,
                ),
                password.clone(),
            ),
            Credentials::Username { username, password } => (username.clone(), password.clone()),
        };

        opts.set_credentials(username, password);
        if let Some(keep_alive) = self.config.keep_alive {
            opts.set_keep_alive(keep_alive);
        }

        let (client, mut event_loop) = AsyncClient::new(opts, 10);

        let mut stream = StreamExt::fuse(Box::pin(stream));
        let mut sink = Box::pin(sink);

        let mqtt = async {
            loop {
                match event_loop.poll().await {
                    Ok(rumqttc::Event::Incoming(Packet::ConnAck(ConnAck {
                        session_present: false,
                        code: ConnectReturnCode::Success,
                    }))) => {
                        log::debug!("Connected (without session)");
                        if let Err(err) =
                            client.subscribe("command/inbox//#", QoS::AtMostOnce).await
                        {
                            log::warn!("Failed to subscribe to commands: {err}");
                        }
                    }
                    Ok(rumqttc::Event::Incoming(Packet::Publish(publish))) => {
                        log::debug!("Received command: {publish:?}");
                        Self::handle_command(&mut sink, publish).await;
                    }
                    Ok(event) => {
                        log::debug!("MQTT event: {event:?}");
                    }
                    Err(err) => {
                        log::warn!("Connection error: {err}");
                    }
                }
            }
        };

        let mut mqtt = Box::pin(mqtt.fuse());

        loop {
            select! {
                event = stream.next() => {
                    log::info!("Data event: {event:?}");
                    match event {
                        Some(event) => {
                            self.handle_event(&client, event).await?;
                        }
                        None => {
                            // Stream ended
                            break Ok(());
                        }
                    }

                },
                _ = mqtt => {
                    log::info!("MQTT loop returned");
                    break Ok(());
                },

            }
        }
    }

    async fn handle_event(&mut self, client: &AsyncClient, event: Event) -> anyhow::Result<()> {
        let payload = serde_json::to_string(&event.payload)?;

        #[cfg(feature = "megolm")]
        let payload = self.encrypt(payload);

        client
            .publish(event.channel, QoS::AtMostOnce, false, payload)
            .await?;

        Ok(())
    }

    async fn handle_command<S, E>(sink: &mut S, publish: Publish)
    where
        S: Sink<middleware::Event, Error = E> + Unpin,
        E: std::error::Error,
    {
        log::info!("Handle command: {}", publish.topic);

        #[derive(Clone, Debug, Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct WriteCommand {
            pub connection: String,
            pub value: Value,
            pub node_id: String,
        }

        match publish.topic.as_str() {
            "command/inbox//write" => {
                let command: WriteCommand = match serde_json::from_slice(publish.payload.as_ref()) {
                    Ok(payload) => payload,
                    Err(err) => {
                        log::info!("Invalid command payload: {err}");
                        return;
                    }
                };

                let mut update = Update::new(
                    ["cloud", "commands", &command.connection],
                    command.connection.clone(),
                    command.value,
                );
                update
                    .extensions
                    .insert("nodeId".to_string(), command.node_id.into());

                log::info!("Scheduling write command: {update:?}");

                let updates = vec![update];

                if let Err(err) = sink.send(middleware::Event { updates }).await {
                    log::warn!("Failed to queue command: {err}");
                }
            }
            _ => {
                log::info!("Invalid command: {}", publish.topic);
            }
        }
    }

    fn encrypt(&mut self, payload: String) -> Vec<u8> {
        if let Some(group_session) = &mut self.group_session {
            group_session.encrypt(&payload).to_bytes()
        } else {
            payload.into_bytes()
        }
    }
}

fn random_id() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(20)
        .map(char::from)
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_des() {
        let config: Configuration = serde_json::from_value(json!({
            "host": "localhost",
            "port": 1234i32,
            "application": "app1",
            "device": "device1",
            "password": "password1",
        }))
        .unwrap();

        assert_eq!(
            config,
            Configuration {
                client_id: None,
                host: "localhost".to_string(),
                port: 1234,
                tls: true,
                identity: Identity {
                    application: "app1".to_string(),
                    device: "device1".to_string()
                },
                credentials: Credentials::Password {
                    password: "password1".to_string()
                },
                keep_alive: None,
                #[cfg(feature = "megolm")]
                group_session_pickle: None,
            }
        );
    }
}
