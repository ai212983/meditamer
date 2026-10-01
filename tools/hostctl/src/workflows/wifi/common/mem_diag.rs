#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemDiagKind {
    Radio,
    Upload,
}

#[derive(Clone, Debug)]
pub struct MemDiagSample {
    pub kind: MemDiagKind,
    pub stage: String,
    pub free: u64,
    pub internal_free: u64,
    pub external_free: u64,
    pub min_internal_free: u64,
}

#[derive(Clone, Debug, Default)]
pub struct MemDiagSummary {
    pub samples: u32,
    pub radio_samples: u32,
    pub upload_samples: u32,
    pub nomem_stage_samples: u32,
    pub min_free: Option<(u64, String)>,
    pub min_internal_free: Option<(u64, String)>,
    pub min_external_free: Option<(u64, String)>,
    pub min_internal_low_water: Option<(u64, String)>,
}

impl MemDiagSummary {
    pub fn record_line(&mut self, line: &str) {
        let Some(sample) = parse_mem_diag_line(line) else {
            return;
        };
        self.samples = self.samples.saturating_add(1);
        match sample.kind {
            MemDiagKind::Radio => self.radio_samples = self.radio_samples.saturating_add(1),
            MemDiagKind::Upload => self.upload_samples = self.upload_samples.saturating_add(1),
        }
        if sample.stage.contains("nomem") {
            self.nomem_stage_samples = self.nomem_stage_samples.saturating_add(1);
        }
        let label = match sample.kind {
            MemDiagKind::Radio => format!("radio:{}", sample.stage),
            MemDiagKind::Upload => format!("upload:{}", sample.stage),
        };
        update_min_sample(&mut self.min_free, sample.free, &label);
        update_min_sample(&mut self.min_internal_free, sample.internal_free, &label);
        update_min_sample(&mut self.min_external_free, sample.external_free, &label);
        update_min_sample(
            &mut self.min_internal_low_water,
            sample.min_internal_free,
            &label,
        );
    }
}

fn update_min_sample(slot: &mut Option<(u64, String)>, value: u64, label: &str) {
    match slot {
        Some((current, _)) if value >= *current => {}
        _ => *slot = Some((value, label.to_string())),
    }
}

fn token_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace()
        .find_map(|token| token.strip_prefix(&format!("{key}=")))
}

fn token_u64(line: &str, key: &str) -> Option<u64> {
    token_value(line, key)?.parse::<u64>().ok()
}

pub fn parse_mem_diag_line(line: &str) -> Option<MemDiagSample> {
    let kind = if line.starts_with("upload_http: radio_mem ") {
        MemDiagKind::Radio
    } else if line.starts_with("upload_http: upload_mem ") {
        MemDiagKind::Upload
    } else {
        return None;
    };
    Some(MemDiagSample {
        kind,
        stage: token_value(line, "stage")?.to_string(),
        free: token_u64(line, "free")?,
        internal_free: token_u64(line, "internal_free")?,
        external_free: token_u64(line, "external_free")?,
        min_internal_free: token_u64(line, "min_internal_free")?,
    })
}

pub fn fmt_min(value: &Option<(u64, String)>) -> String {
    match value {
        Some((bytes, stage)) => format!("{bytes}@{stage}"),
        None => "n/a".to_string(),
    }
}
