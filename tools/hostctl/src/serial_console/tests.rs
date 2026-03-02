#[cfg(test)]
mod tests {
    use super::sdreq_regex;

    #[test]
    fn sdreq_regex_matches_exact_op_token() {
        let fat_stat = sdreq_regex(Some("fat_stat")).expect("regex compiles");
        assert!(fat_stat.is_match("SDREQ id=7 op=fat_stat"));
        assert!(fat_stat.is_match("SDREQ id=7 op=fat_stat path=/foo"));
        assert!(!fat_stat.is_match("SDREQ id=7 op=fat_stat_extra"));
    }
}
