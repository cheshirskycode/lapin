use std::sync::Arc;
use prometheus::Histogram;
use prometheus::HistogramOpts;
use prometheus::IntCounter;
use prometheus::Opts;
use prometheus::IntGauge;

#[derive(Debug, Clone)]
pub struct Metrics {
    inner: Arc<Inner>,
}

#[derive(Debug)]
pub struct Inner {
    pub(crate) frames_push_frames: Histogram,
    pub(crate) loop_duration: Histogram,
    pub(crate) loop_write_duration: Histogram,
    pub(crate) bytes_written: IntCounter,
    pub(crate) loop_read_duration: Histogram,
    pub(crate) bytes_read: IntCounter,

    pub(crate) write_speed: IntGauge,
    pub(crate) read_speed: IntGauge,

    pub(crate) frames_publish: IntGauge,
    pub(crate) frames_retry: IntGauge,
    pub(crate) frames_low_prio: IntGauge,
    pub(crate) frames_frames: IntGauge,
    pub(crate) frames_expected_replies: IntGauge,
}


impl Metrics {
    pub fn new(registry: &prometheus::Registry, const_labels: &[(&str, &str)]) -> Self {
        let labels: std::collections::HashMap<String, String> = const_labels
            .iter()
            .map(|(k, v)| (String::from(*k), String::from(*v)))
            .collect();

        let frames_push_frames = Histogram::with_opts(
            HistogramOpts::new(
                "lapin_frames_push_frames",
                "Time taken to push frames",
            )
            .const_labels(labels.clone())
            .buckets(vec![0.000005, 0.00001, 0.00025, 0.0005, 0.001, 0.025, 0.05, 0.1, 0.2, 0.5, 1.0,])
        ).unwrap();
        let loop_duration = Histogram::with_opts(
            HistogramOpts::new(
                "lapin_loop_duration",
                "Time taken to run the main loop",
            )
                .const_labels(labels.clone())
                .buckets(vec![0.000005, 0.00001, 0.00025, 0.0005, 0.001, 0.025, 0.05, 0.1, 0.2, 0.5, 1.0,])
        ).unwrap();
        let loop_write_duration = Histogram::with_opts(
            HistogramOpts::new(
                "lapin_loop_write_duration",
                "Time taken to write to the socket",
            )
                .const_labels(labels.clone())
                .buckets(vec![0.000005, 0.00001, 0.00025, 0.0005, 0.001, 0.025, 0.05, 0.1, 0.2, 0.5, 1.0,])
        ).unwrap();
        let bytes_written = IntCounter::with_opts(
            Opts::new(
                "lapin_bytes_written",
                "Number of bytes written to the socket",
            )
                .const_labels(labels.clone())
        ).unwrap();
        let loop_read_duration = Histogram::with_opts(
            HistogramOpts::new(
                "lapin_loop_read_duration",
                "Time taken to read from the socket",
            )
                .const_labels(labels.clone())
                .buckets(vec![0.000005, 0.00001, 0.00025, 0.0005, 0.001, 0.025, 0.05, 0.1, 0.2, 0.5, 1.0,])
        ).unwrap();
        let bytes_read = IntCounter::with_opts(
            Opts::new(
                "lapin_bytes_read",
                "Number of bytes read from the socket",
            )
                .const_labels(labels.clone())
        ).unwrap();

        let write_speed = IntGauge::with_opts(
            Opts::new(
                "lapin_write_speed",
                "Number of bytes written to the socket per second",
            )
                .const_labels(labels.clone())
        ).unwrap();
        let read_speed = IntGauge::with_opts(
            Opts::new(
                "lapin_read_speed",
                "Number of bytes read from the socket per second",
            )
                .const_labels(labels.clone())
        ).unwrap();

        let frames_publish = IntGauge::with_opts(
            Opts::new(
                "lapin_frames_publish",
                "Number of frames published",
            )
                .const_labels(labels.clone())
        ).unwrap();
        let frames_retry = IntGauge::with_opts(
            Opts::new(
                "lapin_frames_retry",
                "Number of frames retried",
            )
                .const_labels(labels.clone())
        ).unwrap();
        let frames_low_prio = IntGauge::with_opts(
            Opts::new(
                "lapin_frames_low_prio",
                "Number of frames with low priority",
            )
                .const_labels(labels.clone())
        ).unwrap();
        let frames_frames = IntGauge::with_opts(
            Opts::new(
                "lapin_frames_frames",
                "Number of frames",
            )
                .const_labels(labels.clone())
        ).unwrap();
        let frames_expected_replies = IntGauge::with_opts(
            Opts::new(
                "lapin_frames_expected_replies",
                "Number of expected replies",
            )
                .const_labels(labels.clone())
        ).unwrap();

        registry.register(Box::new(frames_push_frames.clone())).unwrap();
        registry.register(Box::new(loop_duration.clone())).unwrap();
        registry.register(Box::new(loop_write_duration.clone())).unwrap();
        registry.register(Box::new(bytes_written.clone())).unwrap();
        registry.register(Box::new(loop_read_duration.clone())).unwrap();
        registry.register(Box::new(bytes_read.clone())).unwrap();
        registry.register(Box::new(write_speed.clone())).unwrap();
        registry.register(Box::new(read_speed.clone())).unwrap();
        registry.register(Box::new(frames_publish.clone())).unwrap();
        registry.register(Box::new(frames_retry.clone())).unwrap();
        registry.register(Box::new(frames_low_prio.clone())).unwrap();
        registry.register(Box::new(frames_frames.clone())).unwrap();
        registry.register(Box::new(frames_expected_replies.clone())).unwrap();

        Self {
            inner: Arc::new(Inner {
                frames_push_frames,
                loop_duration,
                loop_write_duration,
                bytes_written,
                loop_read_duration,
                bytes_read,
                write_speed,
                read_speed,
                frames_publish,
                frames_retry,
                frames_low_prio,
                frames_frames,
                frames_expected_replies,
            })
        }
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new(prometheus::default_registry(), &[])
    }
}

impl std::ops::Deref for Metrics {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}