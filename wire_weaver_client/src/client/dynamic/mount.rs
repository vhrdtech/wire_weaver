//! Walking into APIs mounted at slots (CLT-84): a step past a slot continues in the API mounted there, from its own
//! introspection data, through a [MountView] of the slot.

use super::{DynResource, Node, parse_path};
use crate::client::dynamic::PathStep;
use crate::{Commander, Error, MountView};
use ww_client_server::PathKind;

/// A resource [DynResource::walk_mounted] found, and the view of the slot it is in.
#[derive(Clone)]
pub struct DynWalked {
    /// The resource, its path in the API it belongs to.
    pub resource: DynResource,
    /// The view of the innermost slot the walk went through, `None` in the device's own API.
    pub view: Option<MountView>,
}

impl DynWalked {
    /// The Commander to use the resource with: the view's, or `link` (the device's) outside a mount.
    pub fn cmd<'a>(&'a self, link: &'a Commander) -> &'a Commander {
        self.view.as_ref().map_or(link, MountView::commander)
    }
}

/// How a step walks on from a mount slot: into the API mounted there, then maybe to a resource of it.
enum Enter {
    /// `api`: the mounted API's root.
    Root,
    /// Another name: that resource of the mounted API.
    Child(String),
}

impl DynResource {
    /// [Self::walk_on], into mounted APIs as well: a step past a slot of `mount!` or an element of `mounts!`
    /// continues in the API mounted there, learned through a view of the slot. The slot's `api` names the mounted
    /// API's root and may be left out: `hub.api.player.pause` and `hub.player.pause` are the same resource, while
    /// `hub.presence` stays the slot's own; `api.presence` reaches a mounted item named `presence`. A name after a
    /// `mounts!` array picks its slot by name: `ext.cast` is `ext["cast"]`.
    pub async fn walk_mounted(&self, cmd: &Commander, path: &str) -> Result<DynWalked, Error> {
        let mut walked = DynWalked {
            resource: self.clone(),
            view: None,
        };
        for step in parse_path(path)? {
            let res = &walked.resource;
            let step_cmd = walked.cmd(cmd).clone();
            walked.resource = match step {
                // the key of `mounts!` is always `ww_api::mount::MountName`
                PathStep::Name(name) if res.is_mount_array() => {
                    let key = res.by_key_text(&format!("MountName({name:?})"))?;
                    res.by_key(&step_cmd, &key)
                        .await?
                        .ok_or_else(|| super::keys::no_element(res, &name))?
                }
                PathStep::Name(name) => match res.enters(name) {
                    Ok(enter) => {
                        let view = step_cmd.mount_slot(res.slot_path()?)?.view().await?;
                        let root = res.mounted_root(&view)?;
                        walked.view = Some(view);
                        match enter {
                            Enter::Root => root,
                            Enter::Child(name) => root.child(&name)?,
                        }
                    }
                    Err(name) => res.child(&name)?,
                },
                PathStep::Index(index) => res.index(index)?,
                PathStep::Key(text) => {
                    let key = res.by_key_text(&text)?;
                    res.by_key(&step_cmd, &key)
                        .await?
                        .ok_or_else(|| super::keys::no_element(res, &text))?
                }
            };
        }
        Ok(walked)
    }

    /// Blocking variant of [Self::walk_mounted], from outside an async task with a tokio runtime entered.
    pub fn walk_mounted_blocking(&self, cmd: &Commander, path: &str) -> Result<DynWalked, Error> {
        let mut walked = DynWalked {
            resource: self.clone(),
            view: None,
        };
        for step in parse_path(path)? {
            let res = &walked.resource;
            let step_cmd = walked.cmd(cmd).clone();
            walked.resource = match step {
                // the key of `mounts!` is always `ww_api::mount::MountName`
                PathStep::Name(name) if res.is_mount_array() => {
                    let key = res.by_key_text(&format!("MountName({name:?})"))?;
                    res.by_key_blocking(&step_cmd, &key)?
                        .ok_or_else(|| super::keys::no_element(res, &name))?
                }
                PathStep::Name(name) => match res.enters(name) {
                    Ok(enter) => {
                        let view = step_cmd.mount_slot(res.slot_path()?)?.view_blocking()?;
                        let root = res.mounted_root(&view)?;
                        walked.view = Some(view);
                        match enter {
                            Enter::Root => root,
                            Enter::Child(name) => root.child(&name)?,
                        }
                    }
                    Err(name) => res.child(&name)?,
                },
                PathStep::Index(index) => res.index(index)?,
                PathStep::Key(text) => {
                    let key = res.by_key_text(&text)?;
                    res.by_key_blocking(&step_cmd, &key)?
                        .ok_or_else(|| super::keys::no_element(res, &text))?
                }
            };
        }
        Ok(walked)
    }

    /// An array of `mounts!` slots, not yet indexed.
    fn is_mount_array(&self) -> bool {
        matches!(&self.node, Node::Array(element) if matches!(**element, Node::Mount(_)))
    }

    /// How the step `name` walks on from this resource: into the mounted API when this is a slot and `name` is not
    /// one of the slot's own items but `api`; `Err(name)` otherwise, a step within this API.
    fn enters(&self, name: String) -> Result<Enter, String> {
        let Node::Mount(slot) = &self.node else {
            return Err(name);
        };
        if ww_self::mount::item_name(ww_self::mount::API) == Some(name.as_str()) {
            Ok(Enter::Root)
        } else if slot.items.iter().any(|item| item.ident == name) {
            Err(name)
        } else {
            Ok(Enter::Child(name))
        }
    }

    /// Path of this slot's level in its API.
    fn slot_path(&self) -> Result<PathKind<'_>, Error> {
        self.path_kind()
    }

    /// The root of the API mounted at this slot, from `view`'s introspection data: an error while the slot is
    /// empty, or when the backend has no introspection data.
    fn mounted_root(&self, view: &MountView) -> Result<DynResource, Error> {
        let Some(bundle) = view.commander().device_introspect() else {
            let why = if view.has_learned() {
                "its backend has no introspection data"
            } else {
                "nothing is mounted there"
            };
            return Err(Error::Other(format!(
                "the API mounted at '{}' is not known: {why}",
                self.display_path()
            )));
        };
        let mut root = DynResource::root(bundle.api_bundle);
        root.path = self.path.clone();
        Ok(root)
    }
}
