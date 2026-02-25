include!("normalize/prod.rs");

#[cfg(test)]
mod tests {
    use super::*;

    include!("normalize/tests_part_1.rs");
    include!("normalize/tests_part_2.rs");
}
