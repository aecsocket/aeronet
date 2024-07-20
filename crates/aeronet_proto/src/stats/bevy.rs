use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use bevy_app::prelude::*;
use bevy_ecs::prelude::*;
use bevy_time::common_conditions::on_timer;
use web_time::Duration;

use crate::session::SessionBacked;

use super::SessionStats;

/// Provides the [`ClientSessionStats`] and automatically updates it by sampling
/// the client transport `T`, which must implement [`SessionBacked`].
///
/// If you are using Bevy along with the visualizer, you can use this plugin to
/// access the [`ClientSessionStats`] and pass it to the visualizer.
#[derive(Debug, Clone)]
pub struct ClientSessionStatsPlugin<T> {
    /// How many times per second we sample the session and insert new samples
    /// into the stats.
    pub sample_rate: u32,
    /// How many seconds of sample history we should keep.
    pub history: usize,
    _phantom: PhantomData<T>,
}

impl<T> ClientSessionStatsPlugin<T> {
    /// Creates a new plugin with the given parameters.
    #[must_use]
    pub const fn new(sample_rate: u32, history: usize) -> Self {
        Self {
            sample_rate,
            history,
            _phantom: PhantomData,
        }
    }
}

impl<T> Default for ClientSessionStatsPlugin<T> {
    fn default() -> Self {
        Self::new(30, 15)
    }
}

/// Stores [`SessionStats`] for the client transport `T`, to be used in
/// conjunction with [`ClientSessionStatsPlugin`].
#[derive(Resource)]
pub struct ClientSessionStats<T> {
    /// Session stats.
    pub stats: SessionStats,
    _phantom: PhantomData<T>,
}

impl<T> ClientSessionStats<T> {
    /// See [`SessionStats::new`].
    #[must_use]
    pub fn new(sample_rate: u32, history: usize) -> Self {
        Self {
            stats: SessionStats::new(sample_rate, history),
            _phantom: PhantomData,
        }
    }
}

impl<T: SessionBacked + Resource> Plugin for ClientSessionStatsPlugin<T> {
    fn build(&self, app: &mut App) {
        app.insert_resource(ClientSessionStats::<T>::new(self.sample_rate, self.history))
            .add_systems(
                Update,
                Self::update_stats.run_if(
                    resource_exists::<ClientSessionStats<T>>
                        .and_then(on_timer(Duration::from_secs(1) / self.sample_rate)),
                ),
            );
    }
}

impl<T> Deref for ClientSessionStats<T> {
    type Target = SessionStats;

    fn deref(&self) -> &Self::Target {
        &self.stats
    }
}

impl<T> DerefMut for ClientSessionStats<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.stats
    }
}

impl<T: SessionBacked + Resource> ClientSessionStatsPlugin<T> {
    fn update_stats(mut stats: ResMut<ClientSessionStats<T>>, client: Res<T>) {
        let Some(session) = client.get_session() else {
            stats.clear();
            return;
        };
        stats.update(session);
    }
}
