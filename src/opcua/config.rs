use crate::opcua::opcua::types::TimestampsToReturn;
use serde::Deserialize;
use std::collections::HashMap;
use std::default::Default;
use std::time::Duration;

#[derive(Clone, Debug, Deserialize)]
pub struct Configuration {
    #[serde(default)]
    pub connections: HashMap<String, Connection>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub url: String,

    pub security_policy: String,
    pub security_mode: String,

    #[serde(default)]
    pub auto_accept_server_certificate: bool,

    #[serde(default)]
    pub create_sample_keypair: bool,

    #[serde(default)]
    #[serde(with = "humantime_serde")]
    pub session_timeout: Option<Duration>,

    #[serde(default)]
    pub session_retry_limit: Option<i32>,

    #[serde(default)]
    pub credentials: Credentials,

    #[serde(default)]
    pub subscriptions: HashMap<String, Subscription>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    #[serde(default = "defaults::publish_interval", with = "humantime_serde")]
    pub publish_interval: Duration,
    pub nodes: Vec<String>,
    #[serde(default)]
    pub timestamps: Timestamps,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub enum Timestamps {
    None,
    Source,
    Server,
    Both,
}

impl Default for Timestamps {
    fn default() -> Self {
        Timestamps::Source
    }
}

impl From<Timestamps> for TimestampsToReturn {
    fn from(value: Timestamps) -> Self {
        match value {
            Timestamps::None => TimestampsToReturn::Neither,
            Timestamps::Source => TimestampsToReturn::Source,
            Timestamps::Server => TimestampsToReturn::Server,
            Timestamps::Both => TimestampsToReturn::Both,
        }
    }
}

mod defaults {
    use std::time::Duration;

    pub const fn publish_interval() -> Duration {
        Duration::from_secs(1)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
#[serde(rename_all = "camelCase")]
pub enum Credentials {
    User { username: String, password: String },
    Anonymous,
}

impl Default for Credentials {
    fn default() -> Self {
        Self::Anonymous
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_cfg() {
        let _config: Configuration = serde_json::from_value(json!({
            "url": "opc.tcp://localhost:1234",
            "subscriptions": [
                {
                    "nodes": [
                        "ns=1,s=Foo",
                        {
                            "id": "ns=1,s=Bar",
                            "alias": "bar",
                        }
                    ]
                }
            ]
        }))
        .unwrap();
    }
}
