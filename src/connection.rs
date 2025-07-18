use crate::{
    Error,
    ErrorKind,
    Promise,
    Result,
    channel::Channel,
    channels::Channels,
    configuration::Configuration,
    connection_closer::ConnectionCloser,
    connection_properties::ConnectionProperties,
    connection_status::{ConnectionState, ConnectionStatus, ConnectionStep},
    frames::Frames,
    heartbeat::Heartbeat,
    internal_rpc::{InternalRPC, InternalRPCHandle},
    io_loop::IoLoop,
    recovery_config::RecoveryConfig,
    socket_state::{SocketState, SocketStateHandle},
    tcp::{AMQPUriTcpExt, HandshakeResult, OwnedTLSConfig},
    thread::ThreadHandle,
    // topology::TopologyDefinition,
    types::ReplyCode,
    uri::AMQPUri,
};
use amq_protocol::frame::{AMQPFrame, ProtocolVersion};
use async_trait::async_trait;
use executor_trait::FullExecutor;
use reactor_trait::IOHandle;
use std::{fmt, io, sync::Arc};
use tracing::{Level, level_enabled};

/// A TCP connection to the AMQP server.
///
/// To connect to the server, one of the [`connect`] methods has to be called.
///
/// Afterwards, create a [`Channel`] by calling [`create_channel`].
///
/// Also see the RabbitMQ documentation on [connections](https://www.rabbitmq.com/connections.html).
///
/// [`connect`]: ./struct.Connection.html#method.connect
/// [`Channel`]: ./struct.Channel.html
/// [`create_channel`]: ./struct.Connection.html#method.create_channel
pub struct Connection {
    configuration: Configuration,
    status: ConnectionStatus,
    channels: Channels,
    io_loop: ThreadHandle,
    closer: Arc<ConnectionCloser>,
}

impl Connection {
    fn new(
        waker: SocketStateHandle,
        internal_rpc: InternalRPCHandle,
        frames: Frames,
        executor: Arc<dyn FullExecutor + Send + Sync>,
        recovery_config: RecoveryConfig,
    ) -> Self {
        let configuration = Configuration::default();
        let status = ConnectionStatus::default();
        let channels = Channels::new(
            configuration.clone(),
            status.clone(),
            waker,
            internal_rpc.clone(),
            frames,
            executor,
            recovery_config,
        );
        let closer = Arc::new(ConnectionCloser::new(status.clone(), internal_rpc));
        let connection = Self {
            configuration,
            status,
            channels,
            io_loop: ThreadHandle::default(),
            closer,
        };

        connection.channels.create_zero();
        connection
    }

    /// Connect to an AMQP Server.
    ///
    /// The URI must be in the following format:
    ///
    /// * `amqp://127.0.0.1:5672` will connect to the default virtual host `/`.
    /// * `amqp://127.0.0.1:5672/` will connect to the virtual host `""` (empty string).
    /// * `amqp://127.0.0.1:5672/%2f` will connect to the default virtual host `/`.
    ///
    /// Note that the virtual host has to be escaped with
    /// [URL encoding](https://en.wikipedia.org/wiki/Percent-encoding).
    pub async fn connect(uri: &str, options: ConnectionProperties) -> Result<Connection> {
        Connect::connect(uri, options, OwnedTLSConfig::default()).await
    }

    /// Connect to an AMQP Server.
    pub async fn connect_with_config(
        uri: &str,
        options: ConnectionProperties,
        config: OwnedTLSConfig,
    ) -> Result<Connection> {
        Connect::connect(uri, options, config).await
    }

    /// Connect to an AMQP Server.
    pub async fn connect_uri(uri: AMQPUri, options: ConnectionProperties) -> Result<Connection> {
        Connect::connect(uri, options, OwnedTLSConfig::default()).await
    }

    /// Connect to an AMQP Server
    pub async fn connect_uri_with_config(
        uri: AMQPUri,
        options: ConnectionProperties,
        config: OwnedTLSConfig,
    ) -> Result<Connection> {
        Connect::connect(uri, options, config).await
    }

    /// Creates a new [`Channel`] on this connection.
    ///
    /// This method is only successful if the client is connected.
    /// Otherwise, [`InvalidConnectionState`] error is returned.
    ///
    /// [`Channel`]: ./struct.Channel.html
    /// [`InvalidConnectionState`]: ./enum.Error.html#variant.InvalidConnectionState
    pub async fn create_channel(&self) -> Result<Channel> {
        if !self.status.connected() {
            return Err(ErrorKind::InvalidConnectionState(self.status.state()).into());
        }
        let channel = self.channels.create(self.closer.clone())?;
        channel.clone().channel_open(channel).await
    }

