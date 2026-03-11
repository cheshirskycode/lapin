use crate::{Error, ErrorKind, Result, types::ShortString, uri::AMQPUri};
use std::{
    fmt,
    sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard},
};

#[derive(Clone, Default)]
pub struct ConnectionStatus(Arc<RwLock<Inner>>);

impl ConnectionStatus {
    pub(crate) fn new(uri: &AMQPUri) -> Self {
        let status = Self::default();
        status.set_vhost(&uri.vhost);
        status.set_username(&uri.authority.userinfo.username);
        status
    }

    pub(crate) fn state(&self) -> ConnectionState {
        self.read().state
    }

    pub(crate) fn set_state(&self, state: ConnectionState) -> ConnectionState {
        let mut inner = self.write();
        std::mem::replace(&mut inner.state, state)
    }

    pub(crate) fn set_connecting(&self) -> Result<()> {
        self.write().set_connecting()
    }

    pub(crate) fn set_reconnecting(&self) {
        self.write().set_reconnecting();
    }

    pub fn vhost(&self) -> ShortString {
        self.read().vhost.clone()
    }

    pub(crate) fn set_vhost(&self, vhost: &str) {
        self.write().vhost = vhost.into();
    }

    pub fn username(&self) -> String {
        self.read().username.clone()
    }

    pub(crate) fn set_username(&self, username: &str) {
        self.write().username = username.into();
    }

    pub(crate) fn block(&self) {
        self.write().blocked = true;
    }

    pub(crate) fn unblock(&self) {
        self.write().blocked = false;
    }

    pub fn blocked(&self) -> bool {
        self.read().blocked
    }

    pub fn connected(&self) -> bool {
        self.state() == ConnectionState::Connected
    }

    pub(crate) fn ensure_connected(&self) -> Result<()> {
        if !self.connected() {
            return Err(ErrorKind::InvalidConnectionState(self.state()).into());
        }
        Ok(())
    }

    pub fn connecting(&self) -> bool {
        self.state() == ConnectionState::Connecting
    }

    pub fn reconnecting(&self) -> bool {
        self.state() == ConnectionState::Reconnecting
    }

    pub fn closing(&self) -> bool {
        self.state() == ConnectionState::Closing
    }

    pub fn closed(&self) -> bool {
        self.state() == ConnectionState::Closed
    }

    pub fn errored(&self) -> bool {
        self.state() == ConnectionState::Error
    }

    pub(crate) fn poison(&self, err: Error) {
        self.write().poison(err);
    }

    pub(crate) fn auto_close(&self) -> bool {
        [ConnectionState::Connecting, ConnectionState::Connected].contains(&self.state())
    }

    fn read(&self) -> RwLockReadGuard<'_, Inner> {
        self.0.read().unwrap_or_else(|e| e.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, Inner> {
        self.0.write().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ConnectionState {
    #[default]
    Initial,
    Connecting,
    Connected,
    Closing,
    Closed,
    Reconnecting,
    Error,
}

impl fmt::Debug for ConnectionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("ConnectionStatus");
        if let Ok(inner) = self.0.try_read() {
            debug
                .field("state", &inner.state)
                .field("vhost", &inner.vhost)
                .field("username", &inner.username)
                .field("blocked", &inner.blocked);
        }
        debug.finish()
    }
}

struct Inner {
    state: ConnectionState,
    vhost: ShortString,
    username: String,
    blocked: bool,
    poison: Option<Error>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            state: ConnectionState::default(),
            vhost: "/".into(),
            username: "guest".into(),
            blocked: false,
            poison: None,
        }
    }
}

impl Inner {
    fn set_connecting(&mut self) -> Result<()> {
        self.state = ConnectionState::Connecting;
        self.poison.take().map(Err).unwrap_or(Ok(()))
    }

    fn set_reconnecting(&mut self) {
        let _ = self.poison.take();
        self.state = ConnectionState::Reconnecting;
        self.blocked = false;
    }

    fn poison(&mut self, err: Error) {
        self.poison = Some(err);
    }
}
