use super::*;
use std::sync::mpsc;
use vt100::ProgramState;

fn until_req(pane_id: Option<usize>) -> (CtrlReq, mpsc::Receiver<i32>) {
    let (resp, rx) = mpsc::channel();
    let req = CtrlReq::WaitPaneUntil {
        pane_id,
        want: ProgramState::Idle,
        deadline: None,
        resp,
    };
    (req, rx)
}

#[test]
fn enqueue_wait_until_when_no_pane_id_and_no_windows_then_gone() {
    let mut app = AppState::new("q".to_string());
    let (req, rx) = until_req(None);
    enqueue_wait_until(&mut app, req);
    assert_eq!(rx.try_recv(), Ok(crate::wait_until::UNTIL_GONE));
    assert!(app.wait_until_queue.is_empty());
}

#[test]
fn enqueue_wait_until_when_pane_id_unknown_then_gone() {
    let mut app = AppState::new("q".to_string());
    let (req, rx) = until_req(Some(7));
    enqueue_wait_until(&mut app, req);
    assert_eq!(rx.try_recv(), Ok(crate::wait_until::UNTIL_GONE));
    assert!(app.wait_until_queue.is_empty());
}

#[test]
fn drain_wait_until_queue_when_pane_pruned_then_gone_and_dequeued() {
    let mut app = AppState::new("q".to_string());
    let (resp, rx) = mpsc::channel();
    app.wait_until_queue.push(crate::wait_until::UntilWaiter {
        pane_id: 7,
        want: ProgramState::Working,
        deadline: None,
        resp,
    });
    drain_wait_until_queue(&mut app);
    assert_eq!(rx.try_recv(), Ok(crate::wait_until::UNTIL_GONE));
    assert!(app.wait_until_queue.is_empty());
}
