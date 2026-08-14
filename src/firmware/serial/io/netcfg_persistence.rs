#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PersistenceResponseDisposition {
    Stale,
    Succeeded,
    Failed,
}

pub(crate) const fn classify_persistence_response(
    expected_request_id: u32,
    actual_request_id: u32,
    ok: bool,
    code_is_ok: bool,
) -> PersistenceResponseDisposition {
    if actual_request_id != expected_request_id {
        PersistenceResponseDisposition::Stale
    } else if ok && code_is_ok {
        PersistenceResponseDisposition::Succeeded
    } else {
        PersistenceResponseDisposition::Failed
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_persistence_response, PersistenceResponseDisposition};

    #[test]
    fn stale_response_cannot_complete_current_command() {
        assert_eq!(
            classify_persistence_response(42, 41, true, true),
            PersistenceResponseDisposition::Stale
        );
    }

    #[test]
    fn matching_success_requires_ok_flag_and_ok_code() {
        assert_eq!(
            classify_persistence_response(42, 42, true, true),
            PersistenceResponseDisposition::Succeeded
        );
        assert_eq!(
            classify_persistence_response(42, 42, false, true),
            PersistenceResponseDisposition::Failed
        );
        assert_eq!(
            classify_persistence_response(42, 42, true, false),
            PersistenceResponseDisposition::Failed
        );
    }

    #[test]
    fn matching_negative_response_fails_current_command() {
        assert_eq!(
            classify_persistence_response(42, 42, false, false),
            PersistenceResponseDisposition::Failed
        );
    }
}
