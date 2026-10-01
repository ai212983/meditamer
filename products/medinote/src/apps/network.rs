//! Product-owned metadata and display vocabulary for the network app.

pub mod descriptor;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NetworkStatus {
    Off,
    Connecting,
    Ready([u8; 4]),
    Failed,
}

impl NetworkStatus {
    pub const fn text(self) -> &'static [u8] {
        match self {
            Self::Off => b"Wi-Fi off\0",
            Self::Connecting => b"Connecting\0",
            Self::Ready(_) => b"Wi-Fi ready\0",
            Self::Failed => b"Wi-Fi failed\0",
        }
    }
}

pub fn format_status<'a>(buffer: &'a mut [u8; 24], status: NetworkStatus) -> &'a core::ffi::CStr {
    let text = status.text();
    if !matches!(status, NetworkStatus::Ready(_)) {
        buffer[..text.len()].copy_from_slice(text);
        return core::ffi::CStr::from_bytes_with_nul(&buffer[..text.len()]).unwrap();
    }
    let NetworkStatus::Ready(ip) = status else {
        unreachable!()
    };
    buffer[..6].copy_from_slice(b"Ready ");
    let mut pos = 6;
    for (index, octet) in ip.into_iter().enumerate() {
        if index != 0 {
            buffer[pos] = b'.';
            pos += 1;
        }
        let hundreds = octet / 100;
        let tens = (octet / 10) % 10;
        if hundreds != 0 {
            buffer[pos] = b'0' + hundreds;
            pos += 1;
        }
        if hundreds != 0 || tens != 0 {
            buffer[pos] = b'0' + tens;
            pos += 1;
        }
        buffer[pos] = b'0' + octet % 10;
        pos += 1;
    }
    buffer[pos] = 0;
    core::ffi::CStr::from_bytes_with_nul(&buffer[..=pos]).unwrap()
}
