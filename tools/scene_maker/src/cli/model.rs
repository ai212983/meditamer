use std::path::PathBuf;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Compression {
    None,
    Rle,
}

impl Compression {
    pub(crate) fn as_u8(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Rle => 1,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Rle => "rle",
        }
    }

    pub(crate) fn from_str(raw: &str) -> Result<Self, String> {
        match raw {
            "none" => Ok(Self::None),
            "rle" => Ok(Self::Rle),
            _ => Err(format!("invalid compression '{raw}', expected none|rle")),
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub(crate) enum ChannelId {
    Albedo = 1,
    Light = 2,
    Ao = 3,
    Depth = 4,
    Edge = 5,
    Mask = 6,
    Stroke = 7,
    NormalX = 8,
    NormalY = 9,
}

#[derive(Clone, Copy)]
pub(crate) struct ChannelTemplate {
    pub(crate) id: ChannelId,
    pub(crate) name: &'static str,
    pub(crate) required: bool,
    pub(crate) default_value: u8,
}

pub(crate) const CHANNELS: [ChannelTemplate; 9] = [
    ChannelTemplate {
        id: ChannelId::Albedo,
        name: "albedo",
        required: true,
        default_value: 255,
    },
    ChannelTemplate {
        id: ChannelId::Light,
        name: "light",
        required: true,
        default_value: 255,
    },
    ChannelTemplate {
        id: ChannelId::Ao,
        name: "ao",
        required: false,
        default_value: 255,
    },
    ChannelTemplate {
        id: ChannelId::Depth,
        name: "depth",
        required: false,
        default_value: 0,
    },
    ChannelTemplate {
        id: ChannelId::Edge,
        name: "edge",
        required: false,
        default_value: 0,
    },
    ChannelTemplate {
        id: ChannelId::Mask,
        name: "mask",
        required: false,
        default_value: 255,
    },
    ChannelTemplate {
        id: ChannelId::Stroke,
        name: "stroke",
        required: false,
        default_value: 128,
    },
    ChannelTemplate {
        id: ChannelId::NormalX,
        name: "normal_x",
        required: false,
        default_value: 128,
    },
    ChannelTemplate {
        id: ChannelId::NormalY,
        name: "normal_y",
        required: false,
        default_value: 128,
    },
];

#[derive(Clone)]
pub(crate) struct BuildConfig {
    pub(crate) input_dir: PathBuf,
    pub(crate) out_bundle: PathBuf,
    pub(crate) metadata_out: PathBuf,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) strip_height: u16,
    pub(crate) compression: Compression,
    pub(crate) derive_edge: bool,
    pub(crate) albedo: Option<PathBuf>,
    pub(crate) light: Option<PathBuf>,
    pub(crate) ao: Option<PathBuf>,
    pub(crate) depth: Option<PathBuf>,
    pub(crate) edge: Option<PathBuf>,
    pub(crate) mask: Option<PathBuf>,
    pub(crate) stroke: Option<PathBuf>,
    pub(crate) normal_x: Option<PathBuf>,
    pub(crate) normal_y: Option<PathBuf>,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            input_dir: PathBuf::from("tools/scene_maker/input"),
            out_bundle: PathBuf::from("tools/scene_maker/out/scene.scenebundle"),
            metadata_out: PathBuf::from("tools/scene_maker/out/scene.scenebundle.json"),
            width: 600,
            height: 600,
            strip_height: 32,
            compression: Compression::Rle,
            derive_edge: true,
            albedo: None,
            light: None,
            ao: None,
            depth: None,
            edge: None,
            mask: None,
            stroke: None,
            normal_x: None,
            normal_y: None,
        }
    }
}

pub(crate) struct ExplicitChannelPaths {
    pub(crate) albedo: Option<PathBuf>,
    pub(crate) light: Option<PathBuf>,
    pub(crate) ao: Option<PathBuf>,
    pub(crate) depth: Option<PathBuf>,
    pub(crate) edge: Option<PathBuf>,
    pub(crate) mask: Option<PathBuf>,
    pub(crate) stroke: Option<PathBuf>,
    pub(crate) normal_x: Option<PathBuf>,
    pub(crate) normal_y: Option<PathBuf>,
}

impl ExplicitChannelPaths {
    pub(crate) fn lookup(&self, name: &str) -> Option<PathBuf> {
        match name {
            "albedo" => self.albedo.clone(),
            "light" => self.light.clone(),
            "ao" => self.ao.clone(),
            "depth" => self.depth.clone(),
            "edge" => self.edge.clone(),
            "mask" => self.mask.clone(),
            "stroke" => self.stroke.clone(),
            "normal_x" => self.normal_x.clone(),
            "normal_y" => self.normal_y.clone(),
            _ => None,
        }
    }
}
