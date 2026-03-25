const BASE64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=";
const CLIPBOARD_SELECTOR: &[u8] = b"cpqs01234567";

/// Maximum DCS buffer size (10 MB). Sequences exceeding this are truncated
/// and the buffer is cleared to prevent unbounded memory growth from
/// malformed or missing DCS terminators.
const MAX_DCS_BUF_SIZE: usize = 10 * 1024 * 1024;

pub struct WrappedScreen<CB: crate::callbacks::Callbacks = ()> {
    pub screen: crate::screen::Screen,
    pub callbacks: CB,
    dcs_buf: Vec<u8>,
    dcs_is_tmux: bool,
    dcs_action: char,
    dcs_overflow: bool,
}

impl WrappedScreen<()> {
    pub fn new(rows: u16, cols: u16, scrollback_len: usize) -> Self {
        Self::new_with_callbacks(rows, cols, scrollback_len, ())
    }
}

impl<CB: crate::callbacks::Callbacks> WrappedScreen<CB> {
    pub fn new_with_callbacks(rows: u16, cols: u16, scrollback_len: usize, callbacks: CB) -> Self {
        Self {
            screen: crate::screen::Screen::new(crate::grid::Size { rows, cols }, scrollback_len),
            callbacks,
            dcs_buf: Vec::new(),
            dcs_is_tmux: false,
            dcs_action: '\0',
            dcs_overflow: false,
        }
    }
}

impl<CB: crate::callbacks::Callbacks> vte::Perform for WrappedScreen<CB> {
    fn print(&mut self, c: char) {
        if c == '\u{fffd}' || ('\u{80}'..'\u{a0}').contains(&c) {
            self.callbacks.unhandled_char(&mut self.screen, c);
        } else {
            self.screen.text(c);
        }
    }

