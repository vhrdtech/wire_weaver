use crate::CommandSender;
use ww_version::FullVersion;

/// Trait attachment point that carries:
/// * which device it belongs to
/// * at which resource path
/// * default timeout carried over client code
/// * crate name, it's version and trait name
///
/// Trait attachments are useful for implementing generic drivers that only know about
/// a trait and nothing about a device that implements that trait.
///
/// For example: `ww_gpio_hl` uses "low-level" `ww_gpio` trait describing IO pins and
/// exposes a more user-friendly and canonical API with each pin represented by a separate struct.
pub struct Attachment {
    cmd_tx: CommandSender,
    source_crate: FullVersion<'static>,
    trait_name: &'static str,
}

impl Attachment {
    pub fn new(
        cmd_tx: CommandSender,
        source_crate: FullVersion<'static>,
        trait_name: &'static str,
    ) -> Self {
        Self {
            cmd_tx,
            source_crate,
            trait_name,
        }
    }

    pub fn cmd_tx(&mut self) -> &mut CommandSender {
        &mut self.cmd_tx
    }

    pub fn cmd_tx_take(self) -> CommandSender {
        self.cmd_tx
    }

    pub fn source_crate(&self) -> &FullVersion<'static> {
        &self.source_crate
    }

    pub fn trait_name(&self) -> &str {
        &self.trait_name
    }
}
