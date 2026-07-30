pub mod codegen;
// pub mod eval;
pub mod layout;
mod local_registry;
pub mod method_model;
pub mod property_model;
pub mod transform;

pub use transform::{load_dep, load_v2};

pub use codegen::api_client::{ClientModel, GenClientConfig, gen_client};
pub use codegen::api_server::{GenServerConfig, gen_server};

// for convenience in build.rs scripts
pub mod prelude {
    pub use crate::codegen::api_client::{ClientModel, GenClientConfig, gen_client};
    pub use crate::codegen::api_server::{GenServerConfig, gen_server};
    pub use crate::method_model::{MethodModel, MethodModelItem, MethodModelKind};
    pub use crate::property_model::{PropertyModel, PropertyModelItem, PropertyModelKind};
    pub use crate::transform::{load_dep, load_v2};
}
