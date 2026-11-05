//! The server's run/stop boundary, shared by integrated and dedicated servers.

use super::Game;
use horizon_client::Result;
use std::panic::{AssertUnwindSafe, catch_unwind};

fn shutdown_stage(stage: impl FnOnce() -> Result<()>) -> Result<()> {
    match catch_unwind(AssertUnwindSafe(stage)) {
        Ok(result) => result,
        Err(payload) => Err(format!(
            "server shutdown panicked: {}",
            super::dedicated::panic_message(payload.as_ref())
        )
        .into()),
    }
}

impl Game<'_> {
    /// `MinecraftServer.runServer`'s finally: once startup begins, every exit
    /// stops and saves the server, including a failed init or tick.
    pub(super) fn run_server_lifecycle(
        &mut self,
        dedicated: bool,
        run: impl FnOnce(&mut Self, &mut bool) -> Result<()>,
    ) -> Result<()> {
        let mut startup_began = false;
        let run = catch_unwind(AssertUnwindSafe(|| run(self, &mut startup_began)));
        let crashed = run.is_err();
        let result = match run {
            Ok(result) => result,
            Err(payload) => {
                let message = super::dedicated::panic_message(payload.as_ref());
                log::error!("The server thread panicked: {message}");
                self.prepare_server_crash();
                Err(format!("the server thread panicked: {message}").into())
            }
        };
        if !startup_began {
            return result;
        }
        // Listeners still see the loaded worlds and players, even on failure.
        let stopping = shutdown_stage(|| {
            self.native_server_stopping(dedicated);
            Ok(())
        });
        if let Err(error) = &result {
            let _ = shutdown_stage(|| {
                self.native_lifecycle_world_failed(&error.to_string());
                self.native_lifecycle_close_failed(&error.to_string());
                Ok(())
            });
        }
        let mut save_reached = false;
        let stopped = shutdown_stage(|| {
            let result = if dedicated {
                self.stop_dedicated(crashed || stopping.is_err())
            } else {
                self.stop_integrated(crashed || stopping.is_err())
            };
            save_reached = true;
            result
        });
        // A callback in the early stop sequence may panic before final saving.
        let fallback = if save_reached {
            Ok(())
        } else {
            self.finish_server_save(true)
        };
        if let Err(error) = &stopped {
            let _ = shutdown_stage(|| {
                self.native_lifecycle_close_failed(&error.to_string());
                Ok(())
            });
            log::error!("Exception stopping the server: {error}");
        }
        let completed = shutdown_stage(|| {
            self.native_server_stopped(dedicated);
            Ok(())
        });
        result
            .and(stopping)
            .and(stopped)
            .and(fallback)
            .and(completed)
    }

    /// A panic can interrupt `as_guest` with the player's current state swapped
    /// into Game. Keep that player's last saved file rather than an older guest
    /// record. If the network tick lost the hub, only world state can be saved.
    fn prepare_server_crash(&mut self) {
        let acting = self.lan_acting;
        if let (Some(conn), Some(host)) = (acting, self.lan.as_mut())
            && let Some(guest) = host.hub.guests.iter_mut().find(|g| g.conn == conn)
        {
            guest.placed = false;
        }
        if self.lan.is_none() {
            log::warn!(
                "The panic interrupted the network tick: players are not saved, only the world"
            );
        } else if acting.is_some() {
            log::warn!(
                "The panic interrupted a player's turn: that player keeps the last saved file"
            );
        }
    }

    /// Complete teardown even if freeing beds or the final save fails. A failed
    /// open must never overwrite a world the server did not successfully adopt.
    pub(super) fn finish_server_save(&mut self, crashed: bool) -> Result<()> {
        let beds = shutdown_stage(|| {
            if crashed {
                Ok(())
            } else {
                self.free_player_beds()
            }
        });
        let saved = shutdown_stage(|| {
            if self.server_world_adopted {
                self.save_game()
            } else {
                Ok(())
            }
        });
        let closed = shutdown_stage(|| {
            self.lan_close(false);
            Ok(())
        });
        let flushed = shutdown_stage(|| self.save.flush());
        let result = beds.and(saved).and(closed).and(flushed);
        let notified = if result.is_ok() {
            shutdown_stage(|| {
                self.native_lifecycle_world_closed();
                Ok(())
            })
        } else {
            Ok(())
        };
        self.session_lock = None;
        self.server_world_adopted = false;
        result.and(notified)
    }
}

#[cfg(test)]
#[path = "server_lifecycle_tests.rs"]
mod tests;
