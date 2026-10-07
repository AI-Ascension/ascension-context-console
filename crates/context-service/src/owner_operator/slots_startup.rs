use super::config::OperatorConfig;
use super::protected_files::AdapterError;

pub(super) fn validate(config: &OperatorConfig) -> Result<(), AdapterError> {
    let (root, refs) = super::slots::startup_bearer_refs(config)?;
    for (reference, expected_digest) in refs {
        let bearer = root.read_file(&reference, 4 * 1024)?;
        if !super::slots::valid_bearer(&bearer)
            || super::slots::hex_digest(&bearer) != expected_digest
        {
            return Err(AdapterError::Invalid);
        }
    }
    Ok(())
}
