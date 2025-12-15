#[derive(Debug, Default)]
pub(super) struct DragSession {
    next_id: u64,
    active: Option<ActiveSession>,
    observed_active_this_frame: bool,
}

#[derive(Debug)]
struct ActiveSession {
    id: u64,
    started_frame: u64,
    release_action_frame: Option<u64>,
    last_source: Option<&'static str>,
}

impl DragSession {
    pub(super) fn begin_frame(&mut self) {
        self.observed_active_this_frame = false;
    }

    pub(super) fn observe_active(&mut self, frame: u64, source: &'static str) -> Option<String> {
        self.observed_active_this_frame = true;

        match &mut self.active {
            Some(active) => {
                active.last_source = Some(source);
                None
            }
            None => {
                let id = self.next_id.max(1);
                self.next_id = id.saturating_add(1);
                self.active = Some(ActiveSession {
                    id,
                    started_frame: frame,
                    release_action_frame: None,
                    last_source: Some(source),
                });
                Some(format!("session START id={id} source={source}"))
            }
        }
    }

    pub(super) fn take_release_action(
        &mut self,
        frame: u64,
        kind: &'static str,
    ) -> (bool, Option<String>) {
        let Some(active) = &mut self.active else {
            // Still allow release actions if a drag source didn't get observed this frame
            // (e.g. a payload-only drag that is cleared early).
            return (
                true,
                Some(format!("session RELEASE kind={kind} (no active session)")),
            );
        };

        if active.release_action_frame == Some(frame) {
            return (
                false,
                Some(format!(
                    "session RELEASE ignored id={} kind={kind}",
                    active.id
                )),
            );
        }

        active.release_action_frame = Some(frame);
        (
            true,
            Some(format!(
                "session RELEASE id={} kind={kind} source={}",
                active.id,
                active.last_source.unwrap_or("unknown")
            )),
        )
    }

    pub(super) fn end_frame(&mut self, frame: u64) -> Option<String> {
        if self.active.is_some() && !self.observed_active_this_frame {
            let ended = self.active.take().unwrap();
            return Some(format!(
                "session END id={} started_frame={} end_frame={}",
                ended.id, ended.started_frame, frame
            ));
        }
        None
    }
}
