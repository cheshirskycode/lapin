#[derive(Default, Clone)]
pub struct RecoveryConfig {
    pub(crate) auto_recover_channels: bool,
}

impl RecoveryConfig {
    pub fn auto_recover_channels(&mut self) -> &mut Self {
        self.auto_recover_channels = true;
        self
    }
}
