use super::*;
use crate::{middleware::Update, mqtt};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// A data layer based on the Drogue IoT channel/feature model, sending full updates.
pub struct FullFeatureDataLayer {
    channels: HashMap<String, Channel>,
}

#[derive(Clone, Debug, Serialize)]
struct Channel {
    features: HashMap<String, Value>,
}

impl FullFeatureDataLayer {
    pub fn new() -> Self {
        Self {
            channels: Default::default(),
        }
    }
}

impl DataLayer for FullFeatureDataLayer {
    fn update<I>(&mut self, updates: I) -> Result<Vec<mqtt::Event>, DataError>
    where
        I: Iterator<Item = Update>,
    {
        let mut channels = HashSet::new();

        for update in updates {
            let feature = update
                .extensions
                .get("feature")
                .and_then(|f| f.as_str())
                .or_else(|| update.address.last().map(|s| s.as_str()))
                .map(|s| s.to_string());

            if let Some(feature) = feature {
                let channel = update.channel.clone();
                channels.insert(channel.clone());

                match self.channels.entry(channel) {
                    hash_map::Entry::Vacant(entry) => {
                        let mut features = HashMap::new();
                        features.insert(feature, update.value);
                        entry.insert(Channel { features });
                    }
                    hash_map::Entry::Occupied(mut entry) => {
                        entry.get_mut().features.insert(feature, update.value);
                    }
                }
            }
        }

        let result = channels
            .into_iter()
            .filter_map(|c| self.channels.get(&c).zip(Some(c)))
            .map(|(payload, channel)| {
                Ok(mqtt::Event {
                    channel,
                    payload: serde_json::to_value(payload).map_err(DataError::Encoding)?,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(result)
    }
}
