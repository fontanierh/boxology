boxology::contract! {
    #[error]
    pub enum HelloError {
        EmptyName,
    }

    #[capability(exposure = external)]
    pub async fn ping(nonce: u64) -> Result<u64, HelloError>;
}
