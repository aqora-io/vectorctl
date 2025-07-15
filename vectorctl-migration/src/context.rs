use fnv::FnvHashMap;
use std::{
    any::{Any, TypeId},
    fmt::{self, Debug, Formatter},
    ops::Deref,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ContextError {
    #[error("Resource: {0}")]
    Resource(String),
}

#[derive(Default)]
pub struct Resource(FnvHashMap<TypeId, Box<dyn Any + Sync + Send>>);

impl Deref for Resource {
    type Target = FnvHashMap<TypeId, Box<dyn Any + Sync + Send>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Resource {
    pub fn insert<R: Any + Send + Sync>(&mut self, resource: R) {
        self.0.insert(TypeId::of::<R>(), Box::new(resource));
    }

    pub fn insert_many<R: Any + Send + Sync>(&mut self, resources: Vec<R>) {
        let _ = resources.into_iter().map(|resource| self.insert(resource));
    }
}

impl Debug for Resource {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_tuple("Resource").finish()
    }
}

#[cfg(feature = "qdrant-backend")]
pub type Backend = vectorctl_backend::Qdrant;

#[cfg(not(any(feature = "qdrant-backend")))]
compile_error!(
    "You must enable exactly one vectordb backend feature: \
     either `qdrant-backend`"
);

pub struct Context {
    pub resources: Resource,
    pub backend: Backend,
}

impl Context {
    pub fn new(backend: Backend) -> Self {
        Self {
            backend,
            resources: Resource::default(),
        }
    }

    pub fn resource<R: Any + Send + Sync>(&self) -> Result<&R, ContextError> {
        self.resource_opt::<R>().ok_or_else(|| {
            ContextError::Resource(format!(
                "Resource `{}` does not exist.",
                std::any::type_name::<R>()
            ))
        })
    }

    pub fn resource_unchecked<R: Any + Send + Sync>(&self) -> &R {
        self.resource_opt::<R>()
            .unwrap_or_else(|| panic!("Resource `{}` does not exist.", std::any::type_name::<R>()))
    }

    pub fn resource_opt<R: Any + Send + Sync>(&self) -> Option<&R> {
        self.resources
            .0
            .get(&TypeId::of::<R>())
            .and_then(|d| d.downcast_ref::<R>())
    }

    pub fn insert_resource<R: Any + Send + Sync>(&mut self, resource: R) {
        self.resources.insert(resource)
    }

    pub fn insert_resources<R: Any + Send + Sync>(&mut self, resources: Vec<R>) {
        self.resources.insert_many(resources)
    }
}
