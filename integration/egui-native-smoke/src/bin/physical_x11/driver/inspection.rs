//! Deadline-bounded typed client for the private egui inspection endpoint.

use std::net::{SocketAddr, TcpStream};
use std::time::{Duration, Instant};

use eframe::egui::ViewportId;
use eframe::egui::accesskit::TreeUpdate;
use egui_inspection::protocol::read_handshake;
use egui_inspection::{
    PROTOCOL_VERSION, Request, Response, ViewportDescriptor, read_message, write_message,
};

#[derive(Debug)]
pub(super) struct InspectionClient {
    stream: TcpStream,
}

pub(super) struct ViewportTreeSnapshot {
    pub(super) viewport: ViewportId,
    pub(super) step: u64,
    pub(super) pixels_per_point: f32,
    pub(super) tree: TreeUpdate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ViewportTreeAvailability {
    Present { step: u64 },
    Unavailable(egui_inspection::ViewportErrorReason),
}

pub(super) fn viewport_retirement_is_observed(
    listed: bool,
    availability: ViewportTreeAvailability,
) -> bool {
    !listed
        && matches!(
            availability,
            ViewportTreeAvailability::Unavailable(
                egui_inspection::ViewportErrorReason::Unknown
                    | egui_inspection::ViewportErrorReason::Removed
            )
        )
}

impl InspectionClient {
    pub(super) fn connect(endpoint: SocketAddr, deadline: Instant) -> Result<Self, String> {
        let timeout = remaining_budget(deadline, "connect semantic inspection endpoint")?;
        let stream = TcpStream::connect_timeout(&endpoint, timeout)
            .map_err(|error| format!("connect semantic inspection endpoint {endpoint}: {error}"))?;
        let mut client = Self { stream };
        client.handshake(deadline)?;
        Ok(client)
    }

    fn handshake(&mut self, deadline: Instant) -> Result<(), String> {
        self.set_read_deadline(deadline, "read semantic inspection handshake")?;
        let version = read_handshake(&self.stream)
            .map_err(|error| format!("read semantic inspection handshake: {error}"))?;
        if version != PROTOCOL_VERSION {
            return Err(format!(
                "semantic inspection protocol mismatch: peer={version}, driver={PROTOCOL_VERSION}"
            ));
        }
        Ok(())
    }

    fn request(&mut self, request: &Request, deadline: Instant) -> Result<Response, String> {
        self.set_write_deadline(deadline, "write semantic inspection request")
            .map_err(|error| format!("{error}; request={request:?}"))?;
        write_message(&mut self.stream, request)
            .map_err(|error| format!("write semantic inspection request {request:?}: {error}"))?;
        self.set_read_deadline(deadline, "read semantic inspection response")
            .map_err(|error| format!("{error}; request={request:?}"))?;
        read_message(&mut self.stream)
            .map_err(|error| format!("read semantic inspection response for {request:?}: {error}"))
    }

    pub(super) fn viewports(
        &mut self,
        deadline: Instant,
    ) -> Result<Vec<ViewportDescriptor>, String> {
        match self.request(&Request::ListViewports, deadline)? {
            Response::Viewports { viewports } => Ok(viewports),
            response => Err(format!(
                "semantic inspection returned {response:?} for ListViewports"
            )),
        }
    }

    pub(super) fn tree(
        &mut self,
        viewport: ViewportId,
        deadline: Instant,
    ) -> Result<ViewportTreeSnapshot, String> {
        match self.request(&Request::GetViewportTree { viewport }, deadline)? {
            Response::ViewportTree {
                viewport: returned,
                step,
                pixels_per_point,
                accesskit: Some(tree),
            } if returned == viewport => Ok(ViewportTreeSnapshot {
                viewport,
                step,
                pixels_per_point,
                tree,
            }),
            Response::ViewportTree {
                viewport: returned,
                accesskit: None,
                ..
            } => Err(format!(
                "viewport {returned:?} completed without an AccessKit tree"
            )),
            Response::ViewportError { viewport, reason } => Err(format!(
                "semantic viewport {viewport:?} is unavailable: {reason:?}"
            )),
            response => Err(format!(
                "semantic inspection returned {response:?} for viewport {viewport:?}"
            )),
        }
    }

