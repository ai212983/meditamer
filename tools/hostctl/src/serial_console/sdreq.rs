fn sdreq_regex(op: Option<&str>) -> Result<Regex> {
    match op {
        Some(op) => Regex::new(&format!(r"^SDREQ id=([0-9]+) op={}\b", regex::escape(op)))
            .map_err(Into::into),
        None => Regex::new(r"^SDREQ id=([0-9]+) op=").map_err(Into::into),
    }
}