    /// Block current thread while the connection is still active.
    /// This is useful when you only have a consumer and nothing else keeping your application
    /// "alive".
    pub fn run(self) -> Result<()> {
        let io_loop = self.io_loop.clone();
        drop(self);
        io_loop.wait("io loop")
    }

    pub fn on_error<E: FnMut(Error) + Send + 'static>(&self, handler: E) {
        self.channels.set_error_handler(handler);
    }

    pub fn configuration(&self) -> &Configuration {
        &self.configuration
    }

    pub fn status(&self) -> &ConnectionStatus {
        &self.status
    }

    /// Request a connection close.
    ///
    /// This method is only successful if the connection is in the connected state,
    /// otherwise an [`InvalidConnectionState`] error is returned.
    ///
    /// [`InvalidConnectionState`]: ./enum.Error.html#variant.InvalidConnectionState
    pub async fn close(&self, reply_code: ReplyCode, reply_text: &str) -> Result<()> {
        if !self.status.connected() {
            return Err(ErrorKind::InvalidConnectionState(self.status.state()).into());
        }

        self.channels.set_connection_closing();
        if let Some(channel0) = self.channels.get(0) {
            channel0
                .connection_close(reply_code, reply_text, 0, 0)
                .await
        } else {
            Ok(())
        }
    }

    /// Block all consumers and publishers on this connection
    pub async fn block(&self, reason: &str) -> Result<()> {
        if let Some(channel0) = self.channels.get(0) {
            channel0.connection_blocked(reason).await
        } else {
            Err(ErrorKind::InvalidConnectionState(self.status.state()).into())
        }
    }

    /// Unblock all consumers and publishers on this connection
    pub async fn unblock(&self) -> Result<()> {
        if let Some(channel0) = self.channels.get(0) {
            channel0.connection_unblocked().await
        } else {
            Err(ErrorKind::InvalidConnectionState(self.status.state()).into())
        }
    }

    /// Update the secret used by some authentication module such as OAuth2
    pub async fn update_secret(&self, new_secret: &str, reason: &str) -> Result<()> {
        if let Some(channel0) = self.channels.get(0) {
            channel0.connection_update_secret(new_secret, reason).await
        } else {
            Err(ErrorKind::InvalidConnectionState(self.status.state()).into())
        }
    }

    pub async fn connector(
        uri: AMQPUri,
        connect: Box<dyn FnOnce(&AMQPUri) -> HandshakeResult + Send + Sync>,
        mut options: ConnectionProperties,
    ) -> Result<Connection> {
        let executor = options.take_executor()?;
        let reactor = options.take_reactor()?;
        let (connect_promise, resolver) = Promise::new();
        let connect_uri = uri.clone();
        executor.spawn({
            let executor = executor.clone();
            Box::pin(async move {
                executor
                    .spawn_blocking(Box::new(move || {
                        let mut res = connect(&connect_uri);
                        loop {
                            match res {
                                Ok(stream) => {
                                    resolver.resolve(stream);
                                    break;
                                }
                                Err(mid) => match mid.into_mid_handshake_tls_stream() {
                                    Err(err) => {
                                        resolver.reject(err.into());
                                        break;
                                    }
                                    Ok(mid) => {
                                        res = mid.handshake();
                                    }
                                },
                            }
                        }
                    }))
                    .await;
            })
        });

        let socket_state = SocketState::default();
        let waker = socket_state.handle();
        let internal_rpc = InternalRPC::new(executor.clone(), waker.clone());
        let frames = Frames::default();
        let conn = Connection::new(
            waker,
            internal_rpc.handle(),
            frames.clone(),
            executor.clone(),
            options.recovery_config.clone().unwrap_or_default(),
        );
        let status = conn.status.clone();
        let configuration = conn.configuration.clone();
        status.set_vhost(&uri.vhost);
        status.set_username(&uri.authority.userinfo.username);
        if let Some(frame_max) = uri.query.frame_max {
            configuration.set_frame_max(frame_max);
        }
        if let Some(channel_max) = uri.query.channel_max {
            configuration.set_channel_max(channel_max);
        }
        if let Some(heartbeat) = uri.query.heartbeat {
            configuration.set_heartbeat(heartbeat);
        }
        let (promise_out, resolver) = Promise::new();
        if level_enabled!(Level::TRACE) {
            promise_out.set_marker("ProtocolHeader".into());
        }
        let channels = conn.channels.clone();
        if let Some(channel0) = channels.get(0) {
            channel0.send_frame(
                AMQPFrame::ProtocolHeader(ProtocolVersion::amqp_0_9_1()),
                resolver,
                None,
            )
        };
        let (promise_in, resolver) = Promise::new();
        if level_enabled!(Level::TRACE) {
            promise_in.set_marker("ProtocolHeader.Ok".into());
        }
        let io_loop_handle = conn.io_loop.clone();
        status.set_state(ConnectionState::Connecting);
        status.set_connection_step(ConnectionStep::ProtocolHeader(
            resolver,
            conn,
            uri.authority.userinfo.into(),
            uri.query.auth_mechanism.unwrap_or_default(),
            options,
        ));
        let stream = connect_promise
            .await
            .and_then(|stream| reactor.register(IOHandle::new(stream)).map_err(Into::into))
            .inspect_err(|_| {
                // We don't actually need the resolver as we already pass it around to the failing
                // code which will propagate the error. We only want to flush the status internal
                // state.
                let _ = status.connection_resolver();
            })?
            .into();
        let heartbeat = Heartbeat::new(status.clone(), channels.clone(), executor.clone(), reactor);
        let internal_rpc_handle = internal_rpc.handle();
        executor.spawn(Box::pin(internal_rpc.run(channels.clone())));
        IoLoop::new(
            status,
            configuration,
            channels,
            internal_rpc_handle,
            frames,
            socket_state,
            io_loop_handle,
            stream,
            heartbeat,
        )
        .await
        .and_then(IoLoop::start)?;
        promise_out.await?;
        promise_in.await
    }
}