    pub(super) fn tree_availability(
        &mut self,
        viewport: ViewportId,
        deadline: Instant,
    ) -> Result<ViewportTreeAvailability, String> {
        match self.request(&Request::GetViewportTree { viewport }, deadline)? {
            Response::ViewportTree {
                viewport: returned,
                step,
                ..
            } if returned == viewport => Ok(ViewportTreeAvailability::Present { step }),
            Response::ViewportError {
                viewport: returned,
                reason,
            } if returned == viewport => Ok(ViewportTreeAvailability::Unavailable(reason)),
            response => Err(format!(
                "semantic inspection returned {response:?} for viewport availability {viewport:?}"
            )),
        }
    }

    fn set_read_deadline(&self, deadline: Instant, operation: &str) -> Result<(), String> {
        let timeout = remaining_budget(deadline, operation)?;
        self.stream
            .set_read_timeout(Some(timeout))
            .map_err(|error| format!("set inspection read timeout for {operation}: {error}"))
    }

    fn set_write_deadline(&self, deadline: Instant, operation: &str) -> Result<(), String> {
        let timeout = remaining_budget(deadline, operation)?;
        self.stream
            .set_write_timeout(Some(timeout))
            .map_err(|error| format!("set inspection write timeout for {operation}: {error}"))
    }
}

fn remaining_budget(deadline: Instant, operation: &str) -> Result<Duration, String> {
    let remaining = deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err(format!("{operation}: absolute deadline elapsed"));
    }
    Ok(remaining)
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;
    use std::net::TcpListener;
    use std::sync::mpsc;

    use egui_inspection::protocol::write_handshake;

    use super::*;

    #[test]
    fn retirement_requires_absent_inventory_and_typed_unavailability() {
        assert!(!viewport_retirement_is_observed(
            true,
            ViewportTreeAvailability::Unavailable(egui_inspection::ViewportErrorReason::Removed),
        ));
        assert!(!viewport_retirement_is_observed(
            false,
            ViewportTreeAvailability::Present { step: 17 },
        ));
        assert!(viewport_retirement_is_observed(
            false,
            ViewportTreeAvailability::Unavailable(egui_inspection::ViewportErrorReason::Removed),
        ));
        assert!(viewport_retirement_is_observed(
            false,
            ViewportTreeAvailability::Unavailable(egui_inspection::ViewportErrorReason::Unknown),
        ));
    }

    #[test]
    fn handshake_read_is_bounded_by_the_absolute_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener binds");
        let endpoint = listener.local_addr().expect("listener has a local address");
        let (release_sender, release_receiver) = mpsc::sync_channel(0);
        let server = std::thread::spawn(move || {
            let (_stream, _) = listener.accept().expect("deadline client connects");
            release_receiver
                .recv()
                .expect("test releases the silent handshake peer");
        });

        let started = Instant::now();
        let deadline = started + Duration::from_millis(80);
        let error = InspectionClient::connect(endpoint, deadline)
            .expect_err("a silent handshake peer must hit the absolute deadline");
        let elapsed = started.elapsed();
        release_sender
            .send(())
            .expect("silent handshake peer remains connected");
        server.join().expect("silent handshake peer exits");

        assert!(error.contains("handshake"), "unexpected error: {error}");
        assert!(
            elapsed < Duration::from_secs(1),
            "handshake exceeded its bounded deadline: {elapsed:?}"
        );
    }

    #[test]
    fn request_read_uses_the_remaining_absolute_budget() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback listener binds");
        let endpoint = listener.local_addr().expect("listener has a local address");
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("deadline client connects");
            write_handshake(&mut stream).expect("test peer writes the handshake");
            let _: Request = read_message(&mut stream).expect("test peer receives one request");
            let mut closed = [0_u8; 1];
            let _ = stream.read(&mut closed);
        });

        let mut client =
            InspectionClient::connect(endpoint, Instant::now() + Duration::from_secs(1))
                .expect("the test handshake completes");
        let started = Instant::now();
        let deadline = started + Duration::from_millis(80);
        let error = client
            .viewports(deadline)
            .expect_err("the missing response must hit the request deadline");
        let elapsed = started.elapsed();
        drop(client);
        server.join().expect("test inspection peer exits");

        assert!(error.contains("response"), "unexpected error: {error}");
        assert!(
            error.contains("ListViewports"),
            "missing request identity: {error}"
        );
        assert!(
            elapsed < Duration::from_secs(1),
            "request exceeded its bounded deadline: {elapsed:?}"
        );
    }
}
