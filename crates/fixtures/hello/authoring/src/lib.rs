use boxology_contract::CallContext;

#[boxology::contract(error)]
pub enum GreetError {
    EmptyName,
}

pub struct HelloService;

impl HelloService {
    #[boxology::capability(exposure = "external")]
    pub async fn greet(&self, context: CallContext, name: String) -> Result<String, GreetError> {
        let _ = context;
        if name.is_empty() {
            Err(GreetError::EmptyName)
        } else {
            Ok(format!("Hello, {name}!"))
        }
    }
}