impl fmt::Debug for Connection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connection")
            .field("configuration", &self.configuration)
            .field("status", &self.status)
            .field("channels", &self.channels)
            .finish()
    }
}

/// Trait providing a method to connect to an AMQP server
#[async_trait]
pub trait Connect {
    /// connect to an AMQP server
    async fn connect(
        self,
        options: ConnectionProperties,
        config: OwnedTLSConfig,
    ) -> Result<Connection>;
}

#[async_trait]
impl Connect for AMQPUri {
    async fn connect(
        self,
        options: ConnectionProperties,
        config: OwnedTLSConfig,
    ) -> Result<Connection> {
        Connection::connector(
            self,
            Box::new(move |uri| AMQPUriTcpExt::connect_with_config(uri, config.as_ref())),
            options,
        )
        .await
    }
}

#[async_trait]
impl Connect for &str {
    async fn connect(
        self,
        options: ConnectionProperties,
        config: OwnedTLSConfig,
    ) -> Result<Connection> {
        match self.parse::<AMQPUri>() {
            Ok(uri) => Connect::connect(uri, options, config).await,
            Err(err) => Err(io::Error::other(err).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BasicProperties;
    use crate::channel_receiver_state::{ChannelReceiverState, DeliveryCause};
    use crate::channel_status::ChannelState;
    use crate::options::BasicConsumeOptions;
    use crate::types::{ChannelId, FieldTable, ShortString};
    use amq_protocol::frame::AMQPContentHeader;
    use amq_protocol::protocol::{AMQPClass, basic};

    #[test]
    fn channel_limit() {
        let _ = tracing_subscriber::fmt::try_init();

        // Bootstrap connection state to a consuming state
        let executor = Arc::new(async_global_executor_trait::AsyncGlobalExecutor);
        let socket_state = SocketState::default();
        let waker = socket_state.handle();
        let internal_rpc = InternalRPC::new(executor.clone(), waker.clone());
        let conn = Connection::new(
            waker,
            internal_rpc.handle(),
            Frames::default(),
            executor.clone(),
            RecoveryConfig::default(),
        );
        conn.status.set_state(ConnectionState::Connected);
        conn.configuration.set_channel_max(ChannelId::MAX);
        for _ in 1..=ChannelId::MAX {
            conn.channels.create(conn.closer.clone()).unwrap();
        }

        assert_eq!(
            conn.channels.create(conn.closer.clone()),
            Err(ErrorKind::ChannelsLimitReached.into())
        );
    }

    #[test]
    fn basic_consume_small_payload() {
        let _ = tracing_subscriber::fmt::try_init();

        use crate::consumer::Consumer;

        // Bootstrap connection state to a consuming state
        let executor = Arc::new(async_global_executor_trait::AsyncGlobalExecutor);
        let socket_state = SocketState::default();
        let waker = socket_state.handle();
        let internal_rpc = InternalRPC::new(executor.clone(), waker.clone());
        let conn = Connection::new(
            waker,
            internal_rpc.handle(),
            Frames::default(),
            executor.clone(),
            RecoveryConfig::default(),
        );
        conn.status.set_state(ConnectionState::Connected);
        conn.configuration.set_channel_max(2047);
        let channel = conn.channels.create(conn.closer.clone()).unwrap();
        channel.set_state(ChannelState::Connected);
        let queue_name = ShortString::from("consumed");
        let consumer_tag = ShortString::from("consumer-tag");
        let consumer = Consumer::new(
            consumer_tag.clone(),
            executor,
            None,
            queue_name.clone(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        );
        if let Some(c) = conn.channels.get(channel.id()) {
            c.register_consumer(consumer_tag.clone(), consumer);
            c.register_queue(queue_name.clone(), Default::default(), Default::default());
        }
        // Now test the state machine behaviour
        {
            let method = AMQPClass::Basic(basic::AMQPMethod::Deliver(basic::Deliver {
                consumer_tag: consumer_tag.clone(),
                delivery_tag: 1,
                redelivered: false,
                exchange: "".into(),
                routing_key: queue_name,
            }));
            let class_id = method.get_amqp_class_id();
            let deliver_frame = AMQPFrame::Method(channel.id(), method);
            conn.channels.handle_frame(deliver_frame).unwrap();
            let channel_state = channel.status().receiver_state();
            let expected_state = ChannelReceiverState::WillReceiveContent(
                class_id,
                DeliveryCause::Consume(consumer_tag.clone()),
            );
            assert_eq!(channel_state, expected_state);
        }
        {
            let header_frame = AMQPFrame::Header(
                channel.id(),
                60,
                Box::new(AMQPContentHeader {
                    class_id: 60,
                    body_size: 2,
                    properties: BasicProperties::default(),
                }),
            );
            conn.channels.handle_frame(header_frame).unwrap();
            let channel_state = channel.status().receiver_state();
            let expected_state =
                ChannelReceiverState::ReceivingContent(DeliveryCause::Consume(consumer_tag), 2);
            assert_eq!(channel_state, expected_state);
        }
        {
            let body_frame = AMQPFrame::Body(channel.id(), b"{}".to_vec());
            conn.channels.handle_frame(body_frame).unwrap();
            let channel_state = channel.status().state();
            let expected_state = ChannelState::Connected;
            assert_eq!(channel_state, expected_state);
        }
    }

    #[test]
    fn basic_consume_empty_payload() {
        let _ = tracing_subscriber::fmt::try_init();

        use crate::consumer::Consumer;

        // Bootstrap connection state to a consuming state
        let socket_state = SocketState::default();
        let waker = socket_state.handle();
        let executor = Arc::new(async_global_executor_trait::AsyncGlobalExecutor);
        let internal_rpc = InternalRPC::new(executor.clone(), waker.clone());
        let conn = Connection::new(
            waker,
            internal_rpc.handle(),
            Frames::default(),
            executor.clone(),
            RecoveryConfig::default(),
        );
        conn.status.set_state(ConnectionState::Connected);
        conn.configuration.set_channel_max(2047);
        let channel = conn.channels.create(conn.closer.clone()).unwrap();
        channel.set_state(ChannelState::Connected);
        let queue_name = ShortString::from("consumed");
        let consumer_tag = ShortString::from("consumer-tag");
        let consumer = Consumer::new(
            consumer_tag.clone(),
            executor,
            None,
            queue_name.clone(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        );
        if let Some(c) = conn.channels.get(channel.id()) {
            c.register_consumer(consumer_tag.clone(), consumer);
            c.register_queue(queue_name.clone(), Default::default(), Default::default());
        }
        // Now test the state machine behaviour
        {
            let method = AMQPClass::Basic(basic::AMQPMethod::Deliver(basic::Deliver {
                consumer_tag: consumer_tag.clone(),
                delivery_tag: 1,
                redelivered: false,
                exchange: "".into(),
                routing_key: queue_name,
            }));
            let class_id = method.get_amqp_class_id();
            let deliver_frame = AMQPFrame::Method(channel.id(), method);
            conn.channels.handle_frame(deliver_frame).unwrap();
            let channel_state = channel.status().receiver_state();
            let expected_state = ChannelReceiverState::WillReceiveContent(
                class_id,
                DeliveryCause::Consume(consumer_tag),
            );
            assert_eq!(channel_state, expected_state);
        }
        {
            let header_frame = AMQPFrame::Header(
                channel.id(),
                60,
                Box::new(AMQPContentHeader {
                    class_id: 60,
                    body_size: 0,
                    properties: BasicProperties::default(),
                }),
            );
            conn.channels.handle_frame(header_frame).unwrap();
            let channel_state = channel.status().state();
            let expected_state = ChannelState::Connected;
            assert_eq!(channel_state, expected_state);
        }
    }
}
