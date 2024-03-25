use crate::types::{AMQPValue, FieldTable, LongString};
use executor_trait::FullExecutor;
use reactor_trait::Reactor;
use std::sync::Arc;

#[derive(Clone)]
pub struct ConnectionProperties {
    pub locale: String,
    pub client_properties: FieldTable,
    pub executor: Option<Arc<dyn FullExecutor + Send + Sync>>,
    pub reactor: Option<Arc<dyn Reactor + Send + Sync>>,
    pub metrics: Option<crate::metrics::Metrics>,
}

impl Default for ConnectionProperties {
    fn default() -> Self {
        Self {
            locale: "en_US".into(),
            client_properties: FieldTable::default(),
            executor: None,
            reactor: None,
            metrics: None,
        }
    }
}

impl ConnectionProperties {
    #[must_use]
    pub fn with_connection_name(mut self, connection_name: LongString) -> Self {
        self.client_properties.insert(
            "connection_name".into(),
            AMQPValue::LongString(connection_name),
        );
        self
    }

    #[must_use]
    pub fn with_executor<E: FullExecutor + Send + Sync + 'static>(mut self, executor: E) -> Self {
        self.executor = Some(Arc::new(executor));
        self
    }

    #[must_use]
    pub fn with_reactor<R: Reactor + Send + Sync + 'static>(mut self, reactor: R) -> Self {
        self.reactor = Some(Arc::new(reactor));
        self
    }

    #[must_use]
    pub fn with_metrics(mut self, metrics: crate::metrics::Metrics) -> Self {
        self.metrics = Some(metrics);
        self
    }
}
