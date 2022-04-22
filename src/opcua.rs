use anyhow::anyhow;
use futures::{
    channel::mpsc::{Receiver, SendError, Sender},
    SinkExt,
};
use opcua::client::prelude::*;
use serde::Deserialize;
use std::{
    ops::{Deref, DerefMut},
    str::FromStr,
    sync::{Arc, RwLock},
    time::Duration,
};
use tokio::runtime::Handle;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Configuration {
    pub url: String,
    #[serde(default)]
    pub credentials: Credentials,

    #[serde(default)]
    pub subscriptions: Vec<Subscription>,
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

pub struct OpcUaConnector {
    config: Configuration,
}

#[derive(Clone, Debug)]
pub enum State {
    Connected,
    Disconnected(StatusCode),
}

#[derive(Clone, Debug)]
pub enum Event {
    ConnectionState(State),
    ItemState(Box<ItemState>),
    Data(Box<DataChange>),
}

#[derive(Clone, Debug)]
pub struct ItemState {
    pub node_id: ReadValueId,
    pub state: State,
}

#[derive(Clone, Debug)]
pub struct DataChange {
    pub node_id: ReadValueId,
    pub value: DataValue,
}

pub struct EventStream(Receiver<Event>);

impl EventStream {
    pub fn into_inner(self) -> Receiver<Event> {
        self.0
    }
}

impl Deref for EventStream {
    type Target = Receiver<Event>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for EventStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Clone)]
pub struct EventSender(Sender<Event>);

impl EventSender {
    fn send_sync(&mut self, events: Vec<Event>) {
        let mut tx = self.clone();
        Handle::current().spawn(async move {
            if let Err(err) = tx.send_all(events).await {
                log::warn!("Failed to queue events: {err}");
            }
        });
    }

    async fn send_all(&mut self, events: Vec<Event>) -> Result<(), SendError> {
        for event in events {
            self.0.feed(event).await?;
        }
        self.0.flush().await
    }
}

impl OnSubscriptionNotification for EventSender {
    fn on_data_change(&mut self, data_change_items: &[&MonitoredItem]) {
        let mut events = Vec::with_capacity(data_change_items.len());
        for item in data_change_items {
            log::info!("Change: {item:?}");
            for value in item.values() {
                events.push(Event::Data(Box::new(DataChange {
                    node_id: item.item_to_monitor().clone(),
                    value: value.clone(),
                })));
            }
        }

        self.send_sync(events);
    }
}

impl OpcUaConnector {
    pub fn new(config: Configuration) -> Self {
        Self { config }
    }

    pub fn subscribe(
        &mut self,
        session: Arc<RwLock<Session>>,
        mut tx: EventSender,
    ) -> anyhow::Result<()> {
        log::debug!("Creating subscriptions");

        let session = session.read().unwrap();

        for sub in &self.config.subscriptions {
            self.subscribe_one(&session, &sub, &mut tx)?;
        }

        Ok(())
    }

    fn subscribe_one(
        &self,
        session: &Session,
        subscription: &Subscription,
        tx: &mut EventSender,
    ) -> anyhow::Result<()> {
        let subscription_id = session.create_subscription(
            subscription.publish_interval.as_millis() as f64,
            10,
            30,
            0,
            0,
            true,
            tx.clone(),
        )?;

        log::debug!("Created a subscription with id = {}", subscription_id);

        // Create some monitored items
        let items_to_create: Vec<_> = subscription
            .nodes
            .iter()
            .map(|node| NodeId::from_str(node).map(|node| node.into()))
            .collect::<Result<_, _>>()?;

        let result = session.create_monitored_items(
            subscription_id,
            subscription.timestamps.into(),
            &items_to_create,
        )?;

        // the result has the same order as the request list

        let mut events = Vec::with_capacity(result.len());
        for (req, res) in items_to_create.into_iter().zip(result.into_iter()) {
            let state = if res.status_code.is_good() {
                State::Connected
            } else {
                State::Disconnected(res.status_code)
            };

            events.push(Event::ItemState(Box::new(ItemState {
                node_id: req.item_to_monitor,
                state,
            })))
        }

        // send subscription events

        tx.send_sync(events);

        // done

        Ok(())
    }

    pub async fn run(self) -> anyhow::Result<EventStream> {
        let (tx, rx) = futures::channel::mpsc::channel::<Event>(10_000);

        Handle::current().spawn_blocking(move || self.do_run(EventSender(tx)));

        Ok(EventStream(rx))
    }

    fn do_run(mut self, mut tx: EventSender) -> anyhow::Result<()> {
        let client = ClientBuilder::new()
            .application_name("Drogue IoT OPC UA Agent")
            .application_uri("https://drogue.io")
            .product_uri("https://drogue.io")
            // FIXME: this is insecure
            .trust_server_certs(true)
            // FIXME: this is insecure
            .create_sample_keypair(true)
            .session_timeout(15_000)
            .session_retry_limit(3);

        let id = match &self.config.credentials {
            Credentials::Anonymous => IdentityToken::Anonymous,
            Credentials::User { username, password } => {
                IdentityToken::UserName(username.to_string(), password.to_string())
            }
        };

        let mut client = client
            .client()
            .ok_or_else(|| anyhow!("Invalid configuration"))?;

        let session = client.connect_to_endpoint(
            (
                self.config.url.as_str(),
                // FIXME: this is insecure
                SecurityPolicy::None.to_str(),
                // FIXME: this is insecure
                MessageSecurityMode::None,
                // FIXME: this is insecure
                UserTokenPolicy::anonymous(),
            ),
            id,
        )?;

        tx.send_sync(vec![Event::ConnectionState(State::Connected)]);

        self.subscribe(session.clone(), tx.clone())?;

        // FIXME: notify connection state changes

        Session::run(session);

        log::warn!("Session loop exited. Terminating...");

        tx.0.close_channel();

        Ok(())
    }
}
