use std::sync::mpsc;
use std::thread;

/// Create a tx/rx pair with a background thread that handles CtrlReq variants
/// by sending back mock responses. This simulates the server's main loop.
///
/// Shared across backend test files to avoid duplication.
pub fn make_mock_server() -> mpsc::Sender<psmux::types::CtrlReq> {
    let (tx, rx) = mpsc::channel::<psmux::types::CtrlReq>();
    thread::spawn(move || {
        while let Ok(req) = rx.recv() {
            match req {
                psmux::types::CtrlReq::BackendInitialize { resp } => {
                    let _ = resp.send("%0".to_string());
                }
                psmux::types::CtrlReq::BackendSpawnAgent { resp, .. } => {
                    let _ = resp.send("%1".to_string());
                }
                psmux::types::CtrlReq::BackendCapturePane { resp, .. } => {
                    let _ = resp.send("line1\nline2\nline3\n\n\n".to_string());
                }
                psmux::types::CtrlReq::BackendListPanes { resp } => {
                    let _ = resp.send(r#"{"contexts":[]}"#.to_string());
                }
                psmux::types::CtrlReq::BackendKillPane { resp, .. } => {
                    let _ = resp.send(());
                }
                psmux::types::CtrlReq::BackendKillAll { resp, .. } => {
                    let _ = resp.send(vec!["%1".to_string(), "%2".to_string()]);
                }
                psmux::types::CtrlReq::BackendSetMetadata { resp, .. } => {
                    let _ = resp.send(true);
                }
                psmux::types::CtrlReq::BackendSendText { resp, .. } => {
                    if let Some(tx) = resp {
                        let _ = tx.send(true);
                    }
                }
                psmux::types::CtrlReq::QueryPaneReady(_, resp) => {
                    // Return a stable, ready-looking version: dv=1, lot=1 (epoch ms)
                    let _ = resp.send((1, 1));
                }
                _ => {}
            }
        }
    });
    tx
}
