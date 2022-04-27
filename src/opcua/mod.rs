mod config;

pub use config::*;

use crate::{
    middleware::{Address, Event, Update},
    ToJson,
};
use anyhow::{anyhow, bail};
use futures::{
    channel::mpsc::{channel, Receiver, Sender},
    Sink, SinkExt, Stream, StreamExt,
};
use opcua::client::prelude::*;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Duration;
use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
    str::FromStr,
    sync::{Arc, RwLock},
    time::SystemTime,
};
use tokio::sync::oneshot;
use tokio::task::spawn_blocking;
use tokio::{runtime::Handle, spawn};

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

    pub async fn start(
        self,
    ) -> (
        impl Stream<Item = Event>,
        impl Sink<Event, Error = impl std::error::Error>,
    ) {
        let (tx, rx) = channel::<Event>(1_000);
        let (cmd_tx, mut cmd_rx) = channel::<Event>(1_000);

        let tx = EventSender(tx);

        let mut commands = HashMap::with_capacity(self.connections.len());
        for (id, connection) in self.connections {
            let cmd = connection.start(tx.clone());
            commands.insert(id, Box::pin(cmd));
        }

        spawn(async move {
            loop {
                match cmd_rx.next().await {
                    None => {
                        break;
                    }
                    Some(event) => {
                        log::info!("Dispatch command: {event:?}");
                        Self::handle_command(&mut commands, event).await;
                    }
                }
            }
        });

        (rx, cmd_tx)
    }

    async fn handle_command(
        connections: &mut HashMap<String, impl Sink<Update> + Unpin>,
        event: Event,
    ) {
        for update in event.updates {
            if let Some(commands) = connections.get_mut(&update.channel) {
                if let Err(_) = commands.send(update).await {
                    log::warn!("Failed to queue command");
                }
            }
        }
    }
}

impl OpcUaConnection {
    pub fn new(id: String, config: Connection) -> Self {
        Self { id, config }
    }

