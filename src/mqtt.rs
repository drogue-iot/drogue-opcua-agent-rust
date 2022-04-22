use anyhow::Context;
use futures::{select, FutureExt, Stream, StreamExt};
use rand::distributions::Alphanumeric;
use rand::Rng;
use rumqttc::{AsyncClient, ClientConfig, MqttOptions, QoS, Transport};
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum Event {
    Feature { feature: String, properties: Value },
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
}

mod defaults {
    pub const fn tls() -> bool {
        true
    }
}

pub struct MqttCloudConnector {
    config: Configuration,
}

impl MqttCloudConnector {
    pub fn new(config: Configuration) -> Self {
        Self { config }
    }

    pub async fn run(self, stream: impl Stream<Item = Event>) -> anyhow::Result<()> {
        let mut opts = MqttOptions::new(
            self.config.client_id.unwrap_or_else(random_id),
            self.config.host,
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

            let client_config = ClientConfig::builder()
                .with_safe_defaults()
                .with_root_certificates(roots)
                .with_no_client_auth();
            opts.set_transport(Transport::Tls(client_config.into()));
        }

        let (username, password) = match self.config.credentials {
            Credentials::Password { password } => (
                format!(
                    "{}@{}",
                    // FIXME: need to URL encode the components
                    self.config.identity.device,
                    self.config.identity.application
                ),
                password,
            ),
            Credentials::Username { username, password } => (username, password),
        };

        opts.set_credentials(username, password);
        if let Some(keep_alive) = self.config.keep_alive {
            opts.set_keep_alive(keep_alive);
        }

        let (client, mut event_loop) = AsyncClient::new(opts, 10);

        let mut stream = StreamExt::fuse(Box::pin(stream));

        // FIXME: subscribe to commands

        let mqtt = async {
            loop {
                match event_loop.poll().await {
                    Ok(event) => {
                        log::info!("MQTT event: {event:?}");
                    }
                    Err(err) => {
                        log::warn!("Error: {err}");
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
                            // FIXME: handle error differently
                            Self::handle_event(&client, event).await?;
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

        //Ok(())
    }

    async fn handle_event(client: &AsyncClient, event: Event) -> anyhow::Result<()> {
        match event {
            Event::Feature {
                feature,
                properties,
            } => {
                let payload = json!({
                    "features": {
                        feature: properties
                    }
                });
                client
                    .publish(
                        "state",
                        QoS::AtMostOnce,
                        false,
                        serde_json::to_vec(&payload)?,
                    )
                    .await?;
            }
        }

        Ok(())
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
            }
        );
    }
}
