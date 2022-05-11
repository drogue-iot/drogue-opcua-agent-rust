use crate::{
    data::{DataLayer, FeatureDataLayer},
    mqtt,
};
use futures::{select, Sink, SinkExt, Stream, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use serde_with::{serde_as, DeserializeFromStr};
use std::{
    borrow::Borrow,
    collections::HashMap,
    convert::Infallible,
    fmt::{Debug, Display, Formatter},
    ops::{Deref, DerefMut},
    str::FromStr,
};

#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, DeserializeFromStr)]
pub struct Address(Vec<String>);

impl From<Vec<String>> for Address {
    fn from(address: Vec<String>) -> Self {
        Address(address)
    }
}

impl Deref for Address {
    type Target = Vec<String>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Address {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Borrow<Vec<String>> for Address {
    fn borrow(&self) -> &Vec<String> {
        &self.0
    }
}

impl Borrow<[String]> for Address {
    fn borrow(&self) -> &[String] {
        &self.0
    }
}

impl IntoIterator for Address {
    type Item = String;
    type IntoIter = <Vec<String> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl FromStr for Address {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            return Ok(Address(vec![]));
        }

        let mut paths = vec![];
        let mut current = String::new();

        let mut s = s.chars();
        while let Some(c) = s.next() {
            match c {
                '/' => {
                    paths.push(current);
                    current = String::new();
                }
                '\\' => {
                    if let Some(c) = s.next() {
                        current.push(c);
                    }
                }
                c => {
                    current.push(c);
                }
            }
        }

        paths.push(current);

        Ok(Address(paths))
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[serde_as]
#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub struct Configuration {
    #[serde(default)]
    pub sources: HashMap<Address, Source>,
    #[serde(default)]
    pub sinks: HashMap<Address, Source>,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize)]
pub struct Source {
    #[serde(default)]
    pub drop: Option<bool>,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub extensions: HashMap<String, Value>,
}

pub struct Middleware {
    config: Configuration,
    data: FeatureDataLayer,
}

#[derive(Clone, Debug)]
pub struct Update {
    pub address: Address,
    pub channel: String,
    pub value: Value,
    pub extensions: HashMap<String, Value>,
}

#[derive(Clone, Debug)]
pub struct Event {
    pub updates: Vec<Update>,
}

impl Update {
    pub fn new<I, C, S>(address: I, channel: C, value: Value) -> Self
    where
        C: Into<String>,
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            address: Address(address.into_iter().map(|s| s.into()).collect()),
            channel: channel.into(),
            value,
            extensions: Default::default(),
        }
    }
}

impl Middleware {
    pub fn new(config: Configuration) -> Self {
        Self {
            config,
            data: FeatureDataLayer::new(),
        }
    }

    pub async fn run(
        mut self,
        events: impl Stream<Item = Event>,
        cloud: impl Sink<mqtt::Event, Error = impl std::error::Error + Send + Sync + 'static>,
        command_stream: impl Stream<Item = Event>,
        commands: impl Sink<Event, Error = impl std::error::Error + Send + Sync + 'static>,
    ) -> anyhow::Result<()> {
        let mut events = Box::pin(events).fuse();
        let mut cloud = Box::pin(cloud);
        let mut command_stream = Box::pin(command_stream).fuse();
        let mut commands = Box::pin(commands);

        loop {
            select! {
                event = events.next() => {
                    match event {
                        None => {
                            break;
                        }
                        Some(event) => {
                            self.handle_event(event, &mut cloud).await?;
                        }
                    }
                }
                command = command_stream.next() => {
                    match command {
                        None => {
                            break;
                        }
                        Some(command) => {
                            self.handle_command(command, &mut commands).await?;
                        }
                    }
                }
            }
        }

        log::info!("Exiting middleware loop");

        Ok(())
    }

    async fn handle_command<S, E>(&self, command: Event, agent: &mut S) -> Result<(), E>
    where
        S: Sink<Event, Error = E> + Unpin,
        E: std::error::Error,
    {
        for event in self.process_command(command) {
            log::info!("Handle command: {event:?}");
            agent.send(event).await?;
        }
        Ok(())
    }

    async fn handle_event<S, E>(&mut self, event: Event, cloud: &mut S) -> Result<(), E>
    where
        S: Sink<mqtt::Event, Error = E> + Unpin,
        E: std::error::Error,
    {
        for event in self.process_event(event) {
            cloud.send(event).await?;
        }

        Ok(())
    }

    fn process_command(&self, command: Event) -> Vec<Event> {
        log::info!("Process command: {command:?}");

        let updates = command
            .updates
            .into_iter()
            .filter_map(|u| Self::process_update(&self.config.sinks, u))
            .collect();

        vec![Event { updates }]
    }

    fn process_event(&mut self, event: Event) -> Vec<mqtt::Event> {
        self.data.update(
            event
                .updates
                .into_iter()
                .filter_map(|u| Self::process_update(&self.config.sources, u)),
        )
    }

    fn process_update(config: &HashMap<Address, Source>, mut update: Update) -> Option<Update> {
        // collect relevant sources, from least specific, to most specific
        let mut sources = vec![];
        for i in 0..=update.address.0.len() {
            let path = &update.address.as_slice()[..i];
            log::debug!("Checking path: {path:?}");
            if let Some(config) = config.get(path) {
                sources.push(config);
            }
        }

        log::debug!("Update: {update:?}");
        log::debug!("Matching sources: {sources:?}");

        // check if we need to drop
        if let Some(true) = find(&sources, |source| source.drop.as_ref()) {
            return None;
        }

        // apply channel
        if let Some(channel) = find(&sources, |source| source.channel.as_ref()) {
            update.channel = channel.to_string();
        }

        // apply extensions
        for s in sources {
            update.extensions.extend(s.extensions.clone());
        }

        // return result
        Some(update)
    }
}

/// Find the most specific override.
fn find<'t, T, F>(sources: &'t [&Source], f: F) -> Option<&'t T>
where
    F: Fn(&Source) -> Option<&T>,
{
    let mut r = None;

    for s in sources {
        if let Some(t) = f(s) {
            r = Some(t);
        }
    }

    r
}
