pub mod codegen;
pub mod const_model;
// pub mod eval;
pub mod layout;
mod local_registry;
pub mod method_model;
pub mod property_model;
pub mod transform;

pub use codegen::CodegenBundle;
pub use transform::{load, load_crate, load_dep, load_for_codegen};

pub use codegen::api_client::{ClientModel, GenClientConfig, gen_client};
pub use codegen::api_server::{GenServerConfig, gen_server};
pub use codegen::server::scaffold::{ServerScaffoldConfig, gen_server_scaffold};

// for convenience in build.rs scripts
pub mod prelude {
    pub use crate::codegen::CodegenBundle;
    pub use crate::codegen::api_client::{ClientModel, GenClientConfig, gen_client};
    pub use crate::codegen::api_server::{GenServerConfig, gen_server};
    pub use crate::const_model::{ConstModel, ConstModelItem, ConstModelKind};
    pub use crate::property_model::{PropertyModel, PropertyModelItem, PropertyModelKind};
    pub use crate::transform::{load, load_crate, load_dep, load_for_codegen};
}

pub mod internal {
    pub use crate::transform::{ErrorListError, PropertyMacroArgs, StreamAndImplMacroArgs};
    pub use ww_self;
    pub use ww_self::PropertyAccess;
}
