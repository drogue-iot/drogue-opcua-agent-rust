pub mod mqtt;
pub mod opcua;
pub mod types;

use crate::mqtt::MqttCloudConnector;
use crate::opcua::{OpcUaConnector, State};
use crate::types::ToJson;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;
use std::fs::File;
use std::time::SystemTime;

#[derive(Clone, Debug, Deserialize)]
pub struct Configuration {
    pub opcua: opcua::Configuration,
    pub cloud: mqtt::Configuration,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let config = std::env::var_os("CONFIG_FILE")
        .and_then(|s| s.into_string().ok())
        .unwrap_or_else(|| "/etc/opcua-agent/config.yaml".to_string());

    let config: Configuration = serde_yaml::from_reader(File::open(&config)?)?;

    let connector = OpcUaConnector::new(config.opcua);
    let cloud = MqttCloudConnector::new(config.cloud);

    let stream = connector.run().await?;

    log::info!("Running main");

    let stream = stream.into_inner().filter_map(|e| async move {
        match e {
            // FIXME: when the connection goes down, mark all values as invalid
            opcua::Event::ConnectionState(state) => Some(mqtt::Event::Feature {
                feature: "connection".to_string(),
                properties: {
                    let now = SystemTime::now();
                    match state {
                        State::Connected => {
                            json!({
                                "timestamp": now,
                                "connected": true,
                            })
                        }
                        State::Disconnected(code) => {
                            json!({
                                "timestamp": now,
                                "connected": false,
                                "cause": code.to_json(),
                            })
                        }
                    }
                },
            }),
            opcua::Event::ItemState(state) => match state.state {
                State::Connected => None,
                State::Disconnected(code) => Some(mqtt::Event::Feature {
                    feature: state.node_id.node_id.to_string(),
                    properties: {
                        json!({
                            "timestamp": SystemTime::now(),
                            "connected": false,
                            "status": code.to_json(),
                        })
                    },
                }),
            },
            opcua::Event::Data(change) => Some(mqtt::Event::Feature {
                feature: change.node_id.node_id.to_string(),
                properties: {
                    let mut p = change.value.to_json();
                    if let Some(v) = p.as_object_mut() {
                        v.insert("connected".to_string(), true.into());
                    }
                    p
                },
            }),
        }
    });

    cloud.run(stream).await?;

    log::info!("Exiting main ...");

    Ok(())
}