    fn execute(&mut self, b: u8) {
        match b {
            7 => self.callbacks.audible_bell(&mut self.screen),
            8 => self.screen.bs(),
            9 => self.screen.tab(),
            10 => self.screen.lf(),
            11 => self.screen.vt(),
            12 => self.screen.ff(),
            13 => self.screen.cr(),
            // we don't implement shift in/out alternate character sets, but
            // it shouldn't count as an "error"
            14 | 15 => {}
            _ => self.callbacks.unhandled_control(&mut self.screen, b),
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, b: u8) {
        if let Some(i) = intermediates.first() {
            self.callbacks.unhandled_escape(
                &mut self.screen,
                Some(*i),
                intermediates.get(1).copied(),
                b,
            );
        } else {
            match b {
                b'7' => self.screen.decsc(),
                b'8' => self.screen.decrc(),
                b'=' => self.screen.deckpam(),
                b'>' => self.screen.deckpnm(),
                b'M' => self.screen.ri(),
                b'c' => self.screen.ris(),
                b'g' => self.callbacks.visual_bell(&mut self.screen),
                _ => {
                    self.callbacks
                        .unhandled_escape(&mut self.screen, None, None, b);
                }
            }
        }
    }

    fn csi_dispatch(&mut self, params: &vte::Params, intermediates: &[u8], _ignore: bool, c: char) {
        let unhandled = |screen: &mut crate::screen::Screen| {
            self.callbacks.unhandled_csi(
                screen,
                intermediates.first().copied(),
                intermediates.get(1).copied(),
                &params.iter().collect::<Vec<_>>(),
                c,
            );
        };
        match intermediates.first() {
            None => match c {
                '@' => self.screen.ich(canonicalize_params_1(params, 1)),
                'A' => self.screen.cuu(canonicalize_params_1(params, 1)),
                'B' => self.screen.cud(canonicalize_params_1(params, 1)),
                'C' => self.screen.cuf(canonicalize_params_1(params, 1)),
                'D' => self.screen.cub(canonicalize_params_1(params, 1)),
                'E' => self.screen.cnl(canonicalize_params_1(params, 1)),
                'F' => self.screen.cpl(canonicalize_params_1(params, 1)),
                'G' => self.screen.cha(canonicalize_params_1(params, 1)),
                'H' | 'f' => self.screen.cup(canonicalize_params_2(params, 1, 1)),
                'J' => self.screen.ed(canonicalize_params_1(params, 0), unhandled),
                'K' => self.screen.el(canonicalize_params_1(params, 0), unhandled),
                'L' => self.screen.il(canonicalize_params_1(params, 1)),
                'M' => self.screen.dl(canonicalize_params_1(params, 1)),
                'P' => self.screen.dch(canonicalize_params_1(params, 1)),
                'S' => self.screen.su(canonicalize_params_1(params, 1)),
                'T' => self.screen.sd(canonicalize_params_1(params, 1)),
                'X' => self.screen.ech(canonicalize_params_1(params, 1)),
                'd' => self.screen.vpa(canonicalize_params_1(params, 1)),
                'm' => self.screen.sgr(params, unhandled),
                'n' => {
                    // DSR (Device Status Report) — in passthrough mode the
                    // child sends this and expects a response.  We ignore it
                    // at the parser level (the host must respond via the PTY
                    // writer if needed), but we must not call unhandled.
                }
                'r' => self.screen.decstbm(canonicalize_params_decstbm(
                    params,
                    self.screen.grid().size(),
                )),
                's' => self.screen.decsc(),
                'u' => self.screen.decrc(),
                't' => {
                    let mut params_iter = params.iter();
                    let op = params_iter.next().and_then(|x| x.first().copied());
                    if op == Some(8) {
                        let (screen_rows, screen_cols) = self.screen.size();
                        let rows = params_iter
                            .next()
                            .map_or(screen_rows, |x| *x.first().unwrap_or(&screen_rows));
                        let cols = params_iter
                            .next()
                            .map_or(screen_cols, |x| *x.first().unwrap_or(&screen_cols));
                        self.callbacks.resize(&mut self.screen, (rows, cols));
                    } else {
                        self.callbacks.unhandled_csi(
                            &mut self.screen,
                            None,
                            None,
                            &params.iter().collect::<Vec<_>>(),
                            c,
                        );
                    }
                }
                _ => {
                    self.callbacks.unhandled_csi(
                        &mut self.screen,
                        None,
                        None,
                        &params.iter().collect::<Vec<_>>(),
                        c,
                    );
                }
            },
            Some(b'?') => match c {
                'J' => self
                    .screen
                    .decsed(canonicalize_params_1(params, 0), unhandled),
                'K' => self
                    .screen
                    .decsel(canonicalize_params_1(params, 0), unhandled),
                'h' => self.screen.decset(params, unhandled),
                'l' => self.screen.decrst(params, unhandled),
                _ => {
                    self.callbacks.unhandled_csi(
                        &mut self.screen,
                        Some(b'?'),
                        intermediates.get(1).copied(),
                        &params.iter().collect::<Vec<_>>(),
                        c,
                    );
                }
            },
            Some(i) => {
                self.callbacks.unhandled_csi(
                    &mut self.screen,
                    Some(*i),
                    intermediates.get(1).copied(),
                    &params.iter().collect::<Vec<_>>(),
                    c,
                );
            }
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bel_terminated: bool) {
        match params {
            [b"0", s] => {
                self.callbacks.set_window_icon_name(&mut self.screen, s);
                self.callbacks.set_window_title(&mut self.screen, s);
                self.screen.set_title(s);
            }
            [b"1", s] => {
                self.callbacks.set_window_icon_name(&mut self.screen, s);
            }
            [b"2", s] => {
                self.callbacks.set_window_title(&mut self.screen, s);
                self.screen.set_title(s);
            }
            [b"7", uri] => {
                self.screen.set_path(uri);
            }
            [b"9999", ..] => {
                self.screen.squelch_cleared = true;
            }
            [b"52", ty, data] => match (ty.iter().all(|c| CLIPBOARD_SELECTOR.contains(c)), *data) {
                (true, b"?") => {
                    self.callbacks.paste_from_clipboard(&mut self.screen, ty);
                }
                (true, data) if data.iter().all(|c| BASE64.contains(c)) => {
                    self.callbacks.copy_to_clipboard(&mut self.screen, ty, data);
                }
                _ => {
                    self.callbacks.unhandled_osc(&mut self.screen, params);
                }
            },
            _ => {
                self.callbacks.unhandled_osc(&mut self.screen, params);
            }
        }
    }

    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, action: char) {
        self.dcs_buf.clear();
        self.dcs_is_tmux = false;
        self.dcs_overflow = false;
        self.dcs_action = action;
    }

    fn put(&mut self, byte: u8) {
        // Guard against unbounded growth from malformed DCS sequences
        // (missing terminator). Once overflow is detected, discard all
        // further bytes until unhook() resets the state.
        if self.dcs_overflow {
            return;
        }
        if self.dcs_buf.len() >= MAX_DCS_BUF_SIZE {
            eprintln!(
                "psmux: DCS buffer exceeded {}MB limit, truncating sequence",
                MAX_DCS_BUF_SIZE / (1024 * 1024)
            );
            self.dcs_buf.clear();
            self.dcs_overflow = true;
            return;
        }
        self.dcs_buf.push(byte);
        // vte consumes the first printable char as the action in hook(),
        // so for "\x1bPtmux;..." the action is 't' and put receives "mux;...".
        if self.dcs_action == 't' && self.dcs_buf.len() == 4 && &self.dcs_buf[..4] == b"mux;" {
            self.dcs_is_tmux = true;
            self.dcs_buf.clear();
        }
    }

    fn unhook(&mut self) {
        if self.dcs_is_tmux && !self.dcs_overflow {
            // vte terminates DCS on ESC, so the inner passthrough content
            // may be empty when escaped ESC sequences cause early DCS exit.
            // We still fire the callback so consumers know a tmux
            // passthrough was detected; the data may be empty if the inner
            // sequence started with ESC (common case).
            let inner = unescape_tmux_passthrough(&self.dcs_buf);
            self.callbacks.dcs_passthrough(&mut self.screen, &inner);
        }
        self.dcs_buf.clear();
        self.dcs_is_tmux = false;
        self.dcs_overflow = false;
    }
}

fn unescape_tmux_passthrough(data: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(data.len());
    let mut i = 0;
    while i < data.len() {
        if i + 1 < data.len() && data[i] == 0x1b && data[i + 1] == 0x1b {
            result.push(0x1b);
            i += 2;
        } else {
            result.push(data[i]);
            i += 1;
        }
    }
    result
}

fn canonicalize_params_1(params: &vte::Params, default: u16) -> u16 {
    let first = params.iter().next().map_or(0, |x| *x.first().unwrap_or(&0));
    if first == 0 {
        default
    } else {
        first
    }
}

fn canonicalize_params_2(params: &vte::Params, default1: u16, default2: u16) -> (u16, u16) {
    let mut iter = params.iter();
    let first = iter.next().map_or(0, |x| *x.first().unwrap_or(&0));
    let first = if first == 0 { default1 } else { first };

    let second = iter.next().map_or(0, |x| *x.first().unwrap_or(&0));
    let second = if second == 0 { default2 } else { second };

    (first, second)
}

fn canonicalize_params_decstbm(params: &vte::Params, size: crate::grid::Size) -> (u16, u16) {
    let mut iter = params.iter();
    let top = iter.next().map_or(0, |x| *x.first().unwrap_or(&0));
    let top = if top == 0 { 1 } else { top };

    let bottom = iter.next().map_or(0, |x| *x.first().unwrap_or(&0));
    let bottom = if bottom == 0 { size.rows } else { bottom };

    (top, bottom)
}

#[cfg(test)]
mod dcs_tests {
    use std::sync::{Arc, Mutex};