    pub async fn command(&self, update: Update) {
        log::info!("Handling command update: {update:?}");
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
            .map(|node| NodeId::from_str(&node).map(|node| node.into()))
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

    pub fn start(self, tx: EventSender) -> impl Sink<Update> {
        let (cmd_tx, cmd_rx) = channel::<Update>(1_000);

        Handle::current().spawn_blocking(move || {
            if let Err(err) = self.do_run(tx, cmd_rx) {
                log::error!("Failed to run OPC connection: {err}");
            }
        });

        cmd_tx
    }

    fn do_run(
        mut self,
        mut tx: EventSender,
        commands: impl Stream<Item = Update> + Send + 'static,
    ) -> anyhow::Result<()> {
        let pki_dir = std::env::var_os("PKI_DIR")
            .map(|p| PathBuf::from(p))
            .unwrap_or_else(|| std::env::temp_dir().join("drogue-opcua-agent").join("pki"));

        let client = ClientBuilder::new()
            .application_name("Drogue IoT OPC UA Agent")
            .application_uri("https://drogue.io")
            .product_uri("https://drogue.io")
            .trust_server_certs(self.config.auto_accept_server_certificate)
            .create_sample_keypair(self.config.create_sample_keypair)
            .session_timeout(
                self.config
                    .session_timeout
                    .unwrap_or_else(|| Duration::from_secs(15))
                    .as_millis() as _,
            )
            .session_retry_limit(self.config.session_retry_limit.unwrap_or(3))
            .pki_dir(pki_dir);

        let id = match &self.config.credentials {
            Credentials::Anonymous => IdentityToken::Anonymous,
            Credentials::User { username, password } => {
                IdentityToken::UserName(username.to_string(), password.to_string())
            }
        };

        let mut client = client
            .client()
            .ok_or_else(|| anyhow!("Invalid configuration"))?;

        let security_mode = match self.config.security_mode.as_str() {
            "None" | "none" => MessageSecurityMode::None,
            "Sign" | "sign" => MessageSecurityMode::Sign,
            "SignAndEncrypt" | "signAndEncrypt" => MessageSecurityMode::SignAndEncrypt,
            _ => bail!("Invalid security mode. Must be: none, sign, or signAndEncrypt"),
        };

        let session = client.connect_to_endpoint(
            (
                self.config.url.as_str(),
                self.config.security_policy.as_str(),
                security_mode,
                None,
            ),
            id,
        )?;

        if let Ok(mut session) = session.write() {
            let sender = ConnectionEventSender {
                connection: self.id.clone(),
                sender: tx.clone(),
                // we parse and re-encode the node id to have a normalized form
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
                                    NodeId::from_str(n)
                                        .map(|id| id.to_string())
                                        .unwrap_or_else(|_| n.to_string())
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

        let (session_tx, rx) = oneshot::channel();

        let cmd_session = session.clone();
        spawn(async move {
            Self::command_loop(cmd_session, commands).await;
            log::warn!("Command loop exited");
            session_tx.send(SessionCommand::Stop).ok();
        });

        // the next call will block, until the session loop terminates
        Session::run_loop(session, 10, rx);

        log::warn!("Session loop exited. Terminating...");

        tx.0.close_channel();

        // done

        Ok(())
    }

    async fn command_loop(session: Arc<RwLock<Session>>, commands: impl Stream<Item = Update>) {
        let mut commands = Box::pin(commands);
        loop {
            match commands.next().await {
                None => {
                    log::info!("Command queue closed");
                    break;
                }
                Some(update) => {
                    let session = session.clone();
                    spawn_blocking(move || match session.read() {
                        Ok(session) => {
                            Self::write_command(&session, update);
                        }
                        Err(_) => {
                            log::warn!("Failed to lock session");
                        }
                    });
                }
            }
        }
    }

    fn write_command(session: &Session, update: Update) {
        log::debug!("Writing command: {update:?}");

        let node_id = update
            .extensions
            .get("nodeId")
            .and_then(|id| id.as_str())
            .or_else(|| update.address.last().map(|s| s.as_str()));

        let node_id = if let Some(node_id) = node_id {
            node_id
        } else {
            return;
        };

        let node_id = match NodeId::from_str(&node_id) {
            Ok(node_id) => node_id,
            Err(err) => {
                log::info!("Failed to parse NodeId: {err}");
                return;
            }
        };

        let value = DataValue::value_only(update.value.into_variant());

        let value = WriteValue {
            node_id,
            attribute_id: AttributeId::Value as u32,
            index_range: Default::default(),
            value,
        };

        if let Err(err) = session.write(&[value]) {
            log::info!("Failed to write: {err}");
        }
    }
}

pub trait IntoVariant {
    fn into_variant(self) -> Variant;
}

impl IntoVariant for Value {
    fn into_variant(self) -> Variant {
        match self {
            Value::Null => Variant::Empty,
            Value::Bool(value) => value.into(),
            Value::Number(value) => {
                if let Some(value) = value.as_u64() {
                    value.into()
                } else if let Some(value) = value.as_i64() {
                    value.into()
                } else if let Some(value) = value.as_f64() {
                    value.into()
                } else {
                    log::warn!("Unknown numeric type");
                    Variant::StatusCode(StatusCode::BadInvalidArgument)
                }
            }
            Value::String(value) => value.into(),
            Value::Array(_) => {
                log::debug!("Arrays are only supported using the complex object type");
                Variant::StatusCode(StatusCode::BadDataEncodingUnsupported)
            }
            Value::Object(obj) => match serde_json::from_value::<Variant>(Value::Object(obj)) {
                Ok(value) => value,
                Err(err) => {
                    log::debug!("Failed to deserialize variant: {err}");
                    Variant::StatusCode(StatusCode::BadDataEncodingInvalid)
                }
            },
        }
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

#[cfg(test)]
mod test {
    use super::*;

    fn setup() {
        env_logger::try_init().ok();
    }

    #[test]
    fn test_variant_simple() {
        setup();

        assert_eq!(Value::Null.into_variant(), Variant::Empty);
        assert_eq!(json!(true).into_variant(), Variant::Boolean(true));
        assert_eq!(json!(100).into_variant(), Variant::UInt64(100));
        assert_eq!(json!(-100).into_variant(), Variant::Int64(-100));
        assert_eq!(json!(1.23).into_variant(), Variant::Double(1.23));
    }

    #[test]
    fn test_variant_invalid() {
        setup();

        assert_eq!(
            json!([false, 1, "2"]).into_variant(),
            Variant::StatusCode(StatusCode::BadDataEncodingUnsupported)
        );
        assert_eq!(
            json!({}).into_variant(),
            Variant::StatusCode(StatusCode::BadDataEncodingInvalid)
        );
    }

    #[test]
    fn test_variant_complex() {
        setup();

        assert_eq!(
            json!({
                "Int32": 100,
            })
            .into_variant(),
            Variant::Int32(100)
        );

        assert_eq!(
            json!({
                "Array": {
                    "value_type": "Byte",
                    "values": [
                        { "Byte": 1 },
                        { "Byte": 2 },
                        { "Byte": 3 },
                    ],
                    "dimensions": []
                },
            })
            .into_variant(),
            Variant::Array(Box::new(
                Array::new_single(
                    VariantTypeId::Byte,
                    [Variant::Byte(1), Variant::Byte(2), Variant::Byte(3)]
                )
                .unwrap()
            ))
        );
    }
}
