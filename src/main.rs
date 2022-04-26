pub mod middleware;
pub mod mqtt;
pub mod opcua;
pub mod types;

use crate::middleware::Middleware;
use crate::mqtt::MqttCloudConnector;
use crate::opcua::OpcUaConnector;
use crate::types::ToJson;
use serde::Deserialize;
use std::fs::File;

#[derive(Clone, Debug, Deserialize)]
pub struct Configuration {
    pub opcua: opcua::Configuration,
    pub middleware: middleware::Configuration,
    pub cloud: mqtt::Configuration,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let config = std::env::var_os("CONFIG_FILE")
        .and_then(|s| s.into_string().ok())
        .unwrap_or_else(|| "/etc/opcua-agent/config.yaml".to_string());

    let config: Configuration = serde_yaml::from_reader(File::open(&config)?)?;

    log::info!("Configuration: {config:#?}");

    let connector = OpcUaConnector::new(config.opcua);
    let middleware = Middleware::new(config.middleware);
    let cloud = MqttCloudConnector::new(config.cloud);

    let opcua_stream = connector.start().await?;
    let cloud_stream = cloud.start().await;

    log::info!("Running main");

    middleware
        .run(opcua_stream.into_inner(), cloud_stream)
        .await?;

    log::info!("Exiting main ...");

    Ok(())
}
