use sdcard::fat::FatStageLabel;

pub(super) fn stage_before_tag(stage: FatStageLabel) -> &'static str {
    match stage {
        FatStageLabel::MountMbr | FatStageLabel::MountBoot => "sd_fat_mount_io_before",
        FatStageLabel::ResolvePath | FatStageLabel::ScanDirectory | FatStageLabel::ReadFat => {
            "sd_fat_metadata_io_before"
        }
        FatStageLabel::ReadFile | FatStageLabel::ListDirectory => "sd_fat_read_io_before",
        _ => "sd_fat_write_io_before",
    }
}

pub(super) fn stage_after_tag(stage: FatStageLabel) -> &'static str {
    match stage {
        FatStageLabel::MountMbr | FatStageLabel::MountBoot => "sd_fat_mount_io_after",
        FatStageLabel::ResolvePath | FatStageLabel::ScanDirectory | FatStageLabel::ReadFat => {
            "sd_fat_metadata_io_after"
        }
        FatStageLabel::ReadFile | FatStageLabel::ListDirectory => "sd_fat_read_io_after",
        _ => "sd_fat_write_io_after",
    }
}
