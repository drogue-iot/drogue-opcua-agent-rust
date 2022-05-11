use crate::{middleware::Update, mqtt};
use serde_json::{json, Value};
use std::collections::{hash_map, HashMap};

/// A data layer, handling data towards the cloud.
pub trait DataLayer {
    fn update<I>(&mut self, updates: I) -> Vec<mqtt::Event>
    where
        I: Iterator<Item = Update>;
}

/// A data layer based on the Drogue IoT channel/feature model.
pub struct FeatureDataLayer {}

impl FeatureDataLayer {
    pub fn new() -> Self {
        Self {}
    }
}

impl DataLayer for FeatureDataLayer {
    fn update<I>(&mut self, updates: I) -> Vec<mqtt::Event>
    where
        I: Iterator<Item = Update>,
    {
        let mut compacted = HashMap::<String, mqtt::Event>::new();

        for update in updates {
            let feature = update
                .extensions
                .get("feature")
                .and_then(|f| f.as_str())
                .or_else(|| update.address.last().map(|s| s.as_str()))
                .map(|s| s.to_string());

            if let Some(feature) = feature {
                match compacted.entry(update.channel.clone()) {
                    hash_map::Entry::Vacant(entry) => {
                        entry.insert(mqtt::Event {
                            channel: update.channel,
                            payload: json!({
                                "features": {
                                    feature: update.value,
                                }
                            }),
                        });
                    }
                    hash_map::Entry::Occupied(mut entry) => {
                        if let Value::Object(features) = &mut entry.get_mut().payload["features"] {
                            features.insert(feature, update.value);
                        }
                    }
                }
            }
        }

        compacted.into_values().collect()
    }
}
