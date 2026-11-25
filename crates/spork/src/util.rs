/// Zed's `ResultExt`: an error that can't change what happens next is still reported.
pub(crate) trait ResultExt<T> {
    /// The value, or None after writing the error and where it happened to stderr.
    fn log_err(self) -> Option<T>;
}

impl<T, E: std::fmt::Display> ResultExt<T> for Result<T, E> {
    #[track_caller]
    fn log_err(self) -> Option<T> {
        match self {
            Ok(value) => Some(value),
            Err(error) => {
                // shortcut: stderr, as Spork has no logger yet; move to `log` with one.
                eprintln!("{}: {error}", std::panic::Location::caller());
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ResultExt;

    #[test]
    fn keeps_the_value_and_drops_the_error() {
        assert_eq!(Ok::<_, String>(3).log_err(), Some(3));
        assert_eq!(Err::<u8, _>("disk full").log_err(), None);
    }
}
