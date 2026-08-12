boxology::contract! {
    #[error]
    pub enum GreetLoudlyError {
        Refused,
    }

    #[capability(exposure = external)]
    pub async fn greet_loudly(name: String) -> Result<String, GreetLoudlyError>;
}
