pub(crate) const PIPELINE_REPLAY_X: u16 = 100;
pub(crate) const PIPELINE_REPLAY_Y: u16 = 240;

const ELAN_TOUCH_REPORT_HEADER: u8 = 0x5A;
const ELAN_PRIMARY_SLOT_ACTIVE: u8 = 0x01;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PipelineReplayFrame {
    pub(crate) offset_ms: u64,
    pub(crate) touch_count: u8,
    pub(crate) x: u16,
    pub(crate) y: u16,
    pub(crate) raw: [u8; 8],
}

const fn active_frame(offset_ms: u64) -> PipelineReplayFrame {
    PipelineReplayFrame {
        offset_ms,
        touch_count: 1,
        x: PIPELINE_REPLAY_X,
        y: PIPELINE_REPLAY_Y,
        // The coordinate bytes encode approximately (100, 240) after the
        // TEMPERA rotation-0 transform at the controller's 1152x1152
        // resolution. The pipeline consumes the decoded point while retaining
        // the raw report header and active-slot mask for presence/multitouch.
        raw: [
            ELAN_TOUCH_REPORT_HEADER,
            0x20,
            0xB3,
            0xC0,
            0x00,
            0x00,
            0x00,
            ELAN_PRIMARY_SLOT_ACTIVE,
        ],
    }
}

const fn released_frame(offset_ms: u64) -> PipelineReplayFrame {
    PipelineReplayFrame {
        offset_ms,
        touch_count: 0,
        x: 0,
        y: 0,
        raw: [ELAN_TOUCH_REPORT_HEADER, 0, 0, 0, 0, 0, 0, 0],
    }
}

pub(crate) const PIPELINE_REPLAY_TAP: [PipelineReplayFrame; 6] = [
    active_frame(0),
    active_frame(20),
    active_frame(35),
    active_frame(90),
    released_frame(110),
    released_frame(150),
];
