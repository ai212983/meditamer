#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LegacyInitConfigContract {
    pub(crate) static_rx_buf_num: u8,
    pub(crate) dynamic_rx_buf_num: u8,
    pub(crate) static_tx_buf_num: u8,
    pub(crate) dynamic_tx_buf_num: u8,
    pub(crate) rx_mgmt_buf_num: u8,
    pub(crate) rx_ba_win: u8,
    pub(crate) nvs_enable: bool,
    pub(crate) nano_enable: bool,
    pub(crate) wifi_task_core_id: u32,
    pub(crate) feature_caps: u64,
    pub(crate) magic: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LegacyWifiTaskContract {
    pub(crate) name: &'static str,
    pub(crate) stack_size: usize,
    pub(crate) requested_priority: u32,
    pub(crate) core: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct LegacyPortScope {
    pub(crate) implements_init_start_scan_stop_first: bool,
    pub(crate) defers_connect_path: bool,
    pub(crate) defers_full_embassy_net_device_port: bool,
}

pub(crate) const LEGACY_INIT_CONFIG_CONTRACT: LegacyInitConfigContract = LegacyInitConfigContract {
    static_rx_buf_num: 10,
    dynamic_rx_buf_num: 32,
    static_tx_buf_num: 0,
    dynamic_tx_buf_num: 32,
    rx_mgmt_buf_num: 5,
    rx_ba_win: 6,
    nvs_enable: false,
    nano_enable: false,
    wifi_task_core_id: 0,
    feature_caps: 0x81,
    magic: 0x1f2f3f4f,
};

pub(crate) const LEGACY_WIFI_TASK_CONTRACT: LegacyWifiTaskContract = LegacyWifiTaskContract {
    name: "wifi",
    stack_size: 6656,
    requested_priority: 253,
    core: 0,
};

pub(crate) const LEGACY_PORT_SCOPE: LegacyPortScope = LegacyPortScope {
    implements_init_start_scan_stop_first: true,
    defers_connect_path: true,
    defers_full_embassy_net_device_port: true,
};
