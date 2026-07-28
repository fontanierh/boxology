boxology::contract! {
    #[error]
    pub enum GreetLoudlyError {
        Refused,
    }

    #[capability(exposure = external)]
    pub async fn greet_loudly(name: String) -> Result<String, GreetLoudlyError>;
}

pub struct GreeterService {
    hello: generated::HelloImport,
}

impl GreeterService {
    pub fn new(hello: generated::HelloImport) -> Self {
        Self { hello }
    }
}

#[boxology::implementation]
impl GreeterService {
    pub async fn greet_loudly(
        &self,
        context: boxology::CallContext,
        name: String,
    ) -> Result<String, GreetLoudlyError> {
        let greeting = self
            .hello
            .greet(context.child(), name)
            .await
            .map_err(|_| GreetLoudlyError::Refused)?;
        Ok(greeting.to_uppercase())
    }
}

pub mod generated {
    include!("../../generated/adapter/adapter.rs");
}

#[cfg(test)]
mod tests {
    use boxology_contract::BoxId;
    use boxology_runtime::{AssemblyError, CompositionBuilder};

    use super::{GreeterService, generated};

    #[test]
    fn generated_adapter_and_dispatch_are_send_sync() {
        fn assert_receiver<T: Send + Sync + 'static>() {}
        fn assert_dispatch<
            T: boxology_generated_contract::GreeterDispatch + Send + Sync + 'static,
        >() {
        }
        fn assert_bounds<T: Send + Sync + 'static>() {}

        assert_receiver::<GreeterService>();
        assert_dispatch::<GreeterService>();
        assert_bounds::<generated::GreeterAdapter<GreeterService>>();
        assert!(std::ptr::eq(
            generated::implementation_descriptor().contract(),
            boxology_generated_contract::contract_descriptor()
        ));
    }

    #[test]
    fn descriptor_has_exactly_one_hello_import() {
        let descriptor = generated::implementation_descriptor();
        let imports = descriptor.imports();
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].slot_id(), &BoxId::new("hello").unwrap());
        assert_eq!(imports[0].capabilities().len(), 1);
        assert_eq!(imports[0].capabilities()[0].to_string(), "hello.greet");
    }

    #[test]
    fn unresolved_import_reports_the_exact_diagnostic() {
        let descriptor = generated::implementation_descriptor();
        let mut builder = CompositionBuilder::new();
        builder.add_box(descriptor, |imports| {
            let deps = generated::typed_imports(&imports);
            generated::factory(GreeterService { hello: deps.hello }, imports)
        });

        let error = builder.validate().unwrap_err();
        assert_eq!(
            error.errors(),
            &[AssemblyError::MissingImportResolution {
                consumer: BoxId::new("greeter").unwrap(),
                slot: BoxId::new("hello").unwrap(),
            }]
        );
    }
}
