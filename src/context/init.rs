//! A context passed during plugin initialization.

use std::sync::Arc;

use super::{PluginApi, TrackInfo};
use crate::prelude::{GuiContext, Plugin};

/// Callbacks the plugin can make while it is being initialized. This is passed to the plugin during
/// [`Plugin::initialize()`][crate::plugin::Plugin::initialize()].
//
// # Safety
//
// The implementing wrapper needs to be able to handle concurrent requests, and it should perform
// the actual callback within [MainThreadQueue::schedule_gui].
pub trait InitContext<P: Plugin> {
    /// Get the current plugin API.
    fn plugin_api(&self) -> PluginApi;

    /// Run a task directly on this thread. This ensures that the task has finished executing before
    /// the plugin finishes initializing.
    ///
    /// # Note
    ///
    /// There is no asynchronous alternative for this function as that may result in incorrect
    /// behavior when doing offline rendering.
    fn execute(&self, task: P::BackgroundTask);

    /// Update the current latency of the plugin. If the plugin is currently processing audio, then
    /// this may cause audio playback to be restarted.
    fn set_latency_samples(&self, samples: u32);

    /// Set the current voice **capacity** for this plugin (so not the number of currently active
    /// voices). This may only be called if
    /// [`ClapPlugin::CLAP_POLY_MODULATION_CONFIG`][crate::prelude::ClapPlugin::CLAP_POLY_MODULATION_CONFIG]
    /// is set. `capacity` must be between 1 and the configured maximum capacity. Changing this at
    /// runtime allows the host to better optimize polyphonic modulation, or to switch to strictly
    /// monophonic modulation when dropping the capacity down to 1.
    fn set_current_voice_capacity(&self, capacity: u32);

    /// The most recent information the host sent about the track this instance sits on, such as
    /// the track's name and color. Returns `None` if the host has not sent any track information,
    /// or if the plugin API or host does not support it. Implement
    /// [`Plugin::track_info_changed()`][crate::prelude::Plugin::track_info_changed()] to be
    /// notified when this changes.
    fn track_info(&self) -> Option<TrackInfo> {
        None
    }

    /// Get a [`GuiContext`] for this plugin instance that works without an open editor. The
    /// plugin can store the returned context and later use it (for instance through a
    /// [`ParamSetter`][crate::prelude::ParamSetter]) to inform the host about parameter changes,
    /// exactly like the context passed to [`Editor::spawn()`][crate::prelude::Editor::spawn()].
    /// This makes it possible to change this instance's parameters when its editor is closed,
    /// for example from another instance's editor.
    ///
    /// The context stays valid for the life of the plugin instance and does not keep the instance
    /// alive. Once the host has destroyed the instance, calls on the context do nothing. Returns
    /// `None` only for `InitContext` implementations that cannot provide such a context.
    ///
    /// # Note
    ///
    /// The context's methods must never be called from the audio thread, and so they must never
    /// be called from [`Plugin::process()`][crate::prelude::Plugin::process()]. Call them from the
    /// main or GUI thread, the same as the context an editor receives. The parameter pointers
    /// passed to the context must belong to this instance, so keep the instance's `Arc<Params>`
    /// together with the context.
    fn instance_gui_context(&self) -> Option<Arc<dyn GuiContext>> {
        None
    }
}
