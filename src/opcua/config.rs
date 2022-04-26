use opcua::types::TimestampsToReturn;
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
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub timestamps: Timestamps,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Node {
    Simple(String),
    Complex(NodeDefinition),
}

impl Node {
    pub fn node_id(&self) -> &str {
        match self {
            Self::Simple(node_id) => &node_id,
            Self::Complex(NodeDefinition { id, .. }) => &id,
        }
    }
}

impl From<Node> for NodeDefinition {
    fn from(value: Node) -> Self {
        match value {
            Node::Simple(id) => NodeDefinition {
                id,
                ..Default::default()
            },
            Node::Complex(def) => def,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct NodeDefinition {
    pub id: String,
    #[serde(default)]
    pub alias: Option<String>,
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
