mod prod;

pub(crate) use prod::*;

#[cfg(all(test, not(target_os = "none")))]
mod tests;
