mod config;

pub use config::*;

use crate::middleware::{Address, Event, Update};
use crate::ToJson;
use anyhow::anyhow;
use futures::{
    channel::mpsc::{Receiver, SendError, Sender},
    SinkExt,
};
use opcua::client::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use std::time::SystemTime;
use std::{
    ops::{Deref, DerefMut},
    str::FromStr,
    sync::{Arc, RwLock},
};
use tokio::runtime::Handle;

pub struct OpcUaConnector {
    connections: HashMap<String, OpcUaConnection>,
}

pub struct OpcUaConnection {
    id: String,
    config: Connection,
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

#[derive(Clone)]
pub struct SubscriptionEventSender {
    connection: String,
    subscription: String,
    sender: EventSender,
}

#[derive(Clone)]
pub struct ConnectionEventSender {
    connection: String,
    sender: EventSender,
    subscriptions: HashMap<String, Vec<String>>,
}

impl EventSender {
    fn update_sync<I>(&mut self, updates: I)
    where
        I: IntoIterator<Item = Update>,
    {
        let mut tx = self.clone();
        let event = Event {
            updates: updates.into_iter().collect(),
        };
        Handle::current().spawn(async move {
            if let Err(err) = tx.0.send(event).await {
                log::warn!("Failed to queue updates: {err}");
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

impl OnSubscriptionNotification for SubscriptionEventSender {
    fn on_data_change(&mut self, data_change_items: &[&MonitoredItem]) {
        let mut updates = Vec::with_capacity(data_change_items.len());
        for item in data_change_items {
            log::debug!("Change: {item:?}");
            let address = address(
                &self.connection,
                &self.subscription,
                &item.item_to_monitor().node_id,
            );
            for value in item.values() {
                updates.push(Update::new(
                    address.clone(),
                    &self.connection,
                    value.clone().to_json(),
                ));
            }
        }

        self.sender.update_sync(updates);
    }
}

impl OnConnectionStatusChange for ConnectionEventSender {
    fn on_connection_status_change(&mut self, connected: bool) {
        if connected {
            self.sender
                .update_sync([connection_state(&self.connection, now(), StatusCode::Good)]);
        }
    }
}

impl OnSessionClosed for ConnectionEventSender {
    fn on_session_closed(&mut self, status_code: StatusCode) {
        let now = now();

        let mut updates = vec![];

        // notify connection
        updates.push(connection_state(&self.connection, now.clone(), status_code));

        // notify items
        for (id, sub) in &self.subscriptions {
            for node in sub {
                let address = address(&self.connection, &id, &node);
                updates.push(Update::new(
                    address,
                    &self.connection,
                    json!({
                        "timestamp": now,
                        "subscribed": false,
                    }),
                ))
            }
        }
        self.sender.update_sync(updates);
    }
}

impl OpcUaConnector {
    pub fn new(config: Configuration) -> Self {
        let mut connections = HashMap::new();

        for (id, connection) in config.connections {
            connections.insert(id.clone(), OpcUaConnection::new(id, connection));
        }

        Self { connections }
    }

    pub async fn start(self) -> anyhow::Result<EventStream> {
        let (tx, rx) = futures::channel::mpsc::channel::<Event>(1_000);

        let tx = EventSender(tx);

        for (_, connection) in self.connections {
            connection.start(tx.clone()).await;
        }

        Ok(EventStream(rx))
    }
}

impl OpcUaConnection {
    pub fn new(id: String, config: Connection) -> Self {
        Self { id, config }
    }

    pub fn subscribe(
        &mut self,
        session: Arc<RwLock<Session>>,
        mut tx: EventSender,
    ) -> anyhow::Result<()> {
        log::debug!("Creating subscriptions");

        let session = session.read().unwrap();

        for (id, sub) in &self.config.subscriptions {
            self.subscribe_one(&session, &id, &sub, &mut tx)?;
        }

        Ok(())
    }

    fn subscribe_one(
        &self,
        session: &Session,
        id: &str,
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
            SubscriptionEventSender {
                connection: self.id.to_string(),
                subscription: id.to_string(),
                sender: tx.clone(),
            },
        )?;

        log::debug!("Created a subscription with id = {}", subscription_id);

        // Create some monitored items
        let items_to_create: Vec<_> = subscription
            .nodes
            .iter()
            .map(|node| {
                NodeId::from_str(match &node {
                    Node::Simple(id) => id,
                    Node::Complex(def) => &def.id,
                })
                .map(|node| node.into())
            })
            .collect::<Result<_, _>>()?;

        let result = session.create_monitored_items(
            subscription_id,
            subscription.timestamps.into(),
            &items_to_create,
        )?;

        // the result has the same order as the request list

        let mut updates = Vec::with_capacity(result.len());
        for (req, res) in items_to_create.into_iter().zip(result.into_iter()) {
            // if the subscription was not good ...
            if !res.status_code.is_good() {
                // ... we send that out.
                updates.push(Update::new(
                    address(&self.id, id, &req.item_to_monitor.node_id),
                    &self.id,
                    json!({
                        "subscribed": false,
                        "timestamp": now(),
                        "status": res.status_code.name(),
                    }),
                ));
            }
            // ... otherwise, the subscription will provide a value
        }

        // send subscription events

        tx.update_sync(updates);

        // done

        Ok(())
    }

    pub async fn start(self, tx: EventSender) {
        Handle::current().spawn_blocking(move || self.do_run(tx));
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

        if let Ok(mut session) = session.write() {
            let sender = ConnectionEventSender {
                connection: self.id.clone(),
                sender: tx.clone(),
                subscriptions: self
                    .config
                    .subscriptions
                    .iter()
                    .map(|(id, subs)| {
                        (
                            id.to_string(),
                            subs.nodes
                                .iter()
                                .map(|n| {
                                    NodeId::from_str(n.node_id())
                                        .map(|n| n.to_string())
                                        .unwrap_or_else(|_| n.node_id().to_string())
                                })
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect(),
            };
            session.set_connection_status_callback(sender.clone());
            session.set_session_closed_callback(sender);
        }

        tx.update_sync([connection_state(&self.id, now(), StatusCode::Good)]);

        self.subscribe(session.clone(), tx.clone())?;

        Session::run(session);

        log::warn!("Session loop exited. Terminating...");

        tx.0.close_channel();

        Ok(())
    }
}

fn address<N>(connection: &str, subscription: &str, node_id: &N) -> Address
where
    N: ToString,
{
    vec![
        "opcua".to_string(),
        connection.to_string(),
        "subscriptions".to_string(),
        subscription.to_string(),
        node_id.to_string(),
    ]
    .into()
}

fn now() -> String {
    humantime::format_rfc3339_millis(SystemTime::now()).to_string()
}

fn connection_state(connection: &str, timestamp: String, code: StatusCode) -> Update {
    Update::new(
        ["opcua", connection, "connection"],
        connection,
        if code.is_good() {
            json!({
                "connected": true,
                "timestamp": timestamp,
            })
        } else {
            json!({
                "connected": false,
                "status": code.to_string(),
                "timestamp": timestamp,
            })
        },
    )
}
