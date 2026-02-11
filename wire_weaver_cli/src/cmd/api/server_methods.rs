use crate::util;
use std::path::PathBuf;
use wire_weaver_core::ast::api::{ApiItem, ApiItemKind};
use wire_weaver_core::ast::visit::{Visit, visit_api_level};
use wire_weaver_core::ast::visitors::IndexChain;

pub fn server_methods(path: PathBuf, name: Option<String>) -> anyhow::Result<()> {
    let level = util::load_level(path, name)?;
    visit_api_level(
        &level,
        &mut ServerMethods {
            ..Default::default()
        },
    );
    Ok(())
}

#[derive(Default)]
struct ServerMethods {
    index_chain: IndexChain,
}

impl Visit for ServerMethods {
    fn hook(&mut self) -> Option<&mut dyn Visit> {
        Some(&mut self.index_chain)
    }

    fn visit_api_item(&mut self, item: &ApiItem) {
        if !matches!(
            item.kind,
            ApiItemKind::Method { .. } | ApiItemKind::Property { .. } | ApiItemKind::Stream { .. }
        ) {
            return;
        }
        println!(
            "{} {:?}",
            self.index_chain.flattened_name(),
            self.index_chain.array_indices()
        );
    }
}
