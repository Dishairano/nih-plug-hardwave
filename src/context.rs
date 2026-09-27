//! Different contexts the plugin can use to make callbacks to the host in different...contexts.

use std::fmt::Display;

pub mod gui;
pub mod init;
pub mod process;

// Contexts for more plugin-API specific features
pub mod remote_controls;

/// The currently active plugin API. This may be useful to display in an about screen in the
/// plugin's GUI for debugging purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginApi {
    Clap,
    Standalone,
    Vst3,
}

/// Information about the track (channel, mixer strip) the plugin instance has been inserted on, as
/// reported by the host. VST3 hosts send this through `Vst::ChannelContext::IInfoListener`, and
/// CLAP hosts through the `track-info` extension. Hosts that support neither never send anything,
/// in which case the track info is `None` wherever it is exposed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrackInfo {
    /// The track's name, if the host provided a non-empty one.
    pub name: Option<String>,
    /// The track's color packed as `0xAARRGGBB`, if the host provided one.
    pub color: Option<u32>,
}

impl Display for PluginApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginApi::Clap => write!(f, "CLAP"),
            PluginApi::Standalone => write!(f, "standalone"),
            PluginApi::Vst3 => write!(f, "VST3"),
        }
    }
}