    struct DcsCapture {
        captured: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl crate::callbacks::Callbacks for DcsCapture {
        fn dcs_passthrough(&mut self, _: &mut crate::Screen, data: &[u8]) {
            self.captured.lock().unwrap().push(data.to_vec());
        }
    }

    #[test]
    fn test_dcs_tmux_passthrough_detected() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let callbacks = DcsCapture {
            captured: captured.clone(),
        };
        let mut parser = crate::Parser::new_with_callbacks(80, 24, 0, callbacks);

        // DCS tmux passthrough: ESC P tmux; ESC ESC ] 0 ; title BEL ESC backslash
        let input = b"\x1bPtmux;\x1b\x1b]0;My Title\x07\x1b\\";
        parser.process(input);

        let data = captured.lock().unwrap();
        assert!(
            !data.is_empty(),
            "DCS tmux passthrough should trigger callback"
        );
    }

    #[test]
    fn test_dcs_buffer_overflow_capped() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let callbacks = DcsCapture {
            captured: captured.clone(),
        };
        let mut parser = crate::Parser::new_with_callbacks(80, 24, 0, callbacks);

        // Start a DCS tmux passthrough
        let header = b"\x1bPtmux;";
        parser.process(header);

        // Feed more than MAX_DCS_BUF_SIZE bytes without a terminator.
        // Use a chunk size to avoid allocating a huge single buffer.
        let chunk = vec![b'A'; 64 * 1024]; // 64KB chunks
        let iterations = (super::MAX_DCS_BUF_SIZE / chunk.len()) + 2;
        for _ in 0..iterations {
            parser.process(&chunk);
        }

        // Terminate the DCS sequence
        parser.process(b"\x1b\\");

        // The overflow should have been detected — no callback fired
        let data = captured.lock().unwrap();
        assert!(
            data.is_empty(),
            "Overflowed DCS passthrough should not trigger callback"
        );
    }

    #[test]
    fn test_non_tmux_dcs_ignored() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let callbacks = DcsCapture {
            captured: captured.clone(),
        };
        let mut parser = crate::Parser::new_with_callbacks(80, 24, 0, callbacks);

        // Non-tmux DCS (e.g., sixel)
        let input = b"\x1bP0;1;0q\x1b\\";
        parser.process(input);

        let data = captured.lock().unwrap();
        assert!(
            data.is_empty(),
            "Non-tmux DCS should not trigger passthrough callback"
        );
    }
}
