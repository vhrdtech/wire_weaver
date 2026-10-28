pub mod codegen;
pub mod const_model;
pub mod handler_model;
// pub mod eval;
pub mod layout;
mod local_registry;
pub mod method_model;
pub mod property_model;
pub mod transform;

pub use transform::{
    CratePaths, UnitHint, dependency_dir, load, load_crate, load_crate_with_hints, load_dep,
    load_dep_with_paths, load_with_hints,
};

pub use codegen::api_client::{ClientModel, GenClientConfig, gen_client};
pub use codegen::api_server::{GenServerConfig, gen_server};
pub use codegen::server::scaffold::{ServerScaffoldConfig, gen_server_scaffold};

// for convenience in build.rs scripts
pub mod prelude {
    pub use crate::codegen::api_client::{ClientModel, GenClientConfig, gen_client};
    pub use crate::codegen::api_server::{GenServerConfig, gen_server};
    pub use crate::const_model::{ConstModel, ConstModelItem, ConstModelKind};
    pub use crate::handler_model::{HandlerKind, HandlerModel, HandlerModelItem};
    pub use crate::property_model::{PropertyModel, PropertyModelItem, PropertyModelKind};
    pub use crate::transform::{load, load_crate, load_dep};
}

pub mod internal {
    pub use crate::transform::{ErrorListError, PropertyMacroArgs, StreamAndImplMacroArgs};
    pub use ww_self;
    pub use ww_self::PropertyAccess;
}
