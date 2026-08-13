boxology::contract! {
    contract_crate = hello_contract;
    #[error]
    pub enum GreetError {
        EmptyName,
    }

    #[capability(exposure = external)]
    pub async fn greet(name: String) -> Result<String, GreetError>;
}
