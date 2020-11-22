pub type Error = rradio_messages::Error<crate::atomic_string::AtomicString>;
pub type Result<T> = std::result::Result<T, Error>;
